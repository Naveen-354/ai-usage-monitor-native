//! Multitasking octopus: eight arms typing on three keyboards, phone and coffee mug.
//! Live types out of phase (flashing keys, trembling mug, sweat); Idle folds arms and sips;
//! Off is tangled asleep with ZZZ. A surge smokes, sparks and shakes under a !!! bubble.

use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const KB: [(f32, f32, f32, f32); 3] = [(14.0, 50.0, 15.0, 6.0), (32.0, 50.0, 16.0, 6.0), (51.0, 50.0, 15.0, 6.0)];
const SH: [(f32, f32); 8] = [(31.0, 43.0), (32.0, 48.0), (34.0, 51.0), (46.0, 51.0), (48.0, 48.0), (49.0, 43.0), (32.5, 38.0), (47.5, 38.0)];
const TIP: [(f32, f32); 8] = [(19.5, 50.0), (25.0, 50.0), (35.0, 50.0), (44.0, 50.0), (53.5, 50.0), (59.0, 50.0), (16.5, 34.0), (63.0, 38.0)];

pub struct Octopus { fx: Particles<32>, pending: u8, timer: f32, seed: u32 }
impl Octopus {
    pub fn new() -> Self { Self { fx: Particles::new(), pending: 0, timer: 0.0, seed: 7 } }
    fn puff(&mut self, rng: &mut dyn Rng, mag: f32) {
        let kb = (rng.gen_f64() * 3.0) as usize % 3;
        let (bx, by, bw, _) = KB[kb];
        let spark = rng.gen_f64() < 0.35;
        let (vy, life, size, col) = if spark {
            (rand(rng, -40.0, -22.0), rand(rng, 0.6, 0.9), rand(rng, 1.4, 2.0), pal::YELLOW)
        } else {
            (rand(rng, -16.0, -9.0), rand(rng, 1.1, 1.7), rand(rng, 2.6, 3.8), pal::GREY)
        };
        let _ = mag;
        self.fx.spawn(Particle { x: bx + rand(rng, 2.0, bw - 2.0), y: by - 1.0, vx: rand(rng, -7.0, 7.0), vy, life, max: life, col, size });
    }
}

impl Default for Octopus { fn default() -> Self { Self::new() } }

