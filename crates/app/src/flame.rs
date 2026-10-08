//! Pure decisions about the flame, kept out of the egui code so they are unit-tested.
//! The flame must never suggest activity that is not there.

use app_api::view::{AnimationIntensity, Overview};
use fire_engine::engine::FlameState;

pub fn flame_state(overview: Option<&Overview>, now_ms: i64) -> FlameState {
    let Some(o) = overview else { return FlameState::Off };
    if o.paused || o.totals.is_none() {
        return FlameState::Off;
    }
    let live = o.agents.iter().any(|a| {
        a.enabled && native_ui::web::hero::is_live(a.last_event_utc_ms, now_ms)
    });
    if live {
        FlameState::Live
    } else {
        FlameState::Idle
    }
}

/// Animation intensity -> the engine's motion factor (0 = still frame only).
pub fn motion_factor(i: AnimationIntensity) -> f64 {
    match i {
        AnimationIntensity::Off => 0.0,
        AnimationIntensity::Low => 0.5,
        AnimationIntensity::Normal => 1.0,
    }
}

/// The period words of the old UI (`PERIOD_LABEL`), shown after "TOKENS".
pub fn period_word(p: app_api::view::PeriodKey) -> &'static str {
    use app_api::view::PeriodKey::*;
    match p {
        Day => "TODAY",
        Week => "THIS WEEK",
        Month => "THIS MONTH",
        Year => "THIS YEAR",
    }
}

/// The count's `pop` animation (`motion.css`): 0 % -> 1, 18 % -> `pop`, 100 % -> 1, over 0.6 s. `t` is the elapsed fraction.
pub fn pop_scale(t: f32, pop: f32) -> f32 {
    if !(0.0..1.0).contains(&t) {
        return 1.0;
    }
    if t < 0.18 {
        1.0 + (pop - 1.0) * (t / 0.18)
    } else {
        pop + (1.0 - pop) * ((t - 0.18) / 0.82)
    }
}

pub fn next_period(p: app_api::view::PeriodKey) -> app_api::view::PeriodKey {
    use app_api::view::PeriodKey::*;
    match p {
        Day => Week,
        Week => Month,
        Month => Year,
        Year => Day,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_ui::web::hero::LIVE_WINDOW_MS;
    use app_api::mock;
    use app_api::view::PeriodKey;

    const NOW: i64 = mock::NOW_MS;

    fn overview_with(last_event_ago_ms: Option<i64>, enabled: bool) -> Overview {
        let mut o = mock::overview();
        for a in &mut o.agents {
            a.last_event_utc_ms = None;
        }
        o.agents[0].last_event_utc_ms = last_event_ago_ms.map(|a| NOW - a);
        o.agents[0].enabled = enabled;
        o
    }

    #[test]
    fn off_without_an_overview_without_numbers_or_when_paused() {
        assert!(flame_state(None, NOW) == FlameState::Off);
        let mut o = mock::overview();
        o.totals = None;
        assert!(flame_state(Some(&o), NOW) == FlameState::Off);
        let mut o = mock::overview();
        o.paused = true;
        assert!(flame_state(Some(&o), NOW) == FlameState::Off);
    }

    #[test]
    fn live_only_when_an_enabled_agent_recorded_usage_in_the_last_minute() {
        assert!(flame_state(Some(&overview_with(Some(10_000), true)), NOW) == FlameState::Live);
        assert!(flame_state(Some(&overview_with(Some(120_000), true)), NOW) == FlameState::Idle);
        assert!(flame_state(Some(&overview_with(None, true)), NOW) == FlameState::Idle);
    }

    #[test]
    fn the_live_window_boundary_is_strict_and_the_future_is_not_live() {
        assert!(flame_state(Some(&overview_with(Some(LIVE_WINDOW_MS - 1), true)), NOW) == FlameState::Live);
        assert!(flame_state(Some(&overview_with(Some(LIVE_WINDOW_MS), true)), NOW) == FlameState::Idle, "exactly 60 s is no longer live");
        assert!(flame_state(Some(&overview_with(Some(-5_000), true)), NOW) == FlameState::Idle, "a clock-skewed future event is not live");
    }

    #[test]
    fn activity_from_a_disabled_agent_is_ignored() {
        assert!(flame_state(Some(&overview_with(Some(1_000), false)), NOW) == FlameState::Idle);
    }

    #[test]
    fn intensity_maps_to_motion() {
        assert_eq!(motion_factor(AnimationIntensity::Off), 0.0);
        assert_eq!(motion_factor(AnimationIntensity::Low), 0.5);
        assert_eq!(motion_factor(AnimationIntensity::Normal), 1.0);
    }

    #[test]
    fn the_period_cycles_day_week_month_year_day_with_matching_labels() {
        let mut p = PeriodKey::Day;
        let mut seen = vec![];
        for _ in 0..5 {
            seen.push(period_word(p));
            p = next_period(p);
        }
        assert_eq!(seen, ["TODAY", "THIS WEEK", "THIS MONTH", "THIS YEAR", "TODAY"]);
    }

    #[test]
    fn the_pop_animation_swells_then_settles_like_the_css_keyframes() {
        assert_eq!(pop_scale(0.0, 1.1), 1.0);
        assert!((pop_scale(0.18, 1.1) - 1.1).abs() < 1e-6, "peak at 18 %");
        assert!((pop_scale(0.09, 1.1) - 1.05).abs() < 1e-6);
        assert!(pop_scale(0.59, 1.1) < 1.1 && pop_scale(0.59, 1.1) > 1.0);
        assert_eq!(pop_scale(1.0, 1.1), 1.0);
        assert_eq!(pop_scale(-0.2, 1.1), 1.0);
        assert_eq!(pop_scale(3.0, 1.1), 1.0, "finished animations rest at 1");
    }
}
