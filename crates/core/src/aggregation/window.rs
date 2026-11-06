//! The current usage window of an agent that limits usage per rolling window (Claude Code: 5 hours).
//!
//! **This is an estimate, not the provider's number.** Claude Code does not write how much of the limit is used to
//! disk (it logs a `five_hour` rate-limit record only when a request is *rejected*), and the limit itself is not a
//! token count. What the local data does support is the *shape* of the window: it opens with the first request after
//! the previous window ended and closes 5 hours later. Checked against the one real rejection on the development
//! machine: the first request after a quiet spell was at 04:14:48 UTC, the model predicted a reset at 09:14:48, the
//! provider's own `resetsAt` was 09:10:00.
//!
//! Only this machine's events are visible. Usage on claude.ai or on another computer counts toward the same limit.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::database::queries::Counters;
use crate::error::Result;
use crate::settings::Settings;

/// Length of the window.
pub const WINDOW_MS: i64 = 5 * 3_600_000;
/// How far back the window chain is rebuilt from. The phase of the windows is fixed by the last quiet spell of 5 hours
/// or more, so a week is far more than needed.
const LOOKBACK_MS: i64 = 7 * 24 * 3_600_000;

/// An open window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// The request that opened it.
    pub start_utc_ms: i64,
    /// `start + 5 h`: when the provider starts counting afresh.
    pub resets_utc_ms: i64,
    /// Tokens used inside the window, counted like the overview counts them (cached tokens only if the setting is on).
    pub tokens: u64,
    pub events: u64,
}

