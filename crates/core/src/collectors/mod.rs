//! Collector framework.
//!
//! Each agent gets one adapter implementing [`AgentCollector`]. Adapters are the *only* code that knows
//! an agent's storage format; everything above them sees normalised [`UsageEvent`]s. A collector never
//! invents data: when an agent exposes no reliable local counts it reports `Unavailable` and emits nothing.

pub mod accumulate;
pub mod aider;
pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod env;
pub mod gemini;
pub mod ollama;
pub mod opencode;
pub mod project;
pub mod tail;
pub mod walk;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::database::CursorUpdate;
use crate::error::AppError;
use crate::model::{AgentId, Availability, CollectorHealth, UsageEvent};

pub use env::Env;
pub use project::ProjectResolver;

/// Is the agent on this machine?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Executable (or app) found.
    Installed,
    /// No executable found, but the agent's data directory exists (uninstalled, or not on PATH).
    DataOnly,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub presence: Presence,
    /// Verified, existing directories/files this collector will read. Also the only things ever watched.
    pub roots: Vec<PathBuf>,
    pub binary: Option<PathBuf>,
    /// Human-readable remark, e.g. "data found, CLI not on PATH".
    pub note: Option<String>,
}

impl Detection {
    pub fn not_found() -> Detection {
        Detection { presence: Presence::NotFound, roots: vec![], binary: None, note: None }
    }

    pub fn is_present(&self) -> bool {
        self.presence != Presence::NotFound
    }
}

/// Static description of an agent adapter (`getAgentInfo()` in the spec).
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub id: AgentId,
    pub name: &'static str,
    /// Where the numbers come from, for the diagnostics page.
    pub data_sources: &'static [&'static str],
    /// Honest limitations, shown in diagnostics.
    pub caveats: &'static [&'static str],
}

/// Where a collector stopped reading one source (loaded from `file_cursors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub size: u64,
    pub mtime_ms: i64,
    pub offset: u64,
    pub state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WatchSpec {
    pub path: PathBuf,
    pub recursive: bool,
    /// Only react to paths ending in one of these (empty = any). Keeps unrelated churn from waking us.
    pub suffixes: &'static [&'static str],
}

/// Everything a collector run needs, borrowed from the supervisor.
pub struct CollectCtx<'a> {
    pub env: &'a Env,
    pub detection: &'a Detection,
    pub cursors: &'a HashMap<String, Cursor>,
    pub projects: &'a ProjectResolver,
    pub cancel: &'a AtomicBool,
}

impl CollectCtx<'_> {
    pub fn cursor(&self, path: &std::path::Path) -> Option<&Cursor> {
        self.cursors.get(env::path_str(path).as_str())
    }

    /// Call between files so Pause/Quit take effect promptly during a long first import.
    pub fn check_cancel(&self) -> Result<(), CollectError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(CollectError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// One atomic unit of ingestion: events plus the cursor positions that produced them.
#[derive(Debug, Default)]
pub struct SinkBatch {
    pub events: Vec<UsageEvent>,
    pub cursors: Vec<CursorUpdate>,
}

pub trait BatchSink {
    fn commit(&mut self, batch: SinkBatch) -> Result<(), CollectError>;
}

/// What a run learned, independent of what was stored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CollectSummary {
    pub files_seen: u32,
    pub files_read: u32,
    /// Understood but intentionally not counted (zero-usage rows, undatable rows, ...). Never silent.
    pub skipped_records: u64,
    pub last_event_ms: Option<i64>,
    pub notes: Vec<String>,
}

