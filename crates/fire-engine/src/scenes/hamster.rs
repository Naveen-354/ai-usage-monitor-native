//! Hamster wheel: a hamster in an exercise wheel on an A-frame stand.
//! Live sprints (legs, bob, turning spokes, speed lines, sweat, now and then a "WHEE!" bubble);
//! Idle stands still; Off is curled up asleep with ZZZ.
//! A surge spins the wheel crazy fast and launches the hamster into a wild somersault loop
//! up out of the wheel, landing back in with a puff of dust.

use std::f32::consts::{PI, TAU};

use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const WX: f32 = 40.0; // wheel centre
const WY: f32 = 39.5;
const WR: f32 = 17.0;
const FLOOR: f32 = 54.8; // inside bottom of the wheel
const HX: f32 = 38.5;
const S: f32 = 1.0;

pub struct Hamster {
    ang: f32,          // wheel angle (clockwise)
    gait: f32,         // leg phase
    acc: f32,          // sweat timer
    fx: Particles<32>, // sweat, dust, stars
}

impl Hamster {
    pub fn new() -> Self {
        Self { ang: 0.5, gait: 0.0, acc: 0.0, fx: Particles::new() }
    }
}

impl Default for Hamster {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mood {
    Calm,
    Sprint,
    Shock,
    Sleep,
}

fn effort(cx: &Ctx) -> f32 {
    if cx.state == FlameState::Live { 1.0 } else { (cx.boost * 4.0).min(1.0) }
}

/// The surge hop as (lift, somersault angle, squash/stretch, airborne, horizontal offset).
fn hop(cx: &Ctx) -> (f32, f32, f32, bool, f32) {
    if cx.state == FlameState::Off || cx.surge_len <= 0.0 || cx.surge_age >= cx.surge_len {
        return (0.0, 0.0, 1.0, false, 0.0);
    }
    let dur = (0.75 + 0.45 * cx.surge_mag).min(cx.surge_len);
    let t = cx.surge_age;
    if t >= dur {
        return (0.0, 0.0, 1.0, false, 0.0);
    }
    const CROUCH: f32 = 0.08;
    let land_t = dur - 0.14;
    if t < CROUCH {
        return (0.0, 0.0, 1.0 - 0.24 * (t / CROUCH), false, 0.0);
    }
    if t > land_t {
        let frac = (t - land_t) / 0.14;
        return (0.0, 0.0, 0.74 + 0.26 * frac, false, 0.0);
    }
    let v = (t - CROUCH) / (land_t - CROUCH);
    let arc = 4.0 * v * (1.0 - v);
    let lift = arc * (10.0 + 13.0 * cx.surge_mag);
    let flips = if cx.surge_mag > 0.65 { 2.0 } else if cx.surge_mag > 0.25 { 1.0 } else { 0.0 };
    let spin = if flips > 0.0 { TAU * flips * v * v * (3.0 - 2.0 * v) } else { -0.5 * arc };
    let dx = (v * TAU).sin() * (2.5 * cx.surge_mag);
    (lift, spin, 1.08, true, dx)
}

struct Pen<'a> {
    c: &'a mut Canvas,
    x: f32,
    y: f32,
    sin: f32,
    cos: f32,
    sy: f32,
}

impl Pen<'_> {
    fn at(&self, lx: f32, ly: f32) -> (f32, f32) {
        let (a, b) = (lx * S, ly * S * self.sy);
        (self.x + a * self.cos - b * self.sin, self.y + a * self.sin + b * self.cos)
    }
    fn ball(&mut self, lx: f32, ly: f32, r: f32, col: Col) {
        let (x, y) = self.at(lx, ly);
        let r = r * S * (1.0 + self.sy) * 0.5;
        self.c.ellipse(x, y, r, r, col);
    }
    fn bar(&mut self, a: (f32, f32), b: (f32, f32), t: f32, col: Col) {
        let (p, q) = (self.at(a.0, a.1), self.at(b.0, b.1));
        self.c.line(p.0, p.1, q.0, q.1, t * S, col);
    }
}

