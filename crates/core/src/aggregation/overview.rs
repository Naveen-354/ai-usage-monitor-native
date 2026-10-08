//! The one DTO the overlay and dashboard render from. It encodes the zero-vs-unknown rule:
//! an agent whose numbers cannot be trusted has `totals: None`, which the UI prints as
//! `TOKEN DATA UNAVAILABLE` — never as `0`.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{Period, Range};
use crate::database::queries::{self, Counters};
use crate::error::Result;
use crate::model::{catalog, Availability, CollectorHealth};
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub estimated: u64,
    pub events: u64,
    /// Headline total, honouring the "count cached tokens in total" setting.
    pub total: u64,
}

impl Totals {
    /// Counters as the UI shows them: the headline total honours the "count cached tokens" setting.
    pub fn from_counters(c: &Counters, count_cached: bool) -> Totals {
        Totals::from(c, count_cached)
    }

    fn from(c: &Counters, count_cached: bool) -> Totals {
        Totals {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
            reasoning: c.reasoning,
            estimated: c.estimated,
            events: c.events,
            total: if count_cached { c.total_all() } else { c.total_excluding_cache() },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverview {
    pub id: String,
    pub name: String,
    pub short: String,
    pub color: String,
    pub enabled: bool,
    pub availability: Availability,
    pub note: Option<String>,
    /// `None` = do not show numbers.
    pub totals: Option<Totals>,
    pub last_event_utc_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub period: Period,
    pub range: Range,
    pub generated_at_utc_ms: i64,
    /// Sum over agents that have numbers; `None` when no agent has any.
    pub totals: Option<Totals>,
    pub agents: Vec<AgentOverview>,
    pub unavailable_agents: u32,
    pub any_data: bool,
    pub paused: bool,
    pub count_cached_in_total: bool,
    /// Agents whose history is being imported for the first time right now (filled in by the command layer).
    pub importing_agents: Vec<String>,
}

pub fn build_overview(
    conn: &Connection,
    settings: &Settings,
    period: Period,
    range: Range,
    now_utc_ms: i64,
) -> Result<Overview> {
    let totals = queries::totals_by_agent(conn, range.start_utc_ms, range.end_utc_ms)?;
    let health: HashMap<String, CollectorHealth> =
        queries::load_health(conn)?.into_iter().map(|h| (h.agent.clone(), h)).collect();
    let last_event = queries::last_event_by_agent(conn)?;
    let count_cached = settings.count_cached_in_total;

    let mut agents = Vec::new();
    let mut sum = Counters::default();
    let mut any_numbers = false;
    let mut unavailable = 0u32;

    for meta in catalog() {
        let h = health.get(meta.id);
        let availability = h.map(|h| h.availability.clone()).unwrap_or_else(|| Availability::Unavailable {
            reason: "collector has not run yet".into(),
        });
        let enabled = settings.agent_enabled(meta.id);
        let stored = totals.get(meta.id);

        // Numbers are shown when the collector vouches for them, or when history already exists in
        // the database (e.g. an agent that was later uninstalled). Otherwise: unavailable, not zero.
        let show = enabled && (availability.has_numbers() || stored.is_some());
        let counters = if show { Some(stored.copied().unwrap_or_default()) } else { None };
        if enabled {
            match &counters {
                Some(c) => {
                    sum.add(c);
                    any_numbers = true;
                }
                None => unavailable += 1,
            }
        }
        agents.push(AgentOverview {
            id: meta.id.into(),
            name: meta.name.into(),
            short: meta.short.into(),
            color: meta.color.into(),
            enabled,
            availability,
            note: h.and_then(|h| h.note.clone()),
            totals: counters.as_ref().map(|c| Totals::from(c, count_cached)),
            last_event_utc_ms: last_event.get(meta.id).copied(),
        });
    }

    Ok(Overview {
        period,
        range,
        generated_at_utc_ms: now_utc_ms,
        totals: any_numbers.then(|| Totals::from(&sum, count_cached)),
        agents,
        unavailable_agents: unavailable,
        any_data: sum.events > 0,
        paused: settings.paused,
        count_cached_in_total: count_cached,
        importing_agents: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::queries::save_health;
    use crate::database::testutil::temp_db;
    use crate::database::Batch;
    use crate::model::{Accuracy, UsageEvent};

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

    fn range() -> Range {
        Range { start_utc_ms: 0, end_utc_ms: 10_000_000 }
    }

    fn agent<'a>(o: &'a Overview, id: &str) -> &'a AgentOverview {
        o.agents.iter().find(|a| a.id == id).unwrap()
    }

    #[test]
    fn unavailable_agents_have_no_numbers_not_zero() {
        let (_d, db) = temp_db();
        db.with_writer(|w| {
            save_health(&w.conn, &CollectorHealth::new("ollama", Availability::Unavailable { reason: "no ledger".into() }))?;
            save_health(&w.conn, &CollectorHealth::new("aider", Availability::NotInstalled))
        })
        .unwrap();
        let o = db.with_reader(|c| build_overview(c, &Settings::default(), Period::Day, range(), 0)).unwrap();
        assert!(agent(&o, "ollama").totals.is_none());
        assert!(agent(&o, "aider").totals.is_none());
        assert!(o.totals.is_none(), "with no available agent the headline is unavailable, not 0");
    }

    #[test]
    fn an_available_agent_with_no_usage_is_a_real_zero() {
        let (_d, db) = temp_db();
        db.with_writer(|w| save_health(&w.conn, &CollectorHealth::new("codex", Availability::NoDataYet))).unwrap();
        let o = db.with_reader(|c| build_overview(c, &Settings::default(), Period::Day, range(), 0)).unwrap();
        assert_eq!(agent(&o, "codex").totals.unwrap().total, 0);
        assert_eq!(o.totals.unwrap().total, 0);
        assert!(!o.any_data);
    }

    #[test]
    fn totals_sum_across_agents_and_exclude_unavailable_ones() {
        let (_d, db) = temp_db();
        db.with_writer(|w| {
            save_health(&w.conn, &CollectorHealth::new("codex", Availability::Ok))?;
            save_health(&w.conn, &CollectorHealth::new("claude", Availability::Ok))?;
            save_health(&w.conn, &CollectorHealth::new("ollama", Availability::NotInstalled))
        })
        .unwrap();
        db.commit(&Batch {
            agent: "codex",
            events: vec![ev("codex", "a", 1000, 100, 10, 50), ev("claude", "b", 2000, 200, 20, 0)],
            cursors: vec![],
        })
        .unwrap();
        let o = db.with_reader(|c| build_overview(c, &Settings::default(), Period::Day, range(), 0)).unwrap();
        assert_eq!(o.totals.unwrap().total, 380);
        assert_eq!(agent(&o, "codex").totals.unwrap().total, 160);
        assert!(o.unavailable_agents >= 1);
        assert!(o.any_data);
    }

    #[test]
    fn the_cached_token_setting_changes_the_headline_but_not_the_breakdown() {
        let (_d, db) = temp_db();
        db.with_writer(|w| save_health(&w.conn, &CollectorHealth::new("codex", Availability::Ok))).unwrap();
        db.commit(&Batch { agent: "codex", events: vec![ev("codex", "a", 1000, 100, 10, 50)], cursors: vec![] }).unwrap();
        let s = Settings { count_cached_in_total: false, ..Settings::default() };
        let o = db.with_reader(|c| build_overview(c, &s, Period::Day, range(), 0)).unwrap();
        let t = agent(&o, "codex").totals.unwrap();
        assert_eq!((t.total, t.cache_read), (110, 50));
    }

    #[test]
    fn history_remains_visible_when_an_agent_is_later_unavailable() {
        let (_d, db) = temp_db();
        db.commit(&Batch { agent: "gemini", events: vec![ev("gemini", "a", 1000, 100, 10, 0)], cursors: vec![] }).unwrap();
        db.with_writer(|w| save_health(&w.conn, &CollectorHealth::new("gemini", Availability::NotInstalled))).unwrap();
        let o = db.with_reader(|c| build_overview(c, &Settings::default(), Period::Day, range(), 0)).unwrap();
        assert_eq!(agent(&o, "gemini").totals.unwrap().total, 110, "stored history must not vanish");
    }

    #[test]
    fn disabled_agents_are_excluded_from_totals() {
        let (_d, db) = temp_db();
        db.with_writer(|w| save_health(&w.conn, &CollectorHealth::new("codex", Availability::Ok))).unwrap();
        db.commit(&Batch { agent: "codex", events: vec![ev("codex", "a", 1000, 100, 10, 0)], cursors: vec![] }).unwrap();
        let mut s = Settings::default();
        s.enabled_agents.retain(|a| a != "codex");
        let o = db.with_reader(|c| build_overview(c, &s, Period::Day, range(), 0)).unwrap();
        assert!(!agent(&o, "codex").enabled);
        assert!(agent(&o, "codex").totals.is_none());
    }

    #[test]
    fn a_collector_error_keeps_showing_stored_numbers() {
        let (_d, db) = temp_db();
        db.commit(&Batch { agent: "claude", events: vec![ev("claude", "a", 1000, 100, 10, 0)], cursors: vec![] }).unwrap();
        db.with_writer(|w| save_health(&w.conn, &CollectorHealth::new("claude", Availability::Error { message: "boom".into() }))).unwrap();
        let o = db.with_reader(|c| build_overview(c, &Settings::default(), Period::Day, range(), 0)).unwrap();
        assert!(agent(&o, "claude").totals.is_some());
        assert_eq!(agent(&o, "claude").availability.as_str(), "error");
    }
}
