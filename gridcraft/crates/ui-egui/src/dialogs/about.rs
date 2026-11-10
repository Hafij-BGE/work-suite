use super::Dialog;

/// Help ▸ About: the app, its version and who made it, with the licence's copyright notice.
pub(super) fn about(ui: &mut egui::Ui, _d: &mut Dialog) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
        paint_worksuite_mark(ui.painter(), r);
        ui.vertical(|ui| {
            ui.heading(crate::APP_NAME);
            ui.label(format!("Version {}", option_env!("WORDCRAFT_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))));
        });
    });
    ui.add_space(6.0);
    ui.label("Developed by Md. Hafijur Rahman. Part of WorkSuite.");
    ui.hyperlink_to("Source code: github.com/Hafij-BGE/work-suite", "https://github.com/Hafij-BGE/work-suite");
    ui.add_space(6.0);
    // The licence asks that its copyright notice go with every copy.
    ui.label(
        egui::RichText::new("Free and open source (MIT OR Apache-2.0). Based on GridCraft by the ArtCraft Team (https://github.com/storytold/gridcraft). Copyright (c) 2026 ArtCraft Team and the GridCraft contributors. See NOTICE and THIRD_PARTY_NOTICES.md.")
            .small()
            .weak(),
    );
    ui.add_space(4.0);
    ui.label(egui::RichText::new("This app was generated with AI. Developer support may not be available.").small().weak());
}

/// WorkSuite's mark, as on its launcher icon: three app cards fanned out on slate (Sheets green,
/// Slides orange, Writer's page in front).
fn paint_worksuite_mark(p: &egui::Painter, r: egui::Rect) {
    use egui::{Color32, Stroke, pos2, vec2};
    let k = r.width() / 56.0;
    p.rect_filled(r, 14.0 * k, Color32::from_rgb(0x1F, 0x29, 0x37));
    let card = |x: f32, y: f32| egui::Rect::from_min_size(r.min + vec2(x * k, y * k), vec2(22.0 * k, 28.0 * k));
    p.rect_filled(card(9.0, 15.0), 3.0 * k, Color32::from_rgb(0x1E, 0x8A, 0x4C));
    p.rect_filled(card(25.0, 12.0), 3.0 * k, Color32::from_rgb(0xD9, 0x5B, 0x2B));
    let page = card(17.0, 18.0);
    p.rect_filled(page, 3.0 * k, Color32::WHITE);
    for (i, w) in [14.0_f32, 14.0, 8.4].into_iter().enumerate() {
        let y = page.min.y + (8.0 + 6.0 * i as f32) * k;
        let x0 = page.min.x + 4.0 * k;
        p.line_segment([pos2(x0, y), pos2(x0 + w * k, y)], Stroke::new(2.2 * k, Color32::from_rgb(0x2B, 0x5C, 0xD6)));
    }
}
