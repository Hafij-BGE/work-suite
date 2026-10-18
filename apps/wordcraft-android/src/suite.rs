//! WorkSuite: Writer (WordCraft), Slides (DeckCraft) and Sheets (GridCraft) in one window.
//!
//! The suite opens on its home screen (the three apps and Open a file…); a thin bar at the top
//! switches between them. Each app is created the first time it is opened and keeps its documents
//! while another one shows. Documents another app hands over ("Open with") and files picked on the
//! home screen open in the app their type belongs to.
//!
//! The three apps share one egui context but install their own fonts and theme into it. Switching
//! asks the new app to install them again ([`restyle`](wordcraft_ui_egui::WordApp::restyle)); egui
//! makes fonts active from the next frame, so the switch frame shows no app.
//!
//! Android's file picker answers later ([`Files`]); what the answer is for waits in [`Picker`].
//! Nothing here needs Android, so it builds and is tested on any machine.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;
use wordcraft_ui_egui::touch::TouchGestures;
use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest};

use crate::shell::Shell;

/// The suite's name, as the launcher shows it.
pub const SUITE_NAME: &str = "WorkSuite";
/// Who made the suite, on its home screen.
pub const CREDIT: &str = "Made by Md Hafijur Rahman";
/// The app's version (the APK's), on its home screen.
const VERSION: &str = include_str!("../../../packaging/android/VERSION");

/// One of the suite's apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    Writer,
    Slides,
    Sheets,
}

impl AppKind {
    pub const ALL: [AppKind; 3] = [AppKind::Writer, AppKind::Slides, AppKind::Sheets];

    pub fn label(self) -> &'static str {
        match self {
            AppKind::Writer => "Writer",
            AppKind::Slides => "Slides",
            AppKind::Sheets => "Sheets",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            AppKind::Writer => "Letters, reports and notes",
            AppKind::Slides => "Presentations and slide shows",
            AppKind::Sheets => "Spreadsheets, formulas and charts",
        }
    }

    fn color(self) -> Color32 {
        match self {
            AppKind::Writer => Color32::from_rgb(0x2B, 0x5C, 0xD6),
            AppKind::Slides => Color32::from_rgb(0xD9, 0x5B, 0x2B),
            AppKind::Sheets => Color32::from_rgb(0x1E, 0x8A, 0x4C),
        }
    }

    /// The app a file belongs to, from its extension (Writer for anything else).
    pub fn for_path(path: &str) -> AppKind {
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        match ext.as_str() {
            "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" | "deckcraft" => AppKind::Slides,
            "xlsx" | "xlsm" | "xltx" | "xltm" | "csv" | "tsv" => AppKind::Sheets,
            _ => AppKind::Writer,
        }
    }
}

/// Android's file picker, as the suite sees it: asking returns whether it showed; the answer comes
/// later through [`Suite::picked`].
pub trait Files {
    /// Pick a file to open: `document`, `presentation`, `spreadsheet`, `office` (any of the three),
    /// `picture`, or anything else for any file.
    fn pick_open(&self, purpose: &str) -> bool;
    /// Pick where to save a file called `name`.
    fn pick_save(&self, name: &str) -> bool;
}

/// What the picker's answer is for.
pub enum Wait {
    /// A Writer file dialog.
    Writer(Sender<FileDialogAnswer>),
    /// Slides asked for a file (`id` from its inbox protocol); a presentation to open is opened
    /// by path, so saving it writes back to it; anything else goes through the inbox as bytes.
    Slides { id: u64, inbox: deckcraft_ui_egui::Inbox, open: bool },
    /// Sheets' File › Open.
    SheetsOpen,
    /// Sheets' Insert › Picture.
    SheetsPicture(gridcraft_ui_egui::Inbox),
    /// Slides or Sheets saved `bytes` ("download"): write them where the user picks.
    Save { app: AppKind, name: String, bytes: Vec<u8> },
    /// The home screen's Open a file….
    Launcher,
}

impl Wait {
    /// The picker couldn't be shown, or another request replaced this one: as good as cancelled.
    fn cancel(self, reason: &str) {
        match self {
            Wait::Writer(tx) => {
                let _ = tx.send(Err(reason.to_string()));
            }
            Wait::Slides { id, inbox, .. } => {
                if let Ok(mut q) = inbox.lock() {
                    q.push(deckcraft_ui_egui::InboxEvent::Cancelled { id });
                }
            }
            _ => log::info!("file request dropped: {reason}"),
        }
    }
}

/// The pending picker request, shared with the apps' services.
pub struct Picker {
    files: Option<Rc<dyn Files>>,
    wait: RefCell<Option<Wait>>,
}

impl Picker {
    pub fn new(files: Option<Rc<dyn Files>>) -> Rc<Picker> {
        Rc::new(Picker { files, wait: RefCell::new(None) })
    }

    /// Whether an answer is awaited.
    pub fn waiting(&self) -> bool {
        self.wait.borrow().is_some()
    }

    fn open(&self, purpose: &str, wait: Wait) {
        let shown = self.files.as_ref().is_some_and(|f| f.pick_open(purpose));
        self.settle(shown, wait);
    }

    fn save(&self, name: &str, wait: Wait) {
        let shown = self.files.as_ref().is_some_and(|f| f.pick_save(name));
        self.settle(shown, wait);
    }

    fn settle(&self, shown: bool, wait: Wait) {
        if let Some(old) = self.wait.borrow_mut().take() {
            old.cancel("replaced by another file request");
        }
        if shown {
            *self.wait.borrow_mut() = Some(wait);
        } else {
            wait.cancel("Android's file picker isn't available");
        }
    }

    fn take(&self) -> Option<Wait> {
        self.wait.borrow_mut().take()
    }
}

/// Where the suite keeps things on the device.
#[derive(Clone, Debug, Default)]
pub struct SuitePaths {
    /// Writer's preferences file.
    pub writer_prefs: Option<PathBuf>,
    /// Writer's local-recovery folder.
    pub recovery: Option<PathBuf>,
    /// Where Android's file picker puts the editors' copies of picked documents (see `saf.rs`).
    pub copies: Option<PathBuf>,
}

/// The suite: its home screen and the three apps.
pub struct Suite {
    /// The app showing; `None` for the home screen.
    pub active: Option<AppKind>,
    /// The app just switched to installs its fonts this frame and shows from the next.
    settling: bool,
    pub writer: Option<Shell>,
    pub slides: Option<deckcraft_ui_egui::SlideApp>,
    pub sheets: Option<gridcraft_ui_egui::SheetApp>,
    picker: Rc<Picker>,
    paths: SuitePaths,
    /// Touch gestures for Slides, Sheets and the home screen (Writer's shell has its own).
    touch: TouchGestures,
    /// Writer still shows the start screen it opened on: Back from there goes home.
    writer_fresh: bool,
    /// What covers the window's edges, in pixels (left, top, right, bottom), and the window's
    /// width and height in pixels.
    insets_px: Option<[i32; 6]>,
    /// The whole window, and the part of it clear of the system bars (points).
    full: Option<Rect>,
    inner: Option<Rect>,
    /// Back was pressed on the home screen: the host sends the app to the background.
    leave: bool,
    /// The colour behind the system bars, and whether it is dark (for their icons).
    pub chrome: Color32,
    pub chrome_dark: bool,
}

