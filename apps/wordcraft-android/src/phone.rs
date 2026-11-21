//! The phone layout: on a small screen the apps' own title bars and ribbons are too dense to use
//! with a finger. The suite draws instead a slim top bar (Back, the document's name, Undo, Redo
//! and a menu) and a floating tool bar at the bottom with the few things most edits need
//! (Format, Paragraph, Picture, Find, Keyboard). The full ribbon is one tap away ("Ribbon" in the
//! tool bar, or Advanced ribbon in the menu) for everything else.
//!
//! This module only draws and reports what was tapped ([`Act`]); `suite.rs` carries it out on the
//! app showing.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::suite::AppKind;

/// A window whose shorter side is under this many points is a phone (portrait or landscape).
pub const PHONE_SIDE: f32 = 600.0;

/// Whether a window of this size gets the phone layout.
pub fn is_phone(size: egui::Vec2) -> bool {
    size.x.min(size.y) < PHONE_SIDE
}

/// Height of the phone top bar (points).
pub const TOP_BAR_H: f32 = 48.0;

/// What a tap in the phone bars asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Back to the suite's home screen.
    Home,
    Undo,
    Redo,
    Save,
    /// The app's File view (open, save as, print, export…).
    File,
    Find,
    /// Show or hide the full ribbon.
    Ribbon,
    /// Bring the soft keyboard up at the caret.
    Keyboard,
    /// Leave reading and edit the document.
    Edit,
    /// Read the document: text wrapped to the screen, typing kept out.
    Read,
    /// Show another app.
    Switch(AppKind),
    /// Run one of the app's commands.
    Cmd(&'static str, serde_json::Value),
}

/// The bottom sheets the tool bar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    Format,
    Paragraph,
}

/// The phone layout's own state.
#[derive(Default)]
pub struct PhoneUi {
    /// The full ribbon shows (otherwise only the minimal tool bar).
    pub advanced: bool,
    /// The sheet open over the tool bar.
    pub sheet: Option<Sheet>,
    /// The top bar's menu is open.
    menu: bool,
    /// Writer's document shows for reading (the tool bar's Read): text wrapped to the screen
    /// (Web Layout), and typing and the keyboard kept out until Edit.
    pub reading: bool,
}

/// What the bars show about the app.
pub struct BarInfo {
    pub kind: AppKind,
    pub title: String,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Writer's document shows for reading: the tool bar offers Edit instead of formatting.
    pub read_mode: bool,
    /// Hide the tool bar (a dialog, the File view, the start screen).
    pub hide_tools: bool,
    /// Writer's character formatting at the caret (bold, italic, underline, strike).
    pub marks: [bool; 4],
}

/// Colours of the phone bars.
struct Ink {
    bar: Color32,
    text: Color32,
    dim: Color32,
    line: Color32,
    press: Color32,
}

impl Ink {
    fn of(dark: bool) -> Ink {
        if dark {
            Ink {
                bar: Color32::from_rgb(0x1B, 0x1D, 0x22),
                text: Color32::from_gray(236),
                dim: Color32::from_gray(110),
                line: Color32::from_white_alpha(20),
                press: Color32::from_white_alpha(26),
            }
        } else {
            Ink {
                bar: Color32::WHITE,
                text: Color32::from_rgb(0x1E, 0x21, 0x27),
                dim: Color32::from_rgb(0xB4, 0xB8, 0xC0),
                line: Color32::from_black_alpha(22),
                press: Color32::from_black_alpha(18),
            }
        }
    }
}

/// The floating tool bar is dark in both themes (it floats over a white page).
const PILL: Color32 = Color32::from_rgb(0x2B, 0x2E, 0x34);
const PILL_INK: Color32 = Color32::from_gray(240);
const PILL_ON: Color32 = Color32::from_rgb(0x4C, 0x6E, 0xF5);

