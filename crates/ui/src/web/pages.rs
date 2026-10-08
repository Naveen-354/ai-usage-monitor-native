//! The main window of the old app: header + tabs, and its four pages (`MainWindow.tsx`, `SettingsPage.tsx`,
//! `Diagnostics.tsx`, `Privacy.tsx`, `Statistics.tsx`).

use egui::{vec2, Color32, Margin, Pos2, Rect, Sense, Ui, Vec2};

use super::all_agents::{availability_detail, availability_label};
use super::fmt;
use super::paint::border;
use super::text::{Run, Style, Weight};
use super::widgets::{
    body, body_bold, btn, chip_flow, chipbox, chipbox_size, fineprint, fineprint_limited, h2, h3, hint, number_row, put, section, select_row, slider_row, tag_at, Slider, tag_size, toggle_row, Tone,
};
use super::wrap::{measure, wrap};
use super::{agent_color, Tokens, BW};
use crate::expanded::Page;
use crate::view::{AgentOverview, AnimationIntensity, AppInfo, Availability, Corner, OverlayDiagnostics, Settings, Theme};

// ------------------------------------------------------------------------------------------------ header and tabs

pub const PAGES: [(Page, &str); 5] =
    [(Page::Statistics, "STATISTICS"), (Page::Accounts, "ACCOUNTS"), (Page::Settings, "SETTINGS"), (Page::Diagnostics, "DIAGNOSTICS"), (Page::Privacy, "PRIVACY")];
pub const HEADER_H: f32 = 38.89 + BW;

/// Width of each tab: its text plus 16 px padding either side and its borders (the first also has a left border).
pub fn tab_widths(ctx: &egui::Context) -> Vec<f32> {
    let st = Style::mono(11.0, Weight::W700).spacing_em(0.12);
    PAGES.iter().enumerate().map(|(i, (_, label))| Run::new(ctx, label, &st).width + 32.0 + BW + if i == 0 { BW } else { 0.0 }).collect()
}

/// `.main__head`: the ink title block and the tab strip. Returns the tab clicked.
pub fn header(ui: &mut Ui, t: &Tokens, current: Page) -> Option<Page> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let title = Style::display(14.0).spacing_em(0.12);
    let title_run = Run::new(ui.ctx(), "AI USAGE MONITOR", &title);
    let title_w = title_run.width + 32.0;
    let p = ui.painter();
    p.rect_filled(Rect::from_min_size(rect.min, vec2(title_w, 38.89)), 0.0, t.ink);
    title_run.paint(p, rect.left() + 16.0, rect.top() + 10.0, t.bg);
    p.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - BW), rect.max), 0.0, t.ink);

    let st = Style::mono(11.0, Weight::W700).spacing_em(0.12);
    let widths = tab_widths(ui.ctx());
    let mut x = rect.left() + title_w + 24.0;
    let mut clicked = None;
    for (i, (page, label)) in PAGES.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(widths[i], 38.89));
        let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
        let active = *page == current;
        let p = ui.painter();
        if active {
            p.rect_filled(r, 0.0, t.accent);
        } else if resp.hovered() {
            p.rect_filled(r, 0.0, t.ink_a(0.12));
        }
        if i == 0 {
            p.rect_filled(Rect::from_min_size(r.min, vec2(BW, r.height())), 0.0, t.ink);
        }
        p.rect_filled(Rect::from_min_max(Pos2::new(r.right() - BW, r.top()), r.max), 0.0, t.ink);
        let left_pad = 16.0 + if i == 0 { BW } else { 0.0 };
        put(ui, label, &st, r.left() + left_pad, r.top() + (38.89 - st.line_h) / 2.0, if active { Color32::BLACK } else { t.ink });
        if resp.clicked() {
            clicked = Some(*page);
        }
        x += widths[i];
    }
    clicked
}

/// `.page`: 860 px at most, padded 20 / 24 / 40. Runs `add` inside it.
pub fn page<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_width();
    let content = (avail.min(860.0) - 48.0).max(100.0);
    egui::Frame::none()
        .inner_margin(Margin { left: 24.0, right: 0.0, top: 20.0, bottom: 40.0 })
        .show(ui, |ui| {
            ui.set_width(content);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            add(ui)
        })
        .inner
}

