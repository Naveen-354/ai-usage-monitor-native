//! Brutalist theme and reusable widgets (module `theme`).
//!
//! Visual style: Sharp corners (zero rounding), hard 2px borders, monospace numerals,
//! flat fills, high contrast, no gradients, no soft shadows, restrained colour.
//! Dark and light palettes taken from `ref/web/styles/base.css`.

use egui::{epaint::Shadow, Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

/// Theme palette tokens derived from `ref/web/styles/base.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// `--bg`: background
    pub bg: Color32,
    /// `--ink`: primary foreground / text / border
    pub ink: Color32,
    /// `--dim`: secondary text / hints
    pub dim: Color32,
    /// `--accent`: highlight / active state
    pub accent: Color32,
    /// `--alert`: warning / error / danger
    pub alert: Color32,
    /// `--ok`: success / status ok
    pub ok: Color32,
    /// `--halo`: text shadow outline color
    pub halo: Color32,
    /// Selection highlight fill
    pub selection_bg: Color32,
    /// Text color on top of selection fill
    pub selection_fg: Color32,
}

impl Palette {
    /// Dark theme tokens matching `:root` in `ref/web/styles/base.css`.
    pub const fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(10, 10, 10),
            ink: Color32::from_rgb(242, 242, 238),
            dim: Color32::from_rgb(140, 140, 132),
            accent: Color32::from_rgb(255, 230, 0),
            alert: Color32::from_rgb(255, 77, 0),
            ok: Color32::from_rgb(70, 220, 120),
            halo: Color32::from_rgb(0, 0, 0),
            selection_bg: Color32::from_rgb(255, 230, 0),
            selection_fg: Color32::from_rgb(0, 0, 0),
        }
    }

    /// Light theme tokens matching `:root[data-theme='light']` in `ref/web/styles/base.css`.
    pub const fn light() -> Self {
        Self {
            bg: Color32::from_rgb(240, 238, 230),
            ink: Color32::from_rgb(10, 10, 10),
            dim: Color32::from_rgb(92, 92, 86),
            accent: Color32::from_rgb(0, 51, 255),
            alert: Color32::from_rgb(214, 48, 0),
            ok: Color32::from_rgb(0, 140, 70),
            halo: Color32::from_rgb(255, 255, 255),
            selection_bg: Color32::from_rgb(0, 51, 255),
            selection_fg: Color32::from_rgb(255, 255, 255),
        }
    }

    /// Returns the palette for a given theme.
    pub fn for_theme(theme: crate::view::Theme) -> Self {
        match theme {
            crate::view::Theme::Dark => Self::dark(),
            crate::view::Theme::Light => Self::light(),
        }
    }
}