impl PhoneUi {
    /// The top bar. Returns what was tapped.
    pub fn top_bar(&mut self, ui: &mut egui::Ui, info: &BarInfo) -> Option<Act> {
        let ink = Ink::of(ui.visuals().dark_mode);
        let mut act = None;
        let mut menu_button = Rect::NOTHING;
        egui::Panel::top("phone_bar").exact_size(TOP_BAR_H).frame(egui::Frame::NONE.fill(ink.bar)).show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment([full.left_bottom(), full.right_bottom()], Stroke::new(1.0, ink.line));
            let y = full.center().y;
            let size = vec2(44.0, 40.0);
            // Back (to the home screen).
            let back = Rect::from_center_size(pos2(full.min.x + 26.0, y), size);
            if icon_button(ui, back, "phone_back", "Back to WorkSuite home", true, &ink, paint_back) {
                act = Some(Act::Home);
            }
            // Right side: menu, redo, undo.
            let menu = Rect::from_center_size(pos2(full.max.x - 26.0, y), size);
            menu_button = menu;
            if icon_button(ui, menu, "phone_menu", "More", true, &ink, paint_menu) {
                self.menu = !self.menu;
            }
            let redo = menu.translate(vec2(-44.0, 0.0));
            if icon_button(ui, redo, "phone_redo", "Redo", info.can_redo, &ink, |p, r, c| paint_undo(p, r, c, true)) {
                act = Some(Act::Redo);
            }
            let undo = redo.translate(vec2(-44.0, 0.0));
            if icon_button(ui, undo, "phone_undo", "Undo", info.can_undo, &ink, |p, r, c| paint_undo(p, r, c, false)) {
                act = Some(Act::Undo);
            }
            // The app's icon and the document's name, cut short to fit.
            let icon = Rect::from_center_size(pos2(back.max.x + 14.0, y), vec2(20.0, 20.0));
            crate::suite::paint_app_icon(ui.painter(), icon, info.kind);
            let x = icon.max.x + 10.0;
            let room = (undo.min.x - 8.0 - x).max(20.0);
            let mut job = egui::text::LayoutJob::simple_singleline(info.title.clone(), FontId::proportional(16.0), ink.text);
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(pos2(x, y - galley.size().y / 2.0), galley, ink.text);
        });
        if self.menu
            && let Some(a) = self.menu_popup(ui.ctx(), info, menu_button, &ink)
        {
            act = Some(a);
        }
        act
    }

    /// The top bar's menu: File, Save, Find, Advanced ribbon, the other apps.
    fn menu_popup(&mut self, ctx: &egui::Context, info: &BarInfo, anchor: Rect, ink: &Ink) -> Option<Act> {
        let mut act = None;
        let mut rows: Vec<(String, Act, bool)> = vec![("File (open, save as, print…)".into(), Act::File, false), ("Save".into(), Act::Save, false)];
        if info.kind == AppKind::Writer {
            rows.push(("Find".into(), Act::Find, false));
        }
        rows.push(("Advanced ribbon".into(), Act::Ribbon, self.advanced));
        for kind in AppKind::ALL {
            if kind != info.kind {
                rows.push((format!("Switch to {}", kind.label()), Act::Switch(kind), false));
            }
        }
        rows.push(("WorkSuite home".into(), Act::Home, false));
        let width = 248.0;
        let pos = pos2((anchor.max.x - width).max(8.0), anchor.max.y + 4.0);
        let area = egui::Area::new(egui::Id::unique("phone_menu_popup")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(ink.bar)
                .corner_radius(CornerRadius::same(14))
                .stroke(Stroke::new(1.0, ink.line))
                .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(60) })
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (label, a, checked) in rows {
                        let (r, resp) = ui.allocate_exact_size(vec2(width - 12.0, 44.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, checked, &label));
                        if resp.is_pointer_button_down_on() || resp.hovered() {
                            ui.painter().rect_filled(r, 10.0, ink.press);
                        }
                        ui.painter().text(pos2(r.min.x + 14.0, r.center().y), Align2::LEFT_CENTER, &label, FontId::proportional(15.0), ink.text);
                        if checked {
                            paint_check(ui.painter(), Rect::from_center_size(pos2(r.max.x - 20.0, r.center().y), vec2(16.0, 16.0)), PILL_ON);
                        }
                        if resp.clicked() {
                            act = Some(a);
                        }
                    }
                });
        });
        // A tap outside the menu (but not on its button, which toggles it) closes it.
        let clicked_elsewhere = ctx
            .input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p)));
        if act.is_some() || clicked_elsewhere {
            self.menu = false;
        }
        act
    }

    /// The floating tool bar at the bottom, and the sheet it opened. Returns what was tapped.
    pub fn tool_bar(&mut self, ctx: &egui::Context, info: &BarInfo) -> Option<Act> {
        if info.hide_tools {
            self.sheet = None;
            return None;
        }
        let screen = ctx.content_rect();
        let mut items: Vec<(&'static str, Tool)> = Vec::new();
        if info.read_mode {
            items.push(("Edit", Tool::Edit));
        } else {
            items.push(("Format", Tool::Sheet(Sheet::Format)));
            items.push((if info.kind == AppKind::Sheets { "Align" } else { "Paragraph" }, Tool::Sheet(Sheet::Paragraph)));
            items.push(("Picture", Tool::Picture));
        }
        if info.kind == AppKind::Writer {
            items.push(("Find", Tool::Find));
            if !info.read_mode {
                items.push(("Keyboard", Tool::Keyboard));
                items.push(("Read", Tool::Read));
            }
        }
        items.push(("Ribbon", Tool::Ribbon));
        let item_w = 58.0;
        let pad = 10.0;
        let width = (items.len() as f32 * item_w + 2.0 * pad).min(screen.width() - 16.0);
        let height = 56.0;
        let pill = Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.max.y - height - 34.0), vec2(width, height));
        let item_w = (width - 2.0 * pad) / items.len() as f32;
        let mut act = None;
        egui::Area::new(egui::Id::unique("phone_tools")).order(egui::Order::Foreground).fixed_pos(pill.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(pill.size(), Sense::hover());
            let p = ui.painter();
            p.add(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(r, CornerRadius::same(28)));
            p.rect_filled(r, 28.0, PILL);
            for (i, (label, tool)) in items.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(r.min.x + pad + i as f32 * item_w, r.min.y + 3.0), vec2(item_w, height - 6.0));
                let resp = ui.interact(cell, egui::Id::unique(("phone_tool", *label)), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *label));
                let on = match tool {
                    Tool::Sheet(s) => self.sheet == Some(*s),
                    Tool::Ribbon => self.advanced,
                    _ => false,
                };
                if on {
                    p.rect_filled(cell.shrink2(vec2(2.0, 0.0)), 14.0, PILL_ON.gamma_multiply(0.85));
                } else if resp.is_pointer_button_down_on() {
                    p.rect_filled(cell.shrink2(vec2(2.0, 0.0)), 14.0, Color32::from_white_alpha(24));
                }
                let icon = Rect::from_center_size(pos2(cell.center().x, cell.min.y + 17.0), vec2(22.0, 22.0));
                paint_tool(p, icon, *tool, PILL_INK);
                let size = (item_w / 5.0).clamp(9.0, 11.0);
                p.text(pos2(cell.center().x, cell.max.y - 9.0), Align2::CENTER_CENTER, *label, FontId::proportional(size), PILL_INK);
                if resp.clicked() {
                    match tool {
                        Tool::Sheet(s) => self.sheet = if self.sheet == Some(*s) { None } else { Some(*s) },
                        Tool::Edit => act = Some(Act::Edit),
                        Tool::Read => act = Some(Act::Read),
                        Tool::Picture => act = Some(Act::Cmd("insert.picture", serde_json::json!({}))),
                        Tool::Find => act = Some(Act::Find),
                        Tool::Keyboard => act = Some(Act::Keyboard),
                        Tool::Ribbon => {
                            self.sheet = None;
                            act = Some(Act::Ribbon);
                        }
                    }
                }
            }
        });
        if info.read_mode {
            self.sheet = None;
        }
        if let Some(sheet) = self.sheet
            && let Some(a) = sheet_panel(ctx, sheet, info, pill)
        {
            act = Some(a);
        }
        act
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Sheet(Sheet),
    Edit,
    Read,
    Picture,
    Find,
    Keyboard,
    Ribbon,
}

