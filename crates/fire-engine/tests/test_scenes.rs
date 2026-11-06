//! What every animation style must do, whatever it draws. A scene that fails one of these is not finished.

use fire_engine::engine::FlameState;
use fire_engine::{create, Animation, Quality, Style, XorShift64};

fn frame(a: &mut dyn Animation) -> Vec<u8> {
    let (w, h) = a.dims();
    let mut b = vec![0u8; w * h * 4];
    a.paint(&mut b);
    b
}

fn live(style: Style, q: Quality) -> (Box<dyn Animation>, XorShift64) {
    let mut rng = XorShift64::new(7);
    let mut a = create(style, q);
    a.set_state(FlameState::Live, 1.0, &mut rng);
    (a, rng)
}

fn scenes() -> impl Iterator<Item = Style> {
    Style::ALL.into_iter().filter(|s| *s != Style::Fire)
}

fn covered(b: &[u8]) -> f32 {
    b.chunks_exact(4).filter(|p| p[3] != 0).count() as f32 / (b.len() / 4) as f32
}

#[test]
fn the_canvas_size_follows_the_quality_and_stays_small() {
    for s in scenes() {
        for (q, (w, h)) in [(Quality::Low, (80, 60)), (Quality::Medium, (160, 120)), (Quality::High, (240, 180))] {
            let a = create(s, q);
            assert_eq!(a.dims(), (w, h), "{s:?} {q:?}");
            // canvas bytes + the RGBA scratch the app keeps: under 220 KB even at the sharpest quality
            assert!(a.bytes() + w * h * 4 <= 220_000, "{s:?} {q:?} holds {} bytes", a.bytes() + w * h * 4);
        }
    }
}

#[test]
fn the_classic_flame_ignores_quality() {
    for q in Quality::ALL {
        assert_eq!(create(Style::Fire, q).dims(), (80, 60));
    }
}

#[test]
fn a_live_scene_draws_a_visible_picture_in_the_lower_part_of_the_canvas() {
    for s in scenes() {
        for q in Quality::ALL {
            let (mut a, mut rng) = live(s, q);
            for _ in 0..20 {
                a.tick(83.0, &mut rng);
            }
            let b = frame(a.as_mut());
            let c = covered(&b);
            assert!((0.03..0.70).contains(&c), "{s:?} {q:?} covers {:.1} % of the canvas", c * 100.0);
            // the top quarter is cut off in the expanded overlay: the subject lives below it
            let (w, h) = a.dims();
            let top = &b[..w * (h / 4) * 4];
            assert!(covered(top) < 0.25, "{s:?} {q:?} puts too much in the top quarter ({:.0} %)", covered(top) * 100.0);
        }
    }
}

#[test]
fn a_live_scene_moves_and_a_still_one_does_not() {
    for s in scenes() {
        let (mut a, mut rng) = live(s, Quality::Medium);
        let first = frame(a.as_mut());
        let mut distinct = 1;
        let mut prev = first.clone();
        for _ in 0..30 {
            a.tick(83.0, &mut rng);
            let f = frame(a.as_mut());
            if f != prev {
                distinct += 1;
            }
            prev = f;
        }
        assert!(distinct >= 15, "{s:?} only changed on {distinct} of 31 frames: that is not an animation");
        assert!(a.needs_loop() && a.next_repaint().is_some());
        assert_eq!(frame(a.as_mut()), frame(a.as_mut()), "{s:?}: painting twice without a tick must give the same picture");
    }
}

#[test]
fn idle_and_off_are_still_poses_that_never_wake_the_app() {
    for s in scenes() {
        let mut rng = XorShift64::new(3);
        for mode in [FlameState::Idle, FlameState::Off] {
            let mut a = create(s, Quality::High);
            a.set_state(mode, 1.0, &mut rng);
            assert!(!a.needs_loop(), "{s:?} {mode:?}");
            assert_eq!(a.next_repaint(), None, "{s:?} {mode:?}");
            let b = frame(a.as_mut());
            assert!(covered(&b) > 0.03, "{s:?} {mode:?} must still show its pose");
        }
    }
}

#[test]
fn off_is_greyscale_so_it_never_suggests_activity() {
    for s in scenes() {
        let mut rng = XorShift64::new(3);
        let mut a = create(s, Quality::Medium);
        a.set_state(FlameState::Off, 1.0, &mut rng);
        for p in frame(a.as_mut()).chunks_exact(4).filter(|p| p[3] != 0) {
            assert!(p[0] == p[1] && p[1] == p[2], "{s:?}: an OFF pixel is coloured: {p:?}");
        }
        assert!(a.blast(500_000.0, &mut rng).is_none(), "{s:?}: an OFF animation must not react to a surge");
    }
}

#[test]
fn zero_motion_is_a_still_frame_even_when_live() {
    for s in scenes() {
        let mut rng = XorShift64::new(3);
        let mut a = create(s, Quality::Medium);
        a.set_state(FlameState::Live, 0.0, &mut rng);
        assert!(!a.needs_loop(), "{s:?}");
        assert!(a.blast(500_000.0, &mut rng).is_none(), "{s:?}");
        assert!(covered(&frame(a.as_mut())) > 0.03);
    }
}

