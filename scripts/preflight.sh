#!/usr/bin/env bash
# The checks that broke CI after past upstream merges, run locally before a push.
#
# Usage: scripts/preflight.sh [--quick] [--fix] [--install-hook]
#
#   (none)          everything below, including clippy and cargo check (minutes)
#   --quick         format, lock files and workspace members only (seconds); the pre-push hook runs this
#   --fix           run `cargo fmt --all` in each workspace instead of only checking
#   --install-hook  make `git push` run `scripts/preflight.sh --quick` first
#
# This repository has three Cargo workspaces: the root (WordCraft), deckcraft/ and gridcraft/.
# `cargo xtask ci` (AGENTS.md, and the gate in ci.yml) covers the root only.
#
# What it catches, and the CI failure each check stands for:
#   1. Unformatted code after an upstream merge. The gate's `cargo fmt --all -- --check` failed
#      four times this way. The root's `cargo fmt --all` also formats the deckcraft/ and gridcraft/
#      crates WordCraft depends on by path, so a gridcraft merge can fail the WordCraft gate.
#      Fix: `scripts/preflight.sh --fix` (formats all three workspaces).
#   2. A Cargo.lock out of date with its Cargo.toml: every CI build passes --locked.
#      Fix: (cd <workspace> && cargo metadata --format-version 1 >/dev/null), commit the lock.
#   3. A glob (crates/*, apps/*) in deckcraft/ or gridcraft/ [workspace] members, or a crate there
#      missing from the list. Dependabot expands a nested workspace's globs against the repository
#      root and fails. Fix: list each crate by path; add new upstream crates to the list.
#   4. clippy -D warnings in the root workspace (failed four times after merges).
#      Fix: the warnings clippy prints.
#   5. Compile errors in deckcraft/ or gridcraft/ (Android builds them), and in WorkSuite-only code
#      after an upstream API change, e.g. apps/wordcraft-android. The Android crate is checked for
#      aarch64-linux-android only when cargo-ndk and ANDROID_NDK_HOME are present; CI's android.yml
#      checks it otherwise.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

QUICK=0
FIX=0
for a in "$@"; do
  case "$a" in
    --quick) QUICK=1 ;;
    --fix) FIX=1 ;;
    --install-hook)
      hooks="$(git rev-parse --git-path hooks)"
      mkdir -p "$hooks"
      printf '#!/bin/sh\nexec "$(git rev-parse --show-toplevel)/scripts/preflight.sh" --quick\n' >"$hooks/pre-push"
      chmod +x "$hooks/pre-push"
      echo "installed $hooks/pre-push"
      exit 0
      ;;
    -h | --help) sed -n '2,31p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

WORKSPACES=(. deckcraft gridcraft)
fail=0
log="$(mktemp)"
trap 'rm -f "$log"' EXIT

echo "==> format"
for ws in "${WORKSPACES[@]}"; do
  if [ "$FIX" = 1 ]; then
    (cd "$ws" && cargo fmt --all) && echo "  fixed $ws" || fail=1
  elif (cd "$ws" && cargo fmt --all -- --check >"$log" 2>&1); then
    echo "  ok    $ws"
  else
    echo "  FAIL  $ws is not formatted"
    grep -m3 -E '^(Diff in|error)' "$log" | sed 's/^/        /' || true
    echo "        fix: scripts/preflight.sh --fix"
    fail=1
  fi
done

echo "==> lock files (--locked builds)"
for ws in "${WORKSPACES[@]}"; do
  if (cd "$ws" && cargo metadata --locked --format-version 1 >/dev/null 2>"$log"); then
    echo "  ok    $ws/Cargo.lock"
  else
    echo "  FAIL  $ws/Cargo.lock is out of date with $ws/Cargo.toml"
    grep -m1 -E '^error' "$log" | sed 's/^/        /' || true
    echo "        fix: (cd $ws && cargo metadata --format-version 1 >/dev/null) and commit the lock"
    fail=1
  fi
done

echo "==> nested workspace members (Dependabot)"
if ! python3 -I - <<'PY'; then fail=1; fi
import pathlib, sys, tomllib
bad = 0
for ws in ("deckcraft", "gridcraft"):
    root = pathlib.Path(ws)
    members = tomllib.loads((root / "Cargo.toml").read_text()).get("workspace", {}).get("members", [])
    globs = [m for m in members if any(c in m for c in "*?[")]
    on_disk = sorted(str(p.parent.relative_to(root)) for d in ("crates", "apps", "xtask")
                     for p in (root / d).glob("**/Cargo.toml") if "target" not in p.parts)
    on_disk = [p for p in on_disk if not any(p != q and p.startswith(q + "/") for q in on_disk)]
    missing = [p for p in on_disk if p not in members]
    if globs or missing:
        bad = 1
        print(f"  FAIL  {ws}/Cargo.toml [workspace] members")
        for g in globs:
            print(f"          glob {g!r}: list each crate by path instead")
        for m in missing:
            print(f"          missing {m!r}")
    else:
        print(f"  ok    {ws}: {len(members)} members, listed by path")
sys.exit(bad)
PY

if [ "$QUICK" = 0 ]; then
  echo "==> cargo clippy (root workspace, as the gate runs it)"
  if ! cargo clippy --workspace --all-targets --locked -- -D warnings; then fail=1; fi
  for ws in deckcraft gridcraft; do
    echo "==> cargo check $ws"
    if ! (cd "$ws" && cargo check --workspace --all-targets --locked); then fail=1; fi
  done
  if command -v cargo-ndk >/dev/null && [ -n "${ANDROID_NDK_HOME:-}" ]; then
    echo "==> cargo ndk check wordcraft-android"
    if ! cargo ndk -t arm64-v8a -P 26 check -p wordcraft-android --locked; then fail=1; fi
  else
    echo "==> wordcraft-android for Android: skipped (no cargo-ndk or ANDROID_NDK_HOME; CI checks it)"
  fi
fi

if [ "$fail" = 1 ]; then
  echo "preflight failed: fix the above before pushing (git push --no-verify skips this check)" >&2
  exit 1
fi
echo "preflight ok"
