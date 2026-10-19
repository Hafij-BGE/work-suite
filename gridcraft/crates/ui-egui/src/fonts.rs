//! The fonts installed on the machine, so the font list can offer them and cells can render with
//! them (#4).
//!
//! egui has no font discovery of its own and panics when asked for a family that was never bound
//! to font data. So we do both halves: scan the platform's font folders for family names (a read
//! of each font file's `name` table — damaged or unreadable files yield nothing, never a panic),
//! and bind the files a workbook actually uses into the families
//! [`theme::font_definitions`](crate::theme::font_definitions) assembles.
//!
//! Native only: on wasm there are no system fonts, every function here reports "nothing", and the
//! font list is the standard [`crate::ribbon::FONTS`] set.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A font file is skipped by the scan past this size: a catalog only needs each face's name, and a
/// damaged or enormous file must not turn startup into a long read.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FONT_FILE: u64 = 64 << 20;

/// One face of one installed font file.
#[derive(Clone, Debug)]
pub struct Face {
    /// Typographic family name, e.g. `"Segoe UI"`.
    pub family: String,
    /// The file the face lives in.
    pub path: PathBuf,
    /// Face index within the file (collections hold several, e.g. a family's Regular and Bold).
    pub index: u32,
    /// Weight implied by the face's style name (400 = regular).
    pub weight: f32,
    /// Whether the style name reads as italic or oblique.
    pub italic: bool,
}

/// The font folders of the platform (the system's and the user's). Empty on wasm.
pub fn system_font_dirs() -> Vec<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        Vec::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dirs: Vec<PathBuf> = Vec::new();
        // Extra folders first (a path list, like PATH): fonts that aren't installed, such as a
        // Windows partition's fonts when comparing with Excel on another system.
        if let Some(extra) = std::env::var_os("CRAFT_EXTRA_FONT_DIRS") {
            dirs.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        if cfg!(target_os = "macos") {
            dirs.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join("Library/Fonts"));
            }
        } else if cfg!(windows) {
            let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
            dirs.push(root.join("Fonts"));
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                let l = PathBuf::from(l);
                dirs.push(l.join("Microsoft\\Windows\\Fonts"));
                // Microsoft 365's cloud fonts (Aptos and others), downloaded on demand.
                dirs.push(l.join("Microsoft\\FontCache\\4\\CloudFonts"));
            }
        } else {
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join(".fonts"));
                dirs.push(h.join(".local/share/fonts"));
            }
        }
        dirs
    }
}