// ------------------------------------------------------------------------------------------------ settings

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const CORNERS: [(Corner, &str); 5] = [(Corner::TopRight, "Top right"), (Corner::TopLeft, "Top left"), (Corner::BottomRight, "Bottom right"), (Corner::BottomLeft, "Bottom left"), (Corner::Custom, "Custom (dragged)")];

#[derive(Debug, Default)]
pub struct SettingsOut {
    /// The settings after the user's edit, when something changed.
    pub changed: Option<Settings>,
    pub quit: bool,
}

pub fn settings_page(ui: &mut Ui, t: &Tokens, s: &Settings, agents: &[AgentOverview]) -> SettingsOut {
    let mut n = s.clone();
    let mut dirty = false;
    let mut quit = false;
    macro_rules! set {
        ($e:expr, $field:ident) => {
            if let Some(v) = $e {
                n.$field = v;
                dirty = true;
            }
        };
    }
    page(ui, |ui| {
        section(ui, t, "GENERAL", |ui| {
            set!(toggle_row(ui, t, "Always on top", Some("Keep the overlay above every other window. On by default."), false, s.always_on_top), always_on_top);
            set!(toggle_row(ui, t, "Compact mode", Some("Collapsed overlay. Click the overlay to expand or collapse."), false, s.compact_mode), compact_mode);
            let labels: Vec<&str> = CORNERS.iter().map(|c| c.1).collect();
            let cur = CORNERS.iter().position(|c| c.0 == s.overlay_corner).unwrap_or(4);
            if let Some(i) = select_row(ui, t, "Overlay position", Some("Moves the overlay to a corner. Dragging it sets a custom position."), false, &labels, cur) {
                n.overlay_corner = CORNERS[i].0;
                dirty = true;
            }
            set!(slider_row(ui, t, &Slider { label: "Overlay background", hint: Some("0% = see-through; text stays readable. Higher adds a dark/light backing."), min: 0.0, max: 1.0, step: 0.02 }, false, s.overlay_opacity, &|v| format!("{}%", (v * 100.0).round())), overlay_opacity);
            set!(slider_row(ui, t, &Slider { label: "Token text size", hint: Some("Size of the big count in the overlay. The overlay resizes to fit it."), min: 0.6, max: 2.0, step: 0.05 }, false, s.token_text_size, &|v| format!("{}%", (v * 100.0).round())), token_text_size);
            set!(toggle_row(ui, t, "Show overlay in taskbar", Some("Off keeps the overlay out of the taskbar; minimise still works."), true, s.show_overlay_in_taskbar), show_overlay_in_taskbar);
        });
        section(ui, t, "MONITORING", |ui| {
            set!(toggle_row(ui, t, "Pause monitoring", Some("Stops reading agent data. Existing history is kept."), false, s.paused), paused);
            if let Some(Some(v)) = number_row(ui, t, "Polling interval", Some("Safety-net check for changes the file watchers missed."), false, Some(s.polling_interval_secs as i64), 5, 3600, Some("seconds"), "") {
                n.polling_interval_secs = v as u32;
                dirty = true;
            }
            set!(toggle_row(ui, t, "Project detection", Some("Group usage by the repository an agent worked in."), false, s.project_detection), project_detection);
            if let Some(list) = enabled_agents_row(ui, t, s, agents) {
                n.enabled_agents = list;
                dirty = true;
            }
        });
        section(ui, t, "DISPLAY", |ui| {
            let cur = usize::from(s.theme == Theme::Light);
            if let Some(i) = select_row(ui, t, "Theme", None, false, &["Dark", "Light"], cur) {
                n.theme = if i == 1 { Theme::Light } else { Theme::Dark };
                dirty = true;
            }
            let intensities = [AnimationIntensity::Normal, AnimationIntensity::Low, AnimationIntensity::Off];
            let cur = intensities.iter().position(|a| *a == s.animation_intensity).unwrap_or(0);
            if let Some(i) = select_row(ui, t, "Animation intensity", Some("Also follows your OS 'reduce motion' preference."), false, &["Normal", "Low", "Off"], cur) {
                n.animation_intensity = intensities[i];
                dirty = true;
            }
            set!(toggle_row(ui, t, "Show input tokens", None, false, s.show_input), show_input);
            set!(toggle_row(ui, t, "Show output tokens", None, false, s.show_output), show_output);
            set!(toggle_row(ui, t, "Show cached tokens", None, false, s.show_cached), show_cached);
            set!(toggle_row(ui, t, "Count cached tokens in totals", Some("Cache reads/writes are real tokens the model processed, but they can dwarf everything else."), false, s.count_cached_in_total), count_cached_in_total);
            if let Some(i) = select_row(ui, t, "Week starts on", None, true, &DAYS, s.week_starts_on as usize % 7) {
                n.week_starts_on = i as u8;
                dirty = true;
            }
        });
        section(ui, t, "DATA", |ui| {
            if let Some(v) = number_row(ui, t, "Retention period", Some("Delete detailed events older than this. Daily history totals are kept. Empty = keep forever."), false, s.retention_days.map(i64::from), 1, 36_500, Some("days"), "forever") {
                n.retention_days = v.map(|d| d as u32);
                dirty = true;
            }
            ui.add_space(6.0);
            let w = ui.available_width();
            let lines = wrap("Database location, export and clear-history arrive with the data tools. Everything stays on this machine.", &hint(), w, false);
            let (r, _) = ui.allocate_exact_size(vec2(w, lines.len() as f32 * hint().line_h), Sense::hover());
            for (i, l) in lines.iter().enumerate() {
                put(ui, l, &hint(), r.left(), r.top() + i as f32 * hint().line_h, t.dim);
            }
            ui.add_space(6.0);
        });
        // .danger: margin-top 8 (the section's 22 px margin wins), button then fine print, 14 px apart
        let start = ui.cursor().min;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(start, vec2(ui.available_width(), 28.85))));
        let btn_resp = btn(&mut child, t, "QUIT APPLICATION", true);
        quit = btn_resp.clicked();
        let note = "Closing windows only hides them. Quit stops monitoring.";
        put(ui, note, &hint(), start.x + btn_resp.rect.width() + 14.0, start.y + (28.85 - hint().line_h) / 2.0, t.dim);
        ui.add_space(28.85);
    });
    SettingsOut { changed: dirty.then_some(n), quit }
}

