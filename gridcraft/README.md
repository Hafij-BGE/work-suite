<p align="center">
  <img alt="WorkSuite" src="../docs/images/worksuite/icon.svg" width="88">
</p>

<h1 align="center">WorkSuite Sheets</h1>

<p align="center">
  The spreadsheet app of <a href="../README.md"><b>WorkSuite</b></a>, made by <b>Md Hafijur Rahman</b>.
</p>

Sheets opens and saves Excel workbooks (`.xlsx`) and `.csv` files, with formulas, charts and ink
on the Draw tab. This folder is its own Cargo workspace; the Android app
([`apps/wordcraft-android`](../apps/wordcraft-android)) uses its interface crate
([`crates/ui-egui`](crates/ui-egui)) and engine ([`crates/engine`](crates/engine)) by path.

| Where | What |
|---|---|
| [`crates/engine`](crates/engine) | The workbook, every command, open and save |
| [`crates/ui-egui`](crates/ui-egui) | The interface: ribbon, grid, formula bar, the File view |
| [`crates/xlsx`](crates/xlsx) | Excel files |
| [`crates/formula`](crates/formula), [`crates/functions`](crates/functions) | Formulas and functions |
| [`crates/chart`](crates/chart) | Charts |

Build and test from this folder with `cargo test -p gridcraft-ui-egui`. How the Android app is
built is in [`packaging/android/README.md`](../packaging/android/README.md).

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE); notices in
[NOTICE](NOTICE), and every bundled asset in [ATTRIBUTION.md](ATTRIBUTION.md).