/// Weight implied by a style name (`"Bold"` → 700, `"Light"` → 300).
#[cfg(not(target_arch = "wasm32"))]
fn style_weight(style: &str) -> f32 {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
    const TABLE: &[(&str, f32)] = &[
        ("hairline", 100.0),
        ("thin", 100.0),
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("demibold", 600.0),
        ("semibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

/// Whether a style name reads as italic or oblique.
#[cfg(not(target_arch = "wasm32"))]
fn style_italic(style: &str) -> bool {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
    s.contains("italic") || s.contains("oblique")
}

/// Every face in `data` as `(index, family, style)`. Damaged data yields nothing, never a panic.
#[cfg(not(target_arch = "wasm32"))]
fn file_faces(data: &[u8]) -> Vec<(u32, String, String)> {
    use skrifa::raw::FileRef;
    use skrifa::{MetadataProvider, string::StringId};
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len(),
        Err(_) => return Vec::new(),
    };
    let name = |f: &skrifa::FontRef<'_>, ids: &[StringId]| -> Option<String> {
        ids.iter().find_map(|id| f.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
    };
    (0..count)
        .filter_map(|i| {
            let f = skrifa::FontRef::from_index(data, i).ok()?;
            let family = name(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME])?;
            let style = name(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
            Some((i, family, style))
        })
        .collect()
}

/// Catalog every face under `dirs`, recursing into subfolders. A folder reached twice (through a
/// symlink) is read once, so a link back to a parent ends the walk.
#[cfg(not(target_arch = "wasm32"))]
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<Face> {
    let mut out: Vec<Face> = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    while let Some(dir) = stack.pop() {
        let key = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen_dirs.insert(key) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_font =
                path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"));
            if !is_font || std::fs::metadata(&path).map(|m| m.len() > MAX_FONT_FILE).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            for (index, family, style) in file_faces(&data) {
                out.push(Face { family, path: path.clone(), index, weight: style_weight(&style), italic: style_italic(&style) });
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub fn scan_dirs(_dirs: &[PathBuf]) -> Vec<Face> {
    Vec::new()
}

static CATALOG: OnceLock<Vec<Face>> = OnceLock::new();

/// The catalog of installed faces, scanned on first use. Blocks until the scan finishes.
pub fn catalog() -> &'static [Face] {
    CATALOG.get_or_init(|| scan_dirs(&system_font_dirs()))
}

/// Faces of fonts installed while the app runs ([`install_fonts_from`]), on top of the catalog.
static ADDED: std::sync::RwLock<Vec<&'static Face>> = std::sync::RwLock::new(Vec::new());

/// The catalog (with the fonts installed since) if the scan has finished, else `None` — never
/// blocks, for use while painting.
fn catalog_now() -> Option<Vec<&'static Face>> {
    let cat = CATALOG.get()?;
    let added = ADDED.read().unwrap_or_else(|e| e.into_inner());
    Some(cat.iter().chain(added.iter().copied()).collect())
}

/// Start cataloging the installed fonts on a background thread, so the first frame doesn't wait for
/// it. A no-op on wasm and after the scan has started.
pub fn start_scan() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        if CATALOG.get().is_none() {
            // A failed spawn leaves the scan to the first blocking lookup.
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(|| {
                catalog();
            });
        }
    });
}

/// Whether the background scan has finished.
pub fn scan_done() -> bool {
    CATALOG.get().is_some()
}

/// The faces of one installed family, regular-weight first (the order roles pick from).
pub fn faces_of(family: &str) -> Vec<Face> {
    let Some(cat) = catalog_now() else { return Vec::new() };
    let mut v: Vec<Face> = cat.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).map(|f| (*f).clone()).collect();
    v.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)).then(a.index.cmp(&b.index)));
    v
}

/// The subset of `names` that is installed, sorted and deduped. These are the families whose files
/// get bound for a workbook.
pub fn installed_among(names: &[String]) -> Vec<String> {
    let Some(cat) = catalog_now() else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for wanted in names {
        if !seen.insert(wanted.to_lowercase()) {
            continue;
        }
        if let Some(f) = cat.iter().find(|f| f.family.eq_ignore_ascii_case(wanted)) {
            out.push(f.family.clone());
        }
    }
    out.sort_by_key(|a| a.to_lowercase());
    out
}

/// The subset of `names` that isn't installed, each once in first-seen order: the fonts a workbook
/// names that are drawn with a substitute. Empty until the background scan has finished (nothing
/// is known to be missing yet). Theme placeholders (`+mn-lt`) and blank names are skipped.
pub fn missing_among(names: &[String]) -> Vec<String> {
    let Some(cat) = catalog_now() else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for wanted in names {
        let w = wanted.trim();
        if w.is_empty() || w.starts_with('+') || !seen.insert(w.to_lowercase()) {
            continue;
        }
        if !cat.iter().any(|f| f.family.eq_ignore_ascii_case(w)) {
            out.push(w.to_string());
        }
    }
    out
}

/// Most font files [`font_files_under`] lists.
#[cfg(not(target_arch = "wasm32"))]
const MAX_LISTED_FILES: usize = 5000;

