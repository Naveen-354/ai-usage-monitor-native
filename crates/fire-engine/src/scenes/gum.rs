//! Bubble gum kid: blows a huge pink bubble while LIVE, chews a tiny one while IDLE, sleeps with a snot bubble while OFF.
//! A surge swells the bubble huge then POPS it: POP! burst, splat + gum strings over the face. Pure in `t`/`surge_age`.
use crate::canvas::{pal::*, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, Ctx, Scene};
use std::f32::consts::TAU;
pub struct Gum;
impl Gum { pub fn new() -> Self { Self } }
impl Default for Gum { fn default() -> Self { Self::new() } }
impl Scene for Gum {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let live = cx.state == FlameState::Live;
        let off = cx.state == FlameState::Off;
        let hi = cx.detail >= 1;
        let hi2 = cx.detail >= 2;
        let m = cx.motion;
        let a = cx.surge_age;
        let len = cx.surge_len;
        let mag = cx.surge_mag.clamp(0.0, 1.0);
        let boost = if off { 0.0 } else { cx.boost };
        let surg = !off && a < len && a < 10.0 && boost > 0.02;
        let pop_at = 0.30 + 0.20 * mag;
        let popped = surg && a >= pop_at;
        let p = if popped { a - pop_at } else { -1.0 };
        let t = if live { cx.t } else { 0.0 };
        let tm = t * m;
        // --- geometry: kid centred ~x=38, bubble centre upper-right, ground y=57
        let mut hx = 38.0;
        let mut hy = 37.5;
        if off { hy += 1.8; }
        if live && !popped { hy += 0.4 * (tm * 3.0).sin(); }
        let mut bcx = 54.0;
        let mut bcy = 29.5;
        if !live && !surg { bcx = 44.0; bcy = 41.0; }
        if surg && !popped {
            let k = (a * 40.0) as u32;
            let sh = (hash01(k) - 0.5) * 1.2 * boost;
            hx += sh;
            hy += (hash01(k + 7) - 0.5) * 0.9 * boost;
            bcx += sh;
            bcy += (hash01(k + 3) - 0.5) * 0.9 * boost;
        }
        // --- bubble radius per state
        let s = 0.5 + 0.5 * (tm * 2.4).sin();
        let mut br = if live { 6.2 + 3.8 * s } else if off { 0.0 } else { 2.6 };
        let mut bwobx = 1.0 + 0.08 * (tm * 5.5).sin() + 0.04 * (tm * 9.0 + 1.3).sin();
        let mut bwoby = 1.0 + 0.07 * (tm * 5.5 + 0.4).sin();
        if surg && !popped {
            let r0 = (a / pop_at).clamp(0.0, 1.0);
            let ramp = r0 * r0 * (3.0 - 2.0 * r0);
            br += (4.0 + 9.0 * mag * mag) * ramp;
            bwobx = 1.0 + 0.15 * (a * 28.0).sin();
            bwoby = 1.0 + 0.13 * (a * 34.0 + 0.6).sin();
        }
        if popped { br = 0.0; }
        if br > 15.5 { br = 15.5; }
        let brx = br * bwobx;
        let bry = br * bwoby;
        let flash = live && !popped && (tm * 0.9).fract() < 0.65;
        let tense = surg && !popped && a > pop_at * 0.45;
        let blink = live && !popped && (tm * 0.35).fract() > 0.97;
        c.ellipse(38.0, 57.8, 18.0, 1.8, SHADOW);
        c.rect(27.0, 46.5, 22.0, 10.5, BLUE);
        c.rect(27.0, 46.5, 22.0, 2.0, DKBLUE);
        c.tri((32.0, 46.5), (44.0, 46.5), (38.0, 50.5), WHITE);
        c.rect(24.5, 47.0, 3.0, 7.0, BLUE);
        c.rect(48.5, 47.0, 3.0, 7.0, BLUE);
        c.dot(25.5, 54.5, TAN);
        c.dot(49.5, 54.5, TAN);
        if hi { c.rect(40.0, 50.0, 5.0, 3.0, DKBLUE); c.rect(40.0, 50.0, 5.0, 1.0, DKBLUE); }
        c.ellipse(hx - 11.0, hy + 1.0, 1.8, 2.2, TAN);
        c.ellipse(hx + 11.0, hy + 1.0, 1.8, 2.2, TAN);
        c.ellipse(hx, hy, 11.0, 10.0, TAN);
        c.ellipse(hx, hy - 8.0, 11.5, 5.5, BROWN);
        c.rect(hx - 11.5, hy - 8.0, 23.0, 2.5, BROWN);
        for i in 0..4 {
            let fx = hx - 8.5 + i as f32 * 4.8;
            c.rect(fx, hy - 6.0, 2.8, 3.0, BROWN);
        }
        if hi { c.ellipse(hx - 3.0, hy - 9.5, 4.0, 1.2, DKBROWN); }
        // cheeks (puffed)
        let puff = if off { 0.7 } else if live { 0.95 + 0.25 * s } else { 1.0 };
        c.ellipse(hx - 7.3, hy + 3.6, 3.2 * puff, 2.4 * puff, PINK);
        let pr = if live || off { puff } else { 0.8 };
        c.ellipse(hx + 7.3, hy + 3.6, 3.2 * pr, 2.4 * pr, PINK);
        if hi && !off { c.dot(hx - 7.3, hy + 3.2, HOTPINK); c.dot(hx + 7.3, hy + 3.2, HOTPINK); }
        // eyes: cross-eyed at the bubble, shut when asleep
        let ey = hy - 1.2;
        for side in 0..2 {
            let sgn = if side == 0 { -1.0f32 } else { 1.0f32 };
            let ex = hx + sgn * 4.2;
            c.line(ex - 1.5, ey - 2.8, ex + 1.5, ey - 3.4, 1.0, BROWN);
            if off {
                c.line(ex - 1.9, ey + 0.6, ex + 1.9, ey + 0.6, 1.4, OUTLINE);
                continue;
            }
            let wide = surg && !popped;
            let (w, h) = if wide { (4.4, 5.4) } else { (3.8, 4.6) };
            c.rect(ex - w * 0.5, ey - h * 0.5, w, h, WHITE);
            if blink {
                c.rect(ex - 1.8, ey - 0.4, 3.6, 0.9, OUTLINE);
                continue;
            }
            let (ox, oy) = if popped { (-sgn * 0.8, 0.8) }
                else if wide { (-sgn * 0.5, -1.3) }
                else if live { (-sgn * 0.9 + 0.4 * (s - 0.5), -1.0) }
                else { (-sgn * 0.4, 0.6) };
            let ps = if wide || popped { 1.3 } else { 1.9 };
            c.rect(ex + ox - ps * 0.5, ey + oy - ps * 0.5, ps, ps + 0.3, BLACK);
            if hi { c.dot(ex + ox - 0.2, ey + oy - 0.5, WHITE); }
        }
        if popped {
            c.ellipse(hx, hy + 6.0, 2.4, 2.2, DKRED);
            c.rect(hx - 1.0, hy + 5.5, 2.0, 1.0, HOTPINK);
        } else if off {
            c.rect(hx - 1.0, hy + 5.5, 2.0, 1.2, DKGREY);
        } else {
            c.rect(hx - 1.3, hy + 5.5, 2.6, 1.8, HOTPINK);
        }
        // gum stem + bubble
        if br > 0.5 {
            let (dx, dy) = (bcx - hx, bcy - (hy + 5.5));
            let dl = (dx * dx + dy * dy).sqrt().max(0.001);
            let ex = bcx - dx / dl * br * 0.8;
            let ey2 = bcy - dy / dl * br * 0.8;
            c.line(hx + 0.5, hy + 5.7, ex, ey2, if surg && !popped { 2.8 } else { 2.0 }, HOTPINK);
            if hi2 { c.ellipse(bcx, bcy, brx + 2.2, bry + 2.2, GLOW); }
            c.ellipse(bcx, bcy, brx, bry, LTPINK);
            c.ellipse(bcx, bcy, (brx - 1.0).max(0.5), (bry - 1.0).max(0.5), HOTPINK);
            if hi { c.ellipse(bcx + brx * 0.18, bcy + bry * 0.32, brx * 0.55, bry * 0.34, PINK); }
            c.ellipse(bcx - brx * 0.32, bcy - bry * 0.44, brx * 0.34, bry * 0.2, LTPINK);
            c.ellipse(bcx - brx * 0.36, bcy - bry * 0.48, brx * 0.2, bry * 0.11, WHITE);
            if hi {
                for i in 0..2 {
                    let ang = tm * 3.5 + i as f32 * (TAU * 0.4) + 0.8;
                    c.ellipse(bcx + ang.cos() * brx * 0.92, bcy + ang.sin() * bry * 0.92, 1.9, 1.6, HOTPINK);
                }
            }
        }
        // POP? speech bubble (flashes while live, tense during surge)
        if flash || tense {
            c.tri((30.5, 26.0), (31.5, 29.0), (46.0, 32.0), DKGREY);
            c.tri((31.0, 26.0), (32.0, 29.0), (46.0, 32.0), WHITE);
            bubble_box(c, 14.0, 22.0, 17.0, 7.0, WHITE, DKGREY);
            c.text(15.0, 23.2, "POP?", BLACK, 1.0);
        }
        if (live || tense) && hi && !popped {
            let bob = if live { 0.4 * (tm * 5.0).sin() } else { 0.0 };
            c.dot(hx + 9.5, hy - 3.0 + bob, LTBLUE);
        }
        // surge aftermath: strings, splat, burst, droplets
        if popped {
            let fade = (1.0 - p / (len - pop_at).max(0.6)).clamp(0.0, 1.0);
            for &(dx, dy) in &[(-3.0f32, 3.5f32), (3.0, 3.5), (0.0, -6.0)] {
                c.line(hx, hy + 6.0, hx + dx, hy + dy, 1.4, HOTPINK);
            }
            let k = (0.45 + 0.65 * mag) * (0.35 + 0.65 * fade);
            for &(dx, dy, rx, ry, col) in &[
                (-5.0f32, -4.0f32, 3.0f32, 2.4f32, HOTPINK),
                (0.0, -5.5, 3.4, 2.6, LTPINK),
                (5.0, -3.5, 2.8, 2.2, HOTPINK),
                (-6.5, 0.5, 2.6, 2.0, PINK),
                (6.5, 0.0, 2.6, 2.0, PINK),
                (-3.0, 3.5, 3.0, 2.4, HOTPINK),
                (3.0, 3.5, 3.0, 2.4, HOTPINK),
                (0.0, 5.5, 3.5, 1.8, LTPINK),
            ] {
                c.ellipse(hx + dx, hy + dy, rx * k, ry * k, col);
            }
            if p < 0.65 {
                let env = (1.0 - p / 0.65).clamp(0.0, 1.0);
                let sc = 1.0 + 0.2 * mag;
                let w = Canvas::text_width("POP!", sc);
                burst(c, bcx + 2.0, bcy + 2.5 * sc, (w * 0.5 + 2.5) * env * (0.65 + 0.45 * mag), a * 0.7, WHITE, DKGREY);
                gag(c, bcx + 2.0 - w * 0.5, bcy, "POP!", BLACK, sc);
            }
            let n = (5.0 + 11.0 * mag) as u32;
            for i in 0..16u32 {
                if i >= n { break; }
                let ang = hash01(i * 13 + 5) * TAU;
                let dmax = (10.0 + 14.0 * mag) * (0.45 + 0.55 * hash01(i * 7 + 11));
                let dist = dmax * (p / (p + 0.25)).min(1.0);
                let sz = (1.1 + 0.8 * hash01(i * 3 + 2)) * fade.clamp(0.2, 1.0);
                let col = if i & 1 == 0 { HOTPINK } else { LTPINK };
                c.rect(bcx + ang.cos() * dist - sz * 0.5, bcy + ang.sin() * dist + 0.3 * dist - sz * 0.5, sz, sz, col);
            }
        }
        if off {
            c.line(hx + 0.8, hy + 3.8, hx + 2.5, hy + 4.5, 1.2, LTBLUE);
            c.ellipse(hx + 2.5, hy + 4.8, 3.0, 2.5, LTBLUE);
            c.ellipse(hx + 1.8, hy + 4.0, 0.9, 0.6, WHITE);
            c.text(hx + 8.0, hy - 11.0, "Z", LTBLUE, 1.0);
            c.text(hx + 13.0, hy - 17.0, "Z", LTBLUE, 1.3);
            c.text(hx + 18.0, hy - 23.0, "Z", LTBLUE, 1.6);
        }
    }
}
/// A rounded box with a one-unit border (reads on light and dark desktops alike).
fn bubble_box(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, fill: Col, edge: Col) {
    for (g, k) in [(1.0, edge), (0.0, fill)] {
        c.rect(x - g + 1.0, y - g, w + 2.0 * g - 2.0, h + 2.0 * g, k);
        c.rect(x - g, y - g + 1.0, w + 2.0 * g, h + 2.0 * g - 2.0, k);
    }
}
/// A spiky burst: an ellipse with eight spikes, `r` is roughly its half width.
fn burst(c: &mut Canvas, x: f32, y: f32, r: f32, spin: f32, fill: Col, edge: Col) {
    for (g, k) in [(1.2, edge), (0.0, fill)] {
        let (rx, ry) = (r * 0.55 + g, r * 0.4 + g);
        if rx <= 0.5 || ry <= 0.5 { continue; }
        c.ellipse(x, y, rx, ry, k);
        for i in 0..8 {
            let a = spin + i as f32 * 0.7854;
            let p = |a: f32, s: f32| (x + rx * s * a.cos(), y + ry * s * a.sin());
            c.tri(p(a - 0.45, 0.95), p(a, 1.75), p(a + 0.45, 0.95), k);
        }
    }
}
fn gag(c: &mut Canvas, x: f32, y: f32, s: &str, col: Col, sc: f32) { c.text(x + 1.0, y + 1.0, s, OUTLINE, sc); c.text(x, y, s, col, sc); }
#[cfg(test)]
mod tests {
    use super::*;
    fn cx(state: FlameState, t: f32) -> Ctx { Ctx { state, t, motion: 1.0, detail: 1, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() } }
    fn lit(c: &Canvas) -> usize { c.pixels().iter().filter(|&&p| p != 0).count() }
    #[test]
    fn an_idle_gum_looks_the_same_whatever_the_clock_says() {
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
        let g = Gum::new();
        g.draw(&mut a, &cx(FlameState::Idle, 0.0)); g.draw(&mut b, &cx(FlameState::Idle, 3.7));
        assert_eq!(a.pixels(), b.pixels());
    }
    #[test]
    fn a_surge_swells_the_bubble_and_a_huge_one_more_than_a_small_one() {
        let g = Gum::new();
        let (mut bare, mut small, mut huge) = (Canvas::new(2), Canvas::new(2), Canvas::new(2));
        g.draw(&mut bare, &cx(FlameState::Idle, 0.0));
        let mk = |mag: f32| Ctx { boost: 0.5 + 0.5 * mag, surge_age: 0.25, surge_len: 2.0, surge_mag: mag, ..cx(FlameState::Idle, 0.0) };
        g.draw(&mut small, &mk(0.2)); g.draw(&mut huge, &mk(1.0));
        assert_ne!(bare.pixels(), small.pixels(), "surge draws nothing");
        assert_ne!(small.pixels(), huge.pixels(), "bigger mag draws the same");
        assert!(lit(&huge) > lit(&small), "huge surge covers {}", lit(&huge));
    }
    #[test]
    fn the_popped_splat_fades_by_the_end_of_the_surge() {
        let g = Gum::new();
        let (mut early, mut late) = (Canvas::new(2), Canvas::new(2));
        let mk = |age: f32| Ctx { boost: (2.0 - age) / 2.0, surge_age: age, surge_len: 2.0, surge_mag: 1.0, ..cx(FlameState::Live, 0.5) };
        g.draw(&mut early, &mk(0.6)); g.draw(&mut late, &mk(1.95));
        assert!(lit(&early) > lit(&late) + 20, "splat should shrink: {}/{}", lit(&early), lit(&late));
    }
}
