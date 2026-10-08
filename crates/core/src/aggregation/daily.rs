//! Per-day totals for the activity heat map: one number per local calendar day, from the 15-minute rollup.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::periods::local_midnight;
use crate::error::Result;
use crate::settings::Settings;

/// Most days a single call may ask for (ten years); keeps one query bounded.
pub const MAX_DAYS: u32 = 3660;

/// Tokens used on one local calendar day. `date` serialises as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DayTotal {
    pub date: NaiveDate,
    pub total: u64,
}

/// Totals for the `days` local days ending at (and including) `today`, ascending, **only days that had usage**: a day
/// missing from the result had none. Counts what the overview counts: enabled agents only, and cached tokens only when
/// the "count cached tokens in total" setting is on, so the heat map never disagrees with the numbers beside it.
///
/// A 15-minute bucket never straddles local midnight (15 minutes divides every UTC offset), so a bucket belongs to
/// exactly one local day.
pub fn daily_totals<Tz: TimeZone>(conn: &Connection, settings: &Settings, tz: &Tz, today: NaiveDate, days: u32) -> Result<Vec<DayTotal>> {
    let days = days.clamp(1, MAX_DAYS);
    let first = today - Duration::days(i64::from(days) - 1);
    let start_s = local_midnight(tz, first).timestamp();
    let end_s = local_midnight(tz, today + Duration::days(1)).timestamp();

    let mut st = conn.prepare_cached(
        "SELECT bucket_utc_s, agent_id, SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens)
         FROM usage_buckets WHERE bucket_utc_s >= ?1 AND bucket_utc_s < ?2 GROUP BY bucket_utc_s, agent_id",
    )?;
    let rows = st.query_map(params![start_s, end_s], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?))
    })?;

    let unsigned = |v: i64| v.max(0) as u64;
    let mut per_day: BTreeMap<NaiveDate, u64> = BTreeMap::new();
    for row in rows {
        let (bucket_s, agent, input, output, cache_read, cache_write) = row?;
        if !settings.agent_enabled(&agent) {
            continue;
        }
        let mut total = unsigned(input).saturating_add(unsigned(output));
        if settings.count_cached_in_total {
            total = total.saturating_add(unsigned(cache_read)).saturating_add(unsigned(cache_write));
        }
        if total == 0 {
            continue;
        }
        let Some(local) = tz.timestamp_opt(bucket_s, 0).single() else { continue };
        let day = per_day.entry(local.date_naive()).or_default();
        *day = day.saturating_add(total);
    }
    Ok(per_day.into_iter().map(|(date, total)| DayTotal { date, total }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::database::Batch;
    use crate::model::{Accuracy, UsageEvent};
    use chrono::{FixedOffset, Utc};

    fn ev(agent: &'static str, key: &str, ts: i64, input: u64, output: u64, cached: u64) -> UsageEvent {
        UsageEvent {
            agent,
            model: "m".into(),
            ts_utc_ms: ts,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cached,
            cache_write_tokens: 0,
            reasoning_tokens: None,
            session_id: None,
            project: None,
            source: "t",
            accuracy: Accuracy::Real,
            dedupe_key: key.into(),
        }
    }

    /// India: UTC+5:30, the offset that makes "local day" differ most visibly from the UTC day.
    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 1800).unwrap()
    }

    fn utc_ms(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp_millis()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn settings(enabled: &[&str], count_cached: bool) -> Settings {
        Settings { enabled_agents: enabled.iter().map(|s| s.to_string()).collect(), count_cached_in_total: count_cached, ..Settings::default() }
    }

    #[test]
    fn an_empty_database_has_no_days_with_usage() {
        let (_d, db) = temp_db();
        let got = db.with_reader(|c| daily_totals(c, &Settings::default(), &ist(), day(2026, 10, 8), 371)).unwrap();
        assert!(got.is_empty(), "no usage means no entries, not a list of zeros");
    }

    #[test]
    fn usage_lands_on_the_local_day_not_the_utc_day() {
        let (_d, db) = temp_db();
        // 20:00 UTC on Oct 6 is 01:30 on Oct 7 in India; 18:29 UTC on Oct 6 is 23:59 on Oct 6.
        db.commit(&Batch {
            agent: "codex",
            events: vec![ev("codex", "a", utc_ms(2026, 10, 6, 20, 0), 100, 10, 0), ev("codex", "b", utc_ms(2026, 10, 6, 18, 29), 7, 3, 0)],
            cursors: vec![],
        })
        .unwrap();
        let s = settings(&["codex"], true);
        let got = db.with_reader(|c| daily_totals(c, &s, &ist(), day(2026, 10, 8), 10)).unwrap();
        assert_eq!(got, vec![DayTotal { date: day(2026, 10, 6), total: 10 }, DayTotal { date: day(2026, 10, 7), total: 110 }]);
    }

    #[test]
    fn days_are_summed_across_agents_and_come_back_ascending() {
        let (_d, db) = temp_db();
        db.commit(&Batch {
            agent: "codex",
            events: vec![
                ev("codex", "a", utc_ms(2026, 10, 7, 6, 0), 100, 0, 0),
                ev("claude", "b", utc_ms(2026, 10, 7, 7, 0), 200, 0, 0),
                ev("codex", "c", utc_ms(2026, 10, 5, 6, 0), 5, 0, 0),
            ],
            cursors: vec![],
        })
        .unwrap();
        let s = settings(&["codex", "claude"], true);
        let got = db.with_reader(|c| daily_totals(c, &s, &ist(), day(2026, 10, 8), 10)).unwrap();
        assert_eq!(got, vec![DayTotal { date: day(2026, 10, 5), total: 5 }, DayTotal { date: day(2026, 10, 7), total: 300 }]);
    }

    #[test]
    fn it_counts_what_the_overview_counts() {
        let (_d, db) = temp_db();
        db.commit(&Batch {
            agent: "codex",
            events: vec![ev("codex", "a", utc_ms(2026, 10, 7, 6, 0), 100, 10, 50), ev("gemini", "b", utc_ms(2026, 10, 7, 6, 5), 1000, 0, 0)],
            cursors: vec![],
        })
        .unwrap();
        let with_cache = db.with_reader(|c| daily_totals(c, &settings(&["codex"], true), &ist(), day(2026, 10, 8), 5)).unwrap();
        assert_eq!(with_cache, vec![DayTotal { date: day(2026, 10, 7), total: 160 }], "gemini is not enabled; cached tokens counted");
        let without = db.with_reader(|c| daily_totals(c, &settings(&["codex"], false), &ist(), day(2026, 10, 8), 5)).unwrap();
        assert_eq!(without, vec![DayTotal { date: day(2026, 10, 7), total: 110 }], "cached tokens left out");
    }

    #[test]
    fn only_the_requested_window_is_returned_and_today_is_included() {
        let (_d, db) = temp_db();
        db.commit(&Batch {
            agent: "codex",
            events: vec![
                ev("codex", "old", utc_ms(2026, 9, 1, 6, 0), 1, 0, 0),
                ev("codex", "edge", utc_ms(2026, 10, 3, 6, 0), 2, 0, 0),
                ev("codex", "today", utc_ms(2026, 10, 8, 6, 0), 4, 0, 0),
                ev("codex", "tomorrow", utc_ms(2026, 10, 9, 6, 0), 8, 0, 0),
            ],
            cursors: vec![],
        })
        .unwrap();
        let s = settings(&["codex"], true);
        // 6 days ending Oct 8 = Oct 3 ..= Oct 8
        let got = db.with_reader(|c| daily_totals(c, &s, &ist(), day(2026, 10, 8), 6)).unwrap();
        assert_eq!(got, vec![DayTotal { date: day(2026, 10, 3), total: 2 }, DayTotal { date: day(2026, 10, 8), total: 4 }]);
    }

    #[test]
    fn a_day_count_of_zero_or_absurd_is_clamped_instead_of_failing() {
        let (_d, db) = temp_db();
        assert!(db.with_reader(|c| daily_totals(c, &Settings::default(), &ist(), day(2026, 10, 8), 0)).unwrap().is_empty());
        assert!(db.with_reader(|c| daily_totals(c, &Settings::default(), &ist(), day(2026, 10, 8), u32::MAX)).unwrap().is_empty());
    }

    #[test]
    fn a_day_total_serialises_with_an_iso_date() {
        let json = serde_json::to_value(DayTotal { date: day(2026, 10, 8), total: 42 }).unwrap();
        assert_eq!(json, serde_json::json!({"date": "2026-10-08", "total": 42}));
    }
}