impl Suite {
    pub fn new(picker: Rc<Picker>, paths: SuitePaths) -> Suite {
        Suite {
            active: None,
            settling: false,
            writer: None,
            slides: None,
            sheets: None,
            picker,
            paths,
            touch: TouchGestures::default(),
            writer_fresh: false,
            insets_px: None,
            full: None,
            inner: None,
            leave: false,
            chrome: Color32::BLACK,
            chrome_dark: true,
        }
    }

    /// What covers the window's edges, from the host: kept clear from the next frame on.
    pub fn set_insets(&mut self, px: Option<[i32; 6]>) {
        self.insets_px = px;
    }

    /// Whether Back was pressed on the home screen since the last call.
    pub fn take_leave(&mut self) -> bool {
        std::mem::take(&mut self.leave)
    }

    pub fn picker(&self) -> &Rc<Picker> {
        &self.picker
    }

    /// Show `app` (`None`: the home screen), creating it the first time.
    pub fn switch(&mut self, ctx: &egui::Context, app: Option<AppKind>) {
        if app == self.active {
            return;
        }
        if let Some(kind) = app {
            self.ensure(ctx, kind);
            match kind {
                AppKind::Writer => {
                    if let Some(w) = &mut self.writer {
                        w.word.restyle();
                    }
                }
                AppKind::Slides => {
                    if let Some(s) = &mut self.slides {
                        s.restyle();
                    }
                }
                AppKind::Sheets => {
                    if let Some(s) = &mut self.sheets {
                        s.restyle();
                    }
                }
            }
            self.settling = true;
        }
        self.active = app;
        ctx.request_repaint();
    }

    fn ensure(&mut self, ctx: &egui::Context, kind: AppKind) {
        match kind {
            AppKind::Writer if self.writer.is_none() => {
                let mut shell = Shell::new(writer_services(&self.picker, self.paths.copies.clone()), self.paths.writer_prefs.clone());
                self.writer_fresh = shell.word.ui.backstage;
                if let Some(dir) = &self.paths.recovery {
                    let wake = ctx.clone();
                    if let Err(e) = shell.word.session.enable_recovery(dir.clone(), std::sync::Arc::new(move || wake.request_repaint())) {
                        log::warn!("local recovery unavailable: {e}");
                    }
                }
                self.writer = Some(shell);
            }
            AppKind::Slides if self.slides.is_none() => {
                let session = deckcraft_engine::Session::new();
                let mut slides = deckcraft_ui_egui::SlideApp::new(session, slides_services(&self.picker));
                // Offline: no invitation to an online community on the start screen.
                slides.community_link = false;
                self.slides = Some(slides);
            }
            AppKind::Sheets if self.sheets.is_none() => {
                let mut session = gridcraft_engine::Session::new();
                session.new_workbook();
                // Starts the background scan of the installed fonts, as the desktop app does.
                gridcraft_ui_egui::SheetApp::setup_context(ctx, ctx.theme() == egui::Theme::Dark);
                self.sheets = Some(gridcraft_ui_egui::SheetApp::new(session, sheets_services(&self.picker)));
            }
            _ => {}
        }
    }

    /// Open `path` in the app its type belongs to, and show that app.
    pub fn open_in_app(&mut self, ctx: &egui::Context, path: &str) {
        let kind = AppKind::for_path(path);
        self.switch(ctx, Some(kind));
        match kind {
            AppKind::Writer => {
                if let Some(w) = &mut self.writer {
                    match w.word.run("file.open", json!({ "path": path })) {
                        // The document shows at once, not the start screen it opened over.
                        Ok(_) => {
                            w.word.ui.backstage = false;
                            w.word.canvas.want_focus = true;
                            self.writer_fresh = false;
                        }
                        Err(e) => log::warn!("{path}: {e}"),
                    }
                }
            }
            AppKind::Slides => {
                if let Some(s) = &mut self.slides
                    && let Err(e) = s.open_path(path)
                {
                    log::warn!("{path}: {e}");
                    s.set_status(e);
                }
            }
            AppKind::Sheets => {
                if let Some(s) = &mut self.sheets {
                    s.open_path(path);
                }
            }
        }
    }

