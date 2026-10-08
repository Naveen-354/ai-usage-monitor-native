pub struct Blast {
    pub magnitude: f64,
    pub sparks: usize,
    pub boost: f64,
    pub boost_ms: f64,
    pub ring: bool,
    pub ring_radius: f64,
    pub pop: f64,
    pub label: bool,
}

const LOG_FLOOR: f64 = 2.7;
const LOG_SPAN: f64 = 4.3;

fn clamp01(n: f64) -> f64 {
    n.clamp(0.0, 1.0)
}

pub fn surge_magnitude(delta: f64) -> f64 {
    if delta.is_nan() || delta <= 0.0 {
        return 0.0;
    }
    clamp01((delta.log10() - LOG_FLOOR) / LOG_SPAN)
}

pub fn blast_tier(m: f64) -> &'static str {
    if m < 0.25 {
        "spark"
    } else if m < 0.45 {
        "flare"
    } else if m < 0.7 {
        "blast"
    } else if m < 0.93 {
        "explosion"
    } else {
        "inferno"
    }
}

pub fn blast_params(delta: f64, motion: f64) -> Option<Blast> {
    if motion <= 0.0 || delta.is_nan() || delta <= 0.0 {
        return None;
    }
    let m = surge_magnitude(delta);
    Some(Blast {
        magnitude: m,
        sparks: 2.max(((4.0 + 74.0 * m) * motion).round() as usize),
        boost: (0.15 + 0.85 * m) * 1.0_f64.min(0.4 + motion * 0.6),
        boost_ms: (500.0 + 1900.0 * m) * motion,
        ring: m >= 0.7 && motion >= 1.0,
        ring_radius: (14.0 + 20.0 * m).round(),
        pop: 1.0 + 0.12 * m * motion,
        label: true,
    })
}
