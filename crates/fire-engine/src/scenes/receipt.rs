//! PLACEHOLDER: replaced by the real Receipt scene. (It already reacts to the token rate and the day's level so the data tests can run.)

use crate::canvas::{pal, Canvas};
use crate::scene::{Ctx, Scene};

pub struct Receipt;

impl Receipt {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Receipt {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Receipt {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let x = 24.0 + 6.0 * (cx.t * 8.0).sin();
        let h = 12.0 + 20.0 * cx.data.level.clamp(0.0, 1.0);
        let w = 14.0 + 10.0 * cx.activity.clamp(0.0, 1.0);
        c.rect(x, 58.0 - h, w, h, pal::WHITE);
    }
}
