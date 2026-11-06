//! Expanded overlay view module for AI Usage Monitor.
//!
//! Provides the 480 x 680 expanded overlay view in modern Brutalist style:
//! sharp corners (0 rounding), hard 2px borders, monospace numerals, high contrast,
//! honest state representations, and transparent-background readability via halo text.

use crate::view::{self, AgentOverview, Availability, PeriodKey, PeriodOrCustom, Settings, Theme};
use egui::{
    vec2, Align2, Color32, FontId, Rect, Response, Rounding, Sense, Stroke, Ui,
};

// ---------------------------------------------------------------------------
// Public Action & State types
// ---------------------------------------------------------------------------

/// Pages in the main application window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Page {
    Statistics,
    Settings,
    Diagnostics,
    Privacy,
}

/// Actions emitted by the expanded view for the host application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandedAction {
    SetPeriod(PeriodKey),
    Collapse,
    Minimize,
    Hide,
    TogglePin,
    OpenWindow(Page),
    SelectAgent(String),
}

/// UI state for the expanded overlay view.
#[derive(Debug, Clone, Default)]
pub struct ExpandedState {
    pub selected_agent: Option<String>,
}

// ---------------------------------------------------------------------------
// Pure formatting module: fmt
// ---------------------------------------------------------------------------

pub mod fmt {
    /// Format an integer with thousands separator commas (e.g. 1,823,982).
    pub fn format_full(n: u64) -> String {
        let s = n.to_string();
        let mut result = String::new();
        let len = s.len();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (len - i).is_multiple_of(3) {
                result.push(',');
            }
            result.push(c);
        }
        result
    }

    /// Compact count with K/M/B/T suffix and `sig` significant digits.
    /// Trailing zeros are dropped (3,000 -> "3K").
    /// Promotes to next unit on rounding (999,999 -> "1M").
    pub fn format_compact(n: u64, sig: usize) -> String {
        if n < 1000 {
            return n.to_string();
        }
        let units: [(f64, &str); 4] = [
            (1e12, "T"),
            (1e9, "B"),
            (1e6, "M"),
            (1e3, "K"),
        ];
        let v = n as f64;
        for (size, suffix) in units {
            if v >= size {
                let scaled = v / size;
                let int_digits = (scaled.log10().floor() as usize) + 1;
                let decimals = sig.saturating_sub(int_digits);
                let mut text = format!("{:.decimals$}", scaled, decimals = decimals);
                // Rounding can reach the next unit (999,999 -> "1000K"): promote it.
                if let Ok(num) = text.parse::<f64>() {
                    if num >= 1000.0 && suffix != "T" {
                        let promoted = (size * 1000.0).round() as u64;
                        return format_compact(promoted, sig);
                    }
                }
                if text.contains('.') {
                    while text.ends_with('0') {
                        text.pop();
                    }
                    if text.ends_with('.') {
                        text.pop();
                    }
                }
                text.push_str(suffix);
                return text;
            }
        }
        n.to_string()
    }

    /// Format percentage share (e.g. "25%" or "7.5%").
    pub fn format_percent(part: u64, whole: u64) -> String {
        if whole == 0 {
            return "0%".to_string();
        }
        let p = (part as f64 / whole as f64) * 100.0;
        if p >= 10.0 {
            format!("{:.0}%", p)
        } else {
            format!("{:.1}%", p)
        }
    }

    /// Relative time string: "just now", "5s ago", "2m ago", "2h ago", "1d ago".
    pub fn format_relative(then_ms: i64, now_ms: i64) -> String {
        let diff = (now_ms - then_ms).max(0);
        let s = (diff as f64 / 1000.0).round() as i64;
        if s < 5 {
            return "just now".to_string();
        }
        if s < 60 {
            return format!("{}s ago", s);
        }
        let m = (s as f64 / 60.0).round() as i64;
        if m < 60 {
            return format!("{}m ago", m);
        }
        let h = (m as f64 / 60.0).round() as i64;
        if h < 48 {
            return format!("{}h ago", h);
        }
        let d = (h as f64 / 24.0).round() as i64;
        format!("{}d ago", d)
    }

    /// Format relative time or "never" if None.
    pub fn format_relative_opt(then_ms: Option<i64>, now_ms: i64) -> String {
        match then_ms {
            Some(t) => format_relative(t, now_ms),
            None => "never".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure logic functions
// ---------------------------------------------------------------------------

/// Constant for the active activity window (60 seconds).
pub const LIVE_WINDOW_MS: i64 = 60_000;

/// Unavailable token placeholder constant.
pub const UNAVAILABLE_TEXT: &str = "TOKEN DATA UNAVAILABLE";

/// Check whether an agent is active now (last event within 60s of now_ms).
/// Boundary is at exactly 60 seconds (0 <= diff <= 60_000).
pub fn is_active(agent: &AgentOverview, now_ms: i64) -> bool {
    is_active_timestamp(agent.last_event_utc_ms, now_ms)
}

/// Check whether a timestamp is within 60 seconds of now_ms.
pub fn is_active_timestamp(last_event_utc_ms: Option<i64>, now_ms: i64) -> bool {
    match last_event_utc_ms {
        Some(last) => {
            let diff = now_ms - last;
            (0..=LIVE_WINDOW_MS).contains(&diff)
        }
        None => false,
    }
}

/// Format relative time for optional timestamp, returning "never" when None.
pub fn relative_time(then_ms: Option<i64>, now_ms: i64) -> String {
    fmt::format_relative_opt(then_ms, now_ms)
}

/// Stable sort of agents: by total descending, unavailable (None) last, preserving original order.
pub fn sort_agents(agents: &[AgentOverview]) -> Vec<AgentOverview> {
    let mut result = agents.to_vec();
    result.sort_by(|a, b| {
        match (&a.totals, &b.totals) {
            (Some(ta), Some(tb)) => tb.total.cmp(&ta.total),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    result
}

/// Largest-remainder method (Hare-Niemeyer) to distribute percentages to exactly 100%.
/// Safe against total sum == 0 (returns all zeros).
pub fn share_percentages(totals: &[u64]) -> Vec<u32> {
    if totals.is_empty() {
        return Vec::new();
    }
    let sum: u128 = totals.iter().map(|&t| t as u128).sum();
    if sum == 0 {
        return vec![0; totals.len()];
    }

    let mut integer_parts = Vec::with_capacity(totals.len());
    let mut remainders = Vec::with_capacity(totals.len());
    let mut int_sum = 0u32;

    for (i, &t) in totals.iter().enumerate() {
        let scaled = (t as u128) * 100;
        let int_part = (scaled / sum) as u32;
        let rem = scaled % sum;
        integer_parts.push(int_part);
        remainders.push((rem, i));
        int_sum += int_part;
    }

    let diff = (100 - int_sum) as usize;
    // Sort remainders descending. For ties, maintain index order (stable).
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    for &(_, idx) in remainders.iter().take(diff) {
        integer_parts[idx] += 1;
    }

    integer_parts
}

/// Largest-remainder percentages for a list of agents.
pub fn share_percentages_agents(agents: &[AgentOverview]) -> Vec<u32> {
    let totals: Vec<u64> = agents
        .iter()
        .map(|a| a.totals.as_ref().map(|t| t.total).unwrap_or(0))
        .collect();
    share_percentages(&totals)
}

/// Human-readable label for availability state.
pub fn availability_label(a: &Availability) -> &'static str {
    match a {
        Availability::Ok => "OK",
        Availability::NoDataYet => "NO USAGE YET",
        Availability::NotInstalled => "NOT INSTALLED",
        Availability::Unavailable { .. } => "UNAVAILABLE",
        Availability::Error { .. } => "COLLECTOR ERROR",
    }
}

/// Detail text for an agent's availability.
pub fn availability_detail(agent: &AgentOverview) -> Option<String> {
    match &agent.availability {
        Availability::Unavailable { reason } => Some(reason.clone()),
        Availability::Error { message } => Some(message.clone()),
        _ => agent.note.clone(),
    }
}

/// Period key to display label.
pub fn period_label(period: PeriodOrCustom) -> &'static str {
    match period {
        PeriodOrCustom::Day => "TODAY",
        PeriodOrCustom::Week => "THIS WEEK",
        PeriodOrCustom::Month => "THIS MONTH",
        PeriodOrCustom::Year => "THIS YEAR",
        PeriodOrCustom::Custom => "CUSTOM RANGE",
    }
}

// ---------------------------------------------------------------------------
// Theme & Brutalist styling palette
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct BrutalistPalette {
    pub bg: Color32,
    pub ink: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub alert: Color32,
    pub ok: Color32,
    pub halo: Color32,
    pub border_width: f32,
}

impl BrutalistPalette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self {
                bg: Color32::from_rgb(10, 10, 10),
                ink: Color32::from_rgb(242, 242, 238),
                dim: Color32::from_rgb(140, 140, 132),
                accent: Color32::from_rgb(255, 230, 0),
                alert: Color32::from_rgb(255, 77, 0),
                ok: Color32::from_rgb(70, 220, 120),
                halo: Color32::from_rgb(0, 0, 0),
                border_width: 2.0,
            },
            Theme::Light => Self {
                bg: Color32::from_rgb(240, 238, 230),
                ink: Color32::from_rgb(10, 10, 10),
                dim: Color32::from_rgb(92, 92, 86),
                accent: Color32::from_rgb(0, 51, 255),
                alert: Color32::from_rgb(214, 48, 0),
                ok: Color32::from_rgb(0, 140, 70),
                halo: Color32::from_rgb(255, 255, 255),
                border_width: 2.0,
            },
        }
    }
}