#[test]
fn a_surge_visibly_changes_the_picture_then_it_settles() {
    for s in scenes() {
        let mut rng = XorShift64::new(11);
        let mut a = create(s, Quality::High);
        a.set_state(FlameState::Idle, 1.0, &mut rng);
        let still = frame(a.as_mut());
        let b = a.blast(20_000_000.0, &mut rng).expect("a surge on a working animation");
        assert!(b.magnitude > 0.9);
        assert!(a.needs_loop(), "{s:?}: a surge must wake the loop even from idle");
        let mut changed = false;
        for _ in 0..12 {
            a.tick(83.0, &mut rng);
            changed |= frame(a.as_mut()) != still;
        }
        assert!(changed, "{s:?}: a huge surge left the picture untouched");
        for _ in 0..200 {
            a.tick(50.0, &mut rng);
        }
        assert!(!a.needs_loop(), "{s:?}: the loop must stop again when the surge is over");
    }
}

#[test]
fn quality_changes_how_it_is_drawn_not_what_it_is() {
    for s in scenes() {
        let (mut lo, mut r1) = live(s, Quality::Low);
        let (mut hi, mut r2) = live(s, Quality::High);
        for _ in 0..10 {
            lo.tick(83.0, &mut r1);
            hi.tick(83.0, &mut r2);
        }
        let (cl, ch) = (covered(&frame(lo.as_mut())), covered(&frame(hi.as_mut())));
        assert!((cl - ch).abs() < 0.12, "{s:?}: low covers {cl:.2}, high {ch:.2}: the same scene should fill about the same area");
        assert!(hi.frame_ms() < lo.frame_ms(), "higher quality runs at a higher frame rate");
    }
}

#[test]
fn the_same_seed_gives_the_same_pictures() {
    for s in scenes() {
        let run = || {
            let (mut a, mut rng) = live(s, Quality::Medium);
            a.blast(300_000.0, &mut rng);
            for _ in 0..25 {
                a.tick(83.0, &mut rng);
            }
            frame(a.as_mut())
        };
        assert_eq!(run(), run(), "{s:?} is not deterministic");
    }
}

fn agents(n: u8) -> fire_engine::scene::Data {
    use fire_engine::canvas::pal;
    use fire_engine::scene::{AgentBar, Data};
    let cols = [pal::ORANGE, pal::GREEN, pal::BLUE, pal::PURPLE, pal::HOTPINK, pal::YELLOW, pal::CYAN, pal::RED];
    let mut d = Data { n_agents: n, ..Data::default() };
    for i in 0..n as usize {
        d.agents[i] = AgentBar { live: i % 2 == 0, share: 1.0 - i as f32 * 0.11, col: cols[i] };
    }
    d
}

#[test]
fn the_data_scenes_draw_the_days_numbers() {
    use fire_engine::scene::Data;
    let paint = |style: Style, mode: FlameState, d: Data| {
        let mut rng = XorShift64::new(7);
        let mut a = create(style, Quality::Medium);
        a.set_data(&d);
        a.set_state(mode, 1.0, &mut rng);
        if mode == FlameState::Live {
            for _ in 0..6 {
                a.tick(83.0, &mut rng);
            }
        }
        frame(a.as_mut())
    };
    for mode in [FlameState::Live, FlameState::Idle] {
        // the tank is as full as the day is
        let (empty, full) = (Data { level: 0.05, ..Data::default() }, Data { level: 0.95, ..Data::default() });
        assert_ne!(paint(Style::Tank, mode, empty), paint(Style::Tank, mode, full), "tank {mode:?}: the level is not drawn");
        // the pet has evolved further and the receipt has more paper out on a fuller day
        for style in [Style::Pet, Style::Receipt] {
            assert_ne!(paint(style, mode, empty), paint(style, mode, full), "{style:?} {mode:?}: the day's level is not drawn");
        }
        // the equalizer has one bar per agent, in that agent's colour
        assert_ne!(paint(Style::Equalizer, mode, agents(2)), paint(Style::Equalizer, mode, agents(6)), "equalizer {mode:?}: the agents are not drawn");
        let mut recoloured = agents(4);
        recoloured.agents[0].col = fire_engine::canvas::pal::RED;
        assert_ne!(paint(Style::Equalizer, mode, agents(4)), paint(Style::Equalizer, mode, recoloured), "equalizer {mode:?}: the agent colours are not drawn");
    }
}

#[test]
fn the_pulse_follows_the_recent_surges() {
    // after a few surges the heartbeat is busier than the same moment with none
    let run = |style: Style, surges: usize| {
        let mut rng = XorShift64::new(7);
        let mut a = create(style, Quality::Medium);
        a.set_state(FlameState::Live, 1.0, &mut rng);
        for _ in 0..surges {
            a.blast(5_000_000.0, &mut rng);
            for _ in 0..3 {
                a.tick(83.0, &mut rng);
            }
        }
        for _ in 0..(12 - surges * 3) {
            a.tick(83.0, &mut rng);
        }
        frame(a.as_mut())
    };
    for style in [Style::Heartbeat, Style::Receipt] {
        assert_ne!(run(style, 0), run(style, 3), "{style:?} ignores the token rate");
    }
}

#[test]
fn low_intensity_runs_slower_than_normal() {
    for s in scenes() {
        let mut rng = XorShift64::new(3);
        let mut a = create(s, Quality::Medium);
        a.set_state(FlameState::Live, 1.0, &mut rng);
        let normal = a.frame_ms();
        a.set_state(FlameState::Live, 0.5, &mut rng);
        assert!(a.frame_ms() > normal, "{s:?}");
    }
}
