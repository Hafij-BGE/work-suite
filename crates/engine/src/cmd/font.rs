//! Fonts the user adds without rebuilding (#582): `font.add`, `font.list`, `font.remove`.
//!
//! An added font joins the process-wide font database on top of the bundled and system fonts
//! (never replacing a face already there), so it shows in the font lists, lays out, renders and
//! embeds in PDFs like any other. Where it is kept between runs is [`FontDb::load_user_fonts`]'s
//! folder (desktop); without one it lasts for the session (the web shell stores it itself).

use serde_json::{Value, json};
use wordcraft_fonts::{FontDb, UserFont};

use crate::cmd::insert::base64_decode;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("font.add", "Add Font…", "Home › Font", add)
            .params(r#"{"path"?: string, "data"?: base64, "name"?: string (file name for "data")} — a TTF, OTF, TTC, WOFF or WOFF2 file; none: pick a file. Or {"folder": string} — every font file in a folder and its subfolders; or {"windows": true} — from the Windows installations on this computer (a dual-boot partition's Windows/Fonts and Microsoft 365 cloud fonts such as Aptos). With "folder" or "windows": "families"?: [string] adds only those families; "windows" without "families" adds the fonts the document is missing unless "all": true. Fonts are copied to this computer's user font folder only"#)
            .pure(),
        CommandSpec::new("font.list", "Added Fonts", "Home › Font", list).params(r#"{"all"?: bool (also every available family)}"#).pure(),
        CommandSpec::new("font.remove", "Remove Added Font", "Home › Font", remove).params(r#"{"family": string}"#).pure(),
        CommandSpec::new("font.loadSystem", "Load System Installed Fonts…", "Home › Font", load_system).pure(),
    ]
}

/// The document's fonts that aren't installed, each with the family drawn instead. Lines and
/// pages can break differently from Word until the font is installed or added (`font.add`).
pub fn missing_fonts(s: &mut Session) -> Vec<wordcraft_layout::MissingFont> {
    let layout = s.layout();
    wordcraft_layout::missing_fonts(&s.doc, &layout)
}

/// [`missing_fonts`] as JSON: `[{"font": "Aptos", "shownWith": "Arial"}]`.
pub fn missing_fonts_json(s: &mut Session) -> Value {
    json!(missing_fonts(s).iter().map(|m| json!({"font": m.requested, "shownWith": m.shown_with})).collect::<Vec<_>>())
}

/// One line naming the document's fonts that aren't installed, for the status bar or a
/// warning: "Not installed: Aptos (shown with Arial). Lines and pages may differ from Word."
pub fn missing_fonts_notice(s: &mut Session) -> Option<String> {
    let list = missing_fonts(s);
    if list.is_empty() {
        return None;
    }
    let names: Vec<String> = list.iter().map(|m| format!("{} (shown with {})", m.requested, m.shown_with)).collect();
    Some(format!("Not installed: {}. Lines and pages may differ from Word.", names.join(", ")))
}

fn load_system(s: &mut Session, _v: &Value) -> CmdResult {
    s.push_ui_request(json!({"open": "loadSystemFonts"}));
    Ok(json!({"pending": true}))
}

fn font_json(f: &UserFont) -> Value {
    json!({
        "file": f.file,
        "families": f.families(),
        "faces": f.faces.iter().map(|(family, style)| json!({"family": family, "style": style})).collect::<Vec<_>>(),
        "path": f.path.as_ref().map(|p| p.to_string_lossy().to_string()),
    })
}

/// Read a font file named by the user, refusing one larger than the cap before reading it.
#[cfg(not(target_arch = "wasm32"))]
fn read_font_file(path: &str) -> Result<Vec<u8>, CmdError> {
    // Through `io`, which keeps automation inside its read root (#769).
    crate::io::read_file_capped(path, wordcraft_fonts::MAX_FONT_FILE_BYTES as u64, "font file").map_err(CmdError::Failed)
}

/// The families a folder or Windows import is limited to: `families`, else (for Windows) the
/// document's missing fonts unless `all`; `None` adds everything.
#[cfg(not(target_arch = "wasm32"))]
fn import_filter(s: &mut Session, v: &Value, windows: bool) -> Option<Vec<String>> {
    if let Some(list) = v.get("families").and_then(Value::as_array) {
        return Some(list.iter().filter_map(Value::as_str).map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect());
    }
    if !windows || p::bool(v, "all") == Some(true) {
        return None;
    }
    Some(missing_fonts(s).into_iter().map(|m| m.requested).collect())
}

/// Add every font file under `dirs` whose families are in `only` (all with `None`). Files
/// already available are skipped; each file is read through the automation access check.
#[cfg(not(target_arch = "wasm32"))]
fn add_from_dirs(s: &mut Session, dirs: &[std::path::PathBuf], only: Option<Vec<String>>) -> CmdResult {
    /// At most this many failures are listed.
    const MAX_ERRORS: usize = 20;
    check_font_store(FontDb::global().user_font_dir().as_deref())?;
    let (mut added, mut skipped, mut errors) = (Vec::<String>::new(), 0usize, Vec::<String>::new());
    for path in wordcraft_fonts::font_files_under(dirs) {
        if let Some(only) = &only {
            let fams = wordcraft_fonts::font_file_families(&path);
            if !fams.iter().any(|f| only.iter().any(|o| o.eq_ignore_ascii_case(f))) {
                continue;
            }
        }
        let shown = path.to_string_lossy().to_string();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "font".into());
        let bytes = match read_font_file(&shown) {
            Ok(b) => b,
            Err(e) => {
                if errors.len() < MAX_ERRORS {
                    errors.push(e.to_string());
                }
                continue;
            }
        };
        match FontDb::global().install_font(&name, &bytes) {
            Ok(r) => {
                for f in r.font.families() {
                    if !added.contains(&f) {
                        added.push(f);
                    }
                }
            }
            Err(wordcraft_fonts::UserFontError::AlreadyAvailable(_)) => skipped += 1,
            Err(e) => {
                if errors.len() < MAX_ERRORS {
                    errors.push(format!("{shown}: {e}"));
                }
            }
        }
    }
    if !added.is_empty() {
        s.relayout();
    }
    let still: Vec<String> = only.unwrap_or_default().into_iter().filter(|f| !FontDb::global().has_family(f)).collect();
    s.status = match (added.is_empty(), still.is_empty()) {
        (false, true) => format!("Added {}", added.join(", ")),
        (false, false) => format!("Added {}; not found: {}", added.join(", "), still.join(", ")),
        (true, false) => format!("Not found: {}", still.join(", ")),
        (true, true) => "No new fonts to add".to_string(),
    };
    Ok(json!({
        "added": added,
        "alreadyAvailable": skipped,
        "notFound": still,
        "errors": errors,
        "searched": dirs.iter().map(|d| d.to_string_lossy().to_string()).collect::<Vec<_>>(),
    }))
}

