//! The font bar: over Writer, Slides or Sheets when the open document uses fonts that aren't on
//! this device, a red bar says so in plain sight ("“Aptos” is not available on this device. Writer
//! will use a substitute font (Source Sans 3).") and offers to fix it, in a card:
//! - **Download from Microsoft**: Aptos, and the Core fonts for the Web (Arial, Times New Roman…),
//!   from Microsoft's own free downloads ([`crate::fontget`]);
//! - **Download from Google Fonts**: the family, or for Calibri and Cambria the open fonts with the
//!   same widths (Carlito, Caladea);
//! - **Add a font file…**: a font file picked in Android's file picker;
//! - **Choose a similar font**: an installed font wherever the document uses the missing one (one
//!   step Undo takes back);
//! - **Don't show this again** (the home screen turns it back on).
//!
//! A font installed from any app is installed in all three, and kept for the next start. Slides'
//! and Sheets' own, smaller notices for the same fonts are left out.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use egui::{Align2, Color32, CornerRadius, FontId, Margin, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::json;

use crate::fontget::{self, Source};
use crate::shell::Shell;
use crate::suite::AppKind;

/// How often the open document's fonts are looked at again (seconds).
const RECHECK_SECS: f64 = 2.0;

/// A font a document uses that isn't installed, and the one shown in its place when the app says.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub name: String,
    pub shown_with: Option<String>,
}

/// The three apps, as far as they have started.
pub struct Apps<'a> {
    pub writer: &'a mut Option<Shell>,
    pub slides: &'a mut Option<deckcraft_ui_egui::SlideApp>,
    pub sheets: &'a mut Option<gridcraft_ui_egui::SheetApp>,
}

/// What the bar asks of the suite.
#[derive(Debug, PartialEq)]
pub enum Request {
    /// Show Android's file picker for a font file.
    PickFont,
}

pub struct FontBar {
    /// Whether the bar shows at all ("Don't show this again" turns it off).
    pub enabled: bool,
    /// The file whose presence turns the bar off (next to Writer's preferences).
    off_file: Option<PathBuf>,
    /// The document looked at: its key, when, and its missing fonts.
    key: String,
    checked: f64,
    missing: Vec<Missing>,
    /// Documents whose bar was closed.
    closed: HashSet<String>,
    /// The options card is open, and its similar-fonts list.
    card: bool,
    similar: bool,
    /// Which missing font the card is about (an index into `missing`).
    pick: usize,
    /// The font typed under "Choose a similar font".
    other: String,
    /// The last action's outcome: (text, it worked, when).
    note: Option<(String, bool, f64)>,
    /// A download in progress: what, and its folder of fonts (or what went wrong).
    download: Option<(String, Receiver<Result<PathBuf, String>>)>,
}

impl FontBar {
    /// `off_file`: a file whose presence means "don't show this again".
    pub fn new(off_file: Option<PathBuf>) -> FontBar {
        let enabled = !off_file.as_ref().is_some_and(|f| f.exists());
        FontBar {
            enabled,
            off_file,
            key: String::new(),
            checked: 0.0,
            missing: Vec::new(),
            closed: HashSet::new(),
            card: false,
            similar: false,
            pick: 0,
            other: String::new(),
            note: None,
            download: None,
        }
    }

    /// Turns the bar on or off, remembered for the next start.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.card = false;
        }
        if let Some(f) = &self.off_file {
            let r = if on {
                std::fs::remove_file(f).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
            } else {
                std::fs::write(f, b"")
            };
            if let Err(e) = r {
                log::warn!("{}: {e}", f.display());
            }
        }
    }

    /// Whether the options card shows (Back closes it).
    pub fn card_open(&self) -> bool {
        self.card
    }

    /// Opens the options card (with its similar-fonts list when `similar`), as the bar's buttons do.
    pub fn open_card(&mut self, similar: bool) {
        self.card = true;
        self.similar = similar;
    }

    /// Closes the options card.
    pub fn close_card(&mut self) {
        self.card = false;
    }

    /// The missing fonts the bar shows now.
    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }

    /// Installs the font file the picker gave (`path`), in every app.
    pub fn add_file(&mut self, apps: &mut Apps, active: AppKind, path: &str, now: f64) {
        let note = match font_file_folder(path) {
            Ok(dir) => {
                let r = installed(install_folder(apps, active, &dir), "that file");
                let _ = std::fs::remove_dir_all(&dir);
                r
            }
            Err(e) => (e, false),
        };
        self.finish(note, now);
    }

    fn finish(&mut self, note: (String, bool), now: f64) {
        self.note = Some((note.0, note.1, now));
        self.checked = 0.0; // look again at once
    }
}

