//! The one interface the app drives, and the choice of what to run behind it.

use crate::blast::Blast;
use crate::engine::{FireEngine, FlameState};
use crate::scene::Data;
use crate::sim::{FIRE_H, FIRE_W};
use crate::Rng;

/// Which animation the overlay shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// The classic pixel flame.
    Fire,
    Hamster,
    Duck,
    Slots,
    Pig,
    Cat,
    Muncher,
    Spitter,
    Chicken,
    Snail,
    Panda,
    Octopus,
    Gum,
    /// Follows the recent token rate.
    Heartbeat,
    /// Its level is how full the day is.
    Tank,
    /// One bar per agent.
    Equalizer,
    Warp,
    /// The AI meme: a many-eyed blob behind a smiley mask.
    Shoggoth,
    /// A receipt printer: paper speed is the token rate, how much paper hangs out is how full the day is.
    Receipt,
    /// A handheld pet that evolves as the day fills.
    Pet,
    /// A bent-wire paperclip in sunglasses, vibe coding.
    Paperclip,
}

impl Style {
    pub const ALL: [Style; 21] = [
        Style::Fire,
        Style::Hamster,
        Style::Duck,
        Style::Slots,
        Style::Pig,
        Style::Cat,
        Style::Muncher,
        Style::Spitter,
        Style::Chicken,
        Style::Snail,
        Style::Panda,
        Style::Octopus,
        Style::Gum,
        Style::Heartbeat,
        Style::Tank,
        Style::Equalizer,
        Style::Warp,
        Style::Shoggoth,
        Style::Receipt,
        Style::Pet,
        Style::Paperclip,
    ];
}

/// How sharp and how smooth the animated scenes are. The classic flame ignores it (it is always its own 80 x 60 self).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// 80 x 60 pixels, 8 frames a second: chunky, the lightest.
    Low,
    /// 160 x 120 pixels, 12 frames a second.
    Medium,
    /// 240 x 180 pixels, 20 frames a second, plus the extra flourishes scenes keep for it.
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    /// Canvas pixels per logical unit.
    pub fn scale(self) -> usize {
        match self {
            Quality::Low => 1,
            Quality::Medium => 2,
            Quality::High => 3,
        }
    }

    pub fn frame_ms(self) -> f64 {
        match self {
            Quality::Low => 125.0,
            Quality::Medium => 83.0,
            Quality::High => 50.0,
        }
    }

    /// 0, 1 or 2: how many optional flourishes a scene may add.
    pub fn detail(self) -> u8 {
        match self {
            Quality::Low => 0,
            Quality::Medium => 1,
            Quality::High => 2,
        }
    }
}

/// What the app needs from whatever is animating: a texture size, a tick, a surge and a way to paint RGBA.
pub trait Animation {
    fn mode(&self) -> FlameState;
    /// Texture size in pixels. Fixed for the life of the animation (a quality change builds a new one).
    fn dims(&self) -> (usize, usize);
    fn set_state(&mut self, mode: FlameState, motion: f64, rng: &mut dyn Rng);
    /// The day's numbers, for the scenes that draw them (the flame ignores them).
    fn set_data(&mut self, _data: &Data) {}
    /// A sudden jump of `delta` tokens. `None` when the animation is off or there is no motion to show it with.
    fn blast(&mut self, delta: f64, rng: &mut dyn Rng) -> Option<Blast>;
    fn needs_loop(&self) -> bool;
    fn frame_ms(&self) -> f64;
    fn next_repaint(&self) -> Option<f64> {
        self.needs_loop().then(|| self.frame_ms())
    }
    fn tick(&mut self, ms: f64, rng: &mut dyn Rng);
    /// Paints into `buf`, which must be `dims().0 * dims().1 * 4` bytes of RGBA.
    fn paint(&mut self, buf: &mut [u8]);
    /// Bytes of drawing memory held (the part that grows with quality).
    fn bytes(&self) -> usize;
}

impl Animation for FireEngine {
    fn mode(&self) -> FlameState {
        self.mode
    }

    fn dims(&self) -> (usize, usize) {
        (FIRE_W, FIRE_H)
    }

    fn set_state(&mut self, mode: FlameState, motion: f64, rng: &mut dyn Rng) {
        FireEngine::set_state(self, mode, motion, rng)
    }

    fn blast(&mut self, delta: f64, rng: &mut dyn Rng) -> Option<Blast> {
        FireEngine::blast(self, delta, rng)
    }

    fn needs_loop(&self) -> bool {
        FireEngine::needs_loop(self)
    }

    fn frame_ms(&self) -> f64 {
        FireEngine::frame_ms(self)
    }

    fn tick(&mut self, ms: f64, rng: &mut dyn Rng) {
        FireEngine::tick(self, ms, rng)
    }

    fn paint(&mut self, buf: &mut [u8]) {
        FireEngine::paint(self, buf)
    }

    fn bytes(&self) -> usize {
        self.sim.heat.len() * std::mem::size_of::<f32>()
    }
}

/// Builds the animation for a style. Dropping the old one frees its memory, so switching never accumulates.
pub fn create(style: Style, quality: Quality) -> Box<dyn Animation> {
    match crate::scenes::make(style) {
        Some(scene) => Box::new(crate::scene::SceneEngine::new(scene, quality)),
        None => Box::new(FireEngine::new()),
    }
}
