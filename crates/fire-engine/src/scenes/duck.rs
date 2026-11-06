//! Rubber-duck debugging: a duck on a pond quacks at a bug on a leaf. A surge throws it out of a splash.

use std::f32::consts::PI;

use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const WL: f32 = 52.0; const BX: f32 = 25.0; const LX: f32 = 58.0; const G: f32 = 90.0;
const POND: (f32, f32, f32, f32) = (40.0, 53.0, 33.0, 5.5);

pub struct Duck {
    drops: Particles<30>,
    fresh: bool,
    landed: bool,
}

impl Duck {
    pub fn new() -> Self {
        Self { drops: Particles::new(), fresh: false, landed: true }
    }

    fn splash(&mut self, n: usize, power: f32, cx: &Ctx, rng: &mut dyn Rng) {
        let (m, k) = (cx.surge_mag, cx.motion);
        for i in 0..n {
            let left = i % 2 == 0;
            let sx = if left { BX - 7.0 + rand(rng, -3.0, 2.0) } else { BX + 9.0 + rand(rng, -2.0, 3.0) };
            let vx = (if left { -1.0 } else { 1.0 }) * rand(rng, 6.0, 16.0 + 16.0 * m) * k;
            let v = (30.0 + 40.0 * m) * power * k * rand(rng, 0.7, 1.2);
            let col = [pal::LTBLUE, pal::WHITE, pal::BLUE, pal::CYAN][i % 4];
            let size = if i % 3 == 0 { 2.0 } else { 1.2 };
            self.drops.spawn(Particle { x: sx, y: WL - 1.0, vx, vy: -v, life: 2.0 * v / G, max: 2.0 * v / G, col, size });
        }
    }
}

impl Default for Duck { fn default() -> Self { Self::new() } }

fn air(cx: &Ctx) -> f32 { (0.5 + 0.4 * cx.surge_mag).min(0.7 * cx.surge_len) }
fn arc(u: f32) -> f32 { 4.0 * u * (1.0 - u) }
fn fade(a: f32, start: f32, dur: f32) -> f32 { if a < start { 0.0 } else { (1.0 - (a - start) / dur).max(0.0) } }

fn hop(cx: &Ctx) -> f32 {
    if cx.boost <= 0.0 { return 0.0; }
    let (a, t1) = (cx.surge_age, air(cx));
    let h = (8.0 + 18.0 * cx.surge_mag) * (0.5 + 0.5 * cx.motion);
    if a < t1 { h * arc(a / t1) } else if a < t1 * 1.4 { 0.2 * h * arc((a - t1) / (t1 * 0.4)) } else { 0.0 }
}

fn ripples(c: &mut Canvas, t: f32, live: bool, front: bool) {
    let a0 = if front { 0.0 } else { PI };
    for i in 0..3 {
        let p = if live { (t * 0.45 + i as f32 / 3.0).fract() } else { 0.15 + 0.3 * i as f32 };
        let (rx, ry, col) = (9.0 + 10.0 * p, 1.2 + 2.3 * p, if p < 0.65 { pal::LTBLUE } else { pal::BLUE });
        for k in 0..12 {
            let (a, b) = (k as f32, k as f32 + 1.0);
            let (x0, y0) = (BX + 1.0 + rx * (a0 + PI * a / 12.0).cos(), WL + ry * (a0 + PI * a / 12.0).sin());
            let (x1, y1) = (BX + 1.0 + rx * (a0 + PI * b / 12.0).cos(), WL + ry * (a0 + PI * b / 12.0).sin());
            c.line(x0, y0, x1, y1, 0.8, col);
        }
    }
}

fn eye(c: &mut Canvas, x: f32, y: f32, open: f32, wow: bool) {
    let (rx, ry) = if wow { (2.5, 3.0) } else { (2.1, (2.4 * open).max(0.5)) };
    c.ellipse(x, y, rx, ry, pal::WHITE);
    if open > 0.3 {
        c.ellipse(x + 0.8, y + 0.2, if wow { 0.8 } else { 1.1 }, if wow { 0.9 } else { 1.2 }, pal::BLACK);
    }
}

