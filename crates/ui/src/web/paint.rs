//! Box painting the way CSS does it: inside borders, dashed borders and hard (un-blurred) box shadows. Everything is made
//! of whole-pixel rectangles, so nothing is anti-aliased.

use egui::{Color32, Painter, Pos2, Rect};

/// A CSS border drawn *inside* `rect` (`box-sizing: border-box`).
pub fn border(painter: &Painter, rect: Rect, w: f32, color: Color32) {
    for r in border_bands(rect, w) {
        painter.rect_filled(r, 0.0, color);
    }
}

/// The four rectangles of an inside border: top, bottom, left, right.
pub fn border_bands(rect: Rect, w: f32) -> [Rect; 4] {
    [
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + w)),
        Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - w), rect.max),
        Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + w), Pos2::new(rect.min.x + w, rect.max.y - w)),
        Rect::from_min_max(Pos2::new(rect.max.x - w, rect.min.y + w), Pos2::new(rect.max.x, rect.max.y - w)),
    ]
}

/// Dash positions along a line of `len` px: dashes of `dash` px with the gap stretched so both ends start with a dash,
/// like a browser's `border-style: dashed`. Returns (start, end) pairs.
pub fn dashes(len: f32, dash: f32, gap: f32) -> Vec<(f32, f32)> {
    if len <= dash {
        return vec![(0.0, len.max(0.0))];
    }
    let n = ((len + gap) / (dash + gap)).round().max(2.0);
    let gap = (len - n * dash) / (n - 1.0);
    (0..n as usize).map(|i| (i as f32 * (dash + gap), i as f32 * (dash + gap) + dash)).collect()
}

/// A dashed CSS border inside `rect` (dash = 3 x width, as browsers draw it).
pub fn dashed_border(painter: &Painter, rect: Rect, w: f32, color: Color32) {
    let (dash, gap) = (3.0 * w, 1.5 * w);
    for (a, b) in dashes(rect.width(), dash, gap) {
        painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x + a, rect.min.y), Pos2::new(rect.min.x + b, rect.min.y + w)), 0.0, color);
        painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x + a, rect.max.y - w), Pos2::new(rect.min.x + b, rect.max.y)), 0.0, color);
    }
    for (a, b) in dashes(rect.height(), dash, gap) {
        painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + a), Pos2::new(rect.min.x + w, rect.min.y + b)), 0.0, color);
        painter.rect_filled(Rect::from_min_max(Pos2::new(rect.max.x - w, rect.min.y + a), Pos2::new(rect.max.x, rect.min.y + b)), 0.0, color);
    }
}

/// The visible part of `box-shadow: dx dy 0 color`: the shadow is clipped away under the box itself, so only an
/// L-shaped band to the right of and below the box remains (for positive offsets).
pub fn hard_shadow_bands(rect: Rect, dx: f32, dy: f32) -> [Rect; 2] {
    let s = rect.translate(egui::vec2(dx, dy));
    [Rect::from_min_max(Pos2::new(rect.max.x, s.min.y), s.max), Rect::from_min_max(Pos2::new(s.min.x, rect.max.y), Pos2::new(rect.max.x, s.max.y))]
}

pub fn hard_shadow(painter: &Painter, rect: Rect, dx: f32, dy: f32, color: Color32) {
    for r in hard_shadow_bands(rect, dx, dy) {
        painter.rect_filled(r, 0.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn area(r: Rect) -> f32 {
        r.width().max(0.0) * r.height().max(0.0)
    }

    #[test]
    fn an_inside_border_covers_exactly_the_frame_and_nothing_inside() {
        let r = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(100.0, 40.0));
        let bands = border_bands(r, 2.0);
        let covered: f32 = bands.iter().map(|b| area(*b)).sum();
        assert_eq!(covered, 100.0 * 40.0 - 96.0 * 36.0, "frame area");
        for (i, a) in bands.iter().enumerate() {
            assert!(r.contains_rect(*a), "band {i} escapes the box");
            for b in bands.iter().skip(i + 1) {
                assert_eq!(area(a.intersect(*b)), 0.0, "bands overlap");
            }
        }
    }

    #[test]
    fn dashes_start_and_end_on_a_dash_and_fit_the_line() {
        for len in [20.0, 100.0, 452.0, 26.19] {
            let d = dashes(len, 6.0, 3.0);
            assert!(d.len() >= 2, "{len}");
            assert_eq!(d[0].0, 0.0);
            assert!((d.last().unwrap().1 - len).abs() < 1e-3, "{len}: ends at {}", d.last().unwrap().1);
            assert!(d.windows(2).all(|w| w[0].1 <= w[1].0 + 1e-3), "dashes overlap on {len}");
        }
        assert_eq!(dashes(4.0, 6.0, 3.0), vec![(0.0, 4.0)], "a line shorter than a dash is one dash");
    }

    #[test]
    fn a_hard_shadow_is_an_l_shape_outside_the_box() {
        let r = Rect::from_min_size(Pos2::new(24.0, 60.0), vec2(812.0, 100.0));
        let bands = hard_shadow_bands(r, 5.0, 5.0);
        assert_eq!(bands[0], Rect::from_min_max(Pos2::new(836.0, 65.0), Pos2::new(841.0, 165.0)));
        assert_eq!(bands[1], Rect::from_min_max(Pos2::new(29.0, 160.0), Pos2::new(836.0, 165.0)));
        for b in bands {
            assert_eq!(area(b.intersect(r)), 0.0, "the shadow must not be drawn under the box");
        }
        let total: f32 = bands.iter().map(|b| area(*b)).sum();
        // the shadow box (812 x 100) minus the part of it that lies under the original box (807 x 95)
        assert_eq!(total, 812.0 * 100.0 - 807.0 * 95.0);
    }
}
