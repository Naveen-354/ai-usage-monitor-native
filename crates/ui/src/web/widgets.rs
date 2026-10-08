//! The old design's widgets (`base.css` / `main.css`), each with its measured size.

use egui::{vec2, Color32, FontId, Id, Margin, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::hero::ping;
use super::paint::{border, dashed_border, hard_shadow};
use super::text::{Run, Style, Weight};
use super::wrap::measure;
use super::{with_alpha, Tokens, BW};
use crate::view::{AnimationIntensity, PeriodKey};

// ------------------------------------------------------------------------------------------------ text styles

pub fn body() -> Style {
    Style::mono(12.0, Weight::W400)
}
pub fn body_bold() -> Style {
    Style::mono(12.0, Weight::W700)
}
/// `.frow__hint`, `.fineprint`: 11px dim.
pub fn hint() -> Style {
    Style::mono(11.0, Weight::W400)
}
pub fn h2_style() -> Style {
    Style::display(15.0).spacing_em(0.04)
}
pub fn h3_style() -> Style {
    Style::display(11.0).spacing_em(0.12)
}

/// Paints `text` with its line box at (`left`, `line_top`) and returns its advance width.
pub fn put(ui: &Ui, text: &str, style: &Style, left: f32, line_top: f32, color: Color32) -> f32 {
    let run = Run::new(ui.ctx(), text, style);
    run.paint(ui.painter(), left, line_top, color);
    run.width
}

/// Like [`put`] but centred horizontally on `cx` (CSS centres the trailing letter-spacing too).
pub fn put_centered(ui: &Ui, text: &str, style: &Style, cx: f32, line_top: f32, color: Color32) {
    let run = Run::new(ui.ctx(), text, style);
    run.paint(ui.painter(), cx - run.width / 2.0, line_top, color);
}

// ------------------------------------------------------------------------------------------------ headings and prose

/// `.h2`: Archivo Black 15px, 0.04em, margin 0.
pub fn h2(ui: &mut Ui, t: &Tokens, text: &str) {
    let st = h2_style();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), st.line_h), Sense::hover());
    put(ui, text, &st, rect.left(), rect.top(), t.ink);
}

/// `.h3`: Archivo Black 11px dim, 0.12em, margin 18 0 8, bottom border.
pub fn h3(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(18.0);
    let st = h3_style();
    let h = st.line_h + 4.0 + BW;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    put(ui, text, &st, rect.left(), rect.top(), t.dim);
    ui.painter().rect_filled(Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - BW), rect.max), 0.0, t.ink);
    ui.add_space(8.0);
}

/// A wrapped paragraph in `style` (`max_w` limits the line length, e.g. `62ch` for prose).
pub fn paragraph(ui: &mut Ui, text: &str, style: &Style, color: Color32, max_w: Option<f32>, anywhere: bool) {
    let w = max_w.map_or(ui.available_width(), |m| m.min(ui.available_width()));
    let lines = super::wrap::wrap(text, style, w, anywhere);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), lines.len() as f32 * style.line_h), Sense::hover());
    for (i, l) in lines.iter().enumerate() {
        put(ui, l, style, rect.left(), rect.top() + i as f32 * style.line_h, color);
    }
}

/// `.fineprint`: 11px dim, margin 6 0.
pub fn fineprint(ui: &mut Ui, t: &Tokens, text: &str) {
    fineprint_limited(ui, t, text, f32::INFINITY);
}

/// A `.fineprint` whose line length is capped (`max-width: 62ch` inside `.prose`).
pub fn fineprint_limited(ui: &mut Ui, t: &Tokens, text: &str, max_w: f32) {
    ui.add_space(6.0);
    paragraph(ui, text, &hint(), t.dim, Some(max_w), false);
    ui.add_space(6.0);
}

// ------------------------------------------------------------------------------------------------ buttons

