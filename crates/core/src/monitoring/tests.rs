use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::collectors::{AgentCollector, AgentInfo, CollectSummary, Presence, WatchSpec};
use crate::database::testutil::temp_db;
use crate::database::CursorUpdate;
use crate::model::{Accuracy, UsageEvent};

#[derive(Clone)]
enum Mode {
    Emit(Vec<UsageEvent>),
    Unavailable(&'static str),
    Fail,
    Panic,
    PanicInDetect,
    SlowThenEmit(u64, Vec<UsageEvent>),
}

struct Fake {
    id: &'static str,
    presence: Presence,
    root: Option<PathBuf>,
    mode: Mode,
    runs: AtomicUsize,
}

impl Fake {
    fn new(id: &'static str, mode: Mode) -> Arc<Fake> {
        Arc::new(Fake { id, presence: Presence::Installed, root: None, mode, runs: AtomicUsize::new(0) })
    }
    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

impl AgentCollector for Fake {
    fn id(&self) -> AgentId {
        self.id
    }
    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo { id: self.id, name: "Fake", data_sources: &[], caveats: &[] }
    }
    fn detect(&self, _env: &Env) -> Detection {
        if matches!(self.mode, Mode::PanicInDetect) {
            panic!("detect exploded");
        }
        Detection { presence: self.presence, roots: self.root.iter().cloned().collect(), binary: None, note: None }
    }
    fn watch_specs(&self, d: &Detection) -> Vec<WatchSpec> {
        d.roots.iter().map(|p| WatchSpec { path: p.clone(), recursive: true, suffixes: &[".jsonl"] }).collect()
    }
    fn collect_usage(&self, ctx: &CollectCtx<'_>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let emit = |sink: &mut dyn BatchSink, events: &Vec<UsageEvent>| {
            sink.commit(SinkBatch {
                events: events.clone(),
                cursors: vec![CursorUpdate { path: "fake-source".into(), size: 1, mtime_ms: 1, offset: 1, state: None }],
            })
        };
        match &self.mode {
            Mode::Emit(events) => {
                emit(sink, events)?;
                let mut s = CollectSummary::default();
                for e in events {
                    s.saw_event(e.ts_utc_ms);
                }
                Ok(s)
            }
            Mode::SlowThenEmit(ms, events) => {
                let deadline = Instant::now() + Duration::from_millis(*ms);
                while Instant::now() < deadline {
                    ctx.check_cancel()?;
                    std::thread::sleep(Duration::from_millis(10));
                }
                emit(sink, events)?;
                Ok(CollectSummary::default())
            }
            Mode::Unavailable(r) => Err(CollectError::Unavailable((*r).into())),
            Mode::Fail => Err(CollectError::Io(std::io::Error::other("disk on fire"))),
            Mode::Panic => panic!("collector exploded"),
            Mode::PanicInDetect => unreachable!(),
        }
    }
}

fn event(agent: &'static str, key: &str, ts: i64) -> UsageEvent {
    UsageEvent {
        agent,
        model: "m".into(),
        ts_utc_ms: ts,
        input_tokens: 100,
        output_tokens: 10,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: None,
        session_id: None,
        project: None,
        source: "test",
        accuracy: Accuracy::Real,
        dedupe_key: key.into(),
    }
}

#[derive(Default)]
struct CountingNotifier(AtomicUsize);
impl Notifier for CountingNotifier {
    fn usage_updated(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    settings: Arc<SettingsStore>,
    notifier: Arc<CountingNotifier>,
    sup: Supervisor,
}

fn rig(collectors: Vec<SharedCollector>) -> Rig {
    let (dir, db) = temp_db();
    let settings = Arc::new(SettingsStore::load(db.clone()).unwrap());
    let notifier = Arc::new(CountingNotifier::default());
    let sup = Supervisor::start(db.clone(), settings.clone(), Env::with_home(dir.path()), collectors, notifier.clone());
    Rig { _dir: dir, db, settings, notifier, sup }
}

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for: {what}");
}

impl Rig {
    fn health(&self, agent: &str) -> Option<CollectorHealth> {
        self.db.with_reader(queries::load_health).unwrap().into_iter().find(|h| h.agent == agent)
    }
    fn events(&self, agent: &str) -> u64 {
        self.db.with_reader(|c| queries::agent_event_stats(c, agent)).unwrap().0
    }
}

