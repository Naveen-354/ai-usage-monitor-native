//! "ALL AGENTS": headline, input/output/cached split and the per-agent rows (`AllAgents.tsx` + `AgentRows.tsx`).
//! It fills the expanded overlay and the Statistics page.

use egui::{vec2, Pos2, Rect, Sense, Ui, Vec2};

use super::fmt;
use super::hero::is_live;
use super::paint::{border, dashed_border};
use super::text::{Run, Style, Weight};
use super::widgets::{agent_dot, btn, h2_style, h3, period_switch, put, tag_at, tag_size, usage_bar_at, Tone, BAR_H};
use super::wrap::{measure, wrap};
use super::{agent_color, Tokens, BW};
use crate::motion::format::format_compact;
use crate::view::{AgentOverview, Availability, Overview, PeriodKey, Settings, Totals};

pub const UNAVAILABLE_TEXT: &str = "TOKEN DATA UNAVAILABLE";

/// Short, honest label for why an agent has no numbers (`availabilityLabel`).
pub fn availability_label(a: &Availability) -> &'static str {
    match a {
        Availability::NotInstalled => "NOT INSTALLED",
        Availability::Unavailable { .. } => "UNAVAILABLE",
        Availability::Error { .. } => "COLLECTOR ERROR",
        Availability::NoDataYet => "NO USAGE YET",
        Availability::Ok => "OK",
    }
}

/// `availabilityDetail`: the reason, the error text, or the agent's own note.
pub fn availability_detail(a: &AgentOverview) -> Option<&str> {
    match &a.availability {
        Availability::Unavailable { reason } => Some(reason),
        Availability::Error { message } => Some(message),
        _ => a.note.as_deref(),
    }
}

/// Agents with numbers first (largest to smallest), then those without, in catalogue order; disabled agents are dropped.
pub fn sort_agents(agents: &[AgentOverview]) -> Vec<&AgentOverview> {
    let mut with: Vec<&AgentOverview> = agents.iter().filter(|a| a.enabled && a.totals.is_some()).collect();
    with.sort_by(|a, b| b.totals.map_or(0, |t| t.total).cmp(&a.totals.map_or(0, |t| t.total)));
    with.extend(agents.iter().filter(|a| a.enabled && a.totals.is_none()));
    with
}

pub fn max_total(agents: &[&AgentOverview]) -> u64 {
    agents.iter().filter_map(|a| a.totals.map(|t| t.total)).max().unwrap_or(0)
}

/// The cells of the INPUT / OUTPUT / CACHED / REQUESTS box: (label, value, tooltip-free text).
pub fn split_cells(t: &Totals, s: &Settings) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    if s.show_input {
        v.push(("INPUT", format_compact(t.input as f64, 3)));
    }
    if s.show_output {
        v.push(("OUTPUT", format_compact(t.output as f64, 3)));
    }
    if s.show_cached {
        v.push(("CACHED", format_compact((t.cache_read + t.cache_write) as f64, 3)));
    }
    v.push(("REQUESTS", format_compact(t.events as f64, 3)));
    v
}

/// `repeat(auto-fit, minmax(90px, 1fr))` over `inner_w` px: how many columns hold `n` cells.
pub fn split_columns(n: usize, inner_w: f32) -> usize {
    ((inner_w / 90.0).floor() as usize).clamp(1, n.max(1)).min(n.max(1))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Period(PeriodKey),
    OpenAgent(String),
}

pub struct Props<'a> {
    pub tokens: Tokens,
    pub overview: Option<&'a Overview>,
    pub settings: &'a Settings,
    pub now_ms: i64,
    pub time_ms: u64,
    /// The overlay shows the headline in its own hero; the Statistics page shows it here.
    pub hide_headline: bool,
}

