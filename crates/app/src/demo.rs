//! `--demo`: the fixture data (the same one the web reference page uses) instead of the real backend.
//!
//! It exists so the native UI can be photographed and compared with the old one on identical numbers, and so screenshots
//! never need real usage data. Timestamps are shifted to "now", so the first agent is live and the flame burns.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use app_api::api::{Backend, BackendError, BackendEvent, CollectorHealth, SettingsPatch};
use app_api::mock;
use app_api::view::{AppInfo, History, OverlayDiagnostics, Overview, PeriodOrCustom, Settings};
use serde_json::Value;

use crate::backend::AppBackend;
use crate::bridge;

pub struct DemoBackend {
    settings: Mutex<Settings>,
    shift_ms: i64,
    quit: AtomicBool,
    overlay: Mutex<OverlayDiagnostics>,
    listeners: Mutex<Vec<Sender<BackendEvent>>>,
}

fn err(m: impl std::fmt::Display) -> BackendError {
    BackendError::Message(m.to_string())
}

/// Merges `patch` (camelCase keys) into `s`; a key the settings do not have, or a value of the wrong type, is an error.
pub fn merge_settings(s: &Settings, patch: &Value) -> Result<Settings, String> {
    let mut doc = serde_json::to_value(s).map_err(|e| e.to_string())?;
    let (Some(map), Some(patch)) = (doc.as_object_mut(), patch.as_object()) else { return Err("settings patch must be an object".into()) };
    for (k, v) in patch {
        if !map.contains_key(k) {
            return Err(format!("unknown setting '{k}'"));
        }
        map.insert(k.clone(), v.clone());
    }
    serde_json::from_value(doc).map_err(|e| format!("invalid setting value: {e}"))
}

impl DemoBackend {
    /// `overrides` are `--set key=value` pairs applied to the fixture settings.
    pub fn new(overrides: &[(String, Value)]) -> Result<Self, String> {
        let mut settings = mock::settings();
        // The fixture's overlay position is the developer's own screen position; a demo starts wherever the OS puts it.
        settings.overlay_x = None;
        settings.overlay_y = None;
        for (k, v) in overrides {
            settings = merge_settings(&settings, &serde_json::json!({ k: v }))?;
        }
        let now = chrono::Utc::now().timestamp_millis();
        Ok(Self {
            settings: Mutex::new(settings),
            shift_ms: now - mock::NOW_MS,
            quit: AtomicBool::new(false),
            overlay: Mutex::new(mock::diagnostics()),
            listeners: Mutex::new(Vec::new()),
        })
    }

    fn shifted_overview(&self, period: PeriodOrCustom) -> Overview {
        let mut o = mock::overview();
        let s = self.settings.lock().map(|g| g.clone()).unwrap_or_else(|_| mock::settings());
        o.period = period;
        o.paused = s.paused;
        o.count_cached_in_total = s.count_cached_in_total;
        o.generated_at_utc_ms += self.shift_ms;
        o.range.start_utc_ms += self.shift_ms;
        o.range.end_utc_ms += self.shift_ms;
        for a in &mut o.agents {
            a.last_event_utc_ms = a.last_event_utc_ms.map(|t| t + self.shift_ms);
            a.enabled = s.enabled_agents.contains(&a.id) || a.totals.is_none();
        }
        o
    }

    fn broadcast(&self, ev: BackendEvent) {
        if let Ok(mut l) = self.listeners.lock() {
            l.retain(|tx| tx.send(ev.clone()).is_ok());
        }
    }
}

fn demo_only(what: &str) -> BackendError {
    err(format!("{what} is not available in the demo"))
}

