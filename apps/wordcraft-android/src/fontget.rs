//! Downloads of the fonts a document needs, for the font bar ([`crate::fontbar`]):
//! - **Microsoft**: the Aptos family (Microsoft's own download, a zip) and the Core fonts for the
//!   Web (Arial, Times New Roman, Courier New, Georgia, Verdana…, each a cabinet inside its original
//!   installer, from the corefonts mirror on SourceForge, as Linux distributions fetch them). Both
//!   are free downloads under Microsoft's licence: for the user's own devices, never passed on.
//! - **Google Fonts**: any family it has (SIL Open Font Licence or Apache 2.0), and for Microsoft
//!   fonts that have no free download the open fonts with the same widths (Carlito for Calibri,
//!   Caladea for Cambria), so lines and pages break as they do in Word.
//!
//! Each download goes to a new folder of font files, which the font bar installs on this device.
//! Nothing is sent but the request for the font.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::time::Duration;

/// Where Microsoft's free Aptos download is (Microsoft Download Center, "Microsoft Aptos Fonts").
const APTOS_ZIP: &str = "https://download.microsoft.com/download/8/6/0/860a94fa-7feb-44ef-ac79-c072d9113d69/Microsoft%20Aptos%20Fonts.zip";
/// Microsoft's page for it, with its terms.
pub const APTOS_PAGE: &str = "https://www.microsoft.com/download/details.aspx?id=106087";
/// The Core fonts for the Web, in their original installers.
const CORE_FONTS: &str = "https://downloads.sourceforge.net/corefonts/";

/// The largest download read (the Aptos zip is 3 MB, a font file rarely 20).
const MAX_DOWNLOAD: u64 = 32 << 20;

/// Where a font can be downloaded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Microsoft's Aptos download (every Aptos family).
    Aptos,
    /// One of the Core fonts for the Web: its installer's file name.
    Core(&'static str),
    /// Google Fonts: the family to get, and whether it only has the same widths (a stand-in).
    Google { family: String, twin: bool },
}

impl Source {
    /// Who it comes from, for the button.
    pub fn vendor(&self) -> &'static str {
        match self {
            Source::Aptos | Source::Core(_) => "Microsoft",
            Source::Google { .. } => "Google Fonts",
        }
    }
}

/// The Core fonts for the Web: family, installer.
const CORE: &[(&str, &str)] = &[
    ("andale mono", "andale32.exe"),
    ("arial", "arial32.exe"),
    ("arial black", "arialb32.exe"),
    ("comic sans ms", "comic32.exe"),
    ("courier new", "courie32.exe"),
    ("georgia", "georgi32.exe"),
    ("impact", "impact32.exe"),
    ("times new roman", "times32.exe"),
    ("trebuchet ms", "trebuc32.exe"),
    ("verdana", "verdan32.exe"),
    ("webdings", "webdin32.exe"),
];

/// Open fonts with the same widths as a Microsoft font, on Google Fonts.
const TWINS: &[(&str, &str)] = &[
    ("calibri", "Carlito"),
    ("cambria", "Caladea"),
    ("arial", "Arimo"),
    ("times new roman", "Tinos"),
    ("courier new", "Cousine"),
    ("georgia", "Gelasio"),
];

/// Microsoft fonts no one may download for free (they come with Windows and Office): only a
/// stand-in from Google Fonts, when there is one.
const NOT_FREE: &[&str] = &[
    "symbol",
    "wingdings",
    "webdings",
    "marlett",
    "mt extra",
    "calibri",
    "cambria",
    "candara",
    "consolas",
    "constantia",
    "corbel",
    "segoe",
    "tahoma",
    "century gothic",
    "franklin gothic",
    "garamond",
    "book antiqua",
    "bookman old style",
];

/// Where `name` can be downloaded from, best first.
pub fn sources(name: &str) -> Vec<Source> {
    let n = name.trim().to_lowercase();
    let mut out = Vec::new();
    if n.starts_with("aptos") {
        out.push(Source::Aptos);
    }
    if let Some((_, exe)) = CORE.iter().find(|(f, _)| *f == n) {
        out.push(Source::Core(exe));
    }
    if let Some((_, twin)) = TWINS.iter().find(|(f, _)| *f == n) {
        out.push(Source::Google { family: (*twin).to_string(), twin: true });
    } else if !n.starts_with("aptos") && !NOT_FREE.iter().any(|f| n.starts_with(f)) && google_name_ok(name) {
        out.push(Source::Google { family: name.trim().to_string(), twin: false });
    }
    out
}

/// Downloads `source` into a new folder of font files.
pub fn download(source: &Source) -> Result<PathBuf, String> {
    match source {
        Source::Aptos => aptos(),
        Source::Core(exe) => core_font(exe),
        Source::Google { family, .. } => google(family),
    }
}

fn new_dir(what: &str) -> Result<PathBuf, String> {
    let tag: String = what.chars().filter(char::is_ascii_alphanumeric).collect();
    let dir = std::env::temp_dir().join(format!("worksuite-font-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for the download: {e}"))?;
    Ok(dir)
}

