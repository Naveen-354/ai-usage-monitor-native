//! Water tank: a glass tank whose water level is the day's fullness. Live sloshes
//! with rising bubbles, Idle rests flat, Off is drained and dry. A surge splashes
//! over the rim with flying droplets.
use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

const CX: f32 = 40.0;
const BOT: f32 = 54.0; // water bottom (inside the glass)
const TOP: f32 = 27.0; // surface at a full day

fn surf_base(level: f32) -> f32 {
    BOT - (0.06 + 0.94 * level.clamp(0.0, 1.0)) * (BOT - TOP)
}

pub struct Tank {
    bubbles: Particles<12>,
    drops: Particles<16>,
    pending: u8,
    acc: f32,
    surge_acc: f32,
}

impl Tank {
    pub fn new() -> Self {
        Self { bubbles: Particles::new(), drops: Particles::new(), pending: 0, acc: 0.0, surge_acc: 0.0 }
    }
}

impl Default for Tank {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Tank {
    fn reset(&mut self, _rng: &mut dyn Rng) {
        self.bubbles.clear();
        self.drops.clear();
        self.pending = 0;
        self.acc = 0.0;
        self.surge_acc = 0.0;
    }

    fn surge(&mut self, mag: f32, _rng: &mut dyn Rng) {
        self.pending = (self.pending as f32 + 4.0 + 20.0 * mag).min(28.0) as u8;
    }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.bubbles.step(dt, 0.0, 1.0);
        self.drops.step(dt, 170.0, 0.9);
        if cx.state == FlameState::Live {
            let rate = (3.0 + 3.0 * cx.detail as f32) * cx.motion.max(0.1);
            self.acc += dt * rate;
            let base = surf_base(cx.data.level);
            while self.acc >= 1.0 {
                self.acc -= 1.0;
                if self.bubbles.len() < 12 {
                    let x = rand(rng, 27.5, 52.5);
                    let y = rand(rng, BOT - 2.0, BOT - 0.5);
                    let vy = -rand(rng, 6.0, 11.0) * cx.motion.max(0.1);
                    let life = ((y - base - 1.0) / vy.abs()).clamp(0.4, 3.0);
                    self.bubbles.spawn(Particle { x, y, vx: 0.0, vy, life, max: life, col: pal::LTBLUE, size: 1.0 });
                }
            }
        }
        // emit queued surge droplets now so their life can fit in the time left
        if self.pending > 0 {
            self.surge_acc += dt * 30.0;
            let remain = (cx.surge_len - cx.surge_age).max(0.05);
            let mag = cx.surge_mag;
            while self.pending > 0 && self.surge_acc >= 1.0 {
                self.surge_acc -= 1.0;
                self.pending -= 1;
                if self.drops.len() < 16 {
                    let life = rand(rng, 0.35, 0.7).min(remain);
                    let vx = rand(rng, -1.0, 1.0) * (12.0 + 22.0 * mag);
                    let vy = -rand(rng, 30.0, 60.0) * (0.7 + 0.5 * mag);
                    let col = if hash01(self.pending as u32 * 7 + 3) < 0.5 { pal::LTBLUE } else { pal::WHITE };
                    let size = rand(rng, 1.0, 2.0);
                    self.drops.spawn(Particle { x: rand(rng, 28.0, 52.0), y: surf_base(cx.data.level) - 2.0, vx, vy, life, max: life, col, size });
                }
            }
        }
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let d = cx.detail;
        let m = cx.motion;
        let base = surf_base(cx.data.level);
        let boost = if cx.state == FlameState::Off { 0.0 } else { cx.boost };
        let lift = boost * (4.0 + 10.0 * cx.surge_mag);
        // ground shadow and side tick marks
        c.ellipse(CX, 58.2, 19.0, 1.4, pal::SHADOW);
        for i in 0..5 {
            c.rect(17.0, 55.0 - 7.0 * i as f32, 4.0, 1.0, pal::GREY);
        }
        // glass tank: dark frame, pale glass, white rim, foot
        c.rect(23.0, 23.0, 34.0, 34.0, pal::DKGREY);
        c.rect(25.0, 25.0, 30.0, 30.0, pal::LTBLUE);
        c.rect(22.0, 22.0, 36.0, 3.0, pal::WHITE);
        c.rect(22.0, 57.0, 36.0, 1.6, pal::DKGREY);

