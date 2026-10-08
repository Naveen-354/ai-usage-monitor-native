use crate::Rng;

pub const FIRE_W: usize = 80;
pub const FIRE_H: usize = 60;
const CEILING_ROWS: usize = 12;

pub struct FireParams {
    pub fuel: f64,
    pub half_width: f64,
    pub decay: f64,
    pub edge_decay: f64,
    pub sway: f64,
    pub inward: f64,
    pub jitter: f64,
    pub blur: f64,
}

impl Default for FireParams {
    fn default() -> Self {
        Self {
            fuel: 0.0,
            half_width: 0.0,
            decay: 0.0,
            edge_decay: 0.0,
            sway: 0.3,
            inward: 0.35,
            jitter: 0.5,
            blur: 0.5,
        }
    }
}

pub struct FireSim {
    pub w: usize,
    pub h: usize,
    pub heat: Vec<f32>,
}

impl FireSim {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            heat: vec![0.0; w * h],
        }
    }

    pub fn reset(&mut self) {
        self.heat.fill(0.0);
    }

    pub fn step(&mut self, p: &FireParams, rng: &mut dyn Rng) {
        let w = self.w;
        let h = self.h;
        let cx = (w as f64 - 1.0) / 2.0;

        // Burning base
        let base = (h - 1) * w;
        for x in 0..w {
            let d = (x as f64 - cx).abs() / p.half_width;
            let profile = if d >= 1.0 { 0.0 } else { 1.0 - d.powf(2.4) };
            self.heat[base + x] = (p.fuel * profile * (0.86 + 0.14 * rng.gen_f64())) as f32;
        }

        // Rise and cool
        let sway = p.sway;
        let inward = p.inward;
        let jitter = p.jitter;
        let blur = p.blur;

        for y in 0..(h - 1) {
            let row = y * w;
            let below = (y + 1) * w;
            for x in 0..w {
                let mut dx = if rng.gen_f64() < sway {
                    if rng.gen_f64() < 0.5 { -1 } else { 1 }
                } else {
                    0
                };

                if x as f64 > cx + 1.0 && rng.gen_f64() < inward {
                    dx = 1;
                } else if (x as f64) < cx - 1.0 && rng.gen_f64() < inward {
                    dx = -1;
                }

                let mut sx = x as i32 + dx;
                if sx < 0 {
                    sx = 0;
                } else if sx >= w as i32 {
                    sx = w as i32 - 1;
                }
                let sx = sx as usize;

                let edge = (3.0_f64).min((x as f64 - cx).abs() / (p.half_width * 1.15));
                let ceiling = if y < CEILING_ROWS { ((CEILING_ROWS - y) as f64) * 0.05 } else { 0.0 };
                let cool = p.decay * (1.0 + p.edge_decay * edge * edge) * (1.0 - jitter / 2.0 + jitter * rng.gen_f64()) + ceiling;

                let l_idx = below + if sx > 0 { sx - 1 } else { 0 };
                let r_idx = below + if sx < w - 1 { sx + 1 } else { w - 1 };
                
                let l = self.heat[l_idx] as f64;
                let r = self.heat[r_idx] as f64;
                let mid = self.heat[below + sx] as f64;

                let v = mid * (1.0 - blur) + (l + r) * 0.5 * blur - cool;
                self.heat[row + x] = if v > 0.0 { v as f32 } else { 0.0 };
            }
        }
    }

    pub fn flare(&mut self, height: f64, half_width: f64, intensity: f64, rng: &mut dyn Rng) {
        let w = self.w;
        let h = self.h;
        let cx = (w as f64 - 1.0) / 2.0;
        let rows = ((h - 1) as f64).min(1.0_f64.max(height.round())) as usize;

        for r in 0..rows {
            let y = h - 1 - r;
            let t = r as f64 / rows as f64;
            let half = 1.0_f64.max(half_width * (1.0 - 0.7 * t));
            for x in 0..w {
                let d = (x as f64 - cx).abs() / half;
                if d >= 1.0 { continue; }
                let v = intensity * (1.0 - 0.55 * t) * (1.0 - d).powf(0.7) * (0.9 + 0.1 * rng.gen_f64());
                let i = y * w + x;
                if (v as f32) > self.heat[i] {
                    self.heat[i] = v as f32;
                }
            }
        }
    }

    pub fn top_row(&self, min: f32) -> usize {
        for y in 0..self.h {
            for x in 0..self.w {
                if self.heat[y * self.w + x] >= min {
                    return y;
                }
            }
        }
        self.h
    }

    pub fn total_heat(&self) -> f32 {
        self.heat.iter().sum()
    }
}
