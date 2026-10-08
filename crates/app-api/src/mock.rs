//! Deterministic synthetic data for previews and tests. **Identical in every agent's copy - do not edit.**
//! Nothing here reads a real agent, file or database.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::view::*;

pub const NOW_MS: i64 = 1_760_000_000_000;
const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

pub fn totals(input: u64, output: u64, cache_read: u64, cache_write: u64, count_cached: bool) -> Totals {
    let total = if count_cached { input + output + cache_read + cache_write } else { input + output };
    Totals { input, output, cache_read, cache_write, reasoning: output / 8, estimated: 0, events: 120, total }
}

pub fn agent(id: &str, name: &str, short: &str, color: &str, t: Option<Totals>, last_ago_ms: Option<i64>) -> AgentOverview {
    AgentOverview {
        id: id.into(),
        name: name.into(),
        short: short.into(),
        color: color.into(),
        enabled: true,
        availability: if t.is_some() { Availability::Ok } else { Availability::Unavailable { reason: "no local token ledger".into() } },
        note: None,
        totals: t,
        last_event_utc_ms: last_ago_ms.map(|a| NOW_MS - a),
    }
}

pub fn overview() -> Overview {
    let claude = totals(1_200_000, 2_100_000, 220_000_000, 9_000_000, true);
    let codex = totals(4_000_000, 800_000, 14_000_000, 0, true);
    let gemini = totals(2_000_000, 300_000, 1_000_000, 0, true);
    let anti = totals(900_000, 200_000, 0, 0, true);
    let sum = |f: fn(&Totals) -> u64| f(&claude) + f(&codex) + f(&gemini) + f(&anti);
    let all = Totals {
        input: sum(|t| t.input),
        output: sum(|t| t.output),
        cache_read: sum(|t| t.cache_read),
        cache_write: sum(|t| t.cache_write),
        reasoning: sum(|t| t.reasoning),
        estimated: 0,
        events: sum(|t| t.events),
        total: sum(|t| t.total),
    };
    Overview {
        period: PeriodOrCustom::Day,
        range: Range { start_utc_ms: NOW_MS - 9 * HOUR, end_utc_ms: NOW_MS + 15 * HOUR },
        generated_at_utc_ms: NOW_MS,
        totals: Some(all),
        agents: vec![
            agent("claude", "Claude Code", "CLD", "#ff6a00", Some(claude), Some(5_000)),
            agent("codex", "Codex CLI", "CDX", "#2bb673", Some(codex), Some(400_000)),
            agent("gemini", "Gemini CLI", "GEM", "#4c8bf5", Some(gemini), Some(7_200_000)),
            agent("antigravity", "Antigravity", "AGY", "#b455ff", Some(anti), None),
            agent("ollama", "Ollama", "OLL", "#9aa0a6", None, None),
        ],
        unavailable_agents: 1,
        any_data: true,
        paused: false,
        count_cached_in_total: true,
        importing_agents: vec![],
    }
}

pub fn settings() -> Settings {
    Settings {
        start_with_system: false,
        always_on_top: true,
        overlay_corner: Corner::BottomRight,
        overlay_x: Some(1684),
        overlay_y: Some(815),
        compact_size: None,
        expanded_size: None,
        overlay_opacity: 0.0,
        token_text_size: 1.0,
        compact_mode: true,
        show_overlay_in_taskbar: false,
        polling_interval_secs: 5,
        enabled_agents: vec!["claude".into(), "codex".into(), "gemini".into(), "antigravity".into()],
        project_detection: true,
        paused: false,
        theme: Theme::Dark,
        animation_intensity: AnimationIntensity::Normal,
        show_input: true,
        show_output: true,
        show_cached: true,
        count_cached_in_total: true,
        week_starts_on: 1,
        period: PeriodKey::Day,
        database_location: None,
        retention_days: None,
        shortcut_toggle: "Ctrl+Alt+T".into(),
        shortcut_expand: "Ctrl+Alt+E".into(),
        shortcut_focus: "Ctrl+Alt+F".into(),
    }
}

pub fn app_info() -> AppInfo {
    AppInfo {
        version: "0.1.0".into(),
        os: "windows".into(),
        arch: "x86_64".into(),
        data_dir: r"C:\Users\demo\AppData\Roaming\dev.aiusage.monitor".into(),
        database_path: r"C:\Users\demo\AppData\Roaming\dev.aiusage.monitor\usage.db".into(),
        log_dir: r"C:\Users\demo\AppData\Roaming\dev.aiusage.monitor\logs".into(),
        database_notice: None,
    }
}

pub fn diagnostics() -> OverlayDiagnostics {
    OverlayDiagnostics {
        visible: true,
        minimized: false,
        always_on_top_setting: true,
        decorated: Some(false),
        os_reports_topmost: Some(true),
        x: Some(1684),
        y: Some(815),
        width: Some(236),
        height: Some(208),
        scale_factor: Some(1.25),
        monitors: 1,
        platform_note: "Windows: WS_EX_TOPMOST verified".into(),
    }
}

