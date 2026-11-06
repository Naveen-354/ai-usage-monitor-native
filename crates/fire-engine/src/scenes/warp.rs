//! Starfield Warp: tiny ship flying at the vanishing point, stars streaking outward.
//! Live: streaks stream outward and the ship vibrates. Idle: still twinkling dots.
//! Off: parked ship, a few dim dots. Surge: hyperspace — streaks stretch, flash + ring, colour shift.
use crate::canvas::{pal, Canvas};
use crate::engine::FlameState;
use crate::scene::{hash01, Ctx, Scene};
use std::f32::consts::TAU;

const CX: f32 = 40.0;
const CY: f32 = 40.0;
const NSTARS: u32 = 34;

pub struct Warp;

impl Warp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Warp {
    fn default() -> Self {
        Self::new()
    }
}

/// Direction + speed + phase + brightness for star `i` (pure hash, no state).
fn star(i: u32) -> (f32, f32, f32, f32) {
    let ang = hash01(i * 7 + 1) * TAU;
    let spd = hash01(i * 7 + 2);
    let ph = hash01(i * 7 + 3);
    let br = hash01(i * 7 + 11);
    (ang, spd, ph, br)
}

fn pos(sx: f32, sy: f32, ang: f32, r: f32) -> (f32, f32) {
    (sx + ang.cos() * r * 1.10, sy + ang.sin() * r * 0.68)
}

fn streak_len(spd: f32, bright: bool, stretch: f32) -> f32 {
    (2.5 + 4.5 * spd) * (if bright { 1.7 } else { 0.85 }) * stretch
}

impl Scene for Warp {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let d = cx.detail;
        let m = cx.motion;
        let boost = if off { 0.0 } else { cx.boost.clamp(0.0, 1.0) };
        let mag = cx.surge_mag.clamp(0.0, 1.0);
        let surging = !off && boost > 0.02 && cx.surge_age < cx.surge_len && cx.surge_age < 10.0;
        let t = if live || surging { cx.t } else { 0.0 };

        // Ship vibration: only in Live or mid-surge; still in Idle/Off.
        let amp = (if live { 0.6 * m } else { 0.0 }) + boost * 1.4;
        let sx = CX + (t * 13.0).sin() * amp * 0.6;
        let sy = CY + (t * 17.0).sin() * amp * 0.5;

        // Faint tunnel rings (elliptical, like the poster's tunnel).
        let ring_col = if off { pal::DKGREY } else if surging && boost > 0.4 { pal::CYAN } else { pal::DKGREY };
        for (rx, ry) in [(17.5, 10.5), (25.0, 15.0)] {
            c.ring(sx, sy, rx, ry, 1.0, ring_col);
        }

        if off {
            // Parked: a few dim dots only.
            for i in 0..NSTARS {
                if i % 3 != 0 { continue; }
                let (ang, spd, _, _) = star(i);
                let (x, y) = pos(sx, sy, ang, 11.0 + spd * 14.0);
                c.dot(x, y, if spd > 0.5 { pal::LTGREY } else { pal::GREY });
            }
        } else if live || surging {
            // Streaks outward; stretch, speed and brighten with the surge.
            let rmax = 22.0 + boost * 10.0 * (0.4 + 0.6 * mag);
            let stretch = 1.0 + boost * (2.0 + 2.0 * mag);
            let speed = 1.0 + boost * 1.5;
            for i in 0..NSTARS {
                let (ang, spd, ph, br) = star(i);
                let bright = if surging { br < 0.75 } else { br < 0.32 };
                let p = (t * m * (0.30 + 0.50 * spd) * speed + ph).fract();
                let rh = 5.0 + p * rmax;
                if rh < 7.0 { continue; }
                let rt = (rh - streak_len(spd, bright, stretch)).max(2.0);
                let (x1, y1) = pos(sx, sy, ang, rt);
                let (x2, y2) = pos(sx, sy, ang, rh);
                let col = if bright {
                    if br < 0.12 { pal::YELLOW } else if br < 0.22 { pal::CYAN } else { pal::WHITE }
                } else if br < 0.65 { pal::GREY } else { pal::LTBLUE };
                c.line(x1, y1, x2, y2, if bright { 1.8 } else { 1.0 }, col);
                if d >= 1 && bright {
                    c.dot(x2 - 0.5, y2 - 0.5, pal::WHITE);
                }
            }
            if d >= 1 {
                // A few extra dim specks for higher detail.
                for i in NSTARS..NSTARS + 6 {
                    let (ang, _spd, ph, _) = star(i + 100);
                    let p = (t * m * 0.2 * speed + ph).fract();
                    let (x, y) = pos(sx, sy, ang, 6.0 + p * rmax);
                    c.dot(x, y, pal::LTBLUE);
                }
            }
        } else {
            // Idle: still twinkling dots in place (plus sparkles for the brightest).
            for i in 0..NSTARS {
                let (ang, spd, _, br) = star(i);
                let bright = br < 0.32;
                let (x, y) = pos(sx, sy, ang, 11.0 + spd * 14.0);
                let col = if br < 0.12 { pal::YELLOW } else if br < 0.22 { pal::CYAN } else if bright { pal::WHITE } else if br < 0.65 { pal::GREY } else { pal::LTBLUE };
                if bright && d >= 1 && spd > 0.4 {
                    c.line(x - 2.0, y, x + 2.0, y, 0.8, pal::WHITE);
                    c.line(x, y - 2.0, x, y + 2.0, 0.8, pal::WHITE);
                    c.dot(x - 0.5, y - 0.5, pal::WHITE);
                } else {
                    c.dot(x, y, col);
                }
            }
        }