struct Colors {
    bg: Color32,
    border: Color32,
    red: Color32,
    text: Color32,
    soft: Color32,
    ok_bg: Color32,
    ok: Color32,
}

fn colors(dark: bool) -> Colors {
    if dark {
        Colors {
            bg: Color32::from_rgb(62, 30, 32),
            border: Color32::from_rgb(112, 52, 56),
            red: Color32::from_rgb(236, 88, 76),
            text: Color32::from_gray(238),
            soft: Color32::from_gray(200),
            ok_bg: Color32::from_rgb(26, 56, 38),
            ok: Color32::from_rgb(110, 210, 140),
        }
    } else {
        Colors {
            bg: Color32::from_rgb(253, 236, 236),
            border: Color32::from_rgb(244, 199, 199),
            red: Color32::from_rgb(217, 48, 37),
            text: Color32::from_gray(28),
            soft: Color32::from_gray(70),
            ok_bg: Color32::from_rgb(230, 244, 234),
            ok: Color32::from_rgb(24, 128, 56),
        }
    }
}

/// The bar over the app showing (call before the app draws), and its card when open.
pub fn show(bar: &mut FontBar, apps: &mut Apps, active: AppKind, ui: &mut Ui) -> Option<Request> {
    quiet_own_notice(apps, active);
    if !bar.enabled {
        bar.card = false;
        return None;
    }
    let now = ui.input(|i| i.time);
    poll_download(bar, apps, active, now);
    if now - bar.checked > RECHECK_SECS || bar.checked == 0.0 {
        bar.checked = now;
        if let Some((key, missing)) = missing_fonts(apps, active) {
            if key != bar.key {
                // Another document: its own fonts (the card stays only for the first one seen).
                if !bar.key.is_empty() {
                    bar.card = false;
                }
                bar.key = key;
                bar.pick = 0;
            }
            // A font with a metric-compatible twin installed (Calibri and Carlito) lays out the same,
            // so there's nothing to fix.
            bar.missing = missing.into_iter().filter(|m| !metric_twin(&m.name).iter().any(|t| has_font(active, t))).collect();
            bar.pick = bar.pick.min(bar.missing.len().saturating_sub(1));
        } else {
            bar.key.clear();
            bar.missing.clear();
        }
    }
    ui.ctx().request_repaint_after(Duration::from_secs(2));
    let c = colors(ui.visuals().dark_mode);
    let id = egui::Id::new("worksuite_fontbar");

    // After an install that left nothing missing: a short "installed" line instead.
    if bar.missing.is_empty() || bar.closed.contains(&bar.key) {
        bar.card = false;
        if let Some((text, true, at)) = &bar.note
            && now - at < 6.0
        {
            let text = text.clone();
            egui::Panel::top(id).frame(egui::Frame::NONE.inner_margin(Margin { left: 8, right: 8, top: 6, bottom: 4 })).show(ui, |ui| {
                egui::Frame::NONE.fill(c.ok_bg).corner_radius(10).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(format!("✓  {text}")).size(14.5).color(c.ok)).wrap());
                });
            });
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        return None;
    }

    let names: Vec<String> = bar.missing.iter().map(|m| m.name.clone()).collect();
    let title = match names.as_slice() {
        [one] => format!("“{one}” is not available on this device."),
        [a, b] => format!("“{a}” and “{b}” are not available on this device."),
        [first, rest @ ..] => format!("“{first}” and {} other fonts are not available on this device.", rest.len()),
        [] => String::new(),
    };
    let app = active.label();
    let sub = match (bar.missing.first().and_then(|m| m.shown_with.as_ref()), names.len()) {
        (Some(with), 1) => format!("{app} will use a substitute font ({with})."),
        _ => format!("{app} will use substitute fonts."),
    };
    let busy = bar.download.is_some();
    let mut open_card: Option<bool> = None;
    let mut close = false;
    egui::Panel::top(id).frame(egui::Frame::NONE.inner_margin(Margin { left: 8, right: 8, top: 6, bottom: 4 })).show(ui, |ui| {
        egui::Frame::NONE
            .fill(c.bg)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(10)
            .inner_margin(Margin { left: 12, right: 6, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    error_icon(ui, c.red, 28.0);
                    ui.add_space(2.0);
                    let w = (ui.available_width() - 40.0).max(80.0);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.add(egui::Label::new(RichText::new(&title).size(15.0).strong().color(c.text)).wrap());
                        ui.add(egui::Label::new(RichText::new(&sub).size(14.0).color(c.soft)).wrap());
                    });
                    if close_button(ui, c.soft).clicked() {
                        close = true;
                    }
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(34.0);
                    ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
                    let label = if busy { "Downloading…" } else { "⬇  Download font" };
                    if filled_button(ui, label, c.red).clicked() && !busy {
                        open_card = Some(false);
                    }
                    if outline_button(ui, "Choose font", c.red).clicked() {
                        open_card = Some(true);
                    }
                });
            });
    });
    if close {
        bar.closed.insert(bar.key.clone());
        bar.card = false;
        return None;
    }
    if let Some(similar) = open_card {
        bar.card = true;
        bar.similar = similar;
    }
    if bar.card { card(bar, apps, active, ui, &c, now) } else { None }
}

