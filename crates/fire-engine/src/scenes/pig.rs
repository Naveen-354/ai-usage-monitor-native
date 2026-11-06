//! Piggy bank: gold "T" coins drop into its slot, each landing jiggles it (and sometimes blinks); now and then OINK.
//! A surge puffs it up, shakes its belly and shoots a coin shower out. Idle: still pig. Off: asleep, ZZZ, snot bubble.

use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const SLOT_X: f32 = 36.5;
/// Seconds between coins, and how long one coin takes to fall (so a second one is always on its way).
const P: f32 = 0.8;
const FALL: f32 = 1.2;

pub struct Pig {
    coins: Particles<28>,
    /// Shower coins still waiting to pop out of the slot, and the emission timer.
    pending: u8,
    acc: f32,
}

impl Pig { pub fn new() -> Self { Self { coins: Particles::new(), pending: 0, acc: 0.0 } } }
impl Default for Pig { fn default() -> Self { Self::new() } }

/// The pig's own space (38, 57 = where it stands) squashed, stretched and shaken onto the canvas.
#[derive(Clone, Copy)]
struct Xf {
    dx: f32,
    dy: f32,
    sx: f32,
    sy: f32,
}

impl Xf {
    fn x(&self, x: f32) -> f32 { 38.0 + self.dx + (x - 38.0) * self.sx }
    fn y(&self, y: f32) -> f32 { 57.0 + self.dy - (57.0 - y) * self.sy }
    fn e(&self, c: &mut Canvas, x: f32, y: f32, rx: f32, ry: f32, k: Col) { c.ellipse(self.x(x), self.y(y), rx * self.sx, ry * self.sy, k); }
    fn r(&self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, k: Col) { c.rect(self.x(x), self.y(y), w * self.sx, h * self.sy, k); }
    fn l(&self, c: &mut Canvas, a: (f32, f32), b: (f32, f32), th: f32, k: Col) { c.line(self.x(a.0), self.y(a.1), self.x(b.0), self.y(b.1), th, k); }
    fn t(&self, c: &mut Canvas, a: (f32, f32), b: (f32, f32), d: (f32, f32), k: Col) { c.tri((self.x(a.0), self.y(a.1)), (self.x(b.0), self.y(b.1)), (self.x(d.0), self.y(d.1)), k); }
}

fn coin(c: &mut Canvas, x: f32, y: f32, rx: f32, ry: f32, rim: f32) { c.ellipse(x, y, rx + rim, ry + rim, pal::DKYELLOW); c.ellipse(x, y, rx, ry, pal::YELLOW); }

/// A rounded box with a one-unit border (so it reads on light and dark desktops alike).
fn bubble(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, fill: Col, edge: Col) {
    for (g, k) in [(1.0, edge), (0.0, fill)] {
        c.rect(x - g + 1.0, y - g, w + 2.0 * g - 2.0, h + 2.0 * g, k);
        c.rect(x - g, y - g + 1.0, w + 2.0 * g, h + 2.0 * g - 2.0, k);
    }
}

/// A spiky shout bubble: an ellipse with eight spikes, `r` is roughly its half width.
fn burst(c: &mut Canvas, x: f32, y: f32, r: f32, spin: f32, fill: Col, edge: Col) {
    for (g, k) in [(1.2, edge), (0.0, fill)] {
        let (rx, ry) = (r * 0.55 + g, r * 0.4 + g);
        c.ellipse(x, y, rx, ry, k);
        for i in 0..8 {
            let a = spin + i as f32 * 0.7854;
            let p = |a: f32, s: f32| (x + rx * s * a.cos(), y + ry * s * a.sin());
            c.tri(p(a - 0.45, 0.95), p(a, 1.75), p(a + 0.45, 0.95), k);
        }
    }
}

