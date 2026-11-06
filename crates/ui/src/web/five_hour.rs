//! "5-HOUR WINDOW": where the agent stands in its current rolling usage window (Claude Code limits usage per 5 hours).
//!
//! **It is an estimate and says so.** The provider's own figure (how much of the limit is used) is not stored on this
//! computer, so this shows what the local data supports: when the window opened, when it closes, how many tokens went
//! through it on this machine. The bar is the *time* elapsed in the window, not the allowance used: it is deliberately
//! filled (unlike the per-agent bars, which the original design draws as plain lines) because it is the point of the box.

use chrono::{DateTime, Local, TimeZone};
use egui::{vec2, Color32, Pos2, Rect, Sense, Ui};

use super::paint::{border, dashed_border};
use super::text::{Run, Style, Weight};
use super::widgets::{agent_dot, fineprint, put};
use super::wrap::wrap;
use super::{with_alpha, Tokens, BW};
use crate::motion::format::format_compact;
use crate::view::{AnimationIntensity, UsageWindow};

/// The window's length.
pub const WINDOW_MS: i64 = 5 * 3_600_000;

const HEAD_H: f32 = 18.9;
const BAR_H: f32 = 12.0;
const LINE_H: f32 = 13.5;
/// What the box says about itself, below it.
pub const ESTIMATE_NOTE: &str = "ESTIMATE from this PC's Claude Code only. The bar is time elapsed in the 5-hour window, not allowance left; claude.ai and other devices count toward the same limit.";

/// What the box shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State<'a> {
    /// A window is open.
    Open(&'a UsageWindow),
    /// No window is open: the last one ended (or there was no usage), the next request opens one.
    Idle,
    /// The window could not be read. Never drawn as "idle": that would claim there is no limit pressure.
    Unavailable,
}

/// The state to draw for what the backend last answered. An answer that has gone stale (its window ended since) is idle.
pub fn state_for(answer: Option<&Result<Option<UsageWindow>, ()>>, now_ms: i64) -> Option<State<'_>> {
    match answer? {
        Ok(Some(w)) if now_ms < w.resets_utc_ms => Some(State::Open(w)),
        Ok(_) => Some(State::Idle),
        Err(()) => Some(State::Unavailable),
    }
}

pub struct Props<'a> {
    pub tokens: Tokens,
    pub name: &'a str,
    pub color: Color32,
    pub state: State<'a>,
    pub now_ms: i64,
    pub time_ms: u64,
    pub intensity: AnimationIntensity,
}

/// How much of the window has gone, 0..=1.
pub fn elapsed_fraction(w: &UsageWindow, now_ms: i64) -> f32 {
    let len = (w.resets_utc_ms - w.start_utc_ms).max(1) as f64;
    (((now_ms - w.start_utc_ms) as f64) / len).clamp(0.0, 1.0) as f32
}

/// Time left as `2H 13M` (rounded up to whole minutes, so it never reads 0 while the window is still open) or `45M`.
pub fn remaining_label(resets_ms: i64, now_ms: i64) -> String {
    let minutes = ((resets_ms - now_ms).max(0) + 59_999) / 60_000;
    if minutes >= 60 {
        format!("{}H {:02}M", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}M")
    }
}

/// `HH:MM` in `tz`.
pub fn clock_in<Tz: TimeZone>(tz: &Tz, ms: i64) -> String
where
    Tz::Offset: std::fmt::Display,
{
    DateTime::from_timestamp_millis(ms).map(|t| t.with_timezone(tz).format("%H:%M").to_string()).unwrap_or_else(|| "--:--".into())
}

fn clock(ms: i64) -> String {
    clock_in(&Local, ms)
}

fn name_style() -> Style {
    Style::display(12.0).spacing_em(0.06)
}
fn label_style() -> Style {
    Style::mono(10.0, Weight::W700).spacing_em(0.1).lh_px(LINE_H)
}
fn value_style() -> Style {
    Style::mono(14.0, Weight::W800)
}
fn note_style() -> Style {
    Style::mono(10.0, Weight::W400).lh_px(LINE_H)
}