/// Parse CSS hex color like "#ff6a00" or "#f60".
pub fn parse_hex_color(hex: &str) -> Color32 {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    } else if s.len() == 3 {
        let r = u8::from_str_radix(&s[0..1], 16).map(|v| v * 17);
        let g = u8::from_str_radix(&s[1..2], 16).map(|v| v * 17);
        let b = u8::from_str_radix(&s[2..3], 16).map(|v| v * 17);
        if let (Ok(r), Ok(g), Ok(b)) = (r, g, b) {
            return Color32::from_rgb(r, g, b);
        }
    }
    Color32::from_rgb(200, 200, 200)
}

// ---------------------------------------------------------------------------
// Halo text rendering
// ---------------------------------------------------------------------------

/// Paints text with a thin halo (outline) in 8 directions so it stays crisp
/// and readable on transparent backgrounds and wallpaper.
pub fn paint_halo_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: Align2,
    text: &str,
    font_id: FontId,
    text_color: Color32,
    halo_color: Color32,
) -> Rect {
    let offsets = [
        vec2(-1.0, 0.0),
        vec2(1.0, 0.0),
        vec2(0.0, -1.0),
        vec2(0.0, 1.0),
        vec2(-1.0, -1.0),
        vec2(1.0, -1.0),
        vec2(-1.0, 1.0),
        vec2(1.0, 1.0),
    ];
    for offset in offsets {
        painter.text(pos + offset, align, text, font_id.clone(), halo_color);
    }
    painter.text(pos, align, text, font_id, text_color)
}