struct Pose {
    dx: f32, dy: f32, head: f32, open: f32, blink: f32, flap: bool, wag: f32, wow: bool, calm: bool,
}

fn duck(c: &mut Canvas, p: &Pose, detail: u8) -> (f32, f32) {
    let (bx, by) = (BX + p.dx, WL - 3.0 + p.dy);
    let (hx, hy) = (bx + 9.0 + p.head, by - 10.0 + p.head * 0.3);
    if p.wow { c.ellipse(bx - 4.0, by - if p.flap { 8.0 } else { 6.0 }, 3.0, 4.5, pal::DKYELLOW); }
    c.tri((bx - 10.0, by), (bx - 8.0, by - 5.0), (bx - 17.0, by - if p.wow { 11.0 } else { 9.0 } + p.wag), pal::YELLOW);
    c.ellipse(bx, by + 0.8, 12.5, 7.2, pal::DKYELLOW);
    c.ellipse(bx - 0.3, by, 12.0, 6.7, pal::YELLOW);
    if !p.wow { c.ellipse(bx - 2.0, by - 0.2, 5.5, 3.2, pal::DKYELLOW); }
    c.ellipse(hx, hy + 0.7, 6.7, 6.6, pal::DKYELLOW);
    c.ellipse(hx - 0.3, hy, 6.4, 6.3, pal::YELLOW);
    if detail >= 1 { c.rect(hx + 0.2, hy + 2.4, 2.0, 1.2, pal::PINK); }
    let gap = p.open * 2.2;
    if gap > 0.1 { c.rect(hx + 5.0, hy + 1.2, 6.0, gap + 1.0, pal::DKRED); }
    c.rect(hx + 4.5, hy - 0.8, 7.5, 2.2, pal::ORANGE);
    c.rect(hx + 4.5, hy + 1.4 + gap, 6.5, 1.6, pal::DKORANGE);
    let (ex, ey) = (hx + 2.3, hy - 2.3);
    eye(c, ex, ey, p.blink, p.wow);
    let r = if p.wow { 3.9 } else { 3.3 };
    c.ring(ex, ey, r, r, 0.9, pal::DKGREY);
    c.line(ex - r, ey, ex - 7.0, ey - 0.5, 0.7, pal::DKGREY);
    if detail >= 2 { c.dot(ex - 0.8, ey - 0.8, pal::WHITE); }
    if !p.calm && !p.wow { c.line(hx - 0.5, hy - 6.4, hx + 4.8, hy - 4.8, 1.2, pal::DKORANGE); }
    (hx + 6.0, hy - 3.0)
}

fn bug(c: &mut Canvas, x: f32, y: f32, t: f32, asleep: bool) {
    for k in 0..3 {
        let lx = x - 2.0 + 2.0 * k as f32;
        let w = if (t * 14.0 + 2.1 * k as f32).sin() > 0.0 { 0.9 } else { -0.9 };
        c.line(lx, y + 1.2, lx + w, y + 3.2, 0.7, pal::DKGREY);
    }
    c.ellipse(x, y + 0.5, 3.5, 2.4, pal::DKRED);
    c.ellipse(x, y, 3.2, 2.2, pal::RED);
    c.dot(x + 0.2, y - 1.3, pal::DKRED);
    c.dot(x + 1.7, y - 0.4, pal::DKRED);
    c.ellipse(x - 3.7, y + 0.3, 1.8, 1.6, pal::DKGREY);
    if !asleep { c.rect(x - 4.5, y - 0.9, 1.2, 1.2, pal::WHITE); }
    let (w, up) = ((t * 9.0).sin() * 0.8, if asleep { -0.3 } else { 1.0 });
    c.line(x - 4.0, y - 1.0, x - 5.6 + w, y - 1.0 - 2.8 * up, 0.7, pal::DKGREY);
    c.line(x - 3.0, y - 1.3, x - 3.3 + w, y - 1.3 - 2.7 * up, 0.7, pal::DKGREY);
}

