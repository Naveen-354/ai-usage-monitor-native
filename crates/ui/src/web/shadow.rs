//! Real `text-shadow`: the text is rasterised into a coverage mask from the font's own outlines, blurred with a true
//! Gaussian per shadow layer, composited the way CSS stacks shadows (first layer on top) and drawn as a cached texture
//! under the crisp text. (Drawing many offset copies instead leaves visible ghosts.)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ab_glyph::{point, Font as _, PxScale};
use egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};

use super::text::{advance, font_for, Run, Shadow, Style};

/// A baked shadow image and where it sits relative to the run's top-left (left edge, glyph-row top).
pub struct Baked {
    pub image: ColorImage,
    pub origin: Vec2,
}

/// Separable Gaussian blur of a single-channel image (edges are treated as empty). `sigma` in px.
pub fn gaussian_blur(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 0.05 {
        return src.to_vec();
    }
    let radius = (sigma * 3.0).ceil() as i32;
    let mut k: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);

    let mut tmp = vec![0.0_f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sx = x as i32 + i as i32 - radius;
                if (0..w as i32).contains(&sx) {
                    acc += src[y * w + sx as usize] * kv;
                }
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0.0_f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sy = y as i32 + i as i32 - radius;
                if (0..h as i32).contains(&sy) {
                    acc += tmp[sy as usize * w + x] * kv;
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Rasterises `text` (positioned exactly as [`Run`] positions it) into coverage 0..1 on a `w x h` canvas whose text
/// origin (left edge, top of the glyph rows) is at (`pad`, `pad`).
fn coverage(text: &str, style: &Style, w: usize, h: usize, pad: f32) -> Vec<f32> {
    let font = font_for(style);
    let upem = font.units_per_em().unwrap_or(1000.0);
    let asc = font.ascent_unscaled();
    let desc = font.descent_unscaled();
    let scale = PxScale::from(style.size * (asc - desc) / upem);
    let baseline = pad + asc * style.size / upem;
    let mut cov = vec![0.0_f32; w * h];
    let mut pen = pad;
    for ch in text.chars() {
        let glyph = font.glyph_id(ch).with_scale_and_position(scale, point(pen, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, c| {
                let (x, y) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                    let i = y as usize * w + x as usize;
                    cov[i] = (cov[i] + c).min(1.0);
                }
            });
        }
        pen += advance(style, ch) + style.spacing;
    }
    cov
}

/// Bakes the whole shadow stack of `text` into one premultiplied image.
pub fn bake(text: &str, style: &Style, shadows: &[Shadow]) -> Baked {
    let font = font_for(style);
    let upem = font.units_per_em().unwrap_or(1000.0);
    let rows_h = (font.ascent_unscaled() - font.descent_unscaled()) * style.size / upem;
    let text_w: f32 = text.chars().map(|c| advance(style, c) + style.spacing).sum::<f32>().max(1.0);
    let pad = shadows.iter().map(|s| (s.blur * 1.5).ceil() + s.dx.abs().max(s.dy.abs()).ceil()).fold(0.0_f32, f32::max) + 2.0;
    let (w, h) = ((text_w + 2.0 * pad).ceil() as usize, (rows_h + 2.0 * pad).ceil() as usize);
    let mask = coverage(text, style, w, h, pad);

    // premultiplied accumulation, bottom layer first (CSS paints the first shadow on top)
    let mut acc = vec![[0.0_f32; 4]; w * h];
    for s in shadows.iter().rev() {
        let (dx, dy) = (s.dx.round() as i32, s.dy.round() as i32);
        let mut shifted = vec![0.0_f32; w * h];
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (sx, sy) = (x - dx, y - dy);
                if (0..w as i32).contains(&sx) && (0..h as i32).contains(&sy) {
                    shifted[y as usize * w + x as usize] = mask[sy as usize * w + sx as usize];
                }
            }
        }
        let blurred = gaussian_blur(&shifted, w, h, s.blur / 2.0);
        let [r, g, b, a] = s.color.to_srgba_unmultiplied();
        let alpha = a as f32 / 255.0;
        for (px, cov) in acc.iter_mut().zip(blurred) {
            let sa = (cov * alpha).clamp(0.0, 1.0);
            let src = [r as f32 / 255.0 * sa, g as f32 / 255.0 * sa, b as f32 / 255.0 * sa, sa];
            for c in 0..4 {
                px[c] = src[c] + px[c] * (1.0 - sa);
            }
        }
    }
    let pixels = acc
        .iter()
        .map(|p| Color32::from_rgba_premultiplied((p[0] * 255.0).round() as u8, (p[1] * 255.0).round() as u8, (p[2] * 255.0).round() as u8, (p[3] * 255.0).round() as u8))
        .collect();
    Baked { image: ColorImage { size: [w, h], pixels }, origin: Vec2::splat(-pad) }
}

struct Entry {
    tex: TextureHandle,
    size: Vec2,
    origin: Vec2,
}

/// Textures of baked shadows, keyed by text + style + shadow stack. Small and self-trimming.
#[derive(Default)]
pub struct ShadowCache {
    entries: HashMap<String, Entry>,
}

const MAX_ENTRIES: usize = 64;

fn key(text: &str, s: &Style, shadows: &[Shadow]) -> String {
    let mut k = format!("{text}|{:?}|{:?}|{:.3}|{:.3}", s.display, s.weight, s.size, s.spacing);
    for sh in shadows {
        k.push_str(&format!("|{:.1},{:.1},{:.1},{:?}", sh.dx, sh.dy, sh.blur, sh.color.to_array()));
    }
    k
}

