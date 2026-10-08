//! The overlay "hero": flame, big count, letter-spaced period label, agent dots and the hover chips.
//!
//! `geometry` is the CSS flex layout of `.hero` (`overlay.css`) as a pure function, verified against positions measured
//! from the real component in a browser; `show` paints it.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, TextureId, Ui, Vec2};

use super::paint::border;
use super::shadow::ShadowCache;
use super::text::{overlay_text_shadows, Run, Shadow, Style, Weight};
use super::{with_alpha, Tokens, BW};
use crate::view::{AgentOverview, AnimationIntensity};

/// An agent counts as live when it recorded usage within this window.
pub const LIVE_WINDOW_MS: i64 = 60_000;

/// The hero count is K/M/B/T with 4 significant digits, so a live counter visibly ticks (253.5M).
pub const HERO_DIGITS: u32 = 4;

pub fn is_live(last_event_utc_ms: Option<i64>, now_ms: i64) -> bool {
    last_event_utc_ms.is_some_and(|t| now_ms - t >= 0 && now_ms - t < LIVE_WINDOW_MS)
}

const PAD_X: f32 = 18.0;
const GAP: f32 = 4.0;
/// `.fire`: a 160 x 120 canvas with `margin: -60px -62px 0`, so it takes 60 px of layout and overhangs by 60 above.
pub const FIRE_CANVAS: Vec2 = Vec2::new(160.0, 120.0);
const FIRE_SLOT_H: f32 = 60.0;
const LABEL_PAD: (f32, f32, f32, f32) = (2.0, 6.0, 2.0, 12.0); // top right bottom left
const ARROW_W_EM: f32 = 0.9;
const DOT: f32 = 7.0;
const DOT_GAP: f32 = 7.0;

/// What the big-number slot holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A count, e.g. "255.5M".
    Number(String),
    /// `TOKEN DATA / UNAVAILABLE` (never 0 for "unknown").
    Unavailable,
    /// `DATA ERROR / SEE DIAGNOSTICS`.
    Error,
    /// `···` while nothing has loaded yet.
    Pending,
}

impl Body {
    fn chars(&self) -> usize {
        match self {
            Body::Number(s) => s.chars().count(),
            _ => 6, // `var(--n, 6)`
        }
    }

    fn is_number_like(&self) -> bool {
        matches!(self, Body::Number(_) | Body::Pending)
    }
}

/// `.hero__num` font size: `clamp(14px, min(46px * scale, 118cqw / n), 140px)`, where cqw is the hero's content width.
pub fn number_font_px(num_scale: f32, n_chars: usize, content_w: f32) -> f32 {
    let by_width = 118.0 * content_w / 100.0 / n_chars.max(1) as f32;
    (46.0 * num_scale).min(by_width).clamp(14.0, 140.0)
}

/// Where everything sits, in the hero's own coordinates (origin = top-left of the hero box).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub canvas: Rect,
    pub body: Rect,
    pub body_font_px: f32,
    pub label: Rect,
    pub dots: Option<Rect>,
    pub tag: Option<Rect>,
    /// The hero's own height (equals the window height when compact).
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GeomInput {
    pub width: f32,
    /// `Some(window height)` for the compact overlay (the hero fills it); `None` lets the expanded hero size itself.
    pub fill_height: Option<f32>,
    pub num_scale: f32,
    pub n_dots: usize,
    pub has_tag: bool,
    /// Width of the label's letter-spaced text (it sizes the label button).
    pub label_text_w: f32,
}