/// One button in a sheet: what it draws and the command it runs.
struct Key {
    face: Face,
    tip: &'static str,
    id: &'static str,
    params: serde_json::Value,
    on: bool,
}

#[derive(Clone, Copy)]
enum Face {
    Bold,
    Italic,
    Underline,
    Strike,
    Smaller,
    Larger,
    Color,
    Highlight,
    Align(u8),
    Bullets,
    Numbers,
    Outdent,
    Indent,
}

/// The commands of a sheet, for the app showing (each app names them its own way).
fn keys(sheet: Sheet, info: &BarInfo) -> Vec<Key> {
    use serde_json::json;
    let k = |face, tip, id, params, on| Key { face, tip, id, params, on };
    let m = info.marks;
    match (sheet, info.kind) {
        (Sheet::Format, AppKind::Writer) => vec![
            k(Face::Bold, "Bold", "format.bold", json!({}), m[0]),
            k(Face::Italic, "Italic", "format.italic", json!({}), m[1]),
            k(Face::Underline, "Underline", "format.underline", json!({}), m[2]),
            k(Face::Strike, "Strikethrough", "format.strikethrough", json!({}), m[3]),
            k(Face::Smaller, "Smaller text", "format.shrinkFont", json!({}), false),
            k(Face::Larger, "Bigger text", "format.growFont", json!({}), false),
            // The colours last used (the ribbon's split buttons), filled in by the suite.
            k(Face::Color, "Font colour", "format.color", json!({}), false),
            k(Face::Highlight, "Highlight", "format.highlight", json!({}), false),
        ],
        (Sheet::Paragraph, AppKind::Writer) => vec![
            k(Face::Align(0), "Align left", "para.alignLeft", json!({}), false),
            k(Face::Align(1), "Centre", "para.alignCenter", json!({}), false),
            k(Face::Align(2), "Align right", "para.alignRight", json!({}), false),
            k(Face::Align(3), "Justify", "para.justify", json!({}), false),
            k(Face::Bullets, "Bullets", "para.bullets", json!({}), false),
            k(Face::Numbers, "Numbering", "para.numbering", json!({}), false),
            k(Face::Outdent, "Decrease indent", "para.outdent", json!({}), false),
            k(Face::Indent, "Increase indent", "para.indent", json!({}), false),
        ],
        (Sheet::Format, AppKind::Slides) => vec![
            k(Face::Bold, "Bold", "format.bold", json!({}), false),
            k(Face::Italic, "Italic", "format.italic", json!({}), false),
            k(Face::Underline, "Underline", "format.underline", json!({}), false),
            k(Face::Color, "Font colour", "format.color", json!({"color": "#C00000"}), false),
        ],
        (Sheet::Paragraph, AppKind::Slides) => vec![
            k(Face::Align(0), "Align left", "format.alignLeft", json!({}), false),
            k(Face::Align(1), "Centre", "format.alignCenter", json!({}), false),
            k(Face::Align(2), "Align right", "format.alignRight", json!({}), false),
            k(Face::Bullets, "Bullets", "format.bullets", json!({}), false),
            k(Face::Numbers, "Numbering", "format.numbering", json!({}), false),
        ],
        (Sheet::Format, AppKind::Sheets) => vec![
            k(Face::Bold, "Bold", "home.bold", json!({}), false),
            k(Face::Italic, "Italic", "home.italic", json!({}), false),
            k(Face::Underline, "Underline", "home.underline", json!({}), false),
        ],
        (Sheet::Paragraph, AppKind::Sheets) => vec![
            k(Face::Align(0), "Align left", "home.alignLeft", json!({}), false),
            k(Face::Align(1), "Centre", "home.alignCenter", json!({}), false),
            k(Face::Align(2), "Align right", "home.alignRight", json!({}), false),
        ],
    }
}

