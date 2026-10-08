use egui::{Response, Ui, WidgetText, Rect, Pos2, Vec2, Color32, Stroke, Sense, Margin};

/// Pure helper to snap a value to the nearest step, aligned to min.
pub fn snap_to_step(value: f32, min: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    let offset = value - min;
    let steps = (offset / step).round();
    min + steps * step
}

/// A Brutalist square checkbox. Hard 2px borders, flat fills.
pub fn square_checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<WidgetText>) -> Response {
    let text = text.into();
    let size = 16.0;
    
    // We allocate space for the checkbox and the text
    ui.horizontal(|ui| {
        let (rect, mut response) = ui.allocate_exact_size(Vec2::new(size, size), Sense::click());
        
        if response.clicked() {
            *checked = !*checked;
            response.mark_changed();
        }
        
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            
            // Hard 2px border, zero rounding
            let stroke_color = if *checked {
                ui.visuals().selection.bg_fill
            } else {
                visuals.fg_stroke.color
            };
            
            ui.painter().rect(
                rect,
                0.0,
                if *checked { ui.visuals().selection.bg_fill } else { Color32::TRANSPARENT },
                Stroke::new(2.0, stroke_color),
            );
            
            // If checked, draw a high contrast checkmark or just a solid fill.
            // Let's do a smaller inner square to represent checked state in brutalist style.
            if *checked {
                let inner = rect.shrink(4.0);
                ui.painter().rect(
                    inner,
                    0.0,
                    ui.visuals().text_color(),
                    Stroke::NONE,
                );
            }
        }
        
        ui.label(text);
        
        response
    }).inner
}

/// A Brutalist segmented select with hard borders.
pub fn segmented_select<T: PartialEq + Clone>(
    ui: &mut Ui,
    selected: &mut T,
    options: &[(T, String)],
) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut changed = false;
        let mut overall_response: Option<Response> = None;
        
        for (i, (val, label)) in options.iter().enumerate() {
            let is_selected = *selected == *val;
            
            let bg_color = if is_selected {
                ui.visuals().selection.bg_fill
            } else {
                Color32::TRANSPARENT
            };
            
            let text_color = if is_selected {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            };
            
            // Measure text
            let text: WidgetText = label.clone().into();
            let button_padding = Vec2::new(8.0, 4.0);
            
            let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::FontSelection::Default);
            let size = galley.size() + button_padding * 2.0;
            
            let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
            
            if response.clicked() && !is_selected {
                *selected = val.clone();
                changed = true;
                response.mark_changed();
            }
            
            if ui.is_rect_visible(rect) {
                // Draw borders: 2px hard border. We need to avoid double borders between segments.
                let stroke = Stroke::new(2.0, ui.visuals().widgets.noninteractive.fg_stroke.color);
                
                ui.painter().rect(
                    rect,
                    0.0,
                    bg_color,
                    Stroke::NONE,
                );
                
                // Draw Top, Bottom
                ui.painter().hline(rect.min.x..=rect.max.x, rect.min.y, stroke);
                ui.painter().hline(rect.min.x..=rect.max.x, rect.max.y, stroke);
                
                // Draw Left (always)
                ui.painter().vline(rect.min.x, rect.min.y..=rect.max.y, stroke);
                
                // Draw Right (only if last)
                if i == options.len() - 1 {
                    ui.painter().vline(rect.max.x, rect.min.y..=rect.max.y, stroke);
                }
                
                let text_pos = rect.center() - galley.size() / 2.0;
                ui.painter().galley(text_pos, galley, text_color);
            }
            
            if let Some(r) = overall_response.as_mut() {
                *r = r.clone().union(response);
            } else {
                overall_response = Some(response);
            }
        }
        
        let mut r = overall_response.unwrap();
        if changed {
            r.mark_changed();
        }
        r
    }).inner
}

/// Flat slider with numeric readout and brutalist style.
pub fn flat_slider(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    format_val: impl Fn(f32) -> String,
) -> Response {
    ui.horizontal(|ui| {
        let (rect, mut response) = ui.allocate_exact_size(Vec2::new(150.0, 16.0), Sense::click_and_drag());
        
        if response.dragged() || response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let t = (pos.x - rect.min.x) / rect.width();
                let t = t.clamp(0.0, 1.0);
                let raw_val = *range.start() + t * (*range.end() - *range.start());
                let new_val = snap_to_step(raw_val, *range.start(), step);
                let new_val = new_val.clamp(*range.start(), *range.end());
                if (*value - new_val).abs() > f32::EPSILON {
                    *value = new_val;
                    response.mark_changed();
                }
            }
        }
        
        if ui.is_rect_visible(rect) {
            // Track
            ui.painter().rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
            );
            
            // Fill
            let t = (*value - *range.start()) / (*range.end() - *range.start());
            let t = t.clamp(0.0, 1.0);
            let fill_rect = Rect::from_min_max(
                rect.min,
                Pos2::new(rect.min.x + rect.width() * t, rect.max.y),
            );
            ui.painter().rect_filled(
                fill_rect,
                0.0,
                ui.visuals().selection.bg_fill,
            );
        }
        
        ui.label(format_val(*value));
        
        response
    }).inner
}

/// Labelled text input with an error line.
pub fn labelled_text_input(
    ui: &mut Ui,
    label: &str,
    text: &mut String,
    error: Option<&str>,
) -> Response {
    ui.vertical(|ui| {
        ui.label(label);
        
        // Wrap egui TextEdit in our brutalist frame manually or configure frame
        let mut frame = egui::Frame::default()
            .inner_margin(Margin::same(4.0))
            .stroke(Stroke::new(2.0, ui.visuals().widgets.noninteractive.fg_stroke.color));
        
        if error.is_some() {
            frame.stroke.color = Color32::RED;
        }
        
        let response = frame.show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(text).frame(false))
        }).inner;
        
        if let Some(err) = error {
            ui.colored_label(Color32::RED, err);
        }
        
        response
    }).inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_to_step() {
        assert!((snap_to_step(1.05, 0.6, 0.05) - 1.05).abs() < 1e-5);
        assert!((snap_to_step(0.62, 0.6, 0.05) - 0.60).abs() < 1e-5);
        assert!((snap_to_step(0.63, 0.6, 0.05) - 0.65).abs() < 1e-5);
    }
}
