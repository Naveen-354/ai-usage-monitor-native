use crate::Rng;

#[derive(Clone, Copy, PartialEq)]
pub enum SparkKind {
    Spark,
    Ring,
}

pub struct Spark {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub life: f64,
    pub max: f64,
    pub size: usize,
    pub kind: SparkKind,
}

pub fn ring_points(m: f64) -> usize {
    72 + (90.0 * m).round() as usize
}

pub struct Sparks {
    pub list: Vec<Spark>,
}

impl Default for Sparks {
    fn default() -> Self {
        Self::new()
    }
}

impl Sparks {
    pub fn new() -> Self {
        Self { list: Vec::new() }
    }

    pub fn count(&self) -> usize {
        self.list.len()
    }

    pub fn burst(&mut self, count: usize, m: f64, cx: f64, cy: f64, rng: &mut dyn Rng) {
        for _ in 0..count {
            let spread = 1.5 + m * 1.4;
            let angle = -std::f64::consts::PI / 2.0 + (rng.gen_f64() - 0.5) * spread;
            let speed = (0.7 + rng.gen_f64() * 1.5) * (0.8 + 1.4 * m);
            let max = ((14.0 + rng.gen_f64() * 26.0) * (0.7 + m)).round();
            self.list.push(Spark {
                x: cx + (rng.gen_f64() - 0.5) * 6.0,
                y: cy + (rng.gen_f64() - 0.5) * 4.0,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: max,
                max,
                size: if m > 0.6 && rng.gen_f64() < 0.25 { 2 } else { 1 },
                kind: SparkKind::Spark,
            });
        }
    }

    pub fn ring(&mut self, radius: f64, m: f64, cx: f64, cy: f64) {
        let n = ring_points(m);
        let speed = 1.5 + 1.1 * m;
        let max = 4.0_f64.max((radius / speed).round());
        for i in 0..n {
            let a = (i as f64 / n as f64) * std::f64::consts::PI * 2.0;
            self.list.push(Spark {
                x: cx,
                y: cy,
                vx: a.cos() * speed,
                vy: a.sin() * speed * 0.55,
                life: max,
                max,
                size: 1,
                kind: SparkKind::Ring,
            });
        }
    }

    pub fn step(&mut self, dt: f64) {
        let drag = 0.985_f64.powf(dt);
        for s in &mut self.list {
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            if s.kind == SparkKind::Spark {
                s.vy += 0.03 * dt;
                s.vx *= drag;
                s.vy *= drag;
            }
            s.life -= dt;
        }
        self.list.retain(|s| s.life > 0.0);
    }
}