/// Paints the section into the current `ui` (full available width) and returns what the user did.
pub fn show(ui: &mut Ui, p: &Props) -> Vec<Action> {
    let t = p.tokens;
    let mut actions = Vec::new();
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    let width = ui.available_width();

    // ALL AGENTS + period switch (the bar is 23.5 px high; the heading is centred in it)
    let (bar, _) = ui.allocate_exact_size(vec2(width, 23.5), Sense::hover());
    let h2s = h2_style();
    put(ui, "ALL AGENTS", &h2s, bar.left(), bar.top() + (23.5 - h2s.line_h) / 2.0, t.ink);
    let switch_rect = Rect::from_min_size(Pos2::new(bar.right() - 190.0, bar.top()), vec2(190.0, 23.5));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(switch_rect));
    if let Some(picked) = period_switch(&mut child, &t, p.settings.period, 190.0) {
        actions.push(Action::Period(picked));
    }
    // `.all__bar` has margin-bottom 8; the box below has margin-top 12. Adjacent vertical margins collapse to the larger.
    if !p.hide_headline {
        ui.add_space(8.0);
        headline(ui, &t, p.overview, p.time_ms);
    }

    let totals = p.overview.and_then(|o| o.totals);
    if let Some(tot) = &totals {
        ui.add_space(12.0);
        split_box(ui, &t, &split_cells(tot, p.settings));
    }
    if let (Some(o), Some(_)) = (p.overview, &totals) {
        if !o.count_cached_in_total {
            super::widgets::fineprint(ui, &t, "Total excludes cached tokens (Settings → Display).");
        }
    }

    h3(ui, &t, "AGENTS");
    let agents = p.overview.map(|o| sort_agents(&o.agents)).unwrap_or_default();
    let max = max_total(&agents);
    for (i, a) in agents.iter().enumerate() {
        if i > 0 {
            ui.add_space(10.0);
        }
        if let Some(id) = agent_row(ui, p, a, max) {
            actions.push(Action::OpenAgent(id));
        }
    }
    if p.overview.is_some() && agents.is_empty() {
        super::widgets::fineprint(ui, &t, "No agents enabled. Enable some in Settings → Monitoring.");
    }
    actions
}

/// The big number block of the Statistics page (`Headline.tsx`).
fn headline(ui: &mut Ui, t: &Tokens, overview: Option<&Overview>, time_ms: u64) {
    let _ = time_ms;
    let Some(o) = overview else {
        let st = Style::mono(52.0, Weight::W800).spacing_em(-0.04).lh(1.0);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
        put(ui, "···", &st, r.left(), r.top(), t.dim);
        return;
    };
    let label = period_label(o.period);
    let sub_style = Style::mono(10.0, Weight::W700).spacing_em(0.14).lh(1.35);
    match o.totals {
        None => {
            let st = Style::display(17.0).spacing_em(0.02).lh(1.1);
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), st.line_h), Sense::hover());
            put(ui, UNAVAILABLE_TEXT, &st, r.left(), r.top(), t.alert);
            ui.add_space(2.0 + 2.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), sub_style.line_h), Sense::hover());
            let w = put(ui, label, &sub_style, r.left(), r.top(), t.dim);
            let tag = if o.importing_agents.is_empty() { ("NO COLLECTOR REPORTING", Tone::Warn) } else { ("IMPORTING HISTORY", Tone::Accent) };
            tag_at(ui, t, Pos2::new(r.left() + w + 8.0, r.top() - 2.0), tag.0, tag.1);
        }
        Some(tot) => {
            let st = Style::mono(52.0, Weight::W800).spacing_em(-0.04).lh(1.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
            put(ui, &format_compact(tot.total as f64, 3), &st, r.left(), r.top(), t.ink);
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), sub_style.line_h), Sense::hover());
            let mut x = r.left();
            x += put(ui, "TOKENS", &sub_style, x, r.top(), t.dim) + 8.0;
            x += put(ui, label, &sub_style, x, r.top(), t.ink) + 8.0;
            if tot.estimated > 0 {
                x += tag_at(ui, t, Pos2::new(x, r.top() - 2.0), "INCL. EST", Tone::Warn).x + 8.0;
            }
            if !o.importing_agents.is_empty() {
                tag_at(ui, t, Pos2::new(x, r.top() - 2.0), "IMPORTING HISTORY", Tone::Accent);
            }
        }
    }
}

