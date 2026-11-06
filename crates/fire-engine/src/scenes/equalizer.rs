//! Agent equalizer: one bouncing bar per agent with a falling peak cap.
//! Live bars bounce by activity, Idle rests at share height, Off is flat grey. Surge slams all to the top.
use crate::canvas::{pal, Canvas, Col};
use crate::engine::FlameState;
use crate::scene::{hash01, Ctx, Scene};
use std::f32::consts::TAU;

const BASE: f32 = 53.0;
const MAX_H: f32 = 30.0;
const CAP_H: f32 = 2.0;
const BASE_OFF: f32 = 1.5;
const BASE_SCALE: f32 = 24.5;
const DEFAULT_COLS: [Col; 8] = [pal::ORANGE, pal::GREEN, pal::BLUE, pal::PURPLE, pal::HOTPINK, pal::YELLOW, pal::CYAN, pal::RED];
const DEFAULT_SHARES: [f32; 8] = [0.63, 0.90, 0.43, 1.0, 0.70, 0.93, 0.50, 0.77];

pub struct Equalizer;

impl Equalizer {
    pub fn new() -> Self { Self }
}

impl Default for Equalizer {
    fn default() -> Self { Self::new() }
}

impl Scene for Equalizer {
    fn draw(&self, c: &mut Canvas, cx: &Ctx) {
        let off = cx.state == FlameState::Off;
        let live = cx.state == FlameState::Live;
        let m = cx.motion.max(0.0).min(1.0);
        let detail = cx.detail;
        let n = if cx.data.n_agents == 0 { 8 } else { (cx.data.n_agents as usize).min(8).max(1) };
        let step = 52.0 / n as f32;
        let bw = (step - 2.5).max(3.5).min(6.5);
        let surging = !off && cx.boost > 0.001 && cx.surge_age < cx.surge_len;
        let k = if surging { (cx.boost * (0.80 + 0.45 * cx.surge_mag)).max(0.0).min(0.98) } else { 0.0 };
        if !off && detail >= 1 { c.ellipse(40.0, BASE + 2.0, 28.0, 1.4, pal::SHADOW); }
        c.rect(14.0, BASE, 52.0, 1.8, if off { pal::DKGREY } else { pal::GREY });
        for i in 0..n {
            let (share, col, is_live) = if cx.data.n_agents == 0 {
                (DEFAULT_SHARES[i], DEFAULT_COLS[i], true)
            } else {
                let a = cx.data.agents[i];
                (a.share.max(0.12).min(1.0), if a.col.0 == 0 { DEFAULT_COLS[i] } else { a.col }, a.live)
            };
            let base_h = BASE_OFF + share * BASE_SCALE;
            let phase = hash01((i * 7 + 3) as u32) * TAU;
            let (mut h, mut gap) = if off {
                (4.0, 0.0)
            } else if live {
                let mt = cx.t * m;
                if is_live {
                    let freq = 4.2 + hash01((i * 17 + 9) as u32) * 2.8;
                    let amp = 5.0 * (0.55 + 0.45 * share) * m;
                    let u = (mt * 0.55 + hash01((i * 23 + 5) as u32)).fract();
                    ((base_h + amp * (mt * freq + phase).sin()).max(2.8).min(MAX_H), 1.2 + (1.0 - u) * 3.2)
                } else {
                    let sway_freq = 2.2 + hash01((i * 19 + 13) as u32) * 1.2;
                    let sway = 1.3 * m * (mt * sway_freq + phase).sin();
                    ((base_h + sway).max(2.8).min(MAX_H), 1.0 + 0.7 * (mt * sway_freq * 1.3 + phase + 1.0).sin())
                }
            } else {
                (base_h, 0.0)
            };
            if surging { h += (MAX_H - h) * k; gap *= 1.0 - k; }
            let x = 14.0 + step * i as f32 + (step - bw) * 0.5;
            let bar_col = if off { pal::GREY } else { col };
            c.rect(x, BASE - h, bw, h, bar_col);
            if !off && detail >= 1 {
                let mut yy = BASE - 5.0;
                while yy > BASE - h + 2.0 { c.rect(x, yy, bw, 0.8, pal::DKGREY); yy -= 5.0; }
            }
            if !off && detail >= 2 && h > 2.0 { c.rect(x + 0.7, BASE - h + 0.8, 0.8, (h - 1.6).max(0.0), pal::WHITE); }
            let cap_y = (BASE - h - CAP_H - gap).max(22.8);
            let cap_cx = x + bw * 0.5;
            if surging && detail >= 2 { c.ellipse(cap_cx, cap_y + CAP_H * 0.5, bw * 0.55, 1.6, pal::GLOW); }
            let cap_col = if off { pal::DKGREY } else if surging {
                if ((cx.surge_age * 14.0) as u32) % 2 == 0 { pal::YELLOW } else { pal::WHITE }
            } else { pal::WHITE };
            c.rect(x, cap_y, bw, CAP_H, cap_col);
            if !off { c.rect(x, cap_y + CAP_H - 0.9, bw, 0.9, pal::OUTLINE); }
            c.rect(cap_cx - 1.1, BASE + 2.8, 2.2, 2.2, if off { pal::DKGREY } else { col });
        }
        if surging && detail >= 1 {
            let nsp = if detail >= 2 { 10 } else { 6 };
            for j in 0..nsp {
                let seed = (j * 29 + 7) as u32;
                if hash01(seed.wrapping_add((cx.surge_age * 12.0) as u32)) < cx.boost + 0.12 {
                    let sx = 15.0 + hash01(seed + 1) * 50.0;
                    let sy = 21.0 - hash01(seed + 2) * 7.0;
                    c.dot(sx, sy, if j % 2 == 0 { pal::YELLOW } else { pal::WHITE });
                }
            }
        }
        if off {
            c.text(63.0, 38.0, "Z", pal::LTGREY, 1.0);
            c.text(68.0, 31.0, "Z", pal::LTGREY, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{AgentBar, Data};
    fn data(n: u8, share: f32, col: Col) -> Data {
        let mut d = Data { n_agents: n, ..Data::default() };
        for i in 0..n as usize { d.agents[i] = AgentBar { live: true, share, col }; }
        d
    }
    fn render(d: Data, state: FlameState) -> Vec<u8> {
        let mut c = Canvas::new(1);
        let cx = Ctx { state, t: 2.0, data: d, ..Ctx::default() };
        Equalizer::new().draw(&mut c, &cx);
        c.pixels().to_vec()
    }
    #[test]
    fn bar_count_changes_the_picture() {
        assert_ne!(render(data(2, 0.8, pal::ORANGE), FlameState::Live), render(data(6, 0.8, pal::ORANGE), FlameState::Live));
        assert_ne!(render(data(2, 0.8, pal::ORANGE), FlameState::Idle), render(data(6, 0.8, pal::ORANGE), FlameState::Idle));
    }
    #[test]
    fn share_changes_the_picture() {
        assert_ne!(render(data(4, 0.2, pal::GREEN), FlameState::Idle), render(data(4, 1.0, pal::GREEN), FlameState::Idle));
        assert_ne!(render(data(4, 0.2, pal::GREEN), FlameState::Live), render(data(4, 1.0, pal::GREEN), FlameState::Live));
    }
    #[test]
    fn colour_changes_the_picture() {
        assert_ne!(render(data(4, 0.8, pal::ORANGE), FlameState::Idle), render(data(4, 0.8, pal::RED), FlameState::Idle));
        assert_ne!(render(data(4, 0.8, pal::ORANGE), FlameState::Live), render(data(4, 0.8, pal::RED), FlameState::Live));
    }
    #[test]
    fn a_huge_surge_lifts_bars_higher_than_a_small_one() {
        let d = data(6, 0.5, pal::BLUE);
        let small = Ctx { state: FlameState::Live, t: 1.0, boost: 0.25, surge_age: 0.2, surge_len: 1.0, surge_mag: 0.2, data: d, ..Ctx::default() };
        let huge = Ctx { state: FlameState::Live, t: 1.0, boost: 0.95, surge_age: 0.2, surge_len: 2.4, surge_mag: 1.0, data: d, ..Ctx::default() };
        let mut a = Canvas::new(1); Equalizer::new().draw(&mut a, &small);
        let mut b = Canvas::new(1); Equalizer::new().draw(&mut b, &huge);
        assert_ne!(a.pixels(), b.pixels());
        let lit = |c: &Canvas| c.pixels().iter().filter(|&&p| p != 0).count();
        assert!(lit(&b) > lit(&a), "huge surge covers more");
    }
}