#[test]
fn a_healthy_collector_ingests_and_reports_ok() {
    let c = Fake::new("codex", Mode::Emit(vec![event("codex", "a", 1000), event("codex", "b", 2000)]));
    let r = rig(vec![c]);
    wait_until("events stored", || r.events("codex") == 2);
    wait_until("health ok", || r.health("codex").is_some_and(|h| h.availability == Availability::Ok));
    assert_eq!(r.health("codex").unwrap().events_total, 2);
    assert!(r.notifier.0.load(Ordering::SeqCst) > 0, "the UI must be told");
    r.sup.shutdown();
}

#[test]
fn a_panicking_collector_is_isolated_and_the_others_keep_working() {
    let bad = Fake::new("claude", Mode::Panic);
    let good = Fake::new("codex", Mode::Emit(vec![event("codex", "a", 1000)]));
    let r = rig(vec![bad.clone(), good]);
    wait_until("good collector ingested", || r.events("codex") == 1);
    wait_until("bad collector reported", || r.health("claude").is_some_and(|h| matches!(h.availability, Availability::Error { .. })));
    let msg = match r.health("claude").unwrap().availability {
        Availability::Error { message } => message,
        _ => unreachable!(),
    };
    assert!(msg.contains("exploded") && msg.contains("isolated"), "{msg}");
    // It can be retried and keeps failing without taking anything down.
    r.sup.refresh_now();
    wait_until("retried", || bad.runs() >= 2);
    assert_eq!(r.health("codex").unwrap().availability, Availability::Ok);
    r.sup.shutdown();
}

#[test]
fn a_panic_inside_detect_is_isolated_too() {
    let bad = Fake::new("gemini", Mode::PanicInDetect);
    let good = Fake::new("codex", Mode::Emit(vec![event("codex", "a", 1000)]));
    let r = rig(vec![bad, good]);
    wait_until("good ingested", || r.events("codex") == 1);
    wait_until("detect panic reported", || r.health("gemini").is_some_and(|h| matches!(h.availability, Availability::Error { .. })));
    r.sup.shutdown();
}

#[test]
fn an_io_failure_is_an_error_state_not_zero_tokens() {
    let r = rig(vec![Fake::new("claude", Mode::Fail)]);
    wait_until("error reported", || r.health("claude").is_some_and(|h| matches!(h.availability, Availability::Error { .. })));
    assert_eq!(r.events("claude"), 0);
    r.sup.shutdown();
}

#[test]
fn an_unavailable_collector_reports_its_reason_and_emits_nothing() {
    let r = rig(vec![Fake::new("ollama", Mode::Unavailable("Ollama keeps no usage history"))]);
    wait_until("unavailable reported", || r.health("ollama").is_some());
    match r.health("ollama").unwrap().availability {
        Availability::Unavailable { reason } => assert!(reason.contains("no usage history")),
        other => panic!("expected unavailable, got {other:?}"),
    }
    assert_eq!(r.events("ollama"), 0);
    r.sup.shutdown();
}

#[test]
fn a_missing_agent_is_never_run_and_reported_not_installed() {
    let c = Arc::new(Fake { id: "aider", presence: Presence::NotFound, root: None, mode: Mode::Emit(vec![event("aider", "a", 1)]), runs: AtomicUsize::new(0) });
    let r = rig(vec![c.clone()]);
    wait_until("not installed reported", || r.health("aider").is_some_and(|h| h.availability == Availability::NotInstalled));
    assert_eq!(c.runs(), 0, "collect_usage must not run for an absent agent");
    r.sup.shutdown();
}

#[test]
fn rerunning_the_same_events_is_idempotent() {
    let c = Fake::new("codex", Mode::Emit(vec![event("codex", "a", 1000)]));
    let r = rig(vec![c.clone()]);
    wait_until("first run", || c.runs() >= 1 && r.events("codex") == 1);
    for _ in 0..3 {
        let before = c.runs();
        r.sup.refresh_now();
        wait_until("another run", || c.runs() > before);
    }
    assert_eq!(r.events("codex"), 1);
    let totals = r.db.with_reader(|c| queries::totals_by_agent(c, 0, 10_000_000)).unwrap();
    assert_eq!(totals["codex"].input, 100, "rollup must not double count");
    r.sup.shutdown();
}