impl Backend for DemoBackend {
    fn overview(&self, period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<Overview, BackendError> {
        Ok(self.shifted_overview(period))
    }

    fn history(&self, period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<History, BackendError> {
        Ok(mock::history(period))
    }

    fn settings(&self) -> Result<Settings, BackendError> {
        self.settings.lock().map(|s| s.clone()).map_err(|_| err("settings lock poisoned"))
    }

    fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, BackendError> {
        self.update_settings_json(&bridge::patch_to_json(&patch))
    }

    fn app_info(&self) -> Result<AppInfo, BackendError> {
        Ok(mock::app_info())
    }

    fn overlay_diagnostics(&self) -> Result<OverlayDiagnostics, BackendError> {
        self.overlay.lock().map(|o| o.clone()).map_err(|_| err("overlay lock poisoned"))
    }

    fn collector_health(&self) -> Result<Vec<CollectorHealth>, BackendError> {
        Ok(self.shifted_overview(PeriodOrCustom::Day).agents.iter().map(health).collect())
    }

    fn pause(&self) -> Result<(), BackendError> {
        self.update_settings_json(&serde_json::json!({"paused": true})).map(|_| ())
    }

    fn resume(&self) -> Result<(), BackendError> {
        self.update_settings_json(&serde_json::json!({"paused": false})).map(|_| ())
    }

    fn rescan_agent(&self, _agent_id: &str) -> Result<(), BackendError> {
        Ok(())
    }

    fn export_json(&self, _path: &str) -> Result<(), BackendError> {
        Err(demo_only("export"))
    }

    fn export_csv(&self, _path: &str) -> Result<(), BackendError> {
        Err(demo_only("export"))
    }

    fn clear_history(&self) -> Result<(), BackendError> {
        Err(demo_only("clearing history"))
    }

    fn open_data_folder(&self) -> Result<(), BackendError> {
        Err(demo_only("opening folders"))
    }

    fn open_log_folder(&self) -> Result<(), BackendError> {
        Err(demo_only("opening folders"))
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

fn health(a: &app_api::view::AgentOverview) -> CollectorHealth {
    CollectorHealth {
        agent: a.id.clone(),
        availability: a.availability.clone(),
        note: a.note.clone(),
        last_run_utc_ms: a.last_event_utc_ms,
        last_success_utc_ms: a.last_event_utc_ms,
        last_event_utc_ms: a.last_event_utc_ms,
        events_total: a.totals.map_or(0, |t| t.events),
        skipped_records: 0,
        source_paths: Vec::new(),
    }
}

impl AppBackend for DemoBackend {
    fn update_settings_json(&self, patch: &Value) -> Result<Settings, BackendError> {
        let updated = {
            let mut s = self.settings.lock().map_err(|_| err("settings lock poisoned"))?;
            let next = merge_settings(&s, patch).map_err(err)?;
            *s = next.clone();
            next
        };
        self.broadcast(BackendEvent::SettingsChanged(updated.clone()));
        Ok(updated)
    }

    fn set_overlay_diagnostics(&self, d: OverlayDiagnostics) {
        if let Ok(mut o) = self.overlay.lock() {
            *o = d;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    fn shutdown(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_api::view::{Availability, PeriodKey, Theme};

    fn demo(sets: &[(&str, Value)]) -> DemoBackend {
        let v: Vec<(String, Value)> = sets.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        DemoBackend::new(&v).unwrap()
    }

    #[test]
    fn timestamps_are_shifted_to_now_so_the_first_agent_is_live() {
        let d = demo(&[]);
        let o = d.overview(PeriodOrCustom::Day, None, None).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let claude = &o.agents[0];
        assert!(native_ui::web::hero::is_live(claude.last_event_utc_ms, now), "claude should be live: {:?}", claude.last_event_utc_ms);
        assert!(!native_ui::web::hero::is_live(o.agents[1].last_event_utc_ms, now), "codex is minutes old");
        assert!(o.range.start_utc_ms < now && now < o.range.end_utc_ms, "'today' contains now");
    }

    #[test]
    fn the_fixture_has_the_numbers_the_web_reference_shows() {
        let o = demo(&[]).overview(PeriodOrCustom::Day, None, None).unwrap();
        assert_eq!(o.totals.unwrap().total, 255_500_000);
        assert_eq!(native_ui::motion::format::format_compact(255_500_000.0, 4), "255.5M");
        assert_eq!(o.agents.len(), 5);
        assert!(o.agents[4].totals.is_none(), "ollama is the unavailable one");
    }

    #[test]
    fn set_overrides_apply_and_unknown_or_mistyped_keys_are_errors() {
        let d = demo(&[("compactMode", Value::Bool(false)), ("theme", Value::String("light".into())), ("overlayX", Value::from(300))]);
        let s = d.settings().unwrap();
        assert!(!s.compact_mode);
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.overlay_x, Some(300));
        assert!(DemoBackend::new(&[("nope".into(), Value::Bool(true))]).is_err());
        assert!(DemoBackend::new(&[("compactMode".into(), Value::String("maybe".into()))]).is_err());
    }

    #[test]
    fn the_demo_does_not_start_at_the_developers_own_screen_position() {
        assert_eq!(demo(&[]).settings().unwrap().overlay_x, None);
    }

    #[test]
    fn settings_changes_are_stored_and_announced() {
        let d = demo(&[]);
        let rx = d.subscribe();
        let s = d.update_settings(SettingsPatch { period: Some(PeriodKey::Week), ..Default::default() }).unwrap();
        assert_eq!(s.period, PeriodKey::Week);
        assert_eq!(d.settings().unwrap().period, PeriodKey::Week);
        assert!(matches!(rx.try_recv(), Ok(BackendEvent::SettingsChanged(_))));
    }

    #[test]
    fn pausing_shows_up_in_the_overview() {
        let d = demo(&[]);
        d.pause().unwrap();
        assert!(d.overview(PeriodOrCustom::Day, None, None).unwrap().paused);
        d.resume().unwrap();
        assert!(!d.overview(PeriodOrCustom::Day, None, None).unwrap().paused);
    }

    #[test]
    fn what_a_demo_cannot_do_it_says() {
        let d = demo(&[]);
        for r in [d.export_json("x"), d.export_csv("x"), d.clear_history(), d.open_data_folder(), d.open_log_folder()] {
            assert!(r.unwrap_err().to_string().contains("not available in the demo"));
        }
        assert!(!d.quit_requested());
        d.quit();
        assert!(d.quit_requested());
    }

    #[test]
    fn health_lists_every_demo_agent_and_marks_the_ledgerless_one_unavailable() {
        let h = demo(&[]).collector_health().unwrap();
        assert_eq!(h.len(), 5);
        assert!(matches!(h[4].availability, Availability::Unavailable { .. }));
    }
}