/// The sheet over the tool bar: a row (or two) of large buttons.
fn sheet_panel(ctx: &egui::Context, sheet: Sheet, info: &BarInfo, pill: Rect) -> Option<Act> {
    let keys = keys(sheet, info);
    let screen = ctx.content_rect();
    let key = 46.0;
    let gap = 6.0;
    let pad = 10.0;
    let width = (screen.width() - 16.0).min(pad * 2.0 + keys.len() as f32 * (key + gap) - gap);
    let per_row = (((width - pad * 2.0 + gap) / (key + gap)).floor() as usize).max(1);
    let rows = keys.len().div_ceil(per_row);
    let height = pad * 2.0 + rows as f32 * key + (rows as f32 - 1.0) * gap;
    let rect = Rect::from_min_size(pos2(screen.center().x - width / 2.0, pill.min.y - height - 8.0), vec2(width, height));
    let mut act = None;
    egui::Area::new(egui::Id::unique("phone_sheet")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
        let p = ui.painter();
        p.add(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(r, CornerRadius::same(18)));
        p.rect_filled(r, 18.0, PILL);
        for (i, k) in keys.into_iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let cell = Rect::from_min_size(pos2(r.min.x + pad + col as f32 * (key + gap), r.min.y + pad + row as f32 * (key + gap)), vec2(key, key));
            let resp = ui.interact(cell, egui::Id::unique(("phone_key", k.tip)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, k.on, k.tip));
            let fill = if k.on {
                PILL_ON
            } else if resp.is_pointer_button_down_on() {
                Color32::from_white_alpha(34)
            } else {
                Color32::from_white_alpha(12)
            };
            p.rect_filled(cell, 12.0, fill);
            paint_face(p, cell, k.face, PILL_INK);
            if resp.clicked() {
                act = Some(Act::Cmd(k.id, k.params));
            }
        }
    });
    act
}

