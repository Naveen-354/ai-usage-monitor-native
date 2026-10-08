//! Antigravity (CLI `agy` and IDE) collector.
//!
//! Source (verified by reading the schema embedded in `agy.exe`, then validating on real data — see
//! docs/COLLECTORS.md): each conversation is a SQLite database whose `gen_metadata.data` column holds one
//! protobuf blob per model generation, with the provider-reported usage inside.
//!
//! Two traps, both measured on a real install:
//!   · 70% of the newer rows have no `created_at` in the blob; the time must be recovered from the matching
//!     row in `steps` — otherwise everything after the format change silently disappears.
//!   · The same usage is repeated in `retry_infos`; only the top-level usage block is read.
//!
//! This is an internal, undocumented format. If rows exist but none decode to a usage block, the collector
//! reports `Unavailable` ("format not recognised") rather than zero.

mod decode;
mod wire;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::env::{existing_dirs, path_str};
use super::tail::mtime_ms;
use super::walk;
use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, SinkBatch, WatchSpec,
};
use crate::database::CursorUpdate;
use crate::model::{Accuracy, AgentId, ProjectRef, UsageEvent};
use decode::{decode_generation, step_timestamp};

pub const ID: AgentId = "antigravity";
const SOURCE: &str = "local-session-data";
/// Rows are occasionally rewritten shortly after insertion; re-read this many trailing rows each time.
const OVERLAP_ROWS: i64 = 8;

pub struct AntigravityCollector;

