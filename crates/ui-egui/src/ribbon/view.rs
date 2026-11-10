use super::common::stack;
use super::{Ui, WordApp, big, group, json, small};

/// The Outlining tab (Outline view): outline levels, moving, expanding and collapsing, what is
/// shown, and closing the view.
pub(super) fn outlining(app: &mut WordApp, ui: &mut Ui) {
    use wordcraft_engine::layout::outline::{BODY, para_level};
    let o = app.session.view.outline.clone();
    let level = app.session.doc.para_at(&app.session.sel.focus).map(|p| para_level(&app.session.doc, p)).unwrap_or(BODY);
    let name = |l: u8| if l >= BODY { tl!("Body Text").to_string() } else { crate::i18n::fmt(tl!("Level {n}"), &[("n", &l.to_string())]) };
    group(ui, "Outline Tools", None, app, |ui, app| {
        stack(ui, |ui| {
            crate::widgets::row(ui, |ui| {
                small(
                    ui,
                    app,
                    "outlineTop",
                    None,
                    "Promote to Heading 1",
                    crate::widgets::ButtonAction::new("outline.promoteToHeading1", json!({}), false),
                );
                small(ui, app, "outlinePromote", None, "Promote", crate::widgets::ButtonAction::new("outline.promote", json!({}), false));
                let levels: Vec<String> = (1..=BODY).map(name).collect();
                if let Some(v) = crate::widgets::combo_hint(ui, "outlineLevel", 100.0, &name(level), &levels, None, tl!("Outline Level"))
                    && let Some(i) = levels.iter().position(|x| *x == v)
                {
                    let l = if i + 1 >= usize::from(BODY) { json!("body") } else { json!(i + 1) };
                    let _ = app.run("outline.level", json!({ "level": l }));
                }
                small(ui, app, "outlineDemote", None, "Demote", crate::widgets::ButtonAction::new("outline.demote", json!({}), false));
                small(
                    ui,
                    app,
                    "outlineBody",
                    None,
                    "Demote to Body Text",
                    crate::widgets::ButtonAction::new("outline.demoteToBody", json!({}), false),
                );
            });
            ui.add_space(3.0);
            crate::widgets::row(ui, |ui| {
                small(ui, app, "outlineUp", None, "Move Up", crate::widgets::ButtonAction::new("outline.moveUp", json!({}), false));
                small(ui, app, "outlineDown", None, "Move Down", crate::widgets::ButtonAction::new("outline.moveDown", json!({}), false));
                small(ui, app, "outlineExpand", None, "Expand", crate::widgets::ButtonAction::new("outline.expand", json!({}), false));
                small(ui, app, "outlineCollapse", None, "Collapse", crate::widgets::ButtonAction::new("outline.collapse", json!({}), false));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tl!("Show Level")).size(11.5));
                let mut shown: Vec<String> = vec![tl!("All Levels").to_string()];
                shown.extend((1..BODY).map(name));
                let current = shown.get(usize::from(o.show_level)).cloned().unwrap_or_default();
                if let Some(v) = crate::widgets::combo_hint(ui, "outlineShowLevel", 90.0, &current, &shown, None, tl!("Show Level"))
                    && let Some(i) = shown.iter().position(|x| *x == v)
                {
                    let l = if i == 0 { json!("all") } else { json!(i) };
                    let _ = app.run("outline.showLevel", json!({ "level": l }));
                }
            });
            ui.add_space(3.0);
            crate::widgets::row(ui, |ui| {
                let mut f = !o.hide_formatting;
                if ui.checkbox(&mut f, tl!("Show Text Formatting")).changed() {
                    let _ = app.run("outline.showFormatting", json!({ "value": f }));
                }
                let mut first = o.first_line_only;
                if ui.checkbox(&mut first, tl!("Show First Line Only")).changed() {
                    let _ = app.run("outline.firstLineOnly", json!({ "value": first }));
                }
            });
        });
    });
    group(ui, "Close", None, app, |ui, app| {
        big(ui, app, "closeOutline", "Close Outline\nView", "outline.close", json!({}), false);
    });
}