/// The options card, over everything.
fn card(bar: &mut FontBar, apps: &mut Apps, active: AppKind, ui: &mut Ui, c: &Colors, now: f64) -> Option<Request> {
    let Some(font) = bar.missing.get(bar.pick).cloned() else {
        bar.card = false;
        return None;
    };
    let name = font.name.clone();
    let names: Vec<String> = bar.missing.iter().map(|m| m.name.clone()).collect();
    let sources = fontget::sources(&name);
    let busy = bar.download.is_some();
    let mut action: Option<Action> = None;
    let mut close = false;
    let width = (ui.ctx().content_rect().width() - 24.0).clamp(240.0, 440.0);
    let card_id = egui::Id::new("worksuite_fontcard");
    let modal = egui::Modal::new(card_id)
        .area(egui::Modal::default_area(card_id).fade_in(false))
        .frame(egui::Frame::NONE.fill(ui.visuals().window_fill).stroke(Stroke::new(1.0, c.border)).corner_radius(14).inner_margin(Margin::same(16)))
        .show(ui.ctx(), |ui| {
            ui.set_width(width - 32.0);
            ui.horizontal_top(|ui| {
                error_icon(ui, c.red, 36.0);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.set_width((width - 32.0 - 90.0).max(100.0));
                    ui.label(RichText::new("Font not available").size(18.0).strong());
                    ui.add(egui::Label::new(RichText::new(format!("“{name}” is not installed on this device.")).size(14.0).weak()).wrap());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if close_button(ui, ui.visuals().text_color()).clicked() {
                        close = true;
                    }
                });
            });
            if names.len() > 1 {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Font:").size(13.0).weak());
                    for (i, n) in names.iter().enumerate() {
                        if ui.selectable_label(i == bar.pick, n).clicked() {
                            bar.pick = i;
                            bar.other.clear();
                        }
                    }
                });
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.62).show(ui, |ui| {
                let mut first = true;
                let mut microsoft = false;
                for s in &sources {
                    let (title, line) = match s {
                        Source::Aptos => ("Download from Microsoft".to_string(), "Microsoft's free Aptos fonts (3 MB)".to_string()),
                        Source::Core(_) => ("Download from Microsoft".to_string(), "Microsoft's free Core fonts for the Web".to_string()),
                        Source::Google { family, twin: true } => {
                            ("Download from Google Fonts".to_string(), format!("{family}: free, with the same widths as {name}"))
                        }
                        Source::Google { family, .. } => ("Download from Google Fonts".to_string(), format!("Get “{family}”, free")),
                    };
                    microsoft |= s.vendor() == "Microsoft";
                    let icon = if s.vendor() == "Microsoft" { "⊞" } else { "☁" };
                    if option_row(ui, icon, &title, &line, first.then_some(c.bg), c, busy).clicked() && !busy {
                        action = Some(Action::Download(s.clone()));
                    }
                    first = false;
                }
                if option_row(ui, "📁", "Add a font file…", "A .ttf or .otf file on this device", None, c, false).clicked() {
                    action = Some(Action::PickFile);
                }
                if option_row(ui, "Aa", "Choose a similar font", "Use an installed font in its place", first.then_some(c.bg), c, false).clicked() {
                    bar.similar = !bar.similar;
                }
                if bar.similar {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        for f in suggestions(&name, font.shown_with.as_deref()).into_iter().filter(|f| has_font(active, f)) {
                            if ui.add(egui::Button::new(RichText::new(f).size(15.0)).min_size(vec2(0.0, 36.0))).clicked() {
                                action = Some(Action::Replace(name.clone(), f.to_string()));
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut bar.other).hint_text("Another installed font").desired_width((width - 120.0).max(120.0)),
                        );
                        let typed = bar.other.trim().to_string();
                        let ok = !typed.is_empty() && has_font(active, &typed);
                        if ui.add_enabled(ok, egui::Button::new("Use").min_size(vec2(0.0, 32.0))).clicked() {
                            action = Some(Action::Replace(name.clone(), typed));
                        }
                    });
                }
                if microsoft {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new("Microsoft's fonts come straight from Microsoft, under its licence: for your own devices only.")
                                .size(12.5)
                                .weak(),
                        );
                        ui.hyperlink_to(RichText::new("Terms").size(12.5), fontget::APTOS_PAGE);
                    });
                }
            });
            if let Some((text, ok, at)) = &bar.note
                && now - at < 12.0
            {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(text).size(13.5).color(if *ok { c.ok } else { c.red })).wrap());
            }
            ui.add_space(8.0);
            let mut hide = false;
            if ui.checkbox(&mut hide, "Don't show this again").changed() && hide {
                action = Some(Action::TurnOff);
            }
        });
    if close || modal.should_close() {
        bar.card = false;
    }
    match action? {
        Action::Download(source) => {
            let (tx, rx) = channel();
            let what = source.vendor().to_string();
            let spawned = std::thread::Builder::new().name("font-download".into()).spawn(move || {
                let _ = tx.send(fontget::download(&source));
            });
            match spawned {
                Ok(_) => bar.download = Some((what, rx)),
                Err(e) => bar.finish((format!("Couldn't start the download: {e}"), false), now),
            }
            None
        }
        Action::PickFile => Some(Request::PickFont),
        Action::Replace(from, to) => {
            let note = match replace_font(apps, active, &from, &to) {
                Ok(0) => (format!("“{from}” wasn't found in the document's text or styles."), false),
                Ok(_) => (format!("“{from}” is now {to} throughout. Undo takes it back."), true),
                Err(e) => (format!("Couldn't replace the font: {e}"), false),
            };
            bar.finish(note, now);
            None
        }
        Action::TurnOff => {
            bar.set_enabled(false);
            None
        }
    }
}

