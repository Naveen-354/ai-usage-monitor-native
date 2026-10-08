//! Claude Code collector.
//!
//! Source (verified on a real install, see docs/COLLECTORS.md): `~/.claude/projects/**/<session>.jsonl`,
//! records with `type == "assistant"` carrying `message.usage`.
//!
//! The one trap: Claude Code writes a single API response on *several* lines (one per content block), and
//! resumed sessions copy earlier history into new files. Measured on the author's machine: 22,504 assistant
//! records = 10,061 unique responses. Summing records would over-count 2.2×, so usage is de-duplicated on
//! `message.id` (globally, across files) and merged by per-field maximum.

use std::borrow::Cow;
use std::path::PathBuf;

use chrono::DateTime;
use serde::Deserialize;

use super::accumulate::Accumulator;
use super::env::path_str;
use super::tail::{self, LineVerdict};
use super::walk;
use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, SinkBatch,
};
use crate::database::CursorUpdate;
use crate::model::{Accuracy, AgentId, UsageEvent};

pub const ID: AgentId = "claude";
const SOURCE: &str = "local-session-data";
/// project dir / session file, plus room for `subagents/` style nesting.
const MAX_DEPTH: usize = 4;

pub struct ClaudeCollector;

fn projects_dir(env: &Env) -> PathBuf {
    env.home.join(".claude").join("projects")
}

// Only the fields we need. Everything else (message text, tool output, ...) is skipped by serde without
// being copied, stored or logged.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    cwd: Option<Cow<'a, str>>,
    #[serde(rename = "sessionId", borrow, default)]
    session_id: Option<Cow<'a, str>>,
    #[serde(rename = "requestId", borrow, default)]
    request_id: Option<Cow<'a, str>>,
    #[serde(default)]
    message: Option<Message<'a>>,
}

#[derive(Deserialize)]
struct Message<'a> {
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    model: Option<Cow<'a, str>>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize)]
struct OutputDetails {
    thinking_tokens: Option<u64>,
}

enum Parsed {
    Event(Box<UsageEvent>),
    /// Understood, but deliberately not counted (zero usage, synthetic, undatable, no dedupe key).
    Skip,
    /// Not a usage record at all.
    Other,
}

fn to_event(ctx: &CollectCtx<'_>, l: &Line<'_>) -> Parsed {
    if l.kind.as_deref() != Some("assistant") {
        return Parsed::Other;
    }
    let Some(msg) = &l.message else { return Parsed::Other };
    let Some(u) = &msg.usage else { return Parsed::Other };

    let model = msg.model.as_deref().unwrap_or("").trim();
    if model == "<synthetic>" {
        return Parsed::Skip;
    }
    let (input, output) = (u.input_tokens.unwrap_or(0), u.output_tokens.unwrap_or(0));
    let (cache_read, cache_write) = (u.cache_read_input_tokens.unwrap_or(0), u.cache_creation_input_tokens.unwrap_or(0));
    if input + output + cache_read + cache_write == 0 {
        return Parsed::Skip;
    }
    let Some(ts) = l.timestamp.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()) else {
        return Parsed::Skip; // never guess a time: an undated event cannot be placed in a period
    };
    // Without a response id we cannot de-duplicate safely; skipping (and counting it) beats double counting.
    let key = match (msg.id.as_deref(), l.request_id.as_deref()) {
        (Some(id), _) if !id.is_empty() => format!("claude:{id}"),
        (_, Some(req)) if !req.is_empty() => format!("claude:req:{req}"),
        _ => return Parsed::Skip,
    };
    Parsed::Event(Box::new(UsageEvent {
        agent: ID,
        model: if model.is_empty() { "unknown".into() } else { model.to_string() },
        ts_utc_ms: ts.timestamp_millis(),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: u.output_tokens_details.as_ref().and_then(|d| d.thinking_tokens),
        session_id: l.session_id.as_deref().filter(|s| !s.is_empty()).map(str::to_string),
        project: l.cwd.as_deref().and_then(|c| ctx.projects.resolve(c)),
        source: SOURCE,
        accuracy: Accuracy::Real,
        dedupe_key: key,
    }))
}

impl AgentCollector for ClaudeCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Claude Code",
            data_sources: &["~/.claude/projects/**/*.jsonl (assistant records, message.usage)"],
            caveats: &[
                "One API response is logged on several lines; usage is de-duplicated by message id.",
                "Counts come from the provider-reported usage block (actual, not estimated).",
            ],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let root = projects_dir(env);
        let has_data = root.is_dir();
        let binary = env.which(&["claude"], &[env.home.join(".local").join("bin"), env.home.join(".claude").join("local")]);
        let presence = match (binary.is_some(), has_data) {
            (true, _) => Presence::Installed,
            (false, true) => Presence::DataOnly,
            (false, false) => Presence::NotFound,
        };
        Detection {
            presence,
            roots: if has_data { vec![root] } else { vec![] },
            note: (presence == Presence::DataOnly).then(|| "session data found, but `claude` is not on PATH".to_string()),
            binary,
        }
    }

    fn watch_specs(&self, d: &Detection) -> Vec<super::WatchSpec> {
        d.roots.iter().map(|p| super::WatchSpec { path: p.clone(), recursive: true, suffixes: &[".jsonl"] }).collect()
    }

    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        let mut summary = CollectSummary::default();
        let mut unreadable = 0u32;

        for root in &ctx.detection.roots {
            for (path, meta) in walk::files_with_suffix(root, ".jsonl", MAX_DEPTH) {
                ctx.check_cancel()?;
                summary.files_seen += 1;
                let cursor = ctx.cursor(&path);
                if let Some(c) = cursor {
                    if !tail::changed_since(&meta, c.size, c.mtime_ms) {
                        continue;
                    }
                }
                let start = cursor.map_or(0, |c| c.offset);

                let mut acc = Accumulator::default();
                let mut skipped = 0u64;
                let read = tail::read_new_lines(&path, start, |line, terminated| {
                    // Fast path: most lines (user turns, tool results) carry no usage block. Only a *complete*
                    // line may be skipped on a keyword miss: a half-written line may not have reached
                    // `"usage"` yet and must be retried, never consumed.
                    if terminated && !line.contains("\"usage\"") {
                        return LineVerdict::Consumed;
                    }
                    match serde_json::from_str::<Line<'_>>(line) {
                        Ok(l) => match to_event(ctx, &l) {
                            Parsed::Event(e) => {
                                summary.saw_event(e.ts_utc_ms);
                                acc.push(*e);
                            }
                            Parsed::Skip => skipped += 1,
                            Parsed::Other => {}
                        },
                        // Half-written last line: leave it for the next run.
                        Err(_) if !terminated => return LineVerdict::Incomplete,
                        Err(_) => skipped += 1,
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
                                state: None,
                            }],
                        })?;
                    }
                    // Active session files can be momentarily unreadable; retry on the next cycle.
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
