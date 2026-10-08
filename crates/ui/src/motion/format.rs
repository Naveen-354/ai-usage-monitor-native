pub const FALLBACK_PANEL_ALPHA: f32 = 0.004;

pub fn format_compact(n: f64, sig: u32) -> String {
    let v = n.round().max(0.0);
    if v < 1000.0 {
        return v.to_string();
    }
    let units = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
    for &(size, suffix) in &units {
        if v >= size {
            let scaled = v / size;
            let int_digits = scaled.log10().floor() + 1.0;
            let decimals = (sig as f64 - int_digits).max(0.0) as usize;
            
            let mut text = format!("{:.*}", decimals, scaled);
            
            if let Ok(parsed) = text.parse::<f64>() {
                if parsed >= 1000.0 && suffix != "T" {
                    return format_compact(size * 1000.0, sig);
                }
            }
            
            if text.contains('.') {
                text = text.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            return format!("{}{}", text, suffix);
        }
    }
    v.to_string()
}

pub fn format_grouped(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let mut count = 0;
    for c in s.chars().rev() {
        if count == 3 {
            result.push(',');
            count = 0;
        }
        result.push(c);
        count += 1;
    }
    result.chars().rev().collect()
}

pub fn platform_alpha_floor() -> f32 {
    if cfg!(windows) {
        0.0
    } else {
        FALLBACK_PANEL_ALPHA
    }
}

pub fn panel_alpha(opacity: f32, floor: f32) -> f32 {
    let v = if opacity.is_finite() { opacity } else { 0.0 };
    v.max(floor).min(1.0)
}

pub fn token_font_px(scale: f32, digits: usize, container_px: f32) -> f32 {
    // CSS: clamp(14px, min(calc(46px * var(--num-scale, 1)), calc(118cqw / var(--n, 6))), 140px);
    let n = (digits as f32).max(1.0); // prevent division by zero
    let cqw_px = container_px * (118.0 / 100.0) / n;
    let scaled = 46.0 * scale;
    scaled.min(cqw_px).clamp(14.0, 140.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_boundaries() {
        assert_eq!(format_compact(999.0, 3), "999");
        assert_eq!(format_compact(1000.0, 3), "1K");
        // 999_949 scales to 999.949, which rounds to "1000" at 3 significant digits; like the web version
        // (lib/format.ts, "Rounding can reach the next unit") it is promoted to the next unit.
        assert_eq!(format_compact(999_949.0, 3), "1M");
        assert_eq!(format_compact(999_499.0, 3), "999K");
        assert_eq!(format_compact(999_950.0, 3), "1M");
        assert_eq!(format_compact(1_000_000.0, 3), "1M");
        assert_eq!(format_compact(1e12, 3), "1T");
        assert_eq!(format_compact(1e15, 3), "1000T");
        assert_eq!(format_compact(0.0, 3), "0");
        assert_eq!(format_compact(f64::NAN, 3), "0");
        assert_eq!(format_compact(-50.0, 3), "0");
    }

    #[test]
    fn test_format_grouped() {
        assert_eq!(format_grouped(0), "0");
        assert_eq!(format_grouped(999), "999");
        assert_eq!(format_grouped(1000), "1,000");
        assert_eq!(format_grouped(1_823_982), "1,823,982");
    }
}