impl Scene for Pig {
    fn reset(&mut self, _rng: &mut dyn Rng) { self.coins.clear(); self.pending = 0; self.acc = 0.0; }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.coins.step(dt, 170.0, 0.85);
        self.acc += dt * 30.0;
        while self.pending > 0 && self.acc >= 1.0 {
            self.acc -= 1.0;
            self.pending -= 1;
            let mut life = rand(rng, 1.3, 1.6);
            if cx.state != FlameState::Live {
                // nothing keeps ticking after an idle surge ends: every coin must be gone by then
                life = life.min((cx.surge_len - cx.surge_age).max(0.1));
            }
            let (cap, m) = ([10, 18, 28][cx.detail.min(2) as usize], cx.surge_mag);
            if self.coins.len() < cap {
                let (vx, vy) = (rand(rng, -1.0, 1.0) * (20.0 + 34.0 * m), -rand(rng, 46.0, 68.0) * (0.8 + 0.4 * m));
                let size = rand(rng, 1.9, 2.4);
                self.coins.spawn(Particle { x: SLOT_X + rand(rng, -4.0, 4.0), y: 31.0, vx, vy, life, max: life, col: pal::YELLOW, size });
            }
        }
        if self.pending == 0 { self.acc = 0.0; }
    }

    fn surge(&mut self, mag: f32, _rng: &mut dyn Rng) { self.pending = (self.pending as f32 + 4.0 + 20.0 * mag).min(28.0) as u8; }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let (off, live, d, m) = (cx.state == FlameState::Off, cx.state == FlameState::Live, cx.detail, cx.motion);
        let t2 = cx.t * m + 0.6;
        let jl = ((t2 - FALL) / P).floor(); // the coin that landed last
        let tau = t2 - jl * P - FALL; // seconds since it did
        let b = if off { 0.0 } else { cx.boost };
        let s = (cx.surge_age / 0.2).min(1.0);
        let puff = b * s * s * (3.0 - 2.0 * s); // swells fast, deflates with the boost
        let k = (cx.surge_age * 40.0) as u32;
        let mut xf = Xf { dx: 0.0, dy: 0.0, sx: 1.0 + 0.26 * puff, sy: 1.0 + 0.2 * puff };
        let mut shut = off;
        if off { (xf.sx, xf.sy) = (1.03, 0.94); }
        if live {
            let o = (-5.0 * tau).exp() * (22.0 * tau).cos() * m;
            xf.sx += 0.06 * o;
            xf.sy -= 0.1 * o;
            shut |= hash01(jl as i32 as u32) < 0.5 && (0.08..0.3).contains(&tau) && b < 0.05;
        }
        if puff > 0.0 { xf.dx = (hash01(k) - 0.5) * 2.4 * b; xf.dy = -hash01(k + 9) * 1.2 * b; }
        let bubble_age = {
            let (cyc, ph) = ((cx.t / 5.0).floor(), cx.t % 5.0);
            let from = 1.2 + 1.5 * hash01(cyc as u32 * 3 + 1);
            if live && hash01(cyc as u32 * 3 + 2) > 0.25 { ph - from } else { -1.0 }
        };
        let kb = (cx.boost * 4.0).min(1.0);

        if b > 0.02 { burst(c, 61.0, 25.0, (14.0 + 5.0 * cx.surge_mag) * kb, cx.surge_age * 0.7, pal::YELLOW, pal::DKRED); }
        if live {
            for i in 0..2 {
                let age = t2 - (jl + 1.0 + i as f32) * P;
                if (0.0..FALL).contains(&age) {
                    let u = age / FALL;
                    let mut rx = 4.2 * (7.0 * (0.86 * FALL - age)).sin().abs();
                    if u > 0.86 {
                        rx = rx.min(1.1);
                    }
                    let (x, y) = (xf.x(SLOT_X), -6.0 + (xf.y(36.0) + 6.0) * u);
                    coin(c, x, y, rx, 4.2, 0.9);
                    if rx > 3.6 {
                        c.text(x - 1.5, y - 2.5, "T", pal::BROWN, 1.0);
                    }
                }
            }
        }

        // the pig: shadow, tail, legs, body (darker rim = shading and a border), belly, ear, slot, snout, eye
        c.ellipse(38.0 + xf.dx, 57.2, 21.5 * xf.sx, 2.2, pal::SHADOW);
        let wag = if live { (cx.t * m * 9.0).sin() * 0.9 } else { 0.0 };
        let (tx, ty) = (xf.x(15.0 - 1.5 * puff + wag), xf.y(37.5 - puff));
        let r = 3.3 * (1.0 - 0.4 * puff);
        xf.l(c, (19.5, 42.0), (15.0 - 1.5 * puff + wag + 1.5, 39.5 - puff), 1.6, pal::HOTPINK);
        c.ring(tx, ty, r * xf.sx, r * xf.sy, 1.3, pal::HOTPINK);
        c.rect(tx + 0.8, ty - 4.4, 4.0, 3.0, Col(0)); // a gap in the ring: it curls
        for lx in [21.0, 27.0, 44.0, 50.0] {
            xf.r(c, lx, 47.0, 5.0, 10.0, pal::HOTPINK);
            xf.r(c, lx, 55.5, 5.0, 1.5, pal::DKRED);
        }
        xf.e(c, 38.0, 41.9, 20.4, 11.6, pal::HOTPINK);
        xf.e(c, 38.0, 41.2, 19.6, 10.6, pal::PINK);
        xf.e(c, 37.0 + (hash01(k + 17) - 0.5) * 3.0 * puff, 47.5, 12.0, 3.0, pal::LTPINK);
        if d > 0 { xf.e(c, 28.0, 35.0, 5.0, 1.5, pal::LTPINK); }
        let ear = 2.5 * puff; // ears prick sideways when it puffs, but stay inside the safe area
        xf.t(c, (40.5, 34.0), (45.5 - ear, 28.0), (50.0, 33.5), pal::HOTPINK);
        xf.t(c, (42.8, 33.2), (45.5 - ear, 29.8), (47.6, 33.2), pal::LTPINK);
        xf.r(c, 31.0, 29.6, 11.0, 2.6, pal::DKRED);
        xf.r(c, 31.8, 30.4, 9.4, 1.0, pal::BLACK);
        xf.e(c, 59.5, 42.5, 5.6, 4.6, pal::HOTPINK);
        xf.e(c, 59.2, 42.3, 4.7, 3.7, pal::LTPINK);
        for nx in [58.0, 61.0] {
            xf.r(c, nx, 41.1, 1.2, 2.2 + puff, pal::DKRED);
        }
        xf.e(c, 47.5, 43.0, 1.8, 1.1, pal::HOTPINK);
        if shut {
            xf.l(c, (47.8, 37.3), (50.5, 39.3), 1.1, pal::BLACK);
            xf.l(c, (50.5, 39.3), (53.2, 37.3), 1.1, pal::BLACK);
        } else {
            xf.e(c, 50.5, 38.0, 2.9 + 0.6 * puff, 3.3 + 0.7 * puff, pal::WHITE);
            let (lx, ly) = if puff > 0.1 { (0.0, 0.0) } else if live { (-0.9, -0.8) } else { (0.8, 0.0) };
            xf.e(c, 50.5 + lx, 38.5 + ly, 1.6 - 0.6 * puff, 2.1 - 0.7 * puff, pal::BLACK);
            xf.r(c, 50.2 + lx, 37.3 + ly, 1.0, 1.0, pal::WHITE);
        }
        if puff > 0.25 && d > 0 {
            for sg in [-1.0, 1.0] {
                for j in 0..2 {
                    let (ex, ey) = (xf.x(38.0 + sg * 28.0) + sg * 3.0, xf.y(39.5) + j as f32 * 4.0);
                    c.line(ex, ey, ex + sg * 3.0, ey - 1.5, 1.0, pal::GREY);
                }
            }
        }

        for p in self.coins.iter() {
            let f = (p.life / 0.15).min(1.0);
            let rx = (p.size * (p.life * 15.0 + p.x).cos().abs()).max(0.7) * f;
            coin(c, p.x, p.y.min(55.5), rx, p.size * f, 0.7);
            if d > 1 && hash01((p.life * 60.0) as u32) > 0.8 {
                c.dot(p.x + 1.0, p.y.min(55.5) - 2.0, pal::WHITE);
            }
        }
        if bubble_age > 0.0 && bubble_age < 1.3 && b < 0.02 {
            let g = if bubble_age < 0.15 { 1.5 } else { 0.0 }; // pops out a bit too big, then settles
            c.tri((57.5, 32.8), (63.0, 32.8), (62.0, 38.0), pal::DKGREY);
            bubble(c, 50.0 - g, 23.5 - g, 19.0 + 2.0 * g, 9.0 + 2.0 * g, pal::WHITE, pal::DKGREY);
            c.tri((59.0, 32.2), (62.0, 32.2), (61.5, 36.5), pal::WHITE);
            c.text(52.0, 25.5, "OINK", pal::BLACK, 1.0);
        }
        if b > 0.02 && kb > 0.4 { c.text(51.5, 22.5, "OINK!", pal::RED, 1.0); }
        if off {
            c.ring(66.5, 46.0, 2.4, 2.7, 0.9, pal::LTBLUE);
            c.text(54.0, 33.0, "Z", pal::LTBLUE, 1.0);
            c.text(59.0, 28.0, "Z", pal::LTBLUE, 1.0);
            c.text(64.0, 22.0, "Z", pal::LTBLUE, 2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(state: FlameState, t: f32, detail: u8) -> Ctx { Ctx { state, t, motion: 1.0, detail, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() } }

    #[test]
    fn an_idle_pig_looks_the_same_whatever_the_clock_says() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2)); let pig = Pig::new();
        pig.draw(&mut a, &cx(FlameState::Idle, 0.0, 1)); pig.draw(&mut b, &cx(FlameState::Idle, 3.7, 1));
        assert_eq!(a.pixels(), b.pixels());
    }

    #[test]
    fn the_coin_shower_respects_the_cap_for_the_detail_level() {
        let mut rng = crate::XorShift64::new(1);
        let mut pig = Pig::new();
        pig.surge(1.0, &mut rng);
        let mut most = 0;
        for i in 0..40 {
            let c = Ctx { surge_age: i as f32 * 0.05, surge_len: 2.4, surge_mag: 1.0, ..cx(FlameState::Live, 0.0, 0) };
            pig.update(0.05, &c, &mut rng);
            most = most.max(pig.coins.len());
        }
        assert!(most > 5 && most <= 10, "{most}");
    }
}
