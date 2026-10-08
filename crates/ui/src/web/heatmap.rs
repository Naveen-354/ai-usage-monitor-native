//! The activity heat map above "ALL AGENTS" on the Statistics page: one square per local day for the past year, one column
//! per week, like GitHub's contribution graph. The darker (more yellow) a square, the more tokens that day.
//!
//! The model (grid, levels, hit-testing, wording) is plain functions so it can be tested; [`show`] only paints it.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use egui::{vec2, Color32, Id, LayerId, Order, Pos2, Rect, Sense, Ui, Vec2};

use super::paint::{border, hard_shadow};
use super::text::{Run, Style, Weight};
use super::widgets::{h2_style, put};
use super::wrap::measure;
use super::{Tokens, BW};
use crate::motion::format::format_compact;
use crate::view::DayTotal;

/// Columns of a full year (GitHub draws 53: the 52 complete weeks plus the one that holds today).
pub const WEEKS: usize = 53;
/// How many days the backend is asked for.
pub const DAYS: u32 = (WEEKS * 7) as u32;

const CELL: f32 = 11.0;
const GAP: f32 = 3.0;
const PITCH: f32 = CELL + GAP;
/// Space between the box's border and its content.
const PAD: f32 = 10.0;
/// Width reserved at the left for the weekday names.
const LABEL_W: f32 = 34.0;
/// Height of the row of month names above the squares.
const MONTH_H: f32 = 16.0;
/// Never draw fewer weeks than this, however narrow the window.
const MIN_WEEKS: usize = 8;
const BAR_H: f32 = 23.5;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

// ------------------------------------------------------------------------------------------------ model

/// The squares: `weeks[column][row]` is the date of that square, `None` for days after today. `starts[column]` is the first
/// day of the column (the week's first day, per the "week starts on" setting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub starts: Vec<NaiveDate>,
    pub weeks: Vec<[Option<NaiveDate>; 7]>,
    /// (column, name) for the month names above the squares.
    pub months: Vec<(usize, &'static str)>,
}

/// The first day of the week that contains `date`; `week_start` is 0 = Sunday … 6 = Saturday.
pub fn week_start_of(date: NaiveDate, week_start: u8) -> NaiveDate {
    let back = (i64::from(date.weekday().num_days_from_sunday()) - i64::from(week_start % 7)).rem_euclid(7);
    date - Duration::days(back)
}

/// `weeks` columns ending with the week that holds `today`.
pub fn build_grid(today: NaiveDate, week_start: u8, weeks: usize) -> Grid {
    let weeks = weeks.max(1);
    let first = week_start_of(today, week_start) - Duration::days(7 * (weeks as i64 - 1));
    let starts: Vec<NaiveDate> = (0..weeks).map(|w| first + Duration::days(7 * w as i64)).collect();
    let columns = starts
        .iter()
        .map(|&start| {
            let mut col = [None; 7];
            for (row, cell) in col.iter_mut().enumerate() {
                let day = start + Duration::days(row as i64);
                *cell = (day <= today).then_some(day);
            }
            col
        })
        .collect();
    Grid { months: month_labels(&starts), starts, weeks: columns }
}