impl Scene for Octopus {
    fn reset(&mut self, rng: &mut dyn Rng) { self.fx.clear(); self.pending = 0; self.timer = 0.0; self.seed = (rng.gen_f64() * 1e6) as u32; }
    fn surge(&mut self, mag: f32, rng: &mut dyn Rng) { self.pending = self.pending.saturating_add((14.0 + 26.0 * mag) as u8).min(32); for _ in 0..3 { self.puff(rng, mag); } }
    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.fx.step(dt, -24.0, 0.88);
        if cx.boost > 0.02 && cx.state != FlameState::Off {
            self.timer += dt * (12.0 + 36.0 * cx.surge_mag) * cx.motion;
            while self.timer >= 1.0 && self.pending > 0 {
                self.timer -= 1.0;
                self.pending -= 1;
                self.puff(rng, cx.surge_mag);
            }
            if self.pending == 0 {
                self.timer = 0.0;
            }
        }
    }
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let surging = !off && cx.boost > 0.02;
        let typing = live || surging;
        let t = if live { cx.t } else { 0.0 };
        let (b, mag, m, d) = (cx.boost, cx.surge_mag, cx.motion, cx.detail);
        let k = (cx.surge_age * 30.0) as u32;
        let (ox, oy) = if surging { ((hash01(k) - 0.5) * 2.5 * b, (hash01(k + 9) - 0.5) * 2.0 * b) } else { (0.0, 0.0) };
        c.ellipse(40.0, 57.5, 20.0, 1.8, pal::SHADOW);
        if surging && d >= 2 {
            c.ellipse(40.0 + ox, 51.0 + oy, 16.0 + 6.0 * mag * b, 5.0 + 2.0 * mag * b, pal::GLOW);
        }
        for (i, &(x, y, w, h)) in KB.iter().enumerate() {
            kb(c, x, y, w, h, self.seed + i as u32 * 31, t, live && !surging, b, d);
        }
        c.line(30.5 + ox, 34.0 + oy, 37.0 + ox, 28.0 + oy, 2.0, pal::YELLOW); c.rect(29.0 + ox, 33.5 + oy, 3.0, 3.0, pal::HOTPINK);
        if typing {
            let freq = (10.0 + 14.0 * b) * m;
            for i in 0..8 {
                let (sx, sy) = SH[i];
                let (tx, ty) = TIP[i];
                let ph = i as f32 * 0.93;
                let bob = if i < 6 {
                    -(((t * freq + ph).sin() * 0.5 + 0.5) * (1.1 + 2.2 * b) * m)
                } else {
                    (t * 3.0 * m + ph).sin() * 1.1 * m + if surging { (hash01(k + i as u32) - 0.5) * 3.0 * b } else { 0.0 }
                };
                let ddx = if i < 6 { (t * freq + ph + 1.0).sin() * 0.6 * m } else { bob * 0.5 };
                let bend = if tx < 40.0 { -2.5 } else { 2.5 };
                let ey = (sy + ty) * 0.5 + if i < 6 { 2.0 } else { -1.0 } + bob * 0.4;
                let (ex, ey) = ((sx + tx) * 0.5 + bend, ey);
                arm(c, (sx + ox, sy + oy), (ex + ox, ey + oy), (tx + ddx + ox, ty + bob + oy));
                if surging && b > 0.25 {
                    c.line(sx + ox, sy + oy, tx + ox + (hash01(k + i as u32 + 5) - 0.5) * 5.0 * b, ty + oy + bob + (hash01(k + i as u32 + 6) - 0.5) * 4.0 * b, 1.0, pal::LTPURPLE);
                }
            }
        }
        let bx = 40.0 + ox;
        let by = 40.5 + oy;
        c.ellipse(bx, by, 11.0, 9.5, pal::PURPLE);
        c.ellipse(bx, by + 3.5, 7.5, 5.0, pal::LTPURPLE);
        if d >= 1 {
            c.ellipse(bx - 4.0, by - 5.5, 5.0, 2.2, pal::LTPURPLE);
        }
        for (hx, hy, hr) in [(bx - 6.0, by - 8.0, 2.4), (bx, by - 9.5, 2.8), (bx + 6.0, by - 8.0, 2.4)] {
            c.ellipse(hx, hy, hr, hr * 0.7, pal::DKPURPLE);
        }
        let ey = by - 3.0;
        if off {
            for s in [-1.0f32, 1.0] {
                let ex = bx + s * 4.5;
                c.line(ex - 2.2, ey, ex + 2.2, ey, 1.1, pal::BLACK);
                c.ring(ex, ey, 3.8, 3.8, 0.9, pal::DKGREY);
            }
            c.line(bx - 4.5, ey, bx + 4.5, ey, 0.7, pal::DKGREY);
            c.ellipse(bx, by + 4.5, 1.6, 1.2, pal::DKRED);
        } else {
            let blink = live && (t * 0.33 + 0.4).fract() < 0.06;
            let pupil = if surging { 0.0 } else { (t * 3.7).sin() * 0.8 * m };
            for s in [-1.0f32, 1.0] {
                let ex = bx + s * 4.5;
                c.ellipse(ex, ey, 3.0, 3.3, pal::WHITE);
                if blink {
                    c.rect(ex - 3.0, ey - 3.3, 6.0, 6.6, pal::PURPLE);
                    c.line(ex - 2.2, ey, ex + 2.2, ey, 1.0, pal::BLACK);
                } else if surging {
                    c.ellipse(ex, ey, 1.0, 1.3, pal::BLACK);
                } else {
                    c.ellipse(ex + pupil, ey + 0.3, 1.4, 1.8, pal::BLACK); if d >= 1 { c.dot(ex + pupil + 0.3, ey - 0.6, pal::WHITE); }
                }
                if !live && !surging {
                    c.rect(ex - 3.0, ey - 3.3, 6.0, 2.8, pal::PURPLE);
                }
                c.ring(ex, ey, 3.8, 3.8, 0.9, pal::DKGREY);
            }
            c.line(bx - 4.5, ey, bx + 4.5, ey, 0.7, pal::DKGREY);
            let my = by + 4.5;
            if surging {
                c.ellipse(bx, my, 2.2, 2.6, pal::DKRED);
            } else if live {
                c.rect(bx - 3.5, my - 0.8, 7.0, 1.6, pal::BLACK);
                c.rect(bx - 3.0, my - 0.5, 2.0, 1.0, pal::WHITE);
                c.rect(bx + 0.5, my - 0.5, 2.0, 1.0, pal::WHITE);
            } else {
                c.line(bx - 2.5, my, bx + 2.5, my, 1.0, pal::BLACK);
            }
            c.dot(bx - 8.0, by + 1.5, pal::HOTPINK); c.dot(bx + 8.0, by + 1.5, pal::HOTPINK);
        }
        if typing {
            let (px, py) = (TIP[6].0 + ox, TIP[6].1 + oy + (t * 3.0 * m).sin() * 0.8 * m);
            phone(c, px + 1.0, py + 1.0, d >= 1);
            let mx = TIP[7].0 + ox + (t * 26.0).sin() * 0.6 * m + (if surging { (hash01(k + 3) - 0.5) * 2.0 * b } else { 0.0 });
            mug(c, mx, TIP[7].1 + oy + 2.5 + (t * 26.0).cos() * 0.5 * m, d >= 1);
            for i in 0..4 {
                let f = (t * 0.9 * m + i as f32 * 0.25).fract();
                let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                let sx = bx + side * (11.0 + f * 4.0);
                let sy = by - 7.0 + f * 9.0;
                c.rect(sx, sy, 1.2, 2.2, pal::LTBLUE);
            }
        }
        if !typing && !off {
            for (i, (x0, y0)) in [(33.0, 44.0), (33.0, 47.0), (34.0, 50.0)].iter().enumerate() {
                let yv = y0 + oy;
                c.line(x0 + ox, yv, 47.0 + ox, yv + 0.5, 2.8, if i == 1 { pal::PURPLE } else { pal::DKPURPLE });
            }
            c.line(47.5 + ox, 38.0 + oy, 52.0 + ox, 41.0 + oy, 2.8, pal::DKPURPLE);
            c.line(52.0 + ox, 41.0 + oy, 50.0 + ox, 43.0 + oy, 2.2, pal::PURPLE);
            c.ellipse(50.0 + ox, 43.0 + oy, 1.6, 1.6, pal::LTPURPLE);
            mug(c, 49.5 + ox, 42.5 + oy, d >= 1);
            phone(c, 32.5 + ox, 45.5 + oy, false);
        }
        if off {
            for (a, b2, col) in [((32.0, 41.0), (48.0, 51.0), pal::DKPURPLE), ((48.0, 41.0), (32.0, 51.0), pal::PURPLE), ((30.0, 46.0), (50.0, 44.0), pal::DKPURPLE)] {
                c.line(a.0 + ox, a.1 + oy, b2.0 + ox, b2.1 + oy, 2.4, col);
            }
            for (cx2, cy2) in [(32.0, 51.0), (48.0, 51.0), (30.0, 46.0), (50.0, 44.0)] {
                c.ring(cx2 + ox, cy2 + oy, 2.2, 2.2, 1.1, pal::LTPURPLE);
            }
            c.text(50.0, 36.0, "Z", pal::LTBLUE, 1.0);
            c.text(55.0, 30.0, "Z", pal::LTBLUE, 1.3);
            c.text(60.0, 24.0, "Z", pal::LTBLUE, 1.6);
        }
        if surging && b > 0.05 {
            let w = Canvas::text_width("!!!", 1.0) + 4.0;
            let (bxp, byp) = (49.0 + ox + (hash01(k + 1) - 0.5) * 2.0 * b, 23.0 + oy);
            c.rect(bxp - 1.0, byp - 1.0, w + 2.0, 11.0, pal::DKGREY);
            c.rect(bxp, byp, w, 9.0, pal::WHITE);
            c.tri((bxp + 2.0, byp + 9.0), (bxp + 8.0, byp + 9.0), (bx + 4.0, by - 8.0 + oy), pal::WHITE);
            c.text(bxp + 2.0, byp + 2.0, "!!!", pal::RED, 1.0);
        }
        for p in self.fx.iter() {
            let s = p.size * (0.4 + 0.6 * (p.life / p.max));
            c.rect(p.x - s * 0.5, p.y - s * 0.5, s, s, p.col);
        }
    }
}

