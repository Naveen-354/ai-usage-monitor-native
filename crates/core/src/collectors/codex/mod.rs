//! Codex collector (CLI and Desktop share `~/.codex`).
//!
//! Source (verified, see docs/COLLECTORS.md): `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
//! `event_msg/token_count` events carry `info.total_token_usage`, which is **cumulative per session**, so a
//! usage event is the *delta* between consecutive totals. On the author's machine this reproduces Codex's own
//! per-thread `tokens_used` exactly for 55 of 56 threads.
//!
//! Codex's `input_tokens` already *includes* cached tokens, so `fresh input = input − cached`, and the
//! normalised total equals Codex's `total_tokens` (= input + output).
//!
//! Running totals live in the file cursor's `state`, so tailing a growing rollout across runs never
//! re-counts what came before.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use super::accumulate::Accumulator;
use super::env::path_str;
use super::tail::{self, LineVerdict};
use super::walk;
use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, SinkBatch, WatchSpec,
};
use crate::database::CursorUpdate;
use crate::model::{Accuracy, AgentId, UsageEvent};

pub const ID: AgentId = "codex";
const SOURCE: &str = "local-session-data";
/// sessions / YYYY / MM / DD / file
const MAX_DEPTH: usize = 4;

pub struct CodexCollector;

fn sessions_dir(env: &Env) -> PathBuf {
    env.home.join(".codex").join("sessions")
}

#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(default)]
    payload: Option<Payload<'a>>,
}

#[derive(Deserialize)]
struct Payload<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    session_id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    model: Option<Cow<'a, str>>,
    #[serde(default)]
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    total_token_usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct TokenUsage {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

/// Cumulative counters at one point in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
struct Totals {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl Totals {
    fn from_usage(u: &TokenUsage) -> Totals {
        let (input, output) = (u.input_tokens.unwrap_or(0), u.output_tokens.unwrap_or(0));
        Totals {
            input,
            cached: u.cached_input_tokens.unwrap_or(0).min(input),
            output,
            reasoning: u.reasoning_output_tokens.unwrap_or(0),
            // Codex's own total is input + output; fall back to that if absent.
            total: u.total_tokens.unwrap_or(input + output),
        }
    }

    fn since(self, prev: Totals) -> Totals {
        Totals {
            input: self.input.saturating_sub(prev.input),
            cached: self.cached.saturating_sub(prev.cached),
            output: self.output.saturating_sub(prev.output),
            reasoning: self.reasoning.saturating_sub(prev.reasoning),
            total: self.total.saturating_sub(prev.total),
        }
    }
}

/// Everything needed to resume a rollout mid-file. Persisted in `file_cursors.state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct FileState {
    sid: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    prev: Option<Totals>,
    /// Bumped when the cumulative counter goes backwards (context compaction / restart) so that a total
    /// that recurs later in the session cannot collide with an earlier event key.
    epoch: u32,
}

/// `rollout-2026-08-12T12-18-16-<uuid>.jsonl` → `<uuid>` (fallback session id).
fn uuid_from_file_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    let rest = stem.strip_prefix("rollout-")?;
    // timestamp is 19 chars (YYYY-MM-DDTHH-MM-SS) followed by '-'
    let id = rest.get(20..)?;
    (!id.is_empty()).then(|| id.to_string())
}

