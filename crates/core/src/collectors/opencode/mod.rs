//! OpenCode collector.
//!
//! Source: `~/.local/share/opencode/opencode.db` (SQLite). Two generations of its layout are read:
//!
//! * **1.x** (verified on OpenCode 1.18.4): `message(id, session_id, time_created, time_updated, data JSON)` and
//!   `session(…, directory, …)`. The role is `data.role`, the model `data.modelID`.
//! * **2.x** (verified on OpenCode 2.2.5): those tables were replaced by `session_message(id, session_id, type, seq,
//!   time_created, time_updated, data JSON)` and `session_v2(…, directory, …)`. The role is the `type` column, the
//!   model `data.model.id`. The token block did not change.
//!
//! In both, an assistant message's `data` carries `tokens = {input, output, reasoning, cache: {read, write}}` and
//! `time.created` (ms); the product's own source (read from its binary) agrees. OpenCode's own `stats` command counts
//! `output + reasoning` as output, so we do too.
//!
//! **Honest limit:** there is very little local usage to validate the values against (on the development machine: one 2.x
//! message), so the collector says so in diagnostics and reads only numbers that are really present. In 2.x the session
//! totals (`session_v2.tokens_*`) run a little above the sum of the stored messages, because some calls (a title, say) are
//! not stored as messages. Periods and charts need an event per message, so the messages are what is counted.

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
const VALIDATION_NOTE: &str = "reads OpenCode 1.x and 2.x databases; too little local usage exists to validate the values";

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

/// Only the fields we need; message text lives in `content` / separate `part` rows and is never touched.
#[derive(Deserialize)]
struct Data {
    /// 1.x only: 2.x keeps the role in the `type` column.
    role: Option<String>,
    /// 1.x: the model's name.
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    /// 2.x: `{id, providerID, variant}`. Read leniently: in 1.x a user message has an object of another shape here.
    #[serde(default)]
    model: Option<serde_json::Value>,
    tokens: Option<Tokens>,
    time: Option<Time>,
    path: Option<PathInfo>,
}

/// The name in a 2.x `model` object (or a bare string).
fn model_name(v: &serde_json::Value) -> Option<String> {
    let name = match v {
        serde_json::Value::String(s) => Some(s.as_str()),
        serde_json::Value::Object(o) => o.get("id").or_else(|| o.get("modelID")).and_then(serde_json::Value::as_str),
        _ => None,
    };
    name.filter(|n| !n.is_empty()).map(str::to_string)
}

/// Where the collector stopped in each generation's message table.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// Newest `time_updated` seen in the 1.x `message` table.
    max_updated: i64,
    /// The same for the 2.x `session_message` table (absent in a cursor written before 2.x was supported).
    #[serde(default)]
    max_updated_v2: Option<i64>,
}

/// One generation of OpenCode's database layout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    V1,
    V2,
}

impl Layout {
    fn messages(self) -> &'static str {
        match self {
            Layout::V1 => "message",
            Layout::V2 => "session_message",
        }
    }

    fn sessions(self) -> &'static str {
        match self {
            Layout::V1 => "session",
            Layout::V2 => "session_v2",
        }
    }

    /// Columns: id, session id, created, updated, data, the session's directory, and the role column (2.x; `NULL` in 1.x).
    fn sql(self, with_sessions: bool) -> &'static str {
        match (self, with_sessions) {
            (Layout::V1, true) => {
                "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, s.directory, NULL
                 FROM message m LEFT JOIN session s ON s.id = m.session_id WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
            }
            (Layout::V1, false) => {
                "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, NULL, NULL
                 FROM message m WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
            }
            (Layout::V2, true) => {
                "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, s.directory, m.type
                 FROM session_message m LEFT JOIN session_v2 s ON s.id = m.session_id WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
            }
            (Layout::V2, false) => {
                "SELECT m.id, m.session_id, m.time_created, m.time_updated, m.data, NULL, m.type
                 FROM session_message m WHERE m.time_updated >= ?1 ORDER BY m.time_updated"
            }
        }
    }
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

/// What a read made of the rows, summed over both layouts.
#[derive(Default)]
struct Counts {
    assistant: u64,
    with_tokens: u64,
    skipped: u64,
}

