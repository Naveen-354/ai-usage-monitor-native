//! `Backend` implemented over the real `core` (collectors, SQLite, aggregation). No GUI types here: the UI repaint is
//! injected as a plain callback, so this module is tested without a window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use ai_usage_monitor_core::accounts::LoginMethod;
use ai_usage_monitor_core::aggregation::Period;
use ai_usage_monitor_core::api::{Config, Core, CoreEvent};
use app_api::api::{Backend, BackendError, BackendEvent, CollectorHealth, SettingsPatch};
use app_api::view::{AccountsOverview, AppInfo, DayTotal, History, OverlayDiagnostics, Overview, PeriodOrCustom, Settings};

use crate::accounts_view;
use crate::bridge;

type Listeners = Arc<Mutex<Vec<Sender<BackendEvent>>>>;

fn err(e: impl std::fmt::Display) -> BackendError {
    BackendError::Message(e.to_string())
}

/// Tells every listener (the UI) something happened.
fn emit(listeners: &Listeners, ev: BackendEvent) {
    if let Ok(mut l) = listeners.lock() {
        l.retain(|tx| tx.send(ev.clone()).is_ok());
    }
}

/// What the app needs from a backend beyond the shared [`Backend`] trait, so it can run on the real one or the demo.
pub trait AppBackend: Backend {
    /// Merges a camelCase settings document (only the keys to change) and returns the stored result.
    fn update_settings_json(&self, patch: &serde_json::Value) -> Result<Settings, BackendError>;
    /// Updated by the UI thread so Diagnostics can show what the OS reports about the overlay.
    fn set_overlay_diagnostics(&self, d: OverlayDiagnostics);
    fn quit_requested(&self) -> bool;
    /// Stops background work and checkpoints the database. Later calls return errors instead of panicking.
    fn shutdown(&self);
}

impl AppBackend for CoreBackend {
    fn update_settings_json(&self, patch: &serde_json::Value) -> Result<Settings, BackendError> {
        CoreBackend::update_settings_json(self, patch)
    }
    fn set_overlay_diagnostics(&self, d: OverlayDiagnostics) {
        CoreBackend::set_overlay_diagnostics(self, d)
    }
    fn quit_requested(&self) -> bool {
        CoreBackend::quit_requested(self)
    }
    fn shutdown(&self) {
        CoreBackend::shutdown(self)
    }
}

pub struct CoreBackend {
    core: Mutex<Option<Core>>,
    listeners: Listeners,
    /// Wakes the UI from a background thread.
    repaint: Arc<dyn Fn() + Send + Sync>,
    overlay: Mutex<OverlayDiagnostics>,
    quit: AtomicBool,
    data_dir: PathBuf,
    db_notice: Option<String>,
}

impl CoreBackend {
    /// Starts the backend. `repaint` is called (from a background thread) whenever new data arrives.
    pub fn start(config: Config, repaint: Arc<dyn Fn() + Send + Sync>) -> Result<Arc<Self>, BackendError> {
        let data_dir = config.data_dir.clone();
        let core = Core::start(config).map_err(err)?;
        let db_notice = core.database_notice().map(str::to_owned);
        let events = core.subscribe();
        let listeners: Listeners = Arc::default();
        let repaint_for_threads = repaint.clone();

        let pump = listeners.clone();
        std::thread::Builder::new()
            .name("core-events".into())
            .spawn(move || {
                // Ends when the core is shut down (its senders are dropped).
                while let Ok(ev) = events.recv() {
                    let ev = match ev {
                        CoreEvent::OverviewChanged => BackendEvent::OverviewChanged,
                        CoreEvent::HealthChanged => BackendEvent::HealthChanged,
                    };
                    if let Ok(mut l) = pump.lock() {
                        l.retain(|tx| tx.send(ev.clone()).is_ok());
                    }
                    repaint();
                }
            })
            .map_err(err)?;

        Ok(Arc::new(Self {
            core: Mutex::new(Some(core)),
            listeners,
            repaint: repaint_for_threads,
            overlay: Mutex::new(blank_overlay()),
            quit: AtomicBool::new(false),
            data_dir,
            db_notice,
        }))
    }

    fn with_core<T>(&self, f: impl FnOnce(&Core) -> Result<T, BackendError>) -> Result<T, BackendError> {
        let guard = self.core.lock().map_err(|_| err("backend lock poisoned"))?;
        match guard.as_ref() {
            Some(core) => f(core),
            None => Err(err("backend already shut down")),
        }
    }

