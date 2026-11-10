use super::{Json, Notice, NoticeKind, SheetApp, json, theme};
use crate::fonts;

impl SheetApp {
    /// Test-harness setup: mark a deterministic font set (see
    /// [`theme::pinned_font_definitions`]) as already installed, so the first `logic` pass neither
    /// starts the host font scan nor replaces the harness's pinned set. Production uses
    /// [`Self::setup_context`] instead.
    #[doc(hidden)]
    pub fn use_prepared_fonts(&mut self) {
        self.prepared_fonts = true;
        self.fonts_ready = true;
        self.fonts_set = true;
        self.fonts_rebind = false;
        self.fonts_han = Some(theme::HanOrder::of(self.ui.language));
        self.fonts_bound = self.used_fonts();
        self.fonts_seen = self.fonts_key();
    }

    /// The font names the open workbooks use: every style's font plus the theme's heading/body
    /// fonts. These are the families [`fonts`] binds an installed file for. Every workbook's, as
    /// each may be showing in a window of its own.
    pub fn used_fonts(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for d in self.session.documents() {
            names.extend(d.wb.styles.iter().map(|(_, s)| s.font.name.clone()));
            names.push(d.wb.theme.major_font.clone());
            names.push(d.wb.theme.minor_font.clone());
            // Text boxes and shapes name their own typefaces.
            names.extend(d.wb.sheets.iter().flat_map(|sh| sh.shapes.iter().filter_map(|s| s.format.font.clone())));
        }
        names
    }

    /// Say once per workbook which of its fonts aren't installed, in the notice bar (when it is
    /// free), offering to install them from a Windows installation on this PC when there is one.
    fn note_missing_fonts(&mut self, uid: u64, used: &[String]) {
        if uid == 0 || self.notice.is_some() || self.fonts_noted.contains(&uid) {
            return;
        }
        let missing = fonts::missing_among(used);
        self.fonts_noted.insert(uid);
        let Some(text) = crate::missing_fonts_text(&missing) else { return };
        #[cfg(not(target_arch = "wasm32"))]
        let action = (!fonts::windows_font_dirs().is_empty()).then(|| ("Install from Windows on this PC".to_string(), crate::INSTALL_FONTS.to_string()));
        #[cfg(target_arch = "wasm32")]
        let action = None;
        self.notice = Some(Notice { text, kind: NoticeKind::Info, action });
    }