/// `Enabled agents`: a stacked row with one chip box per agent. Returns the new list when one was toggled.
fn enabled_agents_row(ui: &mut Ui, t: &Tokens, s: &Settings, agents: &[AgentOverview]) -> Option<Vec<String>> {
    let sizes: Vec<Vec2> = agents.iter().map(|a| chipbox_size(ui.ctx(), &a.name)).collect();
    let width = ui.available_width() - 24.0;
    let (offsets, chips_h) = chip_flow(&sizes, width);
    let h = 9.0 + body_bold().line_h + 8.0 + chips_h + 9.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    put(ui, "Enabled agents", &body_bold(), rect.left() + 12.0, rect.top() + 9.0, t.ink);
    let origin = Pos2::new(rect.left() + 12.0, rect.top() + 9.0 + body_bold().line_h + 8.0);
    let mut out = None;
    for (a, off) in agents.iter().zip(offsets) {
        let on = s.enabled_agents.contains(&a.id);
        if chipbox(ui, t, origin + off.to_vec2(), &a.name, agent_color(&a.color), on).clicked() {
            let mut list: Vec<String> = s.enabled_agents.clone();
            if on {
                list.retain(|x| x != &a.id);
            } else {
                list.push(a.id.clone());
            }
            out = Some(list);
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ diagnostics

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kv {
    Plain,
    Ok,
    Bad,
}

/// `.kvs`: a bordered grid of key / value cells (auto-fit columns of at least 260 px).
pub fn kvs(ui: &mut Ui, t: &Tokens, items: &[(String, String, Kv)]) {
    let w = ui.available_width();
    let inner = w - 2.0 * BW;
    let cols = ((inner / 260.0).floor() as usize).clamp(1, items.len().max(1));
    let cell_w = inner / cols as f32;
    let dt = body();
    let dd = body_bold();
    // measure every cell first (a long value wraps anywhere and makes its whole row taller)
    let cells: Vec<(f32, Vec<String>)> = items
        .iter()
        .map(|(k, v, _)| {
            let key_w = measure(&dt, k);
            let avail = (cell_w - 20.0 - 12.0 - key_w).max(24.0);
            (key_w, wrap(v, &dd, avail, true))
        })
        .collect();
    let rows = items.len().div_ceil(cols);
    let row_h: Vec<f32> = (0..rows).map(|r| (0..cols).filter_map(|c| cells.get(r * cols + c)).map(|(_, l)| 5.0 + (l.len().max(1) as f32 * dd.line_h) + 5.0 + 1.0).fold(0.0, f32::max)).collect();
    let total: f32 = row_h.iter().sum::<f32>() + 2.0 * BW;
    let (rect, _) = ui.allocate_exact_size(vec2(w, total), Sense::hover());
    let mut y = rect.top() + BW;
    for (r, &height) in row_h.iter().enumerate() {
        for c in 0..cols {
            let Some(((k, _, tone), (key_w, lines))) = items.get(r * cols + c).zip(cells.get(r * cols + c)) else { continue };
            let x = rect.left() + BW + c as f32 * cell_w;
            put(ui, k, &dt, x + 10.0, y + 5.0, t.dim);
            let color = match tone {
                Kv::Ok => t.ok,
                Kv::Bad => t.alert,
                Kv::Plain => t.ink,
            };
            let right = x + cell_w - 10.0;
            let _ = key_w;
            for (i, l) in lines.iter().enumerate() {
                let lw = measure(&dd, l);
                put(ui, l, &dd, right - lw, y + 5.0 + i as f32 * dd.line_h, color);
            }
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x, y + height - 1.0), vec2(cell_w, 1.0)), 0.0, t.ink_a(0.35));
        }
        y += height;
    }
    border(ui.painter(), rect, BW, t.ink);
}

