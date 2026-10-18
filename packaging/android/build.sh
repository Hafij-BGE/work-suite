#!/usr/bin/env bash
# Build the WordCraft APK (apps/wordcraft-android in a NativeActivity).
#
#   packaging/android/build.sh [debug|release]
#
# Needs: the Android NDK (ANDROID_NDK_HOME) and SDK (ANDROID_HOME, with platform 34),
# `cargo install cargo-ndk`, the Rust targets (`rustup target add aarch64-linux-android`), a JDK 17+
# and Gradle 8.9 or later (`gradle`, or GRADLE=/path/to/gradle).
#
# WORDCRAFT_ANDROID_ABIS picks the CPUs (default arm64-v8a, every current phone; add x86_64 for
# the emulator). A release build is signed with WORDCRAFT_KEYSTORE (and WORDCRAFT_KEYSTORE_PASSWORD,
# WORDCRAFT_KEY_ALIAS, WORDCRAFT_KEY_PASSWORD) when set, else with the debug key.
# Output: $DIST/WordCraft-<version>-android-<mode>.apk
set -euo pipefail
. "$(dirname "$0")/../env.sh"

MODE="${1:-debug}"
case "$MODE" in
  debug) TASK=assembleDebug ;;
  release) TASK=assembleRelease ;;
  *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the Android NDK}"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"

HERE="$ROOT/packaging/android"
OUT="$HERE/app/build/rust"
# From scratch: Gradle's incremental packaging keeps the space a replaced library took in the APK.
rm -rf "$HERE/app/build"
mkdir -p "$OUT/jniLibs" "$OUT/res"

# The Rust library, one per CPU (always optimized: a debug build is too slow to type in).
targets=()
for abi in ${WORDCRAFT_ANDROID_ABIS:-arm64-v8a}; do
  targets+=(-t "$abi")
done
# Without symbols (a fifth of the library). To read a crash backtrace, rebuild without this.
(cd "$ROOT" && CARGO_PROFILE_RELEASE_STRIP=symbols cargo ndk "${targets[@]}" -P 26 -o "$OUT/jniLibs" build -p wordcraft-android --release)

# Launcher icons from the app icon (Android scales the nearest density).
for pair in mdpi:48 xhdpi:128 xxxhdpi:256; do
  density="${pair%%:*}"
  size="${pair##*:}"
  mkdir -p "$OUT/res/mipmap-$density"
  cp "$ROOT/assets/app-icon/hicolor/${size}x${size}/apps/ai.storyteller.wordcraft.png" "$OUT/res/mipmap-$density/ic_launcher.png"
done

echo "sdk.dir=$ANDROID_HOME" > "$HERE/local.properties"
(cd "$HERE" && "${GRADLE:-gradle}" --no-daemon -q "$TASK" -PwordcraftVersion="$VERSION")

APK="$HERE/app/build/outputs/apk/$MODE/app-$MODE.apk"
DEST="$DIST/WordCraft-$VERSION-android-$MODE.apk"
cp "$APK" "$DEST"
echo "$DEST"