/// The CSS flex column of `.hero`: items centred horizontally, 4 px apart, centred vertically in the compact overlay.
pub fn geometry(input: &GeomInput, body: &Body) -> Geometry {
    let w = input.width;
    let content_w = w - 2.0 * PAD_X;
    let (body_h, body_w_hint, font_px) = if body.is_number_like() {
        let fs = number_font_px(input.num_scale, body.chars(), content_w);
        (fs * 1.05, f32::NAN, fs)
    } else {
        // `.hero__na`: Archivo Black 15px, line-height 1.12, two lines.
        (2.0 * 15.0 * 1.12, 0.0, 15.0)
    };
    let _ = body_w_hint;

    let label_h = LABEL_PAD.0 + 10.0 * 1.35 + LABEL_PAD.2;
    let label_w = LABEL_PAD.3 + ARROW_W_EM * 10.0 + input.label_text_w + ARROW_W_EM * 10.0 + LABEL_PAD.1;
    let dots_h = DOT;
    let dots_w = input.n_dots as f32 * DOT + input.n_dots.saturating_sub(1) as f32 * DOT_GAP;
    let tag_h = 9.0 * 1.35;

    // Stack height: every item plus the 4 px gaps between them (and the margins `.adots` / `.hero__tag` carry).
    let mut items = vec![FIRE_SLOT_H, body_h, label_h];
    let mut total = FIRE_SLOT_H + GAP + body_h + GAP + label_h;
    if input.n_dots > 0 {
        total += GAP + 6.0 + dots_h;
        items.push(dots_h);
    }
    if input.has_tag {
        total += GAP + 2.0 + tag_h;
        items.push(tag_h);
    }

    let (pad_top, pad_bottom, height, start) = match input.fill_height {
        Some(h) => {
            // `.overlay[data-compact=true] .hero { padding-top: 52px }`; the content is centred in what is left.
            let (pt, pb) = (52.0, 14.0);
            (pt, pb, h, pt + ((h - pt - pb) - total) / 2.0)
        }
        None => (20.0, 16.0, 20.0 + total + 16.0, 20.0),
    };
    let _ = pad_bottom;
    let _ = pad_top;

    let cx = w / 2.0;
    let canvas = Rect::from_min_size(Pos2::new(cx - FIRE_CANVAS.x / 2.0, start - FIRE_SLOT_H), FIRE_CANVAS);
    let mut y = start + FIRE_SLOT_H + GAP;
    let body_w = if body.is_number_like() { f32::NAN } else { 0.0 };
    let _ = body_w;
    let body_rect = Rect::from_min_size(Pos2::new(cx, y), Vec2::new(0.0, body_h)); // x/width filled in by the painter
    y += body_h + GAP;
    let label = Rect::from_center_size(Pos2::new(cx, y + label_h / 2.0), Vec2::new(label_w, label_h));
    y += label_h;
    let dots = (input.n_dots > 0).then(|| {
        y += GAP + 6.0;
        let r = Rect::from_center_size(Pos2::new(cx, y + dots_h / 2.0), Vec2::new(dots_w, dots_h));
        y += dots_h;
        r
    });
    let tag = input.has_tag.then(|| {
        y += GAP + 2.0;
        Rect::from_center_size(Pos2::new(cx, y + tag_h / 2.0), Vec2::new(0.0, tag_h))
    });
    Geometry { canvas, body: body_rect, body_font_px: font_px, label, dots, tag, height }
}

// ------------------------------------------------------------------------------------------------ painting

/// Stepped `ping` animation of the live ring (`steps(4, end)`): (scale, opacity). With motion off the ring is static.
pub fn ping(time_ms: u64, intensity: AnimationIntensity) -> (f32, f32) {
    let duration = match intensity {
        AnimationIntensity::Off => return (1.0, 1.0),
        AnimationIntensity::Low => 3200,
        AnimationIntensity::Normal => 1600,
    };
    let step = ((time_ms % duration) * 4 / duration) as f32 / 4.0;
    (0.7 + 1.0 * step, 0.9 * (1.0 - step))
}

/// An agent marker square (`.adot`): filled when it has numbers, hollow when it has none, with the live ring.
#[allow(clippy::too_many_arguments)]
pub fn agent_square(painter: &egui::Painter, t: &Tokens, top_left: Pos2, color: Color32, hollow: bool, live: bool, time_ms: u64, intensity: AnimationIntensity) {
    let r = Rect::from_min_size(top_left, Vec2::splat(DOT));
    let fade = if hollow { 0.55 } else { 1.0 };
    // box-shadow: 0 0 0 1px halo / .55 (and the whole element is at 55 % opacity when hollow)
    painter.rect_filled(r.expand(1.0), 0.0, with_alpha(t.halo, 0.55 * fade));
    if hollow {
        border(painter, r, BW, with_alpha(color, fade));
    } else {
        painter.rect_filled(r, 0.0, color);
    }
    if live {
        let (scale, alpha) = ping(time_ms, intensity);
        let ring = Rect::from_center_size(r.center(), Vec2::splat((DOT + 6.0) * scale));
        border(painter, ring, BW, with_alpha(color, alpha * fade));
    }
}