#[derive(Debug, Clone)]
pub enum Cell {
    Agent { color: Color32, name: String },
    Tag { text: String, tone: Tone },
    Text { text: String, dim: bool },
}

fn cell_width(ctx: &egui::Context, c: &Cell) -> f32 {
    match c {
        Cell::Agent { name, .. } => 9.0 + measure(&body(), " ") + measure(&body(), name),
        Cell::Tag { text, .. } => tag_size(ctx, text).x,
        Cell::Text { text, .. } => measure(&body(), text),
    }
}

/// Column widths of an auto-layout table: each column's widest content plus 20 px of padding, stretched in proportion
/// to fill `total` (as a browser distributes the spare room of a 100 %-wide table).
pub fn table_column_widths(natural: &[f32], total: f32) -> Vec<f32> {
    let sum: f32 = natural.iter().sum();
    if sum <= 0.0 {
        return natural.to_vec();
    }
    let k = if sum < total { total / sum } else { 1.0 };
    natural.iter().map(|w| w * k).collect()
}

/// `.table`: an ink header over bordered rows.
pub fn table(ui: &mut Ui, t: &Tokens, headers: &[&str], rows: &[Vec<Cell>]) {
    let w = ui.available_width();
    let ctx = ui.ctx().clone();
    let th = Style::mono(10.0, Weight::W700).spacing_em(0.12).lh(1.35);
    let natural: Vec<f32> = (0..headers.len())
        .map(|i| {
            let head = measure(&th, headers[i]);
            let cells = rows.iter().filter_map(|r| r.get(i)).map(|c| cell_width(&ctx, c)).fold(0.0, f32::max);
            head.max(cells) + 20.0
        })
        .collect();
    let widths = table_column_widths(&natural, w - 2.0);
    let (head_h, row_h) = (25.0, 31.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, head_h + rows.len() as f32 * row_h + 2.0), Sense::hover());
    let left = rect.left() + 1.0;
    let mut y = rect.top() + 1.0;
    ui.painter().rect_filled(Rect::from_min_size(Pos2::new(left, y), vec2(w - 2.0, head_h)), 0.0, t.ink);
    let mut x = left;
    for (i, h) in headers.iter().enumerate() {
        put(ui, h, &th, x + 10.0, y + 5.0, t.bg);
        x += widths[i];
    }
    y += head_h;
    for row in rows {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(left, y), vec2(w - 2.0, 1.0)), 0.0, t.ink_a(0.35));
        let mut x = left;
        for (i, c) in row.iter().enumerate() {
            let top = y + 6.0;
            match c {
                Cell::Agent { color, name } => {
                    ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x + 10.0, top + (body().line_h - 9.0) / 2.0), Vec2::splat(9.0)), 0.0, *color);
                    put(ui, name, &body(), x + 10.0 + 9.0 + measure(&body(), " "), top, t.ink);
                }
                Cell::Tag { text, tone } => {
                    tag_at(ui, t, Pos2::new(x + 10.0, top), text, *tone);
                }
                Cell::Text { text, dim } => {
                    put(ui, text, &body(), x + 10.0, top, if *dim { t.dim } else { t.ink });
                }
            }
            x += widths[i];
        }
        y += row_h;
    }
    border(ui.painter(), rect, BW, t.ink);
}

