//! Text the way the browser draws it: CSS font sizes, `letter-spacing`, line boxes and layered `text-shadow`.

use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontRef};
use egui::{Color32, FontFamily, FontId, Galley, Painter, Pos2, Vec2};

use super::fonts::{ARCHIVO_BYTES, DISPLAY, JB_400_BYTES, JB_700_BYTES, JB_800_BYTES, MONO_400, MONO_700, MONO_800};
use super::with_alpha;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    W400,
    W700,
    W800,
}

/// One text style: face, size, letter-spacing and the CSS line box it sits in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub display: bool,
    pub weight: Weight,
    pub size: f32,
    /// `letter-spacing` in px (added after every glyph, including the last, as CSS does).
    pub spacing: f32,
    /// `line-height` in px.
    pub line_h: f32,
}

impl Style {
    /// JetBrains Mono. `line-height` defaults to the body's 1.35.
    pub fn mono(size: f32, weight: Weight) -> Self {
        Self { display: false, weight, size, spacing: 0.0, line_h: size * 1.35 }
    }

    /// Archivo Black (the `--font-display` face used by headings).
    pub fn display(size: f32) -> Self {
        Self { display: true, weight: Weight::W400, size, spacing: 0.0, line_h: size * 1.35 }
    }

    pub fn spacing_em(mut self, em: f32) -> Self {
        self.spacing = em * self.size;
        self
    }

    pub fn spacing_px(mut self, px: f32) -> Self {
        self.spacing = px;
        self
    }

    /// `line-height` as a multiple of the font size.
    pub fn lh(mut self, factor: f32) -> Self {
        self.line_h = factor * self.size;
        self
    }

    pub fn lh_px(mut self, px: f32) -> Self {
        self.line_h = px;
        self
    }

    pub fn font_id(&self) -> FontId {
        let name = if self.display {
            DISPLAY
        } else {
            match self.weight {
                Weight::W400 => MONO_400,
                Weight::W700 => MONO_700,
                Weight::W800 => MONO_800,
            }
        };
        FontId::new(self.size, FontFamily::Name(name.into()))
    }
}

/// Exact glyph advances straight from the font files. egui rounds every glyph's advance to a whole pixel, which would
/// make a 39 px number about 2 px wider than the browser draws it; the browser uses the font's exact widths.
pub(super) fn font_for(style: &Style) -> &'static FontRef<'static> {
    static JB400: OnceLock<FontRef<'static>> = OnceLock::new();
    static JB700: OnceLock<FontRef<'static>> = OnceLock::new();
    static JB800: OnceLock<FontRef<'static>> = OnceLock::new();
    static ARCHIVO: OnceLock<FontRef<'static>> = OnceLock::new();
    let (cell, bytes) = if style.display {
        (&ARCHIVO, ARCHIVO_BYTES)
    } else {
        match style.weight {
            Weight::W400 => (&JB400, JB_400_BYTES),
            Weight::W700 => (&JB700, JB_700_BYTES),
            Weight::W800 => (&JB800, JB_800_BYTES),
        }
    };
    cell.get_or_init(|| FontRef::try_from_slice(bytes).expect("embedded font parses"))
}

/// The advance of `ch` at `size` px (no letter-spacing).
pub fn advance(style: &Style, ch: char) -> f32 {
    let font = font_for(style);
    let upem = font.units_per_em().unwrap_or(1000.0);
    font.h_advance_unscaled(font.glyph_id(ch)) * style.size / upem
}

/// One `text-shadow` layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    /// CSS blur radius in px (0 = a hard copy).
    pub blur: f32,
    pub color: Color32,
}

impl Shadow {
    pub fn new(dx: f32, dy: f32, blur: f32, color: Color32) -> Self {
        Self { dx, dy, blur, color }
    }
}

/// Offsets (and their weights) that approximate a Gaussian blur of CSS radius `blur` by overlapping copies:
/// the centre plus one to three rings. Weights sum to 1, so the shadow's alpha is spread across the copies.
pub fn blur_offsets(blur: f32) -> Vec<(f32, f32, f32)> {
    if blur <= 0.0 {
        return vec![(0.0, 0.0, 1.0)];
    }
    let sigma = blur / 2.0;
    let radii: &[f32] = if sigma < 2.0 {
        &[0.85]
    } else if sigma < 5.0 {
        &[0.8, 1.6]
    } else {
        &[0.6, 1.2, 1.9]
    };
    let mut pts: Vec<(f32, f32)> = vec![(0.0, 0.0)];
    for r in radii {
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            pts.push((sigma * r * a.cos(), sigma * r * a.sin()));
        }
    }
    let w = 1.0 / pts.len() as f32;
    pts.into_iter().map(|(x, y)| (x, y, w)).collect()
}