/// What one hover chip shows: a text label (PIN) or a line icon, lit up when `on`.
struct ChipSpec<'a> {
    id: &'static str,
    label: Option<&'a Run>,
    icon: Option<Vec<[Pos2; 2]>>,
    on: bool,
}

/// One 18 px high hover chip (`.chip`). Returns its response.
fn chip(ui: &mut Ui, t: &Tokens, spec: &ChipSpec, rect: Rect, visible: f32) -> egui::Response {
    let ChipSpec { id, label, icon, on } = spec;
    let (label, on) = (*label, *on);
    let resp = ui.interact(rect, ui.id().with(*id), if visible > 0.5 { Sense::click() } else { Sense::hover() });
    let hovered = resp.hovered() && visible > 0.5;
    let (bg, fg, bd) = if on {
        (t.accent, Color32::BLACK, t.accent)
    } else if hovered {
        (t.ink, t.bg, t.ink)
    } else {
        (t.bg, t.ink, t.ink)
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, with_alpha(bg, visible));
    border(p, rect, BW, with_alpha(bd, visible));
    if let Some(run) = label {
        run.paint(p, rect.center().x - run.width / 2.0 + run.style.spacing / 2.0, rect.center().y - run.style.line_h / 2.0, with_alpha(fg, visible));
    }
    if let Some(lines) = icon {
        let lines = lines.as_slice();
        // 10 x 10 crisp icon, centred; stroke 1.6
        let origin = rect.center() - Vec2::splat(5.0);
        for [a, b] in lines {
            p.line_segment([origin + a.to_vec2(), origin + b.to_vec2()], Stroke::new(1.6, with_alpha(fg, visible)));
        }
    }
    resp
}

fn icon_expand() -> Vec<[Pos2; 2]> {
    let l = |a: (f32, f32), b: (f32, f32)| [Pos2::new(a.0, a.1), Pos2::new(b.0, b.1)];
    vec![l((1.0, 4.0), (1.0, 1.0)), l((1.0, 1.0), (4.0, 1.0)), l((9.0, 4.0), (9.0, 1.0)), l((9.0, 1.0), (6.0, 1.0)), l((1.0, 6.0), (1.0, 9.0)), l((1.0, 9.0), (4.0, 9.0)), l((9.0, 6.0), (9.0, 9.0)), l((9.0, 9.0), (6.0, 9.0))]
}

fn icon_collapse() -> Vec<[Pos2; 2]> {
    let l = |a: (f32, f32), b: (f32, f32)| [Pos2::new(a.0, a.1), Pos2::new(b.0, b.1)];
    vec![l((1.0, 3.0), (4.0, 3.0)), l((4.0, 3.0), (4.0, 0.0)), l((9.0, 3.0), (6.0, 3.0)), l((6.0, 3.0), (6.0, 0.0)), l((1.0, 7.0), (4.0, 7.0)), l((4.0, 7.0), (4.0, 10.0)), l((9.0, 7.0), (6.0, 7.0)), l((6.0, 7.0), (6.0, 10.0))]
}

fn icon_min() -> Vec<[Pos2; 2]> {
    vec![[Pos2::new(1.0, 8.0), Pos2::new(9.0, 8.0)]]
}

fn icon_close() -> Vec<[Pos2; 2]> {
    vec![[Pos2::new(1.5, 1.5), Pos2::new(8.5, 8.5)], [Pos2::new(8.5, 1.5), Pos2::new(1.5, 8.5)]]
}

/// A burst label that floats up off the count (`+84.2K`).
#[derive(Debug, Clone)]
pub struct Floater {
    pub text: String,
    /// Age of the animation, 0..=1 (it lasts 1.6 s).
    pub age: f32,
    /// Blast magnitude 0..=1 (`--m`).
    pub magnitude: f32,
}

