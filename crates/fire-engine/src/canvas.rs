//! A tiny indexed-colour canvas for the animated scenes.
//!
//! One byte per pixel (a palette index, 0 = transparent) instead of four, so even the sharpest quality is 43 KB of
//! canvas, not 173 KB. Scenes draw in *logical* units (80 x 60, the same space the flame uses) and the canvas rasterises
//! them at `scale` pixels per unit, so one drawing routine serves every quality level and nothing is allocated per frame.

/// The logical canvas every scene draws on. Quality only changes how many pixels each unit is rasterised into.
pub const LW: usize = 80;
pub const LH: usize = 60;

/// A palette index. `Col(0)` is transparent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Col(pub u8);

macro_rules! palette {
    ($($i:literal $name:ident = [$r:expr, $g:expr, $b:expr, $a:expr];)*) => {
        $(pub const $name: Col = Col($i);)*
        const PAL_C: [[u8; 4]; 64] = {
            let mut t = [[0u8; 4]; 64];
            $(t[$i] = [$r, $g, $b, $a];)*
            t
        };
    };
}

/// The one shared palette (the colours of the six scenes plus the usual shading ramps).
pub mod pal {
    use super::Col;

    palette! {
        1 OUTLINE = [24, 16, 22, 255];
        2 WHITE = [250, 250, 250, 255];
        3 BLACK = [16, 12, 12, 255];
        4 CREAM = [255, 240, 208, 255];
        5 TAN = [232, 168, 96, 255];
        6 BROWN = [130, 78, 44, 255];
        7 DKBROWN = [74, 44, 26, 255];
        8 PINK = [255, 150, 170, 255];
        9 HOTPINK = [238, 104, 140, 255];
        10 LTPINK = [255, 190, 200, 255];
        11 YELLOW = [255, 214, 64, 255];
        12 DKYELLOW = [214, 150, 20, 255];
        13 ORANGE = [255, 106, 0, 255];
        14 DKORANGE = [200, 76, 8, 255];
        15 RED = [222, 52, 56, 255];
        16 DKRED = [120, 22, 34, 255];
        17 BLUE = [60, 140, 220, 255];
        18 DKBLUE = [34, 90, 160, 255];
        19 LTBLUE = [160, 214, 250, 255];
        20 GREEN = [96, 204, 116, 255];
        21 DKGREEN = [40, 120, 70, 255];
        22 LTGREEN = [170, 236, 170, 255];
        23 PURPLE = [156, 106, 226, 255];
        24 DKPURPLE = [104, 62, 176, 255];
        25 LTPURPLE = [190, 150, 245, 255];
        26 GREY = [160, 160, 172, 255];
        27 DKGREY = [74, 74, 86, 255];
        28 LTGREY = [205, 205, 218, 255];
        29 MAGENTA = [240, 80, 190, 255];
        30 CYAN = [80, 220, 230, 255];
        31 GOLD = [255, 200, 50, 255];
        32 SHADOW = [0, 0, 0, 90];
        33 GLOW = [255, 236, 170, 80];
        34 LTORANGE = [255, 176, 96, 255];
    }

    /// The same palette greyed and darkened: how every scene looks when the animation is OFF (paused / no data).
    const fn ash(p: [[u8; 4]; 64]) -> [[u8; 4]; 64] {
        let mut o = p;
        let mut i = 1;
        while i < 64 {
            let [r, g, b, a] = p[i];
            if a != 0 {
                let l = (30 * r as u32 + 59 * g as u32 + 11 * b as u32) / 100;
                let v = (50 + l * 55 / 100) as u8;
                o[i] = [v, v, v, a];
            }
            i += 1;
        }
        o
    }

    pub static PAL: [[u8; 4]; 64] = PAL_C;
    pub static ASH: [[u8; 4]; 64] = ash(PAL_C);
}

/// 3 x 5 pixel glyphs (three bits per row, left to right). Unknown characters draw as blanks.
fn glyph(ch: u8) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        b'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        b'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        b'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        b'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        b'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        b'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        b'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        b'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        b'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        b'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        b'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        b'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        b'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        b'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        b'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        b'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        b'Q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        b'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        b'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        b'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        b'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        b'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        b'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        b'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        b'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        b'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        b'3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        b'6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b110],
        b'!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        b'?' => [0b110, 0b001, 0b010, 0b000, 0b010],
        b'.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        b'+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        b'-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0; 5],
    }
}

pub struct Canvas {
    scale: usize,
    w: usize,
    h: usize,
    buf: Vec<u8>,
}

impl Canvas {
    /// `scale` canvas pixels per logical unit (1 = chunky pixel art, 3 = finest).
    pub fn new(scale: usize) -> Self {
        let scale = scale.clamp(1, 4);
        let (w, h) = (LW * scale, LH * scale);
        Self { scale, w, h, buf: vec![0; w * h] }
    }

