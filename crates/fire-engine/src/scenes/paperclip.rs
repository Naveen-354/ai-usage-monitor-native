//! PLACEHOLDER: replaced by the real Paperclip scene.

use crate::canvas::{pal, Canvas};
use crate::scene::{Ctx, Scene};

pub struct Paperclip;

impl Paperclip {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Paperclip {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Paperclip {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let x = 30.0 + 10.0 * (cx.t * 4.0).sin();
        c.rect(x, 40.0, 20.0, 14.0, pal::LTGREY);
    }
}
