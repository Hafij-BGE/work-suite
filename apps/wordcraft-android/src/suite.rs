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

use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;
use wordcraft_ui_egui::touch::TouchGestures;
use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest};

use crate::shell::{Shell, back_is_escape};

/// The suite's name, as the launcher shows it.
pub const SUITE_NAME: &str = "WorkSuite";

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
            AppKind::Writer => "Documents: letters, reports, notes (.docx, .odt, .rtf, .txt, .md)",
            AppKind::Slides => "Presentations and slide shows (.pptx)",
            AppKind::Sheets => "Spreadsheets, formulas and charts (.xlsx, .csv)",
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
    /// Writer's preferences file and local-recovery folder.
    writer_prefs: Option<PathBuf>,
    recovery: Option<PathBuf>,
    /// Touch gestures for Slides, Sheets and the home screen (Writer's shell has its own).
    touch: TouchGestures,
}

impl Suite {
    pub fn new(picker: Rc<Picker>, writer_prefs: Option<PathBuf>, recovery: Option<PathBuf>) -> Suite {
        Suite {
            active: None,
            settling: false,
            writer: None,
            slides: None,
            sheets: None,
            picker,
            writer_prefs,
            recovery,
            touch: TouchGestures::default(),
        }
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
                let mut shell = Shell::new(writer_services(&self.picker), self.writer_prefs.clone());
                if let Some(dir) = &self.recovery {
                    let wake = ctx.clone();
                    if let Err(e) = shell.word.session.enable_recovery(dir.clone(), std::sync::Arc::new(move || wake.request_repaint())) {
                        log::warn!("local recovery unavailable: {e}");
                    }
                }
                self.writer = Some(shell);
            }
            AppKind::Slides if self.slides.is_none() => {
                let session = deckcraft_engine::Session::new();
                self.slides = Some(deckcraft_ui_egui::SlideApp::new(session, slides_services(&self.picker)));
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
                if let Some(w) = &mut self.writer
                    && let Err(e) = w.word.run("file.open", json!({ "path": path }))
                {
                    log::warn!("{path}: {e}");
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

    /// Rewrite a frame's input: Back is Escape, and fingers scroll, tap and zoom. Returns how many
    /// seconds until a frame is needed to see a long press, if one is.
    pub fn raw_input(&mut self, raw: &mut egui::RawInput) -> Option<f64> {
        if self.active == Some(AppKind::Writer)
            && let Some(w) = &mut self.writer
        {
            return w.raw_input(raw);
        }
        back_is_escape(raw);
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

    /// The frame's logic. Returns whether the soft keyboard should come up now (a tap put Writer's
    /// caret on the page).
    pub fn logic(&mut self, ctx: &egui::Context) -> bool {
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
        let ctx = ui.ctx().clone();
        let Some(active) = self.active else {
            self.home(ui);
            return;
        };
        if let Some(go) = suite_bar(ui, active) {
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

    /// The home screen: the three apps, and Open a file….
    fn home(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let dark = ctx.theme() == egui::Theme::Dark;
        let (bg, card, text, dim) = if dark {
            (Color32::from_rgb(0x1B, 0x1D, 0x21), Color32::from_rgb(0x26, 0x29, 0x2E), Color32::from_gray(235), Color32::from_gray(160))
        } else {
            (Color32::from_rgb(0xF3, 0xF4, 0xF6), Color32::WHITE, Color32::from_gray(25), Color32::from_gray(95))
        };
        let font = |size: f32| egui::FontId::proportional(size);
        let mut go = None;
        let mut open_file = false;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width().min(640.0);
                let margin = ((ui.available_width() - width) / 2.0).max(16.0);
                ui.add_space(28.0);
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.vertical(|ui| {
                        ui.set_width(width - 32.0);
                        ui.label(egui::RichText::new(SUITE_NAME).font(font(30.0)).color(text).strong());
                        ui.label(egui::RichText::new("Write, present and calculate. Free and open source.").font(font(15.0)).color(dim));
                        ui.add_space(20.0);
                        for kind in AppKind::ALL {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, kind.label()));
                            let fill = if resp.hovered() || resp.is_pointer_button_down_on() { card.gamma_multiply(0.94) } else { card };
                            ui.painter().rect(r, 14.0, fill, Stroke::new(1.0, dim.gamma_multiply(0.25)), StrokeKind::Inside);
                            let tile = Rect::from_min_size(pos2(r.min.x + 14.0, r.center().y - 26.0), vec2(52.0, 52.0));
                            paint_app_icon(ui.painter(), tile, kind);
                            let x = tile.max.x + 16.0;
                            ui.painter().text(pos2(x, r.center().y - 12.0), Align2::LEFT_CENTER, kind.label(), font(20.0), text);
                            let blurb = ui.painter().layout(kind.blurb().to_string(), font(13.0), dim, (r.max.x - x - 12.0).max(40.0));
                            ui.painter().galley(pos2(x, r.center().y + 2.0), blurb, dim);
                            if resp.clicked() {
                                go = Some(kind);
                            }
                            ui.add_space(12.0);
                        }
                        ui.add_space(6.0);
                        let open = ui.add_sized(
                            vec2(ui.available_width(), 48.0),
                            egui::Button::new(egui::RichText::new("Open a file…").font(font(17.0))).corner_radius(12.0),
                        );
                        if open.clicked() {
                            open_file = true;
                        }
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("Files open in the app they belong to. Documents also open from your file manager with Open with.")
                                .font(font(13.0))
                                .color(dim),
                        );
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

/// The bar above the app: Home and the three apps. Returns where to go when one is tapped.
fn suite_bar(ui: &mut egui::Ui, active: AppKind) -> Option<Option<AppKind>> {
    let mut go = None;
    let dark = ui.ctx().theme() == egui::Theme::Dark;
    let (bg, text) = if dark {
        (Color32::from_rgb(0x15, 0x17, 0x1A), Color32::from_gray(225))
    } else {
        (Color32::from_rgb(0xE9, 0xEB, 0xEF), Color32::from_gray(35))
    };
    egui::Panel::top("suite_bar").exact_size(34.0).frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| {
        let full = ui.max_rect();
        let mut x = full.min.x + 6.0;
        let home = Rect::from_min_size(pos2(x, full.min.y + 3.0), vec2(40.0, 28.0));
        let resp = ui.interact(home, ui.id().with("suite_home"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, SUITE_NAME));
        paint_home(ui.painter(), home.shrink(5.0), text);
        if resp.clicked() {
            go = Some(None);
        }
        x = home.max.x + 6.0;
        for kind in AppKind::ALL {
            let label_w = ui.painter().layout_no_wrap(kind.label().to_string(), egui::FontId::proportional(14.0), text).size().x;
            let r = Rect::from_min_size(pos2(x, full.min.y + 3.0), vec2(label_w + 34.0, 28.0));
            let resp = ui.interact(r, ui.id().with(("suite_app", kind.label())), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, kind == active, kind.label()));
            if kind == active {
                ui.painter().rect_filled(r, 8.0, kind.color().gamma_multiply(if dark { 0.45 } else { 0.18 }));
            }
            paint_app_icon(ui.painter(), Rect::from_min_size(pos2(r.min.x + 6.0, r.center().y - 8.0), vec2(16.0, 16.0)), kind);
            ui.painter().text(pos2(r.min.x + 27.0, r.center().y), Align2::LEFT_CENTER, kind.label(), egui::FontId::proportional(14.0), text);
            if resp.clicked() && kind != active {
                go = Some(Some(kind));
            }
            x = r.max.x + 4.0;
        }
    });
    go
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

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into())
}

/// Writer's file dialogs through the picker.
fn writer_services(picker: &Rc<Picker>) -> wordcraft_ui_egui::Services {
    let picker = picker.clone();
    wordcraft_ui_egui::Services {
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
        (Suite::new(picker, None, None), files)
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
        // Sheets kept its workbook while Writer showed.
        assert!(!s.sheets.as_ref().unwrap().session.documents().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Runs frames as the Android app does: input, logic, then the interface.
    fn frames(ctx: &egui::Context, s: &mut Suite, n: usize) {
        for _ in 0..n {
            let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(420.0, 860.0))), ..Default::default() };
            s.raw_input(&mut raw);
            ctx.run_ui(raw, |ui| {
                s.logic(ui.ctx());
                s.ui(ui);
            })
            .drop_without_applying_deltas();
        }
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
        let services = writer_services(&s.picker);
        let rx = (services.file_dialog.as_ref().unwrap())(FileDialogRequest::Save { name: "Letter.docx".into() });
        assert_eq!(*files.saved.borrow(), ["Letter.docx"]);
        s.picked(&ctx, Some("/data/Letter.docx".into()));
        assert_eq!(rx.try_recv().unwrap(), Ok(vec!["/data/Letter.docx".to_string()]));
        let rx = (services.file_dialog.as_ref().unwrap())(FileDialogRequest::Open { purpose: "document".into() });
        s.picked(&ctx, None);
        assert_eq!(rx.try_recv().unwrap(), Ok(Vec::new()), "cancelled");

        // No picker: an error, which brings up Writer's own file browser.
        let (s, _) = suite(false);
        let services = writer_services(&s.picker);
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