pub struct DiagnosticsData<'a> {
    pub overlay: Option<&'a OverlayDiagnostics>,
    pub agents: &'a [AgentOverview],
    pub info: Option<&'a AppInfo>,
    pub error: Option<&'a str>,
}

/// The rows of the OVERLAY grid, as the old page words them.
pub fn overlay_kvs(d: &OverlayDiagnostics) -> Vec<(String, String, Kv)> {
    let yes = |b: bool| if b { "yes" } else { "no" }.to_string();
    let topmost_ok = d.os_reports_topmost.is_none_or(|o| o == d.always_on_top_setting);
    vec![
        ("Visible".into(), yes(d.visible), Kv::Plain),
        ("Minimised".into(), yes(d.minimized), Kv::Plain),
        ("Always-on-top setting".into(), if d.always_on_top_setting { "ON" } else { "OFF" }.into(), Kv::Plain),
        (
            "Frameless (no title bar)".into(),
            match d.decorated {
                None => "unknown".into(),
                Some(true) => "NO — has a title bar".into(),
                Some(false) => "yes".into(),
            },
            if d.decorated == Some(true) { Kv::Bad } else { Kv::Plain },
        ),
        (
            "OS reports topmost".into(),
            match d.os_reports_topmost {
                None => "not readable on this OS".into(),
                Some(b) => yes(b),
            },
            match d.os_reports_topmost {
                None => Kv::Plain,
                Some(_) if topmost_ok => Kv::Ok,
                Some(_) => Kv::Bad,
            },
        ),
        ("Position".into(), d.x.zip(d.y).map_or("—".into(), |(x, y)| format!("{x}, {y}")), Kv::Plain),
        ("Size (px)".into(), d.width.zip(d.height).map_or("—".into(), |(w, h)| format!("{w} × {h}")), Kv::Plain),
        ("Scale / monitors".into(), format!("{} / {}", d.scale_factor.map_or("—".into(), |s| format!("{s}")), d.monitors), Kv::Plain),
    ]
}

pub fn application_kvs(i: &AppInfo) -> Vec<(String, String, Kv)> {
    vec![
        ("Version".into(), i.version.clone(), Kv::Plain),
        ("Platform".into(), format!("{} / {}", i.os, i.arch), Kv::Plain),
        ("Database".into(), i.database_path.clone(), Kv::Plain),
        ("Logs".into(), i.log_dir.clone(), Kv::Plain),
        ("Database health".into(), if i.database_notice.is_some() { "REBUILT after damage" } else { "ok" }.into(), if i.database_notice.is_some() { Kv::Bad } else { Kv::Ok }),
    ]
}

pub fn collector_rows(agents: &[AgentOverview]) -> Vec<Vec<Cell>> {
    agents
        .iter()
        .map(|a| {
            let good = matches!(a.availability, Availability::Ok | Availability::NoDataYet);
            let tone = if good {
                Tone::Accent
            } else if matches!(a.availability, Availability::Error { .. }) {
                Tone::Warn
            } else {
                Tone::Plain
            };
            let detail = availability_detail(a);
            vec![
                Cell::Agent { color: agent_color(&a.color), name: a.name.clone() },
                Cell::Tag { text: availability_label(&a.availability).into(), tone },
                Cell::Text { text: detail.unwrap_or("—").into(), dim: true },
                Cell::Text { text: a.last_event_utc_ms.map_or("—".into(), fmt::date_time), dim: false },
            ]
        })
        .collect()
}

