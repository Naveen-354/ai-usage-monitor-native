//! User settings: one validated JSON document in the `settings` table.
//! Unknown/missing fields are tolerated so old databases keep loading after upgrades.

use std::sync::{Arc, RwLock};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::aggregation::Period;
use crate::database::Database;
use crate::error::Result;
use crate::model::catalog;

const KEY: &str = "app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    /// The user dragged the overlay; `overlay_x/y` are authoritative.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimationIntensity {
    Off,
    Low,
    Normal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    // GENERAL
    pub start_with_system: bool,
    /// ON by default and persisted.
    pub always_on_top: bool,
    pub overlay_corner: Corner,
    pub overlay_x: Option<i32>,
    pub overlay_y: Option<i32>,
    pub compact_size: Option<(u32, u32)>,
    pub expanded_size: Option<(u32, u32)>,
    pub overlay_opacity: f32,
    /// Size of the big token count, 0.6 – 2.0 (1.0 = default). The compact window grows/shrinks to fit it.
    pub token_text_size: f32,
    /// `true` = the overlay is collapsed to its compact state.
    pub compact_mode: bool,
    pub show_overlay_in_taskbar: bool,
    // MONITORING
    pub polling_interval_secs: u32,
    pub enabled_agents: Vec<String>,
    pub project_detection: bool,
    pub paused: bool,
    // DISPLAY
    pub theme: Theme,
    pub animation_intensity: AnimationIntensity,
    pub show_input: bool,
    pub show_output: bool,
    pub show_cached: bool,
    /// Whether cache reads/writes count toward headline totals (spec example: cached is part of input).
    pub count_cached_in_total: bool,
    /// 0 = Sunday … 6 = Saturday.
    pub week_starts_on: u8,
    pub period: Period,
    // DATA
    pub database_location: Option<String>,
    pub retention_days: Option<u32>,
    // SHORTCUTS
    pub shortcut_toggle: String,
    pub shortcut_expand: String,
    pub shortcut_focus: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            start_with_system: false,
            always_on_top: true,
            overlay_corner: Corner::TopRight,
            overlay_x: None,
            overlay_y: None,
            compact_size: None,
            expanded_size: None,
            overlay_opacity: 0.0,
            token_text_size: 1.0,
            compact_mode: true,
            show_overlay_in_taskbar: false,
            polling_interval_secs: 30,
            enabled_agents: catalog().iter().map(|a| a.id.to_string()).collect(),
            project_detection: true,
            paused: false,
            theme: Theme::Dark,
            animation_intensity: AnimationIntensity::Normal,
            show_input: true,
            show_output: true,
            show_cached: true,
            count_cached_in_total: true,
            week_starts_on: 1,
            period: Period::Day,
            database_location: None,
            retention_days: None,
            shortcut_toggle: "CommandOrControl+Alt+Shift+U".into(),
            shortcut_expand: "CommandOrControl+Alt+Shift+E".into(),
            shortcut_focus: "CommandOrControl+Alt+Shift+F".into(),
        }
    }
}

impl Settings {
    /// Clamp/repair values so a hand-edited or older document can never put the app in a bad state.
    pub fn validated(mut self) -> Settings {
        let d = Settings::default();
        self.overlay_opacity = if self.overlay_opacity.is_finite() { self.overlay_opacity.clamp(0.0, 1.0) } else { d.overlay_opacity };
        self.token_text_size = if self.token_text_size.is_finite() { self.token_text_size.clamp(0.6, 2.0) } else { d.token_text_size };
        self.polling_interval_secs = self.polling_interval_secs.clamp(5, 3600);
        if self.week_starts_on > 6 {
            self.week_starts_on = d.week_starts_on;
        }
        if self.period == Period::Custom {
            self.period = Period::Day;
        }
        self.retention_days = self.retention_days.filter(|d| *d >= 1);
        let clamp_size = |s: Option<(u32, u32)>| s.map(|(w, h)| (w.clamp(160, 4000), h.clamp(80, 4000)));
        self.compact_size = clamp_size(self.compact_size);
        self.expanded_size = clamp_size(self.expanded_size);
        self.database_location = self.database_location.filter(|s| !s.trim().is_empty());
        let mut seen = std::collections::HashSet::new();
        self.enabled_agents.retain(|a| !a.trim().is_empty() && seen.insert(a.clone()));
        for (value, default) in [
            (&mut self.shortcut_toggle, d.shortcut_toggle),
            (&mut self.shortcut_expand, d.shortcut_expand),
            (&mut self.shortcut_focus, d.shortcut_focus),
        ] {
            if value.trim().is_empty() {
                *value = default;
            }
        }
        self
    }

    pub fn agent_enabled(&self, id: &str) -> bool {
        self.enabled_agents.iter().any(|a| a == id)
    }
}

fn merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge(t.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => *t = p.clone(),
    }
}

pub struct SettingsStore {
    db: Arc<Database>,
    current: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(db: Arc<Database>) -> Result<SettingsStore> {
        let raw: Option<String> = db.with_reader(|c| {
            Ok(c.query_row("SELECT value FROM settings WHERE key = ?1", [KEY], |r| r.get(0)).optional()?)
        })?;
        let settings = match raw {
            Some(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s.validated(),
                Err(e) => {
                    tracing::warn!("settings document unreadable ({e}); using defaults");
                    Settings::default()
                }
            },
            None => Settings::default(),
        };
        Ok(SettingsStore { db, current: RwLock::new(settings) })
    }