fn hamster(c: &mut Canvas, (x, y): (f32, f32), rot: f32, sy: f32, gait: f32, amp: f32, mood: Mood) {
    let mut h = Pen { c, x, y: y - 7.2 * S * sy, sin: rot.sin(), cos: rot.cos(), sy };
    if mood == Mood::Sleep {
        h.y = y - 6.6 * S;
        h.ball(0.0, 0.0, 6.8, pal::TAN);
        h.ball(-2.0, -3.6, 3.2, pal::BROWN);
        h.ball(1.0, 4.4, 2.8, pal::CREAM);
        h.ball(5.4, 0.8, 4.2, pal::TAN);
        h.ball(2.6, -3.4, 1.8, pal::BROWN);
        h.ball(2.6, -3.4, 0.9, pal::PINK);
        h.ball(7.6, 2.6, 2.4, pal::CREAM);
        h.bar((5.6, 0.2), (7.8, 0.2), 0.9, pal::BLACK);
        h.ball(9.6, 1.4, 0.9, pal::HOTPINK);
        h.ball(-1.0, 6.4, 1.2, pal::PINK);
        return;
    }
    let leg = |h: &mut Pen, hip: f32, ph: f32, col: Col| {
        let foot = (hip + amp * 3.0 * ph.cos(), 7.2 - amp * 3.2 * (-ph.sin()).max(0.0));
        h.bar((hip, 2.5), foot, 2.0, col);
        h.ball(foot.0, foot.1, 1.1, pal::PINK);
    };
    leg(&mut h, 4.2, gait + PI, pal::BROWN);
    leg(&mut h, -3.0, gait, pal::BROWN);
    h.ball(-7.0, 1.0, 1.1, pal::PINK); // tail
    h.bar((-3.6, 0.0), (2.4, 0.0), 9.6, pal::TAN);
    h.bar((-5.0, -3.4), (0.0, -3.7), 2.6, pal::BROWN);
    h.bar((-3.5, 2.3), (2.0, 2.4), 4.0, pal::CREAM);
    leg(&mut h, 3.2, gait, pal::TAN);
    leg(&mut h, -4.0, gait + PI, pal::TAN);
    h.ball(6.8, -1.4, 4.6, pal::TAN);
    h.ball(4.4, -5.6, 1.9, pal::BROWN);
    h.ball(4.4, -5.6, 0.9, pal::PINK);
    h.ball(9.0, 1.0, 2.7, pal::CREAM);
    h.ball(11.2, -0.8, 0.9, pal::HOTPINK);
    let (eye, pupil, look) = match mood {
        Mood::Shock => (2.5, 0.7, 0.0),
        Mood::Sprint => (2.1, 1.0, 0.7),
        _ => (1.9, 0.95, 0.4),
    };
    h.ball(7.6, -3.0, eye, pal::WHITE);
    h.ball(7.6 + look, -2.8, pupil, pal::BLACK);
    match mood {
        Mood::Shock => h.ball(10.3, 2.8, 1.5, pal::DKRED),
        Mood::Sprint => h.ball(10.4, 3.2, 1.0, pal::HOTPINK),
        _ => h.bar((9.4, 3.0), (11.0, 2.6), 0.8, pal::DKBROWN),
    }
    let up = if mood == Mood::Shock { -1.0 } else { 0.0 };
    h.bar((6.2, -5.7 + up), (9.0, -5.0 + up * 1.4), 1.0, pal::DKBROWN);
}

fn bubble(c: &mut Canvas, x: f32, y: f32, s: &str) {
    let w = Canvas::text_width(s, 1.0) + 4.0;
    c.rect(x, y, w, 9.0, pal::WHITE);
    c.tri((x + 3.0, y + 8.0), (x + 8.0, y + 8.0), (x + 3.5, y + 13.0), pal::WHITE);
    c.text(x + 2.0, y + 2.0, s, pal::BLACK, 1.0);
}

