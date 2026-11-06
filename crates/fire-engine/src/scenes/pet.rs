//! PLACEHOLDER: replaced by the real Pet scene. (It already reacts to the day's level so the data tests can run.)

use crate::canvas::{pal, Canvas};
use crate::scene::{Ctx, Scene};

pub struct Pet;

impl Pet {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Pet {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Pet {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let x = 30.0 + 8.0 * (cx.t * 4.0).sin();
        let h = 8.0 + 24.0 * cx.data.level.clamp(0.0, 1.0);
        c.rect(x, 58.0 - h, 18.0, h, pal::PINK);
    }
}