/// Calculate standard sRGB relative luminance according to WCAG 2.1 specifications.
pub fn relative_luminance(c: Color32) -> f64 {
    let to_linear = |val: u8| -> f64 {
        let s = val as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = to_linear(c.r());
    let g = to_linear(c.g());
    let b = to_linear(c.b());
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Calculate the WCAG 2.1 contrast ratio between two colors (range: 1.0..=21.0).
pub fn contrast_ratio(c1: Color32, c2: Color32) -> f64 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Apply Brutalist styles and visuals to the egui context:
/// - Zero rounding everywhere
/// - 2px hard strokes
/// - No window/popup shadows
/// - Flat fills
/// - Monospace for numerals and UI text
/// - Spacing/padding derived from CSS
/// - High-contrast selection and hover states
pub fn apply(ctx: &egui::Context, theme: crate::view::Theme) {
    let pal = Palette::for_theme(theme);
    let is_dark = matches!(theme, crate::view::Theme::Dark);

    let mut visuals = if is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.dark_mode = is_dark;
    visuals.override_text_color = Some(pal.ink);

    let border_stroke = Stroke::new(2.0, pal.ink);

    // Noninteractive
    visuals.widgets.noninteractive.bg_fill = pal.bg;
    visuals.widgets.noninteractive.weak_bg_fill = pal.bg;
    visuals.widgets.noninteractive.bg_stroke = border_stroke;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.noninteractive.rounding = Rounding::ZERO;

    // Inactive
    visuals.widgets.inactive.bg_fill = pal.bg;
    visuals.widgets.inactive.weak_bg_fill = pal.bg;
    visuals.widgets.inactive.bg_stroke = border_stroke;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.inactive.rounding = Rounding::ZERO;

    // Hovered: high-contrast subtle flat tone
    visuals.widgets.hovered.bg_fill = if is_dark {
        Color32::from_rgb(38, 38, 38)
    } else {
        Color32::from_rgb(215, 212, 202)
    };
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.hovered.bg_stroke = border_stroke;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.hovered.rounding = Rounding::ZERO;

    // Active
    visuals.widgets.active.bg_fill = pal.accent;
    visuals.widgets.active.weak_bg_fill = pal.accent;
    visuals.widgets.active.bg_stroke = border_stroke;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, pal.selection_fg);
    visuals.widgets.active.rounding = Rounding::ZERO;

    // Open
    visuals.widgets.open.bg_fill = pal.bg;
    visuals.widgets.open.weak_bg_fill = pal.bg;
    visuals.widgets.open.bg_stroke = border_stroke;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, pal.ink);
    visuals.widgets.open.rounding = Rounding::ZERO;

    // Selection
    visuals.selection.bg_fill = pal.selection_bg;
    visuals.selection.stroke = Stroke::new(1.0, pal.selection_fg);

    // Hard corners, no shadows
    visuals.window_rounding = Rounding::ZERO;
    visuals.menu_rounding = Rounding::ZERO;
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.window_stroke = border_stroke;
    visuals.window_fill = pal.bg;
    visuals.panel_fill = pal.bg;
    visuals.extreme_bg_color = pal.bg;
    visuals.code_bg_color = pal.bg;

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    // Layout and spacing from CSS
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(14.0);

    // Font hierarchy (default monospace for tabular numerals and Brutalist aesthetic)
    let mono = egui::FontFamily::Monospace;
    style.text_styles.insert(egui::TextStyle::Body, FontId::new(12.0, mono.clone()));
    style.text_styles.insert(egui::TextStyle::Button, FontId::new(11.0, mono.clone()));
    style.text_styles.insert(egui::TextStyle::Heading, FontId::new(15.0, mono.clone()));
    style.text_styles.insert(egui::TextStyle::Monospace, FontId::new(12.0, mono.clone()));
    style.text_styles.insert(egui::TextStyle::Small, FontId::new(10.0, mono));

    ctx.set_style(style);
}

/// Parse `#rrggbb` and `#rgb` hex strings into `Color32`.
///
/// Returns safe grey `#888888` on invalid input.
pub fn agent_color(hex: &str) -> Color32 {
    let s = hex.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    } else if s.len() == 3 {
        let chars: Vec<char> = s.chars().collect();
        if let (Some(r), Some(g), Some(b)) = (
            chars[0].to_digit(16),
            chars[1].to_digit(16),
            chars[2].to_digit(16),
        ) {
            return Color32::from_rgb((r * 17) as u8, (g * 17) as u8, (b * 17) as u8);
        }
    }
    // Safe grey fallback
    Color32::from_rgb(136, 136, 136)
}

/// Pure helper: clamps fraction to `0.0..=1.0`, treating `NaN` as `0.0`.
#[inline]
pub fn clamp_fraction(fraction: f32) -> f32 {
    if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    }
}

/// Tag / chip styling variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagStyle {
    Normal,
    Warn,
    Accent,
    Dim,
}

// ---------------------------------------------------------------------------
// Reusable Brutalist Widgets
// ---------------------------------------------------------------------------

/// Brutalist push button with a hard drop shadow, 2px border, and zero rounding.
///
/// Ported from `.btn` in `ref/web/styles/base.css`.
pub fn brutal_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    brutal_button_styled(ui, text, false)
}

