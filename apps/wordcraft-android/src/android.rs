//! Android's entry point and the folders the app uses.
//!
//! - Settings, logs and local recovery: the app's private folder (`internal_data_path`), which
//!   Android removes with the app.
//! - Documents: `Documents/WordCraft` in shared storage, where the built-in file browser starts
//!   (its Home folder). Android 11 and later let an app write there without a permission and keep
//!   the files when the app is removed; reading documents other apps saved (in Download, say) needs
//!   "All files access" (Settings › Apps › WordCraft › Permissions). When the folder can't be made,
//!   the app's own folder in shared storage (`Android/data/…/files`) is used instead.

use std::path::{Path, PathBuf};

use winit::platform::android::activity::AndroidApp;
use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest, Services};

use crate::shell::Shell;

/// Shared storage's Documents folder (the same path on every current Android device).
const SHARED_DOCUMENTS: &str = "/storage/emulated/0/Documents";

/// Called by `android-activity` on the app's own thread once the activity starts.
#[allow(unsafe_code)] // The C symbol NativeActivity looks up; it takes no pointers.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("WordCraft"));
    let folders = Folders::new(&app);
    folders.export();
    let mut options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    options.viewport = options.viewport.with_title("WordCraft");
    let result = eframe::run_native(
        "WordCraft",
        options,
        Box::new(move |cc| {
            // Fingers are less exact than a mouse: a tap a little beside a small button still
            // presses it.
            cc.egui_ctx.all_styles_mut(|s| s.interaction.interact_radius = 12.0);
            let mut shell = Shell::new(services(), folders.settings.as_ref().map(|d| d.join("ui.json")));
            if let Some(dir) = &folders.settings {
                let ctx = cc.egui_ctx.clone();
                if let Err(e) = shell.word.session.enable_recovery(dir.join("recovery"), std::sync::Arc::new(move || ctx.request_repaint())) {
                    log::warn!("local recovery unavailable: {e}");
                }
            }
            Ok(Box::new(App { shell, android: app }))
        }),
    );
    if let Err(e) = result {
        log::error!("WordCraft stopped: {e}");
    }
}

struct App {
    shell: Shell,
    android: AndroidApp,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.shell.logic(ctx) {
            self.android.show_soft_input(true);
        }
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

/// Android has no system file dialog this app can show: the built-in file browser opens instead.
fn services() -> Services {
    Services {
        file_dialog: Some(Box::new(|_: FileDialogRequest| {
            let (tx, rx) = std::sync::mpsc::channel::<FileDialogAnswer>();
            let _ = tx.send(Err("Android".into()));
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
        let shared = PathBuf::from(SHARED_DOCUMENTS).join("WordCraft");
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