pub struct Props<'a> {
    pub tokens: Tokens,
    pub compact: bool,
    pub num_scale: f32,
    pub body: Body,
    pub live: bool,
    /// "TODAY", "THIS WEEK", ...
    pub period_label: &'a str,
    pub agents: &'a [AgentOverview],
    pub now_ms: i64,
    pub time_ms: u64,
    pub intensity: AnimationIntensity,
    /// "IMPORTING HISTORY" / "PAUSED".
    pub tag: Option<&'a str>,
    pub pin_on: bool,
    pub fire: Option<TextureId>,
    pub floater: Option<Floater>,
    /// Scale of the count during the `pop` animation (1.0 = resting).
    pub pop: f32,
}

/// What the user did to the hero this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Out {
    pub toggle: bool,
    pub cycle_period: bool,
    pub pin: bool,
    pub minimize: bool,
    pub hide: bool,
    pub start_drag: bool,
}

/// The height the expanded hero takes at `width` (the compact one simply fills its window).
pub fn height(ctx: &egui::Context, p: &Props, width: f32) -> f32 {
    let label_style = Style::mono(10.0, Weight::W700).spacing_em(0.34);
    let label = Run::new(ctx, &format!("TOKENS {}", p.period_label), &label_style);
    geometry(
        &GeomInput { width, fill_height: None, num_scale: p.num_scale, n_dots: p.agents.iter().filter(|a| a.enabled).count(), has_tag: p.tag.is_some(), label_text_w: label.width },
        &p.body,
    )
    .height
}