/// Styled Brutalist push button with optional danger style.
pub fn brutal_button_styled(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    is_danger: bool,
) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let border_color = if is_danger { pal.alert } else { pal.ink };
    let text_color = if is_danger { pal.alert } else { pal.ink };

    let widget_text = text.into();
    let font_id = FontId::new(11.0, egui::FontFamily::Monospace);
    let galley = widget_text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);

    // Padding: 5px vertical, 12px horizontal + 4px extra room for hard shadow
    let padding = egui::vec2(12.0, 5.0);
    let face_size = galley.size() + padding * 2.0;
    let total_size = face_size + egui::vec2(4.0, 4.0);

    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let (offset, shadow_offset) = if response.is_pointer_button_down_on() {
            // Active: pressed in
            (egui::vec2(2.0, 2.0), egui::vec2(1.0, 1.0))
        } else if response.hovered() {
            // Hover: translated -1, -1 with 4px shadow
            (egui::vec2(0.0, 0.0), egui::vec2(4.0, 4.0))
        } else {
            // Normal: 3px shadow
            (egui::vec2(1.0, 1.0), egui::vec2(3.0, 3.0))
        };

        let face_rect = Rect::from_min_size(rect.min + offset, face_size);
        let shadow_rect = Rect::from_min_size(rect.min + offset + shadow_offset, face_size);

        // Draw hard drop shadow (no blur, 0 rounding)
        ui.painter().rect_filled(shadow_rect, Rounding::ZERO, border_color);

        // Draw button face
        ui.painter().rect_filled(face_rect, Rounding::ZERO, pal.bg);
        ui.painter().rect_stroke(face_rect, Rounding::ZERO, Stroke::new(2.0, border_color));

        // Center text
        let text_pos = face_rect.center() - galley.size() / 2.0;
        ui.painter().galley_with_override_text_color(text_pos, galley, text_color);
    }

    response
}

/// Brutalist toggle switch widget (42x22 hard box with a 14x14 square slider).
///
/// Ported from `.toggle` in `ref/web/styles/main.css`.
pub fn toggle_button(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let desired_size = egui::vec2(42.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let bg_fill = if *on { pal.accent } else { pal.bg };
        // Outer box: 2px hard border
        ui.painter().rect_filled(rect, Rounding::ZERO, bg_fill);
        ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(2.0, pal.ink));

        // Slider thumb: 14x14 square
        let thumb_x = if *on {
            rect.max.x - 14.0 - 2.0
        } else {
            rect.min.x + 2.0
        };
        let thumb_y = rect.min.y + 2.0;
        let thumb_rect = Rect::from_min_size(egui::pos2(thumb_x, thumb_y), egui::vec2(14.0, 14.0));
        let thumb_fill = if *on { Color32::BLACK } else { pal.ink };

        ui.painter().rect_filled(thumb_rect, Rounding::ZERO, thumb_fill);
    }

    response
}

/// Hard-bordered Brutalist tag chip.
///
/// Ported from `.tag` in `ref/web/styles/base.css`.
pub fn tag(ui: &mut egui::Ui, text: &str, style: TagStyle) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let color = match style {
        TagStyle::Normal => pal.ink,
        TagStyle::Warn => pal.alert,
        TagStyle::Accent => pal.ok,
        TagStyle::Dim => pal.dim,
    };

    let font_id = FontId::new(10.0, egui::FontFamily::Monospace);
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), font_id, color);
    let padding = egui::vec2(5.0, 2.0);
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(2.0, color));
        let text_pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(text_pos, galley, color);
    }

    response
}