/// The font files under `dirs` (subfolders too, each folder once), sorted, at most a few thousand.
#[cfg(not(target_arch = "wasm32"))]
pub fn font_files_under(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    while let Some(dir) = stack.pop() {
        if out.len() >= MAX_LISTED_FILES || !seen_dirs.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
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
/// Per partition: `Windows/Fonts` and, per user, that user's fonts and Microsoft 365 cloud fonts
/// (Aptos…). Nothing is copied by looking.
#[cfg(not(target_arch = "wasm32"))]
pub fn windows_font_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let cloud = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft\\FontCache\\4\\CloudFonts"));
        return cloud.filter(|c| c.is_dir()).into_iter().collect();
    }
    let mut parents: Vec<PathBuf> =
        if cfg!(target_os = "macos") { vec!["/Volumes".into()] } else { vec!["/mnt".into(), "/media".into(), "/run/media".into()] };
    if let Some(u) = std::env::var_os("USER") {
        parents.extend([std::path::Path::new("/media").join(&u), std::path::Path::new("/run/media").join(&u)]);
    }
    windows_font_dirs_under(&parents)
}

/// [`windows_font_dirs`] for Windows partitions that are, or are mounted directly in, `parents`.
#[cfg(not(target_arch = "wasm32"))]
pub fn windows_font_dirs_under(parents: &[PathBuf]) -> Vec<PathBuf> {
    fn add_partition(root: &std::path::Path, out: &mut Vec<PathBuf>) {
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

/// Where fonts installed from the app are copied: the user's own font folder, which every app of
/// the suite (and other programs) reads — `~/.local/share/fonts/Imported` on Linux and BSD,
/// `~/Library/Fonts` on macOS, the per-user Fonts folder on Windows.
#[cfg(not(target_arch = "wasm32"))]
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
    /// Families now available that weren't, in the order installed.
    pub added: Vec<String>,
    /// Matching files whose faces were all available already.
    pub already: usize,
    /// Files that couldn't be read or copied (at most a few).
    pub errors: Vec<String>,
    /// The folder they were copied to.
    pub to: Option<PathBuf>,
}

/// Install the font files under `dirs` (only those with a family in `only`, when given) into
/// `dest` (the user font folder by default): each new file is copied there and its faces join the
/// catalog at once, so cells can use them without restarting. Files whose faces are all available
/// are left alone. Fonts are copied on this computer only, never anywhere else.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_fonts_from(dirs: &[PathBuf], only: Option<&[String]>, dest: Option<PathBuf>) -> InstallReport {
    const MAX_ERRORS: usize = 10;
    let mut report = InstallReport { to: dest.or_else(user_font_dir), ..Default::default() };
    let Some(to) = report.to.clone() else {
        report.errors.push("no user font folder on this system".into());
        return report;
    };
    let err = |r: &mut InstallReport, e: String| {
        if r.errors.len() < MAX_ERRORS {
            r.errors.push(e);
        }
    };
    for path in font_files_under(dirs) {
        if std::fs::metadata(&path).map(|m| m.len() > MAX_FONT_FILE).unwrap_or(true) {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            err(&mut report, format!("{}: can't be read", path.display()));
            continue;
        };
        let faces = file_faces(&data);
        if faces.is_empty() || only.is_some_and(|o| !faces.iter().any(|(_, fam, _)| o.iter().any(|w| w.eq_ignore_ascii_case(fam)))) {
            continue;
        }
        let have = catalog_now().unwrap_or_default();
        let new = |fam: &str, style: &str| {
            !have.iter().any(|f| f.family.eq_ignore_ascii_case(fam) && f.weight == style_weight(style) && f.italic == style_italic(style))
        };
        if !faces.iter().any(|(_, fam, style)| new(fam, style)) {
            report.already += 1;
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&to) {
            err(&mut report, format!("{}: {e}", to.display()));
            break;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "font.ttf".into());
        let mut target = to.join(&name);
        let mut n = 2;
        while target.exists() && n < 1000 {
            target = to.join(format!("{n}-{name}"));
            n += 1;
        }
        if let Err(e) = std::fs::write(&target, &data) {
            err(&mut report, format!("{}: {e}", target.display()));
            continue;
        }
        let mut added = ADDED.write().unwrap_or_else(|e| e.into_inner());
        for (index, family, style) in faces {
            if !report.added.iter().any(|a| a.eq_ignore_ascii_case(&family)) && !have.iter().any(|f| f.family.eq_ignore_ascii_case(&family)) {
                report.added.push(family.clone());
            }
            // Leaked: installed fonts are few and live as long as the app.
            added.push(Box::leak(Box::new(Face { family, path: target.clone(), index, weight: style_weight(&style), italic: style_italic(&style) })));
        }
    }
    report
}

