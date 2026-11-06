//! Slot machine: reels spin and marquee lights chase while Live, lever is pulled periodically.
//! Surge locks 7-7-7, flashes marquee, machine hops and sprays coins. Idle: losing mix. Off: out of order.

use crate::canvas::{pal::*, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const PITCH: f32 = 10.0;
const SPEED: [f32; 3] = [3.1, 3.6, 4.2];
const REST: [i32; 3] = [1, 2, 0]; // cherry, lemon, 7 (0=7, 1=cherry, 2=lemon, 3=bar)
const GRAV: f32 = 130.0;
const CONFETTI: [Col; 5] = [MAGENTA, CYAN, BLUE, GREEN, PURPLE];

pub struct Slots {
    fx: Particles<28>,
    budget: f32,
}

impl Slots {
    pub fn new() -> Self {
        Self { fx: Particles::new(), budget: 0.0 }
    }

    fn shoot(&mut self, rng: &mut dyn Rng, left: f32, confetti: bool) {
        let side = if rng.gen_f64() < 0.5 { -1.0 } else { 1.0 };
        let (x, y) = if confetti { (rand(rng, 22.0, 52.0), 24.0) } else { (rand(rng, 30.0, 44.0), 51.0) };
        let (vx, vy) = (side * rand(rng, 10.0, 28.0), -rand(rng, 55.0, 85.0) * if confetti { 0.5 } else { 1.0 });
        let fall = (-vy + (vy * vy + 2.0 * GRAV * (57.0 - y)).sqrt()) / GRAV;
        let col = if confetti { CONFETTI[(rng.gen_f64() * 5.0) as usize % 5] } else { YELLOW };
        let size = if confetti { 1.5 } else { 3.0 };
        self.fx.spawn(Particle { x, y, vx, vy, life: fall.min(left), max: 1.0, col, size });
    }
}

impl Default for Slots {
    fn default() -> Self { Self::new() }
}

fn reel_q(i: usize, cx: &Ctx) -> f32 {
    let win = cx.boost > 0.0 && cx.state != FlameState::Off;
    let spin = cx.state == FlameState::Live;
    if !win && !spin { return 0.0; }
    let (w, clock) = (SPEED[i] * (0.5 + 0.5 * cx.motion), if spin { cx.t } else { cx.surge_age });
    let at = |t: f32| w * (t % (4.0 / w));
    let lock = 0.08 + 0.1 * i as f32;
    if !win || cx.surge_age < lock { return at(clock); }
    let from = at(clock - (cx.surge_age - lock));
    let mut n = (from + 0.5).ceil() as i32;
    while (n + REST[i]).rem_euclid(4) != 0 { n += 1; }
    let u = ((cx.surge_age - lock) / 0.3).min(1.0);
    from + (n as f32 - from) * (1.0 - (1.0 - u).powi(3))
}

fn lever(u: f32) -> f32 {
    if u < 0.18 { u / 0.18 }
    else if u < 0.32 { 1.0 }
    else if u < 0.8 { let k = (u - 0.32) / 0.48; 1.0 - k * k * (3.0 - 2.0 * k) }
    else { 0.0 }
}

struct V<'a> { c: &'a mut Canvas, x: f32, y: f32 }
impl V<'_> {
    fn r(&mut self, x: f32, y: f32, w: f32, h: f32, k: Col) { self.c.rect(x + self.x, y + self.y, w, h, k); }
    fn e(&mut self, x: f32, y: f32, rx: f32, ry: f32, k: Col) { self.c.ellipse(x + self.x, y + self.y, rx, ry, k); }
    fn l(&mut self, a: (f32, f32), b: (f32, f32), th: f32, k: Col) { self.c.line(a.0 + self.x, a.1 + self.y, b.0 + self.x, b.1 + self.y, th, k); }
    fn t(&mut self, x: f32, y: f32, s: &str, k: Col) { self.c.text(x + self.x, y + self.y, s, k, 1.0); }
}

fn symbol(v: &mut V, kind: i32, x: f32, y: f32) {
    match kind {
        0 => { v.r(x - 3.5, y - 3.5, 7.0, 2.0, RED); v.l((x + 2.5, y - 2.5), (x - 1.0, y + 3.5), 2.4, RED); }
        1 => {
            v.l((x - 2.0, y), (x + 1.0, y - 3.5), 1.0, DKGREEN); v.l((x + 2.0, y), (x + 1.0, y - 3.5), 1.0, DKGREEN);
            v.r(x + 1.0, y - 4.0, 2.5, 1.2, GREEN);
            v.e(x - 2.2, y + 1.8, 2.2, 2.2, RED); v.e(x + 2.2, y + 2.0, 2.2, 2.2, RED);
        }
        2 => { v.e(x, y, 4.2, 3.2, DKYELLOW); v.e(x, y, 3.4, 2.4, YELLOW); }
        _ => {
            v.r(x - 4.0, y - 3.0, 8.0, 7.0, DKBLUE);
            for i in 0..3 { v.r(x - 3.0, y - 2.0 + 2.0 * i as f32, 6.0, 1.0, LTBLUE); }
        }
    }
}

