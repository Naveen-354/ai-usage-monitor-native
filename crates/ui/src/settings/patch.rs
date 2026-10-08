//! Settings diffing, patching, and validation logic.

use serde::{Deserialize, Serialize};
use crate::view::{self, AnimationIntensity, Corner, PeriodKey, Theme};

/// Represents partial changes to `view::Settings`.
/// Each field is `Some(...)` only if modified.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub start_with_system: Option<bool>,
    pub always_on_top: Option<bool>,
    pub overlay_corner: Option<Corner>,
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
    pub theme: Option<Theme>,
    pub animation_intensity: Option<AnimationIntensity>,
    pub show_input: Option<bool>,
    pub show_output: Option<bool>,
    pub show_cached: Option<bool>,
    pub count_cached_in_total: Option<bool>,
    pub week_starts_on: Option<u8>,
    pub period: Option<PeriodKey>,
    pub database_location: Option<Option<String>>,
    pub retention_days: Option<Option<u32>>,
    pub shortcut_toggle: Option<String>,
    pub shortcut_expand: Option<String>,
    pub shortcut_focus: Option<String>,
}

impl SettingsPatch {
    /// Returns true if no fields are present in the patch.
    pub fn is_empty(&self) -> bool {
        self.start_with_system.is_none()
            && self.always_on_top.is_none()
            && self.overlay_corner.is_none()
            && self.overlay_x.is_none()
            && self.overlay_y.is_none()
            && self.compact_size.is_none()
            && self.expanded_size.is_none()
            && self.overlay_opacity.is_none()
            && self.token_text_size.is_none()
            && self.compact_mode.is_none()
            && self.show_overlay_in_taskbar.is_none()
            && self.polling_interval_secs.is_none()
            && self.enabled_agents.is_none()
            && self.project_detection.is_none()
            && self.paused.is_none()
            && self.theme.is_none()
            && self.animation_intensity.is_none()
            && self.show_input.is_none()
            && self.show_output.is_none()
            && self.show_cached.is_none()
            && self.count_cached_in_total.is_none()
            && self.week_starts_on.is_none()
            && self.period.is_none()
            && self.database_location.is_none()
            && self.retention_days.is_none()
            && self.shortcut_toggle.is_none()
            && self.shortcut_expand.is_none()
            && self.shortcut_focus.is_none()
    }

    /// Applies this patch onto a target `view::Settings` instance.
    pub fn apply_to(&self, s: &mut view::Settings) {
        if let Some(v) = self.start_with_system {
            s.start_with_system = v;
        }
        if let Some(v) = self.always_on_top {
            s.always_on_top = v;
        }
        if let Some(v) = self.overlay_corner {
            s.overlay_corner = v;
        }
        if let Some(v) = self.overlay_x {
            s.overlay_x = v;
        }
        if let Some(v) = self.overlay_y {
            s.overlay_y = v;
        }
        if let Some(v) = self.compact_size {
            s.compact_size = v;
        }
        if let Some(v) = self.expanded_size {
            s.expanded_size = v;
        }
        if let Some(v) = self.overlay_opacity {
            s.overlay_opacity = v;
        }
        if let Some(v) = self.token_text_size {
            s.token_text_size = v;
        }
        if let Some(v) = self.compact_mode {
            s.compact_mode = v;
        }
        if let Some(v) = self.show_overlay_in_taskbar {
            s.show_overlay_in_taskbar = v;
        }
        if let Some(v) = self.polling_interval_secs {
            s.polling_interval_secs = v;
        }
        if let Some(ref v) = self.enabled_agents {
            s.enabled_agents = v.clone();
        }
        if let Some(v) = self.project_detection {
            s.project_detection = v;
        }
        if let Some(v) = self.paused {
            s.paused = v;
        }
        if let Some(v) = self.theme {
            s.theme = v;
        }
        if let Some(v) = self.animation_intensity {
            s.animation_intensity = v;
        }
        if let Some(v) = self.show_input {
            s.show_input = v;
        }
        if let Some(v) = self.show_output {
            s.show_output = v;
        }
        if let Some(v) = self.show_cached {
            s.show_cached = v;
        }
        if let Some(v) = self.count_cached_in_total {
            s.count_cached_in_total = v;
        }
        if let Some(v) = self.week_starts_on {
            s.week_starts_on = v;
        }
        if let Some(v) = self.period {
            s.period = v;
        }
        if let Some(ref v) = self.database_location {
            s.database_location = v.clone();
        }
        if let Some(v) = self.retention_days {
            s.retention_days = v;
        }
        if let Some(ref v) = self.shortcut_toggle {
            s.shortcut_toggle = v.clone();
        }
        if let Some(ref v) = self.shortcut_expand {
            s.shortcut_expand = v.clone();
        }
        if let Some(ref v) = self.shortcut_focus {
            s.shortcut_focus = v.clone();
        }
    }
}

