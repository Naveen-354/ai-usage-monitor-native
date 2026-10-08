//! Shared view-model types. Field-for-field mirror of `src/types/index.ts` (the DTOs the Rust backend already
//! produces, serde camelCase). **Identical in every agent's copy - do not edit; define extras in your own module.**
//!
//! The "NEW" section at the bottom has types the web version does not have yet (history timeline for the
//! Statistics page).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeriodKey {
    Day,
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeriodOrCustom {
    Day,
    Week,
    Month,
    Year,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Availability {
    Ok,
    NoDataYet,
    NotInstalled,
    Unavailable { reason: String },
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    /// Part of the total that came from estimated (not actual) events.
    pub estimated: u64,
    pub events: u64,
    /// Headline total, honouring the "count cached tokens in total" setting.
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverview {
    pub id: String,
    pub name: String,
    pub short: String,
    /// CSS-style hex colour, e.g. "#ff6a00".
    pub color: String,
    pub enabled: bool,
    pub availability: Availability,
    pub note: Option<String>,
    /// `None` = numbers must NOT be shown (print TOKEN DATA UNAVAILABLE, never 0).
    pub totals: Option<Totals>,
    pub last_event_utc_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub period: PeriodOrCustom,
    pub range: Range,
    pub generated_at_utc_ms: i64,
    /// `None` when no agent has numbers.
    pub totals: Option<Totals>,
    pub agents: Vec<AgentOverview>,
    pub unavailable_agents: u32,
    pub any_data: bool,
    pub paused: bool,
    pub count_cached_in_total: bool,
    /// Agents whose history is being imported for the first time right now.
    pub importing_agents: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationIntensity {
    Off,
    Low,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub start_with_system: bool,
    pub always_on_top: bool,
    pub overlay_corner: Corner,
    pub overlay_x: Option<i32>,
    pub overlay_y: Option<i32>,
    pub compact_size: Option<(u32, u32)>,
    pub expanded_size: Option<(u32, u32)>,
    /// 0.0 = fully see-through (default) .. 1.0 = opaque panel.
    pub overlay_opacity: f32,
    /// 0.6 ..= 2.0 multiplier for the big count.
    pub token_text_size: f32,
    pub compact_mode: bool,
    pub show_overlay_in_taskbar: bool,
    pub polling_interval_secs: u32,
    pub enabled_agents: Vec<String>,
    pub project_detection: bool,
    pub paused: bool,
    pub theme: Theme,
    pub animation_intensity: AnimationIntensity,
    pub show_input: bool,
    pub show_output: bool,
    pub show_cached: bool,
    pub count_cached_in_total: bool,
    /// 0 = Sunday .. 6 = Saturday.
    pub week_starts_on: u8,
    pub period: PeriodKey,
    pub database_location: Option<String>,
    pub retention_days: Option<u32>,
    pub shortcut_toggle: String,
    pub shortcut_expand: String,
    pub shortcut_focus: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub database_path: String,
    pub log_dir: String,
    /// Present when the database was damaged and rebuilt at start-up.
    pub database_notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayDiagnostics {
    pub visible: bool,
    pub minimized: bool,
    pub always_on_top_setting: bool,
    pub decorated: Option<bool>,
    pub os_reports_topmost: Option<bool>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub scale_factor: Option<f32>,
    pub monitors: u32,
    pub platform_note: String,
}

// ---------------------------------------------------------------------------------------------------------------
// NEW: history timeline (the web Statistics page is still a placeholder; Phase 4 of the plan)
// ---------------------------------------------------------------------------------------------------------------

/// One bar of the timeline: a time bucket (hour for DAY, day for WEEK/MONTH, month for YEAR) with per-agent totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBucket {
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    /// Total tokens in the bucket (all agents, honouring the cached-in-total setting).
    pub total: u64,
    /// (agent id, tokens) for agents with data in this bucket. Sum equals `total`.
    pub by_agent: Vec<(String, u64)>,
}

/// History for the Statistics page. `buckets` is ascending by time and gap-free (empty buckets have total 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub period: PeriodOrCustom,
    pub range: Range,
    pub buckets: Vec<TimelineBucket>,
    /// Same as `Overview::totals` for this range.
    pub totals: Option<Totals>,
    /// Per-model totals: (model name, agent id, tokens).
    pub by_model: Vec<(String, String, u64)>,
    /// Per-project totals: (project name or "UNKNOWN PROJECT", tokens).
    pub by_project: Vec<(String, u64)>,
}