pub fn period_label(p: crate::view::PeriodOrCustom) -> &'static str {
    use crate::view::PeriodOrCustom::*;
    match p {
        Day => "TODAY",
        Week => "THIS WEEK",
        Month => "THIS MONTH",
        Year => "THIS YEAR",
        Custom => "CUSTOM RANGE",
    }
}

/// `.split`: the bordered grid of totals.
fn split_box(ui: &mut Ui, t: &Tokens, cells: &[(&'static str, String)]) {
    let w = ui.available_width();
    let inner = w - 2.0 * BW;
    let cols = split_columns(cells.len(), inner);
    let rows = cells.len().div_ceil(cols);
    let cell_h = 6.0 + 12.15 + 2.0 + 18.9 + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, rows as f32 * cell_h + 2.0 * BW), Sense::hover());
    let cell_w = inner / cols as f32;
    let dt = Style::mono(9.0, Weight::W700).spacing_em(0.14);
    let dd = Style::mono(14.0, Weight::W800);
    for (i, (label, value)) in cells.iter().enumerate() {
        let (c, r) = (i % cols, i / cols);
        let x = rect.left() + BW + c as f32 * cell_w;
        let y = rect.top() + BW + r as f32 * cell_h;
        put(ui, label, &dt, x + 8.0, y + 6.0, t.dim);
        put(ui, value, &dd, x + 8.0, y + 6.0 + 12.15 + 2.0, t.ink);
        if i + 1 < cells.len() {
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x + cell_w - BW, y), vec2(BW, cell_h)), 0.0, t.ink);
        }
    }
    border(ui.painter(), rect, BW, t.ink);
}