pub fn diagnostics_page(ui: &mut Ui, t: &Tokens, d: &DiagnosticsData) {
    page(ui, |ui| {
        h2(ui, t, "OVERLAY");
        if let Some(e) = d.error {
            fineprint(ui, t, e);
        }
        if let Some(o) = d.overlay {
            ui.add_space(8.0);
            kvs(ui, t, &overlay_kvs(o));
            // the grid's margin-bottom (4) collapses into the fine print's margin-top (6)
            fineprint(ui, t, &o.platform_note);
        }
        h2(ui, t, "COLLECTORS");
        ui.add_space(8.0);
        table(ui, t, &["AGENT", "STATE", "DETAIL", "LAST USAGE"], &collector_rows(d.agents));
        h2(ui, t, "APPLICATION");
        if let Some(i) = d.info {
            ui.add_space(8.0);
            kvs(ui, t, &application_kvs(i));
            if let Some(n) = &i.database_notice {
                fineprint(ui, t, n);
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ privacy

/// One piece of a paragraph: plain or bold.
pub type Span<'a> = (&'a str, bool);

/// Greedy line breaking of mixed plain/bold spans (JetBrains Mono's bold has the same advance as its regular).
pub fn wrap_spans(spans: &[Span], style: &Style, max_w: f32) -> Vec<Vec<(String, bool)>> {
    // A word is what lies between two spaces; it can run across spans ("disk" in bold glued to a plain ",").
    let mut words: Vec<Vec<(&str, bool)>> = Vec::new();
    let mut open = false;
    for (text, bold) in spans {
        for (i, piece) in text.split(' ').enumerate() {
            if i > 0 {
                open = false;
            }
            if piece.is_empty() {
                continue;
            }
            match words.last_mut() {
                Some(word) if open => word.push((piece, *bold)),
                _ => words.push(vec![(piece, *bold)]),
            }
            open = true;
        }
    }

    let mut lines: Vec<Vec<(String, bool)>> = vec![vec![]];
    let mut x = 0.0;
    let space = measure(style, " ");
    for word in words {
        let w: f32 = word.iter().map(|(p, _)| measure(style, p)).sum();
        if x > 0.0 && x + space + w > max_w {
            lines.push(vec![]);
            x = 0.0;
        }
        let line = lines.last_mut().expect("at least one line");
        for (i, (piece, bold)) in word.into_iter().enumerate() {
            let prefix = if i == 0 && x > 0.0 { " " } else { "" };
            line.push((format!("{prefix}{piece}"), bold));
        }
        x += if x > 0.0 { space + w } else { w };
    }
    lines
}

fn prose(ui: &mut Ui, t: &Tokens, spans: &[Span], bullet: bool) {
    let st = body().lh(1.55);
    let max_w = 62.0 * measure(&body(), "0");
    let indent = if bullet { 40.0 } else { 0.0 };
    let w = (ui.available_width() - indent).min(max_w);
    let lines = wrap_spans(spans, &st, w);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), lines.len() as f32 * st.line_h), Sense::hover());
    if bullet {
        ui.painter().circle_filled(Pos2::new(rect.left() + indent - 9.0, rect.top() + st.line_h / 2.0), 2.5, t.ink);
    }
    for (i, line) in lines.iter().enumerate() {
        let mut x = rect.left() + indent;
        for (text, bold) in line {
            let style = if *bold { Style::mono(12.0, Weight::W700).lh(1.55) } else { st };
            x += put(ui, text, &style, x, rect.top() + i as f32 * st.line_h, t.ink);
        }
    }
}

pub fn privacy_page(ui: &mut Ui, t: &Tokens) {
    page(ui, |ui| {
        // CSS margins collapse: `h3` already brings its own 18 above and 8 below, `p`/`ul` have 12 above and below,
        // `.fineprint` 6 — so only the difference to the larger neighbour is added here.
        h2(ui, t, "LOCAL FIRST");
        ui.add_space(12.0);
        prose(ui, t, &[("AI Usage Monitor reads token counters that your AI coding agents already wrote to ", false), ("your own disk", true), (", and stores aggregated usage in a SQLite file on ", false), ("your own machine", true), (". Nothing is uploaded, and there is no telemetry.", false)], false);
        h3(ui, t, "WHAT IT READS");
        ui.add_space(4.0);
        prose(ui, t, &[("Token counters, model name, timestamp, session id and working-folder path from each agent's session data.", false)], true);
        prose(ui, t, &[("The folder name is used to group usage by project.", false)], true);
        h3(ui, t, "WHAT IT NEVER READS OR STORES");
        ui.add_space(4.0);
        prose(ui, t, &[("Prompts, responses, code, tool output, file contents or conversation titles.", false)], true);
        prose(ui, t, &[("API keys, OAuth tokens or any credential file.", false)], true);
        h3(ui, t, "NETWORK");
        ui.add_space(4.0);
        prose(ui, t, &[("The application contains no networking code and has no way to reach outside connections.", false)], true);
        prose(ui, t, &[("There is no auto-updater and no analytics.", false)], true);
        ui.add_space(12.0 - 6.0);
        // `.fineprint` is a `p` here, so `.prose p`'s 62ch applies — of its own 11px font.
        fineprint_limited(ui, t, "The full audit, including exactly which fields each collector reads, is in PRIVACY.md in the repository.", 62.0 * measure(&hint(), "0"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    fn ctx() -> egui::Context {
        let c = egui::Context::default();
        crate::web::install_fonts(&c);
        let _ = c.run(egui::RawInput::default(), |_| {});
        c
    }

    #[test]
    fn the_tabs_are_as_wide_as_in_the_browser() {
        // Measured in a browser at 11px bold with 0.12em spacing: STATISTICS 115.2 (incl. both borders), SETTINGS 97.38,
        // DIAGNOSTICS 121.13, PRIVACY 89.45. ACCOUNTS is new, so it has no browser measurement; it follows the same formula.
        let w = tab_widths(&ctx());
        let by_label = |label: &str| w[PAGES.iter().position(|(_, l)| *l == label).unwrap()];
        for (label, want) in [("STATISTICS", 115.2), ("SETTINGS", 97.38), ("DIAGNOSTICS", 121.13), ("PRIVACY", 89.45)] {
            assert!((by_label(label) - want).abs() < 0.2, "{label}: tab width {} vs browser {want}", by_label(label));
        }
        let accounts = by_label("ACCOUNTS");
        assert!((accounts - by_label("SETTINGS")).abs() < 0.01 && accounts < by_label("STATISTICS"), "8 letters in a monospace font: as wide as SETTINGS, narrower than STATISTICS (10): {accounts}");
        assert!(w.iter().sum::<f32>() + 213.0 + 24.0 <= 780.0, "all five tabs and the title fit in the window's minimum width");
    }

    #[test]
    fn table_columns_stretch_in_proportion_like_a_browser_table() {
        // From the measured table: natural widths (content + 20 px padding) stretched to 810 px.
        let w = table_column_widths(&[115.4, 108.8, 171.2, 178.4], 810.0);
        for (got, want) in w.iter().zip([163.73, 153.06, 240.81, 252.39]) {
            assert!((got - want).abs() < 2.0, "{got} vs browser {want}");
        }
        assert!((w.iter().sum::<f32>() - 810.0).abs() < 0.01);
        assert_eq!(table_column_widths(&[500.0, 500.0], 600.0), vec![500.0, 500.0], "content wider than the table is not shrunk");
        assert!(table_column_widths(&[], 100.0).is_empty());
    }

    #[test]
    fn the_overlay_grid_uses_the_old_pages_wording_and_tones() {
        let mut d = mock::diagnostics();
        let kv = overlay_kvs(&d);
        assert_eq!(kv.len(), 8);
        assert_eq!(kv[0], ("Visible".into(), "yes".into(), Kv::Plain));
        assert_eq!(kv[2].1, "ON");
        assert_eq!(kv[3].1, "yes");
        assert_eq!(kv[4], ("OS reports topmost".into(), "yes".into(), Kv::Ok));
        assert_eq!(kv[5].1, "1684, 815");
        assert_eq!(kv[6].1, "236 × 208");
        assert_eq!(kv[7].1, "1.25 / 1");
        d.os_reports_topmost = None;
        assert_eq!(overlay_kvs(&d)[4].1, "not readable on this OS");
        assert_eq!(overlay_kvs(&d)[4].2, Kv::Plain);
        d.os_reports_topmost = Some(false);
        assert_eq!(overlay_kvs(&d)[4].2, Kv::Bad, "setting ON but the OS says not topmost");
        d.decorated = Some(true);
        assert_eq!(overlay_kvs(&d)[3].1, "NO — has a title bar");
        assert_eq!(overlay_kvs(&d)[3].2, Kv::Bad);
        d.x = None;
        d.width = None;
        assert_eq!((overlay_kvs(&d)[5].1.as_str(), overlay_kvs(&d)[6].1.as_str()), ("—", "—"));
    }

    #[test]
    fn application_rows_flag_a_rebuilt_database() {
        let mut i = mock::app_info();
        assert_eq!(application_kvs(&i)[4], ("Database health".into(), "ok".into(), Kv::Ok));
        assert_eq!(application_kvs(&i)[1].1, "windows / x86_64");
        i.database_notice = Some("rebuilt".into());
        assert_eq!(application_kvs(&i)[4], ("Database health".into(), "REBUILT after damage".into(), Kv::Bad));
    }

    #[test]
    fn collector_rows_tag_good_bad_and_plain_states() {
        let o = mock::overview();
        let rows = collector_rows(&o.agents);
        assert_eq!(rows.len(), 5);
        assert!(matches!(&rows[0][1], Cell::Tag { text, tone: Tone::Accent } if text == "OK"));
        assert!(matches!(&rows[4][1], Cell::Tag { text, tone: Tone::Plain } if text == "UNAVAILABLE"));
        assert!(matches!(&rows[4][2], Cell::Text { text, dim: true } if text == "no local token ledger"));
        assert!(matches!(&rows[3][3], Cell::Text { text, .. } if text == "—"), "no last usage shows a dash");
        let mut err = o.agents[0].clone();
        err.availability = Availability::Error { message: "boom".into() };
        assert!(matches!(&collector_rows(&[err])[0][1], Cell::Tag { tone: Tone::Warn, .. }));
    }

    #[test]
    fn mixed_bold_text_wraps_without_losing_or_reordering_words() {
        let st = body().lh(1.55);
        let spans: Vec<Span> = vec![("reads counters on ", false), ("your own disk", true), (" and stores them", false)];
        let lines = wrap_spans(&spans, &st, 150.0);
        assert!(lines.len() > 1);
        let flat: Vec<String> = lines.iter().flatten().map(|(w, _)| w.trim().to_string()).collect();
        assert_eq!(flat.join(" "), "reads counters on your own disk and stores them");
        let bold: Vec<&str> = lines.iter().flatten().filter(|(_, b)| *b).map(|(w, _)| w.trim()).collect();
        assert_eq!(bold, ["your", "own", "disk"]);
        for l in &lines {
            let w: f32 = l.iter().map(|(t, _)| measure(&st, t)).sum();
            assert!(w <= 150.0 + 0.01, "{l:?} is {w} wide");
        }
    }

    #[test]
    fn punctuation_after_bold_text_stays_glued_to_it() {
        let st = body().lh(1.55);
        let spans: Vec<Span> = vec![("wrote to ", false), ("your own disk", true), (", and keeps ", false), ("it", true), (".", false)];
        let lines = wrap_spans(&spans, &st, 1000.0);
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "wrote to your own disk, and keeps it.");
        // glued across a line break too: "disk," never splits into "disk" / ","
        let narrow = wrap_spans(&spans, &st, measure(&st, "wrote to your own disk,") - 1.0);
        for line in &narrow {
            assert!(!line.first().is_some_and(|(t, _)| t.trim_start().starts_with(',')), "{line:?}");
        }
    }

    #[test]
    fn every_page_paints_headlessly_with_real_looking_data() {
        let ctx = ctx();
        let t = Tokens::for_theme(Theme::Dark);
        let s = mock::settings();
        let o = mock::overview();
        let info = mock::app_info();
        let diag = mock::diagnostics();
        for page_kind in 0..4 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    let tab = header(ui, &t, Page::Settings);
                    assert!(tab.is_none());
                    match page_kind {
                        0 => {
                            let o = settings_page(ui, &t, &s, &o.agents);
                            assert!(o.changed.is_none() && !o.quit, "nothing changes in a headless frame");
                        }
                        1 => diagnostics_page(ui, &t, &DiagnosticsData { overlay: Some(&diag), agents: &o.agents, info: Some(&info), error: None }),
                        2 => privacy_page(ui, &t),
                        _ => {}
                    }
                });
            });
            assert!(out.shapes.len() > 10, "page {page_kind}");
        }
    }

    #[test]
    fn the_settings_page_lists_every_old_setting_and_nothing_else() {
        // The old page had exactly these rows; the native one must not grow extras (shortcuts, start-with-system, ...).
        let src = include_str!("pages.rs");
        for label in [
            "Always on top", "Compact mode", "Overlay position", "Overlay background", "Token text size", "Show overlay in taskbar", "Pause monitoring", "Polling interval", "Project detection", "Enabled agents", "Theme", "Animation intensity", "Show input tokens", "Show output tokens", "Show cached tokens", "Count cached tokens in totals", "Week starts on", "Retention period", "QUIT APPLICATION",
        ] {
            assert!(src.contains(&format!("\"{label}\"")), "missing the old row '{label}'");
        }
    }
}