/// `.btn`: bordered, with a hard 3 px shadow that grows on hover and sinks when pressed.
pub fn btn(ui: &mut Ui, t: &Tokens, text: &str, danger: bool) -> Response {
    let st = Style::mono(11.0, Weight::W700).spacing_em(0.1);
    let run = Run::new(ui.ctx(), text, &st);
    let size = vec2(run.width + 2.0 * 12.0 + 2.0 * BW, st.line_h + 2.0 * 5.0 + 2.0 * BW);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (offset, shadow) = if resp.is_pointer_button_down_on() {
        (vec2(2.0, 2.0), 1.0)
    } else if resp.hovered() {
        (vec2(-1.0, -1.0), 4.0)
    } else {
        (Vec2::ZERO, 3.0)
    };
    let color = if danger { t.alert } else { t.ink };
    let r = rect.translate(offset);
    let p = ui.painter();
    hard_shadow(p, r, shadow, shadow, color);
    border(p, r, BW, color);
    run.paint(p, r.left() + BW + 12.0, r.top() + BW + 5.0, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ------------------------------------------------------------------------------------------------ period switch

pub const PERIODS: [PeriodKey; 4] = [PeriodKey::Day, PeriodKey::Week, PeriodKey::Month, PeriodKey::Year];

pub fn period_short(p: PeriodKey) -> &'static str {
    match p {
        PeriodKey::Day => "DAY",
        PeriodKey::Week => "WEEK",
        PeriodKey::Month => "MONTH",
        PeriodKey::Year => "YEAR",
    }
}

/// Border-box widths of the four buttons inside a `width`-wide switch: equal content shares, the first three carry a
/// 2 px right border (so 47, 47, 47, 45 at 190 px).
pub fn period_button_widths(width: f32) -> [f32; 4] {
    let share = (width - 2.0 * BW - 3.0 * BW) / 4.0;
    [share + BW, share + BW, share + BW, share]
}

/// `.period`: the DAY / WEEK / MONTH / YEAR segmented control. Returns the period clicked this frame.
pub fn period_switch(ui: &mut Ui, t: &Tokens, value: PeriodKey, width: f32) -> Option<PeriodKey> {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 3.0 + 13.5 + 3.0 + 2.0 * BW), Sense::hover());
    let widths = period_button_widths(width);
    let st = Style::mono(10.0, Weight::W700).spacing_em(0.1).lh_px(13.5);
    let mut picked = None;
    let mut x = rect.left() + BW;
    for (i, p) in PERIODS.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + BW), vec2(widths[i], rect.height() - 2.0 * BW));
        let resp = ui.interact(r, ui.id().with(("period", i)), Sense::click());
        let active = *p == value;
        let painter = ui.painter();
        if active {
            painter.rect_filled(r, 0.0, t.ink);
        } else if resp.hovered() {
            painter.rect_filled(r, 0.0, t.ink_a(0.12));
        }
        if i < 3 {
            painter.rect_filled(Rect::from_min_max(Pos2::new(r.right() - BW, r.top()), r.max), 0.0, t.ink);
        }
        let content_w = widths[i] - if i < 3 { BW } else { 0.0 };
        put_centered(ui, period_short(*p), &st, r.left() + content_w / 2.0, r.top() + 3.0, if active { t.bg } else { t.ink });
        if resp.clicked() {
            picked = Some(*p);
        }
        x += widths[i];
    }
    border(ui.painter(), rect, BW, t.ink);
    picked
}

// ------------------------------------------------------------------------------------------------ tags and dots

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Warn,
    Accent,
}

pub fn tone_color(t: &Tokens, tone: Tone) -> Color32 {
    match tone {
        Tone::Plain => t.ink,
        Tone::Warn => t.alert,
        Tone::Accent => t.ok,
    }
}

pub fn tag_style() -> Style {
    Style::mono(10.0, Weight::W700).spacing_em(0.08).lh_px(14.0)
}

/// Size of a `.tag` (padding 0 5, border 2, line-height 14).
pub fn tag_size(ctx: &egui::Context, text: &str) -> Vec2 {
    let run = Run::new(ctx, text, &tag_style());
    vec2(run.width + 10.0 + 2.0 * BW, 14.0 + 2.0 * BW)
}

/// Paints a `.tag` with its top-left at `pos`; returns its size.
pub fn tag_at(ui: &Ui, t: &Tokens, pos: Pos2, text: &str, tone: Tone) -> Vec2 {
    let size = tag_size(ui.ctx(), text);
    let c = tone_color(t, tone);
    let r = Rect::from_min_size(pos, size);
    border(ui.painter(), r, BW, c);
    put(ui, text, &tag_style(), r.left() + BW + 5.0, r.top() + BW, c);
    size
}

