pub mod sim;
pub mod sparks;
pub mod blast;
pub mod engine;
pub mod palette;

pub trait Rng {
    fn gen_f64(&mut self) -> f64;
}

pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        let state = if seed == 0 { 0x123456789abcdef0 } else { seed };
        Self { state }
    }
}

impl Rng for XorShift64 {
    fn gen_f64(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        let v = x >> 11;
        (v as f64) / ((1u64 << 53) as f64)
    }
}
