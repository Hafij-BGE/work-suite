# WordCraft for Android

The Android app is the same editor as the desktop one (`crates/ui-egui`), started from
`apps/wordcraft-android` in a `NativeActivity` (no Java code). It runs on Android 8 (API 26) or later,
on phones and tablets; a tablet, or a phone held sideways, shows the most of the ribbon.

## What is different on Android

- **A lighter ribbon.** File, Home, Insert, Design and Layout. Draw, References, Mailings, Review,
  View, Developer, Zotero and Help are left out (their commands still work from keyboard shortcuts
  and the command search, the magnifier in the title bar).
- **A shorter File menu.** Home, New, Open, Save, Save As, Export, Close and About (and Recover,
  only when there is something to recover). Print goes through Export › PDF.
- **Touch.**

  | Gesture | Does |
  |---|---|
  | Drag one finger | Scroll (and coast after a flick) |
  | Tap | Place the caret, press a button |
  | Double-tap | Select a word |
  | Double-tap and drag | Select more, word by word; move a picture or a slider |
  | Long press | The page's menu: Cut, Copy, Paste… |
  | Pinch | Zoom |
  | Back | Close the File menu, a dialog or a menu (like Escape) |

- **Keyboard.** The on-screen keyboard comes up when a tap puts the caret on the page. A
  hardware keyboard works with the usual shortcuts.
- **Files.** Open, Save As and Export use WordCraft's own file browser, starting in
  `Documents/WordCraft`. To open documents saved by other apps (in `Download`, say), allow
  *All files access* in Settings › Apps › WordCraft.
- **Not on Android (yet):** printing (export a PDF and print that), the system clipboard (copy and
  paste work inside WordCraft), the control channel and MCP bridge.

## Building

Needs the Android NDK and SDK (platform 34), a JDK 17 or later, Gradle 8.9 or later, and:

```sh
rustup target add aarch64-linux-android      # and x86_64-linux-android for the emulator
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/android-ndk ANDROID_HOME=/path/to/android-sdk
packaging/android/build.sh                   # or: build.sh release
adb install dist/release/WordCraft-*-android-debug.apk
```

`WORDCRAFT_ANDROID_ABIS="arm64-v8a x86_64"` adds the emulator's CPU. A release build is signed with
`WORDCRAFT_KEYSTORE`, `WORDCRAFT_KEYSTORE_PASSWORD`, `WORDCRAFT_KEY_ALIAS` and
`WORDCRAFT_KEY_PASSWORD` when they are set, else with the debug key.

The **Android** workflow (`.github/workflows/android.yml`) builds the debug APK on every change
and keeps it as the run's artifact.

Logs: `adb logcat -s WordCraft`.

## Known limits

- Typing goes through Android's key events (NativeActivity has no full input-method connection):
  letters, digits and punctuation from the on-screen keyboard work, but keyboards that compose
  text (Bengali, Chinese, Japanese, Korean…), swipe typing and autocorrect suggestions may not.
  A hardware keyboard works.
- The on-screen keyboard can cover the bottom of the screen: scroll the page up to see the caret.
