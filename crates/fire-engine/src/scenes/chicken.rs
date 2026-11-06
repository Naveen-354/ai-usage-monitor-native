//! Golden Egg Chicken: plump white hen lays golden T-eggs nonstop, squats, bobs, flaps, BAWK flash.
//! Idle: proud on nest, one egg under her. Off: asleep, ZZZ, no egg.
//! Surge: one giant gold egg, shaking, BAWK!!, feather and small-egg burst (capped by detail).
use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{hash01, rand, Ctx, Particle, Particles, Scene};
use crate::Rng;

pub struct Chicken { parts: Particles<24>, pending: u8, acc: f32 }
impl Chicken { pub fn new() -> Self { Self { parts: Particles::new(), pending: 0, acc: 0.0 } } }
impl Default for Chicken { fn default() -> Self { Self::new() } }

fn egg(c: &mut Canvas, x: f32, y: f32, rx: f32, ry: f32, hi: bool) {
    c.ellipse(x, y, rx + 0.9, ry + 0.9, pal::DKYELLOW);
    c.ellipse(x, y, rx, ry, pal::GOLD);
    if hi { c.ellipse(x - rx * 0.35, y - ry * 0.4, rx * 0.3, ry * 0.25, pal::YELLOW); }
    if rx > 2.2 { c.text(x - 1.5, y - 2.5, "T", pal::BROWN, 1.0); }
}
fn bubble(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    for (g, k) in [(1.0, pal::DKGREY), (0.0, pal::WHITE)] {
        c.rect(x - g + 1.0, y - g, w + 2.0 * g - 2.0, h + 2.0 * g, k);
        c.rect(x - g, y - g + 1.0, w + 2.0 * g, h + 2.0 * g - 2.0, k);
    }
}
fn burst(c: &mut Canvas, x: f32, y: f32, r: f32, spin: f32) {
    for (g, k) in [(1.2, pal::DKRED), (0.0, pal::YELLOW)] {
        let (rx, ry) = (r * 0.55 + g, r * 0.4 + g);
        c.ellipse(x, y, rx, ry, k);
        for i in 0..8 {
            let a = spin + i as f32 * 0.7854;
            let p = |a: f32, s: f32| (x + rx * s * a.cos(), y + ry * s * a.sin());
            c.tri(p(a - 0.45, 0.95), p(a, 1.75), p(a + 0.45, 0.95), k);
        }
    }
}

impl Scene for Chicken {
    fn reset(&mut self, _rng: &mut dyn Rng) { self.parts.clear(); self.pending = 0; self.acc = 0.0; }

    fn surge(&mut self, mag: f32, _rng: &mut dyn Rng) { self.pending = (self.pending as f32 + 8.0 + 20.0 * mag).min(28.0) as u8; }

    fn update(&mut self, dt: f32, cx: &Ctx, rng: &mut dyn Rng) {
        self.parts.step(dt, 85.0, 0.75);
        self.acc += dt * 26.0;
        while self.pending > 0 && self.acc >= 1.0 {
            self.acc -= 1.0;
            self.pending -= 1;
            if self.parts.len() >= [8, 16, 24][cx.detail.min(2) as usize] { continue; }
            let mut life = rand(rng, 0.9, 1.5);
            if cx.state != FlameState::Live { life = life.min((cx.surge_len - cx.surge_age).max(0.05)); }
            let is_egg = rng.gen_f64() < 0.45;
            let (vx, vy) = (rand(rng, -1.0, 1.0) * (10.0 + 24.0 * cx.surge_mag), -rand(rng, 26.0, 56.0) * (0.7 + 0.5 * cx.surge_mag));
            self.parts.spawn(Particle { x: 39.0 + rand(rng, -5.0, 5.0), y: if is_egg { 49.0 } else { 40.0 }, vx, vy, life, max: life,
                col: if is_egg { pal::GOLD } else if rng.gen_f64() < 0.5 { pal::WHITE } else { pal::LTGREY }, size: if is_egg { 2.4 } else { 1.6 } });
        }
        if self.pending == 0 { self.acc = 0.0; }
    }

    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let live = cx.state == FlameState::Live;
        let off = cx.state == FlameState::Off;
        let hi = cx.detail >= 1;
        let hi2 = cx.detail >= 2;
        let surg = !off && cx.surge_age < cx.surge_len;
        let mag = cx.surge_mag.clamp(0.0, 1.0);
        let boost = if surg { cx.boost } else { 0.0 };
        let ct = if live { cx.t * cx.motion } else { 0.0 };
        let per = 1.7;
        let cyc = (ct / per).floor();
        let u = (ct / per).fract();
        let side = if (cyc as i32 % 2) == 0 { 1.0 } else { -1.0 };
        let sq = if u < 0.25 { 2.0 * u / 0.25 } else if u < 0.55 { 2.0 - 3.0 * (u - 0.25) / 0.30 } else { -1.0 * (1.0 - (u - 0.55) / 0.45) };
        let bawk = live && !surg && (cyc as i32 % 3) == 0 && (0.20..0.60).contains(&u);
        let k = (cx.surge_age * 30.0) as u32;
        let shx = if surg { (hash01(k) - 0.5) * 1.8 * boost } else { 0.0 };
        let shy = if surg { (hash01(k + 9) - 0.5) * 1.2 * boost } else { 0.0 };
        let dy = if off { 1.0 } else if live && !surg { sq + (ct * 5.0).sin() * 0.5 } else { 0.0 };
        let bx = 38.0 + shx;
        let by = 44.0 + dy + shy;