impl CollectSummary {
    pub fn saw_event(&mut self, ts_ms: i64) {
        self.last_event_ms = Some(self.last_event_ms.map_or(ts_ms, |m| m.max(ts_ms)));
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    /// The agent has no reliable local token data (or its format is not recognised). Not a failure.
    #[error("{0}")]
    Unavailable(String),
    #[error("cancelled")]
    Cancelled,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Db(#[from] AppError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Outcome of the most recent run, kept by the supervisor for health reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Ok(CollectSummary),
    Unavailable(String),
    Failed(String),
    /// Collector panicked; isolated, other collectors unaffected.
    Panicked(String),
}

pub trait AgentCollector: Send + Sync {
    fn id(&self) -> AgentId;

    /// `getAgentInfo()`
    fn get_agent_info(&self) -> AgentInfo;

    /// `detect()` — cheap, `stat`-only. Called on every cycle so a newly installed agent is noticed.
    fn detect(&self, env: &Env) -> Detection;

    /// `collectUsage()` — incremental. Must be idempotent (events carry a `dedupe_key`) and must read
    /// only the fields it needs.
    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError>;

    /// Directories/files worth watching, derived from verified detection results.
    fn watch_specs(&self, detection: &Detection) -> Vec<WatchSpec> {
        detection.roots.iter().map(|p| WatchSpec { path: p.clone(), recursive: true, suffixes: &[] }).collect()
    }

    /// `getHealth()` — default mapping from detection and the last run. Override to add notes.
    fn get_health(&self, detection: &Detection, last: Option<&RunOutcome>, events_total: u64, last_event_ms: Option<i64>) -> CollectorHealth {
        default_health(self.id(), detection, last, events_total, last_event_ms)
    }
}

pub type SharedCollector = Arc<dyn AgentCollector>;

/// Every adapter the app ships. Adding an agent = one module + one line here (+ a row in the catalogue).
pub fn registry() -> Vec<SharedCollector> {
    vec![
        Arc::new(claude::ClaudeCollector),
        Arc::new(codex::CodexCollector),
        Arc::new(gemini::GeminiCollector),
        Arc::new(antigravity::AntigravityCollector),
        Arc::new(opencode::OpenCodeCollector),
        Arc::new(ollama::OllamaCollector::new()),
        Arc::new(aider::AiderCollector),
    ]
}

/// The honest-state mapping every collector inherits.
pub fn default_health(
    agent: AgentId,
    detection: &Detection,
    last: Option<&RunOutcome>,
    events_total: u64,
    last_event_ms: Option<i64>,
) -> CollectorHealth {
    let availability = match (detection.presence, last) {
        (Presence::NotFound, _) => Availability::NotInstalled,
        (_, Some(RunOutcome::Unavailable(r))) => Availability::Unavailable { reason: r.clone() },
        (_, Some(RunOutcome::Failed(m))) => Availability::Error { message: m.clone() },
        (_, Some(RunOutcome::Panicked(m))) => Availability::Error { message: format!("collector crashed (isolated): {m}") },
        (_, Some(RunOutcome::Ok(_))) if events_total > 0 => Availability::Ok,
        (_, Some(RunOutcome::Ok(_))) => Availability::NoDataYet,
        // Detected but not yet run: do not claim numbers we do not have.
        (_, None) => Availability::Unavailable { reason: "collector has not run yet".into() },
    };
    let mut h = CollectorHealth::new(agent, availability);
    let mut notes: Vec<String> = detection.note.iter().cloned().collect();
    if let Some(RunOutcome::Ok(s)) = last {
        h.skipped_records = s.skipped_records;
        notes.extend(s.notes.iter().cloned());
    }
    h.note = if notes.is_empty() { None } else { Some(notes.join(" · ")) };
    h.events_total = events_total;
    h.last_event_utc_ms = last_event_ms;
    h.source_paths = detection.roots.iter().map(|p| env::path_str(p)).collect();
    h
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;

    /// A sink that just records what a collector emitted (no database).
    #[derive(Default)]
    pub struct VecSink {
        pub events: Vec<UsageEvent>,
        pub cursors: Vec<CursorUpdate>,
    }

    impl BatchSink for VecSink {
        fn commit(&mut self, batch: SinkBatch) -> Result<(), CollectError> {
            self.events.extend(batch.events);
            self.cursors.extend(batch.cursors);
            Ok(())
        }
    }

    /// Writes to a real database, to test end-to-end idempotency across files and runs.
    pub struct DbSink {
        pub db: std::sync::Arc<crate::database::Database>,
        pub agent: AgentId,
    }

    impl BatchSink for DbSink {
        fn commit(&mut self, batch: SinkBatch) -> Result<(), CollectError> {
            self.db.commit(&crate::database::Batch { agent: self.agent, events: batch.events, cursors: batch.cursors })?;
            Ok(())
        }
    }

    pub struct Fixture {
        pub cancel: AtomicBool,
        pub projects: ProjectResolver,
        pub env: Env,
    }

    impl Fixture {
        pub fn new(home: &std::path::Path) -> Fixture {
            Fixture {
                cancel: AtomicBool::new(false),
                projects: ProjectResolver::new(true, home.join("__home__")),
                env: Env::with_home(home),
            }
        }

        pub fn ctx<'a>(&'a self, detection: &'a Detection, cursors: &'a HashMap<String, Cursor>) -> CollectCtx<'a> {
            CollectCtx { env: &self.env, detection, cursors, projects: &self.projects, cancel: &self.cancel }
        }
    }