    /// Merges a camelCase settings document (only the keys to change) and returns the stored result.
    pub fn update_settings_json(&self, patch: &serde_json::Value) -> Result<Settings, BackendError> {
        self.with_core(|c| {
            let updated = c.update_settings(patch).map_err(err)?;
            bridge::convert(&updated).map_err(BackendError::Message)
        })
    }

    /// Updated by the UI thread so Diagnostics can show what the OS reports about the overlay.
    pub fn set_overlay_diagnostics(&self, d: OverlayDiagnostics) {
        if let Ok(mut o) = self.overlay.lock() {
            *o = d;
        }
    }

    pub fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    /// Stops the collectors and checkpoints the database. Later calls return errors instead of panicking.
    pub fn shutdown(&self) {
        let core = self.core.lock().ok().and_then(|mut g| g.take());
        if let Some(core) = core {
            core.shutdown();
        }
    }
}

fn blank_overlay() -> OverlayDiagnostics {
    OverlayDiagnostics {
        visible: false,
        minimized: false,
        always_on_top_setting: true,
        decorated: None,
        os_reports_topmost: None,
        x: None,
        y: None,
        width: None,
        height: None,
        scale_factor: None,
        monitors: 0,
        platform_note: platform_note().into(),
    }
}

pub fn platform_note() -> &'static str {
    if cfg!(windows) {
        "Windows: stays above normal and borderless-fullscreen windows; exclusive-fullscreen games can still cover it."
    } else if cfg!(target_os = "macos") {
        "macOS: floats across Spaces; above full-screen apps depends on the app's own window level."
    } else {
        "Linux: depends on the window manager/compositor; Wayland compositors may ignore always-on-top."
    }
}

fn open_path(p: &Path) -> Result<(), BackendError> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(p).spawn().map(|_| ()).map_err(err)
}

/// Re-checks every account in a background thread (each agent's own status command; one that cannot be reached does not stop
/// the rest), then tells the UI to re-read the accounts.
fn spawn_account_check(
    manager: Arc<ai_usage_monitor_core::accounts::AccountManager>,
    listeners: Listeners,
    repaint: Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("account-check".into()).spawn(move || {
        let _ = manager.check_all();
        emit(&listeners, BackendEvent::AccountsChanged);
        emit(&listeners, BackendEvent::AccountsChecked);
        repaint();
    })
}

/// The one-line message shown when a sign-in finishes.
fn crate_state_notice(a: &ai_usage_monitor_core::accounts::Account) -> String {
    use ai_usage_monitor_core::accounts::AuthState;
    match a.auth {
        AuthState::Valid => format!("{} is signed in{}.", a.display_ref(), a.identity.as_ref().map(|i| format!(" as {i}")).unwrap_or_default()),
        other => format!("{} finished signing in ({}).", a.display_ref(), other.label().to_lowercase()),
    }
}

fn not_yet(what: &str) -> BackendError {
    err(format!("{what} is not implemented in the native app yet"))
}

impl Backend for CoreBackend {
    fn overview(&self, period: PeriodOrCustom, from: Option<String>, to: Option<String>) -> Result<Overview, BackendError> {
        let p = match period {
            PeriodOrCustom::Day => Period::Day,
            PeriodOrCustom::Week => Period::Week,
            PeriodOrCustom::Month => Period::Month,
            PeriodOrCustom::Year => Period::Year,
            PeriodOrCustom::Custom => Period::Custom,
        };
        let o = self.with_core(|c| c.overview(p, from.as_deref(), to.as_deref()).map_err(err))?;
        bridge::convert(&o).map_err(BackendError::Message)
    }

    fn accounts(&self) -> Result<AccountsOverview, BackendError> {
        self.with_core(|c| {
            let agents = c.accounts().agents().map_err(err)?;
            let usage = c.account_usage().map_err(err)?;
            Ok(accounts_view::overview(&agents, &usage))
        })
    }

    fn use_account(&self, agent: &str, account: &str) -> Result<Option<String>, BackendError> {
        let outcome = self.with_core(|c| c.accounts().use_account(agent, account).map_err(err))?;
        emit(&self.listeners, BackendEvent::AccountsChanged);
        Ok(outcome.warning)
    }

    fn remove_account(&self, agent: &str, account: &str, purge_usage: bool) -> Result<(), BackendError> {
        self.with_core(|c| c.accounts().remove(agent, account, purge_usage).map_err(err))?;
        emit(&self.listeners, BackendEvent::AccountsChanged);
        Ok(())
    }

