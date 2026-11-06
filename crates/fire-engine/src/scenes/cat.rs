//! Typing cat: orange cat bashes keyboard, monitor fills with gibberish.
//! Surge swats mug off desk (CRASH!), cat looks smug. Idle upright, Off sleeps on keyboard. Pure in `t`/`surge_age`.
use crate::canvas::{pal::*, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, Ctx, Scene};
use crate::Rng;
use std::f32::consts::TAU;
const TEXT: &[u8] = b"ASDFJKL";
const LETTERS: &[u8] = b"ASDFJKLQWERTYUIOPZXCVBNMGH";
const MUG: (f32, f32) = (58.0, 44.0);
const FLOOR: f32 = 57.0;
const REST: [(f32, f32); 2] = [(29.5, 48.4), (38.5, 48.4)];
const SWAT: f32 = 52.9;
pub struct Cat { typing: u32, splash: u32 }
impl Cat { pub fn new() -> Self { Self { typing: 1, splash: 1 } } }
impl Default for Cat { fn default() -> Self { Self::new() } }
impl Scene for Cat {
    fn reset(&mut self, rng: &mut dyn Rng) { self.typing = (rng.gen_f64() * 1e6) as u32; }
    fn surge(&mut self, _mag: f32, rng: &mut dyn Rng) { self.splash = (rng.gen_f64() * 1e6) as u32; }
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let live = cx.state == FlameState::Live; let off = cx.state == FlameState::Off;
        let hi = cx.detail >= 1; let hi2 = cx.detail >= 2; let m = cx.motion;
        let t = if live { cx.t } else { 0.0 }; let a = cx.surge_age; let len = cx.surge_len; let mag = cx.surge_mag;
        let swat = !off && a < len && a < 10.0;
        let n = (t * 10.0 * (0.5 + 0.5 * m)) as u32;
        screen(c, if off { None } else { Some((n, live)) }, self.typing, swat && a >= 0.45, hi2);
        if off { desk(c); sleeper(c); mug(c, MUG.0, MUG.1, 0, hi); return; }
        let (look, lid, smirk) = if swat && a < 0.22 { ((1.0, 0.5), 0.0, false) }
            else if swat { ((0.0, 0.0), 0.45, true) }
            else if live { (((t * 7.0).sin() * 0.9, -0.7), 0.0, false) }
            else { ((1.0, 0.3), 0.0, false) };
        let up = if swat && a > 0.25 { 1.0 } else { 0.0 };
        tail(c, if live { (t * 11.0).sin() * 1.6 * m } else { 0.0 }, up);
        c.ellipse(34.0, 46.0, 9.5, 8.0, ORANGE); c.ellipse(34.0, 47.0, 4.6, 5.5, CREAM);
        if hi { c.rect(25.4, 41.0, 1.2, 3.0, DKORANGE); c.rect(41.4, 41.0, 1.2, 3.0, DKORANGE); }
        head(c, if live { (t * 11.0).sin() * 0.5 * m } else { 0.0 }, look, lid, smirk, hi); desk(c);
        let mut pw = [REST[0], REST[1]]; let mut hit = [None, None]; let mut blur = [0.0f32; 2];
        if live { for s in 0..2 { (pw[s], hit[s], blur[s]) = typist(t, s as u32, self.typing, 2.5 + 1.5 * m); } }
        if swat { pw[1] = swat_paw(a); hit[1] = None; blur[1] = if (0.08..0.22).contains(&a) { 0.9 } else { 0.0 }; }
        keyboard(c); let (mx, my, rot) = mug_at(a, len, mag); mug(c, mx, my, rot, hi);
        for (s, p) in pw.iter().enumerate() {
            let sx = if s == 0 { 27.5 } else { 40.5 };
            c.line(sx, 43.0, p.0, p.1 - 0.8, 3.4, ORANGE); c.ellipse(p.0, p.1, 2.9, 2.0, ORANGE);
            if hi { c.dot(p.0 - 1.0, p.1 + 0.4, DKORANGE); c.dot(p.0 + 0.3, p.1 + 0.4, DKORANGE); }
            if blur[s] > 0.3 { for (dx, l) in [(-1.6, 3.0), (0.2, 4.0), (1.9, 2.5)] { c.line(p.0 + dx, p.1 - 3.0, p.0 + dx, p.1 - 3.0 - l * blur[s], 0.8, LTORANGE); } }
            if let Some((r, k)) = hit[s] {
                if hi2 { c.ellipse(p.0, p.1 + 0.5, 3.6, 2.4, GLOW); }
                c.rect(21.5 + 3.1 * k as f32, 48.6 + 2.1 * r as f32, 2.3, 1.5, YELLOW);
                c.dot(p.0 - 4.0, p.1 + 1.0, YELLOW); c.dot(p.0 + 3.5, p.1 + 1.0, YELLOW);
            }
        }
        if live && !(swat && a > 0.15) {
            for i in 0..(if hi { 4 } else { 3 }) {
                let f = (t * 1.3 + i as f32 * 0.27).fract();
                c.dot(MUG.0 - 0.5 + (f * 7.0 + i as f32 * 2.0).sin() * 1.4, 40.0 - f * 8.0, GREY);
            }
        }
        if swat && a >= 0.45 { crash(c, a - 0.45, mag, self.splash, mx, hi2); }
    }
}
fn mug_at(a: f32, len: f32, mag: f32) -> (f32, f32, u32) {
    if a >= len || a > 10.0 { return (MUG.0, MUG.1, 0); }
    let pe = 0.22 - 0.08 * mag; let vx = 5.0 + 13.0 * mag; let g = 180.0 + 220.0 * mag;
    if a < pe { return (MUG.0 + 6.0 * ((a - 0.08) / (pe - 0.08)).clamp(0.0, 1.0), MUG.1, 0); }
    if a < 0.52 { let e = a - pe; return (64.0 + vx * e, (MUG.1 + g * e * e).min(FLOOR - 2.5), 1 + (e * 17.0) as u32); }
    let x = 64.0 + vx * (0.52 - pe); let f = ((a - 0.52) / 0.20).min(1.0);
    (x, FLOOR - 2.5 - (1.0 + 5.0 * mag) * 4.0 * f * (1.0 - f), 1)
}
fn swat_paw(a: f32) -> (f32, f32) {
    let lerp = |p: (f32, f32), q: (f32, f32), e: f32| { let e = e.clamp(0.0, 1.0); let e = e * e * (3.0 - 2.0 * e); (p.0 + (q.0 - p.0) * e, p.1 + (q.1 - p.1) * e) };
    if a < 0.08 { lerp(REST[1], (46.5, 38.0), a / 0.08) }
    else if a < 0.22 { (SWAT + 6.0 * ((a - 0.08) / 0.14).clamp(0.0, 1.0), 44.0) }
    else { lerp((SWAT + 6.0, 44.0), REST[1], (a - 0.30) / 0.25) }
}
fn typist(t: f32, side: u32, seed: u32, lift: f32) -> ((f32, f32), Option<(usize, usize)>, f32) {
    let s = t * 3.6 + 0.5 * side as f32; let (j, ph) = (s.floor() as u32, s.fract());
    let h = |k: u32| hash01(seed.wrapping_add(j.wrapping_mul(8)).wrapping_add(side * 4 + k));
    let (col, row) = ((h(0) * 5.0) as usize + 3 * side as usize, (h(1) * 2.0) as usize);
    let down = 0.5 - 0.5 * (ph * TAU).cos();
    let p = (22.6 + 3.1 * col as f32, 47.2 + 2.1 * row as f32 - lift * (1.0 - down));
    (p, (down > 0.6).then_some((row, col)), (ph * TAU).sin().abs())
}
fn ch(c: &mut Canvas, x: f32, y: f32, b: u8, col: Col) { c.text(x, y, core::str::from_utf8(core::slice::from_ref(&b)).unwrap_or(" "), col, 1.0); }
fn screen(c: &mut Canvas, typed: Option<(u32, bool)>, seed: u32, meow: bool, hi2: bool) {
    c.rect(16.0, 22.0, 36.0, 10.0, DKGREY); c.rect(17.0, 23.0, 34.0, 8.0, BLACK);
    let Some((n, live)) = typed else { return };
    if meow { return c.text(26.5, 24.5, "MEOW", YELLOW, 1.0); }
    if !live { for i in 0..7 { ch(c, 20.5 + 4.0 * i as f32, 24.5, TEXT[i as usize], LTGREEN); } return; }
    let total = n as usize; let start = total.saturating_sub(7); let cnt = 7.min(total - start);
    for i in 0..cnt {
        let k = start + i; let h = hash01(seed.wrapping_add(k as u32)) * LETTERS.len() as f32;
        ch(c, 20.5 + 4.0 * i as f32, 24.5, LETTERS[(h as usize) % LETTERS.len()], LTGREEN);
    }
    let cx = 20.5 + 4.0 * cnt as f32; if hi2 { c.ellipse(cx + 0.8, 27.0, 3.2, 3.2, GLOW); } c.rect(cx, 24.5, 2.0, 5.0, GREEN);
}
fn desk(c: &mut Canvas) {
    c.rect(10.0, 47.0, 53.0, 10.0, BROWN); c.rect(10.0, 47.0, 53.0, 2.0, TAN);
    c.rect(49.0, 51.0, 11.0, 5.0, DKBROWN); c.rect(53.5, 52.5, 2.0, 1.0, TAN);
}
fn keyboard(c: &mut Canvas) {
    c.rect(20.5, 47.8, 27.0, 6.4, LTGREY);
    for r in 0..2 { for k in 0..8 { c.rect(21.5 + 3.1 * k as f32, 48.6 + 2.1 * r as f32, 2.3, 1.5, DKGREY); } }
    c.rect(26.0, 52.6, 16.0, 1.2, DKGREY);
}
fn tail(c: &mut Canvas, w: f32, up: f32) {
    let p = [(24.0, 46.5), (19.5, 45.0), (16.8, 41.0), (17.2 + 0.5 * w, 37.0 - 2.0 * up), (20.5 + w - 2.0 * up, 34.3 - 3.5 * up)];
    for (i, s) in p.windows(2).enumerate() { c.line(s[0].0, s[0].1, s[1].0, s[1].1, 3.0, if i == 3 { DKORANGE } else { ORANGE }); }
}
fn head(c: &mut Canvas, dy: f32, look: (f32, f32), lid: f32, smirk: bool, hi: bool) {
    let (x, y) = (34.0, 36.2 + dy); c.ellipse(x, y, 8.6, 6.2, ORANGE);
    for s in [-1.0f32, 1.0] {
        let e = |a: f32, b: f32| (x + s * a, y + b);
        c.tri(e(7.8, -2.2), e(6.8, -7.4), e(2.0, -4.7), ORANGE); c.tri(e(6.6, -3.6), e(6.2, -5.9), e(3.6, -4.4), PINK);
        c.rect(x + s * 2.7 - 0.5, y - 6.0, 1.0, 1.8, DKORANGE);
        if hi { c.line(x + s * 4.5, y + 3.2, x + s * 10.0, y + 2.2, 0.6, LTGREY); c.line(x + s * 4.5, y + 4.0, x + s * 10.0, y + 5.2, 0.6, LTGREY); }
    }
    c.ellipse(x, y + 3.2, 3.3, 2.2, CREAM);
    for s in [-1.0f32, 1.0] {
        let (ex, ey) = (x + s * 3.8, y - 1.0); c.ellipse(ex, ey, 2.3, 2.6, WHITE);
        let (px, py) = (ex - 0.8 + look.0, ey - 1.1 + look.1); c.rect(px, py, 1.6, 2.3, BLACK);
        if hi { c.dot(px + 0.2, py + 0.2, WHITE); }
        if lid > 0.0 { c.rect(ex - 2.4, ey - 2.7, 4.8, 5.2 * lid, ORANGE); c.rect(ex - 2.4, ey - 2.7 + 5.2 * lid, 4.8, 1.0, OUTLINE); }
    }
    c.rect(x - 0.8, y + 1.7, 1.6, 1.0, HOTPINK);
    let w = [(-2.4, 3.8), (-1.2, 4.5), (0.0, 3.6), (1.2, 4.5), (if smirk { 3.0 } else { 2.4 }, if smirk { 3.1 } else { 3.8 })];
    for s in w.windows(2) { c.line(x + s[0].0, y + s[0].1, x + s[1].0, y + s[1].1, 0.8, OUTLINE); }
}
fn mug(c: &mut Canvas, x: f32, y: f32, rot: u32, hi: bool) {
    let r = rot & 3; let (w, h) = if r & 1 == 0 { (5.0, 6.0) } else { (6.0, 5.0) };
    let (dx, dy) = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)][r as usize];
    c.rect(x - w * 0.5, y - h * 0.5, w, h, RED);
    let (rw, rh) = if dx == 0.0 { (w, 1.4) } else { (1.4, h) };
    c.rect(x + dx * (w * 0.5 - 0.7) - rw * 0.5, y + dy * (h * 0.5 - 0.7) - rh * 0.5, rw, rh, DKBROWN);
    c.ring(x + dy * (w * 0.5 + 0.3), y - dx * (h * 0.5 + 0.3), 1.8, 2.0, 1.0, DKRED);
    if hi && r == 0 { c.rect(x - 1.8, y - 1.0, 1.0, 3.0, LTPINK); }
}
fn gag(c: &mut Canvas, x: f32, y: f32, s: &str, col: Col, sc: f32) { c.text(x + 1.0, y + 1.0, s, OUTLINE, sc); c.text(x, y, s, col, sc); }
fn crash(c: &mut Canvas, e: f32, mag: f32, seed: u32, x: f32, hi2: bool) {
    let sc = 1.0 + 0.5 * mag; let (w, cx, cy) = (Canvas::text_width("CRASH", sc), 57.5, 40.0); let pop = (e / 0.05).min(1.0);
    if hi2 { c.ellipse(cx, cy + 2.5 * sc, (w * 0.5 + 5.0) * pop, (3.5 * sc + 3.0) * pop, GLOW); }
    c.ellipse(cx, cy + 2.5 * sc, (w * 0.5 + 3.0) * pop, (3.5 * sc + 2.0) * pop, RED);
    for s in [-1.0f32, 1.0] { c.tri((cx + s * (w * 0.5 + 6.0) * pop, cy + 2.0), (cx + s * w * 0.3, cy - 1.0), (cx + s * w * 0.3, cy + 6.0), RED); }
    gag(c, cx - w * 0.5 + (hash01(seed.wrapping_add((e * 40.0) as u32)) - 0.5) * 1.5, cy, "CRASH", YELLOW, sc);
    c.ellipse(x + 4.5, FLOOR - 0.6, (2.5 + 4.0 * mag) * pop, 0.9, BROWN);
    for i in 0..(3 + (13.0 * mag) as u32) {
        let (h0, h1) = (hash01(seed.wrapping_add(i * 3)), hash01(seed.wrapping_add(i * 3 + 1)));
        let vy = -(40.0 + 60.0 * h1) * (0.5 + 0.5 * mag); let (px, py) = (x + (h0 - 0.4) * 60.0 * e, FLOOR - 1.5 + vy * e + 250.0 * e * e);
        if py < FLOOR - 0.5 { c.rect(px, py, 1.3, 1.3, if i & 1 == 0 { BROWN } else { YELLOW }); }
    }
}
fn sleeper(c: &mut Canvas) {
    c.rect(20.5, 47.8, 27.0, 6.4, LTGREY); c.ellipse(32.0, 45.0, 13.5, 6.0, ORANGE);
    for x in [24.0, 28.0, 32.0] { c.rect(x, 39.5, 1.2, 3.0, DKORANGE); }
    c.ellipse(41.5, 46.0, 5.5, 4.6, ORANGE);
    c.tri((37.0, 44.0), (38.0, 39.5), (41.0, 42.0), ORANGE); c.tri((46.0, 44.0), (45.0, 39.5), (42.0, 42.0), ORANGE);
    c.ellipse(41.5, 48.2, 2.6, 1.6, CREAM); c.ellipse(35.5, 49.6, 2.4, 1.3, CREAM);
    c.rect(38.2, 45.5, 2.0, 1.0, OUTLINE); c.rect(42.8, 45.5, 2.0, 1.0, OUTLINE); c.rect(41.0, 47.0, 1.2, 1.0, HOTPINK);
    for (i, s) in [(19.5, 47.0), (22.0, 51.5), (30.0, 52.8), (37.5, 51.8)].windows(2).enumerate() { c.line(s[0].0, s[0].1, s[1].0, s[1].1, 2.8, if i == 2 { DKORANGE } else { ORANGE }); }
    for (x, y, sc) in [(46.0, 38.0, 1.0), (50.5, 31.5, 1.3), (55.5, 24.0, 1.6)] { gag(c, x, y, "Z", WHITE, sc); }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_mug_is_back_on_the_desk_when_the_surge_ends_and_falls_in_between() {
        assert_eq!(mug_at(1.0, 1.0, 1.0), (MUG.0, MUG.1, 0)); assert_eq!(mug_at(1e6, 1.0, 0.0), (MUG.0, MUG.1, 0));
        assert!(mug_at(0.4, 2.4, 0.5).1 > MUG.1 + 1.0, "mid-surge falling"); assert!(mug_at(0.9, 2.4, 1.0).0 < 76.0 && mug_at(0.9, 2.4, 1.0).1 > 50.0, "on floor");
    }
    #[test]
    fn each_paw_stays_over_the_keyboard() {
        for i in 0..200 { for side in 0..2 { let ((x, y), _, _) = typist(i as f32 * 0.037, side, 9, 4.0); assert!((21.0..47.0).contains(&x) && (42.0..52.0).contains(&y), "{x} {y}"); } }
    }
}