/// Computes the diff between two `Settings` structs, producing a `SettingsPatch`
/// that contains ONLY the modified fields. If both are identical, `is_empty()` is true.
pub fn diff(old: &view::Settings, new: &view::Settings) -> SettingsPatch {
    SettingsPatch {
        start_with_system: if old.start_with_system != new.start_with_system {
            Some(new.start_with_system)
        } else {
            None
        },
        always_on_top: if old.always_on_top != new.always_on_top {
            Some(new.always_on_top)
        } else {
            None
        },
        overlay_corner: if old.overlay_corner != new.overlay_corner {
            Some(new.overlay_corner)
        } else {
            None
        },
        overlay_x: if old.overlay_x != new.overlay_x {
            Some(new.overlay_x)
        } else {
            None
        },
        overlay_y: if old.overlay_y != new.overlay_y {
            Some(new.overlay_y)
        } else {
            None
        },
        compact_size: if old.compact_size != new.compact_size {
            Some(new.compact_size)
        } else {
            None
        },
        expanded_size: if old.expanded_size != new.expanded_size {
            Some(new.expanded_size)
        } else {
            None
        },
        overlay_opacity: if (old.overlay_opacity - new.overlay_opacity).abs() > 0.0001 {
            Some(new.overlay_opacity)
        } else {
            None
        },
        token_text_size: if (old.token_text_size - new.token_text_size).abs() > 0.0001 {
            Some(new.token_text_size)
        } else {
            None
        },
        compact_mode: if old.compact_mode != new.compact_mode {
            Some(new.compact_mode)
        } else {
            None
        },
        show_overlay_in_taskbar: if old.show_overlay_in_taskbar != new.show_overlay_in_taskbar {
            Some(new.show_overlay_in_taskbar)
        } else {
            None
        },
        polling_interval_secs: if old.polling_interval_secs != new.polling_interval_secs {
            Some(new.polling_interval_secs)
        } else {
            None
        },
        enabled_agents: if old.enabled_agents != new.enabled_agents {
            Some(new.enabled_agents.clone())
        } else {
            None
        },
        project_detection: if old.project_detection != new.project_detection {
            Some(new.project_detection)
        } else {
            None
        },
        paused: if old.paused != new.paused {
            Some(new.paused)
        } else {
            None
        },
        theme: if old.theme != new.theme {
            Some(new.theme)
        } else {
            None
        },
        animation_intensity: if old.animation_intensity != new.animation_intensity {
            Some(new.animation_intensity)
        } else {
            None
        },
        show_input: if old.show_input != new.show_input {
            Some(new.show_input)
        } else {
            None
        },
        show_output: if old.show_output != new.show_output {
            Some(new.show_output)
        } else {
            None
        },
        show_cached: if old.show_cached != new.show_cached {
            Some(new.show_cached)
        } else {
            None
        },
        count_cached_in_total: if old.count_cached_in_total != new.count_cached_in_total {
            Some(new.count_cached_in_total)
        } else {
            None
        },
        week_starts_on: if old.week_starts_on != new.week_starts_on {
            Some(new.week_starts_on)
        } else {
            None
        },
        period: if old.period != new.period {
            Some(new.period)
        } else {
            None
        },
        database_location: if old.database_location != new.database_location {
            Some(new.database_location.clone())
        } else {
            None
        },
        retention_days: if old.retention_days != new.retention_days {
            Some(new.retention_days)
        } else {
            None
        },
        shortcut_toggle: if old.shortcut_toggle != new.shortcut_toggle {
            Some(new.shortcut_toggle.clone())
        } else {
            None
        },
        shortcut_expand: if old.shortcut_expand != new.shortcut_expand {
            Some(new.shortcut_expand.clone())
        } else {
            None
        },
        shortcut_focus: if old.shortcut_focus != new.shortcut_focus {
            Some(new.shortcut_focus.clone())
        } else {
            None
        },
    }
}

// ---------------------------------------------------------------------------
// Pure Validation and Clamping
// ---------------------------------------------------------------------------

