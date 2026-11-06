//! One running animation and the texture it paints into.
//!
//! The overlay owns one (the flame or whichever scene is chosen). The Settings page owns a second, a live preview of the
//! chosen style, only while that page is open. Switching style or quality drops the old animation and its buffers, so
//! nothing accumulates: at any time the memory held is the canvas, one RGBA scratch buffer and one texture.

use std::time::Instant;

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};
use fire_engine::blast::Blast;
use fire_engine::engine::FlameState;
use fire_engine::scene::Data;
use fire_engine::{create, Animation, Quality, Style, XorShift64};

pub struct AnimHost {
    name: &'static str,
    style: Style,
    quality: Quality,
    anim: Box<dyn Animation>,
    rng: XorShift64,
    pixels: Vec<u8>,
    texture: Option<TextureHandle>,
    dirty: bool,
    last_update: Instant,
    accum_ms: f64,
    motion: f64,
    /// The day's numbers the scenes that draw them are shown (kept so a new animation gets them straight away).
    data: Data,
}

impl AnimHost {
    pub fn new(name: &'static str, style: Style, quality: Quality) -> Self {
        let anim = create(style, quality);
        let (w, h) = anim.dims();
        Self {
            name,
            style,
            quality,
            anim,
            rng: XorShift64::new(0x00C0_FFEE),
            pixels: vec![0; w * h * 4],
            texture: None,
            dirty: true,
            last_update: Instant::now(),
            accum_ms: 0.0,
            motion: 1.0,
            data: Data::default(),
        }
    }

    /// The day's numbers. A change repaints (a still pose, like an idle tank, then shows the new level).
    pub fn set_data(&mut self, data: Data) {
        if data != self.data {
            self.data = data;
            self.anim.set_data(&data);
            self.dirty = true;
        }
    }

    pub fn mode(&self) -> FlameState {
        self.anim.mode()
    }

    pub fn texture_id(&self) -> Option<TextureId> {
        self.texture.as_ref().map(|t| t.id())
    }

    pub fn next_repaint(&self) -> Option<f64> {
        self.anim.next_repaint()
    }

    /// Switches to another style or quality. The old animation is dropped (and its memory with it) and the new one
    /// picks up where the state was. The classic flame has one look, so a quality change alone does not rebuild it.
    pub fn configure(&mut self, style: Style, quality: Quality) {
        if (style, quality) == (self.style, self.quality) {
            return;
        }
        if style == Style::Fire && self.style == Style::Fire {
            self.quality = quality;
            return;
        }
        let mode = self.anim.mode();
        self.anim = create(style, quality);
        self.style = style;
        self.quality = quality;
        let (w, h) = self.anim.dims();
        self.pixels = vec![0; w * h * 4]; // exactly the new size: the old buffer is freed, not kept
        self.anim.set_data(&self.data);
        self.anim.set_state(mode, self.motion, &mut self.rng);
        self.accum_ms = 0.0;
        self.dirty = true;
    }

    /// Follows the wanted state and runs the animation's own clock.
    pub fn step(&mut self, now: Instant, want: FlameState, motion: f64) {
        let elapsed = (now - self.last_update).as_secs_f64() * 1000.0;
        self.last_update = now;

        if want != self.anim.mode() || (motion - self.motion).abs() > f64::EPSILON {
            self.motion = motion;
            self.anim.set_state(want, motion, &mut self.rng);
            self.dirty = true;
        }

        if self.anim.needs_loop() {
            self.accum_ms += elapsed.min(250.0);
            let frame = self.anim.frame_ms();
            let mut ticks = 0;
            while self.accum_ms >= frame && ticks < 4 {
                self.anim.tick(frame, &mut self.rng);
                self.accum_ms -= frame;
                ticks += 1;
                self.dirty = true;
            }
            if ticks == 4 {
                self.accum_ms = 0.0;
            }
        } else {
            self.accum_ms = 0.0;
        }
    }

    /// A sudden jump of `delta` tokens.
    pub fn blast(&mut self, delta: f64) -> Option<Blast> {
        let b = self.anim.blast(delta, &mut self.rng);
        if b.is_some() {
            self.dirty = true;
        }
        b
    }