/// A label and its value, as runs of text with the colour each is painted in.
type Parts = Vec<(Run, Color32)>;

const IDLE_NOTE: &str ="The next Claude Code request opens a new 5-hour window.";
const UNAVAILABLE_NOTE: &str = "The window could not be read.";

/// Paints the box into the current `ui` (full available width), then its fine print.
pub fn show(ui: &mut Ui, p: &Props) {
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let t = p.tokens;
    let width = ui.available_width();
    let inner_w = width - 2.0 * BW - 18.0;

    let idle_lines = match p.state {
        State::Idle => wrap(IDLE_NOTE, &note_style(), inner_w, true),
        State::Unavailable => wrap(UNAVAILABLE_NOTE, &note_style(), inner_w, true),
        State::Open(_) => Vec::new(),
    };
    let body = match p.state {
        State::Open(_) => 7.0 + HEAD_H + 5.0 + BAR_H + 5.0 + LINE_H + 8.0,
        State::Idle | State::Unavailable => 7.0 + HEAD_H + 5.0 + idle_lines.len() as f32 * LINE_H + 8.0,
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, body + 2.0 * BW), Sense::hover());
    let left = rect.left() + BW + 9.0;
    let head = Rect::from_min_size(Pos2::new(left, rect.top() + BW + 7.0), vec2(inner_w, HEAD_H));
    let muted = !matches!(p.state, State::Open(_));
    let fg = if muted { t.dim } else { t.ink };

    // head: dot, name, "5-HOUR WINDOW" ... what is left of it (or why there is nothing)
    agent_dot(ui, Pos2::new(left, head.center().y - 4.5), p.color, muted, false, p.time_ms, p.intensity);
    let name = p.name.to_uppercase();
    let name_w = put(ui, &name, &name_style(), left + 17.0, head.top() + (HEAD_H - name_style().line_h) / 2.0, fg);

    let right_text = match p.state {
        State::Open(w) => Some((Some("RESETS IN"), remaining_label(w.resets_utc_ms, p.now_ms))),
        State::Idle => Some((None, "NO WINDOW OPEN".to_string())),
        State::Unavailable => Some((None, "UNAVAILABLE".to_string())),
    };
    let mut right_w = 0.0;
    if let Some((label, value)) = &right_text {
        let open = label.is_some();
        let st = if open { value_style() } else { label_style() };
        let run = Run::new(ui.ctx(), value, &st);
        let value_top = head.top() + if open { 0.0 } else { (HEAD_H - LINE_H) / 2.0 };
        run.paint(ui.painter(), head.right() - run.width, value_top, if open { t.ink } else { t.dim });
        right_w = run.width;
        if let Some(label) = label {
            let lr = Run::new(ui.ctx(), label, &label_style());
            lr.paint(ui.painter(), head.right() - run.width - 8.0 - lr.width, head.top() + (HEAD_H - LINE_H) / 2.0, t.dim);
            right_w += 8.0 + lr.width;
        }
    }
    let title = "5-HOUR WINDOW";
    let title_w = Run::new(ui.ctx(), title, &label_style()).width;
    let title_left = left + 17.0 + name_w + 8.0;
    if title_left + title_w + 12.0 < head.right() - right_w {
        put(ui, title, &label_style(), title_left, head.top() + (HEAD_H - LINE_H) / 2.0, t.dim);
    }

    match p.state {
        State::Open(w) => {
            // the bar: time elapsed, one tick per hour
            let bar = Rect::from_min_size(Pos2::new(left, head.bottom() + 5.0), vec2(inner_w, BAR_H));
            let track = bar.shrink(BW);
            let frac = elapsed_fraction(w, p.now_ms);
            ui.painter().rect_filled(Rect::from_min_size(track.min, vec2(track.width() * frac, track.height())), 0.0, p.color);
            for k in 1..5 {
                let x = (track.left() + track.width() * k as f32 / 5.0).round();
                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x - 0.5, track.top()), vec2(1.0, track.height())), 0.0, with_alpha(t.ink, 0.45));
            }
            border(ui.painter(), bar, BW, t.ink);

            // opened / resets (right) and tokens / requests (left); the left part gives way when the box is narrow
            let y = bar.bottom() + 5.0;
            let pair = |ui: &Ui, label: &str, value: &str| -> Parts {
                vec![(Run::new(ui.ctx(), &format!("{label} "), &label_style()), t.dim), (Run::new(ui.ctx(), value, &label_style()), t.ink)]
            };
            let gap = 14.0;
            let runs_w = |parts: &[(Run, Color32)]| parts.iter().map(|(r, _)| r.width).sum::<f32>();
            let tokens = pair(ui, "TOKENS", &format_compact(w.tokens as f64, 3));
            let reqs = pair(ui, "REQ", &format_compact(w.events as f64, 3));
            let opened = pair(ui, "OPENED", &clock(w.start_utc_ms));
            let resets = pair(ui, "RESETS", &clock(w.resets_utc_ms));
            let paint = |ui: &Ui, parts: &[(Run, Color32)], mut x: f32| {
                for (r, c) in parts {
                    r.paint(ui.painter(), x, y, *c);
                    x += r.width;
                }
            };
            let full = runs_w(&tokens) + gap + runs_w(&reqs) + gap + runs_w(&opened) + gap + runs_w(&resets);
            let (left_parts, right_parts): (Vec<&Parts>, Vec<&Parts>) = if full <= inner_w {
                (vec![&tokens, &reqs], vec![&opened, &resets])
            } else if runs_w(&tokens) + gap + runs_w(&resets) <= inner_w {
                (vec![&tokens], vec![&resets])
            } else {
                (vec![&tokens], vec![])
            };
            let mut x = left;
            for parts in &left_parts {
                paint(ui, parts, x);
                x += runs_w(parts) + gap;
            }
            let mut x = head.right();
            for parts in right_parts.iter().rev() {
                x -= runs_w(parts);
                paint(ui, parts, x);
                x -= gap;
            }
        }
        State::Idle | State::Unavailable => {
            let y = head.bottom() + 5.0;
            for (i, l) in idle_lines.iter().enumerate() {
                put(ui, l, &note_style(), left, y + i as f32 * LINE_H, t.dim);
            }
        }
    }

    if muted {
        dashed_border(ui.painter(), rect, BW, t.ink);
    } else {
        border(ui.painter(), rect, BW, t.ink);
    }
    if !matches!(p.state, State::Unavailable) {
        fineprint(ui, &t, ESTIMATE_NOTE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Theme;
    use chrono::FixedOffset;

    const MIN: i64 = 60_000;
    const HOUR: i64 = 3_600_000;

    fn window(started_ago: i64, now: i64) -> UsageWindow {
        UsageWindow { start_utc_ms: now - started_ago, resets_utc_ms: now - started_ago + WINDOW_MS, tokens: 118_400_000, events: 62 }
    }

    #[test]
    fn the_time_left_rounds_up_so_an_open_window_never_reads_zero() {
        let now = 1_000 * HOUR;
        assert_eq!(remaining_label(now + 2 * HOUR + 13 * MIN, now), "2H 13M");
        assert_eq!(remaining_label(now + 2 * HOUR + 13 * MIN + 1, now), "2H 14M", "a started minute counts");
        assert_eq!(remaining_label(now + 5 * HOUR, now), "5H 00M");
        assert_eq!(remaining_label(now + 59 * MIN, now), "59M");
        assert_eq!(remaining_label(now + 1, now), "1M");
        assert_eq!(remaining_label(now - 5, now), "0M", "never negative");
    }

    #[test]
    fn the_bar_is_time_elapsed_between_nothing_and_everything() {
        let now = 1_000 * HOUR;
        assert_eq!(elapsed_fraction(&window(0, now), now), 0.0);
        assert!((elapsed_fraction(&window(2 * HOUR + 30 * MIN, now), now) - 0.5).abs() < 1e-6);
        assert_eq!(elapsed_fraction(&window(5 * HOUR, now), now), 1.0);
        assert_eq!(elapsed_fraction(&window(9 * HOUR, now), now), 1.0, "a window that has run out is a full bar, not more");
        assert_eq!(elapsed_fraction(&window(-HOUR, now), now), 0.0, "a clock that went backwards is an empty bar, not negative");
    }

    #[test]
    fn the_clock_is_local_time() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        // 04:14 UTC is 09:44 in India
        assert_eq!(clock_in(&ist, 4 * HOUR + 14 * MIN), "09:44");
        assert_eq!(clock_in(&chrono::Utc, 4 * HOUR + 14 * MIN), "04:14");
        assert_eq!(clock_in(&chrono::Utc, i64::MAX), "--:--", "an impossible time prints dashes, not a panic");
    }

    #[test]
    fn a_stale_answer_is_idle_and_a_failed_read_is_never_idle() {
        let now = 1_000 * HOUR;
        let open = Ok(Some(window(HOUR, now)));
        assert!(matches!(state_for(Some(&open), now), Some(State::Open(_))));
        assert_eq!(state_for(Some(&open), now + 4 * HOUR), Some(State::Idle), "the window ended since it was read");
        assert_eq!(state_for(Some(&Ok(None)), now), Some(State::Idle));
        assert_eq!(state_for(Some(&Err(())), now), Some(State::Unavailable), "unknown is not the same as nothing open");
        assert_eq!(state_for(None, now), None, "nothing read yet: draw nothing");
    }

    fn ctx() -> egui::Context {
        let c = egui::Context::default();
        crate::web::install_fonts(&c);
        let _ = c.run(egui::RawInput::default(), |_| {});
        c
    }

    /// Paints the box at `width` and returns (number of shapes, height used).
    fn paint(state: State, width: f32) -> (usize, f32) {
        let ctx = ctx();
        let now = 1_000 * HOUR;
        let mut used = 0.0;
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                ui.set_width(width);
                let top = ui.cursor().top();
                show(ui, &Props { tokens: Tokens::for_theme(Theme::Dark), name: "Claude Code", color: Color32::from_rgb(255, 106, 0), state, now_ms: now, time_ms: 0, intensity: AnimationIntensity::Normal });
                used = ui.cursor().top() - top;
            });
        });
        (out.shapes.len(), used)
    }

    #[test]
    fn it_paints_every_state_at_the_overlay_width_and_in_a_very_narrow_window() {
        let now = 1_000 * HOUR;
        let w = window(2 * HOUR + 13 * MIN, now);
        for width in [452.0, 300.0, 180.0] {
            for state in [State::Open(&w), State::Idle, State::Unavailable] {
                let (shapes, h) = paint(state, width);
                assert!(shapes > 8 && h > 40.0, "{state:?} at {width}px: {shapes} shapes, {h}px");
            }
        }
    }

    #[test]
    fn an_open_window_is_taller_than_an_idle_one_and_draws_its_fine_print() {
        let now = 1_000 * HOUR;
        let w = window(HOUR, now);
        let (_, open_h) = paint(State::Open(&w), 452.0);
        let (_, idle_h) = paint(State::Idle, 452.0);
        let (_, unavailable_h) = paint(State::Unavailable, 452.0);
        assert!(open_h > idle_h, "the bar and its row add height ({open_h} vs {idle_h})");
        assert!(idle_h > unavailable_h, "an unavailable box has no fine print under it ({idle_h} vs {unavailable_h})");
    }
}
