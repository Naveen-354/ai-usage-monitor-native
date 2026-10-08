//! OpenCode collector.
//!
//! Source: `~/.local/share/opencode/opencode.db` (SQLite). Verified on the installed OpenCode 1.18.4:
//! tables `message(id, session_id, time_created, time_updated, data JSON)` and `session(…, directory, …)`
//! exist, and the product's own source (read from its binary) shows an assistant message's `data` carries
//! `tokens = {input, output, reasoning, cache: {read, write}}`, `modelID`, `providerID`, `time.created` (ms).
//! OpenCode's own `stats` command counts `output + reasoning` as output, so we do too.
//!
//! **Honest limit:** on this machine every table is empty (`opencode stats` reports 0 sessions), so the
//! *values* could not be validated against real data. The collector therefore labels itself as such in
//! diagnostics and reads only numbers that are really present.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::env::path_str;
use super::tail::mtime_ms;
use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, SinkBatch, WatchSpec,
};
use crate::collectors::accumulate::Accumulator;
use crate::database::CursorUpdate;
use crate::model::{Accuracy, AgentId, UsageEvent};

pub const ID: AgentId = "opencode";
const SOURCE: &str = "local-session-data";
/// Rows are updated in place while a response streams; re-read the last minute each time.
const OVERLAP_MS: i64 = 60_000;
const VALIDATION_NOTE: &str = "message format verified from the installed OpenCode binary; no local usage exists to validate the values";

pub struct OpenCodeCollector;

fn db_path(env: &Env) -> PathBuf {
    env.xdg_data().join("opencode").join("opencode.db")
}

#[derive(Deserialize)]
struct Cache {
    read: Option<u64>,
    write: Option<u64>,
}

#[derive(Deserialize)]
struct Tokens {
    input: Option<u64>,
    output: Option<u64>,
    reasoning: Option<u64>,
    cache: Option<Cache>,
}

#[derive(Deserialize)]
struct Time {
    created: Option<i64>,
}

#[derive(Deserialize)]
struct PathInfo {
    cwd: Option<String>,
}

/// Only the fields we need; message text lives in separate `part` rows and is never touched.
#[derive(Deserialize)]
struct Data {
    role: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    tokens: Option<Tokens>,
    time: Option<Time>,
    path: Option<PathInfo>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    max_updated: i64,
}

fn db_stat(path: &Path) -> Option<(u64, i64)> {
    let main = std::fs::metadata(path).ok()?;
    let (mut size, mut mtime) = (main.len(), mtime_ms(&main));
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    if let Ok(w) = std::fs::metadata(PathBuf::from(wal)) {
        size += w.len();
        mtime = mtime.max(mtime_ms(&w));
    }
    Some((size, mtime))
}