/// A round icon button in the top bar. Returns whether it was tapped.
fn icon_button(ui: &mut egui::Ui, r: Rect, id: &str, tip: &str, enabled: bool, ink: &Ink, paint: impl Fn(&egui::Painter, Rect, Color32)) -> bool {
    let resp = ui.interact(r, egui::Id::unique(id), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, 12.0, ink.press);
    }
    paint(ui.painter(), Rect::from_center_size(r.center(), vec2(22.0, 22.0)), if enabled { ink.text } else { ink.dim });
    enabled && resp.clicked()
}

fn stroke(c: Color32) -> Stroke {
    Stroke::new(1.8, c)
}

fn paint_back(p: &egui::Painter, r: Rect, c: Color32) {
    let s = stroke(c);
    let m = r.center();
    p.line_segment([pos2(m.x + 4.0, r.min.y + 3.0), pos2(m.x - 5.0, m.y)], s);
    p.line_segment([pos2(m.x - 5.0, m.y), pos2(m.x + 4.0, r.max.y - 3.0)], s);
}

fn paint_menu(p: &egui::Painter, r: Rect, c: Color32) {
    let s = stroke(c);
    for dy in [-6.0, 0.0, 6.0] {
        p.line_segment([pos2(r.min.x + 3.0, r.center().y + dy), pos2(r.max.x - 3.0, r.center().y + dy)], s);
    }
}

/// A curved arrow: Undo points left, Redo (`mirror`) right.
fn paint_undo(p: &egui::Painter, r: Rect, c: Color32, mirror: bool) {
    let m = r.center();
    let f = |x: f32| if mirror { 2.0 * m.x - x } else { x };
    // The arc: half a circle on the right, from the arrow's tail to its head.
    let (cx, cy, rad) = (m.x + 1.0, m.y + 2.0, 6.5);
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
            pos2(f(cx + rad * a.cos()), cy + rad * a.sin())
        })
        .collect();
    let start = pos2(f(cx - 7.0), cy - rad);
    let mut path = vec![pos2(f(cx - 2.0), cy + rad)];
    path.extend(pts.into_iter().rev());
    path.push(start);
    p.add(egui::Shape::line(path, stroke(c)));
    // The head at the start.
    p.line_segment([start, pos2(f(cx - 3.0), cy - rad - 4.0)], stroke(c));
    p.line_segment([start, pos2(f(cx - 3.0), cy - rad + 4.0)], stroke(c));
}

