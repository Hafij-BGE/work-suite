//! WordCraft for Android phones and tablets.
//!
//! Runs the same [`wordcraft_ui_egui::WordApp`] as the desktop app, through eframe on Android's
//! `NativeActivity` (wgpu: Vulkan, or OpenGL ES where Vulkan is missing). Build the APK with
//! `packaging/android/build.sh` (see `packaging/android/README.md`).
//!
//! Differences from the desktop app:
//! - touch: a finger scrolls the page, a tap clicks, a long press opens the page's menu,
//!   double-tap-and-drag selects, two fingers zoom ([`wordcraft_ui_egui::touch`]);
//! - the soft keyboard comes up when a tap puts the caret on the page, and Back works as Escape
//!   (closes the File menu, a dialog or a menu);
//! - files are opened and saved through the built-in file browser, starting in
//!   `Documents/WordCraft` (see `android.rs` for the folders);
//! - no control channel, printing (Export › PDF instead) or system clipboard (copy and paste work
//!   inside the app).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod android;
pub mod shell;