enum Action {
    Download(Source),
    PickFile,
    Replace(String, String),
    TurnOff,
}

/// A finished download: install what came, in every app.
fn poll_download(bar: &mut FontBar, apps: &mut Apps, active: AppKind, now: f64) {
    let Some((from, rx)) = &bar.download else { return };
    let from = from.clone();
    let note = match rx.try_recv() {
        Ok(Ok(dir)) => {
            let r = installed(install_folder(apps, active, &dir), &from);
            let _ = std::fs::remove_dir_all(&dir);
            r
        }
        Ok(Err(e)) => (e, false),
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => ("The download stopped.".into(), false),
    };
    bar.download = None;
    bar.finish(note, now);
}

// ---- the apps

/// The open document's fonts that aren't installed, and a key naming the document (so a closed
/// bar comes back for the next one). `None` before a document shows.
fn missing_fonts(apps: &mut Apps, active: AppKind) -> Option<(String, Vec<Missing>)> {
    match active {
        AppKind::Writer => {
            let w = apps.writer.as_mut()?;
            if w.word.ui.backstage {
                return None;
            }
            let s = &mut w.word.session;
            let key = format!("writer:{}:{}", s.document_id(), s.path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
            // A new document is in Writer's own default fonts (Word's Aptos): nothing to tell.
            let new = s.path.is_none();
            let list = wordcraft_engine::cmd::font::missing_fonts(s)
                .into_iter()
                .filter(|m| !(new && ["Aptos", "Aptos Display"].iter().any(|d| m.requested.eq_ignore_ascii_case(d))))
                .map(|m| Missing { name: m.requested, shown_with: Some(m.shown_with) })
                .collect();
            Some((key, list))
        }
        AppKind::Slides => {
            let d = apps.slides.as_ref()?.session.active()?;
            // Slides draws symbol fonts' characters (Wingdings bullets…) as Unicode symbols, as
            // Writer does: nothing is missing.
            let list = d
                .missing_fonts
                .iter()
                .filter(|n| !wordcraft_doc::symbol_fonts::is_symbol_font(n))
                .map(|n| Missing { name: n.clone(), shown_with: None })
                .collect();
            Some((format!("slides:{}", d.uid), list))
        }
        AppKind::Sheets => {
            let s = apps.sheets.as_ref()?;
            let d = s.session.active()?;
            // A new workbook is in Sheets' own default font (Excel's Calibri).
            let (uid, new) = (d.uid, d.path.is_none());
            gridcraft_ui_egui::fonts::start_scan();
            let list = gridcraft_ui_egui::fonts::missing_among(&s.used_fonts())
                .into_iter()
                .filter(|n| !(new && n.eq_ignore_ascii_case("Calibri")))
                .map(|n| Missing { name: n, shown_with: None })
                .collect();
            Some((format!("sheets:{uid}"), list))
        }
    }
}

/// Slides and Sheets say once per document that its fonts aren't installed; the bar says it instead.
fn quiet_own_notice(apps: &mut Apps, active: AppKind) {
    match active {
        AppKind::Slides => {
            if let Some(s) = apps.slides.as_mut()
                && let Some(uid) = s.session.active().map(|d| d.uid)
            {
                s.fonts_noted.insert(uid);
            }
        }
        AppKind::Sheets => {
            if let Some(s) = apps.sheets.as_mut()
                && let Some(uid) = s.session.active().map(|d| d.uid)
            {
                s.fonts_noted.insert(uid);
            }
        }
        AppKind::Writer => {}
    }
}

/// Whether `active`'s app has the font `family` (installed, or built into the app).
fn has_font(active: AppKind, family: &str) -> bool {
    match active {
        AppKind::Writer => wordcraft_fonts::FontDb::global().has_family(family),
        AppKind::Slides => deckcraft_fonts::FontDb::global().has_family(family),
        AppKind::Sheets => {
            !gridcraft_ui_egui::fonts::faces_of(family).is_empty()
                || (gridcraft_ui_egui::fonts::scan_done() && gridcraft_ui_egui::fonts::missing_among(&[family.to_string()]).is_empty())
        }
    }
}

/// Installs the fonts in `dir` on this device, in all three apps (each keeps them for the next
/// start); answers the `active` app's `{added, notFound}`.
pub fn install_folder(apps: &mut Apps, active: AppKind, dir: &Path) -> Result<serde_json::Value, String> {
    let folder = dir.to_string_lossy().to_string();
    let params = json!({ "folder": folder });
    let writer = match apps.writer.as_mut() {
        Some(w) => w.word.run("font.add", params.clone()),
        None => Ok(writer_install(dir)),
    };
    let slides = match apps.slides.as_mut() {
        Some(s) => s.run("font.add", params.clone()),
        None => {
            let r = deckcraft_fonts::install::install_fonts_from(&[dir.to_path_buf()], None, None);
            Ok(json!({ "added": r.added, "notFound": [] }))
        }
    };
    let sheets = match apps.sheets.as_mut() {
        Some(s) => s.install_fonts(&params),
        None => {
            let r = gridcraft_ui_egui::fonts::install_fonts_from(&[dir.to_path_buf()], None, None);
            Ok(json!({ "added": r.added, "notFound": [] }))
        }
    };
    for (app, r) in [("Writer", &writer), ("Slides", &slides), ("Sheets", &sheets)] {
        if let Err(e) = r {
            log::warn!("installing fonts in {app}: {e}");
        }
    }
    match active {
        AppKind::Writer => writer,
        AppKind::Slides => slides,
        AppKind::Sheets => sheets,
    }
}

/// Writer before it has started: its font database is the same one, and keeps what it adds.
fn writer_install(dir: &Path) -> serde_json::Value {
    let mut added = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if wordcraft_fonts::FontDb::global().install_font(&name, &bytes).is_ok() {
            added.push(name);
        }
    }
    json!({ "added": added, "notFound": [] })
}

