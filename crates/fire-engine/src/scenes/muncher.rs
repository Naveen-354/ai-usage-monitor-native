//! The Token Muncher: a purple blob with googly eyes and a huge round mouth that gobbles gold tokens rolling in from the left.
//! Live: tokens stream in, the mouth snaps shut as each one vanishes, crumbs fly, "NOM NOM". Idle: calm, mouth shut. Off: asleep.
//! Surge: a giant token rolls in, the mouth stretches wide, GULP, the belly bulges, BURP.

use std::f32::consts::PI;

use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const MX: f32 = 44.0; // closed mouth centre on the left face of the blob
const MY: f32 = 45.0;
const GY: f32 = 57.0; // ground
const P: f32 = 0.8; // seconds between tokens; each one is in flight for 3 of these

pub struct Muncher {
    crumbs: Particles<28>,
    eaten: i32,
    burst: bool,
}

impl Muncher {
    pub fn new() -> Self {
        Self { crumbs: Particles::new(), eaten: -1, burst: true }
    }

    fn spark(&mut self, rng: &mut dyn Rng, x: f32, y: f32, vx: f32, vy: f32, spread: f32, life: f32) {
        let col = [pal::GOLD, pal::YELLOW, pal::DKYELLOW][rand(rng, 0.0, 2.99) as usize];
        let (a, s) = (rand(rng, -0.9, 0.9), rand(rng, 0.4, 1.0));
        self.crumbs.spawn(Particle { x, y, vx: vx * s + spread * a, vy: vy * s, life: life * rand(rng, 0.6, 1.0), max: life, col, size: 1.0 });
    }
}

impl Default for Muncher { fn default() -> Self { Self::new() } }