    /// The picker's answer: a path, or `None` when cancelled.
    pub fn picked(&mut self, ctx: &egui::Context, answer: Option<String>) {
        let Some(wait) = self.picker.take() else { return };
        match (wait, answer) {
            (Wait::Writer(tx), answer) => {
                let _ = tx.send(Ok(answer.into_iter().collect()));
            }
            (Wait::Slides { id, inbox, open }, Some(path)) => {
                if open {
                    self.push_slides(&inbox, deckcraft_ui_egui::InboxEvent::Cancelled { id });
                    self.open_in_app(ctx, &path);
                } else {
                    let event = match std::fs::read(&path) {
                        Ok(bytes) => deckcraft_ui_egui::InboxEvent::File(deckcraft_ui_egui::InboxFile { id, name: file_name(&path), bytes }),
                        Err(e) => {
                            log::warn!("{path}: {e}");
                            deckcraft_ui_egui::InboxEvent::Cancelled { id }
                        }
                    };
                    self.push_slides(&inbox, event);
                }
            }
            (Wait::Slides { id, inbox, .. }, None) => self.push_slides(&inbox, deckcraft_ui_egui::InboxEvent::Cancelled { id }),
            (Wait::SheetsOpen | Wait::Launcher, Some(path)) => self.open_in_app(ctx, &path),
            (Wait::SheetsPicture(inbox), Some(path)) => match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Ok(mut q) = inbox.lock() {
                        q.push((file_name(&path), bytes));
                    }
                }
                Err(e) => log::warn!("{path}: {e}"),
            },
            (Wait::Save { app, name, bytes }, Some(path)) => self.save_to(app, &name, &bytes, &path),
            (_, None) => {}
        }
        ctx.request_repaint();
    }

    fn push_slides(&self, inbox: &deckcraft_ui_egui::Inbox, event: deckcraft_ui_egui::InboxEvent) {
        if let Ok(mut q) = inbox.lock() {
            q.push(event);
        }
    }

    /// Write a saved file where the user picked. A presentation or workbook that had no file yet
    /// takes that one as its own, so the next Save goes there without asking.
    fn save_to(&mut self, app: AppKind, name: &str, bytes: &[u8], path: &str) {
        if let Err(e) = std::fs::write(path, bytes) {
            log::warn!("{path}: {e}");
            return;
        }
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        match app {
            AppKind::Slides => {
                if let Some(s) = &mut self.slides
                    && matches!(ext.as_str(), "pptx" | "deckcraft")
                    && s.session.active().is_some_and(|d| d.path.is_none())
                {
                    let saved = s.session.execute("file.save", &json!({ "path": path, "format": ext }));
                    s.set_status(match saved {
                        Ok(_) => format!("Saved {}", file_name(path)),
                        Err(e) => e.to_string(),
                    });
                }
            }
            AppKind::Sheets => {
                if let Some(s) = &mut self.sheets
                    && ext == "xlsx"
                    && s.session.active().is_some_and(|d| d.path.is_none())
                {
                    s.run_or_alert("file.save", json!({ "path": path }));
                }
            }
            AppKind::Writer => {}
        }
    }

    /// Rewrite a frame's input: the system bars are kept clear, Back closes what is open or goes
    /// home, and fingers scroll, tap and zoom. Returns how many seconds until a frame is needed to
    /// see a long press, if one is.
    pub fn raw_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) -> Option<f64> {
        self.keep_clear(raw);
        if take_back(raw) {
            self.back(ctx, raw);
        }
        if self.active == Some(AppKind::Writer)
            && let Some(w) = &mut self.writer
        {
            return w.raw_input(raw);
        }
        let wait = self.touch.process(raw);
        match self.active {
            Some(AppKind::Slides) => {
                if let Some(s) = &mut self.slides {
                    s.raw_input_hook(raw);
                }
            }
            Some(AppKind::Sheets) => {
                if let Some(s) = &mut self.sheets {
                    s.raw_input_hook(raw);
                }
            }
            _ => {}
        }
        wait
    }

    /// The part of the window clear of the system bars (and the keyboard), as egui's safe area:
    /// the panels lay out in it, and the suite draws its own colour behind the bars.
    fn keep_clear(&mut self, raw: &mut egui::RawInput) {
        let Some(full) = raw.screen_rect.or(self.full) else { return };
        self.full = Some(full);
        let mut margin = egui::epaint::MarginF32::ZERO;
        if let Some([l, t, r, b, w, h]) = self.insets_px
            && w > 0
            && h > 0
        {
            // Points per pixel, from the window's width in both.
            let k = full.width() / w as f32;
            let m = |v: i32| v.clamp(0, w.max(h)) as f32 * k;
            let clear = egui::epaint::MarginF32 { left: m(l), top: m(t), right: m(r), bottom: m(b) };
            // Never so little left that nothing fits (a stale or odd answer).
            let rest = full - clear;
            if rest.width() >= 160.0 && rest.height() >= 160.0 {
                margin = clear;
            }
        }
        self.inner = Some(full - margin);
        raw.safe_area_insets = Some(egui::SafeAreaInsets(margin));
    }

    /// Android's Back: it closes what is open (a menu, a dialog, the File view over a document, a
    /// slide show); with nothing open it goes to the suite's home screen, and from there it sends
    /// the app to the background.
    fn back(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        match self.active {
            None => self.leave = true,
            Some(kind) if self.back_closes(ctx, kind) => {
                for pressed in [true, false] {
                    raw.events.push(egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: Some(egui::Key::Escape),
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
            Some(_) => self.switch(ctx, None),
        }
    }

    /// Whether the app showing has something open that Escape closes.
    fn back_closes(&self, ctx: &egui::Context, kind: AppKind) -> bool {
        if egui::Popup::is_any_open(ctx) {
            return true;
        }
        match kind {
            AppKind::Writer => self.writer.as_ref().is_some_and(|w| w.word.dialog.is_some() || (w.word.ui.backstage && !self.writer_fresh)),
            AppKind::Slides => self.slides.as_ref().is_some_and(|s| {
                s.dialog.is_some() || s.palette.is_some() || s.show.is_some() || (s.backstage.is_some() && s.session.active().is_some())
            }),
            AppKind::Sheets => {
                self.sheets.as_ref().is_some_and(|s| s.dialog.is_some() || s.message.is_some() || s.backstage.is_some() || s.editor.is_some())
            }
        }
    }

    /// The frame's logic. Returns whether the soft keyboard should come up now (a tap put Writer's
    /// caret on the page).
    pub fn logic(&mut self, ctx: &egui::Context) -> bool {
        if self.writer_fresh && self.writer.as_ref().is_some_and(|w| !w.word.ui.backstage) {
            self.writer_fresh = false;
        }
        match self.active {
            Some(AppKind::Writer) => self.writer.as_mut().is_some_and(|w| w.logic(ctx)),
            Some(AppKind::Slides) => {
                if let Some(s) = &mut self.slides {
                    s.logic(ctx);
                }
                false
            }
            Some(AppKind::Sheets) => {
                if let Some(s) = &mut self.sheets {
                    s.logic(ctx);
                }
                false
            }
            None => false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // The panels lay out in the part of the window clear of the system bars (the frame's
        // top-level area is the whole window).
        let clear = ui.ctx().content_rect();
        if ui.max_rect() != clear {
            ui.scope_builder(egui::UiBuilder::new().max_rect(clear), |ui| self.frame(ui));
        } else {
            self.frame(ui);
        }
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(active) = self.active else {
            let pal = Palette::home(ctx.theme() == egui::Theme::Dark);
            self.paint_chrome(&ctx, pal.bg, pal.dark);
            self.home(ui);
            return;
        };
        // The bar follows the app's own light or dark look.
        let bar = Palette::bar(ui.visuals().dark_mode);
        self.paint_chrome(&ctx, bar.bg, bar.dark);
        if let Some(go) = suite_bar(ui, active, &bar) {
            self.switch(&ctx, go);
            egui::CentralPanel::default().show(ui, |_| {});
            return;
        }
        if std::mem::take(&mut self.settling) {
            // The app's fonts become active next frame.
            egui::CentralPanel::default().show(ui, |_| {});
            ctx.request_repaint();
            return;
        }
        match active {
            AppKind::Writer => {
                if let Some(w) = &mut self.writer {
                    w.ui(ui);
                }
            }
            AppKind::Slides => {
                if let Some(s) = &mut self.slides {
                    s.ui(ui);
                }
            }
            AppKind::Sheets => {
                if let Some(s) = &mut self.sheets {
                    s.ui(ui);
                }
            }
        }
    }

    /// The suite's colour behind the system bars (outside the frame's area).
    fn paint_chrome(&mut self, ctx: &egui::Context, color: Color32, dark: bool) {
        self.chrome = color;
        self.chrome_dark = dark;
        let (Some(full), Some(inner)) = (self.full, self.inner) else { return };
        if full == inner {
            return;
        }
        let p = egui::Painter::new(ctx.clone(), egui::LayerId::background(), full);
        for r in [
            Rect::from_min_max(full.min, pos2(full.max.x, inner.min.y)),
            Rect::from_min_max(pos2(full.min.x, inner.max.y), full.max),
            Rect::from_min_max(pos2(full.min.x, inner.min.y), pos2(inner.min.x, inner.max.y)),
            Rect::from_min_max(pos2(inner.max.x, inner.min.y), pos2(full.max.x, inner.max.y)),
        ] {
            if r.is_positive() {
                p.rect_filled(r, 0.0, color);
            }
        }
    }

    /// The home screen: the three apps, Open a file, and who made it.
    fn home(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let pal = Palette::home(ctx.theme() == egui::Theme::Dark);
        let font = |size: f32| egui::FontId::proportional(size);
        let mut go = None;
        let mut open_file = false;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(pal.bg)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_width();
                let width = (avail - 40.0).clamp(200.0, 880.0);
                let margin = ((avail - width) / 2.0).max(16.0);
                ui.add_space(28.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // Title: the suite's mark, its name and what it is for.
                        let (r, _) = ui.allocate_exact_size(vec2(width, 56.0), Sense::hover());
                        paint_suite_mark(ui.painter(), Rect::from_min_size(r.min, vec2(56.0, 56.0)));
                        ui.painter().text(pos2(r.min.x + 72.0, r.min.y + 18.0), Align2::LEFT_CENTER, SUITE_NAME, font(28.0), pal.text);
                        ui.painter().text(
                            pos2(r.min.x + 72.0, r.min.y + 44.0),
                            Align2::LEFT_CENTER,
                            "Documents, slides and spreadsheets",
                            font(14.5),
                            pal.dim,
                        );
                        ui.add_space(18.0);
                        offline_badge(ui, &pal);
                        ui.add_space(26.0);
                        ui.label(egui::RichText::new("Start").font(font(13.0)).color(pal.dim));
                        ui.add_space(10.0);
                        // The apps: one per row on a phone, side by side on a wide screen.
                        let wide = width >= 720.0;
                        let gap = 14.0;
                        if wide {
                            let w = (width - 2.0 * gap) / 3.0;
                            let (row, _) = ui.allocate_exact_size(vec2(width, 150.0), Sense::hover());
                            for (i, kind) in AppKind::ALL.into_iter().enumerate() {
                                let r = Rect::from_min_size(pos2(row.min.x + i as f32 * (w + gap), row.min.y), vec2(w, 150.0));
                                if app_card(ui, r, kind, &pal, false).clicked() {
                                    go = Some(kind);
                                }
                            }
                        } else {
                            for kind in AppKind::ALL {
                                let (r, _) = ui.allocate_exact_size(vec2(width, 88.0), Sense::hover());
                                if app_card(ui, r, kind, &pal, true).clicked() {
                                    go = Some(kind);
                                }
                                ui.add_space(gap);
                            }
                        }
                        ui.add_space(if wide { 22.0 } else { 8.0 });
                        if open_button(ui, width.min(if wide { 360.0 } else { width }), &pal).clicked() {
                            open_file = true;
                        }
                        ui.add_space(12.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(
                                    "Each file opens in its own app: documents in Writer, presentations in Slides, spreadsheets in Sheets. \
                                     Files also open from your file manager with Open with.",
                                )
                                .font(font(13.0))
                                .color(pal.dim),
                            )
                            .wrap(),
                        );
                        ui.add_space(36.0);
                        let (line, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
                        ui.painter().rect_filled(line, 0.0, pal.line);
                        ui.add_space(18.0);
                        ui.label(egui::RichText::new(CREDIT).font(font(15.0)).color(pal.text));
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "Version {} · built on the open-source WordCraft, DeckCraft and GridCraft",
                                    VERSION.trim()
                                ))
                                .font(font(12.5))
                                .color(pal.dim),
                            )
                            .wrap(),
                        );
                        ui.add_space(28.0);
                    });
                });
            });
        });
        if let Some(kind) = go {
            self.switch(&ctx, Some(kind));
        }
        if open_file {
            self.picker.open("office", Wait::Launcher);
        }
    }
}