/// `.dot`: a 9 px square agent marker; `muted` = hollow at 60 % opacity; `live` adds the stepped ping ring.
#[allow(clippy::too_many_arguments)]
pub fn agent_dot(ui: &Ui, pos: Pos2, color: Color32, muted: bool, live: bool, time_ms: u64, intensity: AnimationIntensity) {
    let p = ui.painter();
    let r = Rect::from_min_size(pos, Vec2::splat(9.0));
    let fade = if muted { 0.6 } else { 1.0 };
    if muted {
        border(p, r, BW, with_alpha(color, fade));
    } else {
        p.rect_filled(r, 0.0, color);
    }
    if live {
        let (scale, alpha) = ping(time_ms, intensity);
        let ring = Rect::from_center_size(r.center(), Vec2::splat(15.0 * scale));
        border(p, ring, BW, with_alpha(color, alpha * fade));
    }
}

/// `.bar`. The old stylesheet gives it `flex: 1` inside a column, which collapses it to its 4 px of border and a
/// zero-height fill, so in the old app every bar shows only as a solid line. That look is reproduced on purpose
/// (`FILL_VISIBLE = false`); flip it to draw the fill in the agent's colour at `ratio`.
pub const FILL_VISIBLE: bool = false;

/// Height of a `.bar` as the old page renders it.
pub const BAR_H: f32 = if FILL_VISIBLE { 10.0 } else { 2.0 * BW };

/// Paints a `.bar` with its top-left at (`left`, `top`).
pub fn usage_bar_at(p: &egui::Painter, t: &Tokens, left: f32, top: f32, width: f32, ratio: f32, color: Color32) {
    let rect = Rect::from_min_size(Pos2::new(left, top), vec2(width, BAR_H));
    if FILL_VISIBLE {
        let r = ratio.clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_size(rect.min + vec2(BW, BW), vec2((rect.width() - 2.0 * BW) * r, rect.height() - 2.0 * BW)), 0.0, color);
        border(p, rect, BW, t.ink);
    } else {
        p.rect_filled(rect, 0.0, t.ink);
    }
}

pub fn usage_bar(ui: &mut Ui, t: &Tokens, ratio: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR_H), Sense::hover());
    usage_bar_at(ui.painter(), t, rect.left(), rect.top(), rect.width(), ratio, color);
}

// ------------------------------------------------------------------------------------------------ settings: sections and rows

pub const SECTION_TITLE_H: f32 = 5.0 + 16.2 + 5.0; // measured 26.19 (12px display title, line-height 1.35)

/// `.sect`: a bordered box with an ink title bar and a hard 5 px shadow. Returns what `rows` returns.
pub fn section<R>(ui: &mut Ui, t: &Tokens, title: &str, rows: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    let frame = egui::Frame::none().inner_margin(Margin::same(BW)).show(ui, |ui| {
        ui.set_width(width - 2.0 * BW);
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let st = Style::display(12.0).spacing_em(0.14);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), SECTION_TITLE_H), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, t.ink);
        put(ui, title, &st, bar.left() + 12.0, bar.top() + 5.0, t.bg);
        rows(ui)
    });
    let outer = frame.response.rect;
    hard_shadow(ui.painter(), outer, 5.0, 5.0, t.ink);
    border(ui.painter(), outer, BW, t.ink);
    ui.add_space(22.0);
    frame.inner
}

/// One `.frow`: label (bold) with an optional dim hint on the left, a control of `control` size on the right.
/// `last` removes the bottom hairline. Returns the row's response (clicking anywhere on it is a click on its label).
pub fn frow(ui: &mut Ui, t: &Tokens, label: &str, hint_text: Option<&str>, last: bool, control: Vec2, add_control: impl FnOnce(&mut Ui, Rect)) -> Response {
    let label_h = body_bold().line_h;
    let text_h = label_h + hint_text.map_or(0.0, |_| 2.0 + hint().line_h);
    let content_h = text_h.max(control.y);
    let row_h = 9.0 + content_h + 9.0 + if last { 0.0 } else { 1.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
    let text_top = rect.top() + 9.0 + (content_h - text_h) / 2.0;
    put(ui, label, &body_bold(), rect.left() + 12.0, text_top, t.ink);
    if let Some(h) = hint_text {
        put(ui, h, &hint(), rect.left() + 12.0, text_top + label_h + 2.0, t.dim);
    }
    if !last {
        ui.painter().rect_filled(Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 1.0), rect.max), 0.0, t.ink_a(0.35));
    }
    let control_rect = Rect::from_min_size(Pos2::new(rect.right() - 12.0 - control.x, rect.top() + 9.0 + (content_h - control.y) / 2.0), control);
    add_control(ui, control_rect);
    resp
}