fn has_table(c: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(c.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", [name], |_| Ok(())).optional()?.is_some())
}

impl AgentCollector for OpenCodeCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "OpenCode",
            data_sources: &["~/.local/share/opencode/opencode.db (message.data → tokens)"],
            caveats: &[VALIDATION_NOTE, "reasoning tokens are counted as output, as OpenCode's own stats does"],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let db = db_path(env);
        let has_data = db.is_file();
        let binary = env.which(&["opencode"], &[]);
        let presence = match (binary.is_some(), has_data) {
            (true, _) => Presence::Installed,
            (false, true) => Presence::DataOnly,
            (false, false) => Presence::NotFound,
        };
        Detection {
            presence,
            roots: if has_data { vec![db] } else { vec![] },
            note: Some(VALIDATION_NOTE.to_string()),
            binary,
        }
    }

    fn watch_specs(&self, d: &Detection) -> Vec<WatchSpec> {
        d.roots
            .iter()
            .filter_map(|p| p.parent())
            .map(|dir| WatchSpec { path: dir.to_path_buf(), recursive: false, suffixes: &["opencode.db", "opencode.db-wal"] })
            .collect()
    }

    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        let mut summary = CollectSummary::default();
        let Some(path) = ctx.detection.roots.first() else { return Ok(summary) };
        ctx.check_cancel()?;
        summary.files_seen = 1;

        let Some((size, mtime)) = db_stat(path) else { return Ok(summary) };
        let cursor = ctx.cursor(path);
        if cursor.is_some_and(|c| c.size == size && c.mtime_ms == mtime) {
            return Ok(summary);
        }
        let prev = cursor.and_then(|c| serde_json::from_str::<State>(c.state.as_deref()?).ok()).map(|s| s.max_updated);

        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_millis(1500))?;
        conn.pragma_update(None, "query_only", "ON")?;

        if !has_table(&conn, "message")? {
            return Err(CollectError::Unavailable("opencode.db has no `message` table: the OpenCode schema has changed".into()));
        }
        let joined = has_table(&conn, "session")?;
        let sql = if joined {
            "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, s.directory
             FROM message m LEFT JOIN session s ON s.id = m.session_id WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
        } else {
            "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, NULL
             FROM message m WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
        };
        let start = prev.map_or(0, |p| (p - OVERLAP_MS).max(0));

        let mut acc = Accumulator::default();
        let (mut assistant, mut with_tokens, mut skipped, mut max_updated) = (0u64, 0u64, 0u64, prev.unwrap_or(0));
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query([start])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let session: Option<String> = row.get(1)?;
            let created: Option<i64> = row.get(2)?;
            let updated: i64 = row.get(3)?;
            let data: String = row.get(4)?;
            let directory: Option<String> = row.get(5)?;
            max_updated = max_updated.max(updated);

            let Ok(d) = serde_json::from_str::<Data>(&data) else {
                skipped += 1;
                continue;
            };
            if d.role.as_deref() != Some("assistant") {
                continue;
            }
            assistant += 1;
            let Some(t) = d.tokens else { continue };
            with_tokens += 1;
            let (input, output, reasoning) = (t.input.unwrap_or(0), t.output.unwrap_or(0), t.reasoning.unwrap_or(0));
            let (cache_read, cache_write) = t.cache.map_or((0, 0), |c| (c.read.unwrap_or(0), c.write.unwrap_or(0)));
            // A row that has just been created streams in at zero: nothing to count yet.
            if input + output + reasoning + cache_read + cache_write == 0 {
                continue;
            }
            let Some(ts) = d.time.and_then(|t| t.created).or(created).filter(|t| *t > 0) else {
                skipped += 1;
                continue;
            };
            let cwd = d.path.and_then(|p| p.cwd).filter(|c| !c.is_empty()).or(directory);
            let ev = UsageEvent {
                agent: ID,
                model: d.model_id.filter(|m| !m.is_empty()).unwrap_or_else(|| "unknown".to_string()),
                ts_utc_ms: ts,
                input_tokens: input,
                output_tokens: output + reasoning,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                reasoning_tokens: Some(reasoning),
                session_id: session,
                project: cwd.and_then(|c| ctx.projects.resolve(&c)),
                source: SOURCE,
                accuracy: Accuracy::Real,
                dedupe_key: format!("opencode:{id}"),
            };
            summary.saw_event(ev.ts_utc_ms);
            acc.push(ev);
        }

        // Assistant messages exist but none carries a tokens block: the shape has probably changed.
        if assistant > 0 && with_tokens == 0 {
            return Err(CollectError::Unavailable(format!(
                "{assistant} OpenCode assistant message(s) found without a `tokens` block: the message format may have changed"
            )));
        }
        summary.skipped_records += skipped;
        summary.files_read = 1;
        sink.commit(SinkBatch {
            events: acc.into_events(),
            cursors: vec![CursorUpdate {
                path: path_str(path),
                size,
                mtime_ms: mtime,
                offset: 0,
                state: serde_json::to_string(&State { max_updated }).ok(),
            }],
        })?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests;