fn paint_check(p: &egui::Painter, r: Rect, c: Color32) {
    let s = Stroke::new(2.2, c);
    p.line_segment([pos2(r.min.x + 1.0, r.center().y), pos2(r.min.x + 6.0, r.max.y - 2.0)], s);
    p.line_segment([pos2(r.min.x + 6.0, r.max.y - 2.0), pos2(r.max.x - 1.0, r.min.y + 2.0)], s);
}

fn paint_lines(p: &egui::Painter, r: Rect, c: Color32, align: u8) {
    let s = stroke(c);
    for (i, w) in [1.0, 0.65, 1.0, 0.65].into_iter().enumerate() {
        let w = if align == 3 { 1.0 } else { w } * r.width();
        let y = r.min.y + 3.0 + i as f32 * (r.height() - 6.0) / 3.0;
        let x0 = match align {
            1 => r.center().x - w / 2.0,
            2 => r.max.x - w,
            _ => r.min.x,
        };
        p.line_segment([pos2(x0, y), pos2(x0 + w, y)], s);
    }
}

fn paint_tool(p: &egui::Painter, r: Rect, tool: Tool, c: Color32) {
    let s = stroke(c);
    match tool {
        Tool::Sheet(Sheet::Format) => {
            p.rect_stroke(r.shrink(1.0), 3.0, s, StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, "T", FontId::proportional(14.0), c);
        }
        Tool::Sheet(Sheet::Paragraph) => paint_lines(p, r.shrink(1.0), c, 0),
        Tool::Picture => {
            let b = r.shrink(1.0);
            p.rect_stroke(b, 3.0, s, StrokeKind::Inside);
            p.circle_stroke(pos2(b.min.x + 6.0, b.min.y + 6.0), 2.0, s);
            p.add(egui::Shape::line(
                vec![
                    pos2(b.min.x + 2.0, b.max.y - 3.0),
                    pos2(b.min.x + 8.0, b.center().y + 1.0),
                    pos2(b.min.x + 12.0, b.max.y - 5.0),
                    pos2(b.max.x - 6.0, b.center().y - 1.0),
                    pos2(b.max.x - 2.0, b.max.y - 4.0),
                ],
                s,
            ));
        }
        Tool::Find => {
            let m = r.center() + vec2(-2.0, -2.0);
            p.circle_stroke(m, 6.5, s);
            p.line_segment([m + vec2(4.8, 4.8), r.max - vec2(1.5, 1.5)], Stroke::new(2.2, c));
        }
        Tool::Keyboard => {
            let b = Rect::from_center_size(r.center(), vec2(r.width(), r.height() * 0.7));
            p.rect_stroke(b, 3.0, s, StrokeKind::Inside);
            for row in 0..2 {
                for col in 0..4 {
                    p.circle_filled(pos2(b.min.x + 4.5 + col as f32 * 4.3, b.min.y + 5.0 + row as f32 * 4.0), 1.0, c);
                }
            }
            p.line_segment([pos2(b.min.x + 6.0, b.max.y - 3.5), pos2(b.max.x - 6.0, b.max.y - 3.5)], s);
        }
        Tool::Ribbon => {
            let q = (r.width() - 4.0) / 2.0;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let cell = Rect::from_min_size(pos2(r.min.x + 1.0 + dx * (q + 2.0), r.min.y + 1.0 + dy * (q + 2.0)), vec2(q, q));
                p.rect_stroke(cell, 3.0, s, StrokeKind::Inside);
            }
        }
        Tool::Read => {
            // An open book.
            let s = Stroke::new(1.8, c);
            let mid = r.center_bottom() - vec2(0.0, 3.0);
            let top = pos2(r.center().x, r.min.y + 5.0);
            p.line_segment([top, mid], s);
            for side in [-1.0f32, 1.0] {
                let outer_top = pos2(r.center().x + side * (r.width() / 2.0 - 1.0), r.min.y + 3.0);
                let outer_bot = pos2(outer_top.x, r.max.y - 5.0);
                p.line_segment([top, outer_top], s);
                p.line_segment([outer_top, outer_bot], s);
                p.line_segment([outer_bot, mid], s);
            }
        }
        Tool::Edit => {
            // A pencil.
            let a = pos2(r.min.x + 3.0, r.max.y - 3.0);
            let b = pos2(r.max.x - 4.0, r.min.y + 4.0);
            p.line_segment([a, b], Stroke::new(4.0, c));
            p.line_segment([a, a + vec2(-1.5, 1.5)], Stroke::new(2.0, c));
        }
    }
}