/// The day the fixture's `NOW_MS` falls on (UTC).
pub fn today() -> NaiveDate {
    chrono::DateTime::from_timestamp_millis(NOW_MS).expect("NOW_MS is a valid time").date_naive()
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Synthetic activity for the heat map: the `days` days ending at `today`, ascending, only days with usage. It has the
/// rhythm of real work (quiet weekends, idle days, busy and slow weeks, more recent activity) and is repeatable; today
/// equals the fixture overview's own total, so the Statistics page agrees with itself.
pub fn daily_totals(today: NaiveDate, days: u32) -> Vec<DayTotal> {
    let today_total = overview().totals.map(|t| t.total).unwrap_or(0);
    let mut out = Vec::new();
    for back in (0..days).rev() {
        let date = today - Duration::days(i64::from(back));
        let total = if back == 0 { today_total } else { synthetic_day(back, date) };
        if total > 0 {
            out.push(DayTotal { date: date.format("%Y-%m-%d").to_string(), total });
        }
    }
    out
}

fn synthetic_day(back: u32, date: NaiveDate) -> u64 {
    let h = splitmix64(u64::from(back));
    let weekend = matches!(date.weekday(), Weekday::Sat | Weekday::Sun);
    if h % 100 < if weekend { 45 } else { 12 } {
        return 0; // an idle day
    }
    let u = ((h >> 8) % 10_000) as f64 / 10_000.0;
    let magnitude = 80_000.0 * 3_750f64.powf(u); // 80K ..= 300M, log-uniform
    let week = f64::from(back / 7);
    let busy = 0.45 + 0.55 * ((week * 0.7).sin() + 1.0) / 2.0; // busy and slow weeks
    let recent = 0.3 + 0.7 * (1.0 - f64::from(back.min(371)) / 371.0); // older days were lighter
    (magnitude * busy * recent).max(1.0) as u64
}

/// `n` gap-free buckets of `bucket_ms` ending at NOW_MS, with a repeatable pseudo-random shape.
pub fn history(period: PeriodOrCustom) -> History {
    let (n, bucket_ms): (usize, i64) = match period {
        PeriodOrCustom::Day => (24, HOUR),
        PeriodOrCustom::Week => (7, DAY),
        PeriodOrCustom::Month => (30, DAY),
        PeriodOrCustom::Year => (12, 30 * DAY),
        PeriodOrCustom::Custom => (14, DAY),
    };
    let start = NOW_MS - n as i64 * bucket_ms;
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let ids = ["claude", "codex", "gemini", "antigravity"];
    let buckets: Vec<TimelineBucket> = (0..n)
        .map(|i| {
            let by_agent: Vec<(String, u64)> = ids
                .iter()
                .filter_map(|id| {
                    let v = next() % 40_000_000;
                    if v % 5 == 0 { None } else { Some((id.to_string(), v)) }
                })
                .collect();
            let total = by_agent.iter().map(|(_, v)| *v).sum();
            TimelineBucket { start_utc_ms: start + i as i64 * bucket_ms, end_utc_ms: start + (i as i64 + 1) * bucket_ms, total, by_agent }
        })
        .collect();
    let sum: u64 = buckets.iter().map(|b| b.total).sum();
    History {
        period,
        range: Range { start_utc_ms: start, end_utc_ms: NOW_MS },
        totals: Some(Totals { total: sum, input: sum / 50, output: sum / 20, cache_read: sum - sum / 50 - sum / 20, ..Totals::default() }),
        buckets,
        by_model: vec![
            ("claude-opus-4".into(), "claude".into(), sum / 2),
            ("gpt-5-codex".into(), "codex".into(), sum / 4),
            ("gemini-2.5-pro".into(), "gemini".into(), sum / 8),
        ],
        by_project: vec![("ai-token".into(), sum / 2), ("website".into(), sum / 3), ("UNKNOWN PROJECT".into(), sum / 6)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_total_is_sum_of_agents() {
        let o = overview();
        let s: u64 = o.agents.iter().filter_map(|a| a.totals.map(|t| t.total)).sum();
        assert_eq!(o.totals.unwrap().total, s);
    }

    #[test]
    fn history_buckets_are_contiguous_and_sum_up() {
        for p in [PeriodOrCustom::Day, PeriodOrCustom::Week, PeriodOrCustom::Month, PeriodOrCustom::Year] {
            let h = history(p);
            assert!(h.buckets.windows(2).all(|w| w[0].end_utc_ms == w[1].start_utc_ms));
            assert!(h.buckets.iter().all(|b| b.by_agent.iter().map(|(_, v)| v).sum::<u64>() == b.total));
        }
    }

    #[test]
    fn the_fixture_heat_map_is_ascending_repeatable_and_ends_with_the_days_own_total() {
        let today = today();
        let days = daily_totals(today, 371);
        assert!(days.len() > 150 && days.len() < 371, "some idle days, mostly busy ones: {}", days.len());
        assert!(days.windows(2).all(|w| w[0].date < w[1].date), "ascending, no duplicates");
        assert!(days.iter().all(|d| d.total > 0), "only days with usage are listed");
        let last = days.last().unwrap();
        assert_eq!(last.date, today.format("%Y-%m-%d").to_string());
        assert_eq!(Some(last.total), overview().totals.map(|t| t.total), "the Statistics page must agree with itself");
        assert_eq!(days, daily_totals(today, 371), "repeatable");
        assert_eq!(daily_totals(today, 1).len(), 1);
        assert!(daily_totals(today, 0).is_empty());
    }

    #[test]
    fn view_types_roundtrip_as_camel_case_json() {
        let o = overview();
        let j = serde_json::to_string(&o).unwrap();
        assert!(j.contains("\"generatedAtUtcMs\"") && j.contains("\"state\":\"ok\""));
        assert_eq!(serde_json::from_str::<Overview>(&j).unwrap(), o);
    }
}
