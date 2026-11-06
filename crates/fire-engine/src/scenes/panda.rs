//! Sneezing panda: nose twitches, head tilts back, gold coin dust puffs out (ACHOO).
//! Surge is a GIANT sneeze: head jerks back, snaps forward, coins blast right, ACHOO!!.
//! Idle chews bamboo at the mouth; Off sleeps on its back with ZZZ. Pure in `t`/`surge_age`.
use crate::canvas::{pal::*, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

/// Seconds between nose-twitch puffs.
const PUFF: f32 = 1.5;

pub struct Panda { coins: Particles<32>, pending: u8, acc: f32, last_puff: u32 }
impl Panda { pub fn new() -> Self { Self { coins: Particles::new(), pending: 0, acc: 0.0, last_puff: 0 } } }
impl Default for Panda { fn default() -> Self { Self::new() } }
fn coin(c: &mut Canvas, x: f32, y: f32, rx: f32, ry: f32, rim: f32) {
    c.ellipse(x, y, rx + rim, ry + rim, DKYELLOW); c.ellipse(x, y, rx, ry, YELLOW);
    if rx > 1.2 { c.dot(x - 0.5, y - 0.5, DKYELLOW); }
}
/// A rounded box with a one-unit border (reads on light and dark desktops alike).
fn bubble(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, fill: Col, edge: Col) {
    for (g, k) in [(1.0, edge), (0.0, fill)] {
        c.rect(x - g + 1.0, y - g, w + 2.0 * g - 2.0, h + 2.0 * g, k); c.rect(x - g, y - g + 1.0, w + 2.0 * g, h + 2.0 * g - 2.0, k);
    }
}
fn mk(x: f32, vx: f32, vy: f32, life: f32, size: f32) -> Particle { Particle { x, y: 37.5, vx, vy, life, max: life, col: GOLD, size } }

impl Scene for Panda {
    fn reset(&mut self, _rng: &mut dyn Rng) { self.coins.clear(); self.pending = 0; self.acc = 0.0; self.last_puff = 0; }
    fn surge(&mut self, mag: f32, _rng: &mut dyn Rng) { self.pending = (self.pending as f32 + 4.0 + 20.0 * mag).min(24.0) as u8; }
    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.coins.step(dt, 120.0, 0.9);
        if cx.state == FlameState::Live {
            let idx = ((cx.t * cx.motion + 0.7) / PUFF).floor() as u32;
            if idx > self.last_puff {
                self.last_puff = idx;
                for _ in 0..5 { let l = rand(rng, 0.8, 1.1); self.coins.spawn(mk(38.0, rand(rng, 16.0, 30.0) * cx.motion, rand(rng, -10.0, 4.0), l, rand(rng, 1.0, 1.7))); }
            }
        }
        if self.pending > 0 {
            self.acc += dt * 30.0;
            while self.pending > 0 && self.acc >= 1.0 {
                self.acc -= 1.0; self.pending -= 1;
                let mut l = rand(rng, 1.0, 1.4);
                if cx.state != FlameState::Live { l = l.min((cx.surge_len - cx.surge_age).max(0.12)); } // nothing ticks after an idle surge ends
                let m = cx.surge_mag;
                self.coins.spawn(mk(38.5, rand(rng, 28.0, 60.0) * (0.5 + 0.9 * m) * (0.6 + 0.4 * cx.motion), rand(rng, -24.0, 16.0), l, rand(rng, 1.6, 2.6)));
            }
        }
        if self.pending == 0 { self.acc = 0.0; }
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        if cx.state == FlameState::Off { sleeper(c); return; }
        let (live, hi, hi2, m) = (cx.state == FlameState::Live, cx.detail >= 1, cx.detail >= 2, cx.motion);
        let t = if live { cx.t } else { 0.0 };
        let a = t * m;
        let ph = (a + 0.7) % PUFF;
        let puff = live && ph < 0.6;
        let build = if live { ((ph - (PUFF - 0.5)) / 0.5).clamp(0.0, 1.0) } else { 0.0 };
        let tw = if live { (a * 28.0).sin() * 0.6 * build * m + (a * 2.0).sin() * 0.15 * m } else { 0.0 };
        let sa = cx.surge_age;
        let surge = sa < cx.surge_len && cx.boost > 0.01;
        let mag = cx.surge_mag;
        let k = (sa * 40.0) as u32;
        let (mut ox, mut oy) = (0.0, 0.0);
        if surge {
            ox = (hash01(k) - 0.5) * 2.6 * cx.boost;
            oy = -hash01(k + 9) * 1.4 * cx.boost;
        }
        let mut hx = ox + tw * 0.3;
        let mut hy = oy + if live { (a * 2.0).sin() * 0.4 * m - 1.4 * build } else { 1.0 };
        let mut mouth = puff;
        let mut squint = live && build > 0.45;
        if surge {
            if sa < 0.25 {
                let f = sa / 0.25;
                hy -= 3.0 * (0.5 + 0.7 * mag) * f;
                hx -= 1.6 * f;
                squint = true;
            } else {
                let e = (-(sa - 0.25) * 5.0).exp();
                hx += 3.2 * (0.4 + 0.6 * mag) * e;
                hy += 0.8 * e + (sa * 20.0).sin() * 0.5 * cx.boost;
                mouth = sa < 0.7;
                squint = sa < 0.45;
            }
        }
        let blink = live && !squint && !mouth && (a / 2.7).fract() < 0.045 && hash01((a / 2.7) as u32) < 0.35;
        let (hx, hy) = (34.0 + hx, 33.0 + hy);

        c.ellipse(34.0 + ox, 57.5, 20.0, 2.2, SHADOW);
        c.ellipse(26.0 + ox, 54.5 + oy, 4.5, 2.6, BLACK);
        c.ellipse(42.0 + ox, 54.5 + oy, 4.5, 2.6, BLACK);
        c.ellipse(34.0 + ox, 47.0 + oy, 11.5, 8.0, LTGREY);
        c.ellipse(34.0 + ox, 46.4 + oy, 10.5, 7.3, WHITE);
        c.ellipse(34.0 + ox, 48.6 + oy, 7.0, 4.4, CREAM);
        c.ellipse(43.5 + ox, 44.0 + oy, 3.0, 5.0, BLACK);

        // bamboo: diagonal grip while live, horizontal at the mouth while idle
        if live || surge {
            c.line(26.0 + ox, 46.0 + oy, 29.0 + ox, 41.0 + oy, 3.2, BLACK);
            c.line(19.0 + ox, 47.0 + oy, 33.0 + ox, 39.0 + oy, 2.2, GREEN);
            c.line(24.0 + ox, 44.6 + oy, 26.5 + ox, 43.0 + oy, 1.0, DKGREEN);
            c.tri((19.0 + ox, 47.0 + oy), (15.5 + ox, 43.5 + oy), (20.5 + ox, 43.0 + oy), GREEN);
            c.tri((20.5 + ox, 46.0 + oy), (17.5 + ox, 42.5 + oy), (21.5 + ox, 42.5 + oy), DKGREEN);
            c.ellipse(29.5 + ox, 41.0 + oy, 3.0, 2.4, WHITE);
        } else { c.ellipse(hx - 5.0, hy + 7.7, 2.8, 2.2, WHITE); }
        // head
        for s in [-1.0f32, 1.0] { c.ellipse(hx + s * 8.0, hy - 6.0, 3.4, 3.4, BLACK); }
        c.ellipse(hx, hy, 10.0, 8.8, LTGREY);
        c.ellipse(hx, hy - 0.7, 9.0, 8.0, WHITE);
        for s in [-1.0f32, 1.0] {
            c.ellipse(hx + s * 4.2, hy + 0.5, 3.0, 2.3, BLACK);
            let (ex, ey) = (hx + s * 4.2, hy + 0.5);
            if squint {
                c.line(ex - 1.6, ey - 0.9, ex + 1.6, ey, 0.9, WHITE);
                c.line(ex - 1.6, ey + 0.9, ex + 1.6, ey, 0.9, WHITE);
            } else if blink {
                c.line(ex - 1.6, ey, ex + 1.6, ey, 0.9, WHITE);
                c.line(ex - 1.4, ey + 0.7, ex + 1.4, ey + 0.7, 0.7, WHITE);
            } else {
                c.ellipse(ex, ey, 1.6, 1.9, WHITE);
                let (lx, ly) = if live { ((a * 3.0).sin() * 0.5, -0.3) } else { (0.0, 0.5) };
                c.rect(ex - 0.7 + lx, ey - 0.9 + ly, 1.4, 1.7, BLACK);
                if hi { c.dot(ex - 0.4 + lx, ey - 0.6 + ly, WHITE); }
            }
        }
        c.ellipse(hx - 6.8, hy + 3.4, 1.5, 1.1, LTPINK);
        c.ellipse(hx + 6.8, hy + 3.4, 1.5, 1.1, LTPINK);
        let (nx, ny) = (hx + 0.8 + tw, hy + 4.0);
        c.ellipse(nx, ny, 1.9, 1.2, BLACK);
        if mouth {
            let ro = 1.5 + 0.8 * cx.boost + 0.6 * mag;
            c.ellipse(hx + 0.8, hy + 6.2, ro, ro * 0.8, BLACK);
            c.dot(hx + 0.8, hy + 6.7, HOTPINK);
        } else {
            c.line(hx - 0.5, hy + 6.2, hx + 2.0, hy + 6.2, 0.9, BLACK);
        }
        if squint && hi {
            c.line(hx - 3.5, hy - 4.5, hx - 1.0, hy - 3.8, 0.7, GREY);
            c.line(hx + 1.0, hy - 3.8, hx + 3.5, hy - 4.5, 0.7, GREY);
        }
        // idle: bamboo held across the mouth, chewing (drawn over the face so it reads)
        if !live && !surge {
            let my = hy + 6.2;
            c.line(hx - 13.0, my, hx + 8.0, my, 2.0, GREEN);
            c.tri((hx - 12.0, my), (hx - 16.0, my - 2.5), (hx - 11.0, my - 5.0), GREEN);
            c.tri((hx - 12.5, my + 0.5), (hx - 16.5, my + 2.5), (hx - 12.0, my + 5.0), DKGREEN);
        }

        if hi && live && build > 0.2 {
            for i in 0..3 { c.line(nx + 4.0 + i as f32 * 1.2, ny - 1.5 + i as f32 * 1.5, nx + 7.5 + i as f32 * 1.2, ny - 1.5 + i as f32 * 1.5, 0.8, GREY); }
        }
        for p in self.coins.iter() {
            let f = (p.life / 0.15).min(1.0);
            let rx = (p.size * (p.life * 13.0 + p.x).cos().abs()).max(0.7) * f;
            coin(c, p.x, p.y.min(55.5), rx, p.size * f, 0.7);
        }

        if surge && sa < 1.0 {
            let kb = (cx.boost * 4.0).min(1.0);
            if hi2 { c.ellipse(28.5, 27.0, 20.0 * kb, 10.0 * kb, GLOW); }
            c.ellipse(28.5, 27.0, 17.0 * kb, 8.0 * kb, YELLOW);
            for i in 0..6 {
                let (dx, dy) = [(18.0, 0.0), (13.0, 6.0), (0.0, 9.0), (-13.0, 6.0), (-18.0, 0.0), (0.0, -9.0)][i];
                c.tri((28.5 + dx * 0.85, 27.0 + dy * 0.7), (28.5 + dx, 27.0 + dy), (28.5 + dx * 0.85, 27.0 + dy * 0.7 + 2.5), DKYELLOW);
            }
            c.tri((30.0, 32.0), (36.0, 32.0), (31.0, 26.0), DKGREY);
            bubble(c, 13.0, 22.0, 31.0, 10.0, WHITE, DKGREY);
            c.tri((31.0, 31.6), (35.0, 31.6), (31.5, 26.5), WHITE);
            c.text(15.0, 24.5, "ACHOO!!", BLACK, 1.0);
        } else if puff {
            c.tri((30.0, 32.0), (36.0, 32.0), (30.0, 26.0), DKGREY);
            bubble(c, 15.0, 23.0, 24.0, 9.0, WHITE, DKGREY);
            c.tri((31.0, 31.6), (35.0, 31.6), (30.5, 26.5), WHITE);
            c.text(17.0, 25.0, "ACHOO", BLACK, 1.0);
        }
    }
}

