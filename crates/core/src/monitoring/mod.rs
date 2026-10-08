//! Background monitoring.
//!
//! One worker thread per collector, so a slow, failing or panicking collector can never stall or take
//! down another (requirement: "one failed collector must not affect the others"). A worker sleeps until
//! one of three things wakes it: a filesystem event on a verified directory (debounced), the polling
//! timer (safety net), or an explicit refresh (settings changed, unpaused). On wake it re-detects,
//! compares source files against saved cursors, and ingests only what changed.

mod watch;

use std::any::Any;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::collectors::{
    default_health, BatchSink, CollectCtx, CollectError, Detection, Env, ProjectResolver, RunOutcome, SharedCollector, SinkBatch,
};
use crate::database::{queries, Batch, Database};
use crate::model::{AgentId, Availability, CollectorHealth};
use crate::settings::SettingsStore;
use watch::Watcher;

/// How the monitor tells the UI "numbers changed". The facade (`api.rs`) implements it with a channel to subscribers;
/// tests use a counter.
pub trait Notifier: Send + Sync {
    fn usage_updated(&self);
}

pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn usage_updated(&self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wake {
    /// A watched directory changed.
    Dirty,
    /// Re-run now (settings changed, unpaused, manual scan).
    Refresh,
    Shutdown,
}

const DEBOUNCE: Duration = Duration::from_millis(500);
/// A busy agent writes constantly; never postpone a run longer than this.
const MAX_DEBOUNCE: Duration = Duration::from_secs(2);
/// While importing history, push partial results to the UI at most this often.
const NOTIFY_EVERY: Duration = Duration::from_millis(750);
/// Unchanged health is re-written at most this often (keeps "last scan" fresh without disk churn).
const HEALTH_REFRESH: Duration = Duration::from_secs(60);

struct Shared {
    db: Arc<Database>,
    settings: Arc<SettingsStore>,
    env: Env,
    notifier: Arc<dyn Notifier>,
    /// Aborts in-flight runs (pause / shutdown). Collectors check it between files.
    cancel: AtomicBool,
    shutdown: AtomicBool,
    /// agent id → "this run is the first import of its history".
    running: Mutex<HashMap<AgentId, bool>>,
}

pub struct Supervisor {
    shared: Arc<Shared>,
    senders: Vec<Sender<Wake>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Supervisor {
    pub fn start(
        db: Arc<Database>,
        settings: Arc<SettingsStore>,
        env: Env,
        collectors: Vec<SharedCollector>,
        notifier: Arc<dyn Notifier>,
    ) -> Supervisor {
        let shared = Arc::new(Shared {
            db,
            settings,
            env,
            notifier,
            cancel: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            running: Mutex::new(HashMap::new()),
        });
        shared.cancel.store(shared.settings.get().paused, Ordering::Relaxed);

        let mut senders = Vec::new();
        let mut threads = Vec::new();
        for collector in collectors {
            let (tx, rx) = mpsc::channel::<Wake>();
            senders.push(tx.clone());
            let worker = Worker::new(shared.clone(), collector.clone(), tx);
            let name = format!("collector-{}", collector.id());
            match std::thread::Builder::new().name(name).spawn(move || worker.run(rx)) {
                Ok(h) => threads.push(h),
                Err(e) => tracing::error!("could not start collector thread: {e}"),
            }
        }
        Supervisor { shared, senders, threads: Mutex::new(threads) }
    }

    /// Re-evaluate everything now: call after settings change, on unpause, or for a manual scan.
    pub fn refresh_now(&self) {
        self.shared.cancel.store(self.shared.settings.get().paused, Ordering::Relaxed);
        for tx in &self.senders {
            let _ = tx.send(Wake::Refresh);
        }
    }

    /// Agents currently importing their history for the first time (UI shows "IMPORTING").
    pub fn importing_agents(&self) -> Vec<String> {
        let running = self.shared.running.lock().unwrap_or_else(|p| p.into_inner());
        let mut v: Vec<String> = running.iter().filter(|(_, importing)| **importing).map(|(a, _)| a.to_string()).collect();
        v.sort();
        v
    }

    pub fn shutdown(&self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        self.shared.cancel.store(true, Ordering::Relaxed);
        for tx in &self.senders {
            let _ = tx.send(Wake::Shutdown);
        }
        let handles: Vec<_> = std::mem::take(&mut *self.threads.lock().unwrap_or_else(|p| p.into_inner()));
        for h in handles {
            let _ = h.join();
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if !self.shared.shutdown.load(Ordering::Relaxed) {
            self.shutdown();
        }
    }
}

/// Run collector code without letting a panic escape.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| panic_message(&*p))
}

fn panic_message(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct Worker {
    shared: Arc<Shared>,
    collector: SharedCollector,
    watcher: Watcher,
    last: Option<RunOutcome>,
    events_total: Option<u64>,
    last_event_ms: Option<i64>,
    last_success_ms: Option<i64>,
    /// Records understood but not counted, summed over every run since start (shown in Diagnostics).
    skipped_total: u64,
    projects: Option<(bool, Arc<ProjectResolver>)>,
    written: Option<(CollectorHealth, Instant)>,
}

impl Worker {
    fn new(shared: Arc<Shared>, collector: SharedCollector, tx: Sender<Wake>) -> Worker {
        Worker {
            shared,
            collector,
            watcher: Watcher::new(tx),
            last: None,
            events_total: None,
            last_event_ms: None,
            last_success_ms: None,
            skipped_total: 0,
            projects: None,
            written: None,
        }
    }

    fn run(mut self, rx: Receiver<Wake>) {
        let mut first = true;
        loop {
            if !first {
                let settings = self.shared.settings.get();
                // While paused there is nothing to poll for; sleep long.
                let poll = if settings.paused { Duration::from_secs(300) } else { Duration::from_secs(u64::from(settings.polling_interval_secs)) };
                match rx.recv_timeout(poll) {
                    Ok(Wake::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Wake::Dirty) => {
                        if Self::debounce(&rx) {
                            break;
                        }
                    }
                    Ok(Wake::Refresh) | Err(RecvTimeoutError::Timeout) => {}
                }
            }
            first = false;
            if self.shared.shutdown.load(Ordering::Relaxed) {
                break;
            }
            self.cycle();
        }
    }

    /// Coalesce a burst of file events into one run. Returns `true` if shutdown was requested.
    fn debounce(rx: &Receiver<Wake>) -> bool {
        let started = Instant::now();
        loop {
            let left = MAX_DEBOUNCE.saturating_sub(started.elapsed());
            if left.is_zero() {
                return false;
            }
            match rx.recv_timeout(DEBOUNCE.min(left)) {
                Ok(Wake::Dirty) => continue,
                Ok(Wake::Refresh) | Err(RecvTimeoutError::Timeout) => return false,
                Ok(Wake::Shutdown) | Err(RecvTimeoutError::Disconnected) => return true,
            }
        }
    }

    fn id(&self) -> AgentId {
        self.collector.id()
    }

    fn resolver(&mut self, enabled: bool) -> Arc<ProjectResolver> {
        match &self.projects {
            Some((e, r)) if *e == enabled => r.clone(),
            _ => {
                let r = Arc::new(ProjectResolver::new(enabled, self.shared.env.home.clone()));
                self.projects = Some((enabled, r.clone()));
                r
            }
        }
    }

    fn cycle(&mut self) {
        let id = self.id();
        let settings = self.shared.settings.get();

        let detection = match guarded(|| self.collector.detect(&self.shared.env)) {
            Ok(d) => d,
            Err(msg) => {
                self.publish_error(format!("detect() crashed (isolated): {msg}"));
                return;
            }
        };
        let specs = guarded(|| self.collector.watch_specs(&detection)).unwrap_or_default();
        self.watcher.sync(specs);

        if !detection.is_present() {
            self.last = None;
            self.publish(&detection, false);
            return;
        }
        if settings.paused || !settings.agent_enabled(id) {
            return;
        }

        let cursors = self.shared.db.with_reader(|c| queries::load_cursors(c, id)).unwrap_or_default();
        let importing = cursors.is_empty();
        self.shared.running.lock().unwrap_or_else(|p| p.into_inner()).insert(id, importing);

        let projects = self.resolver(settings.project_detection);
        let mut sink = DbSink::new(&self.shared.db, id, self.shared.notifier.as_ref());
        let ctx = CollectCtx {
            env: &self.shared.env,
            detection: &detection,
            cursors: &cursors,
            projects: &projects,
            cancel: &self.shared.cancel,
        };

        let result = guarded(|| self.collector.collect_usage(&ctx, &mut sink));
        self.shared.running.lock().unwrap_or_else(|p| p.into_inner()).remove(id);

        let outcome = match result {
            Ok(Ok(summary)) => {
                self.last_success_ms = Some(now_ms());
                self.skipped_total += summary.skipped_records;
                RunOutcome::Ok(summary)
            }
            Ok(Err(CollectError::Cancelled)) => return,
            Ok(Err(CollectError::Unavailable(reason))) => RunOutcome::Unavailable(reason),
            Ok(Err(e)) => RunOutcome::Failed(e.to_string()),
            Err(msg) => {
                tracing::error!(agent = id, "collector panicked (isolated): {msg}");
                RunOutcome::Panicked(msg)
            }
        };
        let committed = sink.changed();
        let finished_with_events = sink.committed_any();
        self.last = Some(outcome);

        let first_stats = self.events_total.is_none();
        let ingested = committed || finished_with_events;
        self.publish(&detection, ingested || first_stats);
    }

    /// Recompute health (and, if events may have changed, the stored totals), persist it, notify the UI.
    fn publish(&mut self, detection: &Detection, refresh_stats: bool) {
        let id = self.id();
        if refresh_stats || self.events_total.is_none() {
            if let Ok((n, last)) = self.shared.db.with_reader(|c| queries::agent_event_stats(c, id)) {
                self.events_total = Some(n);
                self.last_event_ms = last;
            }
        }
        let total = self.events_total.unwrap_or(0);
        let mut health = match guarded(|| self.collector.get_health(detection, self.last.as_ref(), total, self.last_event_ms)) {
            Ok(h) => h,
            Err(_) => default_health(id, detection, self.last.as_ref(), total, self.last_event_ms),
        };
        health.last_run_utc_ms = Some(now_ms());
        health.last_success_utc_ms = self.last_success_ms;
        health.skipped_records = self.skipped_total;
        self.persist(health, refresh_stats);
    }

    fn publish_error(&mut self, message: String) {
        let mut h = CollectorHealth::new(self.id(), Availability::Error { message });
        h.last_run_utc_ms = Some(now_ms());
        self.persist(h, false);
    }

    fn persist(&mut self, health: CollectorHealth, force_notify: bool) {
        let materially_changed = match &self.written {
            None => true,
            Some((prev, at)) => {
                prev.availability != health.availability
                    || prev.note != health.note
                    || prev.events_total != health.events_total
                    || prev.skipped_records != health.skipped_records
                    || prev.source_paths != health.source_paths
                    || at.elapsed() >= HEALTH_REFRESH
            }
        };
        if !materially_changed {
            return;
        }
        let state_changed = self.written.as_ref().map_or(true, |(p, _)| p.availability != health.availability);
        if let Err(e) = self.shared.db.with_writer(|w| queries::save_health(&w.conn, &health)) {
            tracing::warn!(agent = health.agent, "could not save collector health: {e}");
            return;
        }
        self.written = Some((health, Instant::now()));
        if state_changed || force_notify {
            self.shared.notifier.usage_updated();
        }
    }
}

/// Writes collector batches to the database and tells the UI as history streams in.
struct DbSink<'a> {
    db: &'a Database,
    agent: AgentId,
    notifier: &'a dyn Notifier,
    inserted: u64,
    raised: u64,
    batches: u64,
    last_notify: Instant,
}

impl<'a> DbSink<'a> {
    fn new(db: &'a Database, agent: AgentId, notifier: &'a dyn Notifier) -> Self {
        DbSink { db, agent, notifier, inserted: 0, raised: 0, batches: 0, last_notify: Instant::now() }
    }

    /// Did this run change any stored counts?
    fn changed(&self) -> bool {
        self.inserted + self.raised > 0
    }

    fn committed_any(&self) -> bool {
        self.batches > 0
    }
}

impl BatchSink for DbSink<'_> {
    fn commit(&mut self, batch: SinkBatch) -> Result<(), CollectError> {
        let stats = self.db.commit(&Batch { agent: self.agent, events: batch.events, cursors: batch.cursors })?;
        self.inserted += stats.inserted;
        self.raised += stats.raised;
        self.batches += 1;
        if stats.inserted + stats.raised > 0 && self.last_notify.elapsed() >= NOTIFY_EVERY {
            self.notifier.usage_updated();
            self.last_notify = Instant::now();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
