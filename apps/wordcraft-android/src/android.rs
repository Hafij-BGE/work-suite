//! Android's entry point, the system file picker and the folders the app uses.
//!
//! - Open, Save As, Export and Insert › Pictures show Android's own file picker ([`crate::saf`]),
//!   which reaches every storage app (Downloads, Drive…); documents opened from a file manager
//!   ("Open with") arrive the same way. When the picker can't be reached, the built-in browser
//!   shows instead.
//! - Settings, logs and local recovery: the app's private folder (`internal_data_path`), which
//!   Android removes with the app.
//! - The built-in file browser starts in `Documents/WorkSuite Writer` in shared storage (its Home
//!   folder), or the app's own folder there (`Android/data/…/files`) when that can't be made.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use winit::platform::android::activity::AndroidApp;
use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest, Services};

use crate::saf::Saf;
use crate::shell::Shell;

/// The pending picker's answer goes here.
type Pending = Rc<RefCell<Option<Sender<FileDialogAnswer>>>>;

/// Shared storage's Documents folder (the same path on every current Android device).
const SHARED_DOCUMENTS: &str = "/storage/emulated/0/Documents";

/// Called by `android-activity` on the app's own thread once the activity starts.
#[allow(unsafe_code)] // The C symbol NativeActivity looks up; it takes no pointers.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("WorkSuiteWriter"));
    let folders = Folders::new(&app);
    folders.export();
    let mut options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    options.viewport = options.viewport.with_title(wordcraft_ui_egui::APP_NAME);
    let result = eframe::run_native(
        wordcraft_ui_egui::APP_NAME,
        options,
        Box::new(move |cc| {
            // Fingers are less exact than a mouse: a tap a little beside a small button still
            // presses it.
            cc.egui_ctx.all_styles_mut(|s| s.interaction.interact_radius = 12.0);
            let saf = Saf::new(&app).map(Rc::new);
            let pending = Pending::default();
            let mut shell = Shell::new(services(saf.clone(), pending.clone()), folders.settings.as_ref().map(|d| d.join("ui.json")));
            if let Some(dir) = &folders.settings {
                let ctx = cc.egui_ctx.clone();
                if let Err(e) = shell.word.session.enable_recovery(dir.join("recovery"), std::sync::Arc::new(move || ctx.request_repaint())) {
                    log::warn!("local recovery unavailable: {e}");
                }
            }
            Ok(Box::new(App { shell, android: app, saf, pending, synced_at: 0.0 }))
        }),
    );
    if let Err(e) = result {
        log::error!("stopped: {e}");
    }
}

struct App {
    shell: Shell,
    android: AndroidApp,
    saf: Option<Rc<Saf>>,
    pending: Pending,
    /// When saved documents were last copied back (input time, seconds).
    synced_at: f64,
}

/// How often saved documents are copied back to where they came from.
const SYNC_EVERY: f64 = 2.0;

impl App {
    /// The picker's answer, documents other apps opened with this one, and copying saves back.
    fn files(&mut self, ctx: &egui::Context) {
        let Some(saf) = self.saf.clone() else { return };
        let waiting = self.pending.borrow().is_some();
        if waiting {
            match saf.take_result() {
                Some(picked) => {
                    let answer = if picked.is_empty() { Vec::new() } else { vec![picked] };
                    if let Some(tx) = self.pending.borrow_mut().take() {
                        let _ = tx.send(Ok(answer));
                    }
                    ctx.request_repaint();
                }
                // Android wakes nothing here when the picker answers: look again shortly.
                None => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
            }
        }
        while let Some(path) = saf.take_opened() {
            if let Err(e) = self.shell.word.run("file.open", serde_json::json!({ "path": path })) {
                log::warn!("{path}: {e}");
            }
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
        if self.shell.logic(ctx) {
            self.android.show_soft_input(true);
        }
        self.files(ctx);
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(wait) = self.shell.raw_input(raw) {
            ctx.request_repaint_after_secs(wait as f32);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.ui(ui);
    }
    fn on_exit(&mut self) {
        self.shell.save_prefs();
    }
}

/// File dialogs through Android's picker; without it, an error that brings up the built-in file
/// browser.
fn services(saf: Option<Rc<Saf>>, pending: Pending) -> Services {
    Services {
        file_dialog: Some(Box::new(move |req: FileDialogRequest| {
            let (tx, rx) = std::sync::mpsc::channel::<FileDialogAnswer>();
            let shown = saf.as_ref().is_some_and(|saf| match &req {
                FileDialogRequest::Open { purpose } | FileDialogRequest::OpenMany { purpose } => saf.pick_open(purpose),
                FileDialogRequest::Save { name } => saf.pick_save(name),
            });
            if shown {
                *pending.borrow_mut() = Some(tx);
            } else {
                let _ = tx.send(Err("Android's file picker isn't available".into()));
            }
            rx
        })),
        ..Default::default()
    }
}

struct Folders {
    /// Preferences, logs and recovery.
    settings: Option<PathBuf>,
    /// Where documents go by default.
    documents: Option<PathBuf>,
    /// Temporary files.
    cache: Option<PathBuf>,
}

impl Folders {
    fn new(app: &AndroidApp) -> Folders {
        let internal = app.internal_data_path();
        let settings = internal.as_ref().map(|d| d.join("settings")).filter(|d| made(d));
        let shared = PathBuf::from(SHARED_DOCUMENTS).join("WorkSuite Writer");
        let documents = if made(&shared) { Some(shared) } else { app.external_data_path().filter(|d| made(d)) };
        let cache = internal.as_ref().map(|d| d.join("cache")).filter(|d| made(d));
        log::info!("folders: settings {settings:?}, documents {documents:?}");
        Folders { settings, documents, cache }
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
