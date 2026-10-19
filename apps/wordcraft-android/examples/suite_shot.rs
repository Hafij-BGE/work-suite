//! Screenshots of the suite without a GPU or a phone: runs a few frames headless and draws egui's
//! output with a small software rasteriser, as a binary PPM image.
//!
//! `cargo run -p wordcraft-android --example suite_shot -- out.ppm [home|writer|slides|sheets] [dark|light] [width height scale] [ribbon tab]`
//!
//! With a ribbon tab, the app shows its built-in sample (Writer's handbook, Slides' sample deck,
//! Sheets' first sample workbook).
//!
//! The window has a 24-point status bar and a 16-point gesture bar, as a phone does.

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, TextureId, pos2, vec2};
use wordcraft_android::suite::{AppKind, Picker, Suite, SuitePaths};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "suite.ppm".into());
    let app = match args.get(1).map(String::as_str) {
        Some("writer") => Some(AppKind::Writer),
        Some("slides") => Some(AppKind::Slides),
        Some("sheets") => Some(AppKind::Sheets),
        _ => None,
    };
    let dark = args.get(2).is_some_and(|a| a == "dark");
    let num = |i: usize, d: f32| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(d);
    let (w, h, ppp) = (num(3, 412.0), num(4, 892.0), num(5, 2.0));

    let ctx = egui::Context::default();
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut suite = Suite::new(Picker::new(None), SuitePaths::default());
    let window = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
    let (wp, hp) = ((w * ppp) as i32, (h * ppp) as i32);
    suite.set_insets(Some([0, (24.0 * ppp) as i32, 0, (16.0 * ppp) as i32, wp, hp]));
    if app.is_some() {
        suite.switch(&ctx, app);
    }
    // A ribbon tab to show, over the app's sample.
    if let Some(tab) = args.get(6) {
        if let Some(w) = &mut suite.writer {
            let _ = w.word.run("file.new", serde_json::json!({"template": "sample"}));
            w.word.ui.backstage = false;
            w.word.ui.tab = tab.clone();
        }
        if let Some(s) = &mut suite.slides {
            if deckcraft_engine::sample::open_sample(&mut s.session).is_err() {
                let _ = s.run("file.new", serde_json::json!({}));
            }
            s.ui.tab = tab.clone();
        }
        if let Some(s) = &mut suite.sheets {
            if let Some((name, _)) = gridcraft_engine::sample::SAMPLES.first() {
                let _ = s.run("file.new", serde_json::json!({ "sample": name }));
            }
            s.ui.ribbon_tab = tab.clone();
        }
    }
    let mut textures: HashMap<TextureId, ([usize; 2], Vec<Color32>)> = HashMap::new();
    let mut last = None;
    for _ in 0..8 {
        let mut raw = egui::RawInput { screen_rect: Some(window), ..Default::default() };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        suite.raw_input(&ctx, &mut raw);
        let mut output = ctx.run_ui(raw, |ui| {
            suite.logic(ui.ctx());
            suite.ui(ui);
        });
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                let egui::ImageData::Color(img) = &delta.image;
                match delta.pos {
                    None => {
                        textures.insert(*id, (img.size, img.pixels.clone()));
                    }
                    Some([x0, y0]) => {
                        if let Some((size, px)) = textures.get_mut(id) {
                            for y in 0..img.size[1] {
                                for x in 0..img.size[0] {
                                    if let Some(p) = px.get_mut((y0 + y) * size[0] + x0 + x) {
                                        *p = img.pixels[y * img.size[0] + x];
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Applied above (egui checks that a frame's textures were handed on).
        output.textures_delta.clear();
        last = Some(output);
    }
    let Some(output) = last else { return };
    let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
    let clear = suite.chrome;
    let mut img = vec![clear; (wp * hp) as usize];
    for p in prims {
        let egui::epaint::Primitive::Mesh(mesh) = p.primitive else { continue };
        let clip = Rect::from_min_max(pos2(p.clip_rect.min.x * ppp, p.clip_rect.min.y * ppp), pos2(p.clip_rect.max.x * ppp, p.clip_rect.max.y * ppp));
        let tex = textures.get(&mesh.texture_id);
        for tri in mesh.indices.chunks_exact(3) {
            let v = [&mesh.vertices[tri[0] as usize], &mesh.vertices[tri[1] as usize], &mesh.vertices[tri[2] as usize]];
            let pt = |i: usize| pos2(v[i].pos.x * ppp, v[i].pos.y * ppp);
            let (a, b, c) = (pt(0), pt(1), pt(2));
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = a.x.min(b.x).min(c.x).max(clip.min.x).max(0.0).floor() as i32;
            let x1 = a.x.max(b.x).max(c.x).min(clip.max.x).min(wp as f32).ceil() as i32;
            let y0 = a.y.min(b.y).min(c.y).max(clip.min.y).max(0.0).floor() as i32;
            let y1 = a.y.max(b.y).max(c.y).min(clip.max.y).min(hp as f32).ceil() as i32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let q = pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.x - q.x) * (c.y - q.y) - (b.y - q.y) * (c.x - q.x)) / area;
                    let w1 = ((c.x - q.x) * (a.y - q.y) - (c.y - q.y) * (a.x - q.x)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let mix = |f: fn(Color32) -> u8| w0 * f(v[0].color) as f32 + w1 * f(v[1].color) as f32 + w2 * f(v[2].color) as f32;
                    let mut src = [mix(|c| c.r()), mix(|c| c.g()), mix(|c| c.b()), mix(|c| c.a())];
                    if let Some((size, px)) = tex {
                        let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                        let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                        let tx = ((u * size[0] as f32) as usize).min(size[0] - 1);
                        let ty = ((t * size[1] as f32) as usize).min(size[1] - 1);
                        let s = px[ty * size[0] + tx];
                        for (k, ch) in [s.r(), s.g(), s.b(), s.a()].into_iter().enumerate() {
                            src[k] *= ch as f32 / 255.0;
                        }
                    }
                    let d = &mut img[(y * wp + x) as usize];
                    let inv = 1.0 - src[3] / 255.0;
                    let blend = |s: f32, d: u8| (s + d as f32 * inv).clamp(0.0, 255.0) as u8;
                    *d = Color32::from_rgb(blend(src[0], d.r()), blend(src[1], d.g()), blend(src[2], d.b()));
                }
            }
        }
    }
    let mut bytes = format!("P6\n{wp} {hp}\n255\n").into_bytes();
    for p in img {
        bytes.extend([p.r(), p.g(), p.b()]);
    }
    if let Err(e) = std::fs::write(&out, bytes) {
        eprintln!("{out}: {e}");
    }
}