    /// Feed a collector's own cursor updates back in, as the real supervisor does between runs.
    pub fn cursors_from(updates: &[CursorUpdate]) -> HashMap<String, Cursor> {
        updates
            .iter()
            .map(|u| (u.path.clone(), Cursor { size: u.size, mtime_ms: u.mtime_ms, offset: u.offset, state: u.state.clone() }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(p: Presence) -> Detection {
        Detection { presence: p, roots: vec![PathBuf::from("/x")], binary: None, note: Some("n".into()) }
    }

    #[test]
    fn not_found_is_not_installed_regardless_of_last_run() {
        let h = default_health("a", &Detection::not_found(), Some(&RunOutcome::Ok(CollectSummary::default())), 5, None);
        assert_eq!(h.availability, Availability::NotInstalled);
    }

    #[test]
    fn unavailable_and_failed_runs_map_to_their_states() {
        let d = det(Presence::Installed);
        assert!(matches!(
            default_health("a", &d, Some(&RunOutcome::Unavailable("no ledger".into())), 0, None).availability,
            Availability::Unavailable { .. }
        ));
        assert!(matches!(
            default_health("a", &d, Some(&RunOutcome::Failed("io".into())), 0, None).availability,
            Availability::Error { .. }
        ));
        let p = default_health("a", &d, Some(&RunOutcome::Panicked("boom".into())), 0, None);
        assert!(matches!(&p.availability, Availability::Error { message } if message.contains("isolated")));
    }

    #[test]
    fn a_clean_run_is_ok_with_data_and_no_data_yet_without() {
        let d = det(Presence::Installed);
        let ok = RunOutcome::Ok(CollectSummary::default());
        assert_eq!(default_health("a", &d, Some(&ok), 10, Some(5)).availability, Availability::Ok);
        assert_eq!(default_health("a", &d, Some(&ok), 0, None).availability, Availability::NoDataYet);
    }

    #[test]
    fn before_the_first_run_numbers_are_not_claimed() {
        let h = default_health("a", &det(Presence::Installed), None, 0, None);
        assert!(matches!(h.availability, Availability::Unavailable { .. }));
    }

    #[test]
    fn the_registry_covers_exactly_the_agents_the_ui_knows_about() {
        let mut registered: Vec<_> = registry().iter().map(|c| c.id()).collect();
        let mut known: Vec<_> = crate::model::catalog().iter().map(|a| a.id).collect();
        registered.sort_unstable();
        known.sort_unstable();
        assert_eq!(registered, known, "every catalogued agent needs a collector and vice versa");
        for c in registry() {
            assert_eq!(c.get_agent_info().id, c.id());
            assert!(!c.get_agent_info().name.is_empty());
        }
    }

    #[test]
    fn notes_from_detection_and_the_run_are_joined() {
        let mut s = CollectSummary::default();
        s.notes.push("skipped 3 undatable rows".into());
        let h = default_health("a", &det(Presence::DataOnly), Some(&RunOutcome::Ok(s)), 1, None);
        assert_eq!(h.note.as_deref(), Some("n · skipped 3 undatable rows"));
    }

    #[test]
    fn summary_tracks_the_latest_event_time() {
        let mut s = CollectSummary::default();
        s.saw_event(10);
        s.saw_event(5);
        s.saw_event(20);
        assert_eq!(s.last_event_ms, Some(20));
    }
}
