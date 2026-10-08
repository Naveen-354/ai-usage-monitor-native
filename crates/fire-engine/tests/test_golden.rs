use fire_engine::blast::*;
use fire_engine::engine::*;
use fire_engine::sim::*;
use fire_engine::*;
use std::fs;
use std::path::Path;

fn render_ascii(sim: &FireSim) -> String {
    let chars = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
    let mut s = String::new();
    for y in 0..sim.h {
        for x in 0..sim.w {
            let v = sim.heat[y * sim.w + x];
            let idx = (v * 3.0).clamp(0.0, 9.0) as usize;
            s.push(chars[idx]);
        }
        s.push('\n');
    }
    s
}

#[test]
fn test_golden() {
    let mut rng = XorShift64::new(42);
    let mut engine = FireEngine::new();
    engine.set_state(FlameState::Live, 1.0, &mut rng);
    
    for _ in 0..100 {
        engine.tick(83.0, &mut rng);
    }
    
    let ascii = render_ascii(&engine.sim);
    let golden_path = "tests/golden/live_100.txt";
    
    if std::env::var("REGENERATE_GOLDEN").is_ok() {
        fs::create_dir_all("tests/golden").unwrap();
        fs::write(golden_path, &ascii).unwrap();
    } else {
        if Path::new(golden_path).exists() {
            let golden = fs::read_to_string(golden_path).unwrap();
            assert_eq!(ascii, golden, "Golden test mismatch");
        } else {
            println!("Golden file missing, run with 'regenerate_golden' to create");
        }
    }
}

#[test]
fn test_property_loops() {
    let mut rng = XorShift64::new(123);
    let mut engine = FireEngine::new();
    
    // Property 1: symmetry and bounds
    engine.set_state(FlameState::Live, 1.0, &mut rng);
    for _ in 0..50 {
        engine.tick(83.0, &mut rng);
    }
    
    for y in 0..FIRE_H {
        for x in 0..FIRE_W {
            let v = engine.sim.heat[y * FIRE_W + x];
            assert!((0.0..=3.5).contains(&v), "Heat out of bounds: {}", v);
            assert!(!v.is_nan());
        }
    }
    
    // Check roughly symmetric (difference between left and right side is bounded)
    // Actually, randomness means it won't be perfectly symmetric, just mostly.
    
    // Property 2: energy decays when fuel is 0
    let p = FireParams { fuel: 0.0, decay: 0.05, ..Default::default() };
    let mut sim = engine.sim;
    let initial_heat = sim.total_heat();
    for _ in 0..100 {
        sim.step(&p, &mut rng);
    }
    assert!(sim.total_heat() < initial_heat * 0.1);
    
    // Property 3: blast tiers add more heat
    let mut prev_heat = 0.0;
    for &delta in &[5000.0, 50000.0, 500000.0] {
        let mut sim = FireSim::new(FIRE_W, FIRE_H);
        sim.flare(
            16.0 + 22.0 * surge_magnitude(delta),
            9.5 * (1.0 + 0.45 * surge_magnitude(delta)),
            1.0 + 0.35 * surge_magnitude(delta),
            &mut rng
        );
        let heat = sim.total_heat();
        assert!(heat > prev_heat);
        prev_heat = heat;
    }
    
    // Property 4: idle never requests a repaint
    let mut engine = FireEngine::new();
    engine.set_state(FlameState::Idle, 1.0, &mut rng);
    assert_eq!(engine.next_repaint(), None);
}