/// A folder holding a copy of the picked font file, for [`install_folder`].
fn font_file_folder(path: &str) -> Result<PathBuf, String> {
    let src = Path::new(path);
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "font.ttf".into());
    let lower = name.to_lowercase();
    if ![".ttf", ".otf", ".ttc", ".woff", ".woff2"].iter().any(|e| lower.ends_with(e)) {
        return Err(format!("{name} isn't a font file (.ttf or .otf)."));
    }
    let dir = std::env::temp_dir().join(format!("worksuite-fontfile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::copy(src, dir.join(&name)).map_err(|e| format!("Couldn't read {name}: {e}"))?;
    Ok(dir)
}

/// Uses the font `to` wherever the document uses `from` (its styles, theme fonts and text), as one
/// step Undo takes back. Answers how many places changed.
pub fn replace_font(apps: &mut Apps, active: AppKind, from: &str, to: &str) -> Result<usize, String> {
    match active {
        AppKind::Writer => {
            let Some(w) = apps.writer.as_mut() else { return Ok(0) };
            // Writer keeps the whole document in its JSON form without loss (it opens and saves
            // .json documents), so the names are changed there.
            let s = &mut w.word.session;
            let mut v = serde_json::to_value(&s.doc).map_err(|e| e.to_string())?;
            let n = replace_font_names(&mut v, from, to, &["font", "fontCs", "fontEa", "majorFont", "minorFont"]);
            if n == 0 {
                return Ok(0);
            }
            let doc = serde_json::from_value(v).map_err(|e| e.to_string())?;
            s.checkpoint("Replace Fonts");
            s.doc = doc;
            s.dirty = true;
            s.touch();
            Ok(n)
        }
        AppKind::Slides => {
            let Some(s) = apps.slides.as_mut() else { return Ok(0) };
            s.run("font.replace", json!({ "from": from, "to": to }))
                .map(|r| r.get("replaced").and_then(serde_json::Value::as_u64).unwrap_or(1) as usize)
        }
        AppKind::Sheets => {
            let Some(s) = apps.sheets.as_mut() else { return Ok(0) };
            let Some(d) = s.session.active_mut() else { return Ok(0) };
            let before = (d.wb.clone(), d.selection.clone());
            let wb = std::sync::Arc::make_mut(&mut d.wb);
            let same = |n: &str| n.trim().eq_ignore_ascii_case(from.trim());
            let mut n = 0usize;
            // The interned styles: by way of their JSON form, then indexed again.
            let mut styles = serde_json::to_value(&wb.styles).map_err(|e| e.to_string())?;
            let in_styles = replace_cell_font_names(&mut styles, from, to);
            if in_styles > 0 {
                wb.styles = serde_json::from_value(styles).map_err(|e| e.to_string())?;
                wb.styles.rebuild_index();
                n += in_styles;
            }
            for (_, st) in &mut wb.cell_styles {
                if same(&st.font.name) {
                    st.font.name = to.to_string();
                    n += 1;
                }
            }
            for f in [&mut wb.theme.major_font, &mut wb.theme.minor_font] {
                if same(f) {
                    *f = to.to_string();
                    n += 1;
                }
            }
            for sheet in &mut wb.sheets {
                if sheet.shapes.iter().any(|s| s.format.font.as_deref().is_some_and(same)) {
                    for shape in &mut std::sync::Arc::make_mut(sheet).shapes {
                        if shape.format.font.as_deref().is_some_and(same) {
                            shape.format.font = Some(to.to_string());
                            n += 1;
                        }
                    }
                }
            }
            if n == 0 {
                d.wb = before.0;
                return Ok(0);
            }
            d.undo.push(gridcraft_engine::HistoryEntry { label: "Replace Fonts".into(), wb: before.0, selection: before.1 });
            d.redo.clear();
            d.revision += 1;
            Ok(n)
        }
    }
}

/// Renames the font `from` to `to` in a JSON tree: in every string value of a `keys` field.
fn replace_font_names(v: &mut serde_json::Value, from: &str, to: &str, keys: &[&str]) -> usize {
    let mut n = 0;
    match v {
        serde_json::Value::Object(map) => {
            for (k, child) in map.iter_mut() {
                match child {
                    serde_json::Value::String(name) if keys.contains(&k.as_str()) && name.trim().eq_ignore_ascii_case(from.trim()) => {
                        *name = to.to_string();
                        n += 1;
                    }
                    _ => n += replace_font_names(child, from, to, keys),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                n += replace_font_names(child, from, to, keys);
            }
        }
        _ => {}
    }
    n
}

/// Sheets' styles name their font as `"font": {"name": …}`.
fn replace_cell_font_names(v: &mut serde_json::Value, from: &str, to: &str) -> usize {
    let mut n = 0;
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Object(font)) = map.get_mut("font")
                && let Some(serde_json::Value::String(name)) = font.get_mut("name")
                && name.trim().eq_ignore_ascii_case(from.trim())
            {
                *name = to.to_string();
                n += 1;
            }
            for (k, child) in map.iter_mut() {
                if k != "font" {
                    n += replace_cell_font_names(child, from, to);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                n += replace_cell_font_names(child, from, to);
            }
        }
        _ => {}
    }
    n
}