/// Reusable Brutalist chip button (e.g. for agent filters, tabs, or mini options).
///
/// Ported from `.chip` in `ref/web/styles/overlay.css`.
pub fn chip(ui: &mut egui::Ui, text: &str, is_on: bool) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let (border_color, bg_fill, text_color) = if is_on {
        (pal.accent, pal.accent, Color32::BLACK)
    } else {
        (pal.ink, pal.bg, pal.ink)
    };

    let font_id = FontId::new(9.0, egui::FontFamily::Monospace);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font_id, text_color);
    let padding = egui::vec2(5.0, 2.0);
    let size = egui::vec2((galley.size().x + padding.x * 2.0).max(20.0), 18.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let current_bg = if !is_on && response.hovered() {
            pal.ink
        } else {
            bg_fill
        };
        let current_fg = if !is_on && response.hovered() {
            pal.bg
        } else {
            text_color
        };

        ui.painter().rect_filled(rect, Rounding::ZERO, current_bg);
        ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(2.0, border_color));

        let text_pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley_with_override_text_color(text_pos, galley, current_fg);
    }

    response
}

/// Hard-edged progress bar with 2px ink stroke and clamped fill.
///
/// Clamps fraction to `0.0..=1.0` and handles `NaN` safely.
pub fn bar(ui: &mut egui::Ui, fraction: f32, color: Color32) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let clamped = clamp_fraction(fraction);
    let width = ui.available_width().max(24.0);
    let height = 10.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        // Outer border
        ui.painter().rect_filled(rect, Rounding::ZERO, pal.bg);
        ui.painter().rect_stroke(rect, Rounding::ZERO, Stroke::new(2.0, pal.ink));

        // Inner fill
        if clamped > 0.0 {
            let fill_width = ((rect.width() - 4.0) * clamped).max(0.0);
            let fill_rect = Rect::from_min_size(
                rect.min + egui::vec2(2.0, 2.0),
                egui::vec2(fill_width, rect.height() - 4.0),
            );
            ui.painter().rect_filled(fill_rect, Rounding::ZERO, color);
        }
    }

    response
}

/// Uppercase section header with a 2px underline rule.
///
/// Ported from `.h3` in `ref/web/styles/base.css`.
pub fn section_header(ui: &mut egui::Ui, title: &str) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let font_id = FontId::new(11.0, egui::FontFamily::Monospace);
    let galley = ui.painter().layout_no_wrap(title.to_uppercase(), font_id, pal.dim);
    let height = galley.size().y + 8.0;
    let width = ui.available_width();

    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        // Draw text
        ui.painter().galley(rect.min + egui::vec2(0.0, 2.0), galley, pal.dim);
        // Draw 2px bottom border across available width
        let line_y = rect.max.y - 1.0;
        ui.painter().line_segment(
            [egui::pos2(rect.min.x, line_y), egui::pos2(rect.max.x, line_y)],
            Stroke::new(2.0, pal.ink),
        );
    }

    response
}

/// Solid 2px divider line across the available width.
pub fn divider(ui: &mut egui::Ui) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 4.0), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let line_y = rect.center().y;
        ui.painter().line_segment(
            [egui::pos2(rect.min.x, line_y), egui::pos2(rect.max.x, line_y)],
            Stroke::new(2.0, pal.ink),
        );
    }

    response
}

/// Brutalist key-value stat row (e.g. for diagnostics or statistics).
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str) -> egui::Response {
    let pal = Palette::for_theme(if ui.visuals().dark_mode {
        crate::view::Theme::Dark
    } else {
        crate::view::Theme::Light
    });

    let label_font = FontId::new(10.0, egui::FontFamily::Monospace);
    let value_font = FontId::new(12.0, egui::FontFamily::Monospace);

    let label_galley = ui.painter().layout_no_wrap(label.to_uppercase(), label_font, pal.dim);
    let value_galley = ui.painter().layout_no_wrap(value.to_string(), value_font, pal.ink);

    let height = label_galley.size().y.max(value_galley.size().y) + 8.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        // Label on left
        let label_pos = egui::pos2(rect.min.x + 4.0, rect.center().y - label_galley.size().y / 2.0);
        ui.painter().galley(label_pos, label_galley, pal.dim);

        // Value on right
        let value_pos = egui::pos2(
            rect.max.x - value_galley.size().x - 4.0,
            rect.center().y - value_galley.size().y / 2.0,
        );
        ui.painter().galley(value_pos, value_galley, pal.ink);

        // Subtle bottom border
        let line_y = rect.max.y - 1.0;
        let dim_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(pal.ink.r(), pal.ink.g(), pal.ink.b(), 60));
        ui.painter().line_segment(
            [egui::pos2(rect.min.x, line_y), egui::pos2(rect.max.x, line_y)],
            dim_stroke,
        );
    }

    response
}

