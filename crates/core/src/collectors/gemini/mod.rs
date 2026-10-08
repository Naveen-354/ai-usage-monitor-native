//! Gemini CLI collector.
//!
//! Source (verified, see docs/COLLECTORS.md): `~/.gemini/tmp/<project>/chats/session-*.jsonl` (current) and
//! `session-*.json` (legacy). Messages with `type == "gemini"` carry
//! `tokens {input, output, cached, thoughts, tool, total}`.
//!
//! Verified identity over 1,812 real messages: `total = input + output + thoughts + tool`, and `cached ≤ input`
//! (cached is a *subset* of input; thoughts are *additional* to output). So:
//! `fresh input = input − cached + tool`, `cache read = cached`, `output = output + thoughts`.
//!
//! The JSONL also contains `{"$set":{"messages":[…]}}` snapshot records that re-emit whole message arrays:
//! 4,263 token-bearing records were only 1,812 distinct messages. De-duplicated on `(sessionId, message.id)`.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::accumulate::Accumulator;
use super::env::path_str;
use super::tail::{self, LineVerdict};
use super::walk;
use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, SinkBatch, WatchSpec,
};
use crate::database::CursorUpdate;
use crate::model::{Accuracy, AgentId, CollectorHealth, ProjectRef, UsageEvent};

pub const ID: AgentId = "gemini";
const SOURCE: &str = "local-session-data";

pub struct GeminiCollector;

fn tmp_dir(env: &Env) -> PathBuf {
    env.home.join(".gemini").join("tmp")
}

#[derive(Deserialize)]
struct Tokens {
    input: Option<u64>,
    output: Option<u64>,
    cached: Option<u64>,
    thoughts: Option<u64>,
    tool: Option<u64>,
}

#[derive(Deserialize)]
struct Msg<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    model: Option<Cow<'a, str>>,
    #[serde(default)]
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct SetBlock<'a> {
    #[serde(default, borrow)]
    messages: Vec<Msg<'a>>,
}

/// One JSONL line: a header (`sessionId`), a message, or a `$set` snapshot.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "sessionId", borrow, default)]
    session_id: Option<Cow<'a, str>>,
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    model: Option<Cow<'a, str>>,
    #[serde(default)]
    tokens: Option<Tokens>,
    #[serde(rename = "$set", default, borrow)]
    set: Option<SetBlock<'a>>,
}

#[derive(Deserialize)]
struct LegacyFile<'a> {
    #[serde(rename = "sessionId", borrow, default)]
    session_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    messages: Vec<Msg<'a>>,
}

/// Resume state for a JSONL chat file.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct FileState {
    sid: Option<String>,
}

struct MsgView<'a> {
    kind: Option<&'a str>,
    id: Option<&'a str>,
    timestamp: Option<&'a str>,
    model: Option<&'a str>,
    tokens: Option<&'a Tokens>,
}

fn view_of_msg<'a>(m: &'a Msg<'_>) -> MsgView<'a> {
    MsgView { kind: m.kind.as_deref(), id: m.id.as_deref(), timestamp: m.timestamp.as_deref(), model: m.model.as_deref(), tokens: m.tokens.as_ref() }
}

enum Parsed {
    Event(Box<UsageEvent>),
    Skip,
    Other,
}

fn to_event(sid: &str, project: &Option<ProjectRef>, m: &MsgView<'_>) -> Parsed {
    if m.kind != Some("gemini") {
        return Parsed::Other;
    }
    let Some(t) = m.tokens else { return Parsed::Other };
    let (input, output) = (t.input.unwrap_or(0), t.output.unwrap_or(0));
    let (cached, thoughts, tool) = (t.cached.unwrap_or(0), t.thoughts.unwrap_or(0), t.tool.unwrap_or(0));
    if input + output + thoughts + tool == 0 {
        return Parsed::Skip;
    }
    let Some(id) = m.id.filter(|s| !s.is_empty()) else { return Parsed::Skip };
    let Some(ts) = m.timestamp.and_then(|t| DateTime::parse_from_rfc3339(t).ok()) else { return Parsed::Skip };
    let cached = cached.min(input);
    Parsed::Event(Box::new(UsageEvent {
        agent: ID,
        model: m.model.filter(|s| !s.is_empty()).unwrap_or("unknown").to_string(),
        ts_utc_ms: ts.timestamp_millis(),
        input_tokens: input - cached + tool,
        output_tokens: output + thoughts,
        cache_read_tokens: cached,
        cache_write_tokens: 0,
        reasoning_tokens: Some(thoughts),
        session_id: Some(sid.to_string()),
        project: project.clone(),
        source: SOURCE,
        accuracy: Accuracy::Real,
        dedupe_key: format!("gemini:{sid}:{id}"),
    }))
}

