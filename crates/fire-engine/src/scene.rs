//! What a scene is, and the engine that runs one.
//!
//! A scene is a small struct that draws one frame from the clock and the app's state: no per-frame allocation, no
//! assets. Anything that moves with its own physics (coins, sparks) lives in a fixed-size [`Particles`] array.

use crate::animation::{Animation, Quality};
use crate::blast::{blast_params, Blast};
use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::Rng;

/// What a scene may look at while it draws. The scene itself is never told how big the canvas is: it draws in the
/// 80 x 60 logical space of [`Canvas`].
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    /// `Live` = an agent is working right now, `Idle` = nothing recent (a still pose), `Off` = paused / no data (a still, greyed pose).
    pub state: FlameState,
    /// Seconds since the scene (re)started. Only advances while the engine ticks, so a still pose is `t = 0`.
    pub t: f32,
    /// 1.0 normal, 0.5 for "low" animation intensity: scenes should keep their motion gentler.
    pub motion: f32,
    /// Quality tier: 0 = low, 1 = medium, 2 = high. Gate optional flourishes on this (shading, extra particles, glow).
    pub detail: u8,
    /// Strength of the current surge: 0 when there is none, up to 1 for a huge jump, fading back to 0.
    pub boost: f32,
    /// Seconds since the last surge began (a very large number if there has not been one).
    pub surge_age: f32,
    /// How long that surge lasts, in seconds.
    pub surge_len: f32,
    /// How big the last surge was, 0..=1 (log scaled).
    pub surge_mag: f32,
}

pub trait Scene {
    /// Back to the starting situation (called whenever the state or quality changes).
    fn reset(&mut self, _rng: &mut dyn Rng) {}
    /// Advances anything with its own physics. `dt` is seconds since the previous tick.
    fn update(&mut self, _dt: f32, _cx: &Ctx, _rng: &mut dyn Rng) {}
    /// A sudden jump in tokens: spawn the burst. `mag` is 0..=1.
    fn surge(&mut self, _mag: f32, _rng: &mut dyn Rng) {}
    /// Draws the whole frame onto a cleared, transparent canvas. Keep what matters in rows 22..60: the top of the
    /// canvas is cut off in the expanded overlay, so only throw-away decoration (steam, sparks) belongs above that.
    fn draw(&self, c: &mut Canvas, cx: &Ctx);
}

/// A deterministic pseudo-random number in 0..1 from an integer (stateless twinkles and wobble).
pub fn hash01(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// A uniform random number in `lo..hi`.
pub fn rand(rng: &mut dyn Rng, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * rng.gen_f64() as f32
}

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Seconds left.
    pub life: f32,
    /// Seconds it started with (`life / max` is the fraction left).
    pub max: f32,
    pub col: Col,
    /// Side length in logical units.
    pub size: f32,
}

impl Particle {
    pub const NONE: Particle = Particle { x: 0.0, y: 0.0, vx: 0.0, vy: 0.0, life: 0.0, max: 1.0, col: Col(0), size: 1.0 };
}

/// A fixed-capacity particle pool: never allocates, drops new particles when full.
pub struct Particles<const N: usize> {
    items: [Particle; N],
    n: usize,
}

impl<const N: usize> Default for Particles<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Particles<N> {
    pub const fn new() -> Self {
        Self { items: [Particle::NONE; N], n: 0 }
    }