    pub fn get(&self) -> Settings {
        self.current.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn persist(&self, s: &Settings) -> Result<()> {
        let json = serde_json::to_string(s)?;
        self.db.with_writer(|w| {
            w.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![KEY, json],
            )?;
            Ok(())
        })
    }

    /// Apply a partial JSON document (`{"alwaysOnTop": false}`), validate, persist, and return the result.
    pub fn update(&self, patch: &serde_json::Value) -> Result<Settings> {
        let mut doc = serde_json::to_value(self.get())?;
        merge(&mut doc, patch);
        let next: Settings = serde_json::from_value(doc)
            .map_err(|e| crate::error::AppError::Invalid(format!("settings: {e}")))?;
        self.store(next.validated())
    }

    pub fn modify(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut s = self.get();
        f(&mut s);
        self.store(s.validated())
    }

    fn store(&self, next: Settings) -> Result<Settings> {
        self.persist(&next)?;
        *self.current.write().unwrap_or_else(|p| p.into_inner()) = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(s.overlay_opacity, 0.0, "the overlay background is see-through by default");
        assert!(s.always_on_top, "ALWAYS ON TOP must default to ON");
        assert!(s.compact_mode);
        assert_eq!(s.period, Period::Day);
        assert!(s.project_detection);
        assert_eq!(s.enabled_agents.len(), catalog().len());
        assert!(!s.paused);
    }

    #[test]
    fn partial_updates_merge_and_persist_across_reload() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db.clone()).unwrap();
        store.update(&serde_json::json!({"alwaysOnTop": false, "overlayOpacity": 0.5})).unwrap();
        let reloaded = SettingsStore::load(db).unwrap().get();
        assert!(!reloaded.always_on_top);
        assert_eq!(reloaded.overlay_opacity, 0.5);
        assert!(reloaded.show_input, "untouched fields keep their value");
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db).unwrap();
        let s = store
            .update(&serde_json::json!({"overlayOpacity": 9.0, "pollingIntervalSecs": 1, "weekStartsOn": 12, "retentionDays": 0}))
            .unwrap();
        assert_eq!(s.overlay_opacity, 1.0);
        assert_eq!(store.update(&serde_json::json!({"overlayOpacity": -3})).unwrap().overlay_opacity, 0.0);
        assert_eq!(s.polling_interval_secs, 5);
        assert_eq!(s.week_starts_on, 1);
        assert_eq!(s.retention_days, None);
    }

    #[test]
    fn token_text_size_defaults_to_one_and_is_clamped() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db).unwrap();
        assert_eq!(store.get().token_text_size, 1.0);
        assert_eq!(store.update(&serde_json::json!({"tokenTextSize": 9.0})).unwrap().token_text_size, 2.0);
        assert_eq!(store.update(&serde_json::json!({"tokenTextSize": 0.1})).unwrap().token_text_size, 0.6);
        assert_eq!(store.update(&serde_json::json!({"tokenTextSize": 1.5})).unwrap().token_text_size, 1.5);
    }

    #[test]
    fn a_type_error_is_rejected_and_the_old_value_survives() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db).unwrap();
        assert!(store.update(&serde_json::json!({"alwaysOnTop": "yes"})).is_err());
        assert!(store.get().always_on_top);
    }

    #[test]
    fn an_old_document_missing_new_fields_still_loads() {
        let (_d, db) = temp_db();
        db.with_writer(|w| {
            w.conn.execute("INSERT INTO settings (key, value) VALUES ('app', '{\"alwaysOnTop\": false, \"someRemovedField\": 1}')", [])?;
            Ok(())
        })
        .unwrap();
        let s = SettingsStore::load(db).unwrap().get();
        assert!(!s.always_on_top);
        assert_eq!(s.shortcut_toggle, Settings::default().shortcut_toggle);
    }

    #[test]
    fn a_corrupt_document_falls_back_to_defaults_instead_of_crashing() {
        let (_d, db) = temp_db();
        db.with_writer(|w| {
            w.conn.execute("INSERT INTO settings (key, value) VALUES ('app', 'not json')", [])?;
            Ok(())
        })
        .unwrap();
        assert!(SettingsStore::load(db).unwrap().get().always_on_top);
    }

    #[test]
    fn custom_is_not_a_persistable_period() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db).unwrap();
        assert_eq!(store.update(&serde_json::json!({"period": "custom"})).unwrap().period, Period::Day);
        assert_eq!(store.update(&serde_json::json!({"period": "month"})).unwrap().period, Period::Month);
    }

    #[test]
    fn duplicate_and_blank_agents_are_dropped_and_blank_shortcuts_restored() {
        let (_d, db) = temp_db();
        let store = SettingsStore::load(db).unwrap();
        let s = store
            .update(&serde_json::json!({"enabledAgents": ["codex", "codex", " ", "claude"], "shortcutToggle": ""}))
            .unwrap();
        assert_eq!(s.enabled_agents, vec!["codex", "claude"]);
        assert_eq!(s.shortcut_toggle, Settings::default().shortcut_toggle);
    }
}