/// The window `agent` is in at `now_utc_ms`, or `None` when none is open (its last window ended, or it has no usage in
/// the last week) or the agent is turned off.
pub fn current_window(conn: &Connection, settings: &Settings, agent: &str, now_utc_ms: i64) -> Result<Option<UsageWindow>> {
    if !settings.agent_enabled(agent) {
        return Ok(None);
    }
    let from = now_utc_ms.saturating_sub(LOOKBACK_MS);

    // Events in the future (a clock that jumped) must not open a window; they are ignored.
    let mut st = conn.prepare_cached("SELECT ts_utc_ms FROM usage_events WHERE agent_id = ?1 AND ts_utc_ms >= ?2 AND ts_utc_ms <= ?3 ORDER BY ts_utc_ms")?;
    let rows = st.query_map(params![agent, from, now_utc_ms], |r| r.get::<_, i64>(0))?;
    let mut start: Option<i64> = None;
    for ts in rows {
        let ts = ts?;
        if start.is_none_or(|s| ts >= s.saturating_add(WINDOW_MS)) {
            start = Some(ts);
        }
    }
    let Some(start) = start else { return Ok(None) };
    let resets = start.saturating_add(WINDOW_MS);
    if now_utc_ms >= resets {
        return Ok(None);
    }

    let c = conn.query_row(
        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_write_tokens), 0), COUNT(*)
         FROM usage_events WHERE agent_id = ?1 AND ts_utc_ms >= ?2 AND ts_utc_ms <= ?3",
        params![agent, start, now_utc_ms],
        |r| {
            let u = |i: usize| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
            Ok(Counters { input: u(0)?, output: u(1)?, cache_read: u(2)?, cache_write: u(3)?, events: u(4)?, ..Counters::default() })
        },
    )?;
    let tokens = if settings.count_cached_in_total { c.total_all() } else { c.total_excluding_cache() };
    Ok(Some(UsageWindow { start_utc_ms: start, resets_utc_ms: resets, tokens, events: c.events }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::database::Batch;
    use crate::model::{Accuracy, UsageEvent};
    use chrono::{TimeZone, Utc};

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

    /// UTC time on 2026-11-03 as unix milliseconds.
    fn at(h: u32, m: u32, s: u32) -> i64 {
        Utc.with_ymd_and_hms(2026, 11, 3, h, m, s).unwrap().timestamp_millis()
    }

    fn window(events: Vec<UsageEvent>, settings: &Settings, now: i64) -> Option<UsageWindow> {
        let (_d, db) = temp_db();
        db.commit(&Batch { agent: "claude", events, cursors: vec![] }).unwrap();
        db.with_reader(|c| current_window(c, settings, "claude", now)).unwrap()
    }

    #[test]
    fn no_usage_means_no_window() {
        assert_eq!(window(vec![], &Settings::default(), at(12, 0, 0)), None);
    }

    #[test]
    fn the_first_request_after_a_quiet_spell_opens_a_window_that_lasts_five_hours() {
        // The shape seen on the real machine: activity from 04:14:48, then the window closes at 09:14:48.
        let evs = vec![ev("claude", "a", at(4, 14, 48), 100, 10, 0), ev("claude", "b", at(5, 30, 0), 200, 20, 0), ev("claude", "c", at(7, 45, 34), 300, 30, 0)];
        let w = window(evs.clone(), &Settings::default(), at(8, 0, 0)).unwrap();
        assert_eq!((w.start_utc_ms, w.resets_utc_ms), (at(4, 14, 48), at(9, 14, 48)));
        assert_eq!((w.tokens, w.events), (660, 3));
        // Once the five hours are over there is no open window, however recent the last request was.
        assert_eq!(window(evs, &Settings::default(), at(9, 14, 48)), None);
    }

    #[test]
    fn a_request_after_the_window_ended_opens_the_next_one_even_without_a_quiet_spell() {
        // Steady work every 2 hours: windows open at 00:00, 05:00 (the first event at or after 05:00 is 06:00), ...
        let evs = vec![
            ev("claude", "a", at(0, 0, 0), 10, 0, 0),
            ev("claude", "b", at(2, 0, 0), 10, 0, 0),
            ev("claude", "c", at(4, 0, 0), 10, 0, 0),
            ev("claude", "d", at(6, 0, 0), 10, 0, 0),
            ev("claude", "e", at(8, 0, 0), 10, 0, 0),
        ];
        let w = window(evs, &Settings::default(), at(8, 30, 0)).unwrap();
        assert_eq!(w.start_utc_ms, at(6, 0, 0), "the window that opened at 00:00 ended at 05:00; 06:00 opened the next");
        assert_eq!((w.tokens, w.events), (20, 2), "only the events inside the open window count");
    }

    #[test]
    fn a_request_exactly_at_the_end_belongs_to_the_next_window() {
        let evs = vec![ev("claude", "a", at(0, 0, 0), 10, 0, 0), ev("claude", "b", at(5, 0, 0), 5, 0, 0)];
        let w = window(evs, &Settings::default(), at(5, 1, 0)).unwrap();
        assert_eq!((w.start_utc_ms, w.tokens), (at(5, 0, 0), 5));
    }

    #[test]
    fn cached_tokens_follow_the_setting_like_the_overview() {
        let evs = vec![ev("claude", "a", at(1, 0, 0), 100, 10, 50)];
        let with = window(evs.clone(), &Settings { count_cached_in_total: true, ..Settings::default() }, at(2, 0, 0)).unwrap();
        let without = window(evs, &Settings { count_cached_in_total: false, ..Settings::default() }, at(2, 0, 0)).unwrap();
        assert_eq!((with.tokens, without.tokens), (160, 110));
    }

    #[test]
    fn other_agents_do_not_open_or_fill_the_window() {
        let evs = vec![ev("codex", "a", at(1, 0, 0), 999, 0, 0)];
        assert_eq!(window(evs, &Settings::default(), at(2, 0, 0)), None);
    }

    #[test]
    fn a_turned_off_agent_has_no_window() {
        let mut s = Settings::default();
        s.enabled_agents.retain(|a| a != "claude");
        assert_eq!(window(vec![ev("claude", "a", at(1, 0, 0), 1, 0, 0)], &s, at(2, 0, 0)), None);
    }

    #[test]
    fn events_from_the_future_are_ignored() {
        // A clock that jumped ahead once must not leave a window open for hours.
        let evs = vec![ev("claude", "a", at(20, 0, 0), 1, 0, 0)];
        assert_eq!(window(evs, &Settings::default(), at(2, 0, 0)), None);
    }

    #[test]
    fn usage_older_than_the_look_back_is_not_a_window() {
        let evs = vec![ev("claude", "a", at(1, 0, 0) - 8 * 24 * 3_600_000, 1, 0, 0)];
        assert_eq!(window(evs, &Settings::default(), at(1, 30, 0)), None);
    }
}
