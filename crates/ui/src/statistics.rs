use crate::view::{History, Overview, PeriodOrCustom, TimelineBucket};
use chrono::{Local, TimeZone, Utc, NaiveDate};
use serde_json;

#[derive(Debug, Clone, PartialEq)]
pub enum StatisticsAction {
    SetPeriod(PeriodOrCustom),
    SetCustomRange { start_utc_ms: i64, end_utc_ms: i64 },
    ExportJson,
    ExportCsv,
}

#[derive(Debug, Clone, Default)]
pub struct StatisticsState {
    pub custom_start: String,
    pub custom_end: String,
}

pub fn format_compact(n: u64, sig: usize) -> String {
    if n < 1000 {
        return n.to_string();
    }
    let units = [(1_000_000_000_000, "T"), (1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")];
    for (size, suffix) in units {
        if n >= size {
            let scaled = n as f64 / size as f64;
            let int_digits = if scaled >= 1.0 { scaled.log10().floor() as usize + 1 } else { 1 };
            let decimals = sig.saturating_sub(int_digits);
            let mut text = format!("{:.*}", decimals, scaled);
            if text.parse::<f64>().unwrap_or(0.0) >= 1000.0 && suffix != "T" {
                return format_compact(size * 1000, sig);
            }
            if text.contains('.') {
                text = text.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            return format!("{}{}", text, suffix);
        }
    }
    n.to_string()
}

pub fn nice_ticks(max: u64) -> Vec<u64> {
    if max == 0 {
        return vec![0, 100];
    }
    let p = (max as f64).log10().floor();
    let p10 = 10_f64.powf(p);
    let norm = max as f64 / p10;
    
    let step = if norm <= 1.2 {
        0.2 * p10
    } else if norm <= 2.5 {
        0.5 * p10
    } else if norm <= 5.0 {
        1.0 * p10
    } else {
        2.0 * p10
    };
    
    let mut ticks = vec![];
    let mut cur = 0.0;
    while cur <= max as f64 {
        ticks.push(cur as u64);
        cur += step;
    }
    if *ticks.last().unwrap() < max {
        ticks.push(cur as u64);
    }
    ticks
}

pub fn largest_remainder_percentages(values: &[u64]) -> Vec<u64> {
    let total: u64 = values.iter().sum();
    if total == 0 {
        return vec![0; values.len()];
    }
    let mut exacts = Vec::with_capacity(values.len());
    let mut floored = Vec::with_capacity(values.len());
    let mut sum_floored = 0;
    for (i, &v) in values.iter().enumerate() {
        let exact = (v as f64 * 100.0) / total as f64;
        let f = exact.floor() as u64;
        exacts.push((i, exact, exact - f as f64));
        floored.push(f);
        sum_floored += f;
    }
    
    let mut diff = 100 - sum_floored;
    exacts.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    
    for (i, _, _) in exacts {
        if diff == 0 {
            break;
        }
        floored[i] += 1;
        diff -= 1;
    }
    floored
}

pub fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_csv(h: &History) -> String {
    let mut s = String::new();
    s.push_str("start_utc,end_utc,project,model,agent_id,tokens\n");
    
    let start = Utc.timestamp_millis_opt(h.range.start_utc_ms).unwrap().to_rfc3339();
    let end = Utc.timestamp_millis_opt(h.range.end_utc_ms).unwrap().to_rfc3339();
    
    for (proj, tokens) in &h.by_project {
        s.push_str(&format!("{},{},{},,,{}\n", start, end, escape_csv(proj), tokens));
    }
    for (model, agent, tokens) in &h.by_model {
        s.push_str(&format!("{},{},,{},{},{}\n", start, end, escape_csv(model), escape_csv(agent), tokens));
    }
    s
}

pub fn export_json(h: &History) -> String {
    serde_json::to_string_pretty(h).unwrap_or_default()
}

pub fn parse_hex_color(hex: &str) -> egui::Color32 {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        egui::Color32::from_rgb(r, g, b)
    } else {
        egui::Color32::GRAY
    }
}

pub fn format_bucket_label(b: &TimelineBucket, period: PeriodOrCustom) -> String {
    let dt = Local.timestamp_millis_opt(b.start_utc_ms).unwrap();
    match period {
        PeriodOrCustom::Day => dt.format("%H:%M").to_string(),
        PeriodOrCustom::Week => dt.format("%a").to_string(),
        PeriodOrCustom::Month => dt.format("%d").to_string(),
        PeriodOrCustom::Year => dt.format("%b").to_string(),
        PeriodOrCustom::Custom => dt.format("%Y-%m-%d").to_string(),
    }
}

pub fn validate_custom_range(start: &str, end: &str) -> Option<(i64, i64)> {
    if let (Ok(s), Ok(e)) = (NaiveDate::parse_from_str(start, "%Y-%m-%d"), NaiveDate::parse_from_str(end, "%Y-%m-%d")) {
        if s <= e {
            // max 5 years
            let days = (e - s).num_days();
            if days <= 5 * 366 {
                let start_ms = s.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
                let end_ms = e.and_hms_opt(23, 59, 59)?.and_utc().timestamp_millis();
                return Some((start_ms, end_ms));
            }
        }
    }
    None
}

pub fn hit_test_bucket(num_buckets: usize, x: f32, bounds: egui::Rect) -> Option<usize> {
    if num_buckets == 0 || !bounds.contains(egui::pos2(x, bounds.center().y)) {
        return None;
    }
    let w = bounds.width() / num_buckets as f32;
    let idx = ((x - bounds.left()) / w).floor() as usize;
    if idx < num_buckets { Some(idx) } else { None }
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut StatisticsState,
    history: &History,
    overview: &Overview,
) -> Vec<StatisticsAction> {
    let mut actions = vec![];
    
    // Brutalist styling
    let mut style = (*ui.ctx().style()).clone();
    style.visuals.window_rounding = egui::Rounding::ZERO;
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.popup_shadow = egui::epaint::Shadow::NONE;
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::ZERO;
    style.visuals.widgets.inactive.rounding = egui::Rounding::ZERO;
    style.visuals.widgets.hovered.rounding = egui::Rounding::ZERO;
    style.visuals.widgets.active.rounding = egui::Rounding::ZERO;
    ui.ctx().set_style(style);

    ui.heading(egui::RichText::new("STATISTICS").monospace().strong());
    ui.add_space(8.0);
    
    // Period tabs
    ui.horizontal(|ui| {
        for p in [PeriodOrCustom::Day, PeriodOrCustom::Week, PeriodOrCustom::Month, PeriodOrCustom::Year, PeriodOrCustom::Custom] {
            let label = match p {
                PeriodOrCustom::Day => "DAY",
                PeriodOrCustom::Week => "WEEK",
                PeriodOrCustom::Month => "MONTH",
                PeriodOrCustom::Year => "YEAR",
                PeriodOrCustom::Custom => "CUSTOM",
            };
            let is_selected = history.period == p;
            let btn = egui::Button::new(label).fill(if is_selected { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.inactive.bg_fill });
            if ui.add(btn).clicked() {
                actions.push(StatisticsAction::SetPeriod(p));
            }
        }
    });

    if history.period == PeriodOrCustom::Custom {
        ui.horizontal(|ui| {
            ui.label("Start (YYYY-MM-DD):");
            ui.text_edit_singleline(&mut state.custom_start);
            ui.label("End (YYYY-MM-DD):");
            ui.text_edit_singleline(&mut state.custom_end);
            if ui.button("Apply").clicked() {
                if let Some((s, e)) = validate_custom_range(&state.custom_start, &state.custom_end) {
                    actions.push(StatisticsAction::SetCustomRange { start_utc_ms: s, end_utc_ms: e });
                }
            }
        });
    }

    ui.add_space(16.0);

    // Headline
    if let Some(t) = history.totals {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{} TOTAL TOKENS", format_compact(t.total, 4))).size(24.0).monospace().strong());
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("IN: {}", format_compact(t.input, 3))).monospace());
            ui.label(egui::RichText::new(format!("OUT: {}", format_compact(t.output, 3))).monospace());
            ui.label(egui::RichText::new(format!("CACHE-RD: {}", format_compact(t.cache_read, 3))).monospace());
            ui.label(egui::RichText::new(format!("CACHE-WR: {}", format_compact(t.cache_write, 3))).monospace());
            if t.estimated > 0 {
                ui.label(egui::RichText::new(format!("ESTIMATED: {}", format_compact(t.estimated, 3))).monospace());
            }
        });
    } else {
        ui.label(egui::RichText::new("TOKEN DATA UNAVAILABLE").size(24.0).monospace().strong());
    }

    ui.add_space(16.0);

    // Timeline chart
    if history.totals.is_some() {
        let max_y = history.buckets.iter().map(|b| b.total).max().unwrap_or(0);
        let ticks = nice_ticks(max_y);
        let max_tick = *ticks.last().unwrap_or(&100) as f32;
        
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 200.0), egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let chart_rect = rect.shrink(20.0);
            painter.rect_stroke(chart_rect, 0.0, (2.0, ui.visuals().text_color()));
            
            // Y-axis ticks
            for &tick in &ticks {
                let y = chart_rect.bottom() - (tick as f32 / max_tick) * chart_rect.height();
                painter.line_segment([egui::pos2(chart_rect.left(), y), egui::pos2(chart_rect.left() - 5.0, y)], (1.0, ui.visuals().text_color()));
                painter.text(
                    egui::pos2(chart_rect.left() - 8.0, y),
                    egui::Align2::RIGHT_CENTER,
                    format_compact(tick, 3),
                    egui::FontId::monospace(10.0),
                    ui.visuals().text_color(),
                );
            }
            
            // Bars
            let n = history.buckets.len();
            if n > 0 {
                let w = chart_rect.width() / n as f32;
                for (i, b) in history.buckets.iter().enumerate() {
                    let x0 = chart_rect.left() + i as f32 * w;
                    let x1 = x0 + w;
                    let margin = w * 0.1;
                    
                    if b.total == 0 {
                        // Empty baseline tick
                        painter.line_segment(
                            [egui::pos2(x0 + margin, chart_rect.bottom()), egui::pos2(x1 - margin, chart_rect.bottom())],
                            (1.0, ui.visuals().text_color())
                        );
                    } else {
                        let mut current_y = chart_rect.bottom();
                        for (agent_id, amt) in &b.by_agent {
                            if *amt == 0 { continue; }
                            let h = (*amt as f32 / max_tick) * chart_rect.height();
                            let color = overview.agents.iter().find(|a| &a.id == agent_id).map(|a| parse_hex_color(&a.color)).unwrap_or(egui::Color32::GRAY);
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(x0 + margin, current_y - h),
                                egui::pos2(x1 - margin, current_y),
                            );
                            painter.rect_filled(bar_rect, 0.0, color);
                            current_y -= h;
                        }
                    }
                    
                    // X-axis label (sparse to prevent overlap)
                    if i % ((n / 6).max(1)) == 0 || i == n - 1 {
                        let label = format_bucket_label(b, history.period);
                        painter.text(
                            egui::pos2(x0 + w * 0.5, chart_rect.bottom() + 5.0),
                            egui::Align2::CENTER_TOP,
                            label,
                            egui::FontId::monospace(10.0),
                            ui.visuals().text_color(),
                        );
                    }
                }
                
                // Hover Tooltip
                if let Some(pos) = response.hover_pos() {
                    if let Some(idx) = hit_test_bucket(n, pos.x, chart_rect) {
                        let b = &history.buckets[idx];
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("chart_tooltip"), |ui| {
                            ui.label(egui::RichText::new(format_bucket_label(b, history.period)).strong());
                            for (agent_id, amt) in &b.by_agent {
                                if *amt > 0 {
                                    let name = overview.agents.iter().find(|a| &a.id == agent_id).map(|a| a.name.clone()).unwrap_or_else(|| agent_id.to_string());
                                    ui.label(format!("{}: {}", name, format_compact(*amt, 3)));
                                }
                            }
                            ui.label(format!("Total: {}", format_compact(b.total, 3)));
                        });
                    }
                }
            }
        }
    }

    ui.add_space(16.0);

    // Per-agent bars (horizontal share)
    if let Some(t) = history.totals {
        if t.total > 0 {
            ui.heading(egui::RichText::new("BY AGENT").monospace());
            let agent_totals: Vec<(String, u64)> = overview.agents.iter().filter_map(|a| {
                a.totals.map(|at| (a.id.clone(), at.total))
            }).collect();
            
            let values: Vec<u64> = agent_totals.iter().map(|(_, v)| *v).collect();
            let percentages = largest_remainder_percentages(&values);
            
            for (i, (agent_id, val)) in agent_totals.iter().enumerate() {
                let p = percentages[i];
                let agent = overview.agents.iter().find(|a| &a.id == agent_id).unwrap();
                let color = parse_hex_color(&agent.color);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{:>3}%", p)).monospace());
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 16.0), egui::Sense::hover());
                    if ui.is_rect_visible(rect) {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + (rect.width() * p as f32 / 100.0), rect.bottom())),
                            0.0,
                            color
                        );
                        ui.painter().rect_stroke(rect, 0.0, (1.0, ui.visuals().text_color()));
                    }
                    ui.label(egui::RichText::new(&agent.name).monospace());
                    ui.label(egui::RichText::new(format_compact(*val, 3)).monospace());
                });
            }
        }
    }

    ui.add_space(16.0);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.heading(egui::RichText::new("BY MODEL").monospace());
            egui::Grid::new("models_grid").striped(true).show(ui, |ui| {
                ui.label(egui::RichText::new("MODEL").strong().monospace());
                ui.label(egui::RichText::new("AGENT").strong().monospace());
                ui.label(egui::RichText::new("TOKENS").strong().monospace());
                ui.end_row();
                for (model, agent, val) in &history.by_model {
                    ui.label(egui::RichText::new(model).monospace());
                    ui.label(egui::RichText::new(agent).monospace());
                    ui.label(egui::RichText::new(format_compact(*val, 3)).monospace());
                    ui.end_row();
                }
            });
        });
        ui.add_space(32.0);
        ui.vertical(|ui| {
            ui.heading(egui::RichText::new("BY PROJECT").monospace());
            egui::Grid::new("projects_grid").striped(true).show(ui, |ui| {
                ui.label(egui::RichText::new("PROJECT").strong().monospace());
                ui.label(egui::RichText::new("TOKENS").strong().monospace());
                ui.end_row();
                for (proj, val) in &history.by_project {
                    ui.label(egui::RichText::new(proj).monospace());
                    ui.label(egui::RichText::new(format_compact(*val, 3)).monospace());
                    ui.end_row();
                }
            });
        });
    });

    ui.add_space(16.0);

    // Exports
    ui.horizontal(|ui| {
        if ui.button(egui::RichText::new("EXPORT JSON").monospace()).clicked() {
            actions.push(StatisticsAction::ExportJson);
        }
        if ui.button(egui::RichText::new("EXPORT CSV").monospace()).clicked() {
            actions.push(StatisticsAction::ExportCsv);
        }
    });

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Range;

    #[test]
    fn test_format_compact() {
        assert_eq!(format_compact(820, 3), "820");
        assert_eq!(format_compact(12300, 3), "12.3K");
        assert_eq!(format_compact(820000, 3), "820K");
        assert_eq!(format_compact(1820000, 3), "1.82M");
        assert_eq!(format_compact(31700000, 3), "31.7M");
        assert_eq!(format_compact(3000, 3), "3K");
    }

    #[test]
    fn test_nice_ticks() {
        assert_eq!(nice_ticks(0), vec![0, 100]);
        let t = nice_ticks(180);
        assert!(t.contains(&100));
        assert!(t.contains(&200));
    }

    #[test]
    fn test_largest_remainder() {
        assert_eq!(largest_remainder_percentages(&[1, 1, 1]), vec![34, 33, 33]);
        assert_eq!(largest_remainder_percentages(&[0, 0, 0]), vec![0, 0, 0]);
        assert_eq!(largest_remainder_percentages(&[50, 50]), vec![50, 50]);
    }

    #[test]
    fn test_export_csv() {
        let h = History {
            period: PeriodOrCustom::Day,
            range: Range { start_utc_ms: 1000, end_utc_ms: 2000 },
            buckets: vec![],
            totals: None,
            by_model: vec![("gpt-4".to_string(), "bob".to_string(), 400)],
            by_project: vec![("Proj A".to_string(), 100), ("Proj B, with comma".to_string(), 200), ("Proj C\nnewline".to_string(), 300)],
        };
        let csv = export_csv(&h);
        assert!(csv.contains("Proj A"));
        assert!(csv.contains("\"Proj B, with comma\""));
        assert!(csv.contains("\"Proj C\nnewline\""));
        assert!(csv.contains("gpt-4"));
    }

    #[test]
    fn test_custom_range_val() {
        assert!(validate_custom_range("2020-01-01", "2020-01-31").is_some());
        assert!(validate_custom_range("2020-01-31", "2020-01-01").is_none());
        assert!(validate_custom_range("invalid", "2020-01-01").is_none());
    }

    #[test]
    fn test_headless_smoke() {
        let mut state = StatisticsState::default();
        let history = History {
            period: PeriodOrCustom::Day,
            range: Range { start_utc_ms: 0, end_utc_ms: 0 },
            buckets: vec![],
            totals: None,
            by_model: vec![],
            by_project: vec![],
        };
        let overview = Overview {
            period: PeriodOrCustom::Day,
            range: Range { start_utc_ms: 0, end_utc_ms: 0 },
            generated_at_utc_ms: 0,
            totals: None,
            agents: vec![],
            unavailable_agents: 0,
            any_data: false,
            paused: false,
            count_cached_in_total: false,
            importing_agents: vec![],
        };
        
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = show(ui, &mut state, &history, &overview);
                assert!(actions.is_empty());
            });
        });
    }
}