    fn add_account(&self, agent: &str, label: &str, device_code: bool) -> Result<(), BackendError> {
        let manager = self.with_core(|c| Ok(c.accounts().clone()))?;
        let method = if device_code { LoginMethod::DeviceCode } else { LoginMethod::Standard };
        // Preparing is quick and reports "not installed" / "that name is taken" straight back to the caller.
        let plan = manager.plan_login(agent, label, method, None).map_err(err)?;
        let (listeners, repaint) = (self.listeners.clone(), self.repaint.clone());
        emit(&listeners, BackendEvent::AccountsChanged); // the pending account shows up at once
        std::thread::Builder::new()
            .name("account-sign-in".into())
            .spawn(move || {
                let name = plan.account.display_ref();
                // The agent's own sign-in runs in a console window of its own; this thread waits for it.
                let notice = match manager.run_login_in_new_window(plan) {
                    Ok(a) => crate_state_notice(&a),
                    Err(e) => format!("Signing in {name} did not complete: {e}"),
                };
                emit(&listeners, BackendEvent::AccountNotice(notice));
                emit(&listeners, BackendEvent::AccountsChanged);
                repaint();
            })
            .map_err(err)?;
        Ok(())
    }

    fn reauthenticate_account(&self, agent: &str, account: &str) -> Result<(), BackendError> {
        let manager = self.with_core(|c| Ok(c.accounts().clone()))?;
        let plan = manager.plan_relogin(agent, account, LoginMethod::Standard, None).map_err(err)?;
        let (listeners, repaint) = (self.listeners.clone(), self.repaint.clone());
        std::thread::Builder::new()
            .name("account-sign-in".into())
            .spawn(move || {
                let name = plan.account.display_ref();
                let notice = match manager.run_login_in_new_window(plan) {
                    Ok(a) => crate_state_notice(&a),
                    Err(e) => format!("Signing in {name} again did not complete: {e}"),
                };
                emit(&listeners, BackendEvent::AccountNotice(notice));
                emit(&listeners, BackendEvent::AccountsChanged);
                repaint();
            })
            .map_err(err)?;
        Ok(())
    }

    fn check_accounts(&self) -> Result<(), BackendError> {
        let manager = self.with_core(|c| Ok(c.accounts().clone()))?;
        spawn_account_check(manager, self.listeners.clone(), self.repaint.clone()).map(|_| ()).map_err(err)
    }

    fn history(&self, _period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<History, BackendError> {
        Err(not_yet("the history timeline"))
    }

    fn daily_totals(&self, days: u32) -> Result<Vec<DayTotal>, BackendError> {
        self.with_core(|c| {
            let days = c.daily_totals(days).map_err(err)?;
            bridge::convert(&days).map_err(BackendError::Message)
        })
    }

    fn settings(&self) -> Result<Settings, BackendError> {
        self.with_core(|c| bridge::convert(&c.settings()).map_err(BackendError::Message))
    }

    fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, BackendError> {
        self.update_settings_json(&bridge::patch_to_json(&patch))
    }

    fn app_info(&self) -> Result<AppInfo, BackendError> {
        self.with_core(|c| {
            Ok(AppInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                data_dir: self.data_dir.display().to_string(),
                database_path: c.database_path().display().to_string(),
                log_dir: self.data_dir.join("logs").display().to_string(),
                database_notice: self.db_notice.clone(),
            })
        })
    }

    fn overlay_diagnostics(&self) -> Result<OverlayDiagnostics, BackendError> {
        self.overlay.lock().map(|o| o.clone()).map_err(|_| err("overlay diagnostics lock poisoned"))
    }

    fn collector_health(&self) -> Result<Vec<CollectorHealth>, BackendError> {
        self.with_core(|c| {
            let health = c.collector_health().map_err(err)?;
            health
                .iter()
                .map(|h| {
                    Ok(CollectorHealth {
                        agent: h.agent.clone(),
                        availability: bridge::convert(&h.availability).map_err(BackendError::Message)?,
                        note: h.note.clone(),
                        last_run_utc_ms: h.last_run_utc_ms,
                        last_success_utc_ms: h.last_success_utc_ms,
                        last_event_utc_ms: h.last_event_utc_ms,
                        events_total: h.events_total,
                        skipped_records: h.skipped_records,
                        source_paths: h.source_paths.clone(),
                    })
                })
                .collect()
        })
    }

    fn pause(&self) -> Result<(), BackendError> {
        self.update_settings_json(&serde_json::json!({"paused": true})).map(|_| ())
    }

    fn resume(&self) -> Result<(), BackendError> {
        self.update_settings_json(&serde_json::json!({"paused": false})).map(|_| ())
    }