/// The font list a combo box shows: the standard [`crate::ribbon::FONTS`] names first (what Excel
/// offers, and what we substitute), then every other installed family A–Z. Never blocks: until the
/// background scan finishes it returns just the standard names.
pub fn families() -> Vec<String> {
    let mut out: Vec<String> = crate::ribbon::FONTS.iter().map(|s| s.to_string()).collect();
    let Some(cat) = catalog_now() else { return out };
    let mut seen: HashSet<String> = out.iter().map(|s| s.to_lowercase()).collect();
    let mut installed: Vec<String> = cat.iter().map(|f| f.family.clone()).collect();
    installed.sort_by_key(|a| a.to_lowercase());
    installed.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    for f in installed {
        if seen.insert(f.to_lowercase()) {
            out.push(f);
        }
    }
    out
}

/// The egui family name for one role of one family: `fnt:{family}:{b|n}{i|n}`.
pub fn role_key(family: &str, bold: bool, italic: bool) -> String {
    format!("fnt:{family}:{}{}", if bold { 'b' } else { 'n' }, if italic { 'i' } else { 'n' })
}

/// The face to use for a role, or `None` when the family has no faces. Missing bold/italic faces
/// fall back to the family's regular face, so every bound role has data behind it.
fn pick_face(faces: &[Face], bold: bool, italic: bool) -> Option<&Face> {
    let regular = || faces.iter().find(|f| !f.italic && (f.weight - 400.0).abs() < 1.0);
    let exact = |wanted_bold: bool, wanted_italic: bool| {
        faces.iter().find(|f| f.italic == wanted_italic && if wanted_bold { f.weight >= 600.0 } else { f.weight < 600.0 })
    };
    (match (bold, italic) {
        (true, true) => exact(true, true).or_else(|| exact(true, false)).or_else(|| exact(false, true)),
        (true, false) => exact(true, false),
        (false, true) => exact(false, true),
        (false, false) => None,
    })
    .or_else(regular)
    .or_else(|| faces.first())
}

/// A stable key for a face's font data, unique per file/index within the process.
fn face_key(family: &str, face: &Face) -> String {
    let file = face.path.file_name().and_then(|s| s.to_str()).unwrap_or("font");
    format!("fnt-src:{family}:{file}:{}", face.index)
}

/// Bind one installed family's faces into `fonts`, one egui family per cell role, and return the
/// role keys that are now bound. A role with no exact face chains to the family's regular face, so
/// no role is ever left pointing at missing data (egui panics on that). A family with no faces
/// binds nothing.
///
/// Each role's chain ends with egui's default proportional fonts, so glyphs the installed font
/// lacks (emoji, a stray script) still render.
pub fn bind_faces(fonts: &mut egui::FontDefinitions, family: &str, faces: &[Face]) -> Vec<String> {
    if faces.is_empty() {
        return Vec::new();
    }
    let base: Vec<String> = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let mut bound: Vec<String> = Vec::new();
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let Some(primary) = pick_face(faces, bold, italic) else { continue };
        let mut chain: Vec<String> = Vec::new();
        for face in [Some(primary), pick_face(faces, false, false)] {
            let Some(face) = face else { continue };
            let key = face_key(family, face);
            if chain.contains(&key) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&face.path) else { continue };
            let mut fd = egui::FontData::from_owned(bytes);
            fd.index = face.index;
            fonts.font_data.entry(key.clone()).or_insert_with(|| std::sync::Arc::new(fd));
            chain.push(key);
        }
        if chain.is_empty() {
            continue;
        }
        chain.extend(base.iter().cloned());
        let role = role_key(family, bold, italic);
        fonts.families.insert(egui::FontFamily::Name(role.clone().into()), chain);
        bound.push(role);
    }
    bound
}

