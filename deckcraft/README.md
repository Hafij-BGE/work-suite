<p align="center">
  <img alt="WorkSuite" src="../docs/images/worksuite/icon.svg" width="88">
</p>

<h1 align="center">WorkSuite Slides</h1>

<p align="center">
  The presentation app of <a href="../README.md"><b>WorkSuite</b></a>, made by <b>Md Hafijur Rahman</b>.
</p>

Slides opens and saves PowerPoint presentations (`.pptx`), plays slide shows, and draws with a
stylus on the Draw tab. This folder is its own Cargo workspace; the Android app
([`apps/wordcraft-android`](../apps/wordcraft-android)) uses its interface crate
([`crates/ui-egui`](crates/ui-egui)) and engine ([`crates/engine`](crates/engine)) by path.

| Where | What |
|---|---|
| [`crates/engine`](crates/engine) | The presentation, every command, open and save |
| [`crates/ui-egui`](crates/ui-egui) | The interface: ribbon, slides, slide show, the File view |
| [`crates/pptx`](crates/pptx) | PowerPoint files |
| [`crates/render`](crates/render) | Drawing slides |

Build and test from this folder with `cargo test -p deckcraft-ui-egui`. How the Android app is
built is in [`packaging/android/README.md`](../packaging/android/README.md).

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE); notices in
[NOTICE](NOTICE), and every bundled asset in [ATTRIBUTION.md](ATTRIBUTION.md).
