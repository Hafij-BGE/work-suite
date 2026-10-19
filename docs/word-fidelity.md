# Comparing WordCraft's pages with Word's

> **Last updated:** 2026-10-19 · **Change:** new (synthetic corpus, Word export script, `xtask fidelity`, first observations)

How we measure whether WordCraft lays a DOCX out the way Microsoft Word does: same fonts, line
breaks, spacing, tables, pictures, page breaks and page count. This is black-box observation of
Word's output on documents we wrote ourselves ([clean room](../CLAUDE.md#clean-room)); nothing
inside Word is read, and Word's PDFs are never committed.

## The loop

1. `cargo xtask word-corpus DIR` writes the synthetic corpus: 18 small DOCX files, each aimed at
   one area (listed in `DIR/MANIFEST.txt`). Every file defines all its styles, page size and
   compatibility mode, so Word's Normal template and theme can't change the result. Text and
   pictures are generated, so the files carry no third-party content.
2. On a Windows machine with Word, `xtask/word-reference/export-word-pdfs.ps1 -Corpus DIR` opens
   each file read-only through Word's automation interface, exports a PDF (print quality, whole
   document) into `DIR/pdf/`, closes without saving, and writes `manifest.json` with Word's
   version and build, the active printer and which corpus fonts Word has.
3. Back on the development machine: `cargo xtask fidelity DIR OUT`. For each `NAME.docx` with a
   `NAME.pdf` (or `pdf/NAME.pdf`), WordCraft converts the DOCX and
   [`visual-diff`](../xtask/src/visual_diff.rs) compares the two PDFs. `OUT/summary.md` gives,
   per document: page counts, share of Word's lines broken at the same place, pages starting
   with the same line, median vertical offset of matched lines, mean SSIM, and the reference
   PDF's producer. `OUT/NAME/result.json` has per-page numbers and the first differing line runs;
   `diff_pNNN.ppm` shows Word's ink red and WordCraft's blue. `--private` keeps only numbers and
   hashes, for confidential documents.

Fonts decide most line breaks, so compare with Word's fonts. WordCraft never bundles them; for a
comparison on Linux, point `CRAFT_EXTRA_FONT_DIRS` (a path list) at fonts you already have, such as
a Windows partition's `Windows/Fonts` and Microsoft 365's cloud font cache
`Users/<you>/AppData/Local/Microsoft/FontCache/4/CloudFonts` (Aptos). On Windows WordCraft now
scans that cloud font cache itself.

## What a match means

A document counts as matching only when Word's and WordCraft's page counts are equal, 100% of
lines break at the same place, every page starts with the same line, and the median vertical
offset is under 0.5 pt. SSIM is a guide only: anti-aliasing and glyph rasterisation differ even
when the layout is identical. Results come from one Word build (recorded in `manifest.json`);
Word 2021, 2024 and Microsoft 365 share the layout engine for compatibility mode 15 but are not
guaranteed identical, so a result is stated for the build measured.

## Observations

| Date | Word build | Document | Finding | State |
|---|---|---|---|---|
| 2026-10-19 | Microsoft 365 (screenshot; build not recorded) | A converter-made letter: compatibility mode 12, Aptos 12 pt, US Letter, justified, Shift+Enter address and signature lines | With Aptos available: same line breaks in all four body paragraphs, same page split (three signature lines on page 1, two on page 2), first and last baselines within ~1 pt at screenshot resolution. Word spreads justified lines that end in Shift+Enter; WordCraft left them ragged | Fixed: `layout::tests::justified_lines_ending_with_shift_enter_stretch_like_word`; `w:doNotExpandShiftReturn` read, written and honoured |
| 2026-10-19 | — | Same letter without Aptos installed | WordCraft fell back to Arial and fit the letter on one page; Word needs two | Font availability, not layout: WordCraft now says which fonts are missing (status bar, `file.info`, `wordcraft-cli convert`) and installs them locally from a font file, a folder or a Windows installation on the same PC (`font.add`); a metric-compatible open Aptos substitute is still missing |

The synthetic corpus has not been measured against Word yet; its rows go here once exported.
