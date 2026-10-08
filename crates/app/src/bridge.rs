//! Conversions between the backend's JSON-shaped types (`core`) and the UI's view types (`app-api::view`).
//!
//! Both sides serialise to the same camelCase shapes (the former Tauri DTOs), so converting through
//! `serde_json::Value` is exact, and any drift between the two fails loudly in the tests below instead of
//! silently showing wrong numbers.

use app_api::api::SettingsPatch;
use app_api::view::Settings;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

/// Re-shapes `v` as `U` through its JSON form.
pub fn convert<T: Serialize, U: DeserializeOwned>(v: &T) -> Result<U, String> {
    let value = serde_json::to_value(v).map_err(|e| e.to_string())?;
    serde_json::from_value(value).map_err(|e| format!("backend and UI types disagree: {e}"))
}

/// The settings keys a patch sets, as the camelCase JSON document the backend merges. `Some(None)` clears a field (`null`).
pub fn patch_to_json(p: &SettingsPatch) -> Value {
    let mut m = Map::new();
    macro_rules! put {
        ($field:ident, $key:literal) => {
            if let Some(v) = &p.$field {
                m.insert($key.to_string(), serde_json::to_value(v).unwrap_or(Value::Null));
            }
        };
    }
    put!(start_with_system, "startWithSystem");
    put!(always_on_top, "alwaysOnTop");
    put!(overlay_corner, "overlayCorner");
    put!(overlay_x, "overlayX");
    put!(overlay_y, "overlayY");
    put!(compact_size, "compactSize");
    put!(expanded_size, "expandedSize");
    put!(overlay_opacity, "overlayOpacity");
    put!(token_text_size, "tokenTextSize");
    put!(compact_mode, "compactMode");
    put!(show_overlay_in_taskbar, "showOverlayInTaskbar");
    put!(polling_interval_secs, "pollingIntervalSecs");
    put!(enabled_agents, "enabledAgents");
    put!(project_detection, "projectDetection");
    put!(paused, "paused");
    put!(theme, "theme");
    put!(animation_intensity, "animationIntensity");
    put!(show_input, "showInput");
    put!(show_output, "showOutput");
    put!(show_cached, "showCached");
    put!(count_cached_in_total, "countCachedInTotal");
    put!(week_starts_on, "weekStartsOn");
    put!(period, "period");
    put!(database_location, "databaseLocation");
    put!(retention_days, "retentionDays");
    put!(shortcut_toggle, "shortcutToggle");
    put!(shortcut_expand, "shortcutExpand");
    put!(shortcut_focus, "shortcutFocus");
    Value::Object(m)
}

/// Only the keys whose values differ between two settings documents (what the UI changed).
pub fn settings_diff(old: &Settings, new: &Settings) -> Value {
    let (Ok(Value::Object(a)), Ok(Value::Object(b))) = (serde_json::to_value(old), serde_json::to_value(new)) else {
        return Value::Object(Map::new());
    };
    Value::Object(b.into_iter().filter(|(k, v)| a.get(k) != Some(v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_monitor_core::model::{Availability as CoreAvail, CollectorHealth as CoreHealth};
    use ai_usage_monitor_core::settings::Settings as CoreSettings;
    use app_api::view::{self, PeriodKey, Theme};
    use native_ui::diagnostics::CollectorHealth as UiHealth;

    #[test]
    fn the_backends_default_settings_fit_the_ui_settings_type() {
        let ui: Settings = convert(&CoreSettings::default()).unwrap();
        assert_eq!(ui.token_text_size, 1.0);
        assert!(ui.always_on_top);
        assert_eq!(ui.period, PeriodKey::Day);
    }

    #[test]
    fn ui_settings_survive_a_round_trip_through_the_backend_type() {
        let ui = app_api::mock::settings();
        let core: CoreSettings = convert(&ui).unwrap();
        let back: Settings = convert(&core).unwrap();
        assert_eq!(back, ui);
    }

    #[test]
    fn collector_health_fits_the_diagnostics_type_for_every_state() {
        for a in [
            CoreAvail::Ok,
            CoreAvail::NoDataYet,
            CoreAvail::NotInstalled,
            CoreAvail::Unavailable { reason: "no ledger".into() },
            CoreAvail::Error { message: "boom".into() },
        ] {
            let ui: UiHealth = convert(&CoreHealth::new("codex", a.clone())).unwrap();
            assert_eq!(ui.availability.as_str(), a.as_str());
        }
    }

    #[test]
    fn a_patch_becomes_only_the_keys_it_sets() {
        let p = SettingsPatch { token_text_size: Some(1.5), overlay_x: Some(None), theme: Some(Theme::Light), ..Default::default() };
        let j = patch_to_json(&p);
        assert_eq!(j, serde_json::json!({"tokenTextSize": 1.5, "overlayX": null, "theme": "light"}));
        assert_eq!(patch_to_json(&SettingsPatch::default()), serde_json::json!({}));
    }

    #[test]
    fn every_patch_field_is_a_real_backend_setting() {
        // A key the backend does not know would be silently ignored; catch that here.
        let all = SettingsPatch {
            start_with_system: Some(true),
            always_on_top: Some(true),
            overlay_corner: Some(view::Corner::TopLeft),
            overlay_x: Some(Some(1)),
            overlay_y: Some(Some(2)),
            compact_size: Some(Some((1, 2))),
            expanded_size: Some(Some((3, 4))),
            overlay_opacity: Some(0.5),
            token_text_size: Some(1.2),
            compact_mode: Some(false),
            show_overlay_in_taskbar: Some(true),
            polling_interval_secs: Some(7),
            enabled_agents: Some(vec!["codex".into()]),
            project_detection: Some(false),
            paused: Some(true),
            theme: Some(Theme::Light),
            animation_intensity: Some(view::AnimationIntensity::Low),
            show_input: Some(false),
            show_output: Some(false),
            show_cached: Some(false),
            count_cached_in_total: Some(false),
            week_starts_on: Some(3),
            period: Some(PeriodKey::Month),
            database_location: Some(Some("D:\\x".into())),
            retention_days: Some(Some(30)),
            shortcut_toggle: Some("Ctrl+Alt+A".into()),
            shortcut_expand: Some("Ctrl+Alt+B".into()),
            shortcut_focus: Some("Ctrl+Alt+C".into()),
        };
        let json = patch_to_json(&all);
        let n_keys = json.as_object().unwrap().len();
        assert_eq!(n_keys, 28);
        let applied: CoreSettings = serde_json::from_value(json.clone()).unwrap();
        let echoed = serde_json::to_value(&applied).unwrap();
        for (k, v) in json.as_object().unwrap() {
            assert_eq!(echoed.get(k), Some(v), "backend ignored or reshaped `{k}`");
        }
    }

    #[test]
    fn a_diff_contains_only_what_changed() {
        let a = app_api::mock::settings();
        let mut b = a.clone();
        assert_eq!(settings_diff(&a, &b), serde_json::json!({}));
        b.token_text_size = 1.4;
        b.always_on_top = false;
        assert_eq!(settings_diff(&a, &b), serde_json::json!({"tokenTextSize": 1.4_f32, "alwaysOnTop": false}));
    }
}