/// Paint text with a hard, thin dark outline for readability over transparent backgrounds.
///
/// Ports the multi-stop `text-shadow` halo from `overlay.css`:
/// `text-shadow: 0 0 2px rgb(var(--halo) / 1), 0 1px 2px rgb(var(--halo) / 0.9), 0 0 6px rgb(var(--halo) / 0.45);`
/// The text layout (galley) is created ONCE, and drawn at outline offsets before painting the foreground.
pub fn paint_halo_text(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let halo_color = Color32::from_black_alpha(220);
    // Layout galley ONCE
    let galley = painter.layout_no_wrap(text.to_string(), font, color);

    let anchor_offset = Vec2::new(
        match anchor.x() {
            egui::Align::Min => 0.0,
            egui::Align::Center => galley.size().x * 0.5,
            egui::Align::Max => galley.size().x,
        },
        match anchor.y() {
            egui::Align::Min => 0.0,
            egui::Align::Center => galley.size().y * 0.5,
            egui::Align::Max => galley.size().y,
        },
    );

    let origin = pos - anchor_offset;

    // Draw outline copies around the text origin
    const OUTLINE_OFFSETS: [Vec2; 9] = [
        Vec2::new(-1.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(0.0, 1.0),
        Vec2::new(-1.0, -1.0),
        Vec2::new(1.0, -1.0),
        Vec2::new(-1.0, 1.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 2.0),
    ];

    for offset in OUTLINE_OFFSETS {
        painter.galley_with_override_text_color(origin + offset, galley.clone(), halo_color);
    }

    // Paint the foreground text
    painter.galley(origin, galley, color);
}

/// Draw a low-res pixel grid preview with sharp, unblurred, zero-rounding cells.
///
/// Useful for the pixel fire texture canvas preview.
pub fn pixel_grid<F>(
    painter: &egui::Painter,
    rect: Rect,
    cols: usize,
    rows: usize,
    mut color_fn: F,
) where
    F: FnMut(usize, usize) -> Color32,
{
    if cols == 0 || rows == 0 {
        return;
    }
    let cell_w = rect.width() / cols as f32;
    let cell_h = rect.height() / rows as f32;

    for y in 0..rows {
        for x in 0..cols {
            let color = color_fn(x, y);
            if color != Color32::TRANSPARENT {
                let cell_rect = Rect::from_min_size(
                    rect.min + Vec2::new(x as f32 * cell_w, y as f32 * cell_h),
                    Vec2::new(cell_w, cell_h),
                );
                painter.rect_filled(cell_rect, Rounding::ZERO, color);
            }
        }
    }
}

/// Font installation helper.
///
/// Uses egui's default monospace font out-of-the-box. If bundled fonts
/// (`JetBrains Mono`, `Archivo Black`) are embedded in the future,
/// they would be registered here into `egui::FontDefinitions`.
pub fn install_fonts(_ctx: &egui::Context) {
    // Bundled font registration stub:
    // let mut fonts = egui::FontDefinitions::default();
    // fonts.font_data.insert("JetBrainsMono".to_owned(), egui::FontData::from_static(include_bytes!(...)));
    // fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "JetBrainsMono".to_owned());
    // _ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Standard Component State & Show API
// ---------------------------------------------------------------------------

/// State for the theme demonstration / widget gallery component.
#[derive(Debug, Clone)]
pub struct State {
    pub theme: crate::view::Theme,
    pub toggle_on: bool,
    pub progress: f32,
    pub active_chip: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            theme: crate::view::Theme::Dark,
            toggle_on: true,
            progress: 0.65,
            active_chip: 0,
        }
    }
}