/// Removes Android's Back (the browser's Back key) from a frame's input; whether it was pressed.
fn take_back(raw: &mut egui::RawInput) -> bool {
    let mut pressed = false;
    raw.events.retain(|e| match e {
        egui::Event::Key { key: egui::Key::BrowserBack, pressed: p, .. } => {
            pressed |= *p;
            false
        }
        _ => true,
    });
    pressed
}

/// The suite's own colours: the home screen's, or the bar's over a light or dark app.
struct Palette {
    dark: bool,
    bg: Color32,
    card: Color32,
    text: Color32,
    dim: Color32,
    line: Color32,
    shadow: Color32,
    accent: Color32,
}

impl Palette {
    fn home(dark: bool) -> Palette {
        if dark {
            Palette {
                dark,
                bg: Color32::from_rgb(0x12, 0x14, 0x18),
                card: Color32::from_rgb(0x1E, 0x21, 0x27),
                text: Color32::from_gray(238),
                dim: Color32::from_gray(160),
                line: Color32::from_white_alpha(22),
                shadow: Color32::from_black_alpha(90),
                accent: Color32::from_rgb(0x4C, 0x6E, 0xF5),
            }
        } else {
            Palette {
                dark,
                bg: Color32::from_rgb(0xF3, 0xF4, 0xF7),
                card: Color32::WHITE,
                text: Color32::from_rgb(0x16, 0x18, 0x1D),
                dim: Color32::from_rgb(0x5B, 0x61, 0x6E),
                line: Color32::from_black_alpha(20),
                shadow: Color32::from_black_alpha(24),
                accent: Color32::from_rgb(0x3B, 0x5B, 0xDB),
            }
        }
    }

    fn bar(dark: bool) -> Palette {
        let mut p = Palette::home(dark);
        p.bg = if dark { Color32::from_rgb(0x17, 0x19, 0x1D) } else { Color32::WHITE };
        p
    }
}

/// "Works offline": the app has no network access at all.
fn offline_badge(ui: &mut egui::Ui, pal: &Palette) {
    let text = "Works offline · your files stay on this device";
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), pal.text);
    let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + 34.0, 26.0), Sense::hover());
    let green = Color32::from_rgb(0x2F, 0x9E, 0x5B);
    ui.painter().rect_filled(r, 13.0, green.gamma_multiply(if pal.dark { 0.22 } else { 0.12 }));
    ui.painter().circle_filled(pos2(r.min.x + 13.0, r.center().y), 4.0, green);
    ui.painter().galley(pos2(r.min.x + 24.0, r.center().y - galley.size().y / 2.0), galley, pal.text);
}

