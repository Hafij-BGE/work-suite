# winit 0.30.13, patched for WorkSuite (Android stylus)

This is winit 0.30.13 as published on crates.io (Apache-2.0, see `LICENSE`), used through
`[patch.crates-io]` in the root `Cargo.toml`, with one change for Android:

- `src/platform_impl/android/stylus.rs` (new) and the motion-event handling in
  `src/platform_impl/android/mod.rs`: a pointer whose Android tool type is a stylus, its eraser
  end or a mouse drives the cursor (`CursorMoved`, `MouseInput`, `CursorLeft`) instead of being
  reported as a touch, so a pen hovers, clicks and drags like a mouse (the pen's side button is
  the right button). Fingers that land while the pen is on or near the screen are dropped (palm
  rejection).
- `src/platform/android.rs`: `winit::platform::android::stylus()` says what the pen is doing
  (touching, hovering, eraser end, side button, pressure).

Upstream winit 0.30 reports every pointer as a touch and ignores hover events. Removed from the
published crate: `examples/data` and `docs/res` (images the library doesn't use).

Nothing else differs from the published crate. To update winit, copy the new release here and
apply the same change again.