    /// Paints into the texture when something changed.
    pub fn upload(&mut self, ctx: &Context) {
        if !self.dirty && self.texture.is_some() {
            return;
        }
        self.anim.paint(&mut self.pixels);
        let (w, h) = self.anim.dims();
        let image = ColorImage::from_rgba_unmultiplied([w, h], &self.pixels);
        match &mut self.texture {
            Some(t) => t.set(image, TextureOptions::NEAREST),
            None => self.texture = Some(ctx.load_texture(self.name, image, TextureOptions::NEAREST)),
        }
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch_bytes(h: &AnimHost) -> usize {
        h.pixels.len()
    }

    #[test]
    fn switching_style_and_quality_resizes_and_frees_the_buffers() {
        let mut h = AnimHost::new("t", Style::Fire, Quality::Medium);
        assert_eq!(scratch_bytes(&h), 80 * 60 * 4);
        h.configure(Style::Hamster, Quality::High);
        assert_eq!((h.pixels.len(), h.pixels.capacity()), (240 * 180 * 4, 240 * 180 * 4), "exactly the sharpest size, nothing spare");
        h.configure(Style::Hamster, Quality::Low);
        assert_eq!((h.pixels.len(), h.pixels.capacity()), (80 * 60 * 4, 80 * 60 * 4), "dropping to Low gives the memory back");
        h.configure(Style::Fire, Quality::High);
        assert_eq!(scratch_bytes(&h), 80 * 60 * 4, "the flame is always its own 80 x 60");
    }

    #[test]
    fn a_quality_change_alone_does_not_rebuild_the_flame() {
        let mut h = AnimHost::new("t", Style::Fire, Quality::Medium);
        let mut rng = XorShift64::new(1);
        h.anim.set_state(FlameState::Live, 1.0, &mut rng);
        for _ in 0..5 {
            h.anim.tick(83.0, &mut rng);
        }
        let before = h.anim.bytes();
        h.configure(Style::Fire, Quality::Low);
        assert_eq!(h.mode(), FlameState::Live, "still burning");
        assert_eq!(h.anim.bytes(), before);
        assert_eq!(h.quality, Quality::Low, "remembered, so the next frame does not compare again");
    }

    #[test]
    fn a_new_animation_picks_up_the_state_the_old_one_was_in() {
        let mut h = AnimHost::new("t", Style::Fire, Quality::Medium);
        let t0 = Instant::now();
        h.step(t0, FlameState::Live, 1.0);
        assert_eq!(h.mode(), FlameState::Live);
        h.configure(Style::Duck, Quality::Medium);
        assert_eq!(h.mode(), FlameState::Live);
        h.step(t0 + Duration::from_millis(500), FlameState::Idle, 1.0);
        assert_eq!(h.mode(), FlameState::Idle);
        assert_eq!(h.next_repaint(), None, "idle is a still pose: no timer");
    }

    #[test]
    fn a_live_animation_asks_for_frames_and_a_stopped_one_does_not() {
        let mut h = AnimHost::new("t", Style::Cat, Quality::Medium);
        let t0 = Instant::now();
        h.step(t0, FlameState::Live, 1.0);
        assert!(h.next_repaint().is_some());
        h.step(t0 + Duration::from_millis(100), FlameState::Live, 0.0);
        assert_eq!(h.next_repaint(), None, "animation intensity Off: a still frame");
    }

    #[test]
    fn upload_makes_one_texture_and_reuses_it_when_the_size_changes() {
        let ctx = Context::default();
        let mut h = AnimHost::new("t", Style::Pig, Quality::Low);
        assert!(h.texture_id().is_none());
        h.upload(&ctx);
        let id = h.texture_id().expect("a texture after the first upload");
        h.configure(Style::Pig, Quality::High);
        h.upload(&ctx);
        assert_eq!(h.texture_id(), Some(id), "same handle, new size: the old texture is replaced, not kept");
        assert_eq!(h.texture.as_ref().unwrap().size(), [240, 180]);
    }
}