/// `.toggle`: 42 x 22; checked = accent fill with a black thumb on the right.
pub fn toggle_box(ui: &mut Ui, t: &Tokens, rect: Rect, checked: bool) {
    let p = ui.painter();
    if checked {
        p.rect_filled(rect, 0.0, t.accent);
    }
    border(p, rect, BW, t.ink);
    let x = rect.left() + BW + 2.0 + if checked { 20.0 } else { 0.0 };
    p.rect_filled(Rect::from_min_size(Pos2::new(x, rect.top() + BW + 2.0), Vec2::splat(14.0)), 0.0, if checked { Color32::BLACK } else { t.ink });
}

/// A settings row with a toggle. Returns the new value when it changed.
pub fn toggle_row(ui: &mut Ui, t: &Tokens, label: &str, hint_text: Option<&str>, last: bool, value: bool) -> Option<bool> {
    let mut changed = None;
    let row = frow(ui, t, label, hint_text, last, vec2(42.0, 22.0), |ui, r| {
        toggle_box(ui, t, r, value);
    });
    if row.clicked() {
        changed = Some(!value);
    }
    row.on_hover_cursor(egui::CursorIcon::PointingHand);
    changed
}

/// `.field`: a bordered box (min 150 px wide, 30 px high for a select, 28.19 for a number).
pub fn field_box(ui: &Ui, t: &Tokens, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, t.bg);
    border(ui.painter(), rect, BW, t.ink);
}

/// A settings row with a drop-down. Returns the chosen option's index.
pub fn select_row(ui: &mut Ui, t: &Tokens, label: &str, hint_text: Option<&str>, last: bool, options: &[&str], current: usize) -> Option<usize> {
    let widest = options.iter().map(|o| measure(&body(), o)).fold(0.0, f32::max);
    let w = (widest + 6.0 * 2.0 + 2.0 * BW + 22.0).max(152.0);
    let id = ui.id().with(("select", label));
    let mut chosen = None;
    frow(ui, t, label, hint_text, last, vec2(w, 30.0), |ui, r| {
        let resp = ui.interact(r, id, Sense::click());
        field_box(ui, t, r);
        put(ui, options.get(current).copied().unwrap_or(""), &body(), r.left() + BW + 6.0, r.top() + 6.0 + 1.0, t.ink);
        // the drop-down arrow
        let c = Pos2::new(r.right() - 12.0, r.center().y);
        let pts = [c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)];
        ui.painter().line_segment([pts[0], pts[1]], egui::Stroke::new(1.6, t.ink));
        ui.painter().line_segment([pts[1], pts[2]], egui::Stroke::new(1.6, t.ink));
        let popup = id.with("popup");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup::popup_below_widget(ui, popup, &resp, egui::popup::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(r.width());
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            for (i, o) in options.iter().enumerate() {
                let (ir, iresp) = ui.allocate_exact_size(vec2(r.width(), 22.0), Sense::click());
                let on = iresp.hovered() || i == current;
                ui.painter().rect_filled(ir, 0.0, if on { t.accent } else { t.bg });
                put(ui, o, &body(), ir.left() + 8.0, ir.top() + 3.0, if on { Color32::BLACK } else { t.ink });
                if iresp.clicked() {
                    chosen = Some(i);
                }
            }
        });
    });
    chosen
}

pub const SLIDER_W: f32 = 170.0;

/// What a slider row is about: its label, optional hint and the range it moves in.
pub struct Slider<'a> {
    pub label: &'a str,
    pub hint: Option<&'a str>,
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