/// Actions emitted by the theme gallery component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ThemeChanged(crate::view::Theme),
    ToggleChanged(bool),
    ChipSelected(usize),
    ButtonClicked,
}

/// Show the theme showcase and widget gallery.
pub fn show(ui: &mut egui::Ui, state: &mut State) -> Vec<Action> {
    let mut actions = Vec::new();

    ui.horizontal(|ui| {
        ui.label("THEME:");
        let is_dark = matches!(state.theme, crate::view::Theme::Dark);
        if ui.selectable_label(is_dark, "DARK").clicked() && !is_dark {
            state.theme = crate::view::Theme::Dark;
            apply(ui.ctx(), state.theme);
            actions.push(Action::ThemeChanged(state.theme));
        }
        if ui.selectable_label(!is_dark, "LIGHT").clicked() && is_dark {
            state.theme = crate::view::Theme::Light;
            apply(ui.ctx(), state.theme);
            actions.push(Action::ThemeChanged(state.theme));
        }
    });

    divider(ui);

    section_header(ui, "Buttons");
    ui.horizontal(|ui| {
        if brutal_button(ui, "NORMAL BUTTON").clicked() {
            actions.push(Action::ButtonClicked);
        }
        if brutal_button_styled(ui, "DANGER BUTTON", true).clicked() {
            actions.push(Action::ButtonClicked);
        }
    });

    section_header(ui, "Toggle Switch");
    ui.horizontal(|ui| {
        if toggle_button(ui, &mut state.toggle_on).changed() {
            actions.push(Action::ToggleChanged(state.toggle_on));
        }
        ui.label(if state.toggle_on { "ACTIVE (ON)" } else { "INACTIVE (OFF)" });
    });

    section_header(ui, "Chips & Tags");
    ui.horizontal(|ui| {
        tag(ui, "NORMAL", TagStyle::Normal);
        tag(ui, "WARN", TagStyle::Warn);
        tag(ui, "ACCENT", TagStyle::Accent);
        tag(ui, "DIM", TagStyle::Dim);
    });

    ui.horizontal(|ui| {
        for (i, name) in ["DAY", "WEEK", "MONTH", "YEAR"].iter().enumerate() {
            if chip(ui, name, state.active_chip == i).clicked() {
                state.active_chip = i;
                actions.push(Action::ChipSelected(i));
            }
        }
    });

    section_header(ui, "Progress Bar");
    let bar_color = Palette::for_theme(state.theme).accent;
    bar(ui, state.progress, bar_color);

    section_header(ui, "Statistics & Key-Values");
    stat(ui, "TOTAL EVENTS", "1,248,500");
    stat(ui, "STATUS", "HEALTHY");
    stat(ui, "CACHE HIT RATIO", "94.2%");

    section_header(ui, "Pixel Grid Texture Preview");
    let (grid_rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 32.0), egui::Sense::hover());
    if ui.is_rect_visible(grid_rect) {
        pixel_grid(ui.painter(), grid_rect, 16, 4, |x, y| {
            let heat = (x * 16 + y * 60) as u8;
            Color32::from_rgb(255, heat.min(200), 0)
        });
    }

    actions
}