/// Bind every installed face of `family` (see [`bind_faces`]).
pub fn bind_family(fonts: &mut egui::FontDefinitions, family: &str) -> Vec<String> {
    bind_faces(fonts, family, &faces_of(family))
}

/// The role families currently bound to font data. `cell_family` consults this, so it never names a
/// family egui hasn't been given.
static BOUND: OnceLock<std::sync::RwLock<HashSet<String>>> = OnceLock::new();

fn bound() -> &'static std::sync::RwLock<HashSet<String>> {
    BOUND.get_or_init(|| std::sync::RwLock::new(HashSet::new()))
}

/// Record the role families a freshly built [`egui::FontDefinitions`] bound.
pub fn set_bound(keys: impl IntoIterator<Item = String>) {
    *bound().write().unwrap_or_else(|e| e.into_inner()) = keys.into_iter().collect();
}

/// Is this exact role family bound to font data?
pub fn is_bound(role: &str) -> bool {
    bound().read().unwrap_or_else(|e| e.into_inner()).contains(role)
}

/// Serializes tests that mutate the process-wide bound-family registry.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_names_map_to_weight_and_slant() {
        assert_eq!(style_weight("Bold"), 700.0);
        assert_eq!(style_weight("SemiBold Italic"), 600.0);
        assert_eq!(style_weight("Regular"), 400.0);
        assert_eq!(style_weight("Light Oblique"), 300.0);
        assert!(style_italic("Bold Italic") && style_italic("Oblique"));
        assert!(!style_italic("Bold"));
        // The weight table is checked in order, so "extralight" wins over "light".
        assert_eq!(style_weight("ExtraLight"), 200.0);
    }

    #[test]
    fn role_keys_are_stable_and_distinct() {
        assert_eq!(role_key("Segoe UI", false, false), "fnt:Segoe UI:nn");
        assert_eq!(role_key("Segoe UI", true, false), "fnt:Segoe UI:bn");
        assert_eq!(role_key("Segoe UI", false, true), "fnt:Segoe UI:ni");
        assert_eq!(role_key("Segoe UI", true, true), "fnt:Segoe UI:bi");
    }

    #[test]
    fn nothing_is_bound_until_font_definitions_says_so() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A role key is only safe to name after font_definitions bound it.
        let k = role_key("Some Installed Font", true, false);
        assert!(!is_bound(&k));
        set_bound([k.clone()]);
        assert!(is_bound(&k));
        set_bound([]);
        assert!(!is_bound(&k));
    }

    #[test]
    fn damaged_or_non_font_files_contribute_nothing() {
        assert!(file_faces(b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").is_empty());
        assert!(file_faces(b"not a font at all").is_empty());
        assert!(file_faces(&[]).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_scan_catalogs_fonts_and_reads_each_folder_once() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::fs::write(dir.join("damaged.ttf"), b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").expect("write");
        std::fs::write(dir.join("readme.txt"), b"not a font").expect("write");
        // The same folder listed twice (once through `Sub/..`) is read once.
        let faces = scan_dirs(&[dir.clone(), dir.join("Sub/..")]);
        let families: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(families, ["Ubuntu"], "one face, family read from the name table");
        assert_eq!(faces[0].weight, 300.0, "“Light” reads as weight 300");
        assert!(!faces[0].italic);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_back_to_a_parent_folder_ends_the_scan() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Sub/Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::os::unix::fs::symlink(&dir, dir.join("Sub/loop")).expect("symlink");
        assert_eq!(scan_dirs(std::slice::from_ref(&dir)).len(), 1, "the loop doesn't multiply faces");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn binding_a_family_binds_all_four_roles_and_never_stores_missing_data() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("Ubuntu-Light.ttf");
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        let face = Face { family: "Ubuntu".into(), path, index: 0, weight: 300.0, italic: false };

        let mut fonts = egui::FontDefinitions::default();
        let bound = bind_faces(&mut fonts, "Ubuntu", std::slice::from_ref(&face));
        assert_eq!(bound.len(), 4, "regular, bold, italic and bold-italic roles");
        for key in &bound {
            let chain = fonts.families.get(&egui::FontFamily::Name(key.as_str().into())).expect("role family");
            assert!(chain.iter().all(|k| fonts.font_data.contains_key(k)), "every chained key has data");
        }
        // A family with no faces binds nothing and never touches the map.
        assert!(bind_faces(&mut fonts, "No Such Font", &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dual-boot Windows partition mounted under a parent folder gives its Windows/Fonts and,
    /// per user, the Microsoft 365 cloud font folder; other folders give none.
    #[test]
    fn windows_font_folders_are_found_on_a_mounted_partition() {
        let root = std::env::temp_dir().join(format!("gc-winfonts-{}", std::process::id()));
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

    /// A font installed from a folder is copied to the destination and is installed at once (no
    /// longer missing); installing it again copies nothing.
    #[test]
    fn installing_a_font_copies_it_and_it_is_no_longer_missing() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Any installed TrueType font, renamed to a family no system has.
        let Some((data, family)) = font_files_under(&system_font_dirs())
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")))
            .find_map(|p| {
                let data = std::fs::read(&p).ok()?;
                let faces = file_faces(&data);
                (faces.len() == 1).then(|| (data, faces[0].1.clone()))
            })
        else {
            eprintln!("skipped: no TrueType font installed");
            return;
        };
        let unique: String =
            family.chars().enumerate().map(|(i, c)| if c.is_ascii_alphabetic() { if i % 2 == 0 { 'Q' } else { 'z' } } else { c }).collect();
        let mut renamed = data.clone();
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
        for (from, to) in [(utf16(&family), utf16(&unique)), (family.as_bytes().to_vec(), unique.as_bytes().to_vec())] {
            if from.len() != to.len() || from.is_empty() {
                continue;
            }
            let mut i = 0;
            while let Some(at) = renamed[i..].windows(from.len()).position(|w| w == from.as_slice()) {
                renamed[i + at..i + at + to.len()].copy_from_slice(&to);
                i += at + to.len();
            }
        }
        if file_faces(&renamed).first().map(|f| f.1.as_str()) != Some(unique.as_str()) {
            eprintln!("skipped: couldn't rename {family}");
            return;
        }
        let root = std::env::temp_dir().join(format!("gc-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dest) = (root.join("src/sub"), root.join("dest"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("face.ttf"), &renamed).unwrap();
        catalog();
        let wanted = vec![unique.clone()];
        assert_eq!(missing_among(&wanted), wanted);
        let r = install_fonts_from(&[root.join("src")], Some(&wanted), Some(dest.clone()));
        assert_eq!(r.added, wanted, "{r:?}");
        assert!(dest.join("face.ttf").is_file());
        assert!(missing_among(&wanted).is_empty());
        assert_eq!(installed_among(&wanted), wanted);
        let again = install_fonts_from(&[root.join("src")], Some(&wanted), Some(dest.clone()));
        assert_eq!((again.added.len(), again.already), (0, 1));
        let _ = std::fs::remove_dir_all(&root);
    }
}