impl AgentCollector for CodexCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Codex",
            data_sources: &["~/.codex/sessions/**/rollout-*.jsonl (event_msg/token_count)"],
            caveats: &[
                "Usage is the delta of Codex's cumulative per-session counter.",
                "Custom CODEX_HOME locations are not followed yet.",
            ],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let root = sessions_dir(env);
        let has_data = root.is_dir();
        let binary = env.which(&["codex"], &[]);
        let presence = match (binary.is_some(), has_data) {
            (true, _) => Presence::Installed,
            (false, true) => Presence::DataOnly,
            (false, false) => Presence::NotFound,
        };
        Detection {
            presence,
            roots: if has_data { vec![root] } else { vec![] },
            note: (presence == Presence::DataOnly).then(|| "session data found, but the `codex` CLI is not on PATH (Codex Desktop?)".to_string()),
            binary,
        }
    }

    fn watch_specs(&self, d: &Detection) -> Vec<WatchSpec> {
        d.roots.iter().map(|p| WatchSpec { path: p.clone(), recursive: true, suffixes: &[".jsonl"] }).collect()
    }

    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        let mut summary = CollectSummary::default();
        let mut unreadable = 0u32;

        for root in &ctx.detection.roots {
            for (path, meta) in walk::files_with_suffix(root, ".jsonl", MAX_DEPTH) {
                ctx.check_cancel()?;
                if !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rollout-")) {
                    continue;
                }
                summary.files_seen += 1;
                let cursor = ctx.cursor(&path);
                if let Some(c) = cursor {
                    if !tail::changed_since(&meta, c.size, c.mtime_ms) {
                        continue;
                    }
                }
                // Resume only with a valid saved state and a file that has not shrunk; otherwise start
                // over (event keys are deterministic, so a re-read can never double count).
                let (start, mut st) = match cursor.and_then(|c| Some((c, serde_json::from_str::<FileState>(c.state.as_deref()?).ok()?))) {
                    Some((c, st)) if meta.len() >= c.offset => (c.offset, st),
                    _ => (0, FileState::default()),
                };
                if st.sid.is_none() {
                    st.sid = uuid_from_file_name(&path);
                }

                let mut acc = Accumulator::default();
                let mut skipped = 0u64;
                let read = tail::read_new_lines(&path, start, |line, terminated| {
                    // Fast path: only three record kinds matter, and they always contain one of these.
                    // Only a *complete* line may be skipped on a keyword miss: a half-written line may simply
                    // not have reached the keyword yet, and must be retried, never consumed.
                    if terminated
                        && !(line.contains("\"token_count\"") || line.contains("\"session_meta\"") || line.contains("\"turn_context\""))
                    {
                        return LineVerdict::Consumed;
                    }
                    let parsed = match serde_json::from_str::<Line<'_>>(line) {
                        Ok(l) => l,
                        Err(_) if !terminated => return LineVerdict::Incomplete,
                        Err(_) => {
                            skipped += 1;
                            return LineVerdict::Consumed;
                        }
                    };
                    let Some(p) = &parsed.payload else { return LineVerdict::Consumed };
                    match parsed.kind.as_deref() {
                        // A rollout's identity is its file name (it equalled `payload.id` in every file checked).
                        // Later `session_meta` lines can be copies of a *parent* thread's header (forks), so they
                        // must never re-label the session or move the project: first value wins.
                        Some("session_meta") => {
                            if st.sid.is_none() {
                                st.sid = p.id.as_deref().or(p.session_id.as_deref()).filter(|s| !s.is_empty()).map(str::to_string);
                            }
                            if st.cwd.is_none() {
                                st.cwd = p.cwd.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
                            }
                        }
                        Some("turn_context") => {
                            if let Some(m) = p.model.as_deref().filter(|s| !s.is_empty()) {
                                st.model = Some(m.to_string());
                            }
                            if let Some(c) = p.cwd.as_deref().filter(|s| !s.is_empty()) {
                                st.cwd = Some(c.to_string());
                            }
                        }
                        Some("event_msg") if p.kind.as_deref() == Some("token_count") => {
                            let Some(usage) = p.info.as_ref().and_then(|i| i.total_token_usage.as_ref()) else {
                                return LineVerdict::Consumed; // rate-limit-only update: no usage
                            };
                            let cur = Totals::from_usage(usage);
                            let Some(ts) = parsed.timestamp.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()) else {
                                skipped += 1;
                                return LineVerdict::Consumed;
                            };
                            let delta = match st.prev {
                                Some(prev) if cur.total == prev.total => return LineVerdict::Consumed, // repeated total
                                Some(prev) if cur.total < prev.total => {
                                    st.epoch += 1; // counter went backwards: new baseline
                                    cur
                                }
                                Some(prev) => cur.since(prev),
                                None => cur,
                            };
                            st.prev = Some(cur);
                            let Some(sid) = st.sid.clone() else {
                                skipped += 1;
                                return LineVerdict::Consumed;
                            };
                            let ev = UsageEvent {
                                agent: ID,
                                model: st.model.clone().unwrap_or_else(|| "unknown".to_string()),
                                ts_utc_ms: ts.timestamp_millis(),
                                input_tokens: delta.input.saturating_sub(delta.cached),
                                output_tokens: delta.output,
                                cache_read_tokens: delta.cached,
                                cache_write_tokens: 0,
                                reasoning_tokens: Some(delta.reasoning),
                                session_id: Some(sid.clone()),
                                project: st.cwd.as_deref().and_then(|c| ctx.projects.resolve(c)),
                                source: SOURCE,
                                accuracy: Accuracy::Real,
                                dedupe_key: format!("codex:{sid}:{}:{}", st.epoch, cur.total),
                            };
                            if ev.is_empty() {
                                skipped += 1;
                            } else {
                                summary.saw_event(ev.ts_utc_ms);
                                acc.push(ev);
                            }
                        }
                        _ => {}
                    }
                    LineVerdict::Consumed
                });

                match read {
                    Ok(t) => {
                        summary.files_read += 1;
                        summary.skipped_records += skipped;
                        sink.commit(SinkBatch {
                            events: acc.into_events(),
                            cursors: vec![CursorUpdate {
                                path: path_str(&path),
                                size: t.size,
                                mtime_ms: t.mtime_ms,
                                offset: t.new_offset,
                                state: serde_json::to_string(&st).ok(),
                            }],
                        })?;
                    }
                    // Codex keeps its active rollout open; if it is momentarily unreadable, retry later.
                    Err(_) => unreadable += 1,
                }
            }
        }
        if unreadable > 0 {
            summary.notes.push(format!("{unreadable} file(s) unreadable right now, will retry"));
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests;
