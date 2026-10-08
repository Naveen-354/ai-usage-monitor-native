//! `Backend` implemented over the real `core` (collectors, SQLite, aggregation). No GUI types here: the UI repaint is
//! injected as a plain callback, so this module is tested without a window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use ai_usage_monitor_core::aggregation::Period;
use ai_usage_monitor_core::api::{Config, Core, CoreEvent};
use app_api::api::{Backend, BackendError, BackendEvent, CollectorHealth, SettingsPatch};
use app_api::view::{AppInfo, History, OverlayDiagnostics, Overview, PeriodOrCustom, Settings};

use crate::bridge;

type Listeners = Arc<Mutex<Vec<Sender<BackendEvent>>>>;

fn err(e: impl std::fmt::Display) -> BackendError {
    BackendError::Message(e.to_string())
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

    fn history(&self, _period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<History, BackendError> {
        Err(not_yet("the history timeline"))
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
    fn quit_sets_a_flag_and_shutdown_makes_later_calls_fail_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let b = backend(&dir);
        assert!(!b.quit_requested());
        b.quit();
        assert!(b.quit_requested());
        b.shutdown();
        assert!(b.settings().is_err());
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
