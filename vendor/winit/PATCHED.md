# winit 0.30.13 (ArtCraft's fork), patched for WorkSuite (Android stylus)

This is the `storytold/winit` fork at `a517b3846ee8457d84ee4975110bfda6797c44dc` (winit 0.30.13,
Apache-2.0, see `LICENSE`; the fork's own changes are listed in `PATCHES.md`). It is what the
ArtCraft egui fork needs (`ImeResetCompleted`, `reset_ime_with_request`). It is used through
`[patch.crates-io]` in the root `Cargo.toml`, with one change for Android on top:

- `src/platform_impl/android/stylus.rs` (new) and the motion-event handling in
  `src/platform_impl/android/mod.rs`: a pointer whose Android tool type is a stylus, its eraser
  end or a mouse drives the cursor (`CursorMoved`, `MouseInput`, `CursorLeft`) instead of being
  reported as a touch, so a pen hovers, clicks and drags like a mouse (the pen's side button is
  the right button). Fingers that land while the pen is on or near the screen are dropped (palm
  rejection).
- `src/platform/android.rs`: `winit::platform::android::stylus()` says what the pen is doing
  (touching, hovering, eraser end, side button, pressure).

Upstream winit 0.30 reports every pointer as a touch and ignores hover events. Removed from the
fork's tree: `examples/data`, `docs`, `.github`. To update, copy the fork's new revision here and
apply the same change again.