#[test]
fn pausing_stops_collection_and_resuming_restarts_it() {
    let c = Fake::new("codex", Mode::Emit(vec![event("codex", "a", 1000)]));
    let r = rig(vec![c.clone()]);
    wait_until("first run", || c.runs() >= 1);

    r.settings.update(&serde_json::json!({"paused": true})).unwrap();
    r.sup.refresh_now();
    std::thread::sleep(Duration::from_millis(300));
    let paused_at = c.runs();
    r.sup.refresh_now();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(c.runs(), paused_at, "no runs while paused");

    r.settings.update(&serde_json::json!({"paused": false})).unwrap();
    r.sup.refresh_now();
    wait_until("resumed", || c.runs() > paused_at);
    r.sup.shutdown();
}

#[test]
fn a_disabled_agent_is_not_collected() {
    let c = Fake::new("claude", Mode::Emit(vec![event("claude", "a", 1000)]));
    let (dir, db) = temp_db();
    let settings = Arc::new(SettingsStore::load(db.clone()).unwrap());
    settings.update(&serde_json::json!({"enabledAgents": ["codex"]})).unwrap();
    let sup = Supervisor::start(db.clone(), settings, Env::with_home(dir.path()), vec![c.clone()], Arc::new(NoopNotifier));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(c.runs(), 0);
    sup.shutdown();
}

#[test]
fn a_file_event_in_a_watched_directory_triggers_a_run_without_polling() {
    let dir = tempfile::tempdir().unwrap();
    let watched = dir.path().join("sessions");
    std::fs::create_dir_all(&watched).unwrap();
    let c = Arc::new(Fake {
        id: "codex",
        presence: Presence::Installed,
        root: Some(watched.clone()),
        mode: Mode::Emit(vec![event("codex", "a", 1000)]),
        runs: AtomicUsize::new(0),
    });
    let r = rig(vec![c.clone()]);
    r.settings.update(&serde_json::json!({"pollingIntervalSecs": 3600})).unwrap();
    wait_until("initial run", || c.runs() >= 1);
    std::thread::sleep(Duration::from_millis(400)); // let the watcher arm
    let before = c.runs();

    // An irrelevant file must not wake us...
    std::fs::write(watched.join("ignored.txt"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(c.runs(), before, "unrelated files must not trigger runs");

    // ...a relevant one must, well before the 3600 s poll.
    std::fs::write(watched.join("rollout.jsonl"), "{}\n").unwrap();
    wait_until("watcher-triggered run", || c.runs() > before);
    r.sup.shutdown();
}

#[test]
fn first_import_is_reported_while_running_and_cleared_afterwards() {
    let c = Fake::new("codex", Mode::SlowThenEmit(700, vec![event("codex", "a", 1000)]));
    let r = rig(vec![c]);
    wait_until("importing flag", || r.sup.importing_agents() == vec!["codex".to_string()]);
    wait_until("import finished", || r.events("codex") == 1 && r.sup.importing_agents().is_empty());
    r.sup.shutdown();
}

#[test]
fn pausing_cancels_a_long_import_promptly() {
    let c = Fake::new("codex", Mode::SlowThenEmit(30_000, vec![event("codex", "a", 1000)]));
    let r = rig(vec![c.clone()]);
    wait_until("running", || c.runs() >= 1);
    let t = Instant::now();
    r.settings.update(&serde_json::json!({"paused": true})).unwrap();
    r.sup.refresh_now();
    wait_until("import cancelled", || r.sup.importing_agents().is_empty());
    assert!(t.elapsed() < Duration::from_secs(3), "cancel took {:?}", t.elapsed());
    assert_eq!(r.events("codex"), 0, "a cancelled run must not store partial garbage");
    r.sup.shutdown();
}

#[test]
fn shutdown_joins_every_worker() {
    let r = rig(vec![Fake::new("codex", Mode::Emit(vec![])), Fake::new("claude", Mode::Emit(vec![]))]);
    let t = Instant::now();
    r.sup.shutdown();
    assert!(t.elapsed() < Duration::from_secs(3));
}