/// `<tmp>/<project>/.project_root` holds the project folder path.
fn project_of(ctx: &CollectCtx<'_>, project_dir: &Path) -> Option<ProjectRef> {
    let raw = std::fs::read_to_string(project_dir.join(".project_root")).ok()?;
    ctx.projects.resolve(raw.trim())
}

impl AgentCollector for GeminiCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Gemini CLI",
            data_sources: &["~/.gemini/tmp/*/chats/session-*.jsonl and session-*.json (messages with a tokens block)"],
            caveats: &[
                "Chat snapshots repeat messages; usage is de-duplicated by (session, message id).",
                "`input` includes cached tokens upstream; they are split out here.",
            ],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let root = tmp_dir(env);
        let has_data = root.is_dir();
        let binary = env.which(&["gemini"], &[]);
        let presence = match (binary.is_some(), has_data) {
            (true, _) => Presence::Installed,
            (false, true) => Presence::DataOnly,
            (false, false) => Presence::NotFound,
        };
        Detection {
            presence,
            roots: if has_data { vec![root] } else { vec![] },
            note: (presence == Presence::DataOnly).then(|| "session data found, but the `gemini` CLI is not on PATH".to_string()),
            binary,
        }
    }

    fn watch_specs(&self, d: &Detection) -> Vec<WatchSpec> {
        d.roots.iter().map(|p| WatchSpec { path: p.clone(), recursive: true, suffixes: &[".jsonl", ".json"] }).collect()
    }

    fn get_health(&self, d: &Detection, last: Option<&super::RunOutcome>, events_total: u64, last_event_ms: Option<i64>) -> CollectorHealth {
        let mut h = super::default_health(ID, d, last, events_total, last_event_ms);
        // Historical data is not an error, but it should not look current either.
        if let Some(ms) = last_event_ms.filter(|_| d.presence == Presence::DataOnly) {
            if let Some(dt) = DateTime::<Utc>::from_timestamp_millis(ms) {
                let n = format!("last activity {}", dt.format("%Y-%m-%d"));
                h.note = Some(h.note.map_or(n.clone(), |old| format!("{old} · {n}")));
            }
        }
        h
    }

    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        let mut summary = CollectSummary::default();
        let mut unreadable = 0u32;

        for root in &ctx.detection.roots {
            for project_dir in walk::subdirs(root) {
                let chats = project_dir.join("chats");
                let mut files = walk::files_with_suffix(&chats, ".jsonl", 0);
                files.extend(walk::files_with_suffix(&chats, ".json", 0));
                if files.is_empty() {
                    continue;
                }
                let project = project_of(ctx, &project_dir);

                for (path, meta) in files {
                    ctx.check_cancel()?;
                    summary.files_seen += 1;
                    let cursor = ctx.cursor(&path);
                    if let Some(c) = cursor {
                        if !tail::changed_since(&meta, c.size, c.mtime_ms) {
                            continue;
                        }
                    }
                    let is_jsonl = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
                    let outcome = if is_jsonl {
                        read_jsonl(ctx, &path, cursor, &project, &mut summary)
                    } else {
                        read_legacy(&path, &project, &mut summary)
                    };
                    match outcome {
                        Ok((events, update)) => {
                            summary.files_read += 1;
                            sink.commit(SinkBatch { events, cursors: vec![update] })?;
                        }
                        Err(_) => unreadable += 1,
                    }
                }
            }
        }
        if unreadable > 0 {
            summary.notes.push(format!("{unreadable} file(s) unreadable right now, will retry"));
        }
        Ok(summary)
    }
}