impl Scene for Hamster {
    fn reset(&mut self, _rng: &mut dyn Rng) {
        *self = Self::new();
    }

    fn surge(&mut self, mag: f32, rng: &mut dyn Rng) {
        let n = (4.0 + 8.0 * mag) as usize;
        for _ in 0..n {
            let vx = rand(rng, 6.0, 18.0) * if rng.gen_f64() < 0.5 { -1.0 } else { 1.0 };
            let (x, size) = (HX + rand(rng, -5.0, 7.0), rand(rng, 1.3, 2.2));
            self.fx.spawn(Particle {
                x,
                y: FLOOR - 1.0,
                vx,
                vy: rand(rng, -8.0, -2.5),
                life: 0.5,
                max: 0.5,
                col: pal::LTGREY,
                size,
            });
        }
    }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        let (r, b, m) = (effort(cx), cx.boost, cx.motion);
        self.ang = (self.ang + ((2.8 * r + 15.0 * b) * m * dt).min(0.9)) % TAU;
        self.gait = (self.gait + (22.0 * r + 20.0 * b) * m * dt) % TAU;
        self.fx.step(dt, 40.0, 0.5);

        let (_lift, _spin, _stretch, air, _dx) = hop(cx);
        if cx.detail > 0 && r > 0.5 && !air {
            self.acc += dt * (5.0 + 2.0 * cx.detail as f32) * m;
            while self.acc >= 1.0 {
                self.acc -= 1.0;
                let (vx, vy) = (rand(rng, -15.0, -5.0), rand(rng, -16.0, -8.0));
                let x = HX + rand(rng, 3.0, 7.0);
                self.fx.spawn(Particle { x, y: 41.0, vx, vy, life: 0.55, max: 0.55, col: pal::LTBLUE, size: 1.0 });
            }
        }

        let dur = (0.75 + 0.45 * cx.surge_mag).min(cx.surge_len);
        let land_t = dur - 0.14;
        let now = cx.surge_age;
        if now - dt < land_t && now >= land_t {
            let n = (6.0 + 10.0 * cx.surge_mag) as usize;
            for _ in 0..n {
                let vx = rand(rng, 7.0, 20.0) * if rng.gen_f64() < 0.5 { -1.0 } else { 1.0 };
                let (x, size) = (HX + rand(rng, -6.0, 8.0), rand(rng, 1.4, 2.4));
                self.fx.spawn(Particle {
                    x,
                    y: FLOOR - 1.0,
                    vx,
                    vy: rand(rng, -9.0, -3.0),
                    life: 0.55,
                    max: 0.55,
                    col: pal::GREY,
                    size,
                });
            }
        }

        let apex = 0.08 + (land_t - 0.08) * 0.5;
        if cx.detail > 0 && cx.surge_mag > 0.4 && now - dt < apex && now >= apex {
            let top = FLOOR - 11.0 - 14.0 * cx.surge_mag;
            for k in 0..6 {
                let a = k as f32 * TAU / 6.0 + rand(rng, 0.0, 1.0);
                let sp = rand(rng, 10.0, 22.0);
                self.fx.spawn(Particle { x: HX, y: top, vx: a.cos() * sp, vy: a.sin() * sp, life: 0.45, max: 0.45, col: pal::YELLOW, size: 1.6 });
            }
        }
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let (r, b, t) = (effort(cx), cx.boost, cx.t);
        let (lift, spin, stretch, air, dx) = hop(cx);
        let ox = (t * 61.0).sin() * 0.6 * b;

        // Soft ground shadow and A-frame stand
        c.ellipse(WX, 57.6, 21.0, 1.6, pal::SHADOW);
        for s in [-1.0, 1.0] {
            c.line(WX + ox, WY, WX + s * 12.0 + ox, 57.0, 1.8, pal::BROWN);
            c.rect(WX + s * 12.0 - 3.0 + ox, 56.6, 6.0, 1.4, pal::DKBROWN);
        }