/// Paints the hero into `rect` (the hero box: the whole window when compact) and reports interactions.
pub fn show(ui: &mut Ui, rect: Rect, p: &Props) -> Out {
    let ctx = ui.ctx().clone();
    let t = p.tokens;
    let mut out = Out::default();

    // Texts
    let label_style = Style::mono(10.0, Weight::W700).spacing_em(0.34);
    let label_text = format!("TOKENS {}", p.period_label);
    let label_run = Run::new(&ctx, &label_text, &label_style);
    let g = geometry(
        &GeomInput {
            width: rect.width(),
            fill_height: p.compact.then_some(rect.height()),
            num_scale: p.num_scale,
            n_dots: p.agents.iter().filter(|a| a.enabled).count(),
            has_tag: p.tag.is_some(),
            label_text_w: label_run.width,
        },
        &p.body,
    );
    let at = |r: Rect| r.translate(rect.min.to_vec2());

    // Whole hero: click toggles, dragging moves the window (the old 4 px rule is egui's click distance, set by the app).
    let hero = ui.interact(rect, ui.id().with("hero"), Sense::click_and_drag());
    if hero.drag_started_by(egui::PointerButton::Primary) {
        out.start_drag = true;
    }
    let hovered = ui.rect_contains_pointer(rect);

    let painter = ui.painter().clone();
    let cache_arc = ShadowCache::of(&ctx);
    let mut cache = cache_arc.lock().unwrap_or_else(|e| e.into_inner());

    // Flame (crisp pixels)
    if let Some(tex) = p.fire {
        painter.image(tex, at(g.canvas), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }

    // Count / status
    let body_top = rect.min.y + g.body.min.y;
    match &p.body {
        Body::Number(s) => {
            let fs = g.body_font_px * p.pop;
            let style = Style::mono(fs, Weight::W800).spacing_em(-0.02).lh(1.05);
            let run = Run::new(&ctx, s, &style);
            let left = rect.min.x + g.body.center().x - run.width / 2.0;
            let top = body_top - (style.line_h - g.body_font_px * 1.05) / 2.0;
            cache.paint(&ctx, &painter, &run, s, left, top, t.ink, &overlay_text_shadows(t.halo, p.live));
            if let Some(f) = &p.floater {
                paint_floater(&mut cache, &painter, &ctx, &t, f, Pos2::new(left + run.width, body_top), p.num_scale);
            }
        }
        Body::Pending => {
            let style = Style::mono(g.body_font_px, Weight::W800).spacing_em(-0.02).lh(1.05);
            let run = Run::new(&ctx, "···", &style);
            cache.paint(&ctx, &painter, &run, "···", rect.min.x + g.body.center().x - run.width / 2.0, body_top, t.dim, &overlay_text_shadows(t.halo, p.live));
        }
        Body::Unavailable | Body::Error => {
            let (l1, l2) = if matches!(p.body, Body::Unavailable) { ("TOKEN DATA", "UNAVAILABLE") } else { ("DATA ERROR", "SEE DIAGNOSTICS") };
            let style = Style::display(15.0).spacing_em(0.02).lh(1.12);
            for (i, line) in [l1, l2].into_iter().enumerate() {
                let run = Run::new(&ctx, line, &style);
                cache.paint(&ctx, &painter, &run, line, rect.min.x + g.body.center().x - run.width / 2.0, body_top + i as f32 * style.line_h, t.alert, &overlay_text_shadows(t.halo, false));
            }
        }
    }

    // Period label (a button: click cycles the period; arrows appear on hover)
    let label_rect = at(g.label);
    let label_resp = ui.interact(label_rect, ui.id().with("label"), Sense::click());
    let label_color = if label_resp.hovered() { t.ink } else { with_alpha(t.ink, 0.95) };
    let halo = overlay_text_shadows(t.halo, false);
    let text_left = label_rect.min.x + LABEL_PAD.3 + ARROW_W_EM * 10.0;
    let line_top = label_rect.min.y + LABEL_PAD.0;
    cache.paint(&ctx, &painter, &label_run, &label_text, text_left, line_top, label_color, &halo);
    if hovered {
        let arrow = Run::new(&ctx, "‹", &Style::mono(10.0, Weight::W700));
        cache.paint(&ctx, &painter, &arrow, "‹", label_rect.min.x + LABEL_PAD.3, line_top, with_alpha(label_color, 0.9), &halo);
        let arrow = Run::new(&ctx, "›", &Style::mono(10.0, Weight::W700));
        cache.paint(&ctx, &painter, &arrow, "›", text_left + label_run.width, line_top, with_alpha(label_color, 0.9), &halo);
    }
    if label_resp.clicked() {
        out.cycle_period = true;
    }

    // Agent dots
    if let Some(d) = g.dots {
        let d = at(d);
        let shown: Vec<&AgentOverview> = p.agents.iter().filter(|a| a.enabled).collect();
        for (i, a) in shown.iter().enumerate() {
            let pos = Pos2::new(d.min.x + i as f32 * (DOT + DOT_GAP), d.min.y);
            let live = a.totals.is_some() && is_live(a.last_event_utc_ms, p.now_ms);
            agent_square(&painter, &t, pos, super::agent_color(&a.color), a.totals.is_none(), live, p.time_ms, p.intensity);
        }
    }

    // Importing / paused tag
    if let (Some(tag), Some(r)) = (p.tag, g.tag) {
        let r = at(r);
        let run = Run::new(&ctx, tag, &Style::mono(9.0, Weight::W800).spacing_em(0.16));
        cache.paint(&ctx, &painter, &run, tag, r.center().x - run.width / 2.0, r.min.y, t.accent, &halo);
    }

    // Hover chips: PIN, expand/collapse, minimise, hide (top 12, right 14, 3 px apart; fade in on hover)
    let vis = ctx.animate_bool_with_time(ui.id().with("chips"), hovered, 0.15);
    if vis > 0.0 {
        let pin = Run::new(&ctx, "PIN", &Style::mono(9.0, Weight::W800).spacing_em(0.06).lh(1.0));
        let widths = [pin.width + 10.0 + 2.0 * BW, 24.0, 24.0, 24.0];
        let total: f32 = widths.iter().sum::<f32>() + 3.0 * 3.0;
        let mut x = rect.max.x - 14.0 - total;
        let y = rect.min.y + 12.0;
        let specs = [
            ChipSpec { id: "pin", label: Some(&pin), icon: None, on: p.pin_on },
            ChipSpec { id: "toggle", label: None, icon: Some(if p.compact { icon_expand() } else { icon_collapse() }), on: false },
            ChipSpec { id: "min", label: None, icon: Some(icon_min()), on: false },
            ChipSpec { id: "hide", label: None, icon: Some(icon_close()), on: false },
        ];
        for (i, spec) in specs.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(widths[i], 18.0));
            let resp = chip(ui, &t, spec, r, vis);
            if resp.clicked() && vis > 0.5 {
                match spec.id {
                    "pin" => out.pin = true,
                    "toggle" => out.toggle = true,
                    "min" => out.minimize = true,
                    _ => out.hide = true,
                }
            }
            x += widths[i] + 3.0;
        }
    }

    if hero.clicked_by(egui::PointerButton::Primary) {
        out.toggle = true;
    }
    out
}