/// Chrome's range input as the old page styled it (`accent-color`): a thin track filled up to a round thumb.
pub fn slider_row(ui: &mut Ui, t: &Tokens, spec: &Slider, last: bool, value: f32, format: &dyn Fn(f32) -> String) -> Option<f32> {
    let Slider { label, hint: hint_text, min, max, step } = *spec;
    let readout = format(value);
    let readout_w = measure(&hint(), &readout);
    let id = ui.id().with(("slider", label));
    let mut committed = None;
    frow(ui, t, label, hint_text, last, vec2(SLIDER_W + 8.0 + readout_w.max(13.2), 20.0), |ui, r| {
        let track = Rect::from_min_size(r.min + vec2(0.0, 2.0), vec2(SLIDER_W, 16.0));
        let resp = ui.interact(track, id, Sense::click_and_drag());
        let live: f32 = ui.memory_mut(|m| m.data.get_temp(id)).unwrap_or(value);
        let mut v = if resp.dragged() || resp.drag_started() || resp.clicked() {
            resp.interact_pointer_pos().map_or(live, |p| slider_value_at(p.x, track, min, max, step))
        } else {
            value
        };
        if resp.dragged() || resp.drag_started() {
            ui.memory_mut(|m| m.data.insert_temp(id, v));
        }
        if resp.drag_stopped() || resp.clicked() {
            ui.memory_mut(|m| m.data.remove::<f32>(id));
            committed = Some(v);
        }
        if !(resp.dragged() || resp.drag_started()) {
            v = value;
        }
        let frac = ((v - min) / (max - min)).clamp(0.0, 1.0);
        let thumb_x = track.left() + 8.0 + frac * (track.width() - 16.0);
        let y = track.center().y;
        let p = ui.painter();
        p.rect_filled(Rect::from_min_max(Pos2::new(track.left(), y - 2.0), Pos2::new(track.right(), y + 2.0)), 2.0, Color32::from_rgb(59, 59, 59));
        p.rect_filled(Rect::from_min_max(Pos2::new(track.left(), y - 2.0), Pos2::new(thumb_x, y + 2.0)), 2.0, t.accent);
        p.circle_filled(Pos2::new(thumb_x, y), 8.0, t.accent);
        put(ui, &format(v), &hint(), r.left() + SLIDER_W + 8.0, r.top() + 2.5, t.dim);
    });
    committed
}

/// The value a slider takes when the pointer is at `x` over `track` (the thumb travels 8 px in from each end).
pub fn slider_value_at(x: f32, track: Rect, min: f32, max: f32, step: f32) -> f32 {
    let frac = ((x - track.left() - 8.0) / (track.width() - 16.0)).clamp(0.0, 1.0);
    let raw = min + frac * (max - min);
    let snapped = (raw / step).round() * step;
    // remove float noise such as 0.30000000000000004
    ((snapped * 1000.0).round() / 1000.0).clamp(min, max)
}

/// A settings row with a number box (84 px). `text` is the edit buffer (kept by the caller); returns a committed
/// value when the box loses focus or Enter is pressed: `Some(None)` = emptied.
#[allow(clippy::too_many_arguments)]
pub fn number_row(ui: &mut Ui, t: &Tokens, label: &str, hint_text: Option<&str>, last: bool, value: Option<i64>, min: i64, max: i64, suffix: Option<&str>, placeholder: &str) -> Option<Option<i64>> {
    let id = ui.id().with(("number", label));
    let suffix_w = suffix.map_or(0.0, |s| 8.0 + measure(&hint(), s));
    let mut out = None;
    frow(ui, t, label, hint_text, last, vec2(84.0 + suffix_w, 28.2), |ui, r| {
        let field = Rect::from_min_size(r.min, vec2(84.0, 28.2));
        field_box(ui, t, field);
        let mut buf: String = ui.memory_mut(|m| m.data.get_temp(id)).unwrap_or_else(|| value.map(|v| v.to_string()).unwrap_or_default());
        let inner = field.shrink2(vec2(BW + 6.0, BW + 4.0));
        let edit = egui::TextEdit::singleline(&mut buf)
            .frame(false)
            .margin(Margin::ZERO)
            .font(FontId::new(12.0, egui::FontFamily::Name(super::fonts::MONO_400.into())))
            .text_color(t.ink)
            .hint_text(egui::RichText::new(placeholder).color(t.dim))
            .desired_width(inner.width());
        // A child ui, not `put`: the field sits inside a row that has already been allocated, so it must not move the cursor.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        let resp = child.add(edit);
        if resp.changed() {
            ui.memory_mut(|m| m.data.insert_temp(id, buf.clone()));
        }
        if resp.lost_focus() {
            ui.memory_mut(|m| m.data.remove::<String>(id));
            out = if buf.trim().is_empty() { Some(None) } else { buf.trim().parse::<f64>().ok().map(|n| Some((n.round() as i64).clamp(min, max))) };
        }
        if let Some(s) = suffix {
            put(ui, s, &hint(), r.left() + 84.0 + 8.0, r.top() + 6.7, t.dim);
        }
    });
    out
}