/// Reads the rows of one layout updated since `prev` (minus an overlap) and returns the newest `time_updated` seen.
fn read_layout(
    conn: &Connection,
    layout: Layout,
    prev: Option<i64>,
    ctx: &CollectCtx<'_>,
    acc: &mut Accumulator,
    summary: &mut CollectSummary,
    counts: &mut Counts,
) -> rusqlite::Result<i64> {
    let with_sessions = has_table(conn, layout.sessions())?;
    let start = prev.map_or(0, |p| (p - OVERLAP_MS).max(0));
    let mut max_updated = prev.unwrap_or(0);

    let mut stmt = conn.prepare(layout.sql(with_sessions))?;
    let mut rows = stmt.query([start])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let session: Option<String> = row.get(1)?;
        let created: Option<i64> = row.get(2)?;
        let updated: i64 = row.get(3)?;
        let data: String = row.get(4)?;
        let directory: Option<String> = row.get(5)?;
        let kind: Option<String> = row.get(6)?;
        max_updated = max_updated.max(updated);

        let Ok(d) = serde_json::from_str::<Data>(&data) else {
            counts.skipped += 1;
            continue;
        };
        if kind.as_deref().or(d.role.as_deref()) != Some("assistant") {
            continue;
        }
        counts.assistant += 1;
        let Some(t) = d.tokens else { continue };
        counts.with_tokens += 1;
        let (input, output, reasoning) = (t.input.unwrap_or(0), t.output.unwrap_or(0), t.reasoning.unwrap_or(0));
        let (cache_read, cache_write) = t.cache.map_or((0, 0), |c| (c.read.unwrap_or(0), c.write.unwrap_or(0)));
        // A row that has just been created streams in at zero: nothing to count yet.
        if input + output + reasoning + cache_read + cache_write == 0 {
            continue;
        }
        let Some(ts) = d.time.and_then(|t| t.created).or(created).filter(|t| *t > 0) else {
            counts.skipped += 1;
            continue;
        };
        let cwd = d.path.and_then(|p| p.cwd).filter(|c| !c.is_empty()).or(directory);
        let model = d.model_id.filter(|m| !m.is_empty()).or_else(|| d.model.as_ref().and_then(model_name)).unwrap_or_else(|| "unknown".to_string());
        let ev = UsageEvent {
            agent: ID,
            model,
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
            // The same message id in both layouts (a migrated database) is one event, not two.
            dedupe_key: format!("opencode:{id}"),
        };
        summary.saw_event(ev.ts_utc_ms);
        acc.push(ev);
    }
    Ok(max_updated)
}

impl AgentCollector for OpenCodeCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "OpenCode",
            data_sources: &[
                "~/.local/share/opencode/opencode.db (2.x: session_message.data → tokens; 1.x: message.data → tokens)",
            ],
            caveats: &[
                VALIDATION_NOTE,
                "reasoning tokens are counted as output, as OpenCode's own stats does",
                "2.x session totals run a little above the stored messages (calls such as titles are not stored as messages); messages are what is counted",
            ],
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
        let old = cursor.and_then(|c| serde_json::from_str::<State>(c.state.as_deref()?).ok());

        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_millis(1500))?;
        conn.pragma_update(None, "query_only", "ON")?;

        let mut layouts = Vec::new();
        for layout in [Layout::V1, Layout::V2] {
            if has_table(&conn, layout.messages())? {
                layouts.push(layout);
            }
        }
        if layouts.is_empty() {
            return Err(CollectError::Unavailable(
                "opencode.db has neither a `session_message` (OpenCode 2.x) nor a `message` (1.x) table: the OpenCode schema has changed".into(),
            ));
        }

        let mut next = State { max_updated: old.as_ref().map_or(0, |s| s.max_updated), max_updated_v2: old.as_ref().and_then(|s| s.max_updated_v2) };
        let mut acc = Accumulator::default();
        let mut counts = Counts::default();
        for layout in layouts {
            match layout {
                Layout::V1 => {
                    let prev = old.as_ref().map(|s| s.max_updated);
                    next.max_updated = read_layout(&conn, layout, prev, ctx, &mut acc, &mut summary, &mut counts)?;
                }
                Layout::V2 => {
                    let prev = old.as_ref().and_then(|s| s.max_updated_v2);
                    next.max_updated_v2 = Some(read_layout(&conn, layout, prev, ctx, &mut acc, &mut summary, &mut counts)?);
                }
            }
        }

        // Assistant messages exist but none carries a tokens block: the shape has probably changed.
        if counts.assistant > 0 && counts.with_tokens == 0 {
            return Err(CollectError::Unavailable(format!(
                "{} OpenCode assistant message(s) found without a `tokens` block: the message format may have changed",
                counts.assistant
            )));
        }
        summary.skipped_records += counts.skipped;
        summary.files_read = 1;
        sink.commit(SinkBatch {
            events: acc.into_events(),
            cursors: vec![CursorUpdate { path: path_str(path), size, mtime_ms: mtime, offset: 0, state: serde_json::to_string(&next).ok() }],
        })?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests;