/// One app on the home screen: a row on a phone (`row`), a tile on a wide screen.
fn app_card(ui: &mut egui::Ui, r: Rect, kind: AppKind, pal: &Palette, row: bool) -> egui::Response {
    let resp = ui.interact(r, ui.id().with(("app_card", kind.label())), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, kind.label()));
    let p = ui.painter();
    let radius = CornerRadius::same(18);
    p.add(egui::Shadow { offset: [0, 3], blur: 14, spread: 0, color: pal.shadow }.as_shape(r, radius));
    let pressed = resp.is_pointer_button_down_on();
    let fill = if pressed {
        lerp(pal.card, kind.color(), 0.14)
    } else if resp.hovered() {
        lerp(pal.card, kind.color(), 0.06)
    } else {
        pal.card
    };
    p.rect(r, radius, fill, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    let font = |size: f32| egui::FontId::proportional(size);
    if row {
        let icon = Rect::from_min_size(pos2(r.min.x + 16.0, r.center().y - 27.0), vec2(54.0, 54.0));
        paint_app_icon(p, icon, kind);
        let x = icon.max.x + 16.0;
        let text_w = (r.max.x - x - 40.0).max(40.0);
        p.text(pos2(x, r.center().y - 12.0), Align2::LEFT_CENTER, kind.label(), font(19.0), pal.text);
        let mut job = egui::text::LayoutJob::simple_singleline(kind.blurb().to_string(), font(13.0), pal.dim);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_w);
        let blurb = p.layout_job(job);
        p.galley(pos2(x, r.center().y + 2.0), blurb, pal.dim);
        paint_chevron(p, pos2(r.max.x - 22.0, r.center().y), pal.dim);
    } else {
        let icon = Rect::from_min_size(r.min + vec2(18.0, 18.0), vec2(52.0, 52.0));
        paint_app_icon(p, icon, kind);
        p.text(pos2(r.min.x + 18.0, r.min.y + 96.0), Align2::LEFT_CENTER, kind.label(), font(19.0), pal.text);
        let blurb = p.layout(kind.blurb().to_string(), font(13.0), pal.dim, r.width() - 36.0);
        p.galley(pos2(r.min.x + 18.0, r.min.y + 112.0), blurb, pal.dim);
    }
    resp
}

/// The home screen's main action.
fn open_button(ui: &mut egui::Ui, width: f32, pal: &Palette) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 52.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open a file"));
    let fill = if resp.is_pointer_button_down_on() { lerp(pal.accent, Color32::BLACK, 0.18) } else { pal.accent };
    let p = ui.painter();
    p.add(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: pal.accent.gamma_multiply(0.35) }.as_shape(r, CornerRadius::same(14)));
    p.rect_filled(r, 14.0, fill);
    let galley = p.layout_no_wrap("Open a file".to_string(), egui::FontId::proportional(17.0), Color32::WHITE);
    let total = 22.0 + 10.0 + galley.size().x;
    let x = r.center().x - total / 2.0;
    paint_folder(p, Rect::from_min_size(pos2(x, r.center().y - 9.0), vec2(22.0, 18.0)), Color32::WHITE);
    p.galley(pos2(x + 32.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    resp
}

/// The bar above the app: Home and the three apps. Returns where to go when one is tapped.
fn suite_bar(ui: &mut egui::Ui, active: AppKind, pal: &Palette) -> Option<Option<AppKind>> {
    let mut go = None;
    egui::Panel::top("suite_bar").exact_size(46.0).frame(egui::Frame::NONE.fill(pal.bg)).show(ui, |ui| {
        let full = ui.max_rect();
        ui.painter().line_segment([full.left_bottom(), full.right_bottom()], Stroke::new(1.0, pal.line));
        let y = full.center().y;
        let home = Rect::from_center_size(pos2(full.min.x + 28.0, y), vec2(40.0, 34.0));
        let resp = ui.interact(home, egui::Id::new("suite_home"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, SUITE_NAME));
        if resp.hovered() || resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(home, 10.0, pal.line);
        }
        paint_home(ui.painter(), Rect::from_center_size(home.center(), vec2(20.0, 20.0)), pal.text);
        if resp.clicked() {
            go = Some(None);
        }
        ui.painter().line_segment([pos2(home.max.x + 8.0, y - 11.0), pos2(home.max.x + 8.0, y + 11.0)], Stroke::new(1.0, pal.line));
        let mut x = home.max.x + 16.0;
        let font = egui::FontId::proportional(14.5);
        for kind in AppKind::ALL {
            let selected = kind == active;
            let ink = if selected { pal.text } else { pal.dim };
            let galley = ui.painter().layout_no_wrap(kind.label().to_string(), font.clone(), ink);
            let r = Rect::from_min_size(pos2(x, y - 17.0), vec2(galley.size().x + 46.0, 34.0));
            let resp = ui.interact(r, egui::Id::new(("suite_app", kind.label())), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, kind.label()));
            if selected {
                ui.painter().rect_filled(r, 17.0, kind.color().gamma_multiply(if pal.dark { 0.32 } else { 0.13 }));
            } else if resp.hovered() || resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(r, 17.0, pal.line);
            }
            paint_app_icon(ui.painter(), Rect::from_min_size(pos2(r.min.x + 10.0, y - 9.0), vec2(18.0, 18.0)), kind);
            ui.painter().galley(pos2(r.min.x + 36.0, y - galley.size().y / 2.0), galley, ink);
            if resp.clicked() && !selected {
                go = Some(Some(kind));
            }
            x = r.max.x + 6.0;
        }
    });
    go
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A house: the suite's home screen.
fn paint_home(p: &egui::Painter, r: Rect, ink: Color32) {
    let s = Stroke::new(1.8, ink);
    let (l, rgt, top, bottom) = (r.min.x + r.width() * 0.2, r.max.x - r.width() * 0.2, r.min.y + 1.0, r.max.y - 1.0);
    let eave = r.min.y + r.height() * 0.45;
    p.line_segment([pos2(r.min.x + 1.0, eave + 1.0), pos2(r.center().x, top)], s);
    p.line_segment([pos2(r.center().x, top), pos2(r.max.x - 1.0, eave + 1.0)], s);
    p.rect_stroke(Rect::from_min_max(pos2(l, eave), pos2(rgt, bottom)), 1.0, s, StrokeKind::Middle);
}

/// "›" at `c`.
fn paint_chevron(p: &egui::Painter, c: egui::Pos2, ink: Color32) {
    let s = Stroke::new(2.0, ink);
    p.line_segment([pos2(c.x - 3.0, c.y - 6.0), pos2(c.x + 3.0, c.y)], s);
    p.line_segment([pos2(c.x + 3.0, c.y), pos2(c.x - 3.0, c.y + 6.0)], s);
}

