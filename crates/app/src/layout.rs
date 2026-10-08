//! Default overlay window sizes (the web version's rule, `platform/overlay.rs`). All the *inside* layout lives in
//! `native_ui::web::hero`, which is verified against the browser.

/// Default compact window size for a text scale (1.0 -> 236 x 208).
pub fn default_compact_size(text_scale: f32) -> (f32, f32) {
    let s = text_scale.clamp(0.6, 2.0);
    let w = (236.0 * s).round().clamp(190.0, 640.0);
    let h = (208.0 + 48.0 * (s - 1.0)).round().clamp(170.0, 320.0);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_window_matches_the_web_versions_sizes() {
        assert_eq!(default_compact_size(1.0), (236.0, 208.0));
        assert_eq!(default_compact_size(0.0), default_compact_size(0.6));
        assert!(default_compact_size(9.0).0 <= 640.0);
    }

    #[test]
    fn a_larger_text_size_grows_the_window() {
        let (w1, h1) = default_compact_size(1.0);
        let (w2, h2) = default_compact_size(1.5);
        assert!(w2 > w1 && h2 > h1);
        assert_eq!(default_compact_size(0.6), (190.0, 189.0), "the smallest window keeps its label and dots");
    }
}
