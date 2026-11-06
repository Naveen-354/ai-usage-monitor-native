//! The animated scenes, one file each. `make` is the only place that knows them all.

use crate::animation::Style;
use crate::scene::Scene;

pub mod cat;
pub mod chicken;
pub mod duck;
pub mod equalizer;
pub mod gum;
pub mod hamster;
pub mod heartbeat;
pub mod muncher;
pub mod octopus;
pub mod panda;
pub mod paperclip;
pub mod pet;
pub mod pig;
pub mod receipt;
pub mod shoggoth;
pub mod slots;
pub mod snail;
pub mod spitter;
pub mod tank;
pub mod warp;

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
        Style::Chicken => Box::new(chicken::Chicken::new()),
        Style::Snail => Box::new(snail::Snail::new()),
        Style::Panda => Box::new(panda::Panda::new()),
        Style::Octopus => Box::new(octopus::Octopus::new()),
        Style::Gum => Box::new(gum::Gum::new()),
        Style::Heartbeat => Box::new(heartbeat::Heartbeat::new()),
        Style::Tank => Box::new(tank::Tank::new()),
        Style::Equalizer => Box::new(equalizer::Equalizer::new()),
        Style::Warp => Box::new(warp::Warp::new()),
        Style::Shoggoth => Box::new(shoggoth::Shoggoth::new()),
        Style::Receipt => Box::new(receipt::Receipt::new()),
        Style::Pet => Box::new(pet::Pet::new()),
        Style::Paperclip => Box::new(paperclip::Paperclip::new()),
    })
}