/// A folder, outlined.
fn paint_folder(p: &egui::Painter, r: Rect, ink: Color32) {
    let s = Stroke::new(1.8, ink);
    let tab = Rect::from_min_size(r.min, vec2(r.width() * 0.42, r.height() * 0.3));
    p.rect_stroke(tab, 2.0, s, StrokeKind::Middle);
    p.rect_stroke(Rect::from_min_max(pos2(r.min.x, r.min.y + r.height() * 0.22), r.max), 3.0, s, StrokeKind::Middle);
}

/// The suite's mark, as on its launcher icon: three app cards fanned on slate.
fn paint_suite_mark(p: &egui::Painter, r: Rect) {
    let k = r.width() / 56.0;
    p.rect_filled(r, 14.0 * k, Color32::from_rgb(0x1F, 0x29, 0x37));
    let card = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(r.min + vec2(x * k, y * k), vec2(w * k, h * k));
    p.rect_filled(card(9.0, 15.0, 22.0, 28.0), 3.0 * k, AppKind::Sheets.color());
    p.rect_filled(card(25.0, 12.0, 22.0, 28.0), 3.0 * k, AppKind::Slides.color());
    let page = card(17.0, 18.0, 22.0, 28.0);
    p.rect_filled(page, 3.0 * k, Color32::WHITE);
    for (i, w) in [0.75_f32, 0.75, 0.45].into_iter().enumerate() {
        let y = page.min.y + (8.0 + 6.0 * i as f32) * k;
        let x0 = page.min.x + 4.0 * k;
        p.line_segment([pos2(x0, y), pos2(x0 + 14.0 * k * w / 0.75, y)], Stroke::new(2.2 * k, AppKind::Writer.color()));
    }
}

/// Each app's icon, drawn in code: a page of lines, a slide with a play triangle, a grid.
pub fn paint_app_icon(p: &egui::Painter, r: Rect, kind: AppKind) {
    let k = r.width() / 52.0;
    p.rect_filled(r, 11.0 * k, kind.color());
    let white = Color32::WHITE;
    match kind {
        AppKind::Writer => {
            let page = Rect::from_min_max(pos2(r.min.x + 14.0 * k, r.min.y + 10.0 * k), pos2(r.max.x - 14.0 * k, r.max.y - 10.0 * k));
            p.rect_filled(page, 2.5 * k, white);
            for (i, w) in [0.8_f32, 0.8, 0.5].into_iter().enumerate() {
                let y = page.min.y + (9.0 + 7.0 * i as f32) * k;
                let x0 = page.min.x + 4.0 * k;
                p.line_segment([pos2(x0, y), pos2(x0 + (page.width() - 8.0 * k) * w, y)], Stroke::new(2.4 * k, kind.color()));
            }
        }
        AppKind::Slides => {
            let slide = Rect::from_min_max(pos2(r.min.x + 9.0 * k, r.min.y + 13.0 * k), pos2(r.max.x - 9.0 * k, r.max.y - 13.0 * k));
            p.rect_filled(slide, 2.5 * k, white);
            let c = slide.center();
            p.add(egui::Shape::convex_polygon(
                vec![pos2(c.x - 4.5 * k, c.y - 6.0 * k), pos2(c.x + 6.5 * k, c.y), pos2(c.x - 4.5 * k, c.y + 6.0 * k)],
                kind.color(),
                Stroke::NONE,
            ));
        }
        AppKind::Sheets => {
            let grid = Rect::from_min_max(pos2(r.min.x + 11.0 * k, r.min.y + 11.0 * k), pos2(r.max.x - 11.0 * k, r.max.y - 11.0 * k));
            p.rect_filled(grid, 2.5 * k, white);
            let s = Stroke::new(1.8 * k, kind.color());
            for i in 1..3 {
                let f = i as f32 / 3.0;
                let x = grid.min.x + grid.width() * f;
                let y = grid.min.y + grid.height() * f;
                p.line_segment([pos2(x, grid.min.y), pos2(x, grid.max.y)], s);
                p.line_segment([pos2(grid.min.x, y), pos2(grid.max.x, y)], s);
            }
        }
    }
}

/// Where a recent document is, in words: the editor's copy of a picked document is "From your
/// files" (its path is the app's private folder); shared storage reads as "Phone storage".
pub fn location_label(copies: Option<&Path>, path: &str) -> String {
    if let Some(copies) = copies {
        // `/data/user/0/<app>/files` and `/data/data/<app>/files` are the same folder.
        let app = copies.parent().and_then(Path::parent).and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        if Path::new(path).starts_with(copies) || app.is_some_and(|app| path.contains(&format!("/{app}/files/documents/"))) {
            return "From your files".into();
        }
    }
    let folder = Path::new(path).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    match folder.strip_prefix("/storage/emulated/0") {
        Some("") => "Phone storage".into(),
        Some(rest) => format!("Phone storage › {}", rest.trim_start_matches('/').replace('/', " › ")),
        None => folder,
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into())
}

/// Writer's file dialogs through the picker.
fn writer_services(picker: &Rc<Picker>, copies: Option<PathBuf>) -> wordcraft_ui_egui::Services {
    let picker = picker.clone();
    wordcraft_ui_egui::Services {
        location_label: Some(Box::new(move |path: &str| location_label(copies.as_deref(), path))),
        file_dialog: Some(Box::new(move |req: FileDialogRequest| {
            let (tx, rx) = std::sync::mpsc::channel::<FileDialogAnswer>();
            match &req {
                FileDialogRequest::Open { purpose } | FileDialogRequest::OpenMany { purpose } => picker.open(purpose, Wait::Writer(tx)),
                FileDialogRequest::Save { name } => picker.save(name, Wait::Writer(tx)),
            }
            rx
        })),
        ..Default::default()
    }
}

/// Slides' files the way its web version gets them: asked for, then delivered (or opened by path).
fn slides_services(picker: &Rc<Picker>) -> deckcraft_ui_egui::Services {
    let inbox = deckcraft_ui_egui::Inbox::default();
    let (open_picker, save_picker, open_inbox) = (picker.clone(), picker.clone(), inbox.clone());
    deckcraft_ui_egui::Services {
        open_async: Some(Box::new(move |purpose: &str, id: u64| {
            let open = purpose == "open";
            let kind = match purpose {
                "open" | "reuse" => "presentation",
                "picture" => "picture",
                _ => "any",
            };
            open_picker.open(kind, Wait::Slides { id, inbox: open_inbox.clone(), open });
        })),
        download: Some(Box::new(move |name: &str, bytes: &[u8]| {
            save_picker.save(name, Wait::Save { app: AppKind::Slides, name: name.to_string(), bytes: bytes.to_vec() });
        })),
        inbox: Some(inbox),
        ..Default::default()
    }
}