/// The soft halo behind overlay text so it reads on any desktop (`text-shadow` in `overlay.css`), optionally with the
/// orange glow a live count gets.
pub fn overlay_text_shadows(halo: Color32, live_glow: bool) -> Vec<Shadow> {
    let mut v = vec![
        Shadow::new(0.0, 0.0, 2.0, halo),
        Shadow::new(0.0, 1.0, 2.0, with_alpha(halo, 0.9)),
        Shadow::new(0.0, 0.0, 6.0, with_alpha(halo, 0.45)),
    ];
    if live_glow {
        v.push(Shadow::new(0.0, 0.0, 18.0, Color32::from_rgba_unmultiplied(255, 106, 0, 89)));
    }
    v
}

/// Laid-out text, ready to paint at any position (laying out is the expensive part, so it is done once).
#[derive(Clone)]
pub struct Run {
    parts: Vec<(f32, Arc<Galley>)>,
    pub width: f32,
    /// Height of the glyph rows (the CSS "content area"), not the line box.
    pub height: f32,
    pub style: Style,
}

impl Run {
    pub fn new(ctx: &egui::Context, text: &str, style: &Style) -> Self {
        ctx.fonts(|f| {
            let font = style.font_id();
            // Each glyph is placed by hand at the font's exact advance plus the letter-spacing (CSS adds it after every
            // glyph, including the last). Laying glyphs out one by one is cheap: egui caches the galleys.
            let mut parts = Vec::with_capacity(text.chars().count());
            let (mut x, mut h) = (0.0_f32, f.row_height(&font));
            for ch in text.chars() {
                if ch != ' ' {
                    let g = f.layout_no_wrap(ch.to_string(), font.clone(), Color32::WHITE);
                    h = h.max(g.size().y);
                    parts.push((x, g));
                }
                x += advance(style, ch) + style.spacing;
            }
            Run { parts, width: x, height: h, style: *style }
        })
    }

    /// Top of the glyph rows when the text is centred in its CSS line box starting at `line_top`.
    pub fn top_in_line(&self, line_top: f32) -> f32 {
        line_top + (self.style.line_h - self.height) / 2.0
    }

    /// Paints with its left edge at `left` and its line box starting at `line_top`.
    pub fn paint(&self, painter: &Painter, left: f32, line_top: f32, color: Color32) {
        let y = self.top_in_line(line_top);
        for (dx, g) in &self.parts {
            painter.galley_with_override_text_color(Pos2::new(left + dx, y), g.clone(), color);
        }
    }

    /// Paints the shadows (first layer on top, as CSS stacks them) and then the text itself.
    pub fn paint_shadowed(&self, painter: &Painter, left: f32, line_top: f32, color: Color32, shadows: &[Shadow]) {
        for s in shadows.iter().rev() {
            for (ox, oy, w) in blur_offsets(s.blur) {
                let c = with_alpha(s.color, (s.color.a() as f32 / 255.0) * w);
                if c.a() == 0 {
                    continue;
                }
                self.paint(painter, left + s.dx + ox, line_top + s.dy + oy, c);
            }
        }
        self.paint(painter, left, line_top, color);
    }

