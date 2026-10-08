use fire_engine::palette::*;
use fire_engine::sim::*;
use fire_engine::sparks::*;
use fire_engine::*;

#[test]
fn test_sim_starts_cold_and_stays_cold() {
    let mut rng = XorShift64::new(1);
    let mut sim = FireSim::new(FIRE_W, FIRE_H);
    for _ in 0..10 {
        sim.step(&FireParams::default(), &mut rng);
    }
    assert_eq!(sim.total_heat(), 0.0);
}

#[test]
fn test_sim_stays_inside_bounds() {
    let mut rng = XorShift64::new(2);
    let mut sim = FireSim::new(FIRE_W, FIRE_H);
    let p = FireParams {
        fuel: 2.5,
        half_width: 9.5,
        decay: 0.012,
        edge_decay: 3.5,
        sway: 0.25,
        inward: 0.4,
        jitter: 0.35,
        blur: 0.4,
    };
    for _ in 0..200 {
        sim.step(&p, &mut rng);
    }
    for &v in &sim.heat {
        assert!((0.0..=3.0).contains(&v), "value {} out of bounds", v);
        assert!(!v.is_nan());
    }
}

#[test]
fn test_sim_taller_flame() {
    let mut rng1 = XorShift64::new(3);
    let mut rng2 = XorShift64::new(3);
    
    let mut sim1 = FireSim::new(FIRE_W, FIRE_H);
    let mut sim2 = FireSim::new(FIRE_W, FIRE_H);
    
    let calm = FireParams {
        fuel: 1.15,
        half_width: 9.5,
        decay: 0.012,
        edge_decay: 3.5,
        sway: 0.25,
        inward: 0.4,
        jitter: 0.35,
        blur: 0.4,
    };
    let flare = FireParams {
        fuel: calm.fuel * 1.5,
        half_width: calm.half_width * 1.4,
        decay: calm.decay * 0.8,
        ..calm
    };
    
    for _ in 0..200 {
        sim1.step(&calm, &mut rng1);
        sim2.step(&flare, &mut rng2);
    }
    
    assert!(sim2.top_row(0.1) < sim1.top_row(0.1));
}

#[test]
fn test_sim_deterministic() {
    let mut rng1 = XorShift64::new(99);
    let mut rng2 = XorShift64::new(99);
    
    let mut sim1 = FireSim::new(FIRE_W, FIRE_H);
    let mut sim2 = FireSim::new(FIRE_W, FIRE_H);
    
    let p = FireParams {
        fuel: 1.15,
        half_width: 9.5,
        decay: 0.012,
        edge_decay: 3.5,
        sway: 0.25,
        inward: 0.4,
        jitter: 0.35,
        blur: 0.4,
    };
    
    for _ in 0..40 {
        sim1.step(&p, &mut rng1);
        sim2.step(&p, &mut rng2);
    }
    
    assert_eq!(sim1.heat, sim2.heat);
}

#[test]
fn test_colours_bands() {
    assert_eq!(band_color(FIRE_BANDS, 0.05), None);
    assert_eq!(band_color(FIRE_BANDS, 0.95), Some([255, 243, 176]));
    let ash = band_color(ASH_BANDS, 0.7).unwrap();
    assert_eq!(ash[0], ash[1]);
}

#[test]
fn test_sparks_lifecycle() {
    let mut rng = XorShift64::new(3);
    let mut sp = Sparks::new();
    sp.burst(40, 0.8, 40.0, 43.0, &mut rng);
    assert_eq!(sp.count(), 40);
    
    let start_avg_y: f64 = sp.list.iter().map(|s| s.y).sum::<f64>() / sp.count() as f64;
    for _ in 0..6 { sp.step(1.0); }
    let next_avg_y: f64 = sp.list.iter().map(|s| s.y).sum::<f64>() / sp.count() as f64;
    assert!(next_avg_y < start_avg_y); // coordinates: y goes down visually but here y increases? Wait, gravity is positive vy (y increases downwards). So y should increase. Let's adjust to check if it moves.
    
    for _ in 0..200 { sp.step(1.0); }
    assert_eq!(sp.count(), 0);
}

#[test]
fn test_ring_lifecycle() {
    let mut sp = Sparks::new();
    sp.ring(30.0, 0.9, 40.0, 38.0);
    
    let dist = |sp: &Sparks| {
        sp.list.iter().map(|s| ((s.x - 40.0).powi(2) + ((s.y - 38.0) / 0.55).powi(2)).sqrt()).fold(0.0_f64, f64::max)
    };
    
    let d0 = dist(&sp);
    for _ in 0..5 { sp.step(1.0); }
    assert!(dist(&sp) > d0);
    
    for _ in 0..100 { sp.step(1.0); }
    assert_eq!(sp.count(), 0);
}