/// The column that holds the 1st of a month carries that month's name; the leftmost column names its own month too,
/// unless that name would sit on top of the next one.
fn month_labels(starts: &[NaiveDate]) -> Vec<(usize, &'static str)> {
    let mut out: Vec<(usize, &'static str)> = Vec::new();
    for (i, start) in starts.iter().enumerate() {
        if let Some(first) = (0..7).map(|k| *start + Duration::days(k)).find(|d| d.day() == 1) {
            out.push((i, MONTHS[first.month0() as usize]));
        }
    }
    if out.first().is_none_or(|(i, _)| *i >= 3) {
        if let Some(start) = starts.first() {
            out.insert(0, (0, MONTHS[start.month0() as usize]));
        }
    }
    out
}

/// Where the busy days start, as the quartiles of the days that had any usage: a day is at most level 1 up to the first
/// value, level 2 up to the second, and so on. Quartiles (not a linear scale) because token counts are very uneven: one
/// huge day would otherwise turn every other day pale.
pub fn thresholds(totals: &[u64]) -> [u64; 3] {
    let mut v: Vec<u64> = totals.iter().copied().filter(|&n| n > 0).collect();
    if v.is_empty() {
        return [0; 3];
    }
    v.sort_unstable();
    let at = |q: usize| v[(v.len() * q / 4).min(v.len() - 1)];
    [at(1), at(2), at(3)]
}

/// 0 = no usage, 1..=4 = quiet to busiest. The busiest day of the range is always 4.
pub fn level(total: u64, thr: &[u64; 3], max: u64) -> u8 {
    if total == 0 {
        0
    } else if total >= max || total > thr[2] {
        4
    } else if total > thr[1] {
        3
    } else if total > thr[0] {
        2
    } else {
        1
    }
}

/// How many week columns fit in a box `width` px wide (at most a year, at least [`MIN_WEEKS`]).
pub fn weeks_that_fit(width: f32) -> usize {
    let inner = width - 2.0 * (BW + PAD) - LABEL_W;
    (((inner + GAP) / PITCH).floor() as i64).clamp(MIN_WEEKS as i64, WEEKS as i64) as usize
}

/// Height of the box (border, padding, month names and seven rows of squares).
pub fn box_height() -> f32 {
    2.0 * (BW + PAD) + MONTH_H + 7.0 * PITCH - GAP
}

/// The square under `rel` (a point measured from the top-left of the first square), or `None` over a gap or outside.
pub fn cell_at(rel: Vec2, weeks: usize) -> Option<(usize, usize)> {
    if rel.x < 0.0 || rel.y < 0.0 {
        return None;
    }
    let (col, row) = ((rel.x / PITCH).floor() as usize, (rel.y / PITCH).floor() as usize);
    let inside = rel.x - col as f32 * PITCH <= CELL && rel.y - row as f32 * PITCH <= CELL;
    (inside && col < weeks && row < 7).then_some((col, row))
}

/// The line above the box on the right: what the squares add up to.
pub fn caption(total: u64, weeks: usize) -> String {
    let span = if weeks >= WEEKS { "in the last year".to_string() } else { format!("in the last {weeks} weeks") };
    if total == 0 {
        format!("No token usage {span}")
    } else {
        format!("{} tokens {span}", format_compact(total as f64, 3))
    }
}

/// What the hover box says about one square.
pub fn tooltip(date: NaiveDate, total: u64) -> String {
    let day = date.format("%a, %b %-d, %Y");
    if total == 0 {
        format!("No tokens · {day}")
    } else {
        format!("{} tokens · {day}", format_compact(total as f64, 3))
    }
}

/// `fg` over `bg` at opacity `a`, blended in sRGB the way CSS does and returned opaque. (egui premultiplies translucent
/// colours in linear light, which would make a 10 % tint come out as roughly 37 % - far brighter than the browser's.)
fn over(bg: Color32, fg: Color32, a: f32) -> Color32 {
    let ch = |b: u8, f: u8| (f32::from(b) + (f32::from(f) - f32::from(b)) * a).round() as u8;
    Color32::from_rgb(ch(bg.r(), fg.r()), ch(bg.g(), fg.g()), ch(bg.b(), fg.b()))
}

/// The colour of a square: `ink` at a whisper for no usage, then the accent colour in four strengths.
pub fn level_color(t: &Tokens, level: u8) -> Color32 {
    match level {
        0 => over(t.bg, t.ink, 0.10),
        1 => over(t.bg, t.accent, 0.30),
        2 => over(t.bg, t.accent, 0.55),
        3 => over(t.bg, t.accent, 0.80),
        _ => t.accent,
    }
}

/// The three weekday names shown beside the squares and the row each is on: Monday, Wednesday and Friday as on GitHub,
/// wherever the week starts.
pub fn weekday_labels(week_start: u8) -> [(usize, &'static str); 3] {
    [1usize, 3, 5].map(|weekday| ((weekday + 7 - usize::from(week_start % 7)) % 7, WEEKDAYS[weekday]))
}

// ------------------------------------------------------------------------------------------------ painting

pub struct Props<'a> {
    pub tokens: Tokens,
    /// Days with usage, ascending (see `Backend::daily_totals`); `None` when they could not be read.
    pub days: Option<&'a [DayTotal]>,
    /// Does any enabled agent have numbers at all? If not (and there are no days) the honest line is "unavailable", not 0.
    pub has_numbers: bool,
    pub today: NaiveDate,
    /// 0 = Sunday … 6 = Saturday (the "Week starts on" setting).
    pub week_start: u8,
}

fn small() -> Style {
    Style::mono(10.0, Weight::W400).lh_px(13.5)
}

/// Paints the section (heading, box of squares, legend) at the full available width.
pub fn show(ui: &mut Ui, p: &Props) {
    let t = p.tokens;
    let width = ui.available_width();
    let weeks = weeks_that_fit(width);
    let grid = build_grid(p.today, p.week_start, weeks);
    let first = grid.starts[0];

    let by_date: HashMap<NaiveDate, u64> = p
        .days
        .unwrap_or(&[])
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok().map(|date| (date, d.total)))
        .filter(|(date, _)| *date >= first && *date <= p.today)
        .collect();
    let values: Vec<u64> = by_date.values().copied().collect();
    let (thr, max) = (thresholds(&values), values.iter().copied().max().unwrap_or(0));
    let visible_total = values.iter().fold(0u64, |a, &b| a.saturating_add(b));
    let unavailable = p.days.is_none() || (by_date.is_empty() && !p.has_numbers);

    // ACTIVITY ......................................... 358M tokens in the last year
    let (bar, _) = ui.allocate_exact_size(vec2(width, BAR_H), Sense::hover());
    let h2 = h2_style();
    put(ui, "ACTIVITY", &h2, bar.left(), bar.top() + (BAR_H - h2.line_h) / 2.0, t.ink);
    let (text, weight, color) = if unavailable {
        (super::all_agents::UNAVAILABLE_TEXT.to_string(), Weight::W700, t.alert)
    } else {
        (caption(visible_total, weeks), Weight::W400, t.dim)
    };
    let st = Style::mono(11.0, weight);
    let run = Run::new(ui.ctx(), &text, &st);
    run.paint(ui.painter(), bar.right() - run.width, bar.top() + (BAR_H - st.line_h) / 2.0, color);
    ui.add_space(12.0);

    // the box
    let (rect, resp) = ui.allocate_exact_size(vec2(width, box_height()), Sense::hover());
    let origin = rect.min + vec2(BW + PAD, BW + PAD);
    let grid_origin = Pos2::new(origin.x + LABEL_W, origin.y + MONTH_H);
    {
        let painter = ui.painter();
        border(painter, rect, BW, t.ink);
    }
    let label = small();
    for (col, name) in &grid.months {
        put(ui, name, &label, grid_origin.x + *col as f32 * PITCH, origin.y, t.dim);
    }
    for (row, name) in weekday_labels(p.week_start) {
        put(ui, name, &label, origin.x, grid_origin.y + row as f32 * PITCH + (CELL - label.line_h) / 2.0, t.dim);
    }

    let hovered = resp.hover_pos().and_then(|pos| cell_at(pos - grid_origin, weeks)).and_then(|(c, r)| grid.weeks[c][r].map(|d| (c, r, d)));
    for (c, column) in grid.weeks.iter().enumerate() {
        for (r, day) in column.iter().enumerate() {
            let Some(date) = day else { continue };
            let total = by_date.get(date).copied().unwrap_or(0);
            let lvl = if unavailable { 0 } else { level(total, &thr, max) };
            let cell = Rect::from_min_size(grid_origin + vec2(c as f32 * PITCH, r as f32 * PITCH), vec2(CELL, CELL));
            let painter = ui.painter();
            painter.rect_filled(cell, 0.0, if unavailable { over(t.bg, t.ink, 0.06) } else { level_color(&t, lvl) });
            if *date == p.today || hovered.is_some_and(|(hc, hr, _)| (hc, hr) == (c, r)) {
                border(painter, cell, 1.0, t.ink);
            }
        }
    }

    // legend:                                                                Less ▫▪▪▪▪ More
    ui.add_space(6.0);
    let (legend, _) = ui.allocate_exact_size(vec2(width, label.line_h), Sense::hover());
    let (less, more) = (measure(&label, "Less"), measure(&label, "More"));
    let squares = 5.0 * CELL + 4.0 * GAP;
    let mut x = legend.right() - (less + 6.0 + squares + 6.0 + more);
    put(ui, "Less", &label, x, legend.top(), t.dim);
    x += less + 6.0;
    for lvl in 0..5u8 {
        let cell = Rect::from_min_size(Pos2::new(x + f32::from(lvl) * PITCH, legend.top() + (label.line_h - CELL) / 2.0), vec2(CELL, CELL));
        ui.painter().rect_filled(cell, 0.0, level_color(&t, lvl));
    }
    x += squares + 6.0;
    put(ui, "More", &label, x, legend.top(), t.dim);

    if let (Some((_, _, date)), Some(pointer)) = (hovered, resp.hover_pos()) {
        let total = by_date.get(&date).copied().unwrap_or(0);
        draw_tooltip(ui, &t, pointer, &if unavailable { date.format("%a, %b %-d, %Y").to_string() } else { tooltip(date, total) });
    }
}