// ---------------------------------------------------------------------------
// UI Components
// ---------------------------------------------------------------------------

/// Brutalist tag widget (hard 2px border, zero rounding, bold uppercase).
fn draw_tag(ui: &mut Ui, text: &str, color: Color32, pal: &BrutalistPalette) {
    let font = FontId::monospace(10.0);
    let padding = vec2(6.0, 2.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let desired_size = galley.size() + padding * 2.0;
    let (rect, _response) = ui.allocate_exact_size(desired_size, Sense::hover());
    ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(pal.border_width, color));
    let text_pos = rect.min + padding;
    ui.painter().galley(text_pos, galley, color);
}

/// Brutalist chip button (hard 2px border, zero rounding, high contrast hover/active).
fn chip_button(ui: &mut Ui, label: &str, is_active: bool, pal: &BrutalistPalette) -> Response {
    let font = FontId::monospace(10.0);
    let padding = vec2(7.0, 3.0);
    let text_color = if is_active {
        pal.bg
    } else {
        pal.ink
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
    let desired_size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    let (bg_color, stroke_color) = if is_active {
        (pal.ink, pal.ink)
    } else if response.hovered() {
        (pal.ink.gamma_multiply(0.15), pal.ink)
    } else {
        (Color32::TRANSPARENT, pal.ink)
    };

    ui.painter().rect_filled(rect, Rounding::ZERO, bg_color);
    ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(pal.border_width, stroke_color));
    let text_pos = rect.min + padding;
    ui.painter().galley(text_pos, galley, text_color);

    response
}

/// Brutalist action button (sharp 2px border with brutalist 3px drop offset on hover/active).
fn action_button(ui: &mut Ui, label: &str, pal: &BrutalistPalette) -> Response {
    let font = FontId::monospace(11.0);
    let padding = vec2(12.0, 5.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, pal.ink);
    let desired_size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    let (bg_color, stroke_color) = if response.is_pointer_button_down_on() {
        (pal.ink.gamma_multiply(0.2), pal.ink)
    } else if response.hovered() {
        (pal.ink.gamma_multiply(0.1), pal.ink)
    } else {
        (pal.bg, pal.ink)
    };

    // Brutalist hard shadow
    let shadow_rect = rect.translate(vec2(3.0, 3.0));
    ui.painter().rect_filled(shadow_rect, Rounding::ZERO, pal.ink);

    ui.painter().rect_filled(rect, Rounding::ZERO, bg_color);
    ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(pal.border_width, stroke_color));
    let text_pos = rect.min + padding;
    ui.painter().galley(text_pos, galley, pal.ink);

    response
}

/// Agent square swatch: filled if has data, hollow if unavailable, pulsing if active.
fn draw_agent_swatch(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
    has_data: bool,
    is_live: bool,
    pal: &BrutalistPalette,
    now_ms: i64,
) {
    let size = 9.0;
    let rect = Rect::from_center_size(center, vec2(size, size));

    if has_data {
        painter.rect_filled(rect, Rounding::ZERO, color);
    } else {
        painter.rect_stroke(rect, Rounding::ZERO, Stroke::new(pal.border_width, color));
    }

    if is_live {
        // Stepped pulse: 4 discrete steps in a 1.6s cycle
        let step = (((now_ms / 400).rem_euclid(4)) as f32) + 1.0;
        let pulse_rect = rect.expand(step * 1.5);
        painter.rect_stroke(pulse_rect, Rounding::ZERO, Stroke::new(pal.border_width, color));
    }
}

