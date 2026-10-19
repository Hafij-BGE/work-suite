//! `cargo xtask fidelity <corpus-dir> <outdir> [--cli PATH] [--private] [--dpi N]`: compares
//! WordCraft's pages with Word's for a whole folder of documents.
//!
//! For every `NAME.docx` in the corpus folder with a Word export `NAME.pdf` next to it (or in a
//! `pdf/` subfolder), WordCraft converts the DOCX to `OUTDIR/NAME/wordcraft.pdf` and
//! [`visual_diff`](crate::visual_diff) compares the two. `OUTDIR/summary.md` and `summary.json`
//! list each document's page counts, share of lines broken where Word breaks them, pages starting
//! with the same line, median vertical offset and SSIM, plus the producer of the reference PDF
//! (which Word made it). Nothing here is committed: Word's PDFs stay local.
//!
//! The converter is `wordcraft-cli` from `--cli`, else `$CARGO_TARGET_DIR/release`, else
//! `target/release`. Fonts Word used that aren't installed can be pointed at with
//! `CRAFT_EXTRA_FONT_DIRS` (a path list), e.g. a Windows partition's `Windows/Fonts`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

pub fn run(args: &[&str]) -> Result<(), String> {
    let private = args.contains(&"--private");
    let value = |flag: &str| args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1)).copied();
    let dpi = match value("--dpi") {
        Some(v) => v.parse::<u32>().ok().filter(|d| (36..=300).contains(d)).ok_or("--dpi takes 36..=300")?,
        None => 100,
    };
    let pos: Vec<&str> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && matches!(args[i - 1], "--dpi" | "--cli")))
        .map(|(_, a)| *a)
        .collect();
    let [corpus, out] = pos.as_slice() else {
        return Err("usage: cargo xtask fidelity <corpus-dir> <outdir> [--cli PATH] [--private] [--dpi N]".into());
    };
    let (corpus, out) = (Path::new(corpus), PathBuf::from(out));
    let cli = match value("--cli") {
        Some(p) => PathBuf::from(p),
        None => default_cli(),
    };
    if !cli.is_file() {
        return Err(format!("{} not found: build it with `cargo build --release -p wordcraft-cli` or pass --cli", cli.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let pairs = pairs(corpus)?;
    if pairs.is_empty() {
        return Err(format!("no NAME.docx with a NAME.pdf (or pdf/NAME.pdf) in {}", corpus.display()));
    }
    let mut rows = Vec::new();
    for (name, docx, reference) in &pairs {
        let dir = out.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ours = dir.join("wordcraft.pdf");
        let producer = producer(reference);
        let row = match convert(&cli, docx, &ours) {
            Err(e) => json!({"name": name, "error": e, "producer": producer}),
            Ok(()) => match crate::visual_diff::run_one(reference, &ours, &dir, dpi, private) {
                Ok(report) => json!({"name": name, "producer": producer, "summary": report["summary"]}),
                Err(e) => json!({"name": name, "error": e, "producer": producer}),
            },
        };
        eprintln!("{name}: {}", row.get("summary").unwrap_or(&row["error"]));
        rows.push(row);
    }
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&json!(rows)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let md = table(&rows);
    std::fs::write(out.join("summary.md"), &md).map_err(|e| e.to_string())?;
    print!("{md}");
    Ok(())
}

fn default_cli() -> PathBuf {
    let exe = if cfg!(windows) { "wordcraft-cli.exe" } else { "wordcraft-cli" };
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(t) => PathBuf::from(t).join("release").join(exe),
        None => crate::root().join("target/release").join(exe),
    }
}

/// `(name, docx, reference pdf)` for every DOCX with a PDF, sorted by name.
fn pairs(corpus: &Path) -> Result<Vec<(String, PathBuf, PathBuf)>, String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(corpus).map_err(|e| format!("{}: {e}", corpus.display()))?.flatten() {
        let p = e.path();
        if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("docx")) {
            continue;
        }
        let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        let candidates = [corpus.join(format!("{stem}.pdf")), corpus.join("pdf").join(format!("{stem}.pdf"))];
        if let Some(r) = candidates.into_iter().find(|c| c.is_file()) {
            out.push((stem, p, r));
        }
    }
    out.sort();
    Ok(out)
}

fn convert(cli: &Path, docx: &Path, pdf: &Path) -> Result<(), String> {
    let o = Command::new(cli).arg("convert").arg(docx).arg(pdf).output().map_err(|e| format!("{}: {e}", cli.display()))?;
    if o.status.success() { Ok(()) } else { Err(format!("convert failed: {}", String::from_utf8_lossy(&o.stderr).trim())) }
}

/// The reference PDF's Producer (which Word wrote it), from `pdfinfo`.
fn producer(pdf: &Path) -> String {
    let Ok(o) = Command::new("pdfinfo").arg(pdf).output() else { return String::new() };
    String::from_utf8_lossy(&o.stdout).lines().find_map(|l| l.strip_prefix("Producer:")).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn table(rows: &[Value]) -> String {
    let mut md = String::from(
        "| Document | Pages Word/WC | Same line breaks | Page starts same | Median Δy (pt) | SSIM | Reference |\n|---|---|---|---|---|---|---|\n",
    );
    let (mut lines, mut same) = (0.0, 0.0);
    for r in rows {
        let name = r["name"].as_str().unwrap_or("");
        let producer = r["producer"].as_str().unwrap_or("");
        let Some(s) = r.get("summary") else {
            let _ = writeln!(md, "| {name} | error: {} | | | | | {producer} |", r["error"].as_str().unwrap_or("?"));
            continue;
        };
        let pages = &s["pages"];
        let n = s["lines"][0].as_f64().unwrap_or(0.0);
        lines += n;
        same += n * s["same_break_pct"].as_f64().unwrap_or(0.0) / 100.0;
        let _ = writeln!(
            md,
            "| {name} | {}/{} | {}% | {}/{} | {} | {} | {producer} |",
            pages[0], pages[1], s["same_break_pct"], s["page_starts_same"], pages[0], s["dy_median"], s["ssim_mean"]
        );
    }
    let _ = writeln!(
        md,
        "\nAll documents: {:.1}% of Word's {} lines broken at the same place.",
        if lines > 0.0 { 100.0 * same / lines } else { 0.0 },
        lines
    );
    md
}
