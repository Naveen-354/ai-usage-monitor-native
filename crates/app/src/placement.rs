//! Where the overlay goes when the user picks a corner (port of `corner_position` in the old app's `platform/overlay.rs`).

use app_api::view::Corner;

/// Gap kept between the overlay and the screen edge.
pub const MARGIN: f32 = 16.0;

/// A rectangle (the usable desktop area), in the same units as the result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Top-left of a `w` x `h` overlay in `corner` of `area`. `Custom` (and anything unknown) falls back to the top right,
/// like the old app's first launch.
pub fn corner_position(area: Area, corner: Corner, w: f32, h: f32) -> (f32, f32) {
    let left = area.x + MARGIN;
    let right = area.x + area.w - w - MARGIN;
    let top = area.y + MARGIN;
    let bottom = area.y + area.h - h - MARGIN;
    match corner {
        Corner::TopLeft => (left, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
        Corner::TopRight | Corner::Custom => (right, top),
    }
}

/// Keeps a saved position usable: if the window would be (almost) entirely off the screen, it is not.
pub fn is_on_screen(x: f32, y: f32, w: f32, h: f32, area: Area) -> bool {
    let visible_w = (x + w).min(area.x + area.w) - x.max(area.x);
    let visible_h = (y + h).min(area.y + area.h) - y.max(area.y);
    visible_w >= 40.0 && visible_h >= 40.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Area = Area { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };

    #[test]
    fn corner_positions_respect_the_margin_exactly_like_the_old_app() {
        // The old app's own test: a 300 x 200 window on 1920 x 1080.
        assert_eq!(corner_position(SCREEN, Corner::TopLeft, 300.0, 200.0), (16.0, 16.0));
        assert_eq!(corner_position(SCREEN, Corner::TopRight, 300.0, 200.0), (1604.0, 16.0));
        assert_eq!(corner_position(SCREEN, Corner::BottomLeft, 300.0, 200.0), (16.0, 864.0));
        assert_eq!(corner_position(SCREEN, Corner::BottomRight, 300.0, 200.0), (1604.0, 864.0));
    }

    #[test]
    fn custom_and_unplaced_overlays_start_at_the_top_right() {
        assert_eq!(corner_position(SCREEN, Corner::Custom, 236.0, 208.0), (1668.0, 16.0));
    }

    #[test]
    fn a_work_area_with_a_taskbar_keeps_the_overlay_clear_of_it() {
        let area = Area { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 }; // 40 px taskbar
        assert_eq!(corner_position(area, Corner::BottomRight, 236.0, 208.0), (1668.0, 816.0));
    }

    #[test]
    fn a_second_monitor_to_the_left_is_handled_by_its_own_origin() {
        let area = Area { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 };
        assert_eq!(corner_position(area, Corner::TopLeft, 100.0, 100.0), (-1904.0, 16.0));
    }

    #[test]
    fn a_saved_position_that_is_off_screen_is_not_trusted() {
        assert!(is_on_screen(1684.0, 815.0, 236.0, 208.0, SCREEN));
        assert!(is_on_screen(-100.0, 10.0, 236.0, 208.0, SCREEN), "partly off the left edge is fine");
        assert!(!is_on_screen(5000.0, 10.0, 236.0, 208.0, SCREEN), "a monitor that was unplugged");
        assert!(!is_on_screen(10.0, -400.0, 236.0, 208.0, SCREEN));
        assert!(!is_on_screen(1900.0, 1070.0, 236.0, 208.0, SCREEN), "only a sliver visible");
    }
}
