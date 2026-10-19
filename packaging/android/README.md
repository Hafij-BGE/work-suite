# WorkSuite for Android (Writer, Slides and Sheets)

*WorkSuite* is one Android app with three editors: **Writer** (WordCraft, `crates/ui-egui`),
**Slides** (DeckCraft, `deckcraft/`) and **Sheets** (GridCraft, `gridcraft/`). It opens on a home
screen to pick one; a bar at the top switches between them, and each keeps its files open. The app
is `apps/wordcraft-android` (`suite.rs`) in a `NativeActivity`; a small Java activity
(`app/src/main/java/…/WriterActivity.java`) adds what only Java can reach: Android's file picker and
"Open with". The icon is drawn as Android vector drawables (`app/src/main/res`).

Made by Md Hafijur Rahman. The app works offline: it asks for no network permission, and its file
picker shows only files on the device.

Back closes what is open (a menu, a dialog, the File view over a document, a slide show), then goes
to the home screen, then puts the app in the background. The app draws edge to edge and keeps its
controls clear of the system bars and the keyboard (insets from `WriterActivity.insets()`, applied
as egui's safe area).

A stylus works as a pen (`vendor/winit`, see its `PATCHED.md`: winit 0.30 reports a pen as a
touch, so the suite patches it): the pen hovers, clicks, selects and drags as a mouse does while
fingers scroll and zoom; its side button is the right button; a hand resting on the screen while
the pen is near is ignored. The Draw tab (Writer, Slides, Sheets) picks a pen to write with, and
the pen's eraser end erases whatever tool is picked (`suite.rs`, `eraser_end`).

To look at the suite without a phone: `cargo run -p wordcraft-android --example suite_shot --
out.ppm [home|writer|slides|sheets] [dark|light] [width height scale] [ribbon tab]` (a software
renderer).

DeckCraft and GridCraft keep their own Cargo workspaces (the root workspace excludes them); the
Android crate depends on their UI crates by path.

## What is different on Android

- **A lighter ribbon.** Writer: File, Home, Insert, Draw, Design and Layout (References, Mailings,
  Review, View, Developer, Zotero and Help left out). Slides: Home, Insert, Draw, Design,
  Transitions and Slide Show (Animations, Record, Review and View left out). Sheets: Home, Insert,
  Draw, Page Layout, Formulas and Data (Review, View and Automate left out). Everything left out
  is still reachable through keyboard shortcuts and the command search.
- **A shorter File menu** in all three: Home, New, Open, Save, Save As, Export, Close and About
  (and, in Writer, Recover, only when there is something to recover). Print goes through Export › PDF.
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
- **Files.** Open, Save As, Export and Insert › Pictures use Android's file picker (Downloads,
  Documents, SD cards, Drive…), and documents open from file managers, mail and chat apps ("Open
  with", "Share"), each in the app its type belongs to. The editors work on a copy in the app's
  folder; every save is copied back to the picked document within a couple of seconds, and when
  the app goes to the background. A new presentation or workbook asks where to save the first
  time. Writer's File › Open › *Browse this device…* is its own browser, starting in
  `Documents/WorkSuite`.
- **Not on Android (yet):** printing (export a PDF and print that), the system clipboard (copy and
  paste work inside the app), the control channel and MCP bridge.

## Building

Needs the Android NDK and SDK (platform 34), a JDK 17 or later, Gradle 8.9 or later, and:

```sh
rustup target add aarch64-linux-android      # and x86_64-linux-android for the emulator
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/android-ndk ANDROID_HOME=/path/to/android-sdk
packaging/android/build.sh                   # or: build.sh release
adb install dist/release/WorkSuite-*-android-debug.apk
```

The app's version is in [`VERSION`](VERSION), apart from WordCraft's.
`WORDCRAFT_ANDROID_ABIS="arm64-v8a x86_64"` adds the emulator's CPU. A release build is signed with
`WORDCRAFT_KEYSTORE`, `WORDCRAFT_KEYSTORE_PASSWORD`, `WORDCRAFT_KEY_ALIAS` and
`WORDCRAFT_KEY_PASSWORD` when they are set, else with the debug key.

The **Android** workflow (`.github/workflows/android.yml`) builds the debug APK on every change
and keeps it as the run's artifact.

## Releasing

Bump [`VERSION`](VERSION), then run the **Android release** workflow (Actions › Android release ›
Run workflow), which tags the commit `android-v<VERSION>`, or push such a tag yourself. The workflow
(`.github/workflows/android-release.yml`) builds the release APK and publishes a GitHub Release with
[`release-notes.md`](release-notes.md); update that file first.

Android only installs an update signed with the same key as the installed app. Releases are
signed with WorkSuite's release key (certificate SHA-256
`c666b60c857c2c273c12a44efdfb680600e73faf948cd64cb7cad2183b1685f8`, CN=WorkSuite,
O=Md Hafijur Rahman), kept by its owner and given to the workflow as the repository secrets
`WORDCRAFT_KEYSTORE_BASE64` (the keystore, base64), `WORDCRAFT_KEYSTORE_PASSWORD`,
`WORDCRAFT_KEY_ALIAS` (`worksuite`) and `WORDCRAFT_KEY_PASSWORD` (Settings › Secrets and variables
› Actions). The workflow fails without them, or when the APK carries another certificate, rather
than publish an APK that can't update an installed WorkSuite. Keystores never go in the repository
(`.gitignore`). To replace a release that went wrong, run the workflow by hand with **replace**
ticked: once the new APK is built and checked, the old release and tag of that version are deleted
and published again.

Logs: `adb logcat -s WorkSuite`.

## Known limits

- Typing goes through Android's key events (NativeActivity has no full input-method connection):
  letters, digits and punctuation from the on-screen keyboard work, but keyboards that compose
  text (Bengali, Chinese, Japanese, Korean…), swipe typing and autocorrect suggestions may not.
  A hardware keyboard works.
- The on-screen keyboard can cover the bottom of the screen: scroll the page up to see the caret.
