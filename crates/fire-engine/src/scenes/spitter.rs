//! Token Spitter robot: a cream box robot bonked by a red mallet, spitting gold coins.
//! Live: hammer slams every ~0.85s, squash, starburst, "BONK!", orbiting stars, coin spit.
//! Idle: patient robot, mallet up, still. Off: dark screen, flat line, mallet down, ZZZ.
//! Surge: one huge bonk, shaking robot, giant burst, coin shower. Pure in t/surge_age.
use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;
use std::f32::consts::TAU;
const P: f32 = 0.85;
const OFF: f32 = 1.8;
const UP: (f32, f32) = (48.0, 27.0);
const DOWN: (f32, f32) = (28.0, 33.0);
const SLAM: f32 = 0.15;
pub struct Spitter { coins: Particles<32>, last: i32, fired: bool }
impl Spitter { pub fn new() -> Self { Self { coins: Particles::new(), last: 1, fired: true } } }
impl Default for Spitter { fn default() -> Self { Self::new() } }
fn clk(cx: &Ctx) -> f32 { cx.t * cx.motion / P + OFF }
fn sm(a: f32, b: f32, x: f32) -> f32 { let t = ((x - a) / (b - a)).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) }
fn gag(c: &mut Canvas, x: f32, y: f32, s: &str, col: Col, sc: f32) { c.text(x + 1.0, y + 1.0, s, pal::OUTLINE, sc); c.text(x, y, s, col, sc); }
fn star(c: &mut Canvas, x: f32, y: f32, s: f32, col: Col) { c.tri((x, y - s), (x - s, y), (x + s, y), col); c.tri((x, y + s), (x - s, y), (x + s, y), col); }
fn burst(c: &mut Canvas, x: f32, y: f32, r: f32) {
    c.ellipse(x, y, r * 0.55, r * 0.42, pal::YELLOW);
    for i in 0..8 { let a = i as f32 * 0.7854 + 0.3; c.tri((x + r * 0.45 * (a - 0.4).cos(), y + r * 0.35 * (a - 0.4).sin()), (x + r * a.cos(), y + r * a.sin()), (x + r * 0.45 * (a + 0.4).cos(), y + r * 0.35 * (a + 0.4).sin()), pal::YELLOW); }
}
fn coin(c: &mut Canvas, x: f32, y: f32, rx: f32, ry: f32, d: u8) {
    c.ellipse(x, y, rx + 0.7, ry + 0.7, pal::DKORANGE); c.ellipse(x, y, rx, ry, pal::GOLD);
    if d >= 1 && rx > 1.4 { c.ellipse(x, y, rx * 0.45, ry * 0.45, pal::YELLOW); }
    if d >= 2 && rx > 2.0 { c.dot(x - rx * 0.3, y - ry * 0.3, pal::WHITE); }
}
fn mallet(c: &mut Canvas, hx: f32, hy: f32, s: f32, d: u8) {
    let (hand_x, hand_y) = (hx + 19.0 * s, hy - 2.0 * s);
    c.line(hand_x, hand_y, hx, hy, 3.0 * s, pal::BROWN);
    let (w, h) = (18.0 * s, 15.0 * s);
    c.rect(hx - w * 0.5, hy - h * 0.5, w, h, pal::RED);
    c.rect(hx - w * 0.30, hy - h * 0.5, w * 0.15, h, pal::GOLD); c.rect(hx + w * 0.15, hy - h * 0.5, w * 0.15, h, pal::GOLD);
    c.rect(hx - w * 0.5, hy + h * 0.5 - 2.0 * s, w, 2.0 * s, pal::DKRED);
    if d >= 1 { c.rect(hx - w * 0.5, hy - h * 0.5, w, 1.2 * s, pal::LTPINK); }
    c.ellipse(hand_x, hand_y, 4.2 * s, 3.2 * s, pal::WHITE);
    if d >= 1 { c.line(hand_x - 1.5 * s, hand_y - 1.0, hand_x + 0.5 * s, hand_y - 1.0, 0.8, pal::DKGREY); c.line(hand_x - 1.5 * s, hand_y + 1.0, hand_x + 0.5 * s, hand_y + 1.0, 0.8, pal::DKGREY); }
}
impl Scene for Spitter {
    fn reset(&mut self, _rng: &mut dyn Rng) { self.coins.clear(); self.last = 1; self.fired = true; }
    fn surge(&mut self, _mag: f32, _rng: &mut dyn Rng) { self.fired = false; }
    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.coins.step(dt, 85.0, 0.85);
        if cx.state == FlameState::Off { return; }
        let surging = cx.boost > 0.01 && cx.surge_age < cx.surge_len;
        if surging {
            if !self.fired && cx.surge_age >= SLAM {
                self.fired = true;
                let (mag, d) = (cx.surge_mag, cx.detail.min(2) as usize);
                let n = 10 + (14.0 * mag) as usize; let cap = [12, 20, 28][d];
                for _ in 0..n.min(cap) { let mut life = rand(rng, 1.0, 1.3); if cx.state != FlameState::Live { life = life.min((cx.surge_len - cx.surge_age).max(0.15)); } let sz = rand(rng, 2.2, 3.2); self.coins.spawn(Particle { x: rand(rng, 30.0, 34.0), y: rand(rng, 41.0, 44.0), vx: rand(rng, 32.0, 48.0) + 18.0 * mag, vy: rand(rng, -34.0, -8.0), life, max: life, col: pal::GOLD, size: sz }); }
                for _ in 0..6 { let mut life = rand(rng, 0.6, 0.9); if cx.state != FlameState::Live { life = life.min((cx.surge_len - cx.surge_age).max(0.15)); } self.coins.spawn(Particle { x: 32.0, y: 43.0, vx: rand(rng, 16.0, 42.0), vy: rand(rng, -30.0, -5.0), life, max: life, col: pal::YELLOW, size: 1.2 }); }
            }
            return;
        }
        if cx.state == FlameState::Live {
            let idx = clk(cx).floor() as i32;
            if idx > self.last {
                self.last = idx; let d = cx.detail.min(2) as usize;
                for _ in 0..[4, 6, 8][d] { let life = rand(rng, 0.9, 1.25); let sz = rand(rng, 2.2, 3.0); self.coins.spawn(Particle { x: rand(rng, 30.0, 34.0), y: rand(rng, 41.0, 44.0), vx: rand(rng, 28.0, 48.0), vy: rand(rng, -28.0, -6.0), life, max: life, col: pal::GOLD, size: sz }); }
                for _ in 0..[2, 3, 4][d] { let life = rand(rng, 0.6, 0.9); self.coins.spawn(Particle { x: 32.0, y: 43.0, vx: rand(rng, 16.0, 40.0), vy: rand(rng, -24.0, -5.0), life, max: life, col: pal::YELLOW, size: 1.2 }); }
            }
        }
    }
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off; let live = cx.state == FlameState::Live;
        let surging = !off && cx.boost > 0.01 && cx.surge_age < cx.surge_len;
        let d = cx.detail; let mag = cx.surge_mag; let a = cx.surge_age;
        let k = clk(cx); let w = k.fract(); let since = w * P;
        let mut sq = 0.0; let mut hd = 0.0; let mut hs = 1.0; let mut burst_r = 0.0; let mut bonk = false; let mut bsc = 1.15;
        let mut hx_m = UP.0; let mut hy_m = UP.1;
        if surging {
            hd = if a < 0.04 { 0.0 } else if a < SLAM { sm(0.04, SLAM, a) } else if a < 0.45 { 1.0 } else if a < 0.80 { 1.0 - sm(0.45, 0.80, a) } else { 0.0 };
            hs = 1.10 + 0.60 * mag; sq = if a > SLAM { (-6.0 * (a - SLAM)).exp() } else { 0.0 };
            burst_r = if a > SLAM { (7.0 + 6.0 * mag) * (-4.5 * (a - SLAM)).exp() } else { 0.0 };
            bonk = (SLAM..0.85).contains(&a); bsc = 1.20 + 0.50 * mag;
        } else if live {
            hd = if w < 0.10 { 1.0 } else if w < 0.25 { 1.0 - sm(0.10, 0.25, w) } else if w < 0.60 { 0.0 } else if w < 0.78 { sm(0.60, 0.78, w) } else { 1.0 };
            sq = (-9.0 * since).exp(); burst_r = if sq > 0.12 { 3.0 + 8.0 * sq } else { 0.0 }; bonk = since < 0.26;
            hx_m = UP.0 + (DOWN.0 - UP.0) * hd; hy_m = UP.1 + (DOWN.1 - UP.1) * hd + (k * TAU).sin() * 0.5 * (1.0 - hd) + hd * sq * 1.2;
        } else if off { (hx_m, hy_m, hd, hs) = (54.0, 50.0, 0.0, 1.0); }
        if surging { hx_m = UP.0 + (DOWN.0 - UP.0) * hd; hy_m = UP.1 + (DOWN.1 - UP.1) * hd + hd * sq * 1.2; }
        let shake = if surging { (hash01((a * 30.0) as u32) - 0.5) * 3.0 * cx.boost } else { 0.0 };
        let bob = if live && !surging { (k * TAU).sin() * 0.4 } else { 0.0 };
        let (dx, ry) = (shake, bob);
        c.ellipse(30.0 + dx, 57.5, 20.0, 2.0, pal::SHADOW);
        let bh = 8.0 * (1.0 - 0.12 * sq); let by = 57.0 - bh + ry; let bw = 16.0 * (1.0 + 0.10 * sq);
        let hw = 24.0 * (1.0 + 0.12 * sq); let hh = 13.0 * (1.0 - 0.16 * sq); let hx = 28.0 + dx - hw * 0.5; let hy = by - hh;
        c.line(16.0 + dx, 46.0 + ry, 11.0 + dx, 52.0 + ry, 3.0, pal::CREAM); c.line(40.0 + dx, 46.0 + ry, 44.0 + dx, 52.0 + ry, 3.0, pal::CREAM);
        c.ellipse(11.0 + dx, 52.0 + ry, 3.0, 2.5, pal::WHITE); c.ellipse(44.0 + dx, 52.0 + ry, 3.0, 2.5, pal::WHITE);
        c.ellipse(15.0 + dx, 43.0 + ry, 2.5, 3.5, if off { pal::DKBLUE } else { pal::BLUE }); c.ellipse(41.0 + dx, 43.0 + ry, 2.5, 3.5, if off { pal::DKBLUE } else { pal::BLUE });
        c.rect(28.0 + dx - bw * 0.5, by, bw, bh, pal::CREAM);
        if d >= 1 { c.rect(28.0 + dx - bw * 0.5, by + bh - 1.5, bw, 1.5, pal::TAN); c.rect(28.0 + dx - bw * 0.5, by, bw, 1.0, pal::WHITE); }
        let (cx0, cy0) = (28.0 + dx - 5.0, by + bh - 6.5); c.rect(cx0, cy0, 10.0, 5.5, pal::BLACK); c.text(cx0 + 1.5, cy0 + 0.3, "AI", pal::CYAN, 1.0);
        c.rect(hx, hy, hw, hh, pal::CREAM);
        if d >= 1 { c.rect(hx, hy, hw, 1.5, pal::WHITE); c.rect(hx, hy + hh - 1.5, hw, 1.5, pal::TAN); }
        let (vx, vy, vw, vh) = (hx + 3.0, hy + 3.0, hw - 6.0, (8.0 * (1.0 - 0.10 * sq)).max(6.0));
        c.rect(vx, vy, vw, vh, pal::BLACK);
        let wob = if live && !surging { sq * 2.0 * (k * 20.0).sin() } else { 0.0 };
        let (ax, ay) = if off { (30.0 + dx, 31.0 + ry) } else { (28.0 + dx + wob, 30.0 + ry) };
        c.line(28.0 + dx + wob * 0.3, hy, ax, ay, 1.0, pal::DKGREY); c.ellipse(ax, ay, 2.5, 2.5, if off { pal::DKGREY } else { pal::BLUE }); if !off && d >= 1 { c.dot(ax - 0.5, ay - 0.5, pal::LTBLUE); }
        if off {
            c.line(vx + 2.0, vy + 4.0, vx + 5.0, vy + 4.0, 1.0, pal::DKGREY); c.line(vx + vw - 5.0, vy + 4.0, vx + vw - 2.0, vy + 4.0, 1.0, pal::DKGREY);
            c.line(vx + vw * 0.45, vy + vh - 2.0, vx + vw * 0.65, vy + vh - 2.0, 1.0, pal::DKGREY);
        } else {
            let (mx, my) = (vx + vw * 0.55, vy + vh - 2.0); c.ellipse(mx, my, 1.5 + 2.0 * sq, 0.8 + 1.7 * sq, pal::HOTPINK);
            if sq > 0.25 {
                c.line(vx + 2.0, vy + 1.5, vx + 5.0, vy + 3.5, 1.5, pal::CYAN); c.line(vx + 5.0, vy + 3.5, vx + 2.0, vy + 5.5, 1.5, pal::CYAN);
                c.line(vx + vw - 2.0, vy + 1.5, vx + vw - 5.0, vy + 3.5, 1.5, pal::CYAN); c.line(vx + vw - 5.0, vy + 3.5, vx + vw - 2.0, vy + 5.5, 1.5, pal::CYAN);
            } else { let ox = if live && !surging { 0.0 } else { 0.8 }; c.ellipse(vx + 4.0 + ox, vy + 3.0, 1.8, 2.2, pal::CYAN); c.ellipse(vx + vw - 4.0 + ox, vy + 3.0, 1.8, 2.2, pal::CYAN); }
        }
        if !off {
            for p in self.coins.iter() {
                let y = p.y.min(56.0); let f = (p.life / 0.25).min(1.0);
                if p.size > 1.5 { let fx = (p.life * 10.0 + p.x * 0.5).cos().abs(); coin(c, p.x, y, (p.size * fx * f).max(0.7 * f), p.size * f, d); }
                else { c.rect(p.x - p.size * 0.5, y - p.size * 0.5, p.size, p.size, p.col); }
            }
        }
        if off { mallet(c, 54.0, 51.0, 1.0, d); gag(c, 44.0, 33.0, "Z", pal::LTBLUE, 1.0); gag(c, 50.0, 28.0, "Z", pal::LTBLUE, 1.3); gag(c, 56.0, 22.0, "Z", pal::LTBLUE, 1.6); return; }
        mallet(c, hx_m, hy_m, hs, d);
        if burst_r > 0.5 { let (bx, byy) = (28.0 + dx, hy + 1.0); if d >= 2 { c.ellipse(bx, byy, burst_r * 1.25, burst_r * 0.95, pal::GLOW); } burst(c, bx, byy, burst_r); }
        let orbits = live || surging;
        if orbits {
            let (ocx, ocy) = (28.0 + dx, hy + 2.0); let (rx, ry2) = ((14.0 + 4.0 * mag * if surging { 1.0 } else { 0.0}), (7.0 + 2.0 * mag * if surging { 1.0 } else { 0.0}));
            if d >= 1 { for i in 0..8 { let t = i as f32 * TAU / 8.0; c.dot(ocx + rx * t.cos(), ocy + ry2 * t.sin(), pal::YELLOW); } }
            let ang0 = if surging { a * TAU * 0.9 } else { k * TAU * 0.9 }; let n = if surging { 5 + (4.0 * mag) as usize } else if d >= 1 { 3 } else { 2 };
            for i in 0..n { let t = ang0 + i as f32 * TAU / n as f32; star(c, ocx + rx * t.cos(), ocy + ry2 * t.sin(), 2.8, pal::YELLOW); }
        }
        if bonk { gag(c, 8.0, 22.0, "BONK!", pal::YELLOW, bsc); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cx(state: FlameState, t: f32) -> Ctx { Ctx { state, t, motion: 1.0, detail: 1, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() } }
    #[test]
    fn idle_and_off_do_not_move_with_the_clock() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2)); let s = Spitter::new();
        s.draw(&mut a, &cx(FlameState::Idle, 0.0)); s.draw(&mut b, &cx(FlameState::Idle, 3.7));
        assert_eq!(a.pixels(), b.pixels()); s.draw(&mut a, &cx(FlameState::Off, 0.0)); s.draw(&mut b, &cx(FlameState::Off, 3.7));
        assert_eq!(a.pixels(), b.pixels());
    }
    #[test]
    fn a_bonk_spawns_coins_but_idle_does_not() {
        let mut rng = crate::XorShift64::new(1); let mut s = Spitter::new(); s.reset(&mut rng);
        for i in 0..40 { s.update(0.05, &cx(FlameState::Live, i as f32 * 0.05), &mut rng); }
        assert!(s.coins.len() > 0); let n = s.coins.len(); s.reset(&mut rng);
        for i in 0..40 { s.update(0.05, &cx(FlameState::Idle, i as f32 * 0.05), &mut rng); }
        assert!(s.coins.is_empty()); assert!(n <= 32);
    }
}