/// `.chipbox`: a bordered toggle button with an agent colour square; dashed and dim when off.
pub fn chipbox_size(ctx: &egui::Context, label: &str) -> Vec2 {
    let run = Run::new(ctx, label, &body_bold());
    vec2(run.width + 2.0 * 9.0 + 2.0 * BW + 9.0 + 6.0, 3.0 + body_bold().line_h + 3.0 + 2.0 * BW)
}

pub fn chipbox(ui: &mut Ui, t: &Tokens, pos: Pos2, label: &str, color: Color32, on: bool) -> Response {
    let size = chipbox_size(ui.ctx(), label);
    let rect = Rect::from_min_size(pos, size);
    let resp = ui.interact(rect, ui.id().with(("chipbox", label)), Sense::click());
    let fg = if on { t.ink } else { t.dim };
    let p = ui.painter();
    if on {
        border(p, rect, BW, t.ink);
    } else {
        dashed_border(p, rect, BW, t.dim);
    }
    let dot = Rect::from_min_size(Pos2::new(rect.left() + BW + 9.0, rect.center().y - 4.5), Vec2::splat(9.0));
    p.rect_filled(dot, 0.0, color);
    put(ui, label, &body_bold(), dot.right() + 6.0, rect.top() + BW + 3.0, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Flows chip boxes left to right, wrapping at `width`, 8 px apart. Returns each one's top-left and the total height.
pub fn chip_flow(sizes: &[Vec2], width: f32) -> (Vec<Pos2>, f32) {
    let (mut x, mut y, mut row_h) = (0.0, 0.0, 0.0_f32);
    let mut out = Vec::with_capacity(sizes.len());
    for s in sizes {
        if x > 0.0 && x + s.x > width {
            x = 0.0;
            y += row_h + 8.0;
            row_h = 0.0;
        }
        out.push(Pos2::new(x, y));
        x += s.x + 8.0;
        row_h = row_h.max(s.y);
    }
    (out, y + row_h)
}

/// Stable id helper for callers that need per-row state.
pub fn id_of(ui: &Ui, key: &str) -> Id {
    ui.id().with(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_period_buttons_split_the_switch_like_the_browser() {
        // Measured: a 190 px switch has buttons 47, 47, 47, 45 wide (the first three carry a 2 px right border).
        let w = period_button_widths(190.0);
        assert_eq!(w, [47.0, 47.0, 47.0, 45.0]);
        assert_eq!(w.iter().sum::<f32>() + 2.0 * BW, 190.0);
    }

    #[test]
    fn a_slider_maps_the_pointer_to_snapped_values_inside_its_range() {
        let track = Rect::from_min_size(Pos2::new(100.0, 0.0), vec2(170.0, 16.0));
        assert_eq!(slider_value_at(0.0, track, 0.6, 2.0, 0.05), 0.6, "left of the track clamps to min");
        assert_eq!(slider_value_at(9999.0, track, 0.6, 2.0, 0.05), 2.0, "right of it clamps to max");
        let mid = slider_value_at(100.0 + 8.0 + (170.0 - 16.0) / 2.0, track, 0.0, 1.0, 0.02);
        assert!((mid - 0.5).abs() < 1e-6, "{mid}");
        // always a multiple of the step
        for x in (90..280).step_by(7) {
            let v = slider_value_at(x as f32, track, 0.6, 2.0, 0.05);
            let k = (v - 0.6) / 0.05;
            assert!((k - k.round()).abs() < 1e-3, "{v} is not on the 0.05 grid");
        }
    }

    #[test]
    fn chips_wrap_to_the_next_line_when_they_do_not_fit() {
        let s = vec2(116.2, 26.19);
        let (pos, h) = chip_flow(&[s, s, s, s, s], 556.0);
        assert_eq!(pos[0], Pos2::new(0.0, 0.0));
        assert!((pos[1].x - (116.2 + 8.0)).abs() < 1e-3, "8 px apart");
        assert!(pos[4].y > 0.0, "the fifth wraps: {:?}", pos[4]);
        assert!((h - (2.0 * 26.19 + 8.0)).abs() < 1e-3, "two rows and one gap: {h}");
        let (one, h1) = chip_flow(&[s], 556.0);
        assert_eq!((one.len(), h1), (1, 26.19));
        assert_eq!(chip_flow(&[], 100.0), (vec![], 0.0));
    }

    #[test]
    fn tag_and_chip_sizes_match_the_measured_boxes() {
        let ctx = egui::Context::default();
        crate::web::install_fonts(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        // Measured: the "OK" tag is 27.61 x 18; the "Claude Code" chip is 116.2 x 26.19.
        let tag = tag_size(&ctx, "OK");
        assert!((tag.x - 27.61).abs() < 0.2 && (tag.y - 18.0).abs() < 0.01, "{tag:?}");
        let chip = chipbox_size(&ctx, "Claude Code");
        assert!((chip.x - 116.2).abs() < 0.2 && (chip.y - 26.19).abs() < 0.05, "{chip:?}");
    }

    #[test]
    fn tones_use_the_documented_colours() {
        let t = Tokens::for_theme(crate::view::Theme::Dark);
        assert_eq!(tone_color(&t, Tone::Plain), t.ink);
        assert_eq!(tone_color(&t, Tone::Warn), t.alert);
        assert_eq!(tone_color(&t, Tone::Accent), t.ok);
    }

    #[test]
    fn a_number_row_does_not_pull_the_rows_below_it_up() {
        let ctx = egui::Context::default();
        crate::web::install_fonts(&ctx);
        let t = Tokens::for_theme(crate::view::Theme::Dark);
        let mut tops = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                tops.clear();
                for label in ["a", "b"] {
                    tops.push(ui.cursor().top());
                    number_row(ui, &t, label, Some("hint"), false, Some(5), 1, 10, Some("seconds"), "");
                }
                tops.push(ui.cursor().top());
            });
        });
        // each row with a hint is 9 + (16.2 + 2 + 14.85) + 9 + 1 = 52.05 px tall
        assert!((tops[1] - tops[0] - 52.05).abs() < 0.1, "first row is {} px", tops[1] - tops[0]);
        assert!((tops[2] - tops[1] - 52.05).abs() < 0.1, "second row is {} px", tops[2] - tops[1]);
    }

    #[test]
    fn widgets_paint_headlessly_without_panicking() {
        let ctx = egui::Context::default();
        crate::web::install_fonts(&ctx);
        let t = Tokens::for_theme(crate::view::Theme::Dark);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                h2(ui, &t, "ALL AGENTS");
                h3(ui, &t, "AGENTS");
                fineprint(ui, &t, "A short note that is long enough to wrap onto a second line when the column is narrow, hopefully.");
                assert!(!btn(ui, &t, "SETTINGS", false).clicked());
                assert!(period_switch(ui, &t, PeriodKey::Day, 190.0).is_none());
                section(ui, &t, "GENERAL", |ui| {
                    assert_eq!(toggle_row(ui, &t, "Always on top", Some("Keep the overlay above every other window."), false, true), None);
                    assert_eq!(select_row(ui, &t, "Theme", None, false, &["Dark", "Light"], 0), None);
                    assert_eq!(slider_row(ui, &t, &Slider { label: "Token text size", hint: Some("hint"), min: 0.6, max: 2.0, step: 0.05 }, false, 1.0, &|v| format!("{}%", (v * 100.0).round())), None);
                    assert_eq!(number_row(ui, &t, "Polling interval", None, true, Some(30), 5, 3600, Some("seconds"), ""), None);
                });
                usage_bar(ui, &t, 0.5, Color32::RED);
            });
        });
        assert!(out.shapes.len() > 30);
    }
}