/// Clamps token text size to 0.6..=2.0 (fallback 1.0 if NaN/infinite).
pub fn clamp_token_text_size(val: f32) -> f32 {
    if val.is_finite() {
        val.clamp(0.6, 2.0)
    } else {
        1.0
    }
}

/// Clamps overlay opacity to 0.0..=1.0 (fallback 0.0 if NaN/infinite).
pub fn clamp_overlay_opacity(val: f32) -> f32 {
    if val.is_finite() {
        val.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Clamps polling interval to 5..=3600 seconds.
/// - Minimum 5s avoids thrashing disk I/O and burning CPU with continuous SQLite queries.
/// - Maximum 3600s (1 hour) ensures periodic recovery within a sensible monitoring timeframe.
pub fn clamp_polling_interval_secs(val: u32) -> u32 {
    val.clamp(5, 3600)
}

/// Validates week start day: 0 (Sunday) to 6 (Saturday). Defaults to 1 (Monday) if invalid.
pub fn validate_week_starts_on(val: u8) -> u8 {
    if val <= 6 {
        val
    } else {
        1
    }
}

/// Filters retention days: must be >= 1 day, otherwise None (keep forever).
pub fn validate_retention_days(val: Option<u32>) -> Option<u32> {
    val.filter(|&d| d >= 1)
}

/// Validates and repairs an entire `view::Settings` instance against safety bounds.
pub fn validate_settings(mut s: view::Settings) -> view::Settings {
    s.token_text_size = clamp_token_text_size(s.token_text_size);
    s.overlay_opacity = clamp_overlay_opacity(s.overlay_opacity);
    s.polling_interval_secs = clamp_polling_interval_secs(s.polling_interval_secs);
    s.week_starts_on = validate_week_starts_on(s.week_starts_on);
    s.retention_days = validate_retention_days(s.retention_days);
    if let Some((w, h)) = s.compact_size {
        s.compact_size = Some((w.clamp(160, 4000), h.clamp(80, 4000)));
    }
    if let Some((w, h)) = s.expanded_size {
        s.expanded_size = Some((w.clamp(160, 4000), h.clamp(80, 4000)));
    }
    s.database_location = s.database_location.filter(|l| !l.trim().is_empty());
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    #[test]
    fn diff_identical_is_empty() {
        let s = mock::settings();
        let patch = diff(&s, &s);
        assert!(patch.is_empty());
    }

    #[test]
    fn diff_captures_only_changes() {
        let s1 = mock::settings();
        let mut s2 = s1.clone();
        s2.always_on_top = !s1.always_on_top;
        s2.theme = Theme::Light;
        s2.polling_interval_secs = 60;

        let patch = diff(&s1, &s2);
        assert!(!patch.is_empty());
        assert_eq!(patch.always_on_top, Some(s2.always_on_top));
        assert_eq!(patch.theme, Some(Theme::Light));
        assert_eq!(patch.polling_interval_secs, Some(60));
        assert_eq!(patch.compact_mode, None);
        assert_eq!(patch.token_text_size, None);
    }

    #[test]
    fn patch_apply_to_updates_target() {
        let mut s = mock::settings();
        let patch = SettingsPatch {
            always_on_top: Some(false),
            theme: Some(Theme::Light),
            token_text_size: Some(1.5),
            ..Default::default()
        };
        patch.apply_to(&mut s);
        assert!(!s.always_on_top);
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.token_text_size, 1.5);
    }

    #[test]
    fn validation_clamps_values() {
        assert_eq!(clamp_token_text_size(0.1), 0.6);
        assert_eq!(clamp_token_text_size(3.0), 2.0);
        assert_eq!(clamp_token_text_size(f32::NAN), 1.0);

        assert_eq!(clamp_overlay_opacity(-0.5), 0.0);
        assert_eq!(clamp_overlay_opacity(1.5), 1.0);
        assert_eq!(clamp_overlay_opacity(f32::INFINITY), 0.0);

        assert_eq!(clamp_polling_interval_secs(1), 5);
        assert_eq!(clamp_polling_interval_secs(10_000), 3600);
        assert_eq!(clamp_polling_interval_secs(30), 30);

        assert_eq!(validate_week_starts_on(0), 0);
        assert_eq!(validate_week_starts_on(6), 6);
        assert_eq!(validate_week_starts_on(7), 1);
        assert_eq!(validate_week_starts_on(255), 1);

        assert_eq!(validate_retention_days(Some(0)), None);
        assert_eq!(validate_retention_days(Some(30)), Some(30));
        assert_eq!(validate_retention_days(None), None);
    }
}
