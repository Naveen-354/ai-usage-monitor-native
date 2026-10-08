use crate::view::{AccountsOverview, AppInfo, Availability, DayTotal, History, OverlayDiagnostics, Overview, PeriodOrCustom, Settings};
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsPatch {
    pub start_with_system: Option<bool>,
    pub always_on_top: Option<bool>,
    pub overlay_corner: Option<crate::view::Corner>,
    pub overlay_x: Option<Option<i32>>,
    pub overlay_y: Option<Option<i32>>,
    pub compact_size: Option<Option<(u32, u32)>>,
    pub expanded_size: Option<Option<(u32, u32)>>,
    pub overlay_opacity: Option<f32>,
    pub token_text_size: Option<f32>,
    pub compact_mode: Option<bool>,
    pub show_overlay_in_taskbar: Option<bool>,
    pub polling_interval_secs: Option<u32>,
    pub enabled_agents: Option<Vec<String>>,
    pub project_detection: Option<bool>,
    pub paused: Option<bool>,
    pub theme: Option<crate::view::Theme>,
    pub animation_intensity: Option<crate::view::AnimationIntensity>,
    pub show_input: Option<bool>,
    pub show_output: Option<bool>,
    pub show_cached: Option<bool>,
    pub count_cached_in_total: Option<bool>,
    pub week_starts_on: Option<u8>,
    pub period: Option<crate::view::PeriodKey>,
    pub database_location: Option<Option<String>>,
    pub retention_days: Option<Option<u32>>,
    pub shortcut_toggle: Option<String>,
    pub shortcut_expand: Option<String>,
    pub shortcut_focus: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendError {
    Message(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Message(m) => write!(f, "{}", m),
        }
    }
}
impl std::error::Error for BackendError {}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
    OverviewChanged,
    SettingsChanged(Settings),
    HealthChanged,
    DatabaseNotice(String),
    ImportProgress { agent: String, percent: u32 },
    /// An account was added, removed, switched, checked or signed in: re-read the accounts.
    AccountsChanged,
    /// Something about accounts the user should read (a sign-in finished, or failed, and why).
    AccountNotice(String),
    /// The background check of every account's sign-in has finished.
    AccountsChecked,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectorHealth {
    pub agent: String,
    pub availability: Availability,
    pub note: Option<String>,
    pub last_run_utc_ms: Option<i64>,
    pub last_success_utc_ms: Option<i64>,
    pub last_event_utc_ms: Option<i64>,
    pub events_total: u64,
    pub skipped_records: u64,
    pub source_paths: Vec<String>,
}

pub trait Backend: Send + Sync {
    fn overview(&self, period: PeriodOrCustom, from: Option<String>, to: Option<String>) -> Result<Overview, BackendError>;
    fn history(&self, period: PeriodOrCustom, from: Option<String>, to: Option<String>) -> Result<History, BackendError>;
    /// One total per local calendar day for the `days` days up to and including today (ascending; days without usage are
    /// left out). Counts the same things the overview counts. This is what the activity heat map draws.
    fn daily_totals(&self, days: u32) -> Result<Vec<DayTotal>, BackendError>;
    /// Agent -> accounts -> active account -> usage, for the Accounts page.
    fn accounts(&self) -> Result<AccountsOverview, BackendError>;
    /// Makes an account the one its agent uses from now on; returns a warning to show (e.g. "its sign-in has expired").
    fn use_account(&self, agent: &str, account: &str) -> Result<Option<String>, BackendError>;
    /// Forgets an account: its sign-in folder and any key kept for it. Usage history stays unless `purge_usage`.
    fn remove_account(&self, agent: &str, account: &str, purge_usage: bool) -> Result<(), BackendError>;
    /// Starts signing in a new account in a console window of its own and returns at once; the outcome arrives as
    /// [`BackendEvent::AccountsChanged`] and [`BackendEvent::AccountNotice`].
    fn add_account(&self, agent: &str, label: &str, device_code: bool) -> Result<(), BackendError>;
    /// Starts signing an existing account in again (its session expired); same notification as `add_account`.
    fn reauthenticate_account(&self, agent: &str, account: &str) -> Result<(), BackendError>;
    /// Asks every agent, in the background, whether each account is still signed in.
    fn check_accounts(&self) -> Result<(), BackendError>;
    fn settings(&self) -> Result<Settings, BackendError>;
    fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, BackendError>;
    fn app_info(&self) -> Result<AppInfo, BackendError>;
    fn overlay_diagnostics(&self) -> Result<OverlayDiagnostics, BackendError>;
    fn collector_health(&self) -> Result<Vec<CollectorHealth>, BackendError>;

    fn pause(&self) -> Result<(), BackendError>;
    fn resume(&self) -> Result<(), BackendError>;
    fn rescan_agent(&self, agent_id: &str) -> Result<(), BackendError>;
    fn export_json(&self, path: &str) -> Result<(), BackendError>;
    fn export_csv(&self, path: &str) -> Result<(), BackendError>;
    fn clear_history(&self) -> Result<(), BackendError>;

    fn open_data_folder(&self) -> Result<(), BackendError>;
    fn open_log_folder(&self) -> Result<(), BackendError>;
    fn quit(&self);

    fn subscribe(&self) -> Receiver<BackendEvent>;
}