fn sleeper(c: &mut Canvas) {
    c.ellipse(38.0, 57.5, 20.0, 2.2, SHADOW);
    c.line(12.0, 55.0, 30.0, 52.0, 2.0, GREEN);
    c.tri((13.0, 54.5), (10.0, 51.5), (15.0, 51.0), GREEN);
    c.tri((16.0, 53.5), (12.5, 50.5), (17.0, 50.5), DKGREEN);
    c.ellipse(38.0, 50.0, 15.0, 6.5, LTGREY);
    c.ellipse(38.0, 49.3, 14.0, 5.8, WHITE);
    c.ellipse(38.0, 50.5, 9.0, 3.5, CREAM);
    c.ellipse(49.0, 43.0, 3.6, 2.2, BLACK);
    c.ellipse(56.0, 44.5, 3.6, 2.2, BLACK);
    c.ellipse(35.0, 49.0, 4.0, 2.6, BLACK);
    c.ellipse(43.0, 49.5, 4.0, 2.6, BLACK);
    c.ellipse(15.0, 44.0, 3.2, 3.2, BLACK);
    c.ellipse(28.0, 44.0, 3.2, 3.2, BLACK);
    c.ellipse(21.5, 49.0, 8.0, 7.0, LTGREY);
    c.ellipse(21.5, 48.3, 7.2, 6.3, WHITE);
    for s in [-1.0f32, 1.0] {
        c.ellipse(21.5 + s * 3.5, 49.0, 2.6, 2.0, BLACK);
        c.line(21.5 + s * 3.5 - 1.4, 49.0, 21.5 + s * 3.5 + 1.4, 49.0, 0.9, WHITE);
    }
    c.ellipse(22.3, 52.0, 1.6, 1.0, BLACK);
    c.ellipse(22.3, 53.8, 0.9, 0.9, BLACK);
    c.ellipse(15.5, 51.0, 1.2, 0.9, LTPINK);
    c.ring(30.0, 51.0, 2.2, 2.6, 0.9, LTBLUE);
    c.text(31.0, 34.0, "Z", LTBLUE, 1.0);
    c.text(36.0, 28.0, "Z", LTBLUE, 1.3);
    c.text(42.0, 22.0, "Z", LTBLUE, 2.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cx(state: FlameState, t: f32, detail: u8) -> Ctx { Ctx { state, t, motion: 1.0, detail, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() } }
    #[test]
    fn an_idle_panda_looks_the_same_whatever_the_clock_says() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2)); let panda = Panda::new();
        panda.draw(&mut a, &cx(FlameState::Idle, 0.0, 1)); panda.draw(&mut b, &cx(FlameState::Idle, 3.7, 1));
        assert_eq!(a.pixels(), b.pixels());
    }
    #[test]
    fn an_idle_surge_shower_empties_before_the_surge_ends() {
        let mut rng = crate::XorShift64::new(1); let mut panda = Panda::new(); panda.surge(1.0, &mut rng);
        let mut most = 0;
        for i in 0..40 { let c = Ctx { surge_age: i as f32 * 0.05, surge_len: 2.4, surge_mag: 1.0, ..cx(FlameState::Idle, 0.0, 0) }; panda.update(0.05, &c, &mut rng); most = most.max(panda.coins.len()); }
        assert!(most > 5 && most <= 32, "{most}"); assert!(panda.pending == 0);
    }
}