        // Speed lines trailing behind the wheel
        if r > 0.5 {
            for k in 0..(3 + (b * 3.0) as usize) {
                let p = (t * (1.6 + b * 2.0) + hash01(k as u32) * 3.0).fract();
                let (y, len) = ([44.0, 48.5, 52.5, 41.0, 56.0, 46.0][k], 3.0 + 4.0 * hash01(k as u32 + 9) + 4.0 * b);
                let xh = 21.0 - p * (10.0 + 6.0 * b);
                c.line(xh, y, xh + len * (1.0 - p), y, 1.0, pal::GREY);
            }
        }

        // The wheel: spokes with motion blur, rim, axle
        let (wx, blur) = (WX + ox, b > 0.3);
        let n = if blur { 3 } else { 6 };
        for k in 0..n {
            let a = self.ang + k as f32 * TAU / n as f32;
            for (g, col, th) in [(0.4, pal::SHADOW, 2.2), (0.2, pal::SHADOW, 2.2), (0.0, pal::GREY, 1.0)] {
                if g == 0.0 || blur {
                    let a = a - g;
                    c.line(wx, WY, wx + a.cos() * (WR - 1.0), WY + a.sin() * (WR - 1.0), th, col);
                }
            }
        }
        if cx.detail >= 2 && b > 0.5 {
            c.ring(wx, WY, WR + 1.2, WR + 1.2, 1.2, pal::GLOW);
        }
        c.ring(wx, WY, WR, WR, 1.6, pal::PURPLE);
        if cx.detail >= 1 {
            c.line(wx - 7.0, WY - WR + 0.6, wx + 7.0, WY - WR + 0.6, 0.9, pal::LTPURPLE);
        }
        c.ellipse(wx, WY, 1.7, 1.7, pal::DKGREY);
        c.dot(wx - 0.5, WY - 0.5, pal::YELLOW);
        for k in 0..if blur { 4 } else { 1 } {
            let (a, rr, s) = (self.ang - k as f32 * 0.3, WR - 0.8, 1.5 - k as f32 * 0.3);
            c.ellipse(wx + a.cos() * rr, WY + a.sin() * rr, s, s, pal::YELLOW);
        }

        // Live gag speech bubble
        let ph = t % 4.0;
        let gag = cx.state == FlameState::Live && ((0.75..1.05).contains(&ph) || (2.0..2.3).contains(&ph));
        if gag && !air {
            bubble(c, 44.0, 22.5, "WHEE!");
        }

        // Hamster
        if off {
            hamster(c, (HX + 1.5, FLOOR), 0.0, 1.0, 0.0, 0.0, Mood::Sleep);
            for (i, (x, y)) in [(42.0, 41.0), (45.5, 35.5), (48.5, 29.5)].into_iter().enumerate() {
                c.text(x, y, "Z", pal::PURPLE, 1.0 + i as f32 * 0.25);
            }
        } else {
            let mood = if air { Mood::Shock } else if r > 0.5 { Mood::Sprint } else { Mood::Calm };
            let bob = self.gait.sin().abs() * 0.9 * r * cx.motion;
            hamster(c, (HX + ox + dx, FLOOR - bob - lift), -0.1 * r + spin, stretch, self.gait, if air { 1.0 } else { r }, mood);
        }

        // Particles
        for p in self.fx.iter() {
            if p.col == pal::LTBLUE {
                c.rect(p.x, p.y, 1.0, 1.5, p.col);
            } else {
                let s = p.size * (0.4 + 0.6 * p.life / p.max);
                c.rect(p.x - s * 0.5, p.y - s * 0.5, s, s, p.col);
            }
        }

        // Idle calm dots
        if cx.state == FlameState::Idle && !air && b == 0.0 {
            for k in 0..3 {
                c.dot(46.0 + k as f32 * 2.5, 38.0, pal::GREY);
            }
        }
    }
}