// ---------------------------------------------------------------------------
// Main show function
// ---------------------------------------------------------------------------

/// Main entry point for rendering the expanded overlay view (480 x 680).
pub fn show(
    ui: &mut Ui,
    state: &mut ExpandedState,
    overview: &view::Overview,
    settings: &Settings,
    now_ms: i64,
) -> Vec<ExpandedAction> {
    let mut actions = Vec::new();
    let pal = BrutalistPalette::for_theme(settings.theme);

    // Request continuous repaint if any agent is active now so the stepped pulse ticks
    let any_live = overview.agents.iter().any(|a| a.enabled && is_active(a, now_ms));
    if any_live {
        ui.ctx().request_repaint();
    }

    // Outer container: fixed or expanded size (480 x 680 target)
    let outer_rect = ui.available_rect_before_wrap();
    let bg_color = pal.bg.gamma_multiply(settings.overlay_opacity.clamp(0.0, 1.0));
    ui.painter().rect_filled(outer_rect, Rounding::ZERO, bg_color);
    ui.painter().rect_stroke(outer_rect, Rounding::ZERO, Stroke::new(pal.border_width, pal.ink));

    ui.vertical(|ui| {
        // Padding inside the frame
        ui.add_space(8.0);

        // 1. Top Bar: Window Controls (Pin, Collapse, Minimize, Hide)
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.heading(
                egui::RichText::new("AI USAGE MONITOR")
                    .font(FontId::monospace(12.0))
                    .color(pal.ink),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if chip_button(ui, "HIDE", false, &pal).clicked() {
                    actions.push(ExpandedAction::Hide);
                }
                if chip_button(ui, "MIN", false, &pal).clicked() {
                    actions.push(ExpandedAction::Minimize);
                }
                if chip_button(ui, "COLLAPSE", false, &pal).clicked() {
                    actions.push(ExpandedAction::Collapse);
                }
                let pin_text = if settings.always_on_top { "PINNED" } else { "PIN" };
                if chip_button(ui, pin_text, settings.always_on_top, &pal).clicked() {
                    actions.push(ExpandedAction::TogglePin);
                }
            });
        });

        ui.add_space(6.0);
        let separator_stroke = Stroke::new(pal.border_width, pal.ink);
        let sep_start = egui::pos2(outer_rect.left(), ui.cursor().top());
        let sep_end = egui::pos2(outer_rect.right(), ui.cursor().top());
        ui.painter().line_segment([sep_start, sep_end], separator_stroke);
        ui.add_space(10.0);

        // 2. Headline Section
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| {
                let current_period = match overview.period {
                    PeriodOrCustom::Day => PeriodKey::Day,
                    PeriodOrCustom::Week => PeriodKey::Week,
                    PeriodOrCustom::Month => PeriodKey::Month,
                    PeriodOrCustom::Year => PeriodKey::Year,
                    PeriodOrCustom::Custom => settings.period,
                };

                // Period Switcher (DAY WEEK MONTH YEAR)
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("PERIOD:")
                            .font(FontId::monospace(10.0))
                            .color(pal.dim),
                    );
                    let periods = [
                        (PeriodKey::Day, "DAY"),
                        (PeriodKey::Week, "WEEK"),
                        (PeriodKey::Month, "MONTH"),
                        (PeriodKey::Year, "YEAR"),
                    ];
                    for (pk, label) in periods {
                        if chip_button(ui, label, current_period == pk, &pal).clicked() {
                            actions.push(ExpandedAction::SetPeriod(pk));
                        }
                    }
                });

                ui.add_space(6.0);

                // Big Headline Number
                match &overview.totals {
                    Some(t) => {
                        let text_scale = settings.token_text_size.clamp(0.6, 2.0);
                        let font_size = 42.0 * text_scale;
                        let compact_text = fmt::format_compact(t.total, 4);

                        // Use paint_halo_text so it stays readable on transparent wallpapers
                        let num_font = FontId::monospace(font_size);
                        let galley = ui.painter().layout_no_wrap(compact_text.clone(), num_font.clone(), pal.ink);
                        let (rect, _response) = ui.allocate_exact_size(galley.size(), Sense::hover());
                        paint_halo_text(
                            ui.painter(),
                            rect.min,
                            Align2::LEFT_TOP,
                            &compact_text,
                            num_font,
                            pal.ink,
                            pal.halo,
                        );

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("TOKENS {}", period_label(overview.period)))
                                    .font(FontId::monospace(10.0))
                                    .color(pal.dim),
                            );

                            if t.estimated > 0 {
                                draw_tag(ui, "ESTIMATED", pal.alert, &pal);
                            }
                            if overview.paused {
                                draw_tag(ui, "PAUSED", pal.alert, &pal);
                            }
                            if !overview.importing_agents.is_empty() {
                                draw_tag(ui, "IMPORTING HISTORY", pal.ok, &pal);
                            }
                        });

                        ui.add_space(6.0);

                        // Metric split chips: INPUT / OUTPUT / CACHED / REQUESTS
                        ui.horizontal(|ui| {
                            if settings.show_input {
                                draw_split_chip(ui, "INPUT", &fmt::format_compact(t.input, 3), &pal);
                            }
                            if settings.show_output {
                                draw_split_chip(ui, "OUTPUT", &fmt::format_compact(t.output, 3), &pal);
                            }
                            if settings.show_cached {
                                let cached = t.cache_read + t.cache_write;
                                draw_split_chip(ui, "CACHED", &fmt::format_compact(cached, 3), &pal);
                            }
                            draw_split_chip(ui, "REQUESTS", &fmt::format_compact(t.events, 3), &pal);
                        });

                        // Honest explanation of count_cached_in_total
                        ui.add_space(3.0);
                        let cache_notice = if overview.count_cached_in_total {
                            "Total includes cached tokens (Settings -> Display)."
                        } else {
                            "Total excludes cached tokens (Settings -> Display)."
                        };
                        ui.label(
                            egui::RichText::new(cache_notice)
                                .font(FontId::monospace(9.0))
                                .color(pal.dim),
                        );
                    }
                    None => {
                        // When totals == None, NEVER show 0!
                        let na_font = FontId::monospace(18.0);
                        let text = UNAVAILABLE_TEXT;
                        let galley = ui.painter().layout_no_wrap(text.to_string(), na_font.clone(), pal.alert);
                        let (rect, _response) = ui.allocate_exact_size(galley.size(), Sense::hover());
                        paint_halo_text(
                            ui.painter(),
                            rect.min,
                            Align2::LEFT_TOP,
                            text,
                            na_font,
                            pal.alert,
                            pal.halo,
                        );

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(period_label(overview.period))
                                    .font(FontId::monospace(10.0))
                                    .color(pal.dim),
                            );
                            if !overview.importing_agents.is_empty() {
                                draw_tag(ui, "IMPORTING HISTORY", pal.ok, &pal);
                            } else {
                                draw_tag(ui, "NO COLLECTOR REPORTING", pal.alert, &pal);
                            }
                        });
                    }
                }
            });
        });

        ui.add_space(8.0);
        let sep2_start = egui::pos2(outer_rect.left(), ui.cursor().top());
        let sep2_end = egui::pos2(outer_rect.right(), ui.cursor().top());
        ui.painter().line_segment([sep2_start, sep2_end], separator_stroke);
        ui.add_space(6.0);

        // 3. Section Header: ALL AGENTS
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("AGENTS")
                    .font(FontId::monospace(11.0))
                    .color(pal.dim),
            );
        });
        ui.add_space(4.0);

        // 4. All Agents List (Scrollable)
        let sorted = sort_agents(&overview.agents);
        let shares = share_percentages_agents(&sorted);
        let max_total = sorted
            .iter()
            .filter_map(|a| a.totals.as_ref().map(|t| t.total))
            .max()
            .unwrap_or(0);

        let list_height = (ui.available_height() - 44.0).max(120.0);

        egui::ScrollArea::vertical()
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if sorted.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("No agents enabled. Enable some in Settings -> Monitoring.")
                                .font(FontId::monospace(10.0))
                                .color(pal.dim),
                        );
                    }

                    for (idx, agent) in sorted.iter().enumerate() {
                        let is_selected = state.selected_agent.as_deref() == Some(&agent.id);
                        let share_pct = shares.get(idx).copied().unwrap_or(0);
                        let agent_color = parse_hex_color(&agent.color);
                        let live = is_active(agent, now_ms);

                        // Card frame for each agent
                        let card_width = ui.available_width() - 16.0;
                        let card_desired_height = if agent.totals.is_some() { 64.0 } else { 48.0 };
                        let (card_rect, card_resp) = ui.allocate_exact_size(
                            vec2(card_width, card_desired_height),
                            Sense::click(),
                        );

                        if card_resp.clicked() {
                            state.selected_agent = Some(agent.id.clone());
                            actions.push(ExpandedAction::SelectAgent(agent.id.clone()));
                        }

                        // Dimming if disabled
                        let alpha = if agent.enabled { 1.0 } else { 0.45 };
                        let row_ink = pal.ink.gamma_multiply(alpha);
                        let row_dim = pal.dim.gamma_multiply(alpha);
                        let row_border = if is_selected {
                            pal.accent
                        } else {
                            pal.ink.gamma_multiply(alpha)
                        };

                        // Draw card border & background
                        let card_bg = if is_selected {
                            pal.ink.gamma_multiply(0.08)
                        } else if card_resp.hovered() {
                            pal.ink.gamma_multiply(0.04)
                        } else {
                            pal.bg
                        };
                        ui.painter().rect_filled(card_rect, Rounding::ZERO, card_bg);
                        ui.painter().rect_stroke(
                            card_rect,
                            Rounding::ZERO,
                            Stroke::new(pal.border_width, row_border),
                        );

                        // Inside agent card
                        let painter_cursor = card_rect.min + vec2(8.0, 8.0);

                        // Agent swatch square
                        let swatch_center = painter_cursor + vec2(5.0, 5.0);
                        draw_agent_swatch(
                            ui.painter(),
                            swatch_center,
                            agent_color.gamma_multiply(alpha),
                            agent.totals.is_some(),
                            live && agent.enabled,
                            &pal,
                            now_ms,
                        );

                        // Agent Name
                        let name_font = FontId::monospace(11.0);
                        let name_pos = painter_cursor + vec2(16.0, 0.0);
                        ui.painter().text(
                            name_pos,
                            Align2::LEFT_TOP,
                            agent.name.to_uppercase(),
                            name_font,
                            row_ink,
                        );

                        // Top-right: Total or Availability status
                        match &agent.totals {
                            Some(t) => {
                                let total_text = fmt::format_compact(t.total, 4);
                                let total_font = FontId::monospace(12.0);
                                let total_pos = egui::pos2(card_rect.right() - 8.0, card_rect.min.y + 8.0);
                                ui.painter().text(
                                    total_pos,
                                    Align2::RIGHT_TOP,
                                    total_text,
                                    total_font,
                                    row_ink,
                                );

                                // Share-of-total bar
                                let bar_y = card_rect.min.y + 26.0;
                                let bar_left = card_rect.min.x + 8.0;
                                let bar_right = card_rect.right() - 8.0;
                                let bar_width = (bar_right - bar_left).max(10.0);
                                let bar_height = 8.0;
                                let bar_rect = Rect::from_min_size(
                                    egui::pos2(bar_left, bar_y),
                                    vec2(bar_width, bar_height),
                                );

                                // Outer border of usage bar
                                ui.painter().rect_stroke(
                                    bar_rect,
                                    Rounding::ZERO,
                                    Stroke::new(pal.border_width, row_ink),
                                );

                                // Fill ratio
                                let ratio = if max_total > 0 {
                                    (t.total as f32 / max_total as f32).clamp(0.0, 1.0)
                                } else {
                                    0.0
                                };
                                let fill_width = bar_width * ratio;
                                if fill_width > 0.0 {
                                    let fill_rect = Rect::from_min_size(
                                        egui::pos2(bar_left, bar_y),
                                        vec2(fill_width, bar_height),
                                    );
                                    let fill_color = if t.estimated > 0 {
                                        pal.alert.gamma_multiply(alpha)
                                    } else {
                                        agent_color.gamma_multiply(alpha)
                                    };
                                    ui.painter().rect_filled(fill_rect, Rounding::ZERO, fill_color);
                                }

                                // Breakdown line: IN / OUT / CACHE / REQ / Recency / Share%
                                let split_y = card_rect.min.y + 40.0;
                                let mut split_parts = Vec::new();
                                if settings.show_input {
                                    split_parts.push(format!("IN {}", fmt::format_compact(t.input, 3)));
                                }
                                if settings.show_output {
                                    split_parts.push(format!("OUT {}", fmt::format_compact(t.output, 3)));
                                }
                                if settings.show_cached {
                                    let c = t.cache_read + t.cache_write;
                                    split_parts.push(format!("CACHE {}", fmt::format_compact(c, 3)));
                                }
                                split_parts.push(format!("REQ {}", fmt::format_compact(t.events, 3)));
                                split_parts.push(format!("SHARE {}%", share_pct));

                                let split_text = split_parts.join(" · ");
                                let split_font = FontId::monospace(9.0);
                                ui.painter().text(
                                    egui::pos2(bar_left, split_y),
                                    Align2::LEFT_TOP,
                                    split_text,
                                    split_font,
                                    row_dim,
                                );

                                // Recency on right of split line
                                let recency_text = relative_time(agent.last_event_utc_ms, now_ms);
                                ui.painter().text(
                                    egui::pos2(bar_right, split_y),
                                    Align2::RIGHT_TOP,
                                    recency_text,
                                    FontId::monospace(9.0),
                                    row_dim,
                                );
                            }
                            None => {
                                // Agent without totals: TOKEN DATA UNAVAILABLE + reason, NEVER 0!
                                let state_font = FontId::monospace(10.0);
                                let state_label = availability_label(&agent.availability);
                                ui.painter().text(
                                    egui::pos2(card_rect.right() - 8.0, card_rect.min.y + 8.0),
                                    Align2::RIGHT_TOP,
                                    state_label,
                                    state_font,
                                    pal.alert.gamma_multiply(alpha),
                                );

                                let detail = availability_detail(agent);
                                let reason_str = match detail {
                                    Some(d) => format!("{} — {}", UNAVAILABLE_TEXT, d),
                                    None => UNAVAILABLE_TEXT.to_string(),
                                };

                                let reason_font = FontId::monospace(9.0);
                                ui.painter().text(
                                    card_rect.min + vec2(8.0, 26.0),
                                    Align2::LEFT_TOP,
                                    reason_str,
                                    reason_font,
                                    row_dim,
                                );
                            }
                        }

                        ui.add_space(4.0);
                    }
                });
            });

        ui.add_space(6.0);
        let sep3_start = egui::pos2(outer_rect.left(), ui.cursor().top());
        let sep3_end = egui::pos2(outer_rect.right(), ui.cursor().top());
        ui.painter().line_segment([sep3_start, sep3_end], separator_stroke);
        ui.add_space(8.0);

        // 5. Bottom Navigation Bar
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if action_button(ui, "STATISTICS", &pal).clicked() {
                actions.push(ExpandedAction::OpenWindow(Page::Statistics));
            }
            ui.add_space(8.0);
            if action_button(ui, "SETTINGS", &pal).clicked() {
                actions.push(ExpandedAction::OpenWindow(Page::Settings));
            }
            ui.add_space(8.0);
            if action_button(ui, "DIAGNOSTICS", &pal).clicked() {
                actions.push(ExpandedAction::OpenWindow(Page::Diagnostics));
            }
        });
    });

    actions
}