fn sm(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Live clock in token periods; starts with tokens already on their way.
fn clock(cx: &Ctx) -> f32 {
    cx.t * (0.5 + 0.5 * cx.motion) / P + 2.2
}

/// Progress through the surge (0..1), or `None` when there is none.
fn surge_u(cx: &Ctx) -> Option<f32> {
    (cx.state != FlameState::Off && cx.boost > 0.0 && cx.surge_age < cx.surge_len).then(|| cx.surge_age / cx.surge_len)
}

/// Mouth opening 0..1 over one token period: a little chew, open up, snap shut as the token arrives at w = 1.
fn jaw(w: f32) -> f32 {
    if w < 0.18 {
        0.3 * (w / 0.18 * PI).sin()
    } else {
        sm(0.18, 0.5, w) * (1.0 - sm(0.86, 1.0, w))
    }
}

/// Where a token is at progress q: it rolls in, hops over the lip and is sucked into the mouth.
fn coin_pos(q: f32) -> (f32, f32) {
    let h = sm(0.70, 0.84, q);
    (-4.0 + 42.0 * q * q, 53.5 - 11.0 * h - 20.0 * h * (1.0 - h))
}

struct Pose { open: f32, eye: f32, big: f32, bulge: f32, jolt: f32, look: (f32, f32) }

fn pose(cx: &Ctx, u: Option<f32>) -> Pose {
    let mut p = Pose { open: 0.08, eye: 1.0, big: 1.0, bulge: 0.0, jolt: 0.0, look: (-0.7, 0.35) };
    match cx.state {
        FlameState::Off => {
            p.open = 0.30;
            p.eye = 0.0;
        }
        FlameState::Live => {
            let c0 = clock(cx);
            let w = c0.fract();
            let (x, y) = coin_pos((c0 - ((c0 - 3.0).floor() + 1.0)) / 3.0);
            let (dx, dy) = (x - MX, y - MY);
            let n = (dx * dx + dy * dy).sqrt().max(1.0);
            p.look = (dx / n + 0.25 * (cx.t * 9.0).sin(), dy / n);
            p.open = jaw(w);
            p.jolt = 1.4 * (1.0 - sm(0.0, 0.2, w));
            p.eye = 1.0 - 0.3 * (1.0 - sm(0.0, 0.25, w));
        }
        FlameState::Idle => {}
    }
    if let Some(u) = u {
        let (m, wide) = (cx.surge_mag, 1.20 + 0.50 * cx.surge_mag);
        let burp = sm(0.62, 0.68, u) * (1.0 - sm(0.86, 0.96, u));
        let strain = 0.04 * (u * 70.0).sin() * (sm(0.26, 0.34, u) - sm(0.42, 0.48, u));
        p.open = if u < 0.20 { wide * sm(0.0, 0.20, u) } else { wide * (1.0 - sm(0.42, 0.54, u)) } + 0.30 * burp + strain;
        p.big = 1.0 + 0.15 * sm(0.42, 0.52, u) * (1.0 - sm(0.54, 0.62, u));
        p.eye = 1.0;
        p.look = (-0.8, 0.55);
        let env = sm(0.46, 0.56, u) * (1.0 - sm(0.80, 0.98, u));
        p.bulge = (0.35 + 0.65 * m) * env + cx.boost * 0.15 * env;
        p.jolt = 2.0 * (sm(0.54, 0.60, u) - sm(0.60, 0.78, u));
    }
    p
}

fn coin(c: &mut Canvas, x: f32, y: f32, r: f32, d: u8) {
    if r < 1.0 {
        return;
    }
    if d >= 2 && r > 5.0 {
        c.ellipse(x, y, r + 3.0, r + 3.0, pal::GLOW);
    }
    c.ellipse(x, y, r, r, pal::DKYELLOW);
    c.ellipse(x, y, r - 1.0, r - 1.0, pal::GOLD);
    let (s, k) = (x / r).sin_cos();
    if r > 5.0 {
        let rot = |px: f32, py: f32| (x + r * (px * k - py * s), y + r * (px * s + py * k));
        let (a, b, e, f) = (rot(-0.45, -0.4), rot(0.45, -0.4), rot(0.0, -0.4), rot(0.0, 0.5));
        c.line(a.0, a.1, b.0, b.1, r * 0.22, pal::DKYELLOW);
        c.line(e.0, e.1, f.0, f.1, r * 0.22, pal::DKYELLOW);
    } else {
        c.dot(x + 0.5 * r * k - 0.5, y + 0.5 * r * s - 0.5, pal::YELLOW);
    }
}

/// A white speech bubble with black text; `tx` is where its tail points.
fn say(c: &mut Canvas, x: f32, y: f32, s: &str, sc: f32, tx: f32) {
    let (w, h) = (Canvas::text_width(s, sc) + 3.0, 5.0 * sc + 3.0);
    c.tri((tx, y + h + 3.0), (tx - 3.0, y + h - 0.5), (tx + 2.0, y + h - 0.5), pal::WHITE);
    c.rect(x + 1.0, y, w - 2.0, h, pal::WHITE);
    c.rect(x, y + 1.0, w, h - 2.0, pal::WHITE);
    c.text(x + 1.5, y + 1.5, s, pal::BLACK, sc);
}

impl Scene for Muncher {
    fn reset(&mut self, _rng: &mut dyn Rng) {
        self.crumbs.clear();
        self.eaten = -1;
        self.burst = true;
    }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.crumbs.step(dt, 70.0, 0.85);
        if cx.state == FlameState::Live {
            let idx = (clock(cx) - 3.0).floor() as i32;
            if idx > self.eaten && surge_u(cx).is_none() {
                let n = 2 + 2 * cx.detail as usize;
                for _ in 0..n {
                    self.spark(rng, 30.0, 42.0, -6.0, -26.0, 24.0, 0.7);
                }
            }
            self.eaten = idx;
        }
        if !self.burst && cx.surge_age >= 0.60 * cx.surge_len {
            self.burst = true;
            let n = 12 + (12.0 * cx.surge_mag) as usize;
            for _ in 0..n {
                self.spark(rng, 30.0, 40.0, -30.0 - 20.0 * cx.surge_mag, -50.0, 40.0, 1.0);
            }
        }
    }

    fn surge(&mut self, _mag: f32, _rng: &mut dyn Rng) {
        self.burst = false;
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let d = cx.detail;
        let u = surge_u(cx);
        let p = pose(cx, u);
        let b = p.bulge;
        c.ellipse(50.0, 57.5, 25.0 + 4.0 * b, 2.0, pal::SHADOW);
        c.ellipse(47.0, 56.0, 5.0, 2.5, pal::DKPURPLE);
        c.ellipse(60.0, 56.0, 5.0, 2.5, pal::DKPURPLE);
        let (rx, ry) = (13.5 + 5.0 * b, 13.5 + 2.0 * b - p.jolt);
        let (bx, by) = (54.0 + 1.0 * b, GY - ry);
        c.tri((bx + 9.0, by - 8.0), (bx + 8.0, by - 13.0), (bx + 14.0, by - 13.0), pal::DKPURPLE);
        c.tri((bx + 4.0, by - 12.0), (bx + 1.0, by - 16.0), (bx + 7.0, by - 16.0), pal::DKPURPLE);
        if d >= 1 {
            c.ellipse(bx + 1.0, by + 0.8, rx, ry, pal::DKPURPLE);
            c.ellipse(bx - 0.8, by - 0.5, rx - 1.2, ry - 1.0, pal::PURPLE);
        } else {
            c.ellipse(bx, by, rx, ry, pal::PURPLE);
        }
        c.ellipse(bx + 2.5, by + 3.5, 8.0 + 4.0 * b, 7.0 + 3.0 * b, pal::LTPURPLE);
        if b > 0.5 {
            coin(c, bx + 2.5, by + 3.5, 2.5 + 2.5 * cx.surge_mag, d);
        }
        let open = p.open.clamp(0.0, 1.8);
        let (mrx, mry) = (3.5 + 8.5 * open, 1.2 + 6.0 * open);
        let mx = MX - 4.0 * open.min(1.0);
        c.ellipse(mx, MY, mrx, mry, pal::DKRED);
        if open > 0.45 {
            c.ellipse(mx - 1.0, MY + mry * 0.35, mrx * 0.55, mry * 0.38, pal::HOTPINK);
        }
        if let Some(u) = u {
            if u < 0.58 {
                let r = 6.5 + 4.5 * cx.surge_mag;
                let (x, y) = if u < 0.20 {
                    (-2.0 + 18.0 * u / 0.20, GY - r)
                } else {
                    let k = sm(0.20, 0.42, u);
                    (16.0 + 22.0 * k, GY - r - (GY - r - MY) * k)
                };
                coin(c, x, y, r * (1.0 - sm(0.42, 0.58, u)), d);
            }
        } else if cx.state == FlameState::Live {
            let c0 = clock(cx);
            let first = (c0 - 3.0).floor() + 1.0;
            for k in 0..3 {
                let q = (c0 - first - k as f32) / 3.0;
                let (x, y) = coin_pos(q);
                coin(c, x, y, 3.5 - sm(0.93, 1.0, q), d);
            }
        }
        if mry > 2.8 {
            c.rect(mx - mrx * 0.50, MY - mry + 0.5, 3.0, 2.0, pal::WHITE);
            c.rect(mx + mrx * 0.20, MY - mry + 0.5, 3.0, 2.0, pal::WHITE);
            c.rect(mx - mrx * 0.20, MY + mry - 2.5, 3.0, 2.0, pal::WHITE);
            c.rect(mx + mrx * 0.40, MY + mry - 2.5, 3.0, 2.0, pal::WHITE);
        }
        for k in 0..2 {
            let (x, y, r) = if k == 0 { (50.0, 32.0, 5.5) } else { (61.0, 31.0, 5.0) };
            let y = y - p.jolt * 0.5;
            if p.eye < 0.1 {
                c.ellipse(x, y + 1.0, r, r - 1.0, pal::PURPLE);
                c.line(x - 3.5, y, x - 2.0, y + 1.4, 1.0, pal::BLACK);
                c.line(x - 2.0, y + 1.4, x + 2.0, y + 1.4, 1.0, pal::BLACK);
                c.line(x + 2.0, y + 1.4, x + 3.5, y, 1.0, pal::BLACK);
                continue;
            }
            let r = r * p.big;
            c.ellipse(x, y, r, r * p.eye, pal::WHITE);
            let (px, py) = (x + p.look.0 * r * 0.45, y + p.look.1 * r * 0.4);
            c.ellipse(px, py, r * 0.46, r * 0.46 * p.eye, pal::BLACK);
            if d >= 1 {
                c.dot(px - 1.2, py - 1.4 * p.eye, pal::WHITE);
            }
        }
        match (cx.state, u) {
            (FlameState::Off, _) => say(c, 26.0, 22.0, "ZZZ", 1.0, 40.0),
            (_, Some(u)) if (0.62..0.93).contains(&u) => say(c, 2.0, 22.0, "BURP", 1.0 + 0.5 * cx.surge_mag, 20.0),
            (FlameState::Live, None) if ((clock(cx) - 3.0).floor() as i32).rem_euclid(6) < 2 => say(c, 1.0, 22.0, "NOM NOM", 1.0, 24.0),
            _ => {}
        }
        self.crumbs.draw(c);
    }
}
