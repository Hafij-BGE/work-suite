//! The part of the Android app that doesn't need Android: the editor with touch gestures, the
//! Back key, when to bring up the soft keyboard, and the preferences file. Kept apart so it builds
//! and is tested on any machine.

use std::path::{Path, PathBuf};

use wordcraft_engine::Session;
use wordcraft_ui_egui::touch::TouchGestures;
use wordcraft_ui_egui::{Services, UiState, WordApp};

/// The editor and what the Android app adds around it.
pub struct Shell {
    pub word: WordApp,
    touch: TouchGestures,
    /// `ui.json` in the app's private folder; `None` keeps no preferences.
    prefs_path: Option<PathBuf>,
    /// The window had the focus last frame (it loses it when the app goes to the background).
    focused: bool,
}

impl Shell {
    /// A blank document (or the File › Home start screen), with the preferences saved at
    /// `prefs_path` when there are any.
    pub fn new(services: Services, prefs_path: Option<PathBuf>) -> Shell {
        let session = Session::new(wordcraft_engine::blank_document(&wordcraft_engine::Region::default()));
        let mut word = WordApp::new(session, services);
        let saved = prefs_path.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|bytes| UiState::from_json(&bytes));
        match saved {
            Some(ui) => word.apply_prefs(ui),
            None => first_run(&mut word),
        }
        // The title bar has no room for the community button on a phone.
        word.ui.show_discord = false;
        word.start_screen();
        Shell { word, touch: TouchGestures::default(), prefs_path, focused: true }
    }

    /// Rewrite a frame's input for touch. Returns how many seconds until the next frame is needed
    /// to see a long press, if one is.
    pub fn raw_input(&mut self, raw: &mut egui::RawInput) -> Option<f64> {
        back_is_escape(raw);
        let wait = self.touch.process(raw);
        self.word.raw_input_hook(raw);
        wait
    }

    /// The frame's logic. Returns whether the soft keyboard should come up now: a tap put the caret
    /// on the page. (egui itself brings it up for text fields and hides it when nothing takes
    /// typing; this brings it back after the user put it away.)
    pub fn logic(&mut self, ctx: &egui::Context) -> bool {
        self.word.logic(ctx);
        // Going to the background is often the last chance to write anything: Android may stop
        // the app there without notice. (Unsaved documents are kept by local recovery.)
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.focused && !focused {
            self.save_prefs();
        }
        self.focused = focused;
        self.tapped_page(ctx)
    }

    fn tapped_page(&self, ctx: &egui::Context) -> bool {
        if self.word.ui.backstage || self.word.dialog.is_some() || !self.word.canvas.focused {
            return false;
        }
        // Drawing with a pen (the Draw tab): a tap is a dot of ink, not a place to type.
        if self.word.session.view.draw.mode != wordcraft_engine::cmd::draw::DrawMode::Select {
            return false;
        }
        let Some(page) = self.word.canvas.canvas_rect else { return false };
        ctx.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| page.contains(p)))
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.word.ui(ui);
    }

    /// Write the preferences (to a temporary file renamed over the old one).
    pub fn save_prefs(&self) {
        let Some(path) = &self.prefs_path else { return };
        match serde_json::to_vec_pretty(&self.word.prefs()) {
            Ok(bytes) => {
                if let Err(e) = write_atomic(path, &bytes) {
                    log::warn!("preferences: cannot save {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("preferences: cannot serialize: {e}"),
        }
    }
}

/// Defaults for a small touch screen, on the first run only (the user's own choices win later).
fn first_run(word: &mut WordApp) {
    // The rulers take a fifth of a phone's height and are hard to drag with a finger.
    word.session.view.ruler = false;
}

/// Android's Back key (and gesture) arrives as the browser's Back key: it closes what Escape
/// closes (the File menu, a dialog, a menu).
pub fn back_is_escape(raw: &mut egui::RawInput) {
    for event in &mut raw.events {
        if let egui::Event::Key { key, physical_key, .. } = event
            && *key == egui::Key::BrowserBack
        {
            *key = egui::Key::Escape;
            *physical_key = Some(egui::Key::Escape);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn back_closes_what_escape_closes() {
        let mut raw = egui::RawInput { events: vec![key(egui::Key::BrowserBack), key(egui::Key::A)], ..Default::default() };
        back_is_escape(&mut raw);
        assert_eq!(raw.events, vec![key(egui::Key::Escape), key(egui::Key::A)]);
    }

    #[test]
    fn first_run_hides_the_rulers_and_preferences_round_trip() {
        let dir = std::env::temp_dir().join(format!("wordcraft-android-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("ui.json");
        let shell = Shell::new(Services::default(), Some(path.clone()));
        assert!(!shell.word.session.view.ruler, "no rulers on the first run");
        assert!(!shell.word.ui.show_discord);
        assert!(shell.word.ui.backstage, "starts on File › Home");
        shell.save_prefs();
        assert!(UiState::from_json(&std::fs::read(&path).unwrap()).is_some());
        // A saved file means it isn't the first run any more.
        let again = Shell::new(Services::default(), Some(path));
        assert!(again.word.ui.backstage);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
