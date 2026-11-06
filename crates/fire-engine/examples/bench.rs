//! What each animation costs: `cargo run --release -p fire-engine --example bench`.
//! Prints, per style and quality, the memory the drawing holds and the time to tick + paint one frame.

use std::time::Instant;

use fire_engine::engine::FlameState;
use fire_engine::{create, Quality, Style, XorShift64};

fn main() {
    println!("{:<10} {:<7} {:>9} {:>11} {:>10} {:>9}", "style", "quality", "canvas px", "memory KB", "us/frame", "fps");
    for style in Style::ALL {
        for q in Quality::ALL {
            let mut rng = XorShift64::new(1);
            let mut a = create(style, q);
            let (w, h) = a.dims();
            let mut buf = vec![0u8; w * h * 4];
            a.set_state(FlameState::Live, 1.0, &mut rng);
            for _ in 0..50 {
                a.tick(a.frame_ms(), &mut rng);
                a.paint(&mut buf);
            }
            let frames = 2000;
            let t0 = Instant::now();
            for i in 0..frames {
                if i % 200 == 0 {
                    a.blast(1_000_000.0, &mut rng);
                }
                a.tick(a.frame_ms(), &mut rng);
                a.paint(&mut buf);
            }
            let us = t0.elapsed().as_secs_f64() * 1e6 / frames as f64;
            // what is held while it runs: the canvas, plus the RGBA scratch the app keeps for the texture upload
            let kb = (a.bytes() + buf.len()) as f64 / 1024.0;
            println!("{:<10} {:<7} {:>9} {:>11.1} {:>10.1} {:>9.0}", format!("{style:?}"), format!("{q:?}"), w * h, kb, us, 1000.0 / a.frame_ms());
            if style == Style::Fire {
                break; // the flame ignores quality
            }
        }
    }
}