fn kb(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, seed: u32, t: f32, flash: bool, boost: f32, detail: u8) {
    c.rect(x, y, w, h, pal::LTGREY);
    c.rect(x, y, w, 1.2, pal::WHITE);
    let tick = (t * 10.0) as u32;
    for r in 0..2 {
        for k2 in 0..4 {
            let kx = x + 1.5 + k2 as f32 * 2.9;
            let ky2 = y + 1.6 + r as f32 * 1.9;
            let lit = flash && hash01(seed.wrapping_add(tick.wrapping_mul(7)).wrapping_add(r as u32 * 10 + k2 as u32)) > 0.72;
            let col = if lit { pal::YELLOW } else if boost > 0.3 { pal::ORANGE } else { pal::DKGREY };
            c.rect(kx, ky2, 2.0, 1.3, col);
            if detail >= 2 && lit {
                c.ellipse(kx + 1.0, ky2 + 0.6, 2.4, 1.6, pal::GLOW);
            }
        }
    }
    if detail >= 1 { c.rect(x + 3.5, y + h - 1.1, w - 7.0, 0.8, pal::DKGREY); }
}

fn arm(c: &mut Canvas, s: (f32, f32), e: (f32, f32), tip: (f32, f32)) {
    c.line(s.0, s.1, e.0, e.1, 3.0, pal::DKPURPLE);
    c.line(e.0, e.1, tip.0, tip.1, 2.2, pal::PURPLE);
    c.ellipse(tip.0, tip.1, 1.7, 1.7, pal::LTPURPLE);
}

fn mug(c: &mut Canvas, x: f32, y: f32, detail: bool) {
    c.rect(x - 2.6, y - 2.6, 5.2, 5.2, pal::WHITE);
    c.rect(x - 2.6, y - 2.6, 5.2, 1.2, pal::DKBROWN);
    c.ring(x + 3.2, y + 0.2, 1.7, 2.0, 0.9, pal::WHITE); if detail { c.rect(x - 1.8, y - 1.0, 1.0, 3.0, pal::LTPINK); }
}

fn phone(c: &mut Canvas, x: f32, y: f32, lit: bool) {
    c.rect(x - 2.6, y - 3.6, 5.2, 7.2, pal::BLACK);
    c.rect(x - 1.9, y - 2.5, 3.8, 5.0, if lit { pal::CYAN } else { pal::LTBLUE });
    c.dot(x - 1.2, y - 1.8, pal::WHITE);
}
