//! Draws a contact sheet of one style so it can be looked at: `cargo run --example sheet -- hamster medium out.ppm`.
//!
//! Columns: live (three moments), two surges (small, huge), idle, off. Row 1 is on a dark desktop, row 2 on a light
//! one, because the overlay is see-through. Convert for viewing with `python -c "from PIL import Image; Image.open('out.ppm').save('out.png')"`.

use fire_engine::engine::FlameState;
use fire_engine::scene::{AgentBar, Data};
use fire_engine::{create, Animation, Quality, Style, XorShift64};

const CELL_W: usize = 240;
const CELL_H: usize = 180;

fn style_of(name: &str) -> Style {
    match name {
        "fire" => Style::Fire,
        "hamster" => Style::Hamster,
        "duck" => Style::Duck,
        "slots" => Style::Slots,
        "pig" => Style::Pig,
        "cat" => Style::Cat,
        "muncher" => Style::Muncher,
        "spitter" => Style::Spitter,
        "chicken" => Style::Chicken,
        "snail" => Style::Snail,
        "panda" => Style::Panda,
        "octopus" => Style::Octopus,
        "gum" => Style::Gum,
        "heartbeat" => Style::Heartbeat,
        "tank" => Style::Tank,
        "equalizer" => Style::Equalizer,
        "warp" => Style::Warp,
        "shoggoth" => Style::Shoggoth,
        "receipt" => Style::Receipt,
        "pet" => Style::Pet,
        "paperclip" => Style::Paperclip,
        other => panic!("unknown style {other}"),
    }
}

/// A made-up day for the scenes that draw data: a fairly full day and six agents (two of them working right now).
fn demo_data() -> Data {
    use fire_engine::canvas::pal;
    let cols = [pal::ORANGE, pal::GREEN, pal::BLUE, pal::PURPLE, pal::HOTPINK, pal::YELLOW];
    let shares = [1.0, 0.7, 0.45, 0.3, 0.15, 0.06];
    let mut d = Data { level: 0.62, n_agents: 6, ..Data::default() };
    for i in 0..6 {
        d.agents[i] = AgentBar { live: i == 0 || i == 2, share: shares[i], col: cols[i] };
    }
    d
}

fn quality_of(name: &str) -> Quality {
    match name {
        "low" => Quality::Low,
        "medium" => Quality::Medium,
        "high" => Quality::High,
        other => panic!("unknown quality {other}"),
    }
}

fn grab(a: &mut dyn Animation) -> (usize, usize, Vec<u8>) {
    let (w, h) = a.dims();
    let mut b = vec![0; w * h * 4];
    a.paint(&mut b);
    (w, h, b)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (style, quality, out) = (style_of(&args[1]), quality_of(&args[2]), &args[3]);
    let mut cells: Vec<(usize, usize, Vec<u8>)> = Vec::new();

    let mut rng = XorShift64::new(42);
    let data = demo_data();
    let mut a = create(style, quality);
    a.set_data(&data);
    a.set_state(FlameState::Live, 1.0, &mut rng);
    for step in 0..3 {
        for _ in 0..(if step == 0 { 12 } else { 5 }) {
            a.tick(a.frame_ms(), &mut rng);
        }
        cells.push(grab(a.as_mut()));
    }
    for delta in [20_000.0, 20_000_000.0] {
        let mut a = create(style, quality);
        a.set_data(&data);
        a.set_state(FlameState::Live, 1.0, &mut rng);
        for _ in 0..8 {
            a.tick(a.frame_ms(), &mut rng);
        }
        a.blast(delta, &mut rng);
        for _ in 0..4 {
            a.tick(a.frame_ms(), &mut rng);
        }
        cells.push(grab(a.as_mut()));
    }
    for mode in [FlameState::Idle, FlameState::Off] {
        let mut a = create(style, quality);
        a.set_data(&data);
        a.set_state(mode, 1.0, &mut rng);
        cells.push(grab(a.as_mut()));
    }

    let (cols, rows, gap) = (cells.len(), 2, 6);
    let (sw, sh) = (cols * (CELL_W + gap) + gap, rows * (CELL_H + gap) + gap);
    let mut img = vec![0u8; sw * sh * 3];
    for (row, bg) in [[30u8, 28, 34], [226, 224, 220]].into_iter().enumerate() {
        for (col, (w, h, px)) in cells.iter().enumerate() {
            let (ox, oy) = (gap + col * (CELL_W + gap), gap + row * (CELL_H + gap));
            for y in 0..CELL_H {
                for x in 0..CELL_W {
                    let (sx, sy) = (x * w / CELL_W, y * h / CELL_H);
                    let p = &px[(sy * w + sx) * 4..][..4];
                    let al = p[3] as u32;
                    let o = ((oy + y) * sw + ox + x) * 3;
                    for k in 0..3 {
                        img[o + k] = ((p[k] as u32 * al + bg[k] as u32 * (255 - al)) / 255) as u8;
                    }
                }
            }
        }
    }
    let mut file = format!("P6\n{sw} {sh}\n255\n").into_bytes();
    file.extend_from_slice(&img);
    std::fs::write(out, file).expect("write the sheet");
    println!("wrote {out} ({sw}x{sh})");
}