/// Adding or removing a font writes to the user font folder: automation confined to a write root
/// (#769) may do that only when the folder is inside it.
fn check_font_store(dir: Option<&std::path::Path>) -> Result<(), CmdError> {
    match dir {
        Some(dir) => crate::file_access::check_write(&dir.to_string_lossy()).map_err(CmdError::Failed),
        None => Ok(()),
    }
}

fn add(s: &mut Session, v: &Value) -> CmdResult {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(folder) = p::str(v, "folder") {
            crate::file_access::check_read(folder).map_err(CmdError::Failed)?;
            if !std::path::Path::new(folder).is_dir() {
                return Err(CmdError::Failed(format!("{folder}: not a folder")));
            }
            let only = import_filter(s, v, false);
            return add_from_dirs(s, &[folder.into()], only);
        }
        if p::bool(v, "windows") == Some(true) {
            let dirs = wordcraft_fonts::windows_font_dirs();
            if dirs.is_empty() {
                return Err(CmdError::Failed(
                    "no Windows installation found on this computer (looked for a mounted partition with Windows/Fonts)".into(),
                ));
            }
            let only = import_filter(s, v, true);
            if only.as_ref().is_some_and(Vec::is_empty) {
                s.status = "The document's fonts are all installed".into();
                return Ok(json!({"added": [], "alreadyAvailable": 0, "notFound": [], "errors": [], "searched": []}));
            }
            return add_from_dirs(s, &dirs, only);
        }
    }
    #[cfg(target_arch = "wasm32")]
    if p::str(v, "folder").is_some() || p::bool(v, "windows") == Some(true) {
        return Err(CmdError::Failed("adding fonts from folders isn't available on the web".into()));
    }
    if p::str(v, "url").is_some() {
        return Err(CmdError::Failed(
            "adding a font from a link isn't available yet: download the font file and add it with {\"path\"} or {\"data\"}".into(),
        ));
    }
    let (name, bytes) = if let Some(d) = p::str(v, "data") {
        // Base64 is a third larger than the file: refuse before decoding an oversized one.
        if d.len() / 4 * 3 > wordcraft_fonts::MAX_FONT_FILE_BYTES + 3 {
            return Err(CmdError::Failed(format!("the font file is larger than {} MB", wordcraft_fonts::MAX_FONT_FILE_BYTES >> 20)));
        }
        let bytes = base64_decode(d).ok_or_else(|| CmdError::Params("bad base64 data".into()))?;
        (p::str(v, "name").unwrap_or("font").to_string(), bytes)
    } else if let Some(path) = p::str(v, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            (path.to_string(), read_font_file(path)?)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err(CmdError::Failed("reading files by path isn't available on the web".into()));
        }
    } else {
        // From the ribbon: the UI shows a file picker and runs this again with the file.
        s.push_ui_request(json!({"open": "addFont"}));
        return Ok(json!({"pending": true}));
    };
    check_font_store(FontDb::global().user_font_dir().as_deref())?;
    let r = FontDb::global().install_font(&name, &bytes).map_err(|e| CmdError::Failed(e.to_string()))?;
    // Text already set in the family (shown in a fallback until now) takes the new font.
    s.relayout();
    let families = r.font.families().join(", ");
    s.status = match &r.warning {
        Some(w) => format!("Added {families} ({w})"),
        None => format!("Added {families}"),
    };
    let mut out = font_json(&r.font);
    if let Some(w) = r.warning {
        out["warning"] = json!(w);
    }
    Ok(out)
}