/// The apps' `font.add` answer, in words.
fn installed(r: Result<serde_json::Value, String>, from: &str) -> (String, bool) {
    match r {
        Ok(v) => {
            let list = |k: &str| {
                v.get(k)
                    .and_then(serde_json::Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            };
            let (added, missing) = (list("added"), list("notFound"));
            match (added.is_empty(), missing.is_empty()) {
                (false, true) => (format!("Installed {added} on this device."), true),
                (false, false) => (format!("Installed {added}. Still not installed: {missing}."), true),
                (true, false) => (format!("{missing} wasn't found in {from}."), false),
                (true, true) => ("Nothing new to install: these fonts are already there.".into(), true),
            }
        }
        Err(e) => (e, false),
    }
}

/// Open fonts with the same widths as `name`, so text set in them breaks lines and pages alike.
fn metric_twin(name: &str) -> &'static [&'static str] {
    match name.trim().to_lowercase().as_str() {
        "calibri" => &["Carlito"],
        "cambria" => &["Caladea"],
        "arial" | "helvetica" => &["Liberation Sans", "Arimo"],
        "arial narrow" => &["Liberation Sans Narrow"],
        "times new roman" | "times" => &["Liberation Serif", "Tinos"],
        "courier new" | "courier" => &["Liberation Mono", "Cousine"],
        "georgia" => &["Gelasio"],
        _ => &[],
    }
}