impl ShadowCache {
    /// The shared cache of this egui context.
    pub fn of(ctx: &Context) -> Arc<Mutex<ShadowCache>> {
        ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("web-shadow-cache"), || Arc::new(Mutex::new(ShadowCache::default()))).clone())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Paints `run` (with `text`) at (`left`, line box `line_top`): its blurred shadow stack first, then the crisp glyphs.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, ctx: &Context, painter: &Painter, run: &Run, text: &str, left: f32, line_top: f32, color: Color32, shadows: &[Shadow]) {
        if !shadows.is_empty() && !text.trim().is_empty() {
            let k = key(text, &run.style, shadows);
            if self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(&k) {
                self.entries.clear(); // the count animation makes a new string every frame; keep memory bounded
            }
            let e = self.entries.entry(k.clone()).or_insert_with(|| {
                let b = bake(text, &run.style, shadows);
                let size = Vec2::new(b.image.size[0] as f32, b.image.size[1] as f32);
                Entry { tex: ctx.load_texture(format!("shadow:{k}"), b.image, TextureOptions::LINEAR), size, origin: b.origin }
            });
            let top_left = Pos2::new(left, run.top_in_line(line_top)) + e.origin;
            painter.image(e.tex.id(), Rect::from_min_size(top_left, e.size), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        run.paint(painter, left, line_top, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::text::Weight;

    #[test]
    fn blur_preserves_total_light_and_spreads_it_symmetrically() {
        let (w, h) = (41, 41);
        let mut img = vec![0.0; w * h];
        img[20 * w + 20] = 1.0;
        let out = gaussian_blur(&img, w, h, 3.0);
        let total: f32 = out.iter().sum();
        assert!((total - 1.0).abs() < 1e-3, "mass {total}");
        assert!((out[20 * w + 15] - out[20 * w + 25]).abs() < 1e-6, "left/right");
        assert!((out[15 * w + 20] - out[25 * w + 20]).abs() < 1e-6, "up/down");
        assert!(out[20 * w + 20] > out[20 * w + 22] && out[20 * w + 22] > out[20 * w + 26], "falls off with distance");
    }

    #[test]
    fn a_zero_sigma_blur_changes_nothing() {
        let img = vec![0.0, 1.0, 0.5, 0.25];
        assert_eq!(gaussian_blur(&img, 2, 2, 0.0), img);
    }

    #[test]
    fn a_glow_extends_beyond_the_text_and_fades_out() {
        let style = Style::mono(40.0, Weight::W800);
        let glow = Shadow::new(0.0, 0.0, 18.0, Color32::from_rgba_unmultiplied(255, 106, 0, 89));
        let b = bake("I", &style, &[glow]);
        let [w, h] = b.image.size;
        assert!(w as f32 > 0.6 * 40.0 + 2.0 * 27.0, "padded for the blur: {w}");
        let alpha = |x: usize, y: usize| b.image.pixels[y * w + x].a();
        let centre = alpha(w / 2, h / 2);
        assert!(centre > 20, "the glow is visible behind the glyph: {centre}");
        assert!(centre < 100, "and stays at the shadow's own opacity (35 % = 89): {centre}");
        assert_eq!(alpha(0, 0), 0, "far corners are clear");
        assert!(b.origin.x < -20.0 && b.origin.y < -20.0, "the image starts left of and above the text: {:?}", b.origin);
    }

    #[test]
    fn the_halo_stack_is_darker_close_to_the_glyph_than_far_from_it() {
        let style = Style::mono(40.0, Weight::W800);
        let shadows = crate::web::text::overlay_text_shadows(Color32::BLACK, false);
        let b = bake("I", &style, &shadows);
        let [w, h] = b.image.size;
        let a = |x: usize, y: usize| b.image.pixels[y * w + x].a();
        let mid = h / 2;
        // scan right from the glyph's centre column: alpha must fall (not necessarily every pixel, but overall)
        let near = a(w / 2 + 12, mid);
        let far = a(w / 2 + 24, mid);
        assert!(near >= far, "near {near} far {far}");
        assert!(a(w / 2, mid) > 200, "directly behind the glyph the halo is nearly opaque");
    }

    #[test]
    fn spaces_and_empty_text_bake_without_panicking() {
        let style = Style::mono(12.0, Weight::W400);
        let s = [Shadow::new(0.0, 1.0, 2.0, Color32::BLACK)];
        for t in ["", " ", "   ", "A B"] {
            let b = bake(t, &style, &s);
            assert_eq!(b.image.pixels.len(), b.image.size[0] * b.image.size[1]);
        }
    }

    #[test]
    fn the_cache_reuses_textures_and_stays_bounded() {
        let ctx = Context::default();
        crate::web::install_fonts(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let style = Style::mono(20.0, Weight::W700);
        let shadows = crate::web::text::overlay_text_shadows(Color32::BLACK, false);
        let mut cache = ShadowCache::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let run = Run::new(ctx, "123", &style);
                cache.paint(ctx, ui.painter(), &run, "123", 5.0, 5.0, Color32::WHITE, &shadows);
                cache.paint(ctx, ui.painter(), &run, "123", 5.0, 5.0, Color32::WHITE, &shadows);
                assert_eq!(cache.len(), 1, "the same text is baked once");
                for i in 0..(MAX_ENTRIES + 10) {
                    let t = format!("{i}");
                    let run = Run::new(ctx, &t, &style);
                    cache.paint(ctx, ui.painter(), &run, &t, 5.0, 5.0, Color32::WHITE, &shadows);
                }
                assert!(cache.len() <= MAX_ENTRIES, "{}", cache.len());
            });
        });
    }
}