    /// The text's box in CSS terms: its advance width by its line height.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.style.line_h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        let c = egui::Context::default();
        super::super::install_fonts(&c);
        let _ = c.run(egui::RawInput::default(), |_| {});
        c
    }

    #[test]
    fn the_big_number_is_exactly_as_wide_as_in_the_browser() {
        // Measured in Chromium: "255.5M", 39.3333px, weight 800, letter-spacing -0.02em -> 136.88 px wide.
        let c = ctx();
        let s = Style::mono(39.3333, Weight::W800).spacing_em(-0.02).lh(1.05);
        let r = Run::new(&c, "255.5M", &s);
        assert!((r.width - 136.88).abs() < 0.05, "width {} (browser: 136.88)", r.width);
        assert!((s.line_h - 41.3).abs() < 0.05);
    }

    #[test]
    fn the_period_label_is_exactly_as_wide_as_in_the_browser() {
        // Measured: 10px bold, letter-spacing 0.34em; the label button is 148.81 px = 12 + 9 + text + 9 + 6 padding/arrows.
        let c = ctx();
        let s = Style::mono(10.0, Weight::W700).spacing_em(0.34);
        let r = Run::new(&c, "TOKENS TODAY", &s);
        assert!((r.width - 112.8).abs() < 0.05, "width {}", r.width);
        assert!((12.0 + 9.0 + r.width + 9.0 + 6.0 - 148.81).abs() < 0.1);
    }

    #[test]
    fn the_heading_face_is_proportional_and_measured_from_the_font() {
        // Measured in Chromium: "ALL AGENTS" in Archivo Black 15px with 0.04em spacing is 111.56 px wide.
        let c = ctx();
        let r = Run::new(&c, "ALL AGENTS", &Style::display(15.0).spacing_em(0.04));
        assert!((r.width - 111.56).abs() < 1.5, "width {} (browser: 111.56)", r.width);
    }

    #[test]
    fn letter_spacing_is_added_after_every_glyph_including_the_last() {
        let c = ctx();
        let plain = Run::new(&c, "AB", &Style::mono(10.0, Weight::W400));
        let spaced = Run::new(&c, "AB", &Style::mono(10.0, Weight::W400).spacing_px(3.0));
        assert!((spaced.width - plain.width - 6.0).abs() < 0.01, "{} vs {}", spaced.width, plain.width);
    }

    #[test]
    fn jetbrains_mono_advances_exactly_0_6_em_at_every_weight() {
        for w in [Weight::W400, Weight::W700, Weight::W800] {
            for ch in ['0', '5', '.', 'M', 'K', ' ', 'W'] {
                let a = advance(&Style::mono(39.0, w), ch);
                assert!((a - 23.4).abs() < 0.01, "{ch:?} at {w:?} advances {a}");
            }
        }
        let d = Style::display(12.0);
        assert!(advance(&d, 'I') < advance(&d, 'W'), "the display face is proportional");
    }

    #[test]
    fn line_heights_follow_the_css_defaults() {
        assert!((Style::mono(12.0, Weight::W400).line_h - 16.2).abs() < 0.001);
        assert!((Style::display(15.0).line_h - 20.25).abs() < 0.001);
        assert!((Style::mono(11.0, Weight::W700).line_h - 14.85).abs() < 0.001);
        assert!((Style::mono(10.0, Weight::W400).lh(1.0).line_h - 10.0).abs() < 0.001);
    }

    #[test]
    fn text_is_centred_in_its_line_box() {
        let c = ctx();
        let r = Run::new(&c, "X", &Style::mono(12.0, Weight::W400));
        let top = r.top_in_line(100.0);
        assert!((top + r.height / 2.0 - (100.0 + r.style.line_h / 2.0)).abs() < 0.01);
    }

    #[test]
    fn blur_copies_share_their_alpha_and_are_symmetric() {
        for blur in [0.0, 2.0, 6.0, 18.0] {
            let pts = blur_offsets(blur);
            let total: f32 = pts.iter().map(|p| p.2).sum();
            assert!((total - 1.0).abs() < 1e-4, "weights sum to {total} for blur {blur}");
            let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
            assert!(sx.abs() < 1e-3 && sy.abs() < 1e-3, "blur {blur} is not centred");
        }
        assert_eq!(blur_offsets(0.0).len(), 1);
        assert!(blur_offsets(18.0).len() > blur_offsets(6.0).len(), "bigger blurs use more rings so they stay smooth");
    }

    #[test]
    fn the_overlay_halo_has_the_css_layers_and_the_live_glow_is_optional() {
        let plain = overlay_text_shadows(Color32::BLACK, false);
        assert_eq!(plain.len(), 3);
        assert_eq!((plain[0].blur, plain[1].blur, plain[2].blur), (2.0, 2.0, 6.0));
        assert_eq!(plain[1].dy, 1.0);
        let live = overlay_text_shadows(Color32::BLACK, true);
        assert_eq!(live.len(), 4);
        let [r, g, _, a] = live[3].color.to_srgba_unmultiplied();
        assert_eq!(live[3].blur, 18.0);
        assert!(r >= 254 && (105..=106).contains(&g) && a == 89, "{r} {g} {a}");
    }

    #[test]
    fn painting_shadowed_text_emits_shapes_without_panicking() {
        let c = ctx();
        let run = Run::new(&c, "255.5M", &Style::mono(39.0, Weight::W800));
        let out = c.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                run.paint_shadowed(ui.painter(), 10.0, 10.0, Color32::WHITE, &overlay_text_shadows(Color32::BLACK, true));
            });
        });
        assert!(out.shapes.len() > 20, "{} shapes", out.shapes.len());
    }
}
