//! Turbo Snail: green snail, goggles, spiral shell, strapped-on rocket.
//! Live belts along (puffs, speed lines, flashing ZOOM); Idle rests; Off hides with ZZZ; surge spins the shell and blasts the trail.
use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;
use std::f32::consts::TAU;
pub struct Snail {
    puff: Particles<32>,
    emit: f32,
}
impl Snail {
    pub fn new() -> Self {
        Self { puff: Particles::new(), emit: 0.0 }
    }
}
impl Default for Snail {
    fn default() -> Self {
        Self::new()
    }
}
impl Snail {
    fn blast_out(&mut self, mag: f32, cx: &Ctx, rng: &mut dyn Rng) {
        let n = 5 + (16.0 * mag) as usize;
        let bob = bob(cx);
        for _ in 0..n {
            if self.puff.len() >= 32 {
                break;
            }
            let mut life = rand(rng, 0.7, 1.3);
            if cx.state != FlameState::Live {
                life = life.min((cx.surge_len - cx.surge_age).max(0.1));
            }
            let k = cx.motion * (1.0 + cx.boost * 1.4);
            let col = if rand(rng, 0.0, 1.0) < 0.2 { pal::ORANGE } else { pal::GREY };
            self.puff.spawn(Particle {
                x: 29.5 + rand(rng, -0.6, 0.6),
                y: 31.5 + bob + rand(rng, -1.6, 1.6),
                vx: -rand(rng, 16.0, 30.0) * k,
                vy: rand(rng, -3.0, 5.0) * k,
                life,
                max: life,
                col,
                size: rand(rng, 1.7, 2.7) * (1.0 + cx.boost * 0.5),
            });
        }
    }
}
fn bob(cx: &Ctx) -> f32 {
    let live = cx.state == FlameState::Live;
    let surge = cx.state != FlameState::Off && cx.boost > 0.02;
    let wave = if live { cx.t } else if surge { cx.surge_age } else { 0.0 };
    let sp = 5.0 * cx.motion + cx.boost * 11.0;
    (wave * sp).sin() * (0.8 * cx.motion + cx.boost * 0.9)
}
impl Scene for Snail {
    fn reset(&mut self, _rng: &mut dyn Rng) {
        self.puff.clear();
        self.emit = 0.0;
    }
    fn surge(&mut self, _mag: f32, _rng: &mut dyn Rng) {}
    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.puff.step(dt, 0.0, 0.88);
        if cx.state == FlameState::Off {
            self.puff.clear();
            return;
        }
        let live = cx.state == FlameState::Live;
        let surging = cx.boost > 0.05;
        if !live && !surging {
            if cx.state == FlameState::Idle {
                self.puff.clear();
            }
            return;
        }
        self.emit += dt * (13.0 * cx.motion + cx.boost * 45.0);
        while self.emit >= 1.0 {
            self.emit -= 1.0;
            self.blast_out(cx.surge_mag.clamp(0.0, 1.0) * 0.4 + 0.35, cx, rng);
        }
    }
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let surge = !off && cx.boost > 0.02;
        let d = cx.detail;
        let m = cx.motion;
        let t = if live { cx.t } else { 0.0 };
        let wave = if live { t } else if surge { cx.surge_age } else { 0.0 };
        let b = if off { 0.0 } else { cx.boost };
        let k = (cx.surge_age * 40.0) as u32;
        let jx = if surge { (hash01(k) - 0.5) * 2.0 * b } else { 0.0 };
        let jy = if surge { (hash01(k + 9) - 0.5) * 1.6 * b } else { 0.0 };
        let bob = bob(cx) + jy;
        let lean = if live { (t * 5.0).sin() * 0.4 * m } else { 0.0 } + b * 1.5 + jx;
        let (sx, sy) = (34.0 + lean, 42.0 + bob * 0.6);
        let rocket_y = 32.0 + bob * 0.5;
        c.ellipse(40.0, 57.4, 24.0, 2.0, pal::SHADOW);
        let (tx0, tx1, th) = if live || surge { (16.0, 66.0, 1.7) } else if off { (30.0, 52.0, 1.0) } else { (28.0, 64.0, 1.3) };
        c.rect(tx0, 56.8 - th * 0.5, tx1 - tx0, th, pal::LTBLUE);
        if d >= 1 {
            c.rect(tx0 + 2.0, 56.8 - th * 0.5, tx1 - tx0 - 8.0, 0.8, pal::WHITE);
        }
        if live || surge {
            speedlines(c, wave, b, m, bob);
        }
        if (live || surge) && !self.puff.is_empty() {
            for p in self.puff.iter() {
                let f = (p.life / 0.2).min(1.0);
                c.rect(p.x - p.size * 0.5 * f, p.y - p.size * 0.5 * f, p.size * f, p.size * f, p.col);
            }
        }
        if (live || surge) && !off {
            let flick = hash01((wave * 20.0) as u32);
            let len = 5.0 + flick * 3.0 + b * 12.0 + 3.0 * m;
            draw_flame(c, 30.0 + lean * 0.5, rocket_y, len, d);
        }
        if off {
            c.ellipse(38.0, 53.0, 10.0, 2.6, pal::GREEN);
            c.ellipse(46.0, 49.5, 5.0, 3.0, pal::GREEN);
            c.tri((47.0, 48.0), (54.0, 49.5), (47.0, 51.5), pal::GREEN);
        } else {
            c.ellipse(46.0 + lean, 52.0 + bob, 16.0 + b * 2.0, 5.5, pal::GREEN);
            c.ellipse(47.0 + lean, 50.0 + bob, 11.0, 3.0, pal::LTGREEN);
            c.tri((31.0 + lean, 50.5 + bob), (23.0 + lean, 53.0 + bob), (31.0 + lean, 55.0 + bob), pal::GREEN);
            if d >= 1 {
                c.ellipse(50.0 + lean, 54.0 + bob, 9.0, 1.6, pal::DKGREEN);
            }
        }
        shell(c, sx, sy, b, cx.surge_age, cx.surge_mag, d);
        if !off {
            head(c, 60.0 + lean, 45.0 + bob, wave, live, b, m, d);
        }
        rocket(c, 30.0 + lean * 0.5, rocket_y, d);
        if off {
            c.text(47.0, 36.0, "Z", pal::LTBLUE, 1.0);
            c.text(51.0, 30.0, "Z", pal::LTBLUE, 1.0);
            c.text(54.5, 23.5, "Z", pal::LTBLUE, 2.0);
        } else if surge {
            zoom(c, "ZOOOM!!", 1.0, pal::RED, pal::DKRED, b);
        } else if live && (t % 4.0) < 1.0 {
            zoom(c, "ZOOM", ((t % 4.0) / 0.12).min(1.0), pal::BLACK, pal::DKGREY, 0.0);
        }
    }
}
fn speedlines(c: &mut Canvas, wave: f32, b: f32, m: f32, bob: f32) {
    let n = if b > 0.05 { 8 } else { 5 };
    for i in 0..n {
        let fi = i as f32;
        let y = 30.0 + (i % 5) as f32 * 5.0 + bob * 0.4;
        let h = hash01(i * 7 + 3);
        let len = 5.0 + h * 6.0 + b * 9.0;
        let speed = 18.0 + h * 10.0 + b * 36.0;
        let x = 13.0 + ((wave * speed * m + fi * 11.0) % 20.0);
        let col = [pal::LTGREY, pal::WHITE, pal::GREY][(i % 3) as usize];
        c.line(x, y, x + len, y, 1.1 + b * 0.6, col);
    }
}
fn draw_flame(c: &mut Canvas, x: f32, y: f32, len: f32, d: u8) {
    c.tri((x, y - 3.2), (x, y + 3.2), (x - len, y), pal::ORANGE);
    c.tri((x, y - 2.0), (x, y + 2.0), (x - len * 0.62, y), pal::YELLOW);
    if d >= 1 {
        c.tri((x, y - 0.9), (x, y + 0.9), (x - len * 0.3, y), pal::WHITE);
    }
    if d >= 2 {
        c.ellipse(x - len * 0.5, y, len * 0.35, 2.2, pal::GLOW);
    }
}
fn shell(c: &mut Canvas, x: f32, y: f32, b: f32, age: f32, mag: f32, d: u8) {
    let r = 11.0 * (1.0 + b * 0.12);
    c.ellipse(x, y, r, r, pal::TAN);
    c.ellipse(x, y, r - 1.2, r - 1.2, pal::LTORANGE);
    c.ring(x, y, r, r, 1.0, pal::DKBROWN);
    c.ring(x, y, r * 0.68, r * 0.68, 1.0, pal::DKBROWN);
    c.ring(x, y, r * 0.38, r * 0.38, 1.0, pal::DKBROWN);
    if b > 0.05 {
        let th = age * (9.0 + 20.0 * mag) * TAU;
        for k in 0..3 {
            let a = th + k as f32 * 2.094;
            c.line(x, y, x + (r - 1.0) * a.cos(), y + (r - 1.0) * a.sin(), 1.2, pal::DKBROWN);
            c.ellipse(x + r * 0.55 * a.cos(), y + r * 0.55 * a.sin(), 1.6, 1.6, pal::WHITE);
        }
    } else {
        c.dot(x + 1.0, y + 0.5, pal::DKBROWN);
        if d >= 1 {
            c.dot(x - 3.0, y - 4.5, pal::WHITE);
        }
    }
    if d >= 2 {
        c.ellipse(x - r * 0.35, y - r * 0.55, r * 0.3, r * 0.22, pal::GLOW);
    }
}
fn head(c: &mut Canvas, x: f32, y: f32, wave: f32, live: bool, b: f32, m: f32, d: u8) {
    c.ellipse(x, y, 6.5 + b * 0.8, 5.5, pal::GREEN);
    c.ellipse(x - 0.5, y - 1.0, 5.0, 3.5, pal::LTGREEN);
    c.line(x + 1.0, y + 2.2, x + 4.2, y + 1.2, 1.0, pal::DKGREEN);
    c.dot(x - 3.8, y + 1.2, pal::HOTPINK);
    let wob = if live || b > 0.02 { (wave * 3.0).sin() * (0.6 * m + b) } else { 0.0 };
    let (x1, x2) = (x - 3.0 + wob, x + 3.5 - wob);
    c.line(x - 2.0, y - 4.0, x1, y - 11.0, 2.6, pal::GREEN);
    c.line(x + 2.0, y - 4.0, x2, y - 11.0, 2.6, pal::GREEN);
    c.line(x1, y - 11.0, x2, y - 11.0, 2.2, pal::BLACK);
    let blink = live && (wave % 3.2) < 0.16;
    for ex in [x1, x2] {
        c.ellipse(ex, y - 11.0, 2.3, 2.3, pal::WHITE);
        if blink {
            c.line(ex - 1.4, y - 11.0, ex + 1.4, y - 11.0, 1.0, pal::BLACK);
        } else {
            c.rect(ex + 0.1 + b * 0.4, y - 12.2, 1.7, 2.0, pal::BLACK);
            if d >= 1 {
                c.dot(ex + 0.5, y - 11.8, pal::WHITE);
            }
        }
    }
}
fn rocket(c: &mut Canvas, x: f32, y: f32, d: u8) {
    c.rect(x, y - 3.5, 4.0, 2.5, pal::RED);
    c.rect(x, y + 1.0, 4.0, 2.5, pal::RED);
    c.rect(x + 2.0, y - 3.0, 15.0, 6.0, pal::LTGREY);
    c.tri((x + 17.0, y - 3.0), (x + 23.0, y), (x + 17.0, y + 3.0), pal::RED);
    c.rect(x + 6.0, y - 1.5, 4.0, 3.0, pal::LTBLUE);
    c.dot(x + 6.5, y - 1.0, pal::WHITE);
    c.line(x + 6.0, y + 3.0, x + 4.0, y + 10.0, 2.0, pal::DKGREY);
    c.line(x + 12.0, y + 3.0, x + 11.0, y + 10.0, 2.0, pal::DKGREY);
    if d >= 1 {
        c.rect(x + 2.0, y + 1.5, 15.0, 1.2, pal::GREY);
        c.tri((x + 17.0, y - 0.6), (x + 19.5, y), (x + 17.0, y + 0.6), pal::WHITE);
    }
}
fn zoom(c: &mut Canvas, txt: &str, pop: f32, ink: Col, edge: Col, b: f32) {
    let w = Canvas::text_width(txt, 1.0) + 4.0 * pop.max(0.4);
    let h = 9.0 * pop.max(0.5);
    let (x, y) = (15.0, 22.5);
    c.tri((x + w - 4.0, y + h), (x + w, y + h), (33.0, 30.0), edge);
    c.tri((x + w - 3.0, y + h - 0.5), (x + w - 1.0, y + h - 0.5), (33.0, 29.0), pal::WHITE);
    c.rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, edge);
    c.rect(x, y, w, h, pal::WHITE);
    c.text(x + 2.0, y + 2.0, txt, ink, 1.0);
    if b > 0.3 {
        for k in 0..6 {
            let a = k as f32 * TAU / 6.0 + 0.3;
            c.line(x + w * 0.5 + a.cos() * (w * 0.5 + 2.0), y + 4.0 + a.sin() * 6.0, x + w * 0.5 + a.cos() * (w * 0.5 + 5.0 + 4.0 * b), y + 4.0 + a.sin() * (8.0 + 3.0 * b), 1.0, pal::ORANGE);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cx(state: FlameState, t: f32, boost: f32) -> Ctx {
        Ctx { state, t, motion: 1.0, detail: 1, boost, surge_age: 0.3, surge_len: 2.0, surge_mag: 0.8, ..Ctx::default() }
    }
    #[test]
    fn an_idle_snail_looks_the_same_whatever_the_clock_says() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
        let s = Snail::new();
        s.draw(&mut a, &cx(FlameState::Idle, 0.0, 0.0));
        s.draw(&mut b, &cx(FlameState::Idle, 3.7, 0.0));
        assert_eq!(a.pixels(), b.pixels());
    }
    #[test]
    fn a_surge_changes_the_picture() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
        let s = Snail::new();
        s.draw(&mut a, &cx(FlameState::Idle, 0.0, 0.0));
        s.draw(&mut b, &cx(FlameState::Idle, 0.5, 0.9));
        assert_ne!(a.pixels(), b.pixels());
    }
}