fn conversation_dirs(env: &Env) -> Vec<PathBuf> {
    let base = env.home.join(".gemini");
    existing_dirs(&[base.join("antigravity-cli").join("conversations"), base.join("antigravity").join("conversations")])
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DbState {
    max_idx: i64,
}

/// Size and mtime of a database *including its WAL*, since a live agent writes to `<db>-wal` first.
fn db_stat(path: &Path) -> Option<(u64, i64)> {
    let main = std::fs::metadata(path).ok()?;
    let mut size = main.len();
    let mut mtime = mtime_ms(&main);
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    if let Ok(w) = std::fs::metadata(PathBuf::from(wal)) {
        size += w.len();
        mtime = mtime.max(mtime_ms(&w));
    }
    Some((size, mtime))
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    c.busy_timeout(std::time::Duration::from_millis(1500))?;
    c.pragma_update(None, "query_only", "ON")?;
    Ok(c)
}

/// `Ok(false)` = a healthy database without that table; `Err` = not a readable database at all.
fn has_table(c: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(c.query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", [name], |_| Ok(())).optional()?.is_some())
}

/// `file:///D:/invest%20mf/x` or `file:///d%3A/x` → `D:/invest mf/x`; `file:///home/me/x` → `/home/me/x`.
pub fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // Work on bytes, never on `&str` slices: a multibyte character after '%' must not panic.
        let decoded = (b[i] == b'%')
            .then(|| b.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match decoded {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    let mut s = String::from_utf8(out).ok()?;
    // `/D:/x` → `D:/x` (Windows drive paths carry a leading slash in file URIs)
    let bytes = s.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        s.remove(0);
    }
    (!s.is_empty()).then_some(s)
}

/// Workspace folder of a conversation, from `conversation_summaries` next to the conversations directory.
/// Only `conversation_id` and `workspace_uris` are ever selected (never the title or preview).
struct ProjectLookup {
    conn: Option<Connection>,
    cache: HashMap<String, Option<ProjectRef>>,
}

impl ProjectLookup {
    fn open(conversations_dir: &Path) -> ProjectLookup {
        let db = conversations_dir.parent().map(|p| p.join("conversation_summaries.db"));
        let conn = db.filter(|p| p.is_file()).and_then(|p| open_read_only(&p).ok()).filter(|c| has_table(c, "conversation_summaries").unwrap_or(false));
        ProjectLookup { conn, cache: HashMap::new() }
    }

    fn project(&mut self, ctx: &CollectCtx<'_>, conversation_id: &str) -> Option<ProjectRef> {
        if let Some(hit) = self.cache.get(conversation_id) {
            return hit.clone();
        }
        let found = self.conn.as_ref().and_then(|c| {
            let raw: Option<String> = c
                .query_row("SELECT workspace_uris FROM conversation_summaries WHERE conversation_id = ?1", [conversation_id], |r| r.get(0))
                .ok()?;
            let uris: Vec<String> = serde_json::from_str(&raw?).ok()?;
            let path = uris.iter().find_map(|u| uri_to_path(u))?;
            ctx.projects.resolve(&path)
        });
        self.cache.insert(conversation_id.to_string(), found.clone());
        found
    }
}

#[derive(Default)]
struct DbTotals {
    rows_seen: u64,
    decoded: u64,
    not_usage: u64,
    decode_failed: u64,
    undatable: u64,
    zero_usage: u64,
}

impl AgentCollector for AntigravityCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Antigravity",
            data_sources: &[
                "~/.gemini/antigravity-cli/conversations/*.db (gen_metadata protobuf)",
                "~/.gemini/antigravity/conversations/*.db (IDE)",
            ],
            caveats: &[
                "Internal, undocumented format; decoded using the schema embedded in agy 1.3.1.",
                "Newer rows carry no start time in the blob; the time is recovered from the matching step.",
            ],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let roots = conversation_dirs(env);
        let local = env.local_app_data.as_ref().map(|d| d.join("agy").join("bin"));
        let binary = env.which(&["agy", "antigravity"], &local.into_iter().collect::<Vec<_>>());
        let presence = match (binary.is_some(), roots.is_empty()) {
            (true, _) => Presence::Installed,
            (false, false) => Presence::DataOnly,
            (false, true) => Presence::NotFound,
        };
        Detection {
            presence,
            note: (presence == Presence::DataOnly).then(|| "conversation data found, but `agy` is not on PATH".to_string()),
            roots,
            binary,
        }
    }

    fn watch_specs(&self, d: &Detection) -> Vec<WatchSpec> {
        // A live agent writes to `<db>-wal` first, so watch both.
        d.roots.iter().map(|p| WatchSpec { path: p.clone(), recursive: false, suffixes: &[".db", ".db-wal"] }).collect()
    }

    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        let mut summary = CollectSummary::default();
        let mut totals = DbTotals::default();
        let (mut unreadable, mut no_table) = (0u32, 0u32);

        for root in &ctx.detection.roots {
            let mut projects = ProjectLookup::open(root);
            for (path, _) in walk::files_with_suffix(root, ".db", 0) {
                ctx.check_cancel()?;
                summary.files_seen += 1;
                let Some((size, mtime)) = db_stat(&path) else { continue };
                let cursor = ctx.cursor(&path);
                if cursor.is_some_and(|c| c.size == size && c.mtime_ms == mtime) {
                    continue;
                }
                let prev_max = cursor.and_then(|c| serde_json::from_str::<DbState>(c.state.as_deref()?).ok()).map_or(-1, |s| s.max_idx);

                let conn = match open_read_only(&path) {
                    Ok(c) => c,
                    Err(_) => {
                        unreadable += 1;
                        continue;
                    }
                };
                match has_table(&conn, "gen_metadata") {
                    Ok(true) => {}
                    Ok(false) => {
                        no_table += 1;
                        continue;
                    }
                    Err(_) => {
                        unreadable += 1; // not a database (corrupt, or still being created)
                        continue;
                    }
                }
                match read_db(ctx, &conn, &path, prev_max, &mut projects, &mut totals) {
                    Ok((events, max_idx)) => {
                        summary.files_read += 1;
                        for e in &events {
                            summary.saw_event(e.ts_utc_ms);
                        }
                        sink.commit(SinkBatch {
                            events,
                            cursors: vec![CursorUpdate {
                                path: path_str(&path),
                                size,
                                mtime_ms: mtime,
                                offset: max_idx.max(0) as u64,
                                state: serde_json::to_string(&DbState { max_idx }).ok(),
                            }],
                        })?;
                    }
                    Err(_) => unreadable += 1, // locked/corrupt right now: retry next cycle
                }
            }
        }

        summary.skipped_records += totals.undatable + totals.zero_usage + totals.decode_failed;
        if unreadable > 0 {
            summary.notes.push(format!("{unreadable} database(s) unreadable right now, will retry"));
        }
        if totals.undatable > 0 {
            summary.notes.push(format!("{} generation(s) had no recoverable timestamp and were not counted", totals.undatable));
        }

        // Rows exist but not one decoded to a usage block: the format has probably changed. Say so.
        if totals.rows_seen > 0 && totals.decoded == 0 {
            return Err(CollectError::Unavailable(format!(
                "Antigravity data format not recognised: {} row(s) found, none contained a usage block \
                 ({} undecodable, {} without usage). The app expects the agy 1.3.1 layout.",
                totals.rows_seen, totals.decode_failed, totals.not_usage
            )));
        }
        if no_table > 0 && totals.rows_seen == 0 && summary.files_read == 0 && ctx.cursors.is_empty() && summary.files_seen == no_table {
            return Err(CollectError::Unavailable("Antigravity databases found, but none has the expected `gen_metadata` table".into()));
        }
        Ok(summary)
    }
}

