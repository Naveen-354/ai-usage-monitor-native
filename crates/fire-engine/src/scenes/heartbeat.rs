//! Heartbeat monitor: hospital monitor whose ECG traces the recent token rate
//! (`cx.activity`). Idle rests at 60 BPM, Off flatlines ("-- FLAT"), a surge
//! slams a giant spike across the screen, jumps the heart, flashes the frame.

use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{Ctx, Scene};

const DIGITS: &str = "0123456789";
const LX: f32 = 18.0; // trace left
const RX: f32 = 62.0; // trace right
const BASE: f32 = 38.0; // trace baseline

pub struct Heartbeat;

impl Heartbeat {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

fn tri(p: f32, c: f32, w: f32) -> f32 {
    (1.0 - (p - c).abs() / w).clamp(0.0, 1.0)
}

/// One ECG beat (0..=1 phase): small P, small Q dip, tall R, small S dip, T bump.
fn ecg(p: f32) -> f32 {
    0.25 * tri(p, 0.15, 0.08) - 0.20 * tri(p, 0.31, 0.035) + tri(p, 0.35, 0.05)
        - 0.25 * tri(p, 0.39, 0.035) + 0.35 * tri(p, 0.62, 0.10)
}

fn digit(c: &mut Canvas, x: f32, y: f32, n: usize, col: Col) {
    c.text(x, y, &DIGITS[n..n + 1], col, 1.0);
}

fn heart(c: &mut Canvas, cx: f32, cy: f32, s: f32, col: Col, shine: bool) {
    c.ellipse(cx - 1.9 * s, cy, 2.1 * s, 2.0 * s, col);
    c.ellipse(cx + 1.9 * s, cy, 2.1 * s, 2.0 * s, col);
    c.tri(
        (cx - 4.2 * s, cy + 0.5 * s),
        (cx + 4.2 * s, cy + 0.5 * s),
        (cx, cy + 5.0 * s),
        col,
    );
    if shine {
        c.dot(cx - 2.6 * s, cy - 1.5 * s, pal::WHITE);
    }
}

impl Scene for Heartbeat {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let surge_on =
            !off && cx.boost > 0.02 && cx.surge_age >= 0.0 && cx.surge_age < cx.surge_len;
        // 0..1 surge strength, bigger for a bigger jump and a fresher boost.
        let surge = if surge_on {
            cx.boost.clamp(0.0, 1.0) * (0.4 + 0.6 * cx.surge_mag.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let act = if off {
            0.0
        } else if live || surge_on {
            cx.activity.clamp(0.0, 1.0).max(surge * 0.6)
        } else {
            0.0 // Idle rests, whatever the clock or the rate says.
        };
        let t = if live || surge_on { cx.t * cx.motion } else { 0.0 };
        let bpm = if off {
            0
        } else {
            (60.0 + 100.0 * act + 30.0 * surge).round() as i32
        };
        let freq = (bpm.max(60) as f32) / 60.0;
        let amp = 2.0 + 7.0 * act; // calm small blip .. tall spikes
        let cycles = 1.0 + 2.0 * act; // few beats across .. many
        let hi = cx.detail >= 1;
        let hi2 = cx.detail >= 2;

        // ground shadow, stand, grey frame and dark screen (poster card 13).
        c.ellipse(40.0, 57.6, 22.0, 1.4, pal::SHADOW);
        c.rect(37.0, 54.0, 6.0, 2.2, pal::DKGREY);
        c.rect(30.0, 56.0, 20.0, 1.6, pal::GREY);
        c.rect(14.0, 24.0, 52.0, 30.0, pal::DKGREY);
        c.rect(14.0, 24.0, 52.0, 1.0, pal::LTGREY);
        c.rect(17.0, 27.0, 46.0, 22.0, pal::BLACK);
        if hi {
            c.rect(17.0, 48.0, 46.0, 1.0, pal::DKGREY);
            for (sx, sy) in [(15.5f32, 25.5f32), (63.5, 25.5), (15.5, 52.0), (63.5, 52.0)] {
                c.dot(sx, sy, pal::LTGREY);
            }
        }
        let phi = if off || freq <= 0.0 {
            0.0
        } else {
            (t * freq).rem_euclid(1.0)
        };
        let pulse = tri(phi, 0.35, 0.18);
        let hs = 1.0 + 0.35 * pulse * (0.5 + 0.5 * act) + 0.9 * surge;
        let hcy = 31.5 - 0.8 * pulse - 1.8 * surge;
        // faint screen grid (none in Off: the screen is dark there).
        if !off {
            for gx in [20.0f32, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0] {
                for gy in [30.0f32, 35.0, 40.0, 45.0] {
                    c.dot(gx, gy, pal::DKGREEN);
                }
            }
        }
        // the ECG trace, scrolling right with token rate activity.
        let mut last = (LX, BASE);
        if off {
            c.line(LX, BASE, RX, BASE, 2.0, pal::DKGREY);
        } else {
            for i in 0..=44 {
                let x = LX + i as f32;
                let u = i as f32 / 44.0;
                let p = (t * freq - u * cycles).rem_euclid(1.0);
                let w = ecg(p);
                let y = BASE - amp * w;
                if i > 0 {
                    c.line(last.0, last.1, x, y, 2.0, pal::GREEN);
                    if hi && w > 0.85 {
                        c.dot(x, y, pal::WHITE);
                    }
                }
                last = (x, y);
            }
            c.ellipse(last.0, last.1, 1.3, 1.3, pal::WHITE); // lead dot at the pen
        }
        // giant surge spike sweeping across the screen, with a rebound dip.
        if surge > 0.05 {
            let frac = (cx.surge_age / cx.surge_len).clamp(0.0, 1.0);
            let xc = LX + 6.0 + (RX - LX - 12.0) * frac;
            let h = (3.0 + 8.0 * surge).min(BASE - 28.5);
            let hd = (1.5 + 4.0 * surge).min(48.0 - BASE);
            if hi2 {
                c.dot(xc, BASE - h - 1.0, pal::WHITE);
            }
            c.line(xc - 7.0, BASE, xc, BASE - h, 3.0, pal::GREEN);
            c.line(xc, BASE - h, xc + 7.0, BASE, 3.0, pal::GREEN);
            c.line(xc + 7.0, BASE, xc + 12.0, BASE + hd, 2.5, pal::GREEN);
            c.line(xc + 12.0, BASE + hd, xc + 16.0, BASE, 2.5, pal::GREEN);
            c.line(xc - 7.0, BASE, xc, BASE - h, 1.0, pal::LTGREEN);
            c.line(xc, BASE - h, xc + 7.0, BASE, 1.0, pal::LTGREEN);
        }
        // BPM readout: digits from DIGITS, then "BPM"; Off reads "-- FLAT".
        if off {
            c.text(20.0, 42.0, "--", pal::LTGREY, 1.0);
            c.text(29.0, 42.0, "FLAT", pal::RED, 1.0);
            c.text(57.0, 12.0, "Z", pal::LTGREY, 2.0);
        } else {
            let mut x = 20.0;
            if bpm >= 100 {
                digit(c, x, 42.0, (bpm / 100) as usize, pal::GREEN);
                x += 4.0;
            }
            digit(c, x, 42.0, ((bpm / 10) % 10) as usize, pal::GREEN);
            x += 4.0;
            digit(c, x, 42.0, (bpm % 10) as usize, pal::GREEN);
            c.text(x + 5.0, 42.0, "BPM", pal::GREEN, 1.0);
        }
        // red heart, pulsing on each beat; grey and still when Off.
        heart(c, 56.5, hcy, hs, if off { pal::DKGREY } else { pal::RED }, !off);
        // status dots and level slot on the frame strip.
        let dots = if off {
            [pal::DKGREY, pal::DKGREY, pal::DKGREY]
        } else {
            [pal::GREEN, pal::YELLOW, pal::RED]
        };
        for (k, d) in [19.0f32, 22.0, 25.0].iter().enumerate() {
            c.rect(*d, 50.8, 1.4, 1.4, dots[k]);
        }
        c.rect(36.0, 50.5, 20.0, 1.6, pal::BLACK);
        c.rect(37.5, 51.0, 17.0, 0.7, if off { pal::DKGREY } else { pal::GREEN });
        // surge: red flash around the frame.
        if surge_on {
            let th = if surge > 0.4 { 2.0 } else { 1.0 };
            c.rect(14.0, 24.0, 52.0, th, pal::RED);
            c.rect(14.0, 54.0 - th, 52.0, th, pal::RED);
            c.rect(14.0, 24.0, th, 30.0, pal::RED);
            c.rect(66.0 - th, 24.0, th, 30.0, pal::RED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;

    fn cx(state: FlameState, activity: f32) -> Ctx {
        Ctx {
            state,
            t: 2.0,
            motion: 1.0,
            detail: 1,
            activity,
            ..Ctx::default()
        }
    }

    fn pixels(state: FlameState, activity: f32, t: f32) -> Vec<u8> {
        let mut c = Canvas::new(2);
        let mut k = cx(state, activity);
        k.t = t;
        Heartbeat::new().draw(&mut c, &k);
        c.pixels().to_vec()
    }

    #[test]
    fn activity_zero_and_one_draw_different_pictures() {
        assert_ne!(
            pixels(FlameState::Live, 0.0, 2.0),
            pixels(FlameState::Live, 1.0, 2.0),
            "the trace, BPM and heart must follow the token rate"
        );
    }

    #[test]
    fn idle_is_a_still_pose_for_any_clock() {
        assert_eq!(
            pixels(FlameState::Idle, 0.0, 0.0),
            pixels(FlameState::Idle, 0.0, 9.0)
        );
    }

    #[test]
    fn off_flatlines_instead_of_beating() {
        assert_ne!(
            pixels(FlameState::Off, 1.0, 2.0),
            pixels(FlameState::Live, 1.0, 2.0)
        );
    }
}