/// One `.drow`. Returns the agent id when its header is clicked.
fn agent_row(ui: &mut Ui, p: &Props, a: &AgentOverview, max: u64) -> Option<String> {
    let t = p.tokens;
    let na = a.totals.is_none();
    let color = agent_color(&a.color);
    let width = ui.available_width();
    let inner_w = width - 2.0 * BW - 18.0;
    let live = is_live(a.last_event_utc_ms, p.now_ms);
    let detail = availability_detail(a);
    let fg = if na { t.dim } else { t.ink };

    // Heights, top to bottom (children are 5 px apart)
    let note_style = Style::mono(10.0, Weight::W400).lh_px(13.5);
    let mut note_lines: Vec<String> = Vec::new();
    let mut note_color = t.dim;
    if let Some(tot) = &a.totals {
        let _ = tot;
        if matches!(a.availability, Availability::Error { .. }) {
            note_lines = wrap(&format!("STALE · {}", detail.unwrap_or("")), &note_style, inner_w, true);
            note_color = t.alert;
        } else if let Some(d) = detail {
            note_lines = wrap(d, &note_style, inner_w, true);
        }
    } else {
        let text = format!("{UNAVAILABLE_TEXT}{}", detail.map(|d| format!(" — {d}")).unwrap_or_default());
        note_lines = wrap(&text, &note_style, inner_w, true);
    }

    let split_lines = a.totals.as_ref().map(|tot| split_layout(ui.ctx(), tot, p.settings, a.last_event_utc_ms, p.now_ms, inner_w));
    let head_h = 18.9;
    let mut h = 7.0 + head_h;
    if let Some(sl) = &split_lines {
        h += 5.0 + BAR_H + 5.0 + sl.lines as f32 * 13.5 + (sl.lines.saturating_sub(1)) as f32 * 4.0;
    }
    if !note_lines.is_empty() {
        h += 5.0 + note_lines.len() as f32 * 13.5;
    }
    h += 8.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h + 2.0 * BW), Sense::hover());
    let top = rect.top() + BW + 7.0;
    let left = rect.left() + BW + 9.0;

    // head: dot, name, total / state
    let head = Rect::from_min_size(Pos2::new(left, top), vec2(inner_w, head_h));
    let head_resp = ui.interact(head, ui.id().with(("agent-head", a.id.as_str())), Sense::click());
    agent_dot(ui, Pos2::new(left, head.center().y - 4.5), color, na, live, p.time_ms, p.settings.animation_intensity);
    let name_style = Style::display(12.0).spacing_em(0.06);
    put(ui, &a.name.to_uppercase(), &name_style, left + 17.0, head.top() + (head_h - name_style.line_h) / 2.0, fg);
    if let Some(tot) = &a.totals {
        let st = Style::mono(14.0, Weight::W800);
        let text = format_compact(tot.total as f64, 4);
        let run = Run::new(ui.ctx(), &text, &st);
        run.paint(ui.painter(), head.right() - run.width, head.top(), t.ink);
    } else {
        let st = Style::mono(10.0, Weight::W700).spacing_em(0.1).lh_px(13.5);
        let text = availability_label(&a.availability);
        let run = Run::new(ui.ctx(), text, &st);
        run.paint(ui.painter(), head.right() - run.width, head.top() + (head_h - 13.5) / 2.0, t.dim);
    }

    let mut y = head.bottom();
    if let Some(sl) = &split_lines {
        y += 5.0;
        usage_bar_at(ui.painter(), &t, left, y, inner_w, if max > 0 { a.totals.map_or(0.0, |x| x.total as f32 / max as f32) } else { 0.0 }, color);
        y += BAR_H + 5.0;
        paint_split(ui, &t, sl, left, y, inner_w);
        y += sl.lines as f32 * 13.5 + sl.lines.saturating_sub(1) as f32 * 4.0;
    }
    if !note_lines.is_empty() {
        y += 5.0;
        for (i, l) in note_lines.iter().enumerate() {
            if na && i == 0 {
                // `<b>TOKEN DATA UNAVAILABLE</b>` is bold, the reason after it is not
                let (bold, rest) = l.split_at(UNAVAILABLE_TEXT.len().min(l.len()));
                let w = put(ui, bold, &Style::mono(10.0, Weight::W700).lh_px(13.5), left, y, note_color);
                put(ui, rest, &note_style, left + w, y, note_color);
            } else {
                put(ui, l, &note_style, left, y + i as f32 * 13.5, note_color);
            }
        }
    }

    if na {
        dashed_border(ui.painter(), rect, BW, t.ink);
    } else {
        border(ui.painter(), rect, BW, t.ink);
    }
    head_resp.clicked().then(|| a.id.clone())
}

/// The second line of a row: `IN 1.2M  OUT 2.1M  CACHE 229M  REQ 120  [EST]  ........ 5s ago`, wrapped.
pub struct SplitLayout {
    pub items: Vec<SplitItem>,
    pub lines: usize,
}

