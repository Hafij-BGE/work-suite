//! WorkSuite for Android: Writer (WordCraft), Slides (DeckCraft) and Sheets (GridCraft) in one
//! app, with a home screen to pick one and a bar to switch between them ([`suite`]).
//!
//! Each app is the same editor as its desktop version (`crates/ui-egui`, `deckcraft/crates/ui-egui`,
//! `gridcraft/crates/ui-egui`), run through eframe on Android's `NativeActivity` (wgpu: Vulkan, or
//! OpenGL ES where Vulkan is missing). Build the APK with `packaging/android/build.sh` (see
//! `packaging/android/README.md`).
//!
//! On Android:
//! - touch: a finger scrolls the page, a tap clicks, a long press opens the page's menu,
//!   double-tap-and-drag selects, two fingers zoom ([`wordcraft_ui_egui::touch`]);
//! - Back works as Escape (closes the File menu, a dialog or a menu); the soft keyboard comes up
//!   when a tap puts Writer's caret on the page, and for text fields;
//! - Open, Save As and Export use Android's own file picker, and documents open from file managers
//!   ("Open with"); saves go back to the picked document (see `saf.rs` and `android.rs`);
//! - no control channel, printing (Export › PDF instead) or system clipboard (copy and paste work
//!   inside the app).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod saf;
pub mod shell;
pub mod suite;