type FileRead = std::io::Result<(Vec<UsageEvent>, CursorUpdate)>;

fn read_jsonl(ctx: &CollectCtx<'_>, path: &Path, cursor: Option<&super::Cursor>, project: &Option<ProjectRef>, summary: &mut CollectSummary) -> FileRead {
    let meta_len = std::fs::metadata(path)?.len();
    let (start, mut st) = match cursor.and_then(|c| Some((c, serde_json::from_str::<FileState>(c.state.as_deref()?).ok()?))) {
        Some((c, st)) if meta_len >= c.offset && st.sid.is_some() => (c.offset, st),
        _ => (0, FileState::default()),
    };
    let _ = ctx;
    let mut acc = Accumulator::default();
    let mut skipped = 0u64;

    let t = tail::read_new_lines(path, start, |line, terminated| {
        // Only a complete line may be skipped on a keyword miss (a half-written line may not have reached it).
        if terminated && !(line.contains("\"tokens\"") || line.contains("\"sessionId\"")) {
            return LineVerdict::Consumed;
        }
        let l = match serde_json::from_str::<Line<'_>>(line) {
            Ok(l) => l,
            Err(_) if !terminated => return LineVerdict::Incomplete,
            Err(_) => {
                skipped += 1;
                return LineVerdict::Consumed;
            }
        };
        if let Some(s) = l.session_id.as_deref().filter(|s| !s.is_empty()) {
            st.sid.get_or_insert_with(|| s.to_string());
        }
        let Some(sid) = st.sid.clone() else { return LineVerdict::Consumed };

        let mut handle = |m: MsgView<'_>| match to_event(&sid, project, &m) {
            Parsed::Event(e) => {
                summary.saw_event(e.ts_utc_ms);
                acc.push(*e);
            }
            Parsed::Skip => skipped += 1,
            Parsed::Other => {}
        };
        handle(MsgView { kind: l.kind.as_deref(), id: l.id.as_deref(), timestamp: l.timestamp.as_deref(), model: l.model.as_deref(), tokens: l.tokens.as_ref() });
        if let Some(set) = &l.set {
            for m in &set.messages {
                handle(view_of_msg(m));
            }
        }
        LineVerdict::Consumed
    })?;

    summary.skipped_records += skipped;
    Ok((
        acc.into_events(),
        CursorUpdate { path: path_str(path), size: t.size, mtime_ms: t.mtime_ms, offset: t.new_offset, state: serde_json::to_string(&st).ok() },
    ))
}

fn read_legacy(path: &Path, project: &Option<ProjectRef>, summary: &mut CollectSummary) -> FileRead {
    let text = std::fs::read_to_string(path)?;
    let meta = std::fs::metadata(path)?;
    let mut acc = Accumulator::default();
    let mut skipped = 0u64;
    match serde_json::from_str::<LegacyFile<'_>>(&text) {
        Ok(f) => {
            if let Some(sid) = f.session_id.as_deref().filter(|s| !s.is_empty()) {
                for m in &f.messages {
                    match to_event(sid, project, &view_of_msg(m)) {
                        Parsed::Event(e) => {
                            summary.saw_event(e.ts_utc_ms);
                            acc.push(*e);
                        }
                        Parsed::Skip => skipped += 1,
                        Parsed::Other => {}
                    }
                }
            }
        }
        // A legacy file being rewritten may be momentarily invalid: leave the cursor so we retry.
        Err(_) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unparseable chat json")),
    }
    summary.skipped_records += skipped;
    Ok((
        acc.into_events(),
        CursorUpdate { path: path_str(path), size: meta.len(), mtime_ms: tail::mtime_ms(&meta), offset: meta.len(), state: None },
    ))
}

#[cfg(test)]
mod tests;
