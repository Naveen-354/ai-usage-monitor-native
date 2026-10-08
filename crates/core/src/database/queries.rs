//! Read-side queries. Period totals come from the 15-minute rollup, never from `usage_events`,
//! so month/year views stay fast with millions of events.

use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::Result;
use crate::model::{Availability, CollectorHealth};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    /// Portion of the total that came from estimated (not actual) events.
    pub estimated: u64,
    pub events: u64,
}

impl Counters {
    pub fn total_all(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    pub fn total_excluding_cache(&self) -> u64 {
        self.input.saturating_add(self.output)
    }

    pub fn add(&mut self, o: &Counters) {
        self.input = self.input.saturating_add(o.input);
        self.output = self.output.saturating_add(o.output);
        self.cache_read = self.cache_read.saturating_add(o.cache_read);
        self.cache_write = self.cache_write.saturating_add(o.cache_write);
        self.reasoning = self.reasoning.saturating_add(o.reasoning);
        self.estimated = self.estimated.saturating_add(o.estimated);
        self.events = self.events.saturating_add(o.events);
    }
}

fn u(v: i64) -> u64 {
    v.max(0) as u64
}

/// Totals per agent for buckets in `[start_ms, end_ms)`.
pub fn totals_by_agent(conn: &Connection, start_ms: i64, end_ms: i64) -> Result<HashMap<String, Counters>> {
    let mut st = conn.prepare_cached(
        "SELECT agent_id, SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(cache_write_tokens),
                SUM(reasoning_tokens), SUM(estimated_tokens), SUM(events)
         FROM usage_buckets WHERE bucket_utc_s >= ?1 AND bucket_utc_s < ?2 GROUP BY agent_id",
    )?;
    let rows = st.query_map(params![start_ms.div_euclid(1000), end_ms.div_euclid(1000)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Counters {
                input: u(r.get(1)?),
                output: u(r.get(2)?),
                cache_read: u(r.get(3)?),
                cache_write: u(r.get(4)?),
                reasoning: u(r.get(5)?),
                estimated: u(r.get(6)?),
                events: u(r.get(7)?),
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (agent, c) = row?;
        out.insert(agent, c);
    }
    Ok(out)
}

/// Most recent event time per agent (uses `idx_events_agent_ts`).
pub fn last_event_by_agent(conn: &Connection) -> Result<HashMap<String, i64>> {
    let mut st = conn.prepare_cached("SELECT agent_id, MAX(ts_utc_ms) FROM usage_events GROUP BY agent_id")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (a, t) = r?;
        out.insert(a, t);
    }
    Ok(out)
}

/// Saved read positions for one agent, keyed by source path.
pub fn load_cursors(conn: &Connection, agent: &str) -> Result<HashMap<String, crate::collectors::Cursor>> {
    let mut st = conn.prepare_cached(
        "SELECT path, size, mtime_ms, byte_offset, state FROM file_cursors WHERE agent_id = ?1",
    )?;
    let rows = st.query_map([agent], |r| {
        Ok((
            r.get::<_, String>(0)?,
            crate::collectors::Cursor {
                size: u(r.get(1)?),
                mtime_ms: r.get(2)?,
                offset: u(r.get(3)?),
                state: r.get(4)?,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, cursor) = row?;
        out.insert(path, cursor);
    }
    Ok(out)
}

/// `(event count, latest event time)` for one agent.
pub fn agent_event_stats(conn: &Connection, agent: &str) -> Result<(u64, Option<i64>)> {
    let (n, last): (i64, Option<i64>) = conn
        .prepare_cached("SELECT COUNT(*), MAX(ts_utc_ms) FROM usage_events WHERE agent_id = ?1")?
        .query_row([agent], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok((u(n), last))
}

pub fn load_health(conn: &Connection) -> Result<Vec<CollectorHealth>> {
    let mut st = conn.prepare_cached(
        "SELECT agent_id, availability, detail, note, last_run_utc_ms, last_success_utc_ms, last_event_utc_ms,
                events_total, skipped_records, source_paths FROM collector_health ORDER BY agent_id",
    )?;
    let rows = st.query_map([], |r| {
        let availability: String = r.get(1)?;
        let detail: Option<String> = r.get(2)?;
        let paths: String = r.get(9)?;
        Ok(CollectorHealth {
            agent: r.get(0)?,
            availability: Availability::from_parts(&availability, detail),
            note: r.get(3)?,
            last_run_utc_ms: r.get(4)?,
            last_success_utc_ms: r.get(5)?,
            last_event_utc_ms: r.get(6)?,
            events_total: u(r.get(7)?),
            skipped_records: u(r.get(8)?),
            source_paths: serde_json::from_str(&paths).unwrap_or_default(),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn save_health(conn: &Connection, h: &CollectorHealth) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO collector_health (agent_id, availability, detail, note, last_run_utc_ms, last_success_utc_ms,
            last_event_utc_ms, events_total, skipped_records, source_paths)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT (agent_id) DO UPDATE SET
            availability = excluded.availability, detail = excluded.detail, note = excluded.note,
            last_run_utc_ms = excluded.last_run_utc_ms, last_success_utc_ms = excluded.last_success_utc_ms,
            last_event_utc_ms = excluded.last_event_utc_ms, events_total = excluded.events_total,
            skipped_records = excluded.skipped_records, source_paths = excluded.source_paths",
    )?
    .execute(params![
        h.agent,
        h.availability.as_str(),
        h.availability.detail(),
        h.note,
        h.last_run_utc_ms,
        h.last_success_utc_ms,
        h.last_event_utc_ms,
        h.events_total as i64,
        h.skipped_records as i64,
        serde_json::to_string(&h.source_paths).unwrap_or_else(|_| "[]".into()),
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::database::Batch;
    use crate::model::{Accuracy, UsageEvent};

    fn ev(agent: &'static str, key: &str, ts_ms: i64, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            agent,
            model: "m".into(),
            ts_utc_ms: ts_ms,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 7,
            cache_write_tokens: 0,
            reasoning_tokens: None,
            session_id: None,
            project: None,
            source: "t",
            accuracy: Accuracy::Real,
            dedupe_key: key.into(),
        }
    }

    #[test]
    fn totals_are_grouped_by_agent_and_respect_half_open_range() {
        let (_d, db) = temp_db();
        let h = 3_600_000;
        db.commit(&Batch {
            agent: "codex",
            events: vec![
                ev("codex", "a", 10 * h, 100, 10),
                ev("codex", "b", 11 * h, 200, 20), // exactly at range end -> excluded
                ev("claude", "c", 10 * h + 1, 5, 5),
            ],
            cursors: vec![],
        })
        .unwrap();
        let totals = db.with_reader(|c| totals_by_agent(c, 10 * h, 11 * h)).unwrap();
        assert_eq!(totals["codex"].input, 100);
        assert_eq!(totals["codex"].cache_read, 7);
        assert_eq!(totals["codex"].total_all(), 117);
        assert_eq!(totals["codex"].total_excluding_cache(), 110);
        assert_eq!(totals["claude"].events, 1);
        assert_eq!(totals.len(), 2);
    }

    #[test]
    fn an_empty_range_yields_no_rows_not_zero_rows() {
        let (_d, db) = temp_db();
        let totals = db.with_reader(|c| totals_by_agent(c, 0, 1_000_000)).unwrap();
        assert!(totals.is_empty(), "absence of data must stay distinguishable from zero");
    }

    #[test]
    fn health_round_trips() {
        let (_d, db) = temp_db();
        let mut h = CollectorHealth::new("gemini", Availability::Unavailable { reason: "no ledger".into() });
        h.note = Some("data found, CLI not on PATH".into());
        h.events_total = 42;
        h.source_paths = vec!["C:/x".into()];
        db.with_writer(|w| save_health(&w.conn, &h)).unwrap();
        let back = db.with_reader(load_health).unwrap();
        assert_eq!(back, vec![h]);
    }
}
