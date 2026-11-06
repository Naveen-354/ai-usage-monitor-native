//! Pure decisions about the flame, kept out of the egui code so they are unit-tested.
//! The flame must never suggest activity that is not there.

use app_api::view::{AnimationIntensity, AnimationQuality, AnimationStyle, Overview};
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

/// The day's numbers for the scenes that draw them: how full the day is, and one bar per enabled agent that has numbers.
/// Nothing is invented: no overview, no numbers, an empty `Data`.
pub fn scene_data(overview: Option<&Overview>, now_ms: i64) -> fire_engine::scene::Data {
    use fire_engine::canvas::pal;
    use fire_engine::scene::{AgentBar, Data, MAX_AGENTS};
    let Some(o) = overview else { return Data::default() };
    let mut d = Data::default();
    // a thousand tokens or fewer is an empty tank, a billion or more a full one
    if let Some(t) = o.totals {
        d.level = (((t.total.max(1) as f64).log10() - 3.0) / 6.0).clamp(0.0, 1.0) as f32;
    }
    let with_numbers = || o.agents.iter().filter(|a| a.enabled && a.totals.is_some());
    let biggest = with_numbers().filter_map(|a| a.totals).map(|t| t.total).max().unwrap_or(0).max(1) as f32;
    for (i, a) in with_numbers().take(MAX_AGENTS).enumerate() {
        let tokens = a.totals.map_or(0, |t| t.total) as f32;
        let own = native_ui::web::agent_color(&a.color);
        d.agents[i] = AgentBar {
            live: native_ui::web::hero::is_live(a.last_event_utc_ms, now_ms),
            share: (tokens / biggest).clamp(0.0, 1.0),
            col: pal::nearest_bright([own.r(), own.g(), own.b()]),
        };
        d.n_agents = i as u8 + 1;
    }
    d
}

/// Animation intensity -> the engine's motion factor (0 = still frame only).
pub fn motion_factor(i: AnimationIntensity) -> f64 {
    match i {
        AnimationIntensity::Off => 0.0,
        AnimationIntensity::Low => 0.5,
        AnimationIntensity::Normal => 1.0,
    }
}

/// The setting's animation -> the engine's.
pub fn engine_style(s: AnimationStyle) -> fire_engine::Style {
    use fire_engine::Style as E;
    match s {
        AnimationStyle::Fire => E::Fire,
        AnimationStyle::Hamster => E::Hamster,
        AnimationStyle::Duck => E::Duck,
        AnimationStyle::Slots => E::Slots,
        AnimationStyle::Pig => E::Pig,
        AnimationStyle::Cat => E::Cat,
        AnimationStyle::Muncher => E::Muncher,
        AnimationStyle::Spitter => E::Spitter,
        AnimationStyle::Chicken => E::Chicken,
        AnimationStyle::Snail => E::Snail,
        AnimationStyle::Panda => E::Panda,
        AnimationStyle::Octopus => E::Octopus,
        AnimationStyle::Gum => E::Gum,
        AnimationStyle::Heartbeat => E::Heartbeat,
        AnimationStyle::Tank => E::Tank,
        AnimationStyle::Equalizer => E::Equalizer,
        AnimationStyle::Warp => E::Warp,
        AnimationStyle::Shoggoth => E::Shoggoth,
        AnimationStyle::Receipt => E::Receipt,
        AnimationStyle::Pet => E::Pet,
        AnimationStyle::Paperclip => E::Paperclip,
    }
}

pub fn engine_quality(q: AnimationQuality) -> fire_engine::Quality {
    match q {
        AnimationQuality::Low => fire_engine::Quality::Low,
        AnimationQuality::Medium => fire_engine::Quality::Medium,
        AnimationQuality::High => fire_engine::Quality::High,
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
    fn the_scene_data_is_the_days_real_numbers_and_empty_without_any() {
        assert_eq!(scene_data(None, NOW), fire_engine::scene::Data::default());
        let mut o = mock::overview();
        o.totals = None;
        assert_eq!(scene_data(Some(&o), NOW).level, 0.0, "no numbers, no level");

        let d = scene_data(Some(&mock::overview()), NOW);
        assert!(d.level > 0.5 && d.level <= 1.0, "the demo day is a big one: {}", d.level);
        assert!(d.n_agents >= 2 && d.n_agents as usize <= fire_engine::scene::MAX_AGENTS);
        let shares: Vec<f32> = d.agents[..d.n_agents as usize].iter().map(|a| a.share).collect();
        assert!(shares.iter().all(|s| (0.0..=1.0).contains(s)) && shares.contains(&1.0), "the busiest agent is a full bar: {shares:?}");
        assert!(d.agents[..d.n_agents as usize].iter().all(|a| a.col.0 != 0), "every bar has a colour");
    }

    #[test]
    fn a_disabled_agent_has_no_bar_and_an_agent_that_just_worked_is_live() {
        let mut o = overview_with(Some(10_000), true);
        let n_all = scene_data(Some(&o), NOW).n_agents;
        assert!(scene_data(Some(&o), NOW).agents[0].live);
        o.agents[0].enabled = false;
        assert_eq!(scene_data(Some(&o), NOW).n_agents, n_all - 1);
    }

    #[test]
    fn every_setting_value_maps_to_its_own_engine_value() {
        let styles = [AnimationStyle::Fire, AnimationStyle::Hamster, AnimationStyle::Duck, AnimationStyle::Slots, AnimationStyle::Pig, AnimationStyle::Cat, AnimationStyle::Muncher, AnimationStyle::Spitter, AnimationStyle::Chicken, AnimationStyle::Snail, AnimationStyle::Panda, AnimationStyle::Octopus, AnimationStyle::Gum, AnimationStyle::Heartbeat, AnimationStyle::Tank, AnimationStyle::Equalizer, AnimationStyle::Warp, AnimationStyle::Shoggoth, AnimationStyle::Receipt, AnimationStyle::Pet, AnimationStyle::Paperclip];
        let mapped: Vec<_> = styles.iter().map(|s| engine_style(*s)).collect();
        assert_eq!(mapped, fire_engine::Style::ALL, "same order, one engine style per setting value");
        assert_eq!(engine_quality(AnimationQuality::Low), fire_engine::Quality::Low);
        assert_eq!(engine_quality(AnimationQuality::Medium), fire_engine::Quality::Medium);
        assert_eq!(engine_quality(AnimationQuality::High), fire_engine::Quality::High);
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
