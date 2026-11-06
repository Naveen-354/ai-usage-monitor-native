//! PLACEHOLDER: replaced by the real Shoggoth scene.

use crate::canvas::{pal, Canvas};
use crate::scene::{Ctx, Scene};

pub struct Shoggoth;

impl Shoggoth {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Shoggoth {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Shoggoth {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let x = 30.0 + 10.0 * (cx.t * 4.0).sin();
        c.rect(x, 40.0, 20.0, 14.0, pal::DKGREEN);
    }
}