/// `.hero__delta`: floats from 8 px below to 26 px above its resting place, fading in and out over 1.6 s.
fn paint_floater(cache: &mut ShadowCache, painter: &egui::Painter, ctx: &egui::Context, t: &Tokens, f: &Floater, num_right_top: Pos2, num_scale: f32) {
    let age = f.age.clamp(0.0, 1.0);
    let opacity = if age < 0.14 { age / 0.14 } else if age < 0.70 { 1.0 } else { 1.0 - (age - 0.70) / 0.30 };
    let dy = 8.0 + (-26.0 - 8.0) * age;
    let fs = (11.0 + 8.0 * f.magnitude) * num_scale;
    let text = format!("+{}", f.text.trim_start_matches('+'));
    let run = Run::new(ctx, &text, &Style::mono(fs, Weight::W800).spacing_em(0.02));
    let shadows = [
        Shadow::new(0.0, 0.0, 2.0, with_alpha(t.halo, opacity)),
        Shadow::new(0.0, 1.0, 3.0, with_alpha(t.halo, 0.95 * opacity)),
        Shadow::new(0.0, 0.0, 12.0, Color32::from_rgba_unmultiplied(255, 106, 0, (153.0 * opacity) as u8)),
    ];
    // right: -6px; top: -16px of the count's box
    let left = num_right_top.x + 6.0 - run.width;
    let top = num_right_top.y - 16.0 + dy;
    cache.paint(ctx, painter, &run, &text, left, top, with_alpha(Color32::from_rgb(255, 176, 48), opacity), &shadows);
    let _ = Align2::LEFT_TOP;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compact_input(n_dots: usize, has_tag: bool) -> GeomInput {
        GeomInput { width: 236.0, fill_height: Some(208.0), num_scale: 1.0, n_dots, has_tag, label_text_w: 112.8 }
    }

    fn near(a: f32, b: f32, tol: f32, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: {a} vs browser {b}");
    }

    #[test]
    fn compact_geometry_matches_the_browser_measurements() {
        // Measured in Chromium at 236 x 208 with "255.5M" and 5 agent dots.
        let g = geometry(&compact_input(5, false), &Body::Number("255.5M".into()));
        near(g.canvas.min.x, 38.0, 0.05, "canvas x");
        near(g.canvas.min.y, -8.89, 0.1, "canvas y");
        near(g.body.min.y, 115.11, 0.1, "number y");
        near(g.body.height(), 41.3, 0.05, "number height");
        near(g.body_font_px, 39.3333, 0.01, "number font");
        near(g.label.min.x, 43.59, 0.1, "label x");
        near(g.label.min.y, 160.39, 0.1, "label y");
        near(g.label.width(), 148.81, 0.1, "label width");
        near(g.label.height(), 17.5, 0.01, "label height");
        let d = g.dots.expect("dots");
        near(d.min.x, 86.5, 0.05, "dots x");
        near(d.min.y, 187.89, 0.1, "dots y");
        near(d.width(), 63.0, 0.01, "dots width");
    }

    #[test]
    fn expanded_hero_sizes_itself_like_the_browser() {
        // Measured at 480 wide: the hero is 186.8 px high (padding 20/16) and the number is the full 46 px.
        let g = geometry(&GeomInput { width: 480.0, fill_height: None, num_scale: 1.0, n_dots: 5, has_tag: false, label_text_w: 112.8 }, &Body::Number("255.5M".into()));
        near(g.height, 186.8, 0.05, "expanded hero height");
        near(g.body_font_px, 46.0, 0.001, "expanded number font");
        near(g.canvas.min.y, 20.0 - 60.0, 0.001, "expanded canvas top");
    }

    #[test]
    fn the_number_font_follows_the_css_clamp() {
        // clamp(14px, min(46px * scale, 118cqw / n), 140px) with cqw = content width
        near(number_font_px(1.0, 6, 200.0), 39.3333, 0.001, "236 wide, 6 chars");
        near(number_font_px(1.0, 4, 200.0), 46.0, 0.001, "short numbers cap at 46");
        near(number_font_px(2.0, 3, 600.0), 92.0, 0.001, "text size doubles it");
        near(number_font_px(0.6, 6, 600.0), 27.6, 0.001, "smallest text size");
        assert_eq!(number_font_px(1.0, 60, 100.0), 14.0, "never below 14");
        assert_eq!(number_font_px(9.0, 1, 4000.0), 140.0, "never above 140");
    }

    #[test]
    fn a_status_tag_pushes_the_stack_up_and_sits_below_the_dots() {
        let a = geometry(&compact_input(5, false), &Body::Number("255.5M".into()));
        let b = geometry(&compact_input(5, true), &Body::Number("255.5M".into()));
        assert!(b.canvas.min.y < a.canvas.min.y, "the taller stack starts higher");
        let (dots, tag) = (b.dots.unwrap(), b.tag.unwrap());
        assert!(tag.min.y >= dots.max.y, "tag {tag:?} below dots {dots:?}");
    }

    #[test]
    fn no_agents_means_no_dots_and_a_shorter_stack() {
        let g = geometry(&compact_input(0, false), &Body::Number("255.5M".into()));
        assert!(g.dots.is_none());
    }

    #[test]
    fn the_unavailable_and_error_messages_take_two_display_lines() {
        for body in [Body::Unavailable, Body::Error] {
            let g = geometry(&compact_input(0, false), &body);
            near(g.body.height(), 33.6, 0.01, "two lines of 15px/1.12");
            assert_eq!(g.body_font_px, 15.0);
        }
    }

    #[test]
    fn everything_is_horizontally_centred() {
        let g = geometry(&compact_input(5, true), &Body::Number("255.5M".into()));
        for r in [g.label, g.dots.unwrap(), g.tag.unwrap()] {
            near(r.center().x, 118.0, 0.01, "centre");
        }
        near(g.canvas.center().x, 118.0, 0.01, "canvas centre");
    }

    #[test]
    fn the_live_window_is_strict_like_the_web_version() {
        assert!(is_live(Some(1_000), 1_000 + 59_999));
        assert!(!is_live(Some(1_000), 1_000 + 60_000), "exactly 60 s is no longer live");
        assert!(!is_live(Some(1_000), 500), "an event from the future is not live");
        assert!(!is_live(None, 1_000));
    }

    #[test]
    fn the_ping_animation_is_stepped_in_four_frames() {
        let normal = AnimationIntensity::Normal;
        assert_eq!(ping(0, normal), (0.7, 0.9));
        let frames: std::collections::BTreeSet<u32> = (0..1600).step_by(10).map(|t| (ping(t, normal).0 * 100.0).round() as u32).collect();
        assert_eq!(frames.len(), 4, "steps(4): {frames:?}");
        assert_eq!(ping(400, normal).0, 0.95);
        assert_eq!(ping(1500, normal).0, 1.45);
        // low motion halves the speed; off is a static ring
        assert_eq!(ping(800, AnimationIntensity::Low).0, 0.95, "low motion runs at half speed");
        assert_eq!(ping(1600, AnimationIntensity::Low).0, 1.2);
        assert_eq!(ping(12_345, AnimationIntensity::Off), (1.0, 1.0));
    }

    #[test]
    fn the_hero_paints_headlessly_in_every_state() {
        let ctx = egui::Context::default();
        super::super::install_fonts(&ctx);
        let agents = crate::mock::overview().agents;
        for (body, tag) in [(Body::Number("255.5M".into()), None), (Body::Unavailable, None), (Body::Error, None), (Body::Pending, None), (Body::Number("1.2T".into()), Some("PAUSED"))] {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let p = Props {
                        tokens: Tokens::for_theme(crate::view::Theme::Dark),
                        compact: true,
                        num_scale: 1.0,
                        body: body.clone(),
                        live: true,
                        period_label: "TODAY",
                        agents: &agents,
                        now_ms: crate::mock::NOW_MS,
                        time_ms: 0,
                        intensity: AnimationIntensity::Normal,
                        tag,
                        pin_on: true,
                        fire: None,
                        floater: Some(Floater { text: "756K".into(), age: 0.4, magnitude: 0.5 }),
                        pop: 1.0,
                    };
                    let o = show(ui, Rect::from_min_size(Pos2::ZERO, Vec2::new(236.0, 208.0)), &p);
                    assert_eq!(o, Out::default(), "nothing is clicked in a headless frame");
                });
            });
            assert!(!out.shapes.is_empty());
        }
    }
}
