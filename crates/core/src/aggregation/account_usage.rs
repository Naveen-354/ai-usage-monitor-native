//! Usage per account for the five spans the UI and `agm usage` show: today, this week, this month, this year and lifetime.
//!
//! Spans are *local* calendar periods (data is stored in UTC), exactly the ones the overview uses, and totals honour the same
//! "count cached tokens" setting, so a per-account number can always be added up to the per-agent number beside it.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::overview::Totals;
use super::periods::{local_midnight, range_for};
use super::{Period, Range};
use crate::database::queries::{self, Counters};
use crate::error::Result;
use crate::settings::Settings;

/// One account's usage over every span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpanTotals {
    pub day: Totals,
    pub week: Totals,
    pub month: Totals,
    pub year: Totals,
    pub lifetime: Totals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsage {
    pub agent_id: String,
    pub account_id: String,
    pub totals: SpanTotals,
}

/// The span that starts at the very beginning and ends at the end of the current local day.
fn lifetime_range<Tz: TimeZone>(tz: &Tz, now: DateTime<Utc>) -> Range {
    let tomorrow = now.with_timezone(tz).date_naive() + Duration::days(1);
    Range { start_utc_ms: 0, end_utc_ms: local_midnight(tz, tomorrow).timestamp_millis() }
}

/// Usage of every account that has any, in the five spans. Accounts without any usage are simply absent: the caller fills in
/// the zero for the accounts it lists (a zero there is real: the account exists and has not been used).
pub fn account_usage<Tz: TimeZone>(conn: &Connection, settings: &Settings, tz: &Tz, now: DateTime<Utc>) -> Result<Vec<AccountUsage>> {
    let week_start = settings.week_starts_on;
    let spans = [
        range_for(tz, now, Period::Day, week_start),
        range_for(tz, now, Period::Week, week_start),
        range_for(tz, now, Period::Month, week_start),
        range_for(tz, now, Period::Year, week_start),
        lifetime_range(tz, now),
    ];
    let per_span: Vec<_> = spans.iter().map(|r| queries::totals_by_account(conn, r.start_utc_ms, r.end_utc_ms)).collect::<Result<_>>()?;

    // Only enabled agents count, as in the overview.
    let keys: BTreeSet<(String, String)> = per_span.iter().flat_map(|m| m.keys().cloned()).filter(|(agent, _)| settings.agent_enabled(agent)).collect();
    let totals = |key: &(String, String), i: usize| -> Totals { Totals::from_counters(&per_span[i].get(key).copied().unwrap_or_else(Counters::default), settings.count_cached_in_total) };
    Ok(keys
        .iter()
        .map(|key| AccountUsage {
            agent_id: key.0.clone(),
            account_id: key.1.clone(),
            totals: SpanTotals { day: totals(key, 0), week: totals(key, 1), month: totals(key, 2), year: totals(key, 3), lifetime: totals(key, 4) },
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::database::Batch;
    use crate::model::{Accuracy, UsageEvent};
    use chrono::FixedOffset;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn ev(agent: &'static str, key: &str, at: DateTime<Utc>, input: u64, cached: u64) -> UsageEvent {
        UsageEvent {
            agent,
            model: "m".into(),
            ts_utc_ms: at.timestamp_millis(),
            input_tokens: input,
            output_tokens: 0,
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

    fn settings(count_cached: bool) -> Settings {
        Settings { count_cached_in_total: count_cached, week_starts_on: 1, ..Settings::default() }
    }

    fn find<'a>(all: &'a [AccountUsage], agent: &str, account: &str) -> &'a AccountUsage {
        all.iter().find(|u| u.agent_id == agent && u.account_id == account).unwrap_or_else(|| panic!("no usage for {agent}/{account}"))
    }

    fn utc_plus_0() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// "Now" is Wednesday 2026-10-07 12:00 UTC. Events: this morning, Monday (this week), Oct 1 (this month), Mar 3 (this
    /// year), and last year.
    fn seeded() -> (tempfile::TempDir, std::sync::Arc<crate::database::Database>, DateTime<Utc>) {
        let (d, db) = temp_db();
        let now = utc(2026, 10, 7, 12);
        let work = [("w-today", utc(2026, 10, 7, 8), 1), ("w-mon", utc(2026, 10, 5, 8), 10), ("w-oct", utc(2026, 10, 1, 8), 100), ("w-mar", utc(2026, 3, 3, 8), 1_000), ("w-old", utc(2025, 6, 1, 8), 10_000)];
        let events: Vec<_> = work.iter().map(|(k, t, n)| ev("codex", k, *t, *n, 0)).collect();
        db.commit_for("work", &Batch { agent: "codex", events, cursors: vec![] }).unwrap();
        db.commit_for("default", &Batch { agent: "codex", events: vec![ev("codex", "d-today", utc(2026, 10, 7, 9), 5, 0)], cursors: vec![] }).unwrap();
        (d, db, now)
    }

    #[test]
    fn every_account_gets_its_own_day_week_month_year_and_lifetime() {
        let (_d, db, now) = seeded();
        let all = db.with_reader(|c| account_usage(c, &settings(true), &utc_plus_0(), now)).unwrap();
        let w = find(&all, "codex", "work").totals;
        assert_eq!(w.day.total, 1);
        assert_eq!(w.week.total, 11, "Monday + today");
        assert_eq!(w.month.total, 111);
        assert_eq!(w.year.total, 1_111);
        assert_eq!(w.lifetime.total, 11_111);
        let d = find(&all, "codex", "default").totals;
        assert_eq!((d.day.total, d.week.total, d.lifetime.total), (5, 5, 5));
    }

    #[test]
    fn the_accounts_add_up_to_the_agents_total_for_every_span() {
        let (_d, db, now) = seeded();
        let s = settings(true);
        let all = db.with_reader(|c| account_usage(c, &s, &utc_plus_0(), now)).unwrap();
        let sum = |f: fn(&SpanTotals) -> u64| all.iter().filter(|u| u.agent_id == "codex").map(|u| f(&u.totals)).sum::<u64>();
        let overview_total = |period| {
            let range = range_for(&utc_plus_0(), now, period, 1);
            db.with_reader(|c| queries::totals_by_agent(c, range.start_utc_ms, range.end_utc_ms)).unwrap()["codex"].total_all()
        };
        assert_eq!(sum(|t| t.day.total), overview_total(Period::Day));
        assert_eq!(sum(|t| t.week.total), overview_total(Period::Week));
        assert_eq!(sum(|t| t.month.total), overview_total(Period::Month));
        assert_eq!(sum(|t| t.year.total), overview_total(Period::Year));
    }

    #[test]
    fn spans_are_local_days_not_utc_days() {
        let (d, db) = temp_db();
        let _keep = d;
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        // 20:00 UTC on Oct 6 is 01:30 on Oct 7 in India: it is *today's* usage there, yesterday's in UTC.
        db.commit_for("work", &Batch { agent: "codex", events: vec![ev("codex", "late", utc(2026, 10, 6, 20), 9, 0)], cursors: vec![] }).unwrap();
        let now = utc(2026, 10, 7, 4); // 09:30 IST on Oct 7
        let india = db.with_reader(|c| account_usage(c, &settings(true), &ist, now)).unwrap();
        assert_eq!(find(&india, "codex", "work").totals.day.total, 9);
        let london = db.with_reader(|c| account_usage(c, &settings(true), &utc_plus_0(), now)).unwrap();
        assert_eq!(find(&london, "codex", "work").totals.day.total, 0);
        assert_eq!(find(&london, "codex", "work").totals.lifetime.total, 9);
    }

    #[test]
    fn cached_tokens_follow_the_setting_in_every_span() {
        let (d, db) = temp_db();
        let _keep = d;
        let now = utc(2026, 10, 7, 12);
        db.commit_for("work", &Batch { agent: "codex", events: vec![ev("codex", "e", utc(2026, 10, 7, 8), 10, 90)], cursors: vec![] }).unwrap();
        let with = db.with_reader(|c| account_usage(c, &settings(true), &utc_plus_0(), now)).unwrap();
        let without = db.with_reader(|c| account_usage(c, &settings(false), &utc_plus_0(), now)).unwrap();
        assert_eq!(find(&with, "codex", "work").totals.lifetime.total, 100);
        assert_eq!(find(&without, "codex", "work").totals.lifetime.total, 10);
        assert_eq!(find(&without, "codex", "work").totals.lifetime.cache_read, 90, "the breakdown is unaffected");
    }

    #[test]
    fn disabled_agents_and_accounts_without_usage_are_absent_not_zero() {
        let (_d, db, now) = seeded();
        let mut s = settings(true);
        s.enabled_agents = vec!["claude".into()];
        assert!(db.with_reader(|c| account_usage(c, &s, &utc_plus_0(), now)).unwrap().is_empty(), "codex is disabled");
        let s = settings(true);
        assert!(db.with_reader(|c| account_usage(c, &s, &utc_plus_0(), now)).unwrap().iter().all(|u| u.agent_id == "codex"));
    }

    #[test]
    fn a_removed_accounts_history_is_still_listed_so_nothing_vanishes_from_the_totals() {
        let (_d, db, now) = seeded();
        let all = db.with_reader(|c| account_usage(c, &settings(true), &utc_plus_0(), now)).unwrap();
        assert_eq!(all.len(), 2, "usage is keyed by account id, whether or not the account still exists");
    }
}