pub enum SplitItem {
    Pair { label: &'static str, value: String, x: f32, line: usize, label_w: f32 },
    Est { x: f32, line: usize },
    Seen { text: String, x: f32, line: usize },
}

pub fn split_layout(ctx: &egui::Context, t: &Totals, s: &Settings, last_event: Option<i64>, now_ms: i64, width: f32) -> SplitLayout {
    let dim = Style::mono(10.0, Weight::W400).lh_px(13.5);
    let bold = Style::mono(10.0, Weight::W700).lh_px(13.5);
    let _ = ctx;
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    if s.show_input {
        pairs.push(("IN", format_compact(t.input as f64, 3)));
    }
    if s.show_output {
        pairs.push(("OUT", format_compact(t.output as f64, 3)));
    }
    if s.show_cached {
        pairs.push(("CACHE", format_compact((t.cache_read + t.cache_write) as f64, 3)));
    }
    pairs.push(("REQ", format_compact(t.events as f64, 3)));

    let gap = 12.0;
    let mut items = Vec::new();
    let (mut x, mut line) = (0.0_f32, 0usize);
    let place = |w: f32, x: &mut f32, line: &mut usize| {
        if *x > 0.0 && *x + w > width {
            *x = 0.0;
            *line += 1;
        }
        let at = *x;
        *x += w + gap;
        at
    };
    for (label, value) in pairs {
        let label_w = measure(&dim, &format!("{label} "));
        let w = label_w + measure(&bold, &value);
        let at = place(w, &mut x, &mut line);
        items.push(SplitItem::Pair { label, value, x: at, line, label_w });
    }
    if t.estimated > 0 {
        let w = tag_size(ctx, "EST").x;
        let at = place(w, &mut x, &mut line);
        items.push(SplitItem::Est { x: at, line });
    }
    if let Some(ms) = last_event {
        let text = fmt::relative(ms, now_ms);
        let w = measure(&dim, &text);
        if x > 0.0 && x + w > width {
            line += 1;
        }
        items.push(SplitItem::Seen { text, x: width - w, line });
    }
    SplitLayout { items, lines: line + 1 }
}

fn paint_split(ui: &Ui, t: &Tokens, sl: &SplitLayout, left: f32, top: f32, _width: f32) {
    let dim = Style::mono(10.0, Weight::W400).lh_px(13.5);
    let bold = Style::mono(10.0, Weight::W700).lh_px(13.5);
    for item in &sl.items {
        match item {
            SplitItem::Pair { label, value, x, line, label_w } => {
                let y = top + *line as f32 * (13.5 + 4.0);
                put(ui, &format!("{label} "), &dim, left + x, y, t.dim);
                put(ui, value, &bold, left + x + label_w, y, t.ink);
            }
            SplitItem::Est { x, line } => {
                tag_at(ui, t, Pos2::new(left + x, top + *line as f32 * 17.5 - 2.25), "EST", Tone::Warn);
            }
            SplitItem::Seen { text, x, line } => {
                put(ui, text, &dim, left + x, top + *line as f32 * 17.5, t.dim);
            }
        }
    }
}

/// The two action buttons under the list in the overlay (`overlay__actions`): returns which was pressed.
pub fn overlay_actions(ui: &mut Ui, t: &Tokens) -> Option<&'static str> {
    ui.add_space(14.0);
    let mut out = None;
    let start = ui.cursor().min;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(start, vec2(ui.available_width(), 28.85))));
    child.spacing_mut().item_spacing = vec2(10.0, 0.0);
    child.horizontal(|ui| {
        if btn(ui, t, "STATISTICS", false).clicked() {
            out = Some("statistics");
        }
        if btn(ui, t, "SETTINGS", false).clicked() {
            out = Some("settings");
        }
    });
    ui.add_space(28.85);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    #[test]
    fn agents_sort_by_total_with_unavailable_ones_last_and_disabled_ones_dropped() {
        let mut o = mock::overview();
        o.agents.swap(0, 2); // gemini first in the catalogue
        let sorted = sort_agents(&o.agents);
        let ids: Vec<&str> = sorted.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "gemini", "antigravity", "ollama"]);
        o.agents[1].enabled = false;
        assert!(sort_agents(&o.agents).iter().all(|a| a.enabled));
        assert_eq!(max_total(&sort_agents(&mock::overview().agents)), 232_300_000);
    }

    #[test]
    fn the_split_box_honours_the_show_settings_and_always_has_requests() {
        let o = mock::overview();
        let t = o.totals.unwrap();
        let mut s = mock::settings();
        let labels = |s: &Settings| split_cells(&t, s).into_iter().map(|c| c.0).collect::<Vec<_>>();
        assert_eq!(labels(&s), ["INPUT", "OUTPUT", "CACHED", "REQUESTS"]);
        s.show_input = false;
        s.show_cached = false;
        assert_eq!(labels(&s), ["OUTPUT", "REQUESTS"]);
        let cells = split_cells(&t, &mock::settings());
        assert_eq!(cells[0].1, "8.1M");
        assert_eq!(cells[2].1, "244M", "cached is read + write");
        assert_eq!(cells[3].1, "480");
    }

    #[test]
    fn the_split_grid_has_as_many_columns_as_fit_and_never_empty_ones() {
        // Measured: 4 cells in 448 px -> 4 columns of 112.
        assert_eq!(split_columns(4, 448.0), 4);
        assert_eq!(split_columns(3, 448.0), 3, "auto-fit collapses empty tracks");
        assert_eq!(split_columns(4, 200.0), 2, "only two 90 px tracks fit");
        assert_eq!(split_columns(1, 10.0), 1);
        assert_eq!(split_columns(0, 448.0), 1);
    }

    #[test]
    fn availability_wording_is_the_old_apps() {
        assert_eq!(availability_label(&Availability::NotInstalled), "NOT INSTALLED");
        assert_eq!(availability_label(&Availability::Unavailable { reason: "x".into() }), "UNAVAILABLE");
        assert_eq!(availability_label(&Availability::Error { message: "x".into() }), "COLLECTOR ERROR");
        assert_eq!(availability_label(&Availability::NoDataYet), "NO USAGE YET");
        let o = mock::overview();
        assert_eq!(availability_detail(&o.agents[4]), Some("no local token ledger"));
        assert_eq!(availability_detail(&o.agents[0]), None);
    }

    #[test]
    fn a_rows_second_line_wraps_and_pushes_the_age_to_the_right_edge() {
        let ctx = egui::Context::default();
        crate::web::install_fonts(&ctx);
        let o = mock::overview();
        let tot = o.agents[0].totals.unwrap();
        let s = mock::settings();
        let wide = split_layout(&ctx, &tot, &s, Some(mock::NOW_MS - 5_000), mock::NOW_MS, 430.0);
        assert_eq!(wide.lines, 1);
        match wide.items.last() {
            Some(SplitItem::Seen { text, x, .. }) => {
                assert_eq!(text, "5s ago");
                assert!((x + 6.0 * 6.0 - 430.0).abs() < 0.1, "right-aligned: {x}");
            }
            _ => panic!("the age is last"),
        }
        let narrow = split_layout(&ctx, &tot, &s, Some(mock::NOW_MS - 5_000), mock::NOW_MS, 140.0);
        assert!(narrow.lines > 1, "{} lines", narrow.lines);
        // an agent that has never been seen shows no age
        let never = split_layout(&ctx, &tot, &s, None, mock::NOW_MS, 430.0);
        assert!(never.items.iter().all(|i| !matches!(i, SplitItem::Seen { .. })));
    }

    #[test]
    fn the_panel_paints_for_every_state_without_panicking() {
        let ctx = egui::Context::default();
        crate::web::install_fonts(&ctx);
        let s = mock::settings();
        let mut o = mock::overview();
        for overview in [Some(&o), None] {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let acts = show(ui, &Props { tokens: Tokens::for_theme(crate::view::Theme::Dark), overview, settings: &s, now_ms: mock::NOW_MS, time_ms: 0, hide_headline: false });
                    assert!(acts.is_empty());
                });
            });
            assert!(!out.shapes.is_empty());
        }
        o.totals = None;
        o.count_cached_in_total = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, &Props { tokens: Tokens::for_theme(crate::view::Theme::Light), overview: Some(&o), settings: &s, now_ms: mock::NOW_MS, time_ms: 0, hide_headline: false });
            });
        });
    }
}
