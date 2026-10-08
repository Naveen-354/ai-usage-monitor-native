use crate::view::AnimationIntensity;

pub struct AnimatedCounter {
    target: f64,
    shown: f64,
    start_time: i64,
    start_val: f64,
    duration_ms: f64,
    last_paint: i64,
}

impl AnimatedCounter {
    pub fn new(val: f64) -> Self {
        Self {
            target: val,
            shown: val,
            start_time: 0,
            start_val: val,
            duration_ms: 0.0,
            last_paint: 0,
        }
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn set_target(&mut self, target: f64, now_ms: i64, intensity: AnimationIntensity, base_duration_ms: f64) {
        if self.target == target {
            return;
        }
        self.target = target;
        let scale = match intensity {
            AnimationIntensity::Off => 0.0,
            AnimationIntensity::Low => 0.5,
            AnimationIntensity::Normal => 1.0,
        };
        let duration = base_duration_ms * scale;
        
        let tiny_delta = (target - self.shown).abs() < 1e-9;
        
        if duration <= 0.0 || tiny_delta {
            self.shown = target;
            self.start_val = target;
            self.duration_ms = 0.0;
        } else {
            self.start_time = now_ms;
            self.start_val = self.shown;
            self.duration_ms = duration;
            self.last_paint = now_ms;
        }
    }

    pub fn tick(&mut self, now_ms: i64) -> f64 {
        if self.duration_ms <= 0.0 || self.shown == self.target {
            return self.shown;
        }
        let elapsed = (now_ms - self.start_time).max(0) as f64;
        let mut t = elapsed / self.duration_ms;
        if t >= 1.0 {
            t = 1.0;
        }
        
        if t >= 1.0 || (now_ms - self.last_paint) >= 33 {
            self.last_paint = now_ms;
            let ease = 1.0 - (1.0 - t).powi(3);
            self.shown = self.start_val + (self.target - self.start_val) * ease;
        }
        
        if t >= 1.0 {
            self.shown = self.target;
            self.duration_ms = 0.0;
        }
        
        self.shown
    }

    pub fn is_animating(&self) -> bool {
        self.duration_ms > 0.0 && self.shown != self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_monotonicity() {
        let mut c = AnimatedCounter::new(0.0);
        c.set_target(100.0, 0, AnimationIntensity::Normal, 700.0);
        let mut prev = 0.0;
        for t in (0..=700).step_by(33) {
            let v = c.tick(t);
            assert!(v >= prev);
            prev = v;
        }
        assert_eq!(c.tick(700), 100.0);
    }
}