fn bubble(c: &mut Canvas, txt: &str, big: Option<(f32, f32)>, tip: (f32, f32)) {
    let (ink, frame) = if big.is_some() { (pal::RED, pal::DKRED) } else { (pal::BLACK, pal::DKGREY) };
    let (w, h) = (Canvas::text_width(txt, 1.0) + 4.0, 9.0);
    let (mx, my) = (50.0 + big.map_or(0.0, |b| 2.0 + b.0), 26.5);
    let (x, y) = (mx - w * 0.5, my - h * 0.5);
    if let Some((_, boost)) = big {
        for k in 0..7 {
            let a = PI * (0.9 + 0.2 * k as f32);
            c.line(mx + a.cos() * (w * 0.5 + 2.0), my + a.sin() * 7.0, mx + a.cos() * (w * 0.5 + 4.0 + 5.0 * boost), my + a.sin() * (9.0 + 4.0 * boost), 1.0, pal::ORANGE);
        }
    }
    let (tx, ty, down) = if tip.1 < y + 3.0 {
        (tip.0.clamp(x - 8.0, x + 2.0), tip.1.clamp(y - 6.0, y + 2.0), false)
    } else {
        (tip.0.clamp(x - 4.0, x + 8.0), tip.1.clamp(y + h, y + h + 6.0), true)
    };
    if down {
        c.tri((x + 1.0, y + h - 1.0), (x + 7.0, y + h - 1.0), (tx, ty), frame);
        c.tri((x + 2.0, y + h - 1.0), (x + 6.0, y + h - 1.0), (tx, ty - 1.0), pal::WHITE);
    } else {
        c.tri((x - 1.0, y + 1.0), (x - 1.0, y + 6.0), (tx, ty), frame);
        c.tri((x, y + 2.0), (x, y + 5.0), (tx + 0.5, ty + 0.5), pal::WHITE);
    }
    c.rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, frame);
    c.rect(x, y, w, h, pal::WHITE);
    c.text(x + 2.0, y + 2.0, txt, ink, 1.0);
}

impl Scene for Duck {
    fn reset(&mut self, _rng: &mut dyn Rng) { self.drops.clear(); self.fresh = false; self.landed = true; }
    fn surge(&mut self, _mag: f32, _rng: &mut dyn Rng) { self.fresh = true; self.landed = false; }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        if cx.boost <= 0.0 { self.reset(rng); return; }
        if self.fresh {
            self.fresh = false;
            self.splash(12 + (12.0 * cx.surge_mag) as usize, 1.0, cx, rng);
        }
        if !self.landed && cx.surge_age >= air(cx) {
            self.landed = true;
            self.splash(6 + (6.0 * cx.surge_mag) as usize, 0.45, cx, rng);
        }
        self.drops.step(dt, G, 1.0);
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let (off, live, wow) = (cx.state == FlameState::Off, cx.state == FlameState::Live, cx.boost > 0.0);
        let (t, up) = (if live { cx.t } else { 0.0 }, hop(cx));
        let quack = wow || (live && (t / 2.2).fract() < 0.45);

        let (px, py, prx, pry) = POND;
        if cx.detail >= 2 { c.ellipse(px, py + 1.0, prx - 1.0, pry - 1.0, pal::GLOW); }
        c.ellipse(px, py, prx, pry, pal::DKBLUE);
        c.ellipse(px, py - 0.4, prx - 1.4, pry - 1.3, pal::BLUE);
        if !off { ripples(c, t, live, false); }

