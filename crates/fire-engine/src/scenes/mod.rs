//! The animated scenes, one file each. `make` is the only place that knows them all.

use crate::animation::Style;
use crate::scene::Scene;

pub mod cat;
pub mod duck;
pub mod hamster;
pub mod muncher;
pub mod pig;
pub mod slots;
pub mod spitter;

/// The scene for a style; `None` for the classic flame, which is not a scene.
pub fn make(style: Style) -> Option<Box<dyn Scene>> {
    Some(match style {
        Style::Fire => return None,
        Style::Hamster => Box::new(hamster::Hamster::new()),
        Style::Duck => Box::new(duck::Duck::new()),
        Style::Slots => Box::new(slots::Slots::new()),
        Style::Pig => Box::new(pig::Pig::new()),
        Style::Cat => Box::new(cat::Cat::new()),
        Style::Muncher => Box::new(muncher::Muncher::new()),
        Style::Spitter => Box::new(spitter::Spitter::new()),
    })
}