// ---------------------------------------------------------------------------
// Unit and Integration Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wcag_contrast_dark_palette() {
        let pal = Palette::dark();
        // Body text on background: ratio >= 4.5:1
        let text_on_bg = contrast_ratio(pal.ink, pal.bg);
        assert!(
            text_on_bg >= 4.5,
            "Dark theme text on bg contrast {text_on_bg} < 4.5"
        );

        // Secondary / dim text on background: ratio >= 4.5:1
        let dim_on_bg = contrast_ratio(pal.dim, pal.bg);
        assert!(
            dim_on_bg >= 4.5,
            "Dark theme dim on bg contrast {dim_on_bg} < 4.5"
        );

        // Border on background: ratio >= 3:1
        let border_on_bg = contrast_ratio(pal.ink, pal.bg);
        assert!(
            border_on_bg >= 3.0,
            "Dark theme border on bg contrast {border_on_bg} < 3.0"
        );

        // Inverted text on solid ink container: ratio >= 4.5:1
        let inverted_contrast = contrast_ratio(pal.bg, pal.ink);
        assert!(
            inverted_contrast >= 4.5,
            "Dark theme inverted contrast {inverted_contrast} < 4.5"
        );

        // Selection text on selection background: ratio >= 4.5:1
        let sel_contrast = contrast_ratio(pal.selection_fg, pal.selection_bg);
        assert!(
            sel_contrast >= 4.5,
            "Dark theme selection contrast {sel_contrast} < 4.5"
        );
    }

    #[test]
    fn wcag_contrast_light_palette() {
        let pal = Palette::light();
        // Body text on background: ratio >= 4.5:1
        let text_on_bg = contrast_ratio(pal.ink, pal.bg);
        assert!(
            text_on_bg >= 4.5,
            "Light theme text on bg contrast {text_on_bg} < 4.5"
        );

        // Secondary / dim text on background: ratio >= 4.5:1
        let dim_on_bg = contrast_ratio(pal.dim, pal.bg);
        assert!(
            dim_on_bg >= 4.5,
            "Light theme dim on bg contrast {dim_on_bg} < 4.5"
        );

        // Border on background: ratio >= 3:1
        let border_on_bg = contrast_ratio(pal.ink, pal.bg);
        assert!(
            border_on_bg >= 3.0,
            "Light theme border on bg contrast {border_on_bg} < 3.0"
        );

        // Inverted text on solid ink container: ratio >= 4.5:1
        let inverted_contrast = contrast_ratio(pal.bg, pal.ink);
        assert!(
            inverted_contrast >= 4.5,
            "Light theme inverted contrast {inverted_contrast} < 4.5"
        );

        // Selection text on selection background: ratio >= 4.5:1
        let sel_contrast = contrast_ratio(pal.selection_fg, pal.selection_bg);
        assert!(
            sel_contrast >= 4.5,
            "Light theme selection contrast {sel_contrast} < 4.5"
        );
    }

    #[test]
    fn apply_produces_zero_rounding_and_no_shadows() {
        for theme in [crate::view::Theme::Dark, crate::view::Theme::Light] {
            let ctx = egui::Context::default();
            apply(&ctx, theme);

            let style = ctx.style();
            let visuals = &style.visuals;

            // Zero rounding everywhere
            assert_eq!(visuals.window_rounding, Rounding::ZERO);
            assert_eq!(visuals.menu_rounding, Rounding::ZERO);
            assert_eq!(visuals.widgets.noninteractive.rounding, Rounding::ZERO);
            assert_eq!(visuals.widgets.inactive.rounding, Rounding::ZERO);
            assert_eq!(visuals.widgets.hovered.rounding, Rounding::ZERO);
            assert_eq!(visuals.widgets.active.rounding, Rounding::ZERO);
            assert_eq!(visuals.widgets.open.rounding, Rounding::ZERO);

            // No window or popup shadows
            assert_eq!(visuals.window_shadow, Shadow::NONE);
            assert_eq!(visuals.popup_shadow, Shadow::NONE);

            // 2px hard border strokes
            assert_eq!(visuals.widgets.noninteractive.bg_stroke.width, 2.0);
            assert_eq!(visuals.widgets.inactive.bg_stroke.width, 2.0);
            assert_eq!(visuals.widgets.hovered.bg_stroke.width, 2.0);
            assert_eq!(visuals.widgets.active.bg_stroke.width, 2.0);
            assert_eq!(visuals.window_stroke.width, 2.0);
        }
    }

    #[test]
    fn color_parsing_table_tests() {
        // 3-hex shorthand
        assert_eq!(agent_color("#fff"), Color32::from_rgb(255, 255, 255));
        assert_eq!(agent_color("#000"), Color32::from_rgb(0, 0, 0));
        assert_eq!(agent_color("#f0a"), Color32::from_rgb(255, 0, 170));

        // 6-hex lowercase & uppercase
        assert_eq!(agent_color("#ff6a00"), Color32::from_rgb(255, 106, 0));
        assert_eq!(agent_color("#FF6A00"), Color32::from_rgb(255, 106, 0));
        assert_eq!(agent_color("#2bb673"), Color32::from_rgb(43, 182, 115));
        assert_eq!(agent_color("#4C8BF5"), Color32::from_rgb(76, 139, 245));

        // Without leading '#'
        assert_eq!(agent_color("ff6a00"), Color32::from_rgb(255, 106, 0));
        assert_eq!(agent_color("fff"), Color32::from_rgb(255, 255, 255));

        // Bad inputs fallback to safe grey (136, 136, 136)
        let safe_grey = Color32::from_rgb(136, 136, 136);
        assert_eq!(agent_color(""), safe_grey);
        assert_eq!(agent_color("invalid"), safe_grey);
        assert_eq!(agent_color("#xyz"), safe_grey);
        assert_eq!(agent_color("#12"), safe_grey);
        assert_eq!(agent_color("#12345"), safe_grey);
        assert_eq!(agent_color("#12345678"), safe_grey);
    }

    #[test]
    fn bar_fraction_clamping() {
        assert_eq!(clamp_fraction(0.0), 0.0);
        assert_eq!(clamp_fraction(0.5), 0.5);
        assert_eq!(clamp_fraction(1.0), 1.0);
        assert_eq!(clamp_fraction(-0.25), 0.0);
        assert_eq!(clamp_fraction(1.5), 1.0);
        assert_eq!(clamp_fraction(f32::NAN), 0.0);
        assert_eq!(clamp_fraction(f32::INFINITY), 1.0);
        assert_eq!(clamp_fraction(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn headless_egui_smoke_test() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);

        for theme in [crate::view::Theme::Dark, crate::view::Theme::Light] {
            let mut state = State {
                theme,
                toggle_on: true,
                progress: 0.5,
                active_chip: 1,
            };

            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                apply(ctx, state.theme);
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _actions = show(ui, &mut state);

                    // Call every widget individually to ensure zero panics
                    let _ = brutal_button(ui, "TEST BUTTON");
                    let _ = brutal_button_styled(ui, "DANGER", true);
                    let mut t = false;
                    let _ = toggle_button(ui, &mut t);
                    let _ = tag(ui, "TAG", TagStyle::Normal);
                    let _ = tag(ui, "WARN", TagStyle::Warn);
                    let _ = tag(ui, "ACCENT", TagStyle::Accent);
                    let _ = tag(ui, "DIM", TagStyle::Dim);
                    let _ = chip(ui, "CHIP_ON", true);
                    let _ = chip(ui, "CHIP_OFF", false);
                    let _ = bar(ui, 0.75, Color32::RED);
                    let _ = section_header(ui, "HEADER TEST");
                    let _ = divider(ui);
                    let _ = stat(ui, "METRIC", "42K");

                    paint_halo_text(
                        ui.painter(),
                        Pos2::new(10.0, 10.0),
                        Align2::LEFT_TOP,
                        "HALO TEXT",
                        FontId::monospace(12.0),
                        Color32::WHITE,
                    );

                    pixel_grid(
                        ui.painter(),
                        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(20.0, 20.0)),
                        4,
                        4,
                        |_x, _y| Color32::YELLOW,
                    );
                });
            });
        }
    }
}