    /// `ui.fonts.install` and the notice's Install button: copy fonts into the user font folder
    /// and use them at once.
    pub fn install_fonts(&mut self, p: &Json) -> Result<Json, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = p;
            Err("installing fonts isn't available on the web".into())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let missing = fonts::missing_among(&self.used_fonts());
            let only: Option<Vec<String>> = match p.get("families").and_then(Json::as_array) {
                Some(list) => Some(list.iter().filter_map(Json::as_str).map(str::to_string).collect()),
                None if p.get("all").and_then(Json::as_bool) == Some(true) => None,
                None if p.get("folder").is_some() => None,
                None => Some(missing.clone()),
            };
            let dirs = match p.get("folder").and_then(Json::as_str) {
                Some(f) if std::path::Path::new(f).is_dir() => vec![std::path::PathBuf::from(f)],
                Some(f) => return Err(format!("{f}: not a folder")),
                None => fonts::windows_font_dirs(),
            };
            if dirs.is_empty() {
                return Err("no Windows installation found on this computer (looked for a mounted partition with Windows/Fonts)".into());
            }
            let r = fonts::install_fonts_from(&dirs, only.as_deref(), None);
            let still = fonts::missing_among(&self.used_fonts());
            // Bind the new families at the next frame.
            self.fonts_bound.clear();
            self.fonts_seen = (0, 0);
            let text = match (r.added.is_empty(), still.is_empty()) {
                (false, true) => format!("Installed {} on this computer.", r.added.join(", ")),
                (false, false) => format!("Installed {}. Still not installed: {}.", r.added.join(", "), still.join(", ")),
                (true, false) => format!("{} wasn't found in the Windows installation on this PC.", still.join(", ")),
                (true, true) => "Every font this workbook uses is installed.".into(),
            };
            self.notice = Some(Notice { text, kind: NoticeKind::Info, action: None });
            Ok(json!({
                "added": r.added,
                "alreadyAvailable": r.already,
                "notFound": still,
                "errors": r.errors,
                "to": r.to.map(|t| t.to_string_lossy().to_string()),
                "searched": dirs.iter().map(|d| d.to_string_lossy().to_string()).collect::<Vec<_>>(),
            }))
        }
    }

    /// The open workbooks' (uid, revision), folded: [`Self::used_fonts`] changes only with it.
    fn fonts_key(&self) -> (u64, u64) {
        self.session
            .documents()
            .iter()
            .fold((0u64, 0u64), |(uid, rev), d| (uid.wrapping_mul(31).wrapping_add(d.uid), rev.wrapping_mul(31).wrapping_add(d.revision)))
    }

    /// Installs the text-width closure once, on the first frame.
    pub(super) fn ensure_text_width(&mut self, ctx: &egui::Context) {
        if self.session.text_width.is_some() {
            return;
        }
        let ctx = ctx.clone();
        self.session.text_width = Some(std::sync::Arc::new(move |text, font, exact| theme::cell_text_width(&ctx, text, font, exact)));
    }

    /// Applies the font gates: a language Han-order change, startup/prepared fonts and workbook
    /// font binding. Every branch keeps its one-frame paint gate.
    pub(super) fn apply_font_state(&mut self, ctx: &egui::Context) {
        // A pending synthetic step owns the next real `ui` pass: `ui` returns at the font gate
        // before it handles input, so a font change requested now would consume the step without
        // applying it. Delay the change one frame; the hidden-paint request keeps it coming.
        let defer_font_change = self.synthetic_pending || !self.synthetic.is_empty();
        if !defer_font_change && self.fonts_han.is_some_and(|han| han != theme::HanOrder::of(self.ui.language)) {
            self.fonts_ready = false;
            self.fonts_set = false;
        }
        if !self.fonts_ready {
            self.start_font_setup(ctx);
        } else if self.fonts_rebind {
            // A font-set change made last frame is active now.
            self.fonts_rebind = false;
        } else if !defer_font_change && !self.prepared_fonts && fonts::scan_done() {
            self.rebind_workbook_fonts(ctx);
        }
    }

    /// Applies the pending font set. New fonts apply from the next frame on: paint nothing until
    /// then. Startup always proceeds: without it a queued step would be re-offered forever.
    fn start_font_setup(&mut self, ctx: &egui::Context) {
        if self.fonts_set {
            self.fonts_ready = true;
            return;
        }
        if self.prepared_fonts {
            ctx.set_fonts(theme::pinned_font_definitions());
            theme::apply(ctx, self.ui.dark);
        } else {
            SheetApp::setup_context_for_language(ctx, self.ui.dark, self.ui.language);
        }
        self.fonts_set = true;
        // The rebuilt set has no workbook fonts yet: bind them again once it is active.
        self.fonts_bound.clear();
        self.fonts_seen = (0, 0);
        self.fonts_han = Some(theme::HanOrder::of(self.ui.language));
        ctx.request_repaint();
    }

    /// Binds the workbook's installed fonts once the background scan has finished. Adding fonts
    /// re-applies the font set, which egui activates at the next pass, so skip this frame's paint
    /// (same one-frame cost as startup). Only when an open workbook changed.
    fn rebind_workbook_fonts(&mut self, ctx: &egui::Context) {
        let seen = self.fonts_key();
        if seen == self.fonts_seen {
            return;
        }
        let used = self.used_fonts();
        self.fonts_seen = seen;
        let uid = self.session.active().map(|d| d.uid).unwrap_or(0);
        self.note_missing_fonts(uid, &used);
        if used == self.fonts_bound {
            return;
        }
        ctx.set_fonts(theme::font_definitions_with(self.ui.language, &used));
        self.fonts_bound = used;
        self.fonts_rebind = true;
        ctx.request_repaint();
    }
}