impl Scene for Slots {
    fn reset(&mut self, _: &mut dyn Rng) { self.fx.clear(); self.budget = 0.0; }
    fn surge(&mut self, mag: f32, _: &mut dyn Rng) { self.budget = 3.0 + 6.0 * mag; }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.fx.step(dt, GRAV, 1.0);
        let left = cx.surge_len - cx.surge_age;
        if cx.boost <= 0.0 || left < 0.15 { return; }
        let cap = [10.0, 18.0, 28.0][cx.detail.min(2) as usize] * (0.25 + 0.75 * cx.boost.min(1.0));
        self.budget = (self.budget + 36.0 * dt).min(9.0);
        while self.budget >= 1.0 {
            self.budget -= 1.0;
            if (self.fx.len() as f32) < cap {
                let confetti = cx.detail > 0 && rng.gen_f64() < 0.3;
                self.shoot(rng, left, confetti);
            }
        }
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let win = !off && cx.boost > 0.0;
        let age = cx.surge_age;
        let flash = win && (age * 10.0) as i32 % 2 == 0;
        let u = if win { age } else if cx.state == FlameState::Live { cx.t % (2.6 / cx.motion.max(0.25)) } else { 9.0 };
        let pull = lever(u);
        let jolt = (1.0 - (u - 0.2).abs() / 0.1).max(0.0);
        let hop = if win { (age * 11.0).sin().abs() * 2.0 * cx.boost.min(1.0) } else { 0.0 };
        let shake = if win { (age * 47.0).sin() * (1.0 - age / 0.6).max(0.0) } else if cx.state == FlameState::Live { 0.6 * (cx.t * 37.0).sin() } else { 0.0 };
        let s = c.scale() as f32;
        let snap = |x: f32| (x * s).round() / s;
        c.ellipse(37.0, 57.5, 26.0 - hop, 2.0, SHADOW);
        let mut v = V { c, x: snap(shake), y: snap(jolt - hop) };

        v.r(22.0, 35.0, 30.0, 12.0, if off { BLACK } else { DKGREY });
        for i in (0..3).filter(|_| !off) {
            let x = 22.0 + 10.5 * i as f32;
            v.r(x, 35.0, 9.0, 12.0, WHITE);
            let q = reel_q(i, cx);
            let f = (q - q.floor()) * PITCH;
            for k in 0..2 {
                symbol(&mut v, (q.floor() as i32 + k + REST[i]).rem_euclid(4), x + 4.5, 41.0 + f - k as f32 * PITCH);
            }
            if cx.detail > 0 {
                v.r(x, 35.0, 9.0, 2.0, SHADOW);
                v.r(x, 45.0, 9.0, 2.0, SHADOW);
            }
        }
        let body = if off { DKRED } else { RED };
        for (x, y, w, h, k) in [
            (17.0, 27.0, 40.0, 6.0, body), (17.0, 33.0, 3.0, 16.0, body),
            (54.0, 33.0, 3.0, 16.0, body), (17.0, 49.0, 40.0, 5.0, body),
            (20.0, 33.0, 34.0, 2.0, DKGREY), (20.0, 47.0, 34.0, 2.0, DKGREY),
            (20.0, 35.0, 2.0, 12.0, DKGREY), (52.0, 35.0, 2.0, 12.0, DKGREY),
            (15.0, 54.0, 44.0, 3.0, DKRED),
            (23.0, 50.0, 28.0, 1.0, GREY), (23.0, 51.0, 28.0, 3.0, BLACK),
        ] {
            v.r(x, y, w, h, k);
        }
        if win && age > 0.25 {
            for x in [29.0, 34.0, 39.0, 44.0] {
                v.e(x, 51.0, 1.8, 1.4, YELLOW);
                if cx.detail > 0 { v.r(x - 0.5, 50.5, 1.0, 1.0, WHITE); }
            }
        }