        if off {
            // drained: a frozen drip, a small sheen in the foot, sleepy Zs
            c.ellipse(CX, 53.6, 7.0, 1.0, pal::BLUE);
            c.line(CX, 25.0, CX, 31.0, 1.0, pal::BLUE);
            c.ellipse(CX, 33.5, 1.2, 1.8, pal::BLUE);
            c.ring(CX, 46.0, 3.0, 1.2, 0.8, pal::BLUE);
            c.text(60.0, 31.0, "Z", pal::LTBLUE, 1.0);
            c.text(63.0, 25.0, "Z", pal::LTBLUE, 1.0);
            return;
        }

        // water, column by column so the surface can wave
        let wave = boost * (1.5 + 3.0 * cx.surge_mag);
        let mut surf = [0.0f32; 28];
        for i in 0..28 {
            let x = 26.0 + i as f32;
            let mut y = base - lift;
            if live {
                y += (x * 0.35 + cx.t * 2.2 * m).sin() * 0.9 + (x * 0.62 - cx.t * 3.0 * m).sin() * 0.5;
            }
            if boost > 0.01 {
                y += (x * 0.5 + cx.surge_age * 28.0).sin() * wave;
            }
            surf[i] = y;
            if y < BOT {
                c.rect(x, y, 1.0, BOT - y, pal::BLUE);
            }
        }
        if d >= 1 {
            c.rect(26.0, BOT - 6.0, 28.0, 6.0, pal::DKBLUE);
            for i in 0..28 {
                c.rect(26.0 + i as f32, surf[i], 1.0, 1.0, pal::LTBLUE);
            }
        }
        // foam dome and spill when the surge lifts the water over the rim
        let centre = base - lift;
        if boost > 0.02 {
            if d >= 2 {
                c.ellipse(CX, centre + 1.0, 8.0 * boost + 4.0, 3.0 * boost + 1.8, pal::GLOW);
            }
            c.ellipse(CX, centre + 1.0, 7.0 * boost + 3.0, 2.5 * boost + 1.0, pal::WHITE);
            if centre < 27.0 {
                c.rect(21.0, 25.0, 2.0, 31.0, pal::BLUE);
                c.rect(57.0, 25.0, 2.0, 31.0, pal::BLUE);
                c.ellipse(CX, 57.4, 13.0, 1.3, pal::BLUE);
            }
        }
        // bubbles: live ones rise, idle ones sit still (same hashes, no clock)
        if live {
            for p in self.bubbles.iter() {
                let x = p.x + (p.y * 1.7).sin() * 0.4;
                if x > 26.0 && x < 54.0 && p.y > base - 1.0 && p.y < BOT {
                    c.ellipse(x, p.y, 0.9, 0.9, pal::LTBLUE);
                    if d >= 1 {
                        c.dot(x + 0.4, p.y - 0.4, pal::WHITE);
                    }
                }
            }
        } else if boost <= 0.02 {
            for i in 0..6 {
                let x = 28.0 + hash01(i * 13 + 1) * 24.0;
                let depth = (BOT - 2.0 - base).max(0.0);
                let y = BOT - 1.0 - hash01(i * 91 + 7) * depth;
                c.ellipse(x, y, 0.9, 0.9, pal::LTBLUE);
            }
        }
        if d >= 1 {
            c.line(28.0, 29.0, 32.0, 34.0, 1.0, pal::WHITE);
            c.dot(30.0, 23.0, pal::YELLOW);
            c.dot(50.0, 23.0, pal::YELLOW);
        }
        for p in self.drops.iter() {
            c.rect(p.x - p.size * 0.5, p.y - p.size * 0.5, p.size, p.size, p.col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blue_count(level: f32, scale: usize) -> usize {
        let tank = Tank::new();
        let mut c = Canvas::new(scale);
        let cx = Ctx { state: FlameState::Live, t: 2.0, detail: 0, data: crate::scene::Data { level, ..Default::default() }, ..Ctx::default() };
        tank.draw(&mut c, &cx);
        c.pixels().iter().filter(|&&p| p == pal::BLUE.0).count()
    }

    #[test]
    fn a_full_tank_holds_much_more_blue_than_a_near_empty_one_at_every_scale() {
        for s in 1..=3 {
            let (low, high) = (blue_count(0.05, s), blue_count(0.95, s));
            assert!(high > low * 2 + 20, "scale {s}: low {low}, high {high}");
        }
    }
}