    /// Pixels across and down.
    pub fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    pub fn scale(&self) -> usize {
        self.scale
    }

    /// Bytes the canvas itself holds.
    pub fn bytes(&self) -> usize {
        self.buf.len()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.buf
    }

    pub fn clear(&mut self) {
        self.buf.fill(0);
    }

    /// Fills every pixel whose centre passes `inside`, within the logical bounding box.
    fn fill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Col, mut inside: impl FnMut(f32, f32) -> bool) {
        let s = self.scale as f32;
        let px0 = (x0 * s).floor().max(0.0) as usize;
        let py0 = (y0 * s).floor().max(0.0) as usize;
        let px1 = ((x1 * s).ceil().max(0.0) as usize).min(self.w);
        let py1 = ((y1 * s).ceil().max(0.0) as usize).min(self.h);
        let inv = 1.0 / s;
        for py in py0..py1 {
            let y = (py as f32 + 0.5) * inv;
            let row = py * self.w;
            for px in px0..px1 {
                if inside((px as f32 + 0.5) * inv, y) {
                    self.buf[row + px] = c.0;
                }
            }
        }
    }

    /// One logical unit square with its top-left corner at (x, y).
    pub fn dot(&mut self, x: f32, y: f32, c: Col) {
        self.rect(x, y, 1.0, 1.0, c);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Col) {
        let (x1, y1) = (x + w, y + h);
        self.fill(x, y, x1, y1, c, |px, py| px >= x && px < x1 && py >= y && py < y1);
    }

    /// A filled ellipse centred on (cx, cy).
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Col) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let (ix, iy) = (1.0 / rx, 1.0 / ry);
        self.fill(cx - rx, cy - ry, cx + rx, cy + ry, c, |px, py| {
            let (dx, dy) = ((px - cx) * ix, (py - cy) * iy);
            dx * dx + dy * dy <= 1.0
        });
    }

    /// An ellipse outline `thick` units wide (growing inward from the radii).
    pub fn ring(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, thick: f32, c: Col) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let (ix, iy) = (1.0 / rx, 1.0 / ry);
        let (jx, jy) = (1.0 / (rx - thick).max(0.001), 1.0 / (ry - thick).max(0.001));
        self.fill(cx - rx, cy - ry, cx + rx, cy + ry, c, |px, py| {
            let (dx, dy) = (px - cx, py - cy);
            let outer = (dx * ix) * (dx * ix) + (dy * iy) * (dy * iy) <= 1.0;
            let inner = (dx * jx) * (dx * jx) + (dy * jy) * (dy * jy) < 1.0;
            outer && !inner
        });
    }

    /// A line `thick` units wide (never thinner than one canvas pixel).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thick: f32, c: Col) {
        let r = thick.max(1.0 / self.scale as f32) * 0.5;
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.fill(x0.min(x1) - r, y0.min(y1) - r, x0.max(x1) + r, y0.max(y1) + r, c, |px, py| {
            let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
            let (qx, qy) = (x0 + t * dx - px, y0 + t * dy - py);
            qx * qx + qy * qy <= r * r
        });
    }

    /// A filled triangle.
    pub fn tri(&mut self, a: (f32, f32), b: (f32, f32), d: (f32, f32), c: Col) {
        let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0);
        self.fill(a.0.min(b.0).min(d.0), a.1.min(b.1).min(d.1), a.0.max(b.0).max(d.0), a.1.max(b.1).max(d.1), c, |x, y| {
            let (e1, e2, e3) = (edge(a, b, x, y), edge(b, d, x, y), edge(d, a, x, y));
            (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0)
        });
    }

    /// 3 x 5 pixel text; `sc` logical units per font pixel (1 = 3 x 5 units). Letters are 4 * `sc` apart.
    pub fn text(&mut self, x: f32, y: f32, s: &str, c: Col, sc: f32) {
        let mut cx = x;
        for ch in s.bytes() {
            for (ry, row) in glyph(ch).iter().enumerate() {
                for rx in 0..3 {
                    if row & (0b100 >> rx) != 0 {
                        self.rect(cx + rx as f32 * sc, y + ry as f32 * sc, sc, sc, c);
                    }
                }
            }
            cx += 4.0 * sc;
        }
    }

    /// Width in logical units of `s` drawn at `sc`.
    pub fn text_width(s: &str, sc: f32) -> f32 {
        (s.len() as f32 * 4.0 - 1.0).max(0.0) * sc
    }

    /// Adds a one-canvas-pixel outline around everything drawn so far (shadows and glows do not get one).
    pub fn outline(&mut self, c: Col) {
        fn solid(i: u8, oc: u8) -> bool {
            i != 0 && i != oc && i != pal::SHADOW.0 && i != pal::GLOW.0
        }
        let (w, h) = (self.w, self.h);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if self.buf[i] != 0 {
                    continue;
                }
                let near = (x > 0 && solid(self.buf[i - 1], c.0))
                    || (x + 1 < w && solid(self.buf[i + 1], c.0))
                    || (y > 0 && solid(self.buf[i - w], c.0))
                    || (y + 1 < h && solid(self.buf[i + w], c.0));
                if near {
                    self.buf[i] = c.0;
                }
            }
        }
    }

    /// Expands to RGBA (`out` is `w * h * 4` bytes). `ash` greys everything for the OFF state.
    pub fn to_rgba(&self, out: &mut [u8], ash: bool) {
        let lut = if ash { &pal::ASH } else { &pal::PAL };
        for (o, &i) in out.chunks_exact_mut(4).zip(&self.buf) {
            o.copy_from_slice(&lut[(i & 63) as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(c: &Canvas) -> usize {
        c.pixels().iter().filter(|&&p| p != 0).count()
    }

    #[test]
    fn the_canvas_is_one_byte_per_pixel_at_every_scale() {
        for s in 1..=3 {
            let c = Canvas::new(s);
            assert_eq!(c.size(), (80 * s, 60 * s));
            assert_eq!(c.bytes(), 80 * s * 60 * s);
        }
    }

    #[test]
    fn a_whole_unit_rect_fills_exactly_those_units_at_any_scale() {
        for s in 1..=3 {
            let mut c = Canvas::new(s);
            c.rect(10.0, 20.0, 5.0, 4.0, pal::RED);
            assert_eq!(lit(&c), 5 * 4 * s * s);
        }
    }

    #[test]
    fn shapes_off_the_canvas_are_clipped_not_a_panic() {
        let mut c = Canvas::new(2);
        c.rect(-50.0, -50.0, 500.0, 500.0, pal::RED);
        assert_eq!(lit(&c), c.bytes());
        c.clear();
        c.ellipse(-10.0, 200.0, 5.0, 5.0, pal::RED);
        c.line(-5.0, -5.0, 400.0, 900.0, 2.0, pal::RED);
        c.tri((-9.0, -9.0), (99.0, -9.0), (40.0, 99.0), pal::RED);
        c.text(70.0, 55.0, "CLIPPED OFF THE EDGE", pal::RED, 2.0);
        c.rect(f32::NAN, 5.0, 5.0, 5.0, pal::RED);
    }

    #[test]
    fn an_ellipse_is_round_and_a_ring_is_hollow() {
        let mut c = Canvas::new(3);
        c.ellipse(40.0, 30.0, 10.0, 10.0, pal::BLUE);
        let area = lit(&c) as f32 / 9.0;
        assert!((area - std::f32::consts::PI * 100.0).abs() < 8.0, "area {area}");
        c.clear();
        c.ring(40.0, 30.0, 10.0, 10.0, 1.0, pal::BLUE);
        assert!(lit(&c) < 9 * 80, "a one-unit ring is far smaller than the disc");
    }

    #[test]
    fn text_draws_pixels_and_an_unknown_glyph_is_blank() {
        let mut c = Canvas::new(1);
        c.text(2.0, 2.0, "7", pal::WHITE, 1.0);
        assert_eq!(lit(&c), 7);
        c.clear();
        c.text(2.0, 2.0, "~", pal::WHITE, 1.0);
        assert_eq!(lit(&c), 0);
        assert_eq!(Canvas::text_width("AB", 1.0), 7.0);
    }

    #[test]
    fn the_outline_wraps_a_shape_once_and_skips_shadows() {
        let mut c = Canvas::new(1);
        c.rect(10.0, 10.0, 4.0, 4.0, pal::RED);
        c.rect(30.0, 30.0, 4.0, 4.0, pal::SHADOW);
        c.outline(pal::OUTLINE);
        assert_eq!(lit(&c), 16 + 16 + 16 /* 4 sides x 4 */ , "a 4x4 block gains 16 edge pixels; the shadow gains none");
    }

    #[test]
    fn ash_is_grey_and_keeps_transparency() {
        let mut c = Canvas::new(1);
        c.rect(0.0, 0.0, 2.0, 1.0, pal::ORANGE);
        let mut out = vec![9u8; 80 * 60 * 4];
        c.to_rgba(&mut out, true);
        assert!(out[0] == out[1] && out[1] == out[2] && out[3] == 255);
        assert_eq!(&out[2 * 4..2 * 4 + 4], &[0, 0, 0, 0], "an empty pixel stays fully transparent");
        c.to_rgba(&mut out, false);
        assert_eq!(&out[0..4], &[255, 106, 0, 255]);
    }
}