/// Fonts that look like `name`: metric-compatible ones first (they keep lines and pages where they
/// were), then ones of the same kind. The caller keeps those that are installed.
pub fn suggestions(name: &str, shown_with: Option<&str>) -> Vec<&'static str> {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let mut out: Vec<&'static str> = if has(&["calibri"]) {
        vec!["Carlito"]
    } else if has(&["cambria"]) {
        vec!["Caladea"]
    } else if has(&["arial", "helvetica"]) {
        vec!["Liberation Sans", "Arimo"]
    } else if has(&["times"]) {
        vec!["Liberation Serif", "Tinos"]
    } else if has(&["courier"]) {
        vec!["Liberation Mono", "Cousine"]
    } else if has(&["aptos"]) {
        vec!["Source Sans 3", "Inter", "Open Sans", "Roboto"]
    } else if has(&["segoe", "frutiger", "myriad"]) {
        vec!["Open Sans", "Noto Sans", "Roboto", "Source Sans 3"]
    } else if has(&["georgia"]) {
        vec!["Gelasio", "Source Serif 4", "Noto Serif"]
    } else if has(&["verdana", "tahoma"]) {
        vec!["DejaVu Sans", "Noto Sans"]
    } else if has(&["garamond"]) {
        vec!["EB Garamond", "Source Serif 4"]
    } else if has(&["palatino", "book antiqua"]) {
        vec!["TeX Gyre Pagella", "Source Serif 4"]
    } else if has(&["century gothic", "futura", "avenir"]) {
        vec!["Montserrat", "Poppins", "Nunito Sans"]
    } else if has(&["gill sans", "trebuchet"]) {
        vec!["Lato", "Fira Sans"]
    } else if has(&["consolas", "menlo", "monaco", "lucida console"]) {
        vec!["JetBrains Mono", "DejaVu Sans Mono", "Droid Sans Mono", "Liberation Mono"]
    } else if has(&["comic"]) {
        vec!["Comic Neue"]
    } else {
        Vec::new()
    };
    let kind: &[&'static str] = if has(&["mono", "code", "console", "typewriter"]) {
        &["JetBrains Mono", "DejaVu Sans Mono", "Droid Sans Mono", "Liberation Mono"]
    } else if has(&["serif", "roman", "book", "garamond", "georgia", "times", "cambria", "minion", "baskerville", "c059"]) && !n.contains("sans") {
        &["Source Serif 4", "Noto Serif", "Liberation Serif", "Merriweather", "DejaVu Serif"]
    } else {
        &["Roboto", "Inter", "Source Sans 3", "Open Sans", "Noto Sans", "Liberation Sans", "DejaVu Sans"]
    };
    out.extend(kind.iter().copied());
    if let Some(w) = shown_with {
        // The font shown now first: choosing it keeps the look and stops the notice.
        let known: &[&'static str] = &[
            "Carlito",
            "Caladea",
            "Liberation Sans",
            "Liberation Serif",
            "Liberation Mono",
            "Source Sans 3",
            "Source Serif 4",
            "Inter",
            "Open Sans",
            "Roboto",
            "Noto Sans",
            "Noto Serif",
            "DejaVu Sans",
            "Arimo",
            "Tinos",
            "Cousine",
        ];
        if let Some(k) = known.iter().find(|k| k.eq_ignore_ascii_case(w)) {
            out.insert(0, k);
        }
    }
    let mut seen = HashSet::new();
    out.retain(|f| seen.insert(*f));
    out.truncate(8);
    out
}

// ---- widgets

fn error_icon(ui: &mut Ui, red: Color32, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.circle_filled(r.center(), size / 2.0, red.gamma_multiply(0.18));
    p.circle_filled(r.center(), size * 0.36, red);
    p.text(r.center(), Align2::CENTER_CENTER, "!", FontId::proportional(size * 0.5), Color32::WHITE);
}

