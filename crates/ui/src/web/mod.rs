//! A faithful port of the old web UI's look (`src/styles/*.css` of the Tauri app) to egui.
//!
//! Every number in here (sizes, paddings, letter-spacing, shadows) was measured from the real React components rendered
//! in a browser (`/reference.html` in the web project), not guessed. The fonts are the web app's own: JetBrains Mono
//! (400/700/800) and Archivo Black, both SIL OFL (licences are in `assets/fonts`).

pub mod all_agents;
pub mod fmt;
pub mod fonts;
pub mod hero;
pub mod pages;
pub mod paint;
pub mod shadow;
pub mod text;
pub mod widgets;
pub mod wrap;

pub use fonts::install_fonts;
pub use text::{Run, Shadow, Style, Weight};

use egui::Color32;

/// `--bw`: the border width of the whole design.
pub const BW: f32 = 2.0;

/// The design tokens of `base.css`, resolved for a theme.
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub bg: Color32,
    pub ink: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub alert: Color32,
    pub ok: Color32,
    pub halo: Color32,
}

impl Tokens {
    pub fn for_theme(theme: crate::view::Theme) -> Self {
        use crate::view::Theme;
        let rgb = |r, g, b| Color32::from_rgb(r, g, b);
        match theme {
            Theme::Dark => Self { bg: rgb(10, 10, 10), ink: rgb(242, 242, 238), dim: rgb(140, 140, 132), accent: rgb(255, 230, 0), alert: rgb(255, 77, 0), ok: rgb(70, 220, 120), halo: rgb(0, 0, 0) },
            Theme::Light => Self { bg: rgb(240, 238, 230), ink: rgb(10, 10, 10), dim: rgb(92, 92, 86), accent: rgb(0, 51, 255), alert: rgb(214, 48, 0), ok: rgb(0, 140, 70), halo: rgb(255, 255, 255) },
        }
    }

    /// `rgb(var(--ink) / a)`
    pub fn ink_a(&self, a: f32) -> Color32 {
        with_alpha(self.ink, a)
    }
}

/// An agent's colour from its `#rrggbb` string (a safe grey when it is malformed).
pub fn agent_color(hex: &str) -> Color32 {
    crate::theme::agent_color(hex)
}

/// The colour with its alpha replaced by `a` (0..=1), as CSS `rgb(r g b / a)` does.
pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Theme;

    #[test]
    fn tokens_match_base_css() {
        let d = Tokens::for_theme(Theme::Dark);
        assert_eq!((d.bg, d.ink, d.dim, d.accent, d.alert, d.ok), (Color32::from_rgb(10, 10, 10), Color32::from_rgb(242, 242, 238), Color32::from_rgb(140, 140, 132), Color32::from_rgb(255, 230, 0), Color32::from_rgb(255, 77, 0), Color32::from_rgb(70, 220, 120)));
        let l = Tokens::for_theme(Theme::Light);
        assert_eq!((l.bg, l.ink, l.accent, l.halo), (Color32::from_rgb(240, 238, 230), Color32::from_rgb(10, 10, 10), Color32::from_rgb(0, 51, 255), Color32::WHITE));
    }

    #[test]
    fn alpha_is_applied_like_css() {
        let c = with_alpha(Color32::from_rgb(10, 20, 30), 0.35);
        // Color32 stores translucent colours premultiplied, so channels can differ by 1 after the round trip.
        let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 1);
        assert!(close(c.to_srgba_unmultiplied(), [10, 20, 30, 89]), "{:?}", c.to_srgba_unmultiplied());
        assert!(close(with_alpha(c, 1.0).to_srgba_unmultiplied(), [10, 20, 30, 255]));
        assert_eq!(with_alpha(Color32::WHITE, 2.0).a(), 255);
        assert_eq!(with_alpha(Color32::WHITE, -1.0).a(), 0);
    }
}