    fn rescan_agent(&self, _agent_id: &str) -> Result<(), BackendError> {
        // The supervisor rescans every collector; per-agent rescans are not a separate operation in the backend.
        self.with_core(|c| {
            c.rescan();
            Ok(())
        })
    }

    fn export_json(&self, _path: &str) -> Result<(), BackendError> {
        Err(not_yet("JSON export"))
    }

    fn export_csv(&self, _path: &str) -> Result<(), BackendError> {
        Err(not_yet("CSV export"))
    }

    fn clear_history(&self) -> Result<(), BackendError> {
        Err(not_yet("clearing history"))
    }

    fn open_data_folder(&self) -> Result<(), BackendError> {
        open_path(&self.data_dir)
    }

    fn open_log_folder(&self) -> Result<(), BackendError> {
        let dir = self.data_dir.join("logs");
        std::fs::create_dir_all(&dir).map_err(err)?;
        open_path(&dir)
    }

    fn quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
    }

    fn subscribe(&self) -> Receiver<BackendEvent> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut l) = self.listeners.lock() {
            l.push(tx);
        }
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_api::view::{PeriodKey, Theme};

    fn backend(dir: &tempfile::TempDir) -> Arc<CoreBackend> {
        let mut cfg = Config::in_dir(dir.path());
        cfg.monitoring_on = false; // tests must not scan the developer's real agent folders
        CoreBackend::start(cfg, Arc::new(|| {})).unwrap()
    }

    #[test]
    fn it_is_usable_as_a_backend_trait_object_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CoreBackend>();
        let dir = tempfile::tempdir().unwrap();
        let b: Arc<dyn Backend> = backend(&dir);
        let o = std::thread::spawn(move || b.overview(PeriodOrCustom::Day, None, None)).join().unwrap();
        assert!(o.is_ok());
    }

    #[test]
    fn overview_settings_and_app_info_have_the_ui_shape() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let o = b.overview(PeriodOrCustom::Week, None, None).unwrap();
        assert_eq!(o.period, PeriodOrCustom::Week);
        assert!(!o.agents.is_empty(), "the catalogue of agents is always listed");
        assert_eq!(b.settings().unwrap().period, PeriodKey::Day);
        let info = b.app_info().unwrap();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.database_path.ends_with("usage.db"));
        assert!(info.database_notice.is_none());
    }

    #[test]
    fn a_settings_patch_is_applied_clamped_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let s = b.update_settings(SettingsPatch { token_text_size: Some(50.0), theme: Some(Theme::Light), ..Default::default() }).unwrap();
        assert!(s.token_text_size <= 2.0);
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(b.settings().unwrap().theme, Theme::Light);
    }

    #[test]
    fn pause_and_resume_flip_the_paused_setting() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        b.pause().unwrap();
        assert!(b.settings().unwrap().paused);
        b.resume().unwrap();
        assert!(!b.settings().unwrap().paused);
    }

    #[test]
    fn features_that_do_not_exist_yet_say_so_instead_of_pretending() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        for r in [
            b.history(PeriodOrCustom::Day, None, None).map(|_| ()),
            b.export_json("x.json"),
            b.export_csv("x.csv"),
            b.clear_history(),
        ] {
            assert!(r.unwrap_err().to_string().contains("not implemented"));
        }
    }

    #[test]
    fn a_damaged_database_is_rebuilt_and_the_user_is_told_in_app_info() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("usage.db"), vec![0xAB_u8; 8192]).unwrap();
        let b = backend(&dir);
        let notice = b.app_info().unwrap().database_notice.expect("a rebuilt database must be reported");
        assert!(!notice.is_empty());
        assert!(b.overview(PeriodOrCustom::Day, None, None).is_ok(), "the app keeps working on the rebuilt database");
    }

    #[test]
    fn a_bad_custom_range_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        assert!(b.overview(PeriodOrCustom::Custom, None, None).is_err());
        assert!(b.overview(PeriodOrCustom::Custom, Some("2026-10-02".into()), Some("2026-10-01".into())).is_err());
    }

    #[test]
    fn a_fresh_backend_has_no_active_days_and_the_list_converts_to_the_view_type() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        assert!(b.daily_totals(371).unwrap().is_empty());
        let core_days = vec![ai_usage_monitor_core::aggregation::DayTotal { date: chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(), total: 42 }];
        let view: Vec<app_api::view::DayTotal> = bridge::convert(&core_days).unwrap();
        assert_eq!(view, vec![app_api::view::DayTotal { date: "2026-10-08".into(), total: 42 }]);
        b.shutdown();
    }

    #[test]
    fn a_fresh_backend_lists_every_agent_with_its_default_account_active_and_nothing_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let o = b.accounts().unwrap();
        assert_eq!(o.agents.len(), ai_usage_monitor_core::model::catalog().len());
        for a in &o.agents {
            assert_eq!(a.active.as_deref(), Some("default"), "{}", a.agent_id);
            assert_eq!(a.accounts.len(), 1);
            assert!(a.accounts[0].active && a.accounts[0].is_default && a.accounts[0].usage.is_none(), "{}: no usage is not zero usage", a.agent_id);
        }
        assert!(o.total.is_none());
        let antigravity = o.agents.iter().find(|a| a.agent_id == "antigravity").unwrap();
        assert!(!antigravity.switching.supported && antigravity.switching.reason.as_deref().unwrap_or("").contains("credential store"));
        let codex = o.agents.iter().find(|a| a.agent_id == "codex").unwrap();
        assert_eq!(codex.switching.mechanism.as_deref(), Some("CODEX_HOME"));
        assert!(!codex.login_methods.iter().any(|m| m == "api-key"), "keys are never typed into the app");
    }

    #[test]
    fn choosing_and_removing_accounts_report_clear_errors_and_tell_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let rx = b.subscribe();
        assert!(b.use_account("codex", "ghost").unwrap_err().to_string().contains("no account"));
        assert!(b.use_account("nope", "x").unwrap_err().to_string().contains("unknown agent"));
        assert!(b.remove_account("codex", "default", false).unwrap_err().to_string().contains("cannot be removed"));
        assert_eq!(b.use_account("codex", "default").unwrap(), None, "choosing the default account is fine and has nothing to warn about");
        assert!(rx.try_iter().any(|e| e == BackendEvent::AccountsChanged), "the UI is told to re-read the accounts");
    }

    #[test]
    fn adding_an_account_for_an_agent_that_cannot_have_one_is_refused_with_the_reason_before_anything_starts() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let e = b.add_account("antigravity", "Second", false).unwrap_err().to_string();
        assert!(e.contains("credential store"), "{e}");
        assert!(!dir.path().join("accounts").join("antigravity").exists());
        assert!(b.reauthenticate_account("codex", "default").unwrap_err().to_string().contains("default account"));
    }

    #[test]
    fn checking_the_accounts_runs_in_the_background_and_reports_back() {
        // A manager that can find no agent at all (an empty PATH under a temp home): the check starts no process, so this
        // never touches a real sign-in.
        use ai_usage_monitor_core::accounts::{AccountManager, AccountStore, MemorySecretStore, SystemRunner};
        use ai_usage_monitor_core::collectors::Env;
        let dir = tempfile::tempdir().unwrap();
        let db = ai_usage_monitor_core::database::Database::open(&dir.path().join("usage.db")).unwrap();
        let manager = Arc::new(AccountManager::new(AccountStore::new(db), Arc::new(SystemRunner), Arc::new(MemorySecretStore::new()), Env::with_home(dir.path()), dir.path().to_path_buf()));
        manager.init().unwrap();
        let listeners: Listeners = Arc::default();
        let (tx, rx) = mpsc::channel();
        listeners.lock().unwrap().push(tx);
        let woken = Arc::new(AtomicBool::new(false));
        let w = woken.clone();
        let handle = spawn_account_check(manager, listeners, Arc::new(move || w.store(true, Ordering::SeqCst))).unwrap();
        handle.join().unwrap();
        assert_eq!(rx.try_recv().unwrap(), BackendEvent::AccountsChanged);
        assert_eq!(rx.try_recv().unwrap(), BackendEvent::AccountsChecked, "the page can stop saying CHECKING");
        assert!(woken.load(Ordering::SeqCst), "the UI is woken, not left waiting for the next input");
    }

    #[test]
    fn quit_sets_a_flag_and_shutdown_makes_later_calls_fail_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        assert!(!b.quit_requested());
        b.quit();
        assert!(b.quit_requested());
        b.shutdown();
        assert!(b.settings().is_err());
        assert!(b.daily_totals(371).is_err());
        b.shutdown(); // idempotent
    }

    #[test]
    fn a_fresh_backend_has_no_collector_health_yet() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        assert!(b.collector_health().unwrap().is_empty());
    }

    #[test]
    fn subscribers_can_attach_and_the_overlay_diagnostics_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        let _rx = b.subscribe();
        let mut d = b.overlay_diagnostics().unwrap();
        d.visible = true;
        d.width = Some(236);
        b.set_overlay_diagnostics(d);
        assert_eq!(b.overlay_diagnostics().unwrap().width, Some(236));
    }
}