        if cx.detail >= 2 && flash { v.r(19.0, 21.0, 36.0, 11.0, GLOW); }
        let (fill, ink) = if off { (DKYELLOW, DKBROWN) } else if flash { (DKRED, YELLOW) } else { (GOLD, DKRED) };
        v.r(20.0, 22.0, 34.0, 9.0, BROWN);
        v.r(21.0, 23.0, 32.0, 7.0, fill);
        v.t(23.5, 24.0, "JACKPOT", ink);
        let step = (cx.t * 8.0 * cx.motion) as i32;
        for i in 0..21 {
            let (x, y) = match i {
                0..=5 => (17.5, 49.0 - 4.0 * i as f32),
                6..=14 => (20.5 + 4.0 * (i - 6) as f32, 22.0),
                _ => (55.5, 29.0 + 4.0 * (i - 15) as f32),
            };
            let col = if off {
                DKGREY
            } else if win {
                if (i + (age * 10.0) as i32) % 2 == 0 { WHITE } else { YELLOW }
            } else if cx.state == FlameState::Live {
                [WHITE, YELLOW, DKYELLOW][(i - step).rem_euclid(3) as usize]
            } else {
                DKYELLOW
            };
            v.r(x, y, 2.0, 2.0, col);
        }

        let (bx, by) = (62.0, 28.0 + 16.0 * pull);
        v.r(57.0, 35.0, 2.0, 8.0, DKGREY);
        v.l((58.0, 39.0), (bx, by), 1.6, GREY);
        v.e(bx, by, 2.6, 2.6, if off { DKRED } else { RED });
        v.r(bx - 1.5, by - 1.5, 1.0, 1.0, WHITE);

        if off {
            v.r(25.0, 35.0, 25.0, 13.0, CREAM);
            v.t(26.0, 36.0, "OUT OF", BLACK);
            v.t(28.0, 42.0, "ORDER", BLACK);
            v.r(24.0, 34.0, 3.0, 2.0, LTGREY);
            v.r(48.0, 34.0, 3.0, 2.0, LTGREY);
        }
        if win {
            for (x, y) in [(8.0, 26.0), (68.0, 23.0)] {
                v.c.text(x, y - (age * 14.0 + x).sin().abs() * 2.0, "!", RED, 1.5);
            }
        }
        for p in self.fx.iter() {
            if p.size < 2.0 {
                v.c.rect(p.x, p.y, p.size, p.size, p.col);
            } else {
                let sq = (p.life / 0.12).min(1.0);
                let w = (0.7 + 1.1 * (p.life * 13.0 + p.x).cos().abs()) * sq;
                v.c.ellipse(p.x, p.y, w, 1.8 * sq, DKYELLOW);
                v.c.ellipse(p.x, p.y, (w - 0.6).max(0.2), 1.2 * sq, YELLOW);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Animation, Quality};
    use crate::scene::SceneEngine;
    use crate::XorShift64;

    fn ctx(state: FlameState, boost: f32, age: f32) -> Ctx {
        Ctx { state, t: 5.0, motion: 1.0, detail: 2, boost, surge_age: age, surge_len: 2.0, surge_mag: 1.0 }
    }

    fn shown(i: usize, cx: &Ctx) -> i32 {
        let q = reel_q(i, cx);
        assert!((q - q.round()).abs() < 1e-3, "reel {i} should be at rest, not at {q}");
        (q.round() as i32 + REST[i]).rem_euclid(4)
    }

    #[test]
    fn idle_shows_a_losing_mix_and_a_win_locks_three_sevens() {
        let idle = ctx(FlameState::Idle, 0.0, 1.0e6);
        assert_eq!([shown(0, &idle), shown(1, &idle), shown(2, &idle)], [1, 2, 0]);
        for state in [FlameState::Idle, FlameState::Live] {
            let won = ctx(state, 0.5, 1.0);
            assert_eq!([shown(0, &won), shown(1, &won), shown(2, &won)], [0, 0, 0]);
        }
    }

    #[test]
    fn no_coin_is_left_hanging_in_the_air_after_a_surge_in_idle() {
        let mut rng = XorShift64::new(1);
        let mut e = SceneEngine::new(Box::new(Slots::new()), Quality::Medium);
        e.set_state(FlameState::Idle, 1.0, &mut rng);
        let (mut b1, mut b2) = (vec![0u8; 160 * 120 * 4], vec![0u8; 160 * 120 * 4]);
        e.paint(&mut b1);
        e.blast(2.0e7, &mut rng);
        while e.needs_loop() { e.tick(83.0, &mut rng); }
        e.paint(&mut b2);
        assert_eq!(b1, b2, "still pose must return with no coins frozen");
    }
}
