//! The old app's display formats (`src/lib/format.ts`).

use chrono::{Local, TimeZone};

/// `formatRelative`: "just now", "42s ago", "7m ago", "5h ago", "3d ago".
pub fn relative(then_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - then_ms) as f64 / 1000.0).round().max(0.0) as i64;
    if s < 5 {
        return "just now".into();
    }
    if s < 60 {
        return format!("{s}s ago");
    }
    let m = (s as f64 / 60.0).round() as i64;
    if m < 60 {
        return format!("{m}m ago");
    }
    let h = (m as f64 / 60.0).round() as i64;
    if h < 48 {
        return format!("{h}h ago");
    }
    format!("{}d ago", (h as f64 / 24.0).round() as i64)
}

/// `formatDateTime` (en-US): "Oct 08, 2026, 09:58 AM" in the local time zone.
pub fn date_time(utc_ms: i64) -> String {
    match Local.timestamp_millis_opt(utc_ms).single() {
        Some(t) => t.format("%b %d, %Y, %I:%M %p").to_string(),
        None => "—".into(),
    }
}

/// `formatFull`: 1,823,982.
pub fn full(n: u64) -> String {
    crate::motion::format::format_grouped(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_times_follow_the_web_thresholds() {
        let now = 1_000_000_000;
        let ago = |s: i64| relative(now - s * 1000, now);
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(4), "just now");
        assert_eq!(ago(5), "5s ago");
        assert_eq!(ago(59), "59s ago");
        assert_eq!(ago(60), "1m ago");
        assert_eq!(ago(3_540), "59m ago");
        assert_eq!(ago(3_599), "1h ago", "rounds up to an hour, as the web version does");
        assert_eq!(ago(7_200), "2h ago");
        assert_eq!(ago(47 * 3600), "47h ago");
        assert_eq!(ago(48 * 3600), "2d ago");
        assert_eq!(ago(76 * 86_400), "76d ago");
        assert_eq!(relative(now + 5_000, now), "just now", "an event from the future is not negative");
    }

    #[test]
    fn date_time_is_month_day_year_and_a_12_hour_clock() {
        let ms = Local.with_ymd_and_hms(2026, 10, 8, 9, 58, 0).unwrap().timestamp_millis();
        assert_eq!(date_time(ms), "Oct 08, 2026, 09:58 AM");
        let pm = Local.with_ymd_and_hms(2026, 1, 3, 21, 5, 0).unwrap().timestamp_millis();
        assert_eq!(date_time(pm), "Jan 03, 2026, 09:05 PM");
        assert_eq!(date_time(i64::MAX), "—");
    }

    #[test]
    fn full_numbers_are_grouped() {
        assert_eq!(full(1_823_982), "1,823,982");
        assert_eq!(full(0), "0");
    }
}
