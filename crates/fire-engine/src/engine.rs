use crate::blast::{blast_params, Blast};
use crate::palette::{band_color, spark_color, ASH_BANDS, FIRE_BANDS};
use crate::sim::{FireParams, FireSim, FIRE_H, FIRE_W};
use crate::sparks::{SparkKind, Sparks};
use crate::Rng;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlameState {
    Live,
    Idle,
    Off,
}

const REFERENCE_FRAME_MS: f64 = 42.0;
const FRAME_MS_NORMAL: f64 = 83.0;
const FRAME_MS_LOW: f64 = 125.0;

const SPARK_ORIGIN_X: f64 = (FIRE_W as f64 - 1.0) / 2.0;
const SPARK_ORIGIN_Y: f64 = FIRE_H as f64 - 17.0;
const RING_ORIGIN_X: f64 = (FIRE_W as f64 - 1.0) / 2.0;
const RING_ORIGIN_Y: f64 = FIRE_H as f64 - 22.0;

pub struct FireEngine {
    pub sim: FireSim,
    pub sparks: Sparks,
    pub mode: FlameState,
    pub motion: f64,
    pub clock: f64,
    pub boost_peak: f64,
    pub boost_total_ms: f64,
    pub boost_left_ms: f64,
    pub frames: usize,
}

impl Default for FireEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FireEngine {
    pub fn new() -> Self {
        Self {
            sim: FireSim::new(FIRE_W, FIRE_H),
            sparks: Sparks::new(),
            mode: FlameState::Off,
            motion: 1.0,
            clock: 0.0,
            boost_peak: 0.0,
            boost_total_ms: 1.0,
            boost_left_ms: 0.0,
            frames: 0,
        }
    }

    pub fn boost(&self) -> f64 {
        if self.boost_left_ms > 0.0 {
            self.boost_peak * (self.boost_left_ms / self.boost_total_ms)
        } else {
            0.0
        }
    }

    pub fn set_state(&mut self, mode: FlameState, motion: f64, rng: &mut dyn Rng) {
        self.mode = mode;
        self.motion = motion;
        self.sim.reset();
        let iters = if mode == FlameState::Live { 30 } else { 36 };
        for _ in 0..iters {
            let p = self.params(0.0, rng);
            self.sim.step(&p, rng);
        }
    }

    pub fn blast(&mut self, delta: f64, rng: &mut dyn Rng) -> Option<Blast> {
        let p = blast_params(delta, self.motion)?;
        if self.mode == FlameState::Off {
            return None;
        }

        let current = self.boost();
        self.boost_peak = current.max(p.boost);
        self.boost_total_ms = self.boost_left_ms.max(p.boost_ms).max(1.0);
        self.boost_left_ms = self.boost_total_ms;

        self.sim.flare(
            16.0 + 22.0 * p.magnitude,
            9.5 * (1.0 + 0.45 * p.magnitude),
            1.0 + 0.35 * p.magnitude,
            rng,
        );
        self.sparks
            .burst(p.sparks, p.magnitude, SPARK_ORIGIN_X, SPARK_ORIGIN_Y, rng);
        if p.ring {
            self.sparks
                .ring(p.ring_radius, p.magnitude, RING_ORIGIN_X, RING_ORIGIN_Y);
        }

        Some(p)
    }

    pub fn frame_ms(&self) -> f64 {
        if self.motion >= 1.0 {
            FRAME_MS_NORMAL
        } else {
            FRAME_MS_LOW
        }
    }

    pub fn needs_loop(&self) -> bool {
        if self.motion <= 0.0 || self.mode == FlameState::Off {
            return false;
        }
        self.mode == FlameState::Live || self.boost_left_ms > 0.0 || self.sparks.count() > 0
    }

    pub fn next_repaint(&self) -> Option<f64> {
        if self.needs_loop() {
            Some(self.frame_ms())
        } else {
            None
        }
    }

    pub fn tick(&mut self, ms: f64, rng: &mut dyn Rng) {
        self.clock += ms;
        self.boost_left_ms = (self.boost_left_ms - ms).max(0.0);

        let b = self.boost();
        let p = self.params(b, rng);
        self.sim.step(&p, rng);
        self.sparks.step(ms / REFERENCE_FRAME_MS);
    }

    fn params(&self, boost: f64, rng: &mut dyn Rng) -> FireParams {
        let (fuel, half_width, decay) = match self.mode {
            FlameState::Live => (1.15, 9.5, 0.012),
            FlameState::Idle => (0.88, 8.5, 0.0135),
            FlameState::Off => (0.72, 7.5, 0.015),
        };
        let flicker = 1.0 + 0.06 * (self.clock * 0.011).sin() + 0.04 * (rng.gen_f64() - 0.5);
        FireParams {
            fuel: fuel * (1.0 + 0.55 * boost) * flicker,
            half_width: half_width * (1.0 + 0.5 * boost),
            decay: decay * (1.0 - 0.2 * boost),
            edge_decay: 3.5,
            sway: 0.25,
            inward: 0.4,
            jitter: 0.35,
            blur: 0.4,
        }
    }

    pub fn paint(&mut self, buf: &mut [u8]) {
        buf.fill(0);
        let bands = if self.mode == FlameState::Off {
            ASH_BANDS
        } else {
            FIRE_BANDS
        };

        for i in 0..self.sim.heat.len() {
            if let Some(c) = band_color(bands, self.sim.heat[i]) {
                let o = i * 4;
                buf[o] = c[0];
                buf[o + 1] = c[1];
                buf[o + 2] = c[2];
                buf[o + 3] = 255;
            }
        }

        for s in &self.sparks.list {
            let f = s.life / s.max;
            let c = if s.kind == SparkKind::Ring {
                if f > 0.6 {
                    [255, 232, 160]
                } else if f > 0.3 {
                    [255, 140, 40]
                } else {
                    [194, 60, 12]
                }
            } else {
                spark_color(f)
            };

            for dy in 0..s.size {
                for dx in 0..s.size {
                    let x = s.x.round() as i32 + dx as i32;
                    let y = s.y.round() as i32 + dy as i32;
                    if x < 0 || y < 0 || x >= FIRE_W as i32 || y >= FIRE_H as i32 {
                        continue;
                    }
                    let o = (y as usize * FIRE_W + x as usize) * 4;
                    buf[o] = c[0];
                    buf[o + 1] = c[1];
                    buf[o + 2] = c[2];
                    buf[o + 3] = 255;
                }
            }
        }
        self.frames += 1;
    }
}