fn filled_button(ui: &mut Ui, label: &str, red: Color32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).size(15.0).strong().color(Color32::WHITE)).fill(red).corner_radius(8).min_size(vec2(0.0, 40.0)))
}

fn outline_button(ui: &mut Ui, label: &str, red: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(15.0).color(red))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.5, red))
            .corner_radius(8)
            .min_size(vec2(0.0, 40.0)),
    )
}

fn close_button(ui: &mut Ui, color: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, CornerRadius::same(6), ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let s = Stroke::new(1.8, color);
    let c = r.center();
    let d = 7.0;
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], s);
    p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], s);
    resp
}

/// A row of the card: an icon, a title and a line under it, and an arrow; `fill` highlights it.
fn option_row(ui: &mut Ui, icon: &str, title: &str, sub: &str, fill: Option<Color32>, c: &Colors, busy: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 60.0), Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    let bg = match (fill, hovered) {
        (Some(f), _) => Some(f),
        (None, true) => Some(ui.visuals().widgets.hovered.weak_bg_fill),
        _ => None,
    };
    if let Some(bg) = bg {
        p.rect_filled(rect, CornerRadius::same(10), bg);
    }
    if hovered {
        p.rect_stroke(rect, CornerRadius::same(10), Stroke::new(1.0, c.border), StrokeKind::Inside);
    }
    let accent = if fill.is_some() { c.red } else { ui.visuals().strong_text_color() };
    p.text(pos2(rect.left() + 24.0, rect.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(20.0), accent);
    let title = if busy { "Downloading…" } else { title };
    p.text(pos2(rect.left() + 48.0, rect.top() + 11.0), Align2::LEFT_TOP, title, FontId::proportional(15.5), accent);
    let sub_font = FontId::proportional(13.0);
    let room = (rect.width() - 48.0 - 30.0).max(40.0);
    let galley = ui.painter().layout(sub.to_string(), sub_font, ui.visuals().weak_text_color(), room);
    p.galley(pos2(rect.left() + 48.0, rect.top() + 33.0), galley, ui.visuals().weak_text_color());
    p.text(pos2(rect.right() - 14.0, rect.center().y), Align2::CENTER_CENTER, "›", FontId::proportional(22.0), ui.visuals().weak_text_color());
    ui.add_space(4.0);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similar_fonts_start_with_metric_compatible_ones() {
        assert_eq!(suggestions("Calibri", Some("Carlito"))[0], "Carlito");
        assert_eq!(suggestions("Times New Roman", None)[0], "Liberation Serif");
        assert!(suggestions("Aptos", Some("Source Sans 3")).contains(&"Roboto"), "Android's own font is offered");
        assert!(suggestions("C059 Roman", None).contains(&"Noto Serif"));
        assert!(suggestions("Fancy Mono", None).contains(&"JetBrains Mono"));
        let s = suggestions("Unknown Display", None);
        assert!(!s.is_empty() && s.len() <= 8);
    }

    #[test]
    fn font_names_are_replaced_only_in_font_fields() {
        let mut v = json!({"font": "Aptos", "text": "Aptos", "styles": [{"fontCs": "aptos "}, {"minorFont": "Calibri"}]});
        assert_eq!(replace_font_names(&mut v, "Aptos", "Inter", &["font", "fontCs", "minorFont"]), 2);
        assert_eq!(v, json!({"font": "Inter", "text": "Aptos", "styles": [{"fontCs": "Inter"}, {"minorFont": "Calibri"}]}));
        let mut g = json!({"styles": [{"font": {"name": "Calibri", "size": 11}}, {"font": {"name": "Arial"}}]});
        assert_eq!(replace_cell_font_names(&mut g, "calibri", "Carlito"), 1);
        assert_eq!(g["styles"][0]["font"]["name"], "Carlito");
    }

    #[test]
    fn only_font_files_can_be_added() {
        assert!(font_file_folder("/x/notes.txt").is_err());
        assert!(font_file_folder("/no/such/Font.ttf").is_err());
    }

    #[test]
    fn dont_show_again_is_remembered() {
        let f = std::env::temp_dir().join(format!("worksuite-fontbar-off-{}", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let mut bar = FontBar::new(Some(f.clone()));
        assert!(bar.enabled);
        bar.set_enabled(false);
        assert!(!FontBar::new(Some(f.clone())).enabled, "off after a restart");
        bar.set_enabled(true);
        assert!(FontBar::new(Some(f.clone())).enabled);
        let _ = std::fs::remove_file(&f);
    }
}
