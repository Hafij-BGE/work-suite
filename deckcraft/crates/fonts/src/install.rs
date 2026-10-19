//! Installing fonts on this computer from the app: from a folder, or from a Windows installation
//! on the same PC (a dual-boot partition's `Windows/Fonts` and Microsoft 365's cloud fonts such as
//! Aptos). Files are copied to the user's own font folder, which every app of the suite (and other
//! programs) reads, and loaded into the running font database so text uses them at once. Nothing
//! is copied anywhere else.

use std::path::{Path, PathBuf};

use crate::FontDb;

/// Largest font file installed.
const MAX_FILE: u64 = 64 << 20;
/// Most font files [`font_files_under`] lists.
const MAX_LISTED_FILES: usize = 5000;

/// The font files under `dirs` (subfolders too, each folder once), sorted, at most a few thousand.
pub fn font_files_under(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        if out.len() >= MAX_LISTED_FILES || !seen.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if out.len() < MAX_LISTED_FILES
                && path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The folders of fonts belonging to Windows installations on this computer: on Linux and BSD a
/// Windows partition mounted under `/mnt`, `/media` or `/run/media` (a dual-boot PC), on macOS
/// one under `/Volumes`; on Windows itself, Microsoft 365's cloud fonts (already read in place).
pub fn windows_font_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let cloud = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft\\FontCache\\4\\CloudFonts"));
        return cloud.filter(|c| c.is_dir()).into_iter().collect();
    }
    let mut parents: Vec<PathBuf> =
        if cfg!(target_os = "macos") { vec!["/Volumes".into()] } else { vec!["/mnt".into(), "/media".into(), "/run/media".into()] };
    if let Some(u) = std::env::var_os("USER") {
        parents.extend([Path::new("/media").join(&u), Path::new("/run/media").join(&u)]);
    }
    windows_font_dirs_under(&parents)
}

/// [`windows_font_dirs`] for Windows partitions that are, or are mounted directly in, `parents`:
/// each gives `Windows/Fonts` and, per user, that user's fonts and Microsoft 365 cloud fonts.
pub fn windows_font_dirs_under(parents: &[PathBuf]) -> Vec<PathBuf> {
    fn add_partition(root: &Path, out: &mut Vec<PathBuf>) {
        let fonts = root.join("Windows").join("Fonts");
        if fonts.is_dir() {
            out.push(fonts);
        }
        let Ok(users) = std::fs::read_dir(root.join("Users")) else { return };
        let mut users: Vec<PathBuf> = users.flatten().map(|u| u.path()).collect();
        users.sort();
        for u in users {
            let local = u.join("AppData").join("Local").join("Microsoft");
            for sub in [local.join("Windows").join("Fonts"), local.join("FontCache").join("4").join("CloudFonts")] {
                if sub.is_dir() {
                    out.push(sub);
                }
            }
        }
    }
    let mut out = Vec::new();
    for parent in parents {
        add_partition(parent, &mut out);
        let Ok(rd) = std::fs::read_dir(parent) else { continue };
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for p in subs {
            add_partition(&p, &mut out);
        }
    }
    out.dedup();
    out
}

/// Where installed fonts are copied: `~/.local/share/fonts/Imported` on Linux and BSD,
/// `~/Library/Fonts` on macOS, the per-user Fonts folder on Windows.
pub fn user_font_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Fonts"));
    }
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home.map(|h| h.join(".local/share")));
    data.map(|d| d.join("fonts").join("Imported"))
}

/// What [`install_fonts_from`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstallReport {
    /// Families that weren't available and now are, in the order installed.
    pub added: Vec<String>,
    /// Matching files whose faces were all available already.
    pub already: usize,
    /// Files that couldn't be read or copied (at most a few).
    pub errors: Vec<String>,
    /// The folder they were copied to.
    pub to: Option<PathBuf>,
}

/// Install the font files under `dirs` (only those with a family in `only`, when given): each
/// file with a face that isn't available is copied into `dest` (the user font folder by default)
/// and loaded into the font database.
pub fn install_fonts_from(dirs: &[PathBuf], only: Option<&[String]>, dest: Option<PathBuf>) -> InstallReport {
    const MAX_ERRORS: usize = 10;
    let db = FontDb::global();
    let mut report = InstallReport { to: dest.or_else(user_font_dir), ..Default::default() };
    let Some(to) = report.to.clone() else {
        report.errors.push("no user font folder on this system".into());
        return report;
    };
    let note = |r: &mut InstallReport, e: String| {
        if r.errors.len() < MAX_ERRORS {
            r.errors.push(e);
        }
    };
    for path in font_files_under(dirs) {
        if std::fs::metadata(&path).map(|m| m.len() > MAX_FILE).unwrap_or(true) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            note(&mut report, format!("{}: can't be read", path.display()));
            continue;
        };
        let families = crate::fontdb::file_families(&data);
        if families.is_empty() || only.is_some_and(|o| !families.iter().any(|f| o.iter().any(|w| w.eq_ignore_ascii_case(f)))) {
            continue;
        }
        let new_families: Vec<String> = families.iter().filter(|f| !db.has_family(f)).cloned().collect();
        let faces = db.add_font(data.clone());
        if faces == 0 {
            report.already += 1;
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&to) {
            note(&mut report, format!("{}: {e}", to.display()));
        } else {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "font.ttf".into());
            let mut target = to.join(&name);
            let mut n = 2;
            while target.exists() && n < 1000 {
                target = to.join(format!("{n}-{name}"));
                n += 1;
            }
            if let Err(e) = std::fs::write(&target, &data) {
                note(&mut report, format!("{}: added for this session only: {e}", target.display()));
            }
        }
        for f in new_families {
            if !report.added.iter().any(|a| a.eq_ignore_ascii_case(&f)) {
                report.added.push(f);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dual-boot Windows partition mounted under a parent folder gives its Windows/Fonts and,
    /// per user, the Microsoft 365 cloud font folder; other folders give none.
    #[test]
    fn windows_font_folders_are_found_on_a_mounted_partition() {
        let root = std::env::temp_dir().join(format!("dc-winfonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let part = root.join("C_drive");
        let cloud = part.join("Users/Ana/AppData/Local/Microsoft/FontCache/4/CloudFonts/Aptos");
        for d in [part.join("Windows/Fonts"), cloud.clone(), part.join("Users/Ben/Documents"), root.join("data/Music")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(cloud.join("123.ttf"), b"x").unwrap();
        std::fs::write(cloud.join("notes.txt"), b"x").unwrap();
        let dirs = windows_font_dirs_under(std::slice::from_ref(&root));
        assert_eq!(dirs, vec![part.join("Windows/Fonts"), part.join("Users/Ana/AppData/Local/Microsoft/FontCache/4/CloudFonts")]);
        assert_eq!(font_files_under(&dirs), vec![cloud.join("123.ttf")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