        if surg && hi2 { let g = (cx.surge_age / 0.18).min(1.0); c.ellipse(40.0, 54.0, (4.0 + 7.0 * mag) * g, (5.0 + 8.0 * mag) * g, pal::GLOW); }
        // ground shadow + straw nest
        c.ellipse(40.0, 57.3, 20.0, 2.0, pal::SHADOW);
        c.ellipse(40.0, 53.0, 16.0, 4.5, pal::TAN);
        c.ellipse(40.0, 52.3, 13.0, 3.6, pal::BROWN);
        for i in 0..7 {
            let x = 26.0 + hash01(i * 7 + 3) * 28.0;
            let y = 51.0 + hash01(i * 13 + 5) * 3.0;
            c.line(x, y, x + 3.0, y + 0.5, 0.6, pal::YELLOW);
        }
        c.rect(bx - 8.0, 54.0, 5.0, 2.0, pal::ORANGE); // feet
        c.rect(bx + 8.0, 54.0, 5.0, 2.0, pal::ORANGE);
        if !off && !surg { egg(c, 22.0, 54.5, 2.0, 2.4, hi); egg(c, 58.0, 54.5, 2.0, 2.4, hi); }

        for (rx, ry, tx, ty) in [(bx - 10.0, by - 7.0, bx - 19.0, by - 12.0), (bx - 11.0, by - 3.0, bx - 20.0, by - 5.0), (bx - 11.0, by + 1.0, bx - 19.0, by + 4.0)] {
            c.tri((rx, ry), (tx, ty), (bx - 13.0, by + 1.0), pal::LTGREY);
            c.tri((rx + 1.5, ry + 1.0), (tx + 2.5, ty + 1.5), (bx - 12.5, by + 0.5), pal::WHITE);
        }
        // body: grey rim under white fill so it reads on light desktops
        c.ellipse(bx, by, 13.6, 10.0, pal::LTGREY);
        c.ellipse(bx, by, 12.6, 9.0, pal::WHITE);
        let lift = if surg { 2.6 } else if live { (ct * 9.0).sin().max(0.0) * 1.5 } else { 0.0 };
        let wy = by - 1.0 - lift; // flapping wing
        c.ellipse(bx - 5.0, wy, 6.5, 4.5, pal::LTGREY);
        c.ellipse(bx - 5.0, wy, 5.5, 3.5, pal::CREAM);
        if hi { for i in 0..3 { c.line(bx - 8.5, wy - 1.5 + i as f32 * 1.5, bx - 2.5, wy - 1.0 + i as f32 * 1.5, 0.6, pal::DKGREY); } }

