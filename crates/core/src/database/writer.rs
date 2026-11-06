//! Idempotent, atomic ingestion.
//!
//! * One batch = one transaction: events, rollup updates and file cursors commit together or not at
//!   all, so a crash can never double count or skip.
//! * `dedupe_key` is unique. A re-seen key can only **raise** counters to the per-field maximum
//!   (a streaming partial becoming final); the 15-minute rollup is adjusted by the difference.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::error::Result;
use crate::model::{catalog, AgentId, ProjectRef, UsageEvent, DEFAULT_ACCOUNT};

pub struct WriterState {
    pub conn: Connection,
    pub cache: IdCache,
}

const BUCKET_SECONDS: i64 = 900;
const MAX_CACHED_SESSIONS: usize = 20_000;

#[derive(Default)]
pub struct IdCache {
    models: HashMap<(String, String), i64>,
    projects: HashMap<String, i64>,
    sessions: HashMap<(String, String), SessionEntry>,
}

#[derive(Clone, Copy)]
struct SessionEntry {
    id: i64,
    first: i64,
    last: i64,
}

impl IdCache {
    pub fn clear(&mut self) {
        self.models.clear();
        self.projects.clear();
        self.sessions.clear();
    }
}

/// Where a collector stopped reading one source file/DB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorUpdate {
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
    pub offset: u64,
    /// Collector-private JSON (e.g. Codex running totals).
    pub state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Batch {
    pub agent: AgentId,
    pub events: Vec<UsageEvent>,
    pub cursors: Vec<CursorUpdate>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommitStats {
    pub inserted: u64,
    pub raised: u64,
    pub unchanged: u64,
    pub rejected: u64,
}

/// Canonical identity of a project root (case-folded on Windows, forward slashes, no trailing slash).
pub fn project_key(root: &str) -> String {
    let mut k = root.replace('\\', "/");
    while k.len() > 1 && k.ends_with('/') {
        k.pop();
    }
    if cfg!(windows) {
        k = k.to_lowercase();
    }
    k
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

#[derive(Clone, Copy, Default)]
struct Counters {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
}

impl Counters {
    fn total(&self) -> i64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }
}

struct Existing {
    id: i64,
    ts_utc_ms: i64,
    agent_id: String,
    account_id: String,
    model_id: i64,
    project_id: i64,
    counters: Counters,
    reasoning_known: bool,
    estimated: bool,
}

pub fn ensure_agents(conn: &Connection) -> Result<()> {
    let mut st = conn.prepare("INSERT OR IGNORE INTO agents (id, display_name) VALUES (?1, ?2)")?;
    for a in catalog() {
        st.execute(params![a.id, a.name])?;
    }
    Ok(())
}

pub(super) fn commit(state: &mut WriterState, batch: &Batch, account: &str) -> Result<CommitStats> {
    let WriterState { conn, cache } = state;
    let result = commit_inner(conn, cache, batch, account);
    if result.is_err() {
        // The transaction rolled back; ids cached during it may no longer exist.
        cache.clear();
    }
    result
}

fn commit_inner(conn: &mut Connection, cache: &mut IdCache, batch: &Batch, account: &str) -> Result<CommitStats> {
    if cache.sessions.len() > MAX_CACHED_SESSIONS {
        cache.sessions.clear();
    }
    let tx = conn.transaction()?;
    let mut stats = CommitStats::default();
    let mut min_ts: Option<i64> = None;

    for ev in &batch.events {
        match upsert_event(&tx, cache, ev, account)? {
            Outcome::Inserted => {
                stats.inserted += 1;
                min_ts = Some(min_ts.map_or(ev.ts_utc_ms, |m| m.min(ev.ts_utc_ms)));
            }
            Outcome::Raised => stats.raised += 1,
            Outcome::Unchanged => stats.unchanged += 1,
            Outcome::Rejected => {
                tracing::warn!(agent = ev.agent, "rejected event (unavailable accuracy or empty key)");
                stats.rejected += 1;
            }
        }
    }

    for c in &batch.cursors {
        tx.prepare_cached(
            "INSERT INTO file_cursors (agent_id, path, size, mtime_ms, byte_offset, state, updated_utc_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (agent_id, path) DO UPDATE SET
               size = excluded.size, mtime_ms = excluded.mtime_ms, byte_offset = excluded.byte_offset,
               state = excluded.state, updated_utc_ms = excluded.updated_utc_ms",
        )?
        .execute(params![
            batch.agent,
            c.path,
            to_i64(c.size),
            c.mtime_ms,
            to_i64(c.offset),
            c.state,
            chrono::Utc::now().timestamp_millis()
        ])?;
    }

    if let Some(ts) = min_ts {
        tx.execute(
            "UPDATE agents SET first_seen_utc_ms = MIN(COALESCE(first_seen_utc_ms, ?1), ?1) WHERE id = ?2",
            params![ts, batch.agent],
        )?;
    }
    tx.commit()?;
    Ok(stats)
}

enum Outcome {
    Inserted,
    Raised,
    Unchanged,
    Rejected,
}

/// The key an event is stored under. The default account keeps the collector's own key (so everything recorded before
/// accounts existed still matches); any other account's keys are scoped by the account, so the same upstream id seen in
/// two accounts can never be mistaken for one event.
fn scoped_key(account: &str, key: &str) -> String {
    if account == DEFAULT_ACCOUNT {
        key.to_string()
    } else {
        format!("{account}\u{1f}{key}")
    }
}

fn upsert_event(tx: &Transaction, cache: &mut IdCache, ev: &UsageEvent, account: &str) -> Result<Outcome> {
    let Some(accuracy) = ev.accuracy.storable() else {
        return Ok(Outcome::Rejected);
    };
    if ev.dedupe_key.is_empty() {
        return Ok(Outcome::Rejected);
    }

    let new = Counters {
        input: to_i64(ev.input_tokens),
        output: to_i64(ev.output_tokens),
        cache_read: to_i64(ev.cache_read_tokens),
        cache_write: to_i64(ev.cache_write_tokens),
        reasoning: ev.reasoning_tokens.map(to_i64).unwrap_or(0),
    };

    let db_key = scoped_key(account, &ev.dedupe_key);
    let existing = tx
        .prepare_cached(
            "SELECT id, ts_utc_ms, agent_id, model_id, project_id, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens, accuracy, account_id
             FROM usage_events WHERE dedupe_key = ?1",
        )?
        .query_row([&db_key], |r| {
            let reasoning: Option<i64> = r.get(9)?;
            let acc: String = r.get(10)?;
            Ok(Existing {
                id: r.get(0)?,
                ts_utc_ms: r.get(1)?,
                agent_id: r.get(2)?,
                account_id: r.get(11)?,
                model_id: r.get(3)?,
                project_id: r.get(4)?,
                counters: Counters {
                    input: r.get(5)?,
                    output: r.get(6)?,
                    cache_read: r.get(7)?,
                    cache_write: r.get(8)?,
                    reasoning: reasoning.unwrap_or(0),
                },
                reasoning_known: reasoning.is_some(),
                estimated: acc == "estimated",
            })
        })
        .optional()?;

    match existing {
        None => {
            let model = if ev.model.trim().is_empty() { "unknown" } else { ev.model.as_str() };
            let model_id = model_id(tx, cache, ev.agent, model)?;
            let project_id = project_id(tx, cache, ev.project.as_ref())?;
            let session_pk = match &ev.session_id {
                Some(s) if !s.is_empty() => Some(touch_session(tx, cache, ev.agent, s, project_id, ev.ts_utc_ms)?),
                _ => None,
            };
            tx.prepare_cached(
                "INSERT INTO usage_events (dedupe_key, agent_id, model_id, project_id, session_id, ts_utc_ms,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, accuracy, source, account_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?
            .execute(params![
                db_key,
                ev.agent,
                model_id,
                project_id,
                session_pk,
                ev.ts_utc_ms,
                new.input,
                new.output,
                new.cache_read,
                new.cache_write,
                ev.reasoning_tokens.map(to_i64),
                accuracy,
                ev.source,
                account
            ])?;
            let estimated = if accuracy == "estimated" { new.total() } else { 0 };
            bucket_add(tx, ev.ts_utc_ms, ev.agent, account, model_id, project_id, &new, estimated, 1)?;
            Ok(Outcome::Inserted)
        }
        Some(old) => {
            let merged = Counters {
                input: old.counters.input.max(new.input),
                output: old.counters.output.max(new.output),
                cache_read: old.counters.cache_read.max(new.cache_read),
                cache_write: old.counters.cache_write.max(new.cache_write),
                reasoning: old.counters.reasoning.max(new.reasoning),
            };
            let reasoning_known = old.reasoning_known || ev.reasoning_tokens.is_some();
            let diff = Counters {
                input: merged.input - old.counters.input,
                output: merged.output - old.counters.output,
                cache_read: merged.cache_read - old.counters.cache_read,
                cache_write: merged.cache_write - old.counters.cache_write,
                reasoning: merged.reasoning - old.counters.reasoning,
            };
            if diff.total() == 0 && diff.reasoning == 0 && reasoning_known == old.reasoning_known {
                return Ok(Outcome::Unchanged);
            }
            tx.prepare_cached(
                "UPDATE usage_events SET input_tokens = ?1, output_tokens = ?2, cache_read_tokens = ?3,
                    cache_write_tokens = ?4, reasoning_tokens = ?5 WHERE id = ?6",
            )?
            .execute(params![
                merged.input,
                merged.output,
                merged.cache_read,
                merged.cache_write,
                if reasoning_known { Some(merged.reasoning) } else { None },
                old.id
            ])?;
            let estimated = if old.estimated { diff.total() } else { 0 };
            // Adjust the bucket the event was originally filed under.
            bucket_add(tx, old.ts_utc_ms, &old.agent_id, &old.account_id, old.model_id, old.project_id, &diff, estimated, 0)?;
            Ok(Outcome::Raised)
        }
    }
}

fn model_id(tx: &Transaction, cache: &mut IdCache, agent: &str, name: &str) -> Result<i64> {
    let key = (agent.to_string(), name.to_string());
    if let Some(id) = cache.models.get(&key) {
        return Ok(*id);
    }
    tx.prepare_cached("INSERT OR IGNORE INTO models (agent_id, name) VALUES (?1, ?2)")?
        .execute(params![agent, name])?;
    let id: i64 = tx
        .prepare_cached("SELECT id FROM models WHERE agent_id = ?1 AND name = ?2")?
        .query_row(params![agent, name], |r| r.get(0))?;
    cache.models.insert(key, id);
    Ok(id)
}

fn project_id(tx: &Transaction, cache: &mut IdCache, project: Option<&ProjectRef>) -> Result<i64> {
    let Some(p) = project else { return Ok(0) };
    let key = project_key(&p.root_path);
    if key.is_empty() {
        return Ok(0);
    }
    if let Some(id) = cache.projects.get(&key) {
        return Ok(*id);
    }
    tx.prepare_cached("INSERT OR IGNORE INTO projects (key, name, root_path) VALUES (?1, ?2, ?3)")?
        .execute(params![key, p.name, p.root_path])?;
    let id: i64 = tx
        .prepare_cached("SELECT id FROM projects WHERE key = ?1")?
        .query_row([&key], |r| r.get(0))?;
    cache.projects.insert(key, id);
    Ok(id)
}

fn touch_session(
    tx: &Transaction,
    cache: &mut IdCache,
    agent: &str,
    external: &str,
    project_id: i64,
    ts: i64,
) -> Result<i64> {
    let key = (agent.to_string(), external.to_string());
    if let Some(e) = cache.sessions.get_mut(&key) {
        if ts < e.first || ts > e.last {
            let (first, last) = (e.first.min(ts), e.last.max(ts));
            tx.prepare_cached("UPDATE sessions SET first_event_utc_ms = ?1, last_event_utc_ms = ?2 WHERE id = ?3")?
                .execute(params![first, last, e.id])?;
            e.first = first;
            e.last = last;
        }
        return Ok(e.id);
    }
    tx.prepare_cached(
        "INSERT INTO sessions (agent_id, external_id, project_id, first_event_utc_ms, last_event_utc_ms)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (agent_id, external_id) DO UPDATE SET
           first_event_utc_ms = MIN(first_event_utc_ms, excluded.first_event_utc_ms),
           last_event_utc_ms  = MAX(last_event_utc_ms,  excluded.last_event_utc_ms),
           project_id = CASE WHEN sessions.project_id = 0 THEN excluded.project_id ELSE sessions.project_id END",
    )?
    .execute(params![agent, external, project_id, ts])?;
    let (id, first, last): (i64, i64, i64) = tx
        .prepare_cached(
            "SELECT id, first_event_utc_ms, last_event_utc_ms FROM sessions WHERE agent_id = ?1 AND external_id = ?2",
        )?
        .query_row(params![agent, external], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    cache.sessions.insert(key, SessionEntry { id, first, last });
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn bucket_add(
    tx: &Transaction,
    ts_utc_ms: i64,
    agent: &str,
    account: &str,
    model_id: i64,
    project_id: i64,
    d: &Counters,
    estimated: i64,
    events: i64,
) -> Result<()> {
    let bucket = ts_utc_ms.div_euclid(1000).div_euclid(BUCKET_SECONDS) * BUCKET_SECONDS;
    tx.prepare_cached(
        "INSERT INTO usage_buckets (bucket_utc_s, agent_id, account_id, model_id, project_id, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_tokens, events)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (bucket_utc_s, agent_id, account_id, model_id, project_id) DO UPDATE SET
            input_tokens       = input_tokens       + excluded.input_tokens,
            output_tokens      = output_tokens      + excluded.output_tokens,
            cache_read_tokens  = cache_read_tokens  + excluded.cache_read_tokens,
            cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
            reasoning_tokens   = reasoning_tokens   + excluded.reasoning_tokens,
            estimated_tokens   = estimated_tokens   + excluded.estimated_tokens,
            events             = events             + excluded.events",
    )?
    .execute(params![
        bucket,
        agent,
        account,
        model_id,
        project_id,
        d.input,
        d.output,
        d.cache_read,
        d.cache_write,
        d.reasoning,
        estimated,
        events
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::model::{Accuracy, ProjectRef};

    fn ev(key: &str, ts_ms: i64, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            agent: "codex",
            model: "model-a".into(),
            ts_utc_ms: ts_ms,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: None,
            session_id: Some("s1".into()),
            project: None,
            source: "local-session-data",
            accuracy: Accuracy::Real,
            dedupe_key: key.into(),
        }
    }

    fn batch(events: Vec<UsageEvent>) -> Batch {
        Batch { agent: "codex", events, cursors: vec![] }
    }

    fn bucket_total(db: &crate::database::Database) -> i64 {
        db.with_reader(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens), 0) FROM usage_buckets",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    fn event_total(db: &crate::database::Database) -> i64 {
        db.with_reader(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens), 0) FROM usage_events",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    #[test]
    fn insert_creates_event_and_bucket() {
        let (_d, db) = temp_db();
        let s = db.commit(&batch(vec![ev("a", 1_000_000, 100, 20)])).unwrap();
        assert_eq!(s.inserted, 1);
        assert_eq!(bucket_total(&db), 120);
        assert_eq!(event_total(&db), 120);
    }

    #[test]
    fn replaying_the_same_batch_never_double_counts() {
        let (_d, db) = temp_db();
        let b = batch(vec![ev("a", 1_000_000, 100, 20), ev("b", 2_000_000, 5, 5)]);
        db.commit(&b).unwrap();
        let again = db.commit(&b).unwrap();
        assert_eq!(again.inserted, 0);
        assert_eq!(again.unchanged, 2);
        assert_eq!(bucket_total(&db), 130);
    }

    #[test]
    fn a_higher_revision_raises_to_the_maximum_and_adjusts_the_bucket_by_the_difference() {
        let (_d, db) = temp_db();
        db.commit(&batch(vec![ev("a", 1_000_000, 10, 1)])).unwrap(); // streaming partial
        let s = db.commit(&batch(vec![ev("a", 1_000_000, 10, 50)])).unwrap(); // final
        assert_eq!(s.raised, 1);
        assert_eq!(event_total(&db), 60);
        assert_eq!(bucket_total(&db), 60, "bucket must hold the max, not partial + final");
    }

    #[test]
    fn a_lower_revision_never_lowers_counters() {
        let (_d, db) = temp_db();
        db.commit(&batch(vec![ev("a", 1_000_000, 10, 50)])).unwrap();
        let s = db.commit(&batch(vec![ev("a", 1_000_000, 10, 1)])).unwrap();
        assert_eq!(s.unchanged, 1);
        assert_eq!(event_total(&db), 60);
    }

    #[test]
    fn events_in_the_same_bucket_are_summed_and_different_buckets_stay_apart() {
        let (_d, db) = temp_db();
        // 3 events within one 15-minute bucket, 1 an hour later.
        db.commit(&batch(vec![
            ev("a", 900_000 * 4, 1, 0),
            ev("b", 900_000 * 4 + 1000, 2, 0),
            ev("c", 900_000 * 4 + 899_000, 4, 0),
            ev("d", 900_000 * 8, 8, 0),
        ]))
        .unwrap();
        let (buckets, events): (i64, i64) = db
            .with_reader(|c| {
                Ok(c.query_row("SELECT count(*), SUM(events) FROM usage_buckets", [], |r| Ok((r.get(0)?, r.get(1)?)))?)
            })
            .unwrap();
        assert_eq!((buckets, events), (2, 4));
        assert_eq!(bucket_total(&db), 15);
    }

    #[test]
    fn unavailable_events_are_rejected_and_nothing_is_stored() {
        let (_d, db) = temp_db();
        let mut e = ev("a", 1000, 100, 100);
        e.accuracy = Accuracy::Unavailable;
        let s = db.commit(&batch(vec![e])).unwrap();
        assert_eq!(s.rejected, 1);
        assert_eq!(event_total(&db), 0);
        assert_eq!(bucket_total(&db), 0);
    }

    #[test]
    fn estimated_tokens_are_tracked_separately_in_the_rollup() {
        let (_d, db) = temp_db();
        let mut e = ev("a", 1000, 100, 0);
        e.accuracy = Accuracy::Estimated;
        db.commit(&batch(vec![e, ev("b", 1000, 50, 0)])).unwrap();
        let (total, est): (i64, i64) = db
            .with_reader(|c| {
                Ok(c.query_row(
                    "SELECT SUM(input_tokens), SUM(estimated_tokens) FROM usage_buckets",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((total, est), (150, 100));
    }

    #[test]
    fn a_failing_batch_rolls_back_events_and_cursors_together() {
        let (_d, db) = temp_db();
        let mut bad = ev("bad", 1000, 1, 1);
        bad.agent = "not-an-agent"; // violates the agents FK
        let b = Batch {
            agent: "codex",
            events: vec![ev("good", 1000, 100, 100), bad],
            cursors: vec![CursorUpdate { path: "p".into(), size: 1, mtime_ms: 1, offset: 1, state: None }],
        };
        assert!(db.commit(&b).is_err());
        assert_eq!(event_total(&db), 0, "earlier events in the failed batch must be rolled back");
        let cursors: i64 = db.with_reader(|c| Ok(c.query_row("SELECT count(*) FROM file_cursors", [], |r| r.get(0))?)).unwrap();
        assert_eq!(cursors, 0, "cursor must not advance when its events were not stored");

        // The cache must have been invalidated: a later valid batch works.
        let s = db.commit(&batch(vec![ev("good", 1000, 100, 100)])).unwrap();
        assert_eq!(s.inserted, 1);
        assert_eq!(event_total(&db), 200);
    }

    #[test]
    fn cursors_are_upserted_with_events() {
        let (_d, db) = temp_db();
        let mk = |offset| Batch {
            agent: "codex",
            events: vec![],
            cursors: vec![CursorUpdate { path: "f.jsonl".into(), size: 10, mtime_ms: 5, offset, state: Some("{}".into()) }],
        };
        db.commit(&mk(3)).unwrap();
        db.commit(&mk(9)).unwrap();
        let (n, off): (i64, i64) = db
            .with_reader(|c| Ok(c.query_row("SELECT count(*), MAX(byte_offset) FROM file_cursors", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!((n, off), (1, 9));
    }

    #[test]
    fn sessions_track_first_and_last_event() {
        let (_d, db) = temp_db();
        db.commit(&batch(vec![ev("a", 5000, 1, 1), ev("b", 1000, 1, 1), ev("c", 9000, 1, 1)])).unwrap();
        let (n, first, last): (i64, i64, i64) = db
            .with_reader(|c| {
                Ok(c.query_row(
                    "SELECT count(*), MIN(first_event_utc_ms), MAX(last_event_utc_ms) FROM sessions",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!((n, first, last), (1, 1000, 9000));
    }

    #[test]
    fn project_identity_ignores_case_and_slash_style_on_windows_only_where_it_should() {
        assert_eq!(project_key("D:\\github\\App\\"), if cfg!(windows) { "d:/github/app" } else { "D:/github/App" });
        assert_eq!(project_key("/home/me/app/"), "/home/me/app");
    }

    #[test]
    fn events_without_a_project_land_in_unknown_project() {
        let (_d, db) = temp_db();
        let mut with = ev("a", 1000, 1, 1);
        with.project = Some(ProjectRef { root_path: "D:\\github\\robot".into(), name: "robot".into() });
        db.commit(&batch(vec![with, ev("b", 1000, 1, 1)])).unwrap();
        let rows: Vec<(String, i64)> = db
            .with_reader(|c| {
                let mut st = c.prepare(
                    "SELECT p.name, COUNT(*) FROM usage_events e JOIN projects p ON p.id = e.project_id GROUP BY p.name ORDER BY p.name",
                )?;
                let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(v)
            })
            .unwrap();
        assert_eq!(rows, vec![("UNKNOWN PROJECT".to_string(), 1), ("robot".to_string(), 1)]);
    }

    /// The rollup must equal a from-scratch recompute no matter what sequence of inserts and raises ran.
    #[test]
    fn rollup_always_equals_recompute_over_a_random_sequence() {
        let (_d, db) = temp_db();
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for round in 0..40 {
            let mut events = Vec::new();
            for _ in 0..25 {
                let k = next() % 60; // heavy key reuse => many raises and no-ops
                let ts = 1_700_000_000_000 + i64::from(k) * 400_000;
                events.push(ev(&format!("k{k}"), ts, u64::from(next() % 5000), u64::from(next() % 800)));
            }
            db.commit(&batch(events)).unwrap();
            assert_eq!(bucket_total(&db), event_total(&db), "diverged at round {round}");
        }
        let per_bucket_matches: bool = db
            .with_reader(|c| {
                // Recompute each bucket from events and compare cell by cell.
                let bad: i64 = c.query_row(
                    "SELECT count(*) FROM (
                       SELECT b.bucket_utc_s FROM usage_buckets b
                       LEFT JOIN (SELECT (ts_utc_ms / 1000 / 900) * 900 AS bk, agent_id, account_id, model_id, project_id,
                                         SUM(input_tokens) i, SUM(output_tokens) o, COUNT(*) n
                                  FROM usage_events GROUP BY bk, agent_id, account_id, model_id, project_id) e
                         ON e.bk = b.bucket_utc_s AND e.agent_id = b.agent_id AND e.account_id = b.account_id
                            AND e.model_id = b.model_id AND e.project_id = b.project_id
                       WHERE e.i IS NOT b.input_tokens OR e.o IS NOT b.output_tokens OR e.n IS NOT b.events)",
                    [],
                    |r| r.get(0),
                )?;
                Ok(bad == 0)
            })
            .unwrap();
        assert!(per_bucket_matches);
    }

    fn account_total(db: &crate::database::Database, account: &str) -> (i64, i64) {
        db.with_reader(|c| {
            let buckets: i64 = c.query_row(
                "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens), 0) FROM usage_buckets WHERE account_id = ?1",
                [account],
                |r| r.get(0),
            )?;
            let events: i64 = c.query_row(
                "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens), 0) FROM usage_events WHERE account_id = ?1",
                [account],
                |r| r.get(0),
            )?;
            Ok((buckets, events))
        })
        .unwrap()
    }

    #[test]
    fn the_plain_commit_files_everything_under_the_default_account() {
        let (_d, db) = temp_db();
        db.commit(&batch(vec![ev("a", 1_000_000, 100, 20)])).unwrap();
        assert_eq!(account_total(&db, "default"), (120, 120));
        assert_eq!(account_total(&db, "work"), (0, 0));
    }

    #[test]
    fn the_same_upstream_id_in_two_accounts_is_two_events_in_two_buckets() {
        let (_d, db) = temp_db();
        let b = batch(vec![ev("same-id", 1_000_000, 100, 20)]);
        assert_eq!(db.commit_for("default", &b).unwrap().inserted, 1);
        assert_eq!(db.commit_for("work", &b).unwrap().inserted, 1, "not mistaken for the default account's event");
        let b2 = batch(vec![ev("same-id", 1_000_000, 7, 3)]);
        assert_eq!(db.commit_for("personal", &b2).unwrap().inserted, 1);
        assert_eq!(account_total(&db, "default"), (120, 120));
        assert_eq!(account_total(&db, "work"), (120, 120));
        assert_eq!(account_total(&db, "personal"), (10, 10));
        assert_eq!(bucket_total(&db), 250, "the agent's total is the sum of its accounts");
    }

    #[test]
    fn replaying_an_accounts_batch_never_double_counts_and_stays_in_that_account() {
        let (_d, db) = temp_db();
        let b = batch(vec![ev("a", 1_000_000, 100, 20), ev("b", 2_000_000, 5, 5)]);
        db.commit_for("work", &b).unwrap();
        let again = db.commit_for("work", &b).unwrap();
        assert_eq!((again.inserted, again.raised), (0, 0));
        assert_eq!(account_total(&db, "work"), (130, 130));
        assert_eq!(account_total(&db, "default"), (0, 0));
    }

    #[test]
    fn a_streamed_event_raised_later_adjusts_only_its_own_accounts_bucket() {
        let (_d, db) = temp_db();
        db.commit_for("default", &batch(vec![ev("x", 1_000_000, 10, 0)])).unwrap();
        db.commit_for("work", &batch(vec![ev("x", 1_000_000, 10, 0)])).unwrap();
        let s = db.commit_for("work", &batch(vec![ev("x", 1_000_000, 60, 0)])).unwrap();
        assert_eq!((s.inserted, s.raised), (0, 1));
        assert_eq!(account_total(&db, "work"), (60, 60), "the work event grew");
        assert_eq!(account_total(&db, "default"), (10, 10), "the default account's event did not");
    }

    #[test]
    fn accounts_have_their_own_cursors_per_file_so_two_profiles_never_share_a_read_position() {
        let (_d, db) = temp_db();
        let cursor = |path: &str| CursorUpdate { path: path.into(), size: 10, mtime_ms: 1, offset: 10, state: None };
        db.commit_for("default", &Batch { agent: "codex", events: vec![], cursors: vec![cursor("C:/home/.codex/sessions/a.jsonl")] }).unwrap();
        db.commit_for("work", &Batch { agent: "codex", events: vec![], cursors: vec![cursor("C:/accounts/work/.codex/sessions/a.jsonl")] }).unwrap();
        let n: i64 = db.with_reader(|c| Ok(c.query_row("SELECT count(*) FROM file_cursors WHERE agent_id = 'codex'", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 2, "different files, so different cursors");
    }
}
