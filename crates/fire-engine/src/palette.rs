pub type Rgb = [u8; 3];

pub const FIRE_BANDS: &[(f32, Rgb)] = &[
    (0.1, [138, 26, 5]),
    (0.22, [194, 38, 12]),
    (0.36, [255, 77, 0]),
    (0.52, [255, 106, 0]),
    (0.68, [255, 160, 0]),
    (0.82, [255, 196, 0]),
    (0.92, [255, 243, 176]),
];

pub const ASH_BANDS: &[(f32, Rgb)] = &[
    (0.1, [58, 58, 54]),
    (0.22, [78, 78, 72]),
    (0.36, [100, 100, 94]),
    (0.52, [124, 124, 116]),
    (0.68, [150, 150, 142]),
    (0.82, [178, 178, 170]),
    (0.92, [210, 210, 202]),
];

pub fn band_color(bands: &[(f32, Rgb)], heat: f32) -> Option<Rgb> {
    let mut out = None;
    for &(t, c) in bands {
        if heat >= t {
            out = Some(c);
        } else {
            break;
        }
    }
    out
}

const SPARK_COLORS: &[(f64, Rgb)] = &[
    (0.7, [255, 243, 176]),
    (0.45, [255, 196, 0]),
    (0.22, [255, 106, 0]),
    (0.0, [194, 38, 12]),
];

pub fn spark_color(life_fraction: f64) -> Rgb {
    for &(t, c) in SPARK_COLORS {
        if life_fraction >= t {
            return c;
        }
    }
    SPARK_COLORS.last().unwrap().1
}