/// A small box next to the pointer, in the design's own style (hard shadow, 2 px border), above everything else.
fn draw_tooltip(ui: &Ui, t: &Tokens, pointer: Pos2, text: &str) {
    let st = Style::mono(11.0, Weight::W700);
    let run = Run::new(ui.ctx(), text, &st);
    let size = vec2(run.width + 2.0 * (BW + 6.0), st.line_h + 2.0 * (BW + 3.0));
    let screen = ui.ctx().screen_rect();
    let mut pos = pointer + vec2(14.0, 18.0);
    if pos.x + size.x > screen.right() - 6.0 {
        pos.x = pointer.x - 14.0 - size.x;
    }
    if pos.y + size.y > screen.bottom() - 6.0 {
        pos.y = pointer.y - 14.0 - size.y;
    }
    let rect = Rect::from_min_size(pos, size);
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::new("activity-heatmap-tooltip")));
    hard_shadow(&painter, rect, 3.0, 3.0, t.ink);
    painter.rect_filled(rect, 0.0, t.bg);
    border(&painter, rect, BW, t.ink);
    run.paint(&painter, rect.left() + BW + 6.0, rect.top() + BW + 3.0, t.ink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Theme;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn the_grid_is_a_year_of_weeks_ending_with_the_week_that_holds_today() {
        let today = d(2026, 10, 8); // a Thursday
        let g = build_grid(today, 0, WEEKS);
        assert_eq!(g.weeks.len(), 53);
        assert_eq!(g.starts[52], d(2026, 10, 4), "the last column starts on that week's Sunday");
        assert_eq!(g.starts[0], d(2025, 10, 5));
        // Sunday..Thursday exist, Friday and Saturday are still in the future
        assert_eq!(g.weeks[52], [Some(d(2026, 10, 4)), Some(d(2026, 10, 5)), Some(d(2026, 10, 6)), Some(d(2026, 10, 7)), Some(d(2026, 10, 8)), None, None]);
    }

    #[test]
    fn the_week_start_setting_decides_the_first_row() {
        let today = d(2026, 10, 8);
        let monday = build_grid(today, 1, WEEKS);
        assert_eq!(monday.starts[52], d(2026, 10, 5));
        assert_eq!(monday.weeks[52][3], Some(d(2026, 10, 8)), "Thursday is the fourth row when weeks start on Monday");
        assert_eq!(monday.weeks[52][4], None);
        let saturday = build_grid(today, 6, WEEKS);
        assert_eq!(saturday.starts[52], d(2026, 10, 3));
    }

    #[test]
    fn every_day_appears_exactly_once_in_order() {
        let g = build_grid(d(2026, 3, 1), 1, WEEKS);
        let days: Vec<NaiveDate> = g.weeks.iter().flatten().flatten().copied().collect();
        assert_eq!(*days.last().unwrap(), d(2026, 3, 1));
        for w in days.windows(2) {
            assert_eq!(w[1] - w[0], Duration::days(1), "{:?}", w);
        }
        assert!(days.len() <= WEEKS * 7);
    }

    #[test]
    fn month_names_sit_over_the_week_that_holds_the_first() {
        let g = build_grid(d(2026, 10, 8), 0, WEEKS);
        // 2025-11-01 is a Saturday, in the week that starts Sunday 2025-10-26 = column 3
        assert!(g.months.contains(&(3, "Nov")), "{:?}", g.months);
        // 2026-10-01 is a Thursday, in the week that starts Sunday 2026-09-27 = column 51 (column 52 starts Oct 4)
        assert!(g.months.contains(&(51, "Oct")), "{:?}", g.months);
        let cols: Vec<usize> = g.months.iter().map(|(c, _)| *c).collect();
        assert!(cols.windows(2).all(|w| w[1] >= w[0] + 3), "labels must not overlap: {cols:?}");
        // the leftmost column names its own month when that does not collide with the next label
        assert_eq!(g.months[0], (0, "Oct"));
    }

    #[test]
    fn the_leftmost_month_name_is_dropped_when_the_next_one_is_right_beside_it() {
        // 8 weeks ending Thursday 2026-10-08, Sunday-first: the columns start Aug 16, 23, 30, Sep 6 ... Oct 4.
        // Sep 1 is in column 2 (week of Aug 30) and Oct 1 in column 6 (week of Sep 27); "Aug" at column 0 would run into "Sep".
        let g = build_grid(d(2026, 10, 8), 0, 8);
        assert_eq!(g.months, vec![(2, "Sep"), (6, "Oct")]);
    }

    #[test]
    fn levels_split_the_days_that_had_usage_into_quartiles() {
        let totals: Vec<u64> = (1..=8).map(|n| n * 100).collect(); // 100 ..= 800
        let thr = thresholds(&totals);
        assert_eq!(thr, [300, 500, 700]);
        let max = 800;
        let levels: Vec<u8> = totals.iter().map(|&n| level(n, &thr, max)).collect();
        assert_eq!(levels, [1, 1, 1, 2, 2, 3, 3, 4]);
        assert_eq!(level(0, &thr, max), 0);
    }

    #[test]
    fn one_huge_day_does_not_turn_every_other_day_pale() {
        let mut totals = vec![1_000u64; 20];
        totals.push(1_000_000_000);
        let thr = thresholds(&totals);
        let busiest = level(1_000_000_000, &thr, 1_000_000_000);
        assert_eq!(busiest, 4);
        assert!(totals.iter().filter(|&&n| n == 1_000).all(|&n| level(n, &thr, 1_000_000_000) <= 1));
        // and with only ordinary days, they spread over the scale instead of all being level 1
        let spread: Vec<u64> = (1..=40).map(|n| n * 10).collect();
        let thr = thresholds(&spread);
        let used: std::collections::BTreeSet<u8> = spread.iter().map(|&n| level(n, &thr, 400)).collect();
        assert_eq!(used.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn degenerate_inputs_still_give_sensible_levels() {
        assert_eq!(thresholds(&[]), [0, 0, 0]);
        assert_eq!(thresholds(&[0, 0]), [0, 0, 0]);
        assert_eq!(level(0, &[0, 0, 0], 0), 0);
        let thr = thresholds(&[50, 50, 50]);
        assert_eq!(level(50, &thr, 50), 4, "when every day is equal they are all the busiest");
        assert_eq!(level(7, &thresholds(&[7]), 7), 4, "a single day is the busiest day");
    }

    #[test]
    fn narrow_windows_show_fewer_weeks_but_never_fewer_than_eight() {
        assert_eq!(weeks_that_fit(812.0), 53, "the Statistics page at its usual width shows the whole year");
        assert!(weeks_that_fit(600.0) < 53 && weeks_that_fit(600.0) > 8);
        assert_eq!(weeks_that_fit(120.0), 8);
        let w = weeks_that_fit(700.0);
        let needed = 2.0 * (BW + PAD) + LABEL_W + w as f32 * PITCH - GAP;
        assert!(needed <= 700.0, "{w} weeks need {needed}px");
    }

    #[test]
    fn hit_testing_finds_squares_and_ignores_the_gaps() {
        assert_eq!(cell_at(vec2(0.0, 0.0), 53), Some((0, 0)));
        assert_eq!(cell_at(vec2(10.9, 10.9), 53), Some((0, 0)));
        assert_eq!(cell_at(vec2(12.0, 5.0), 53), None, "the gap between columns");
        assert_eq!(cell_at(vec2(5.0, 12.0), 53), None, "the gap between rows");
        assert_eq!(cell_at(vec2(PITCH * 3.0 + 2.0, PITCH * 6.0 + 2.0), 53), Some((3, 6)));
        assert_eq!(cell_at(vec2(PITCH * 7.0, 0.0), 7), None, "past the last column");
        assert_eq!(cell_at(vec2(0.0, PITCH * 7.0), 53), None, "past the last row");
        assert_eq!(cell_at(vec2(-1.0, 3.0), 53), None);
    }

    #[test]
    fn the_wording_is_plain_and_never_a_made_up_zero() {
        assert_eq!(caption(358_400_000, 53), "358M tokens in the last year");
        assert_eq!(caption(1_200, 20), "1.2K tokens in the last 20 weeks");
        assert_eq!(caption(0, 53), "No token usage in the last year");
        assert_eq!(tooltip(d(2026, 10, 6), 232_300_000), "232M tokens · Tue, Oct 6, 2026");
        assert_eq!(tooltip(d(2026, 10, 7), 0), "No tokens · Wed, Oct 7, 2026");
    }

    #[test]
    fn colours_get_stronger_with_the_level_on_both_themes() {
        for theme in [Theme::Dark, Theme::Light] {
            let t = Tokens::for_theme(theme);
            let distance = |l: u8| {
                let c = level_color(&t, l);
                (i32::from(c.r()) - i32::from(t.bg.r())).abs() + (i32::from(c.g()) - i32::from(t.bg.g())).abs() + (i32::from(c.b()) - i32::from(t.bg.b())).abs()
            };
            assert!(distance(0) > 0, "even an empty day is faintly visible");
            assert!((0..4).all(|l| distance(l) < distance(l + 1)), "{theme:?}: {:?}", (0..5).map(distance).collect::<Vec<_>>());
            assert_eq!(level_color(&t, 4), t.accent);
            assert_eq!(level_color(&t, 9), t.accent, "out-of-range levels clamp to the strongest");
            assert_eq!(level_color(&t, 2).a(), 255, "squares are opaque");
        }
    }

    #[test]
    fn tints_are_blended_like_css_not_like_egui_premultiplication() {
        // 10 % of ink (242, 242, 238) over the dark page (10, 10, 10): 33 - not the 94 egui's linear-light maths gives.
        let t = Tokens::for_theme(Theme::Dark);
        assert_eq!(level_color(&t, 0), Color32::from_rgb(33, 33, 33));
        assert_eq!(level_color(&t, 1), Color32::from_rgb(84, 76, 7));
    }

    #[test]
    fn the_weekday_names_are_monday_wednesday_friday_wherever_the_week_starts() {
        assert_eq!(weekday_labels(0), [(1, "Mon"), (3, "Wed"), (5, "Fri")]);
        assert_eq!(weekday_labels(1), [(0, "Mon"), (2, "Wed"), (4, "Fri")]);
        assert_eq!(weekday_labels(6), [(2, "Mon"), (4, "Wed"), (6, "Fri")]);
    }

    fn ctx() -> egui::Context {
        let c = egui::Context::default();
        crate::web::install_fonts(&c);
        let _ = c.run(egui::RawInput::default(), |_| {});
        c
    }

    fn paint(days: Option<&[DayTotal]>, has_numbers: bool, width: f32) -> usize {
        let ctx = ctx();
        let t = Tokens::for_theme(Theme::Dark);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_width(width);
                show(ui, &Props { tokens: t, days, has_numbers, today: d(2026, 10, 8), week_start: 1 });
            });
        });
        out.shapes.len()
    }

    #[test]
    fn it_paints_headlessly_with_data_without_data_and_when_unavailable() {
        let days = vec![
            DayTotal { date: "2026-10-08".into(), total: 5_000_000 },
            DayTotal { date: "2026-10-07".into(), total: 90_000 },
            DayTotal { date: "not-a-date".into(), total: 1 },
            DayTotal { date: "2020-01-01".into(), total: 99 },
        ];
        let with = paint(Some(&days), true, 812.0);
        let without = paint(Some(&[]), true, 812.0);
        let unavailable = paint(None, true, 812.0);
        assert!(with > 300 && without > 300 && unavailable > 300, "{with} {without} {unavailable}");
        assert!(paint(Some(&days), true, 300.0) > 30, "a very narrow window still paints");
        assert!(paint(Some(&[]), false, 812.0) > 300);
    }
}
