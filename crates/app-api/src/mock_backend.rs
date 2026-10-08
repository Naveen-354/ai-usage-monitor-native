use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver, Sender};
use crate::api::{Backend, BackendError, BackendEvent, CollectorHealth, SettingsPatch};
use crate::view::{AccountsOverview, AppInfo, DayTotal, History, OverlayDiagnostics, Overview, PeriodOrCustom, Settings};
use crate::mock;

pub struct MockBackend {
    settings: Arc<Mutex<Settings>>,
    next_error: Arc<Mutex<Option<BackendError>>>,
    tx: Sender<BackendEvent>,
    rx: Mutex<Option<Receiver<BackendEvent>>>,
    overview: Arc<Mutex<Overview>>,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            settings: Arc::new(Mutex::new(mock::settings())),
            next_error: Arc::new(Mutex::new(None)),
            tx,
            rx: Mutex::new(Some(rx)),
            overview: Arc::new(Mutex::new(mock::overview())),
        }
    }

    pub fn fail_next(&self, error: BackendError) {
        *self.next_error.lock().unwrap() = Some(error);
    }

    pub fn emit_database_notice(&self, msg: &str) {
        let _ = self.tx.send(BackendEvent::DatabaseNotice(msg.to_string()));
    }

    pub fn tick(&self, additional_tokens: u64) {
        let mut o = self.overview.lock().unwrap();
        if let Some(t) = &mut o.totals {
            t.total += additional_tokens;
            t.input += additional_tokens / 2;
            t.output += additional_tokens / 2;
        }
        let _ = self.tx.send(BackendEvent::OverviewChanged);
    }

    fn check_fail(&self) -> Result<(), BackendError> {
        if let Some(err) = self.next_error.lock().unwrap().take() {
            return Err(err);
        }
        Ok(())
    }
}

impl Backend for MockBackend {
    fn overview(&self, _period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<Overview, BackendError> {
        self.check_fail()?;
        Ok(self.overview.lock().unwrap().clone())
    }

    fn history(&self, period: PeriodOrCustom, _from: Option<String>, _to: Option<String>) -> Result<History, BackendError> {
        self.check_fail()?;
        Ok(mock::history(period))
    }

    fn daily_totals(&self, days: u32) -> Result<Vec<DayTotal>, BackendError> {
        self.check_fail()?;
        Ok(mock::daily_totals(mock::today(), days))
    }

    fn accounts(&self) -> Result<AccountsOverview, BackendError> {
        self.check_fail()?;
        Ok(mock::accounts_overview())
    }

    fn use_account(&self, _agent: &str, _account: &str) -> Result<Option<String>, BackendError> {
        self.check_fail()?;
        Ok(None)
    }

    fn remove_account(&self, _agent: &str, _account: &str, _purge_usage: bool) -> Result<(), BackendError> {
        self.check_fail()
    }

    fn add_account(&self, _agent: &str, _label: &str, _device_code: bool) -> Result<(), BackendError> {
        self.check_fail()
    }

    fn reauthenticate_account(&self, _agent: &str, _account: &str) -> Result<(), BackendError> {
        self.check_fail()
    }

    fn check_accounts(&self) -> Result<(), BackendError> {
        self.check_fail()
    }

    fn settings(&self) -> Result<Settings, BackendError> {
        self.check_fail()?;
        Ok(self.settings.lock().unwrap().clone())
    }

    fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, BackendError> {
        self.check_fail()?;
        let mut s = self.settings.lock().unwrap();
        
        if let Some(v) = patch.start_with_system { s.start_with_system = v; }
        if let Some(v) = patch.always_on_top { s.always_on_top = v; }
        if let Some(v) = patch.overlay_corner { s.overlay_corner = v; }
        if let Some(v) = patch.overlay_x { s.overlay_x = v; }
        if let Some(v) = patch.overlay_y { s.overlay_y = v; }
        if let Some(v) = patch.compact_size { s.compact_size = v; }
        if let Some(v) = patch.expanded_size { s.expanded_size = v; }
        if let Some(v) = patch.overlay_opacity { s.overlay_opacity = v; }
        if let Some(v) = patch.token_text_size { s.token_text_size = v; }
        if let Some(v) = patch.compact_mode { s.compact_mode = v; }
        if let Some(v) = patch.show_overlay_in_taskbar { s.show_overlay_in_taskbar = v; }
        if let Some(v) = patch.polling_interval_secs { s.polling_interval_secs = v; }
        if let Some(v) = patch.enabled_agents { s.enabled_agents = v; }
        if let Some(v) = patch.project_detection { s.project_detection = v; }
        if let Some(v) = patch.paused { s.paused = v; }
        if let Some(v) = patch.theme { s.theme = v; }
        if let Some(v) = patch.animation_intensity { s.animation_intensity = v; }
        if let Some(v) = patch.show_input { s.show_input = v; }
        if let Some(v) = patch.show_output { s.show_output = v; }
        if let Some(v) = patch.show_cached { s.show_cached = v; }
        if let Some(v) = patch.count_cached_in_total { s.count_cached_in_total = v; }
        if let Some(v) = patch.week_starts_on { s.week_starts_on = v; }
        if let Some(v) = patch.period { s.period = v; }
        if let Some(v) = patch.database_location { s.database_location = v; }
        if let Some(v) = patch.retention_days { s.retention_days = v; }
        if let Some(v) = patch.shortcut_toggle { s.shortcut_toggle = v; }
        if let Some(v) = patch.shortcut_expand { s.shortcut_expand = v; }
        if let Some(v) = patch.shortcut_focus { s.shortcut_focus = v; }
        
        let new_s = s.clone();
        let _ = self.tx.send(BackendEvent::SettingsChanged(new_s.clone()));
        Ok(new_s)
    }

    fn app_info(&self) -> Result<AppInfo, BackendError> {
        self.check_fail()?;
        Ok(mock::app_info())
    }

    fn overlay_diagnostics(&self) -> Result<OverlayDiagnostics, BackendError> {
        self.check_fail()?;
        Ok(mock::diagnostics())
    }

    fn collector_health(&self) -> Result<Vec<CollectorHealth>, BackendError> {
        self.check_fail()?;
        Ok(vec![])
    }

    fn pause(&self) -> Result<(), BackendError> { self.check_fail() }
    fn resume(&self) -> Result<(), BackendError> { self.check_fail() }
    fn rescan_agent(&self, _agent_id: &str) -> Result<(), BackendError> { self.check_fail() }
    fn export_json(&self, _path: &str) -> Result<(), BackendError> { self.check_fail() }
    fn export_csv(&self, _path: &str) -> Result<(), BackendError> { self.check_fail() }
    fn clear_history(&self) -> Result<(), BackendError> { self.check_fail() }
    fn open_data_folder(&self) -> Result<(), BackendError> { self.check_fail() }
    fn open_log_folder(&self) -> Result<(), BackendError> { self.check_fail() }
    fn quit(&self) {}

    fn subscribe(&self) -> Receiver<BackendEvent> {
        self.rx.lock().unwrap().take().expect("subscribe can only be called once")
    }
}