    pub fn clear(&mut self) {
        self.n = 0;
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// `false` when the pool is full and the particle was dropped.
    pub fn spawn(&mut self, p: Particle) -> bool {
        if self.n == N {
            return false;
        }
        self.items[self.n] = p;
        self.n += 1;
        true
    }

    /// Moves everything by `dt` seconds: `gravity` in units/s^2 (positive = down), `drag` is the fraction of speed kept each second.
    pub fn step(&mut self, dt: f32, gravity: f32, drag: f32) {
        let keep = drag.clamp(0.0, 1.0).powf(dt);
        let mut i = 0;
        while i < self.n {
            let p = &mut self.items[i];
            p.life -= dt;
            if p.life <= 0.0 {
                self.n -= 1;
                self.items[i] = self.items[self.n];
                continue;
            }
            p.vy += gravity * dt;
            p.vx *= keep;
            p.vy *= keep;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            i += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Particle> {
        self.items[..self.n].iter()
    }

    /// Each particle as a `size` square of its own colour.
    pub fn draw(&self, c: &mut Canvas) {
        for p in self.iter() {
            c.rect(p.x - p.size * 0.5, p.y - p.size * 0.5, p.size, p.size, p.col);
        }
    }
}

const NO_SURGE_S: f32 = 1.0e6;

/// Runs a [`Scene`] behind the same interface as the flame.
pub struct SceneEngine {
    scene: Box<dyn Scene>,
    canvas: Canvas,
    quality: Quality,
    mode: FlameState,
    motion: f64,
    clock_ms: f64,
    surge_left_ms: f64,
    surge_total_ms: f64,
    surge_peak: f64,
    surge_mag: f32,
    surge_age_ms: f64,
}

impl SceneEngine {
    pub fn new(scene: Box<dyn Scene>, quality: Quality) -> Self {
        Self {
            scene,
            canvas: Canvas::new(quality.scale()),
            quality,
            mode: FlameState::Off,
            motion: 1.0,
            clock_ms: 0.0,
            surge_left_ms: 0.0,
            surge_total_ms: 1.0,
            surge_peak: 0.0,
            surge_mag: 0.0,
            surge_age_ms: NO_SURGE_S as f64 * 1000.0,
        }
    }

    fn boost(&self) -> f64 {
        if self.surge_left_ms > 0.0 {
            self.surge_peak * (self.surge_left_ms / self.surge_total_ms)
        } else {
            0.0
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            state: self.mode,
            t: (self.clock_ms / 1000.0) as f32,
            motion: self.motion as f32,
            detail: self.quality.detail(),
            boost: self.boost() as f32,
            surge_age: (self.surge_age_ms / 1000.0) as f32,
            surge_len: (self.surge_total_ms / 1000.0) as f32,
            surge_mag: self.surge_mag,
        }
    }
}

impl Animation for SceneEngine {
    fn mode(&self) -> FlameState {
        self.mode
    }

    fn dims(&self) -> (usize, usize) {
        self.canvas.size()
    }

    fn set_state(&mut self, mode: FlameState, motion: f64, rng: &mut dyn Rng) {
        self.mode = mode;
        self.motion = motion;
        self.clock_ms = 0.0;
        self.surge_left_ms = 0.0;
        self.surge_peak = 0.0;
        self.surge_mag = 0.0;
        self.surge_age_ms = NO_SURGE_S as f64 * 1000.0;
        self.scene.reset(rng);
    }

    fn blast(&mut self, delta: f64, rng: &mut dyn Rng) -> Option<Blast> {
        let p = blast_params(delta, self.motion)?;
        if self.mode == FlameState::Off {
            return None;
        }
        self.surge_peak = self.boost().max(p.boost);
        self.surge_total_ms = self.surge_left_ms.max(p.boost_ms).max(1.0);
        self.surge_left_ms = self.surge_total_ms;
        self.surge_mag = p.magnitude as f32;
        self.surge_age_ms = 0.0;
        self.scene.surge(p.magnitude as f32, rng);
        Some(p)
    }

    /// Live (or mid-surge) runs at the quality's frame rate, a little slower on "low" intensity. Idle and Off are still
    /// poses: no timer at all, so nothing wakes the app up while there is nothing to show.
    fn frame_ms(&self) -> f64 {
        let base = self.quality.frame_ms();
        if self.motion >= 1.0 {
            base
        } else {
            base * 1.5
        }
    }

    fn needs_loop(&self) -> bool {
        self.motion > 0.0 && self.mode != FlameState::Off && (self.mode == FlameState::Live || self.surge_left_ms > 0.0)
    }

    fn tick(&mut self, ms: f64, rng: &mut dyn Rng) {
        self.clock_ms += ms;
        self.surge_left_ms = (self.surge_left_ms - ms).max(0.0);
        self.surge_age_ms += ms;
        let cx = self.ctx();
        self.scene.update((ms / 1000.0) as f32, &cx, rng);
    }

    fn paint(&mut self, buf: &mut [u8]) {
        self.canvas.clear();
        let cx = self.ctx();
        self.scene.draw(&mut self.canvas, &cx);
        if cx.detail >= 1 {
            self.canvas.outline(pal::OUTLINE);
        }
        self.canvas.to_rgba(buf, self.mode == FlameState::Off);
    }

    fn bytes(&self) -> usize {
        self.canvas.bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash01_is_stable_and_spread_over_zero_to_one() {
        let v: Vec<f32> = (0..1000).map(hash01).collect();
        assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
        assert_eq!(hash01(7), hash01(7));
        let mean = v.iter().sum::<f32>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
    }

    #[test]
    fn particles_expire_and_a_full_pool_drops_new_ones() {
        let mut p = Particles::<4>::new();
        for _ in 0..6 {
            p.spawn(Particle { life: 1.0, max: 1.0, ..Particle::NONE });
        }
        assert_eq!(p.len(), 4);
        p.step(0.6, 0.0, 1.0);
        assert_eq!(p.len(), 4);
        p.step(0.6, 0.0, 1.0);
        assert!(p.is_empty());
    }

    #[test]
    fn gravity_pulls_particles_down_and_drag_slows_them() {
        let mut p = Particles::<2>::new();
        p.spawn(Particle { vx: 10.0, life: 5.0, max: 5.0, ..Particle::NONE });
        p.step(1.0, 10.0, 0.5);
        let q = p.iter().next().unwrap();
        assert!(q.vy > 0.0 && q.y > 0.0 && q.vx < 10.0);
    }
}