/// Sheets' files the way its web version gets them.
fn sheets_services(picker: &Rc<Picker>) -> gridcraft_ui_egui::Services {
    let (open_picker, save_picker, picture_picker) = (picker.clone(), picker.clone(), picker.clone());
    gridcraft_ui_egui::Services {
        open_async: Some(Box::new(move || open_picker.open("spreadsheet", Wait::SheetsOpen))),
        download: Some(Box::new(move |name: &str, bytes: &[u8]| {
            save_picker.save(name, Wait::Save { app: AppKind::Sheets, name: name.to_string(), bytes: bytes.to_vec() });
        })),
        pick_picture_async: Some(Box::new(move |inbox| picture_picker.open("picture", Wait::SheetsPicture(inbox)))),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A picker that records what it was asked and always shows.
    #[derive(Default)]
    struct FakeFiles {
        opened: RefCell<Vec<String>>,
        saved: RefCell<Vec<String>>,
        shows: Cell<bool>,
    }

    impl Files for FakeFiles {
        fn pick_open(&self, purpose: &str) -> bool {
            self.opened.borrow_mut().push(purpose.to_string());
            self.shows.get()
        }
        fn pick_save(&self, name: &str) -> bool {
            self.saved.borrow_mut().push(name.to_string());
            self.shows.get()
        }
    }

    fn suite(shows: bool) -> (Suite, Rc<FakeFiles>) {
        let files = Rc::new(FakeFiles::default());
        files.shows.set(shows);
        let picker = Picker::new(Some(files.clone()));
        (Suite::new(picker, SuitePaths::default()), files)
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("worksuite-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_go_to_the_app_their_type_belongs_to() {
        assert_eq!(AppKind::for_path("/x/Report.DOCX"), AppKind::Writer);
        assert_eq!(AppKind::for_path("notes.md"), AppKind::Writer);
        assert_eq!(AppKind::for_path("/x/Talk.pptx"), AppKind::Slides);
        assert_eq!(AppKind::for_path("deck.ppsx"), AppKind::Slides);
        assert_eq!(AppKind::for_path("Budget.xlsx"), AppKind::Sheets);
        assert_eq!(AppKind::for_path("data.csv"), AppKind::Sheets);
        assert_eq!(AppKind::for_path("no-extension"), AppKind::Writer);
    }

    #[test]
    fn opening_from_home_switches_to_the_files_app_and_opens_it() {
        let ctx = egui::Context::default();
        let dir = temp("open");
        let (mut s, files) = suite(true);
        assert_eq!(s.active, None, "starts on the home screen");
        s.picker.open("office", Wait::Launcher);
        assert!(s.picker.waiting());
        assert_eq!(*files.opened.borrow(), ["office"]);
        // A spreadsheet: Sheets opens it.
        let csv = dir.join("data.csv");
        std::fs::write(&csv, "a,b\n1,2\n").unwrap();
        s.picked(&ctx, Some(csv.to_string_lossy().into_owned()));
        assert!(!s.picker.waiting());
        assert_eq!(s.active, Some(AppKind::Sheets));
        let sheets = s.sheets.as_ref().unwrap();
        assert!(sheets.session.documents().iter().any(|d| d.path.as_deref() == Some(csv.to_string_lossy().as_ref())), "the CSV is open");
        // A text document: Writer opens it.
        let txt = dir.join("note.txt");
        std::fs::write(&txt, "Hello suite").unwrap();
        s.open_in_app(&ctx, &txt.to_string_lossy());
        assert_eq!(s.active, Some(AppKind::Writer));
        let w = s.writer.as_ref().unwrap();
        assert!(w.word.session.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("Hello suite"));
        assert!(!w.word.ui.backstage, "the document shows, not the start screen it opened over");
        // Sheets kept its workbook while Writer showed.
        assert!(!s.sheets.as_ref().unwrap().session.documents().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Runs frames as the Android app does: input, logic, then the interface.
    fn frames(ctx: &egui::Context, s: &mut Suite, n: usize) {
        frames_with(ctx, s, n, Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 860.0)));
    }

    fn frames_with(ctx: &egui::Context, s: &mut Suite, n: usize, window: Rect) {
        for _ in 0..n {
            let mut raw = egui::RawInput { screen_rect: Some(window), ..Default::default() };
            s.raw_input(ctx, &mut raw);
            ctx.run_ui(raw, |ui| {
                s.logic(ui.ctx());
                s.ui(ui);
            })
            .drop_without_applying_deltas();
        }
    }

    fn back() -> egui::RawInput {
        let key = |pressed| egui::Event::Key {
            key: egui::Key::BrowserBack,
            physical_key: Some(egui::Key::BrowserBack),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        egui::RawInput { events: vec![key(true), key(false)], ..Default::default() }
    }

    fn escapes(raw: &egui::RawInput) -> usize {
        raw.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, .. })).count()
    }

    #[test]
    fn back_closes_what_is_open_then_goes_home_then_leaves() {
        let ctx = egui::Context::default();
        let (mut s, _) = suite(true);
        // Writer opens on its start screen: Back from there goes home.
        s.switch(&ctx, Some(AppKind::Writer));
        frames(&ctx, &mut s, 3);
        assert!(s.writer.as_ref().unwrap().word.ui.backstage);
        let mut raw = back();
        s.raw_input(&ctx, &mut raw);
        assert_eq!(s.active, None);
        assert_eq!(escapes(&raw), 0);
        assert!(raw.events.is_empty(), "Back doesn't reach the app");
        // On the home screen, Back leaves the app (the host sends it to the background).
        let mut raw = back();
        s.raw_input(&ctx, &mut raw);
        assert!(s.take_leave());
        assert!(!s.take_leave(), "once");
        // A dialog over Slides: Back closes it (as Escape), and stays in Slides.
        s.switch(&ctx, Some(AppKind::Slides));
        frames(&ctx, &mut s, 3);
        s.slides.as_mut().unwrap().run("app.about", json!({})).unwrap();
        assert!(s.slides.as_ref().unwrap().dialog.is_some());
        let mut raw = back();
        s.raw_input(&ctx, &mut raw);
        assert_eq!(s.active, Some(AppKind::Slides));
        assert_eq!(escapes(&raw), 1);
        // Sheets with nothing open: home.
        s.switch(&ctx, Some(AppKind::Sheets));
        frames(&ctx, &mut s, 3);
        let mut raw = back();
        s.raw_input(&ctx, &mut raw);
        assert_eq!(s.active, None);
    }

    #[test]
    fn the_frame_keeps_clear_of_the_system_bars() {
        let ctx = egui::Context::default();
        let (mut s, _) = suite(true);
        // A 400 × 800 point window, 2 pixels a point: a 48 px status bar and a 32 px gesture bar.
        s.set_insets(Some([0, 48, 0, 32, 800, 1600]));
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 800.0));
        frames_with(&ctx, &mut s, 2, window);
        assert_eq!(ctx.content_rect(), Rect::from_min_max(pos2(0.0, 24.0), pos2(400.0, 784.0)));
        // The suite bar lays out below the status bar.
        s.switch(&ctx, Some(AppKind::Sheets));
        frames_with(&ctx, &mut s, 4, window);
        assert_eq!(ctx.content_rect().min.y, 24.0);
        // The frame's top-level area, where the panels go, is the clear part.
        let mut raw = egui::RawInput { screen_rect: Some(window), ..Default::default() };
        s.raw_input(&ctx, &mut raw);
        let mut bar = Rect::NOTHING;
        ctx.run_ui(raw, |ui| {
            s.logic(ui.ctx());
            s.ui(ui);
            bar = ui.ctx().read_response(egui::Id::new("suite_home")).map_or(Rect::NOTHING, |r| r.rect);
        })
        .drop_without_applying_deltas();
        assert!(bar.min.y >= 24.0 && bar.max.y <= 784.0, "the bar's Home button is at {bar:?}");
        // Nonsense (more than the window) is ignored.
        s.set_insets(Some([0, 5000, 0, 0, 800, 1600]));
        frames_with(&ctx, &mut s, 1, window);
        assert_eq!(ctx.content_rect(), window);
    }

    #[test]
    fn recent_documents_say_where_they_are_in_words() {
        let copies = Path::new("/data/user/0/io.github.hafijbge.wordcraft/files/documents");
        assert_eq!(location_label(Some(copies), "/data/user/0/io.github.hafijbge.wordcraft/files/documents/1391/Meem CV.docx"), "From your files");
        assert_eq!(location_label(Some(copies), "/data/data/io.github.hafijbge.wordcraft/files/documents/7/a.docx"), "From your files");
        assert_eq!(location_label(Some(copies), "/storage/emulated/0/Documents/WorkSuite/Letter.docx"), "Phone storage › Documents › WorkSuite");
        assert_eq!(location_label(None, "/home/me/a.docx"), "/home/me");
    }

    /// The apps install different fonts into the one context: every switch, in every order, draws
    /// without a missing font (egui panics on a font family it doesn't have).
    #[test]
    fn every_app_draws_after_every_switch() {
        let ctx = egui::Context::default();
        let (mut s, _) = suite(true);
        frames(&ctx, &mut s, 2);
        for go in [
            Some(AppKind::Slides),
            Some(AppKind::Sheets),
            Some(AppKind::Writer),
            Some(AppKind::Slides),
            None,
            Some(AppKind::Sheets),
            Some(AppKind::Writer),
            None,
        ] {
            s.switch(&ctx, go);
            frames(&ctx, &mut s, 4);
            assert_eq!(s.active, go);
            assert!(!s.settling, "{go:?} shows after its fonts settled");
        }
    }

    #[test]
    fn switching_restyles_and_waits_a_frame() {
        let ctx = egui::Context::default();
        let (mut s, _) = suite(true);
        s.switch(&ctx, Some(AppKind::Slides));
        assert_eq!(s.active, Some(AppKind::Slides));
        assert!(s.slides.is_some() && s.settling, "created, and its fonts settle a frame");
        s.switch(&ctx, None);
        assert_eq!(s.active, None);
        assert!(s.slides.is_some(), "kept while the home screen shows");
    }

    #[test]
    fn writer_dialogs_answer_through_the_channel_and_fail_without_a_picker() {
        let ctx = egui::Context::default();
        let (mut s, files) = suite(true);
        let services = writer_services(&s.picker, None);
        let rx = (services.file_dialog.as_ref().unwrap())(FileDialogRequest::Save { name: "Letter.docx".into() });
        assert_eq!(*files.saved.borrow(), ["Letter.docx"]);
        s.picked(&ctx, Some("/data/Letter.docx".into()));
        assert_eq!(rx.try_recv().unwrap(), Ok(vec!["/data/Letter.docx".to_string()]));
        let rx = (services.file_dialog.as_ref().unwrap())(FileDialogRequest::Open { purpose: "document".into() });
        s.picked(&ctx, None);
        assert_eq!(rx.try_recv().unwrap(), Ok(Vec::new()), "cancelled");

        // No picker: an error, which brings up Writer's own file browser.
        let (s, _) = suite(false);
        let services = writer_services(&s.picker, None);
        let rx = (services.file_dialog.as_ref().unwrap())(FileDialogRequest::Open { purpose: "document".into() });
        assert!(rx.try_recv().unwrap().is_err());
        assert!(!s.picker.waiting());
    }

    #[test]
    fn slides_pictures_arrive_in_its_inbox_and_cancels_are_reported() {
        let ctx = egui::Context::default();
        let dir = temp("slides");
        let (mut s, files) = suite(true);
        let inbox = deckcraft_ui_egui::Inbox::default();
        s.picker.open("picture", Wait::Slides { id: 7, inbox: inbox.clone(), open: false });
        let png = dir.join("dot.png");
        std::fs::write(&png, [1u8, 2, 3]).unwrap();
        s.picked(&ctx, Some(png.to_string_lossy().into_owned()));
        match inbox.lock().unwrap().pop() {
            Some(deckcraft_ui_egui::InboxEvent::File(f)) => assert_eq!((f.id, f.name.as_str(), f.bytes.len()), (7, "dot.png", 3)),
            other => panic!("expected the picture, got {other:?}"),
        }
        // A second request replaces a pending one, which is reported cancelled.
        s.picker.open("picture", Wait::Slides { id: 8, inbox: inbox.clone(), open: false });
        s.picker.open("picture", Wait::Slides { id: 9, inbox: inbox.clone(), open: false });
        assert!(matches!(inbox.lock().unwrap().pop(), Some(deckcraft_ui_egui::InboxEvent::Cancelled { id: 8 })));
        s.picked(&ctx, None);
        assert!(matches!(inbox.lock().unwrap().pop(), Some(deckcraft_ui_egui::InboxEvent::Cancelled { id: 9 })));
        assert_eq!(files.opened.borrow().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_workbook_saved_through_the_picker_keeps_that_file() {
        let ctx = egui::Context::default();
        let dir = temp("save");
        let (mut s, _) = suite(true);
        s.switch(&ctx, Some(AppKind::Sheets));
        let path = dir.join("Book1.xlsx");
        // What Sheets hands over when saved ("downloaded") with no file yet.
        let bytes = s.sheets.as_mut().unwrap().run("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        let bytes = gridcraft_engine::io::base64_decode(bytes["base64"].as_str().unwrap()).unwrap();
        s.picker.save("Book1.xlsx", Wait::Save { app: AppKind::Sheets, name: "Book1.xlsx".into(), bytes });
        s.picked(&ctx, Some(path.to_string_lossy().into_owned()));
        assert!(path.exists(), "written where the user picked");
        let sheets = s.sheets.as_ref().unwrap();
        assert_eq!(
            sheets.session.active().and_then(|d| d.path.clone()).as_deref(),
            Some(path.to_string_lossy().as_ref()),
            "and the workbook took it"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