/// Helper to draw a metric chip in the split row (e.g. INPUT 1.2M)
fn draw_split_chip(ui: &mut Ui, label: &str, value: &str, pal: &BrutalistPalette) {
    let font_label = FontId::monospace(8.0);
    let font_val = FontId::monospace(10.0);
    let padding = vec2(6.0, 4.0);

    let g_label = ui.painter().layout_no_wrap(label.to_string(), font_label, pal.dim);
    let g_val = ui.painter().layout_no_wrap(value.to_string(), font_val, pal.ink);

    let content_width = g_label.size().x.max(g_val.size().x);
    let content_height = g_label.size().y + g_val.size().y + 2.0;
    let desired_size = vec2(content_width + padding.x * 2.0, content_height + padding.y * 2.0);

    let (rect, _resp) = ui.allocate_exact_size(desired_size, Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, pal.bg);
    ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(pal.border_width, pal.ink));

    let top_left = rect.min + padding;
    ui.painter().galley(top_left, g_label, pal.dim);
    ui.painter().galley(top_left + vec2(0.0, 10.0), g_val, pal.ink);
}

// ---------------------------------------------------------------------------
// Unit tests & Headless smoke tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    #[test]
    fn format_compact_required_values() {
        assert_eq!(fmt::format_compact(0, 3), "0");
        assert_eq!(fmt::format_compact(999, 3), "999");
        assert_eq!(fmt::format_compact(1000, 3), "1K");
        assert_eq!(fmt::format_compact(3_000, 3), "3K");
        assert_eq!(fmt::format_compact(820_421, 3), "820K");
        assert_eq!(fmt::format_compact(999_999, 3), "1M");
        assert_eq!(fmt::format_compact(1_000_000, 3), "1M");
        assert_eq!(fmt::format_compact(1_823_982, 3), "1.82M");
        assert_eq!(fmt::format_compact(8_420_000, 3), "8.42M");
        assert_eq!(fmt::format_compact(31_700_000, 3), "31.7M");
        assert_eq!(fmt::format_compact(999_500_000, 3), "1B");
        assert_eq!(fmt::format_compact(1_230_000_000, 3), "1.23B");
        assert_eq!(fmt::format_compact(1_000_000_000_000, 3), "1T"); // 10^12
        assert_eq!(fmt::format_compact(1_234_567_890_123, 3), "1.23T");
    }

    #[test]
    fn format_compact_hero_4_digits() {
        assert_eq!(fmt::format_compact(999, 4), "999");
        assert_eq!(fmt::format_compact(3_000, 4), "3K");
        assert_eq!(fmt::format_compact(12_345, 4), "12.35K");
        assert_eq!(fmt::format_compact(820_421, 4), "820.4K");
        assert_eq!(fmt::format_compact(999_999, 4), "1M");
        assert_eq!(fmt::format_compact(1_823_982, 4), "1.824M");
        assert_eq!(fmt::format_compact(253_516_254, 4), "253.5M");
        assert_eq!(fmt::format_compact(1_234_567_890_123, 4), "1.235T");
    }

    #[test]
    fn format_full_thousands_grouping() {
        assert_eq!(fmt::format_full(0), "0");
        assert_eq!(fmt::format_full(999), "999");
        assert_eq!(fmt::format_full(1000), "1,000");
        assert_eq!(fmt::format_full(1_823_982), "1,823,982");
    }

    #[test]
    fn relative_time_required_cases() {
        let now = 1_000_000_000;
        assert_eq!(relative_time(None, now), "never");
        assert_eq!(relative_time(Some(now - 2000), now), "just now");
        assert_eq!(relative_time(Some(now - 5000), now), "5s ago");
        assert_eq!(relative_time(Some(now - 120_000), now), "2m ago");
        assert_eq!(relative_time(Some(now - 2 * 3600 * 1000), now), "2h ago");
        assert_eq!(relative_time(Some(now - 3 * 86400 * 1000), now), "3d ago");
    }

    #[test]
    fn is_active_boundary_exactly_60s() {
        let now = 1_000_000_000;
        let mut ag = mock::agent("test", "Test", "TST", "#fff", None, None);

        // None -> false
        ag.last_event_utc_ms = None;
        assert!(!is_active(&ag, now));

        // Future -> false
        ag.last_event_utc_ms = Some(now + 1);
        assert!(!is_active(&ag, now));

        // Exactly at now -> true
        ag.last_event_utc_ms = Some(now);
        assert!(is_active(&ag, now));

        // Inside 60s -> true
        ag.last_event_utc_ms = Some(now - 59_999);
        assert!(is_active(&ag, now));

        // Exactly at 60s boundary -> true
        ag.last_event_utc_ms = Some(now - 60_000);
        assert!(is_active(&ag, now));

        // Exactly beyond 60s -> false
        ag.last_event_utc_ms = Some(now - 60_001);
        assert!(!is_active(&ag, now));
    }

    #[test]
    fn sorting_total_desc_unavailable_last_stable() {
        let a1 = mock::agent("a1", "A1", "A1", "#111", Some(mock::totals(10, 0, 0, 0, true)), None); // 10
        let a2 = mock::agent("a2", "A2", "A2", "#222", Some(mock::totals(50, 0, 0, 0, true)), None); // 50
        let a3 = mock::agent("a3", "A3", "A3", "#333", Some(mock::totals(50, 0, 0, 0, true)), None); // 50 (equal, stable)
        let a4 = mock::agent("a4", "A4", "A4", "#444", None, None); // unavailable
        let a5 = mock::agent("a5", "A5", "A5", "#555", None, None); // unavailable

        let sorted = sort_agents(&[a1.clone(), a4.clone(), a2.clone(), a5.clone(), a3.clone()]);
        let ids: Vec<&str> = sorted.iter().map(|a| a.id.as_str()).collect();
        // a2 and a3 have 50 (a2 came before a3 so a2 before a3), then a1 (10), then unavailables a4, a5 in original order
        assert_eq!(ids, vec!["a2", "a3", "a1", "a4", "a5"]);
    }

    #[test]
    fn share_percentages_largest_remainder_to_100() {
        // Zero-total safe
        assert_eq!(share_percentages(&[]), Vec::<u32>::new());
        assert_eq!(share_percentages(&[0, 0, 0]), vec![0, 0, 0]);

        // Exact shares
        assert_eq!(share_percentages(&[50, 50]), vec![50, 50]);
        assert_eq!(share_percentages(&[70, 20, 10]), vec![70, 20, 10]);

        // Remainder rounding (1, 1, 1 -> sum 3 -> 33.3% each -> one gets 34, sum = 100)
        let res = share_percentages(&[1, 1, 1]);
        assert_eq!(res.iter().sum::<u32>(), 100);
        assert_eq!(res, vec![34, 33, 33]);

        // Arbitrary numbers
        let test_cases: Vec<Vec<u64>> = vec![
            vec![10, 20, 30, 40],
            vec![1, 2, 3, 4, 5, 6, 7],
            vec![123_456, 789_012, 345_678],
            vec![999_999_999, 1],
        ];
        for totals in test_cases {
            let shares = share_percentages(&totals);
            assert_eq!(shares.iter().sum::<u32>(), 100);
        }
    }

    #[test]
    fn parse_hex_colors() {
        assert_eq!(parse_hex_color("#ff6a00"), Color32::from_rgb(255, 106, 0));
        assert_eq!(parse_hex_color("#000000"), Color32::from_rgb(0, 0, 0));
        assert_eq!(parse_hex_color("#fff"), Color32::from_rgb(255, 255, 255));
    }

    #[test]
    fn paint_halo_text_allocates_and_draws() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = paint_halo_text(
                    ui.painter(),
                    egui::pos2(10.0, 10.0),
                    Align2::LEFT_TOP,
                    "HALO TEST",
                    FontId::monospace(14.0),
                    Color32::WHITE,
                    Color32::BLACK,
                );
                assert!(rect.width() > 0.0);
                assert!(rect.height() > 0.0);
            });
        });
    }

    #[test]
    fn headless_smoke_test_with_mock_data() {
        let ctx = egui::Context::default();
        let mut state = ExpandedState::default();
        let overview = mock::overview();
        let settings = mock::settings();
        let now = mock::NOW_MS;

        // Run frame 1
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = show(ui, &mut state, &overview, &settings, now);
                // In first render without user interaction, no actions emitted
                assert!(actions.is_empty());
            });
        });

        // Run frame 2 with unavailable overview
        let mut unavail_overview = overview.clone();
        unavail_overview.totals = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = show(ui, &mut state, &unavail_overview, &settings, now);
                assert!(actions.is_empty());
            });
        });
    }
}