/// Decode every new row of one conversation database. Returns the events and the highest `idx` seen.
fn read_db(
    ctx: &CollectCtx<'_>,
    conn: &Connection,
    path: &Path,
    prev_max: i64,
    projects: &mut ProjectLookup,
    totals: &mut DbTotals,
) -> rusqlite::Result<(Vec<UsageEvent>, i64)> {
    let conversation = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let start = if prev_max < 0 { 0 } else { (prev_max - OVERLAP_ROWS).max(0) };
    let has_steps = has_table(conn, "steps")?;

    let mut rows = conn.prepare("SELECT idx, data FROM gen_metadata WHERE idx >= ?1 ORDER BY idx")?;
    let mut step_stmt = if has_steps { Some(conn.prepare("SELECT metadata FROM steps WHERE idx = ?1")?) } else { None };

    let project = projects.project(ctx, &conversation);
    let mut events = Vec::new();
    let mut max_idx = prev_max;

    let mut it = rows.query([start])?;
    while let Some(row) = it.next()? {
        let idx: i64 = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        max_idx = max_idx.max(idx);
        totals.rows_seen += 1;

        let g = match decode_generation(&blob) {
            Ok(Some(g)) => g,
            Ok(None) => {
                totals.not_usage += 1;
                continue;
            }
            Err(_) => {
                totals.decode_failed += 1;
                continue;
            }
        };
        totals.decoded += 1;
        if g.total() == 0 {
            totals.zero_usage += 1;
            continue;
        }

        // Time: the generation's own start time, else the matching step's.
        let secs = g.created_secs.filter(|s| *s > 0).or_else(|| {
            let step = g.step_index?;
            let stmt = step_stmt.as_mut()?;
            let meta: Option<Vec<u8>> = stmt.query_row([i64::from(step)], |r| r.get(0)).ok();
            step_timestamp(&meta?)
        });
        let Some(secs) = secs else {
            totals.undatable += 1;
            continue;
        };

        events.push(UsageEvent {
            agent: ID,
            model: g.model.clone().unwrap_or_else(|| "unknown".to_string()),
            ts_utc_ms: secs.saturating_mul(1000),
            input_tokens: g.input,
            output_tokens: g.output,
            cache_read_tokens: g.cache_read,
            cache_write_tokens: g.cache_write,
            reasoning_tokens: Some(g.thinking),
            session_id: Some(conversation.clone()),
            project: project.clone(),
            source: SOURCE,
            accuracy: Accuracy::Real,
            dedupe_key: match &g.response_id {
                Some(id) => format!("agy:{id}"),
                None => format!("agy:{conversation}:{idx}"),
            },
        });
    }
    Ok((events, max_idx))
}

#[cfg(test)]
mod tests;