fn get(url: &str, from: &str) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(url).call().map_err(|e| format!("Couldn't reach {from}: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err(format!("{from} answered {}.", resp.status()));
    }
    resp.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| format!("The download from {from} stopped: {e}"))
}

/// Microsoft's Aptos fonts, from their zip.
fn aptos() -> Result<PathBuf, String> {
    let bytes = get(APTOS_ZIP, "Microsoft")?;
    let dir = new_dir("aptos")?;
    let n = unzip_fonts(&bytes, &dir)?;
    if n == 0 {
        return Err("Microsoft's Aptos download had no fonts in it.".into());
    }
    Ok(dir)
}

/// The font files in a zip, written to `dir`; how many.
fn unzip_fonts(bytes: &[u8], dir: &std::path::Path) -> Result<usize, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Couldn't read the download: {e}"))?;
    let mut n = 0;
    for i in 0..zip.len() {
        let Ok(mut f) = zip.by_index(i) else { continue };
        let Some(name) = font_file_name(f.name()) else { continue };
        if f.size() > MAX_DOWNLOAD {
            continue;
        }
        let mut data = Vec::new();
        if f.read_to_end(&mut data).is_ok() {
            std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// One of the Core fonts for the Web: its installer holds a cabinet with the font files.
fn core_font(exe: &str) -> Result<PathBuf, String> {
    let bytes = get(&format!("{CORE_FONTS}{exe}"), "Microsoft's Core fonts (SourceForge)")?;
    let dir = new_dir(exe)?;
    let n = uncab_fonts(&bytes, &dir)?;
    if n == 0 {
        return Err(format!("{exe} had no fonts in it."));
    }
    Ok(dir)
}

/// The font files of the cabinet inside a self-extracting installer, written to `dir`; how many.
fn uncab_fonts(bytes: &[u8], dir: &std::path::Path) -> Result<usize, String> {
    // The installer's code may hold "MSCF" too: the cabinet starts where a whole header does
    // (its reserved fields zero, version 1.3).
    let at = (0..bytes.len().saturating_sub(36))
        .find(|&i| {
            let h = |a: usize, b: usize| bytes.get(i + a..i + b);
            h(0, 4) == Some(b"MSCF") && h(4, 8) == Some(&[0; 4]) && h(12, 16) == Some(&[0; 4]) && h(24, 26) == Some(&[3, 1])
        })
        .ok_or("The download isn't a font installer.")?;
    let data = bytes.get(at..).unwrap_or_default().to_vec();
    let mut cab = cab::Cabinet::new(Cursor::new(data)).map_err(|e| format!("Couldn't read the installer: {e}"))?;
    let names: Vec<String> = cab.folder_entries().flat_map(|f| f.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>()).collect();
    let mut n = 0;
    for name in names {
        let Some(out) = font_file_name(&name) else { continue };
        let mut data = Vec::new();
        let read = cab.read_file(&name).and_then(|r| r.take(MAX_DOWNLOAD).read_to_end(&mut data));
        if read.is_ok() {
            std::fs::write(dir.join(out), data).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// The file name to store a font as (`None` for anything that isn't a font file): no folders.
fn font_file_name(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?.trim();
    let lower = name.to_lowercase();
    let ok = [".ttf", ".otf", ".ttc"].iter().any(|e| lower.ends_with(e)) && !name.starts_with('.') && name.len() <= 128;
    ok.then(|| name.to_string())
}

fn google_name_ok(family: &str) -> bool {
    let f = family.trim();
    !f.is_empty() && f.len() <= 100 && f.chars().all(|c| c.is_alphanumeric() || " -_".contains(c))
}

/// A family from Google Fonts (regular, bold, italic and bold italic when it has them).
fn google(family: &str) -> Result<PathBuf, String> {
    let fam = family.trim();
    if !google_name_ok(fam) {
        return Err(format!("“{family}” isn't a name Google Fonts uses."));
    }
    let q = fam.replace(' ', "+");
    let mut css = None;
    for variant in [format!("{q}:ital,wght@0,400;0,700;1,400;1,700"), format!("{q}:wght@400;700"), q.clone()] {
        let url = format!("https://fonts.googleapis.com/css2?family={variant}");
        let mut resp = agent().get(&url).call().map_err(|e| format!("Couldn't reach Google Fonts: {e}"))?;
        match resp.status().as_u16() {
            200 => {
                css = Some(resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| e.to_string())?);
                break;
            }
            400 | 404 => continue,
            s => return Err(format!("Google Fonts answered {s}.")),
        }
    }
    let Some(css) = css else {
        return Err(format!("“{fam}” isn't on Google Fonts. Add its font file, or choose a similar font."));
    };
    let urls = google_font_urls(&css);
    if urls.is_empty() {
        return Err(format!("Google Fonts had no font files for “{fam}”."));
    }
    let dir = new_dir(fam)?;
    let tag = fam.replace(' ', "");
    for (url, weight, italic) in urls {
        let ext = if url.ends_with(".otf") { "otf" } else { "ttf" };
        let bytes = get(&url, "Google Fonts")?;
        std::fs::write(dir.join(format!("{tag}-{weight}{}.{ext}", if italic { "Italic" } else { "" })), bytes).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// The font files a Google Fonts stylesheet names: (url, weight, italic), each once.
fn google_font_urls(css: &str) -> Vec<(String, String, bool)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for face in css.split("@font-face").skip(1) {
        let Some(url) = face.split("url(").nth(1).and_then(|s| s.split(')').next()) else { continue };
        let url = url.trim_matches(['\'', '"']);
        if !url.starts_with("https://fonts.gstatic.com/") || !(url.ends_with(".ttf") || url.ends_with(".otf")) || !seen.insert(url.to_string()) {
            continue;
        }
        let weight: String = face
            .split("font-weight:")
            .nth(1)
            .and_then(|s| s.split(';').next())
            .unwrap_or("400")
            .trim()
            .chars()
            .filter(char::is_ascii_digit)
            .take(4)
            .collect();
        out.push((url.to_string(), if weight.is_empty() { "400".into() } else { weight }, face.contains("font-style: italic")));
    }
    out
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::WebPki).build();
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .user_agent(concat!("WorkSuite/", env!("CARGO_PKG_VERSION")))
        .tls_config(tls)
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microsoft_fonts_come_from_microsoft_and_their_twins_from_google() {
        assert_eq!(sources("Aptos"), [Source::Aptos]);
        assert_eq!(sources("Aptos Display"), [Source::Aptos]);
        assert_eq!(sources("Arial"), [Source::Core("arial32.exe"), Source::Google { family: "Arimo".into(), twin: true }]);
        assert_eq!(sources("times new roman")[0], Source::Core("times32.exe"));
        assert_eq!(sources("Calibri"), [Source::Google { family: "Carlito".into(), twin: true }]);
        assert!(sources("Consolas").is_empty(), "no free download, no stand-in");
        assert!(sources("Symbol").is_empty() && sources("Wingdings 2").is_empty(), "not on Google Fonts");
        assert_eq!(sources("Webdings"), [Source::Core("webdin32.exe")]);
        assert_eq!(sources("Montserrat"), [Source::Google { family: "Montserrat".into(), twin: false }]);
        assert!(sources("../etc").is_empty());
        assert_eq!(sources("Aptos")[0].vendor(), "Microsoft");
    }

    #[test]
    fn only_font_files_are_kept_without_their_folders() {
        assert_eq!(font_file_name("Aptos-Bold.ttf").as_deref(), Some("Aptos-Bold.ttf"));
        assert_eq!(font_file_name("fonts/../Arial.TTF").as_deref(), Some("Arial.TTF"));
        assert_eq!(font_file_name("C:\\x\\times.ttf").as_deref(), Some("times.ttf"));
        assert_eq!(font_file_name("EULA.rtf"), None);
        assert_eq!(font_file_name(".hidden.ttf"), None);
    }

    #[test]
    fn a_zip_gives_its_font_files() {
        use std::io::Write;
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Aptos.ttf", o).unwrap();
            z.write_all(b"font").unwrap();
            z.start_file("Microsoft Aptos Fonts EULA.rtf", o).unwrap();
            z.write_all(b"terms").unwrap();
            z.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("worksuite-unzip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unzip_fonts(buf.get_ref(), &dir).unwrap(), 1);
        assert_eq!(std::fs::read(dir.join("Aptos.ttf")).unwrap(), b"font");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(uncab_fonts(b"not an installer", &dir).is_err());
    }

    #[test]
    fn google_stylesheets_name_their_files() {
        let css = "@font-face {\n  font-family: 'Raleway';\n  font-style: italic;\n  font-weight: 400;\n  src: url(https://fonts.gstatic.com/s/raleway/v1/a.ttf) format('truetype');\n}\n\
                   @font-face {\n  font-style: normal;\n  font-weight: 700;\n  src: url(https://fonts.gstatic.com/s/raleway/v1/b.ttf) format('truetype');\n}\n\
                   @font-face {\n  src: url(https://evil.example/c.ttf);\n}";
        let urls = google_font_urls(css);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], ("https://fonts.gstatic.com/s/raleway/v1/a.ttf".into(), "400".into(), true));
        assert_eq!(urls[1].1, "700");
        assert!(google("../etc").is_err());
    }

    /// Downloads for real (needs the network): `cargo test -p wordcraft-android -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_from_microsoft_and_google() {
        for s in [Source::Aptos, Source::Core("arial32.exe"), Source::Google { family: "Carlito".into(), twin: true }] {
            let dir = download(&s).unwrap_or_else(|e| panic!("{s:?}: {e}"));
            let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
            assert!(files.iter().any(|f| f.to_lowercase().ends_with(".ttf")), "{s:?}: {files:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
