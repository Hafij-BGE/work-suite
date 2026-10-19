//! Android's entry point, the system file picker and the folders the app uses.
//!
//! - The suite ([`crate::suite`]): Writer, Slides and Sheets, and a home screen to pick one.
//! - Open, Save As, Export and inserting pictures show Android's own file picker ([`crate::saf`]),
//!   which reaches every storage app (Downloads, Drive…); documents opened from a file manager
//!   ("Open with") arrive the same way and open in the app their type belongs to. When the picker
//!   can't be reached, Writer shows its built-in browser instead.
//! - Settings, logs and local recovery: the app's private folder (`internal_data_path`), which
//!   Android removes with the app.
//! - Writer's built-in file browser starts in `Documents/WorkSuite` in shared storage (its Home
//!   folder), or the app's own folder there (`Android/data/…/files`) when that can't be made.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use winit::platform::android::activity::AndroidApp;

use crate::saf::Saf;
use crate::suite::{Files, Pen, Picker, SUITE_NAME, Suite, SuitePaths};

/// Shared storage's Documents folder (the same path on every current Android device).
const SHARED_DOCUMENTS: &str = "/storage/emulated/0/Documents";

/// How often saved documents are copied back to where they came from.
const SYNC_EVERY: f64 = 2.0;

/// Called by `android-activity` on the app's own thread once the activity starts.
#[allow(unsafe_code)] // The C symbol NativeActivity looks up; it takes no pointers.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("WorkSuite"));
    let folders = Folders::new(&app);
    folders.export();
    let mut options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    options.viewport = options.viewport.with_title(SUITE_NAME);
    let result = eframe::run_native(
        SUITE_NAME,
        options,
        Box::new(move |_cc| {
            let saf = Saf::new(&app).map(Rc::new);
            let files = saf.clone().map(|s| s as Rc<dyn Files>);
            let suite = Suite::new(
                Picker::new(files),
                SuitePaths {
                    writer_prefs: folders.settings.as_ref().map(|d| d.join("ui.json")),
                    recovery: folders.settings.as_ref().map(|d| d.join("recovery")),
                    copies: folders.copies.clone(),
                },
            );
            Ok(Box::new(App { suite, android: app, saf, synced_at: 0.0, light_bars: None }))
        }),
    );
    if let Err(e) = result {
        log::error!("stopped: {e}");
    }
}

impl Files for Saf {
    fn pick_open(&self, purpose: &str) -> bool {
        Saf::pick_open(self, purpose)
    }
    fn pick_save(&self, name: &str) -> bool {
        Saf::pick_save(self, name)
    }
}

struct App {
    suite: Suite,
    android: AndroidApp,
    saf: Option<Rc<Saf>>,
    /// When saved documents were last copied back (input time, seconds).
    synced_at: f64,
    /// Whether the system bars' icons were last set dark (over a light app).
    light_bars: Option<bool>,
}

impl App {
    /// The picker's answer, documents other apps opened with this one, and copying saves back.
    fn files(&mut self, ctx: &egui::Context) {
        let Some(saf) = self.saf.clone() else { return };
        if self.suite.picker().waiting() {
            match saf.take_result() {
                Some(picked) => self.suite.picked(ctx, Some(picked).filter(|p| !p.is_empty())),
                // Android wakes nothing here when the picker answers: look again shortly.
                None => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
            }
        }
        while let Some(path) = saf.take_opened() {
            self.suite.open_in_app(ctx, &path);
        }
        let now = ctx.input(|i| i.time);
        if now - self.synced_at >= SYNC_EVERY {
            self.synced_at = now;
            saf.sync_back();
        }
        // A document another app sends while this one is open, and saves finishing in the
        // background, need a frame to be seen.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SYNC_EVERY));
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Fingers are less exact than a mouse: a tap a little beside a small button still presses
        // it. (Set again when an app's theme replaced the style.)
        if ctx.global_style().interaction.interact_radius < 12.0 {
            ctx.all_styles_mut(|s| s.interaction.interact_radius = 12.0);
        }
        if self.suite.logic(ctx) {
            self.android.show_soft_input(true);
        }
        self.files(ctx);
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.suite.set_insets(self.saf.as_ref().and_then(|s| s.insets()));
        let pen = winit::platform::android::stylus();
        self.suite.set_pen(Pen { touching: pen.touching, eraser: pen.eraser });
        if let Some(wait) = self.suite.raw_input(ctx, raw) {
            ctx.request_repaint_after_secs(wait as f32);
        }
        if self.suite.take_leave()
            && let Some(saf) = &self.saf
        {
            saf.move_to_back();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.suite.ui(ui);
        let light = !self.suite.chrome_dark;
        if self.light_bars != Some(light)
            && let Some(saf) = &self.saf
        {
            saf.set_light_bars(light);
            self.light_bars = Some(light);
        }
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Behind the system bars until the first frame paints them.
        self.suite.chrome.to_normalized_gamma_f32()
    }
    fn on_exit(&mut self) {
        if let Some(w) = &self.suite.writer {
            w.save_prefs();
        }
    }
}

struct Folders {
    /// Preferences, logs and recovery.
    settings: Option<PathBuf>,
    /// Where documents go by default.
    documents: Option<PathBuf>,
    /// Temporary files.
    cache: Option<PathBuf>,
    /// The editors' copies of documents picked in Android's file picker (made by `WorkSuiteActivity`).
    copies: Option<PathBuf>,
}

impl Folders {
    fn new(app: &AndroidApp) -> Folders {
        let internal = app.internal_data_path();
        let settings = internal.as_ref().map(|d| d.join("settings")).filter(|d| made(d));
        let shared = PathBuf::from(SHARED_DOCUMENTS).join("WorkSuite");
        let documents = if made(&shared) { Some(shared) } else { app.external_data_path().filter(|d| made(d)) };
        let cache = internal.as_ref().map(|d| d.join("cache")).filter(|d| made(d));
        let copies = internal.as_ref().map(|d| d.join("documents"));
        log::info!("folders: settings {settings:?}, documents {documents:?}");
        Folders { settings, documents, cache, copies }
    }

    /// Hand the folders to the rest of the app, which finds them the way it does on Linux:
    /// `WORDCRAFT_PROFILE_DIR` (settings), `HOME` (the file browser's Home folder; fonts and
    /// dictionaries the user adds) and `TMPDIR`.
    fn export(&self) {
        for (name, dir) in [("WORDCRAFT_PROFILE_DIR", &self.settings), ("HOME", &self.documents), ("TMPDIR", &self.cache)] {
            if let Some(dir) = dir {
                // SAFETY: runs first thing in `android_main`, before this app starts any thread of
                // its own or reads the environment; the Java side doesn't read these variables.
                #[allow(unsafe_code)]
                unsafe {
                    std::env::set_var(name, dir);
                }
            }
        }
    }
}

fn made(dir: &Path) -> bool {
    std::fs::create_dir_all(dir).is_ok() && dir.is_dir()
}
