//! Local-time period boundaries. Data is stored in UTC; "today/this week/…" are *local* calendar
//! periods, converted to UTC instants here and nowhere else.

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Week,
    Month,
    Year,
    Custom,
}

/// Half-open UTC interval `[start, end)` in unix milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
}

/// First valid instant of a local date. Handles zones where local midnight does not exist (DST gap).
pub fn local_midnight<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> DateTime<Utc> {
    for half_hours in 0..8 {
        let naive = date.and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(30 * half_hours);
        if let Some(dt) = tz.from_local_datetime(&naive).earliest() {
            return dt.with_timezone(&Utc);
        }
    }
    // Unreachable for real zones; fall back to treating the date as UTC.
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
}

fn first_of_next_month(d: NaiveDate) -> NaiveDate {
    if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
    }
}

/// `week_start`: 0 = Sunday … 6 = Saturday (default 1 = Monday).
pub fn range_for<Tz: TimeZone>(tz: &Tz, now: DateTime<Utc>, period: Period, week_start: u8) -> Range {
    let today = now.with_timezone(tz).date_naive();
    let (start_date, end_date) = match period {
        Period::Day | Period::Custom => (today, today + Duration::days(1)),
        Period::Week => {
            let from_sunday = i64::from(today.weekday().num_days_from_sunday());
            let back = (from_sunday - i64::from(week_start % 7)).rem_euclid(7);
            let start = today - Duration::days(back);
            (start, start + Duration::days(7))
        }
        Period::Month => {
            let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
            (start, first_of_next_month(start))
        }
        Period::Year => (
            NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap(),
        ),
    };
    Range {
        start_utc_ms: local_midnight(tz, start_date).timestamp_millis(),
        end_utc_ms: local_midnight(tz, end_date).timestamp_millis(),
    }
}

/// Inclusive local dates → half-open UTC range. Returns `None` when `to < from`.
pub fn custom_range<Tz: TimeZone>(tz: &Tz, from: NaiveDate, to_inclusive: NaiveDate) -> Option<Range> {
    if to_inclusive < from {
        return None;
    }
    Some(Range {
        start_utc_ms: local_midnight(tz, from).timestamp_millis(),
        end_utc_ms: local_midnight(tz, to_inclusive + Duration::days(1)).timestamp_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 1800).unwrap()
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn iso(ms: i64) -> String {
        DateTime::<Utc>::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    #[test]
    fn day_in_india_is_not_a_utc_day() {
        // 2026-10-07 20:00Z is already 2026-10-08 01:30 in IST.
        let r = range_for(&ist(), at("2026-10-07T20:00:00Z"), Period::Day, 1);
        assert_eq!(iso(r.start_utc_ms), "2026-10-07T18:30:00Z");
        assert_eq!(iso(r.end_utc_ms), "2026-10-08T18:30:00Z");
    }

    #[test]
    fn day_boundaries_are_bucket_aligned_for_half_hour_zones() {
        let r = range_for(&ist(), at("2026-10-07T20:00:00Z"), Period::Day, 1);
        assert_eq!((r.start_utc_ms / 1000) % 900, 0);
        let nepal = FixedOffset::east_opt(5 * 3600 + 45 * 60).unwrap();
        let r = range_for(&nepal, at("2026-10-07T20:00:00Z"), Period::Day, 1);
        assert_eq!((r.start_utc_ms / 1000) % 900, 0);
    }

    #[test]
    fn week_starts_on_monday_by_default() {
        // 2026-10-08 is a Thursday -> week of Monday 2026-10-05 (IST).
        let r = range_for(&ist(), at("2026-10-08T06:00:00Z"), Period::Week, 1);
        assert_eq!(iso(r.start_utc_ms), "2026-10-04T18:30:00Z"); // Mon 10-05 00:00 IST
        assert_eq!(iso(r.end_utc_ms), "2026-10-11T18:30:00Z");
    }

    #[test]
    fn week_start_is_configurable() {
        // Sunday-start: Thursday 2026-10-08 -> Sunday 2026-10-04.
        let r = range_for(&ist(), at("2026-10-08T06:00:00Z"), Period::Week, 0);
        assert_eq!(iso(r.start_utc_ms), "2026-10-03T18:30:00Z");
    }

    #[test]
    fn week_on_the_week_start_day_begins_today() {
        // Monday 2026-10-05.
        let r = range_for(&ist(), at("2026-10-05T06:00:00Z"), Period::Week, 1);
        assert_eq!(iso(r.start_utc_ms), "2026-10-04T18:30:00Z");
    }

    #[test]
    fn month_and_year_span_calendar_boundaries() {
        let m = range_for(&ist(), at("2026-12-15T06:00:00Z"), Period::Month, 1);
        assert_eq!(iso(m.start_utc_ms), "2026-11-30T18:30:00Z");
        assert_eq!(iso(m.end_utc_ms), "2026-12-31T18:30:00Z");
        let y = range_for(&ist(), at("2026-12-15T06:00:00Z"), Period::Year, 1);
        assert_eq!(iso(y.start_utc_ms), "2025-12-31T18:30:00Z");
        assert_eq!(iso(y.end_utc_ms), "2026-12-31T18:30:00Z");
    }

    #[test]
    fn utc_zone_matches_naive_expectations() {
        let r = range_for(&Utc, at("2026-02-28T23:59:59Z"), Period::Month, 1);
        assert_eq!(iso(r.start_utc_ms), "2026-02-01T00:00:00Z");
        assert_eq!(iso(r.end_utc_ms), "2026-03-01T00:00:00Z");
    }

    #[test]
    fn negative_offsets_work() {
        let la = FixedOffset::west_opt(7 * 3600).unwrap();
        let r = range_for(&la, at("2026-10-07T03:00:00Z"), Period::Day, 1); // still Oct 6 in LA
        assert_eq!(iso(r.start_utc_ms), "2026-10-06T07:00:00Z");
    }

    #[test]
    fn custom_range_is_inclusive_of_the_last_day() {
        let r = custom_range(
            &Utc,
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        )
        .unwrap();
        assert_eq!(iso(r.start_utc_ms), "2026-10-01T00:00:00Z");
        assert_eq!(iso(r.end_utc_ms), "2026-10-04T00:00:00Z");
        assert!(custom_range(&Utc, NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(), NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()).is_none());
    }
}