fn list(_: &mut Session, v: &Value) -> CmdResult {
    let db = FontDb::global();
    let mut out = json!({
        "fonts": db.user_fonts().iter().map(font_json).collect::<Vec<_>>(),
        "folder": db.user_font_dir().map(|p| p.to_string_lossy().to_string()),
    });
    if p::bool(v, "all") == Some(true) {
        out["families"] = json!(db.families());
    }
    Ok(out)
}

fn remove(s: &mut Session, v: &Value) -> CmdResult {
    let family = p::str(v, "family").filter(|f| !f.trim().is_empty()).ok_or_else(|| CmdError::Params("family is required".into()))?;
    check_font_store(FontDb::global().user_font_dir().as_deref())?;
    let gone = FontDb::global().remove_user_font(family.trim()).map_err(|e| CmdError::Failed(e.to_string()))?;
    s.relayout();
    s.status = format!("Removed {}", family.trim());
    Ok(json!({"removed": gone.iter().map(font_json).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confined_automation_changes_only_a_font_store_inside_its_write_root() {
        let root = std::env::temp_dir().join(format!("wordcraft-font-store-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("wordcraft-font-store-out-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let write = crate::file_access::AutomationRoots::new(None, Some(&root)).unwrap();
        let read_only = crate::file_access::AutomationRoots::new(Some(&root), None).unwrap();
        crate::file_access::confine(write.as_ref(), || {
            assert!(check_font_store(None).is_ok(), "no folder: the font lasts for the session");
            assert!(check_font_store(Some(&root.join("fonts"))).is_ok());
            assert!(check_font_store(Some(&outside)).is_err());
        });
        crate::file_access::confine(read_only.as_ref(), || assert!(check_font_store(Some(&root)).is_err()));
        assert!(check_font_store(Some(&outside)).is_ok(), "unconfined: the person at the keyboard");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