fn paint_face(p: &egui::Painter, cell: Rect, face: Face, c: Color32) {
    let r = Rect::from_center_size(cell.center(), vec2(22.0, 22.0));
    let glyph = |text: &str, italics: bool, underline: bool, strike: bool, size: f32| {
        let mut fmt = egui::TextFormat::simple(FontId::proportional(size), c);
        fmt.italics = italics;
        if underline {
            fmt.underline = Stroke::new(1.6, c);
        }
        if strike {
            fmt.strikethrough = Stroke::new(1.6, c);
        }
        let mut job = egui::text::LayoutJob::default();
        job.append(text, 0.0, fmt);
        let g = p.layout_job(job);
        p.galley(cell.center() - g.size() / 2.0, g, c);
    };
    match face {
        Face::Bold => {
            // Drawn twice, a hair apart, for weight without a bold face.
            let g = p.layout_no_wrap("B".into(), FontId::proportional(20.0), c);
            let at = cell.center() - g.size() / 2.0;
            p.galley(at, g.clone(), c);
            p.galley(at + vec2(0.9, 0.0), g, c);
        }
        Face::Italic => glyph("I", true, false, false, 20.0),
        Face::Underline => glyph("U", false, true, false, 19.0),
        Face::Strike => glyph("ab", false, false, true, 17.0),
        Face::Smaller => glyph("A-", false, false, false, 15.0),
        Face::Larger => glyph("A+", false, false, false, 19.0),
        Face::Color | Face::Highlight => {
            let swatch = if matches!(face, Face::Color) { Color32::from_rgb(0xE5, 0x39, 0x35) } else { Color32::from_rgb(0xFF, 0xE0, 0x3C) };
            if matches!(face, Face::Highlight) {
                p.rect_filled(Rect::from_center_size(cell.center() - vec2(0.0, 3.0), vec2(20.0, 16.0)), 3.0, swatch.gamma_multiply(0.45));
            }
            p.text(cell.center() - vec2(0.0, 3.0), Align2::CENTER_CENTER, "A", FontId::proportional(18.0), c);
            p.rect_filled(Rect::from_center_size(pos2(cell.center().x, cell.max.y - 10.0), vec2(22.0, 4.0)), 1.0, swatch);
        }
        Face::Align(a) => paint_lines(p, r, c, a),
        Face::Bullets | Face::Numbers => {
            let s = stroke(c);
            for i in 0..3 {
                let y = r.min.y + 4.0 + i as f32 * 7.0;
                if matches!(face, Face::Bullets) {
                    p.circle_filled(pos2(r.min.x + 2.5, y), 2.0, c);
                } else {
                    p.text(pos2(r.min.x + 2.5, y), Align2::CENTER_CENTER, ["1", "2", "3"][i], FontId::proportional(8.5), c);
                }
                p.line_segment([pos2(r.min.x + 8.0, y), pos2(r.max.x, y)], s);
            }
        }
        Face::Outdent | Face::Indent => {
            let s = stroke(c);
            for i in 0..4 {
                let y = r.min.y + 3.0 + i as f32 * 5.3;
                let x0 = if i == 1 || i == 2 { r.min.x + 9.0 } else { r.min.x };
                p.line_segment([pos2(x0, y), pos2(r.max.x, y)], s);
            }
            let (tip, tail) = if matches!(face, Face::Indent) { (r.min.x + 6.0, r.min.x) } else { (r.min.x, r.min.x + 6.0) };
            let y = r.center().y;
            p.line_segment([pos2(tail, y - 3.5), pos2(tip, y)], s);
            p.line_segment([pos2(tip, y), pos2(tail, y + 3.5)], s);
        }
    }
}