        let mut tip = (BX + 15.0, WL - 13.0);
        if off {
            c.ellipse(BX, WL + 2.0, 12.0, 3.0, pal::DKBLUE);
            c.ellipse(BX + 8.0, WL + 1.5, 5.0, 2.5, pal::DKBLUE);
            for x in [33.0, 38.5] { eye(c, x, WL - 0.7, 0.45, false); c.ring(x, WL - 0.7, 2.8, 2.8, 0.8, pal::DKGREY); }
            c.line(35.5, WL - 0.7, 36.0, WL - 0.7, 0.8, pal::DKGREY);
            c.rect(42.0, WL - 1.8, 4.0, 1.8, pal::ORANGE);
            c.rect(42.0, WL, 3.5, 0.8, pal::DKORANGE);
            c.text(32.0, 43.0, "Z", pal::LTGREY, 1.0);
            c.text(37.0, 38.0, "Z", pal::GREY, 1.0);
        } else {
            let bob = if live { cx.motion * (t * 5.6).sin() * 0.9 } else { 0.0 };
            let pose = Pose {
                dx: -0.3 * up,
                dy: bob - up,
                head: if wow { 2.2 } else if quack { 1.5 } else { 0.0 },
                open: if wow { 1.0 } else if quack { 0.8 } else { 0.0 },
                blink: if live && t % 3.1 < 0.14 { 0.2 } else { 1.0 },
                wag: if live { 1.2 * cx.motion * (t * 9.0).sin() } else { 0.0 },
                flap: wow && (cx.surge_age * 12.0) as i32 % 2 == 0,
                wow, calm: !live && !wow,
            };
            if wow {
                let w = (5.0 + 7.0 * cx.surge_mag) * fade(cx.surge_age, 0.0, air(cx) * 0.9);
                let apex = (WL + 4.2 - up).min(WL - 1.0);
                c.tri((BX + 2.0 - w, WL), (BX + 2.0 + w, WL), (BX - 0.3 * up, apex), pal::LTBLUE);
                c.tri((BX + 2.0 - w * 0.5, WL), (BX + 2.0 + w * 0.5, WL), (BX - 0.3 * up, apex), pal::WHITE);
            }
            tip = duck(c, &pose, cx.detail);
        }

        let (rx, ry) = (prx - 1.4, pry - 1.3);
        for xi in 8..72 {
            let u = (xi as f32 + 0.5 - px) / rx;
            let bot = py - 0.4 + ry * (1.0 - u * u).max(0.0).sqrt();
            if bot > WL { c.rect(xi as f32, WL, 1.0, bot - WL, pal::BLUE); }
        }
        c.rect(BX - 12.0, WL, 28.0, 1.0, pal::LTBLUE);
        if !off { ripples(c, t, live, true); }
        if cx.detail >= 1 {
            for (x, y) in [(14.0, 55.0), (47.0, 56.0), (64.0, 54.0)] { c.rect(x, y, 3.0, 1.0, pal::LTBLUE); }
        }

        c.tri((LX - 2.0, 42.0), (LX + 2.0, 42.0), (LX, 44.5), pal::DKRED);
        c.rect(LX - 7.5, 34.5, 15.0, 7.5, pal::DKRED);
        c.rect(LX - 6.75, 35.25, 13.5, 6.0, pal::CREAM);
        c.text(LX - 5.5, 36.0, "BUG", pal::RED, 1.0);

        let scare = if wow { (3.0 + 3.0 * cx.surge_mag) * arc((cx.surge_age / 0.45).min(1.0)) } else { 0.0 };
        c.ellipse(LX, WL + 1.0, 6.5, 2.3, pal::DKGREEN);
        c.ellipse(LX, WL + 0.4, 6.5, 2.1, pal::GREEN);
        c.line(LX - 5.0, WL + 0.4, LX + 5.0, WL + 0.4, 0.6, pal::DKGREEN);
        bug(c, LX + if live { (0.25 + if quack { 0.6 } else { 0.0 }) * (t * 37.0).sin() } else { 0.0 }, WL - 2.3 - scare, t, off);
        if live { c.rect(LX - 7.0, WL - 6.0 + (t * 1.5).fract() * 3.0, 1.0, 1.6, pal::BLUE); }

        if wow {
            let f = (fade(cx.surge_age, 0.0, 0.5) + 0.6 * fade(cx.surge_age, air(cx), 0.3)).min(1.0);
            let r = (7.0 + 9.0 * cx.surge_mag) * (0.4 + 0.6 * f);
            c.ellipse(BX + 2.0, WL + 0.5, r, 1.2 + 2.2 * f, pal::LTBLUE);
            c.ellipse(BX + 2.0, WL + 0.5, r * 0.6, 0.8 + 1.2 * f, pal::WHITE);
            self.drops.draw(c);
        }
        if quack {
            let (txt, big) = if wow { ("QUACK!!", Some(((cx.surge_age * 40.0).sin() * (0.4 + 0.8 * cx.surge_mag), cx.boost))) } else { ("QUACK", None) };
            bubble(c, txt, big, tip);
        }
    }
}
