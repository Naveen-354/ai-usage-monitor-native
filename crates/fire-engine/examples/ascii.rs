use fire_engine::engine::{FireEngine, FlameState};
use fire_engine::XorShift64;

fn main() {
    let mut rng = XorShift64::new(42);
    let mut engine = FireEngine::new();
    engine.set_state(FlameState::Live, 1.0, &mut rng);
    
    // Animate for 100 frames
    for i in 0..100 {
        engine.tick(83.0, &mut rng);
        
        // Render ASCII
        let chars = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
        let mut s = String::new();
        s.push_str("\x1B[2J\x1B[1;1H"); // Clear screen
        s.push_str(&format!("Frame {}\n", i));
        
        for y in 0..engine.sim.h {
            for x in 0..engine.sim.w {
                let v = engine.sim.heat[y * engine.sim.w + x];
                let idx = (v * 3.0).clamp(0.0, 9.0) as usize;
                s.push(chars[idx]);
            }
            s.push('\n');
        }
        println!("{}", s);
        std::thread::sleep(std::time::Duration::from_millis(83));
    }
}