        let hx = bx + 10.0 + if off { -2.0 } else { 0.0 };
        let hy = by - 9.0 + if off { 4.0 } else { 0.0 };
        c.ellipse(hx, hy, 7.0, 6.5, pal::LTGREY);
        c.ellipse(hx, hy, 6.2, 5.7, pal::WHITE);
        for (cx0, cy0, cr) in [(hx - 4.0, hy - 5.0, 2.2), (hx, hy - 6.3, 2.5), (hx + 4.0, hy - 5.0, 2.2)] {
            c.ellipse(cx0, cy0 + if off { 1.5 } else { 0.0 }, cr, cr, pal::RED);
        }
        if off {
            c.line(hx - 3.0, hy - 1.0, hx + 3.5, hy - 1.0, 1.2, pal::OUTLINE);
        } else {
            let es = if surg { 1.25 } else { 1.0 };
            c.ellipse(hx + 1.0, hy - 1.0, 3.2 * es, 3.8 * es, pal::LTGREY);
            c.ellipse(hx + 1.0, hy - 1.0, 2.6 * es, 3.2 * es, pal::WHITE);
            let (lx, ly) = if surg { (0.0, 0.9) } else if live { ((ct * 2.0).sin(), -0.3) } else { (0.9, 0.0) };
            c.ellipse(hx + 1.0 + lx * 1.2, hy - 1.0 + ly * 1.2, 1.5, 1.9, pal::BLACK);
            if hi { c.dot(hx + 0.6 + lx * 1.2, hy - 2.2 + ly * 1.2, pal::WHITE); }
        }
        let open = if surg || bawk || off { 1.6 } else { 0.0 }; // beak gapes when it bawks
        c.tri((hx + 4.5, hy - 0.5), (hx + 10.0, hy + 1.5), (hx + 4.5, hy + 2.5), pal::ORANGE);
        c.tri((hx + 4.5, hy + 2.5), (hx + 8.0, hy + 3.0 + open), (hx + 4.5, hy + 3.5 + open), pal::DKORANGE);
        c.ellipse(hx + 3.0, hy + 4.5, 1.6, 2.2, pal::DKRED);

        // eggs in front of her: laid / idle / giant
        if surg {
            let g = (cx.surge_age / 0.18).min(1.0);
            let (grx, gry) = ((3.0 + 6.0 * mag) * g, (3.6 + 7.5 * mag) * g);
            egg(c, 40.0 + shx * 0.5, 57.0 - gry, grx, gry, hi);
        } else if live {
            let (ex, ey, es) = if u < 0.25 { (40.0 + side * 2.0, 54.0, 0.4 + 0.6 * (u / 0.25)) }
                else if u < 0.45 { let s = (u - 0.25) / 0.2; (40.0 + side * (2.0 + 12.0 * s), 53.5 - 5.0 * 4.0 * s * (1.0 - s), 1.0) }
                else { (40.0 + side * 14.0, 54.5, 1.0) };
            if hi2 { c.ellipse(ex, ey, 4.4 * es, 4.6 * es, pal::GLOW); }
            egg(c, ex, ey, 2.8 * es, 3.4 * es, hi);
        } else if !off { egg(c, 40.0, 54.0, 3.0, 3.6, hi); }

        // burst particles: feathers (squares) and small gold eggs
        for p in self.parts.iter() {
            let y = p.y.min(55.8);
            if p.col == pal::GOLD { egg(c, p.x, y, p.size * 0.8, p.size, hi); } else { c.rect(p.x - 0.8, y - 0.5, 1.6, 1.0, p.col); }
        }

        if surg {
            burst(c, 29.0, 27.0, (13.0 + 5.0 * mag) * (0.4 + 0.6 * boost.min(1.0)), cx.surge_age * 0.7);
            c.text(17.5, 24.5, "BAWK!!", pal::RED, 1.0);
        } else if bawk {
            c.tri((32.0, 31.0), (38.0, 31.0), (45.0, 33.0), pal::DKGREY);
            c.tri((33.5, 30.5), (37.5, 30.5), (44.0, 32.5), pal::WHITE);
            bubble(c, 18.0, 23.0, 20.0, 8.0);
            c.text(20.5, 24.5, "BAWK!", pal::BLACK, 1.0);
        }
        if off {
            c.text(50.0, 32.0, "Z", pal::LTBLUE, 1.0);
            c.text(55.0, 26.0, "Z", pal::LTBLUE, 1.4);
            c.text(60.0, 20.0, "Z", pal::LTBLUE, 1.8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cx(state: FlameState, t: f32, detail: u8) -> Ctx { Ctx { state, t, motion: 1.0, detail, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() } }
    #[test]
    fn an_idle_chicken_looks_the_same_whatever_the_clock_says() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
        let hen = Chicken::new();
        hen.draw(&mut a, &cx(FlameState::Idle, 0.0, 1));
        hen.draw(&mut b, &cx(FlameState::Idle, 3.7, 1));
        assert_eq!(a.pixels(), b.pixels());
    }
    #[test]
    fn the_burst_respects_the_cap_for_the_detail_level() {
        let mut rng = crate::XorShift64::new(1);
        let mut hen = Chicken::new();
        hen.surge(1.0, &mut rng);
        let mut most = 0;
        for i in 0..40 {
            let c = Ctx { surge_age: i as f32 * 0.05, surge_len: 2.4, surge_mag: 1.0, ..cx(FlameState::Live, 0.0, 0) };
            hen.update(0.05, &c, &mut rng);
            most = most.max(hen.parts.len());
        }
        assert!(most > 4 && most <= 8, "{most}");
    }
}