        // Surge flash: bright yellow/white core around the ship (smaller at Low, still pops).
        if surging && boost > 0.02 {
            let fr = 7.0 + boost * 7.0 * (0.4 + 0.6 * mag);
            c.ellipse(sx, sy, fr, fr * 0.62, pal::YELLOW);
            c.ellipse(sx, sy, fr * 0.55, fr * 0.34, pal::WHITE);
        }

        // Engine flame behind the tail (none when parked).
        if !off {
            let fl = if live {
                9.0 + (t * 23.0).sin() * 1.6 * m + boost * 18.0 * (0.5 + 0.5 * mag)
            } else if surging {
                9.0 + boost * 18.0 * (0.5 + 0.5 * mag)
            } else { 5.0 };
            let tip = sx - 12.0 - fl;
            let out = if surging && boost > 0.4 { pal::WHITE } else { pal::YELLOW };
            c.tri((tip, sy), (sx - 12.0, sy - 4.0), (sx - 12.0, sy + 4.0), out);
            c.tri((tip + fl * 0.35, sy), (sx - 12.0, sy - 2.5), (sx - 12.0, sy + 2.5), pal::ORANGE);
            if d >= 1 {
                c.tri((tip + fl * 0.6, sy), (sx - 12.0, sy - 1.2), (sx - 12.0, sy + 1.2), pal::WHITE);
            }
        }

        // Fins, hull, window.
        c.tri((sx - 6.0, sy - 4.0), (sx - 12.0, sy - 11.0), (sx - 14.0, sy - 4.0), pal::RED);
        c.tri((sx - 6.0, sy + 4.0), (sx - 12.0, sy + 11.0), (sx - 14.0, sy + 4.0), pal::RED);
        c.tri((sx - 12.0, sy - 5.0), (sx + 16.0, sy), (sx - 12.0, sy + 5.0), pal::GREY);
        if d >= 1 {
            c.tri((sx - 12.0, sy - 5.0), (sx + 16.0, sy), (sx - 2.0, sy), pal::LTGREY);
        }
        let win = if off { pal::DKGREY } else { pal::BLUE };
        c.ellipse(sx + 2.0, sy, 2.6, 1.9, win);
        if d >= 1 && !off {
            c.dot(sx + 1.0, sy - 0.6, pal::CYAN);
            c.line(sx - 11.0, sy, sx + 8.0, sy, 1.0, pal::DKBLUE);
        }
        if off {
            // Parked legs + sleepy Z.
            c.rect(sx - 6.0, sy + 5.0, 3.0, 4.0, pal::DKGREY);
            c.rect(sx + 6.0, sy + 5.0, 3.0, 4.0, pal::DKGREY);
            c.text(sx + 16.0, sy - 16.0, "Z", pal::LTBLUE, 1.0);
        }

        // Hyperspace ring + shout, while the surge is alive.
        if surging && boost > 0.05 {
            let rr = 6.0 + cx.surge_age * 26.0 * (0.5 + 0.5 * mag);
            if rr < 42.0 {
                c.ring(sx, sy, rr * 1.25, rr * 0.75, 1.5 + boost, pal::YELLOW);
            }
            if d >= 2 && boost > 0.25 {
                let r2 = (rr * 0.6).max(4.0);
                c.ring(sx, sy, r2 * 1.4, r2 * 0.8, 1.2, pal::CYAN);
            }
            if boost > 0.3 {
                let sc = 1.0 + 0.5 * mag;
                let w = Canvas::text_width("ZOOM", sc);
                c.text(sx - w * 0.5, sy + 9.0, "ZOOM", pal::YELLOW, sc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(state: FlameState, t: f32, detail: u8) -> Ctx {
        Ctx { state, t, motion: 1.0, detail, boost: 0.0, surge_age: 1.0e6, surge_len: 1.0, surge_mag: 0.0, ..Ctx::default() }
    }

    #[test]
    fn idle_and_off_ignore_the_clock() {
        let w = Warp::new();
        for state in [FlameState::Idle, FlameState::Off] {
            let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
            w.draw(&mut a, &cx(state, 0.0, 1));
            w.draw(&mut b, &cx(state, 3.7, 1));
            assert_eq!(a.pixels(), b.pixels(), "{state:?}");
        }
    }

    #[test]
    fn streaks_stretch_with_the_surge_and_live_moves() {
        assert!(streak_len(0.5, true, 4.0) > streak_len(0.5, true, 1.0));
        let w = Warp::new();
        let (mut a, mut b) = (Canvas::new(2), Canvas::new(2));
        w.draw(&mut a, &cx(FlameState::Live, 1.0, 1));
        w.draw(&mut b, &cx(FlameState::Live, 1.5, 1));
        assert_ne!(a.pixels(), b.pixels(), "live starfield must move");
    }
}
