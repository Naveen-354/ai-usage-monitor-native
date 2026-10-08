//! The Accounts page: Agent -> Accounts -> Active account -> Usage.
//!
//! Each agent is one bordered section: its name, whether it can keep a second account (and, if not, why), then its accounts.
//! Every account shows whether it is the active one, whether it is signed in, and what it used today / this week / this month /
//! this year / in its lifetime, with the actions that make sense for it (use, sign in again, remove). Below the accounts: their
//! combined usage and a button to add one. The page only *draws* and reports what was clicked; the host does it and re-reads.
//!
//! An API key is never typed here: the only text field is the account's name, so no secret can end up in UI state.

use egui::{vec2, Key, Pos2, Rect, Sense, Ui, UiBuilder};

use super::paint::border;
use super::text::{Style, Weight};
use super::widgets::{agent_dot, field_box, fineprint_limited, h2_style, hint, put, tag_at, tag_size, Tone};
use super::wrap::{measure, wrap};
use super::{agent_color, blend, Tokens, BW};
use crate::motion::format::format_compact;
use crate::view::{AccountView, AccountsOverview, AgentAccountsView, AnimationIntensity, AuthStateView, SpanUsageView};

const PAD_X: f32 = 9.0;
const LINE_H: f32 = 13.5;
const BTN_H: f32 = 18.0;
const BAR_H: f32 = 23.5;
const AGENT_GAP: f32 = 10.0;
/// Space between two usage cells on one line.
const CELL_GAP: f32 = 16.0;
const FIELD_W: f32 = 230.0;
const FORM_H: f32 = 5.0 + 28.0 + 5.0 + LINE_H + 8.0;

/// What the user asked for. The host performs it and re-reads the accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Use { agent: String, account: String },
    Remove { agent: String, account: String },
    Add { agent: String, name: String, device_code: bool },
    Reauthenticate { agent: String, account: String },
    /// Ask every agent whether its accounts are still signed in.
    CheckAll,
}

/// The "add account" form that is open, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddForm {
    pub agent: String,
    pub name: String,
    pub device_code: bool,
    /// Has the name field been given the keyboard focus yet?
    pub focused: bool,
}

/// What belongs to the page itself rather than to the data: which removal is waiting for a second click, which form is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub confirm_remove: Option<(String, String)>,
    pub add_form: Option<AddForm>,
}

pub struct Props<'a> {
    pub tokens: Tokens,
    /// `None` until the first read, or when it failed (then `error` says why).
    pub data: Option<&'a AccountsOverview>,
    pub error: Option<&'a str>,
    /// The last thing that happened (a sign-in finished or failed), shown in a box under the heading.
    pub notice: Option<&'a str>,
    /// Was it a failure? (the box is then drawn in the alert colour)
    pub notice_is_error: bool,
    /// A sign-in check is running in the background.
    pub checking: bool,
}

/// A button drawn this frame. `key` names it (`codex-work-use`, `add-submit`, ...) so tests can click it.
#[derive(Debug, Clone, PartialEq)]
pub struct ButtonInfo {
    pub rect: Rect,
    pub key: String,
    pub label: String,
}

#[derive(Debug, Default)]
pub struct Out {
    pub actions: Vec<Action>,
    pub buttons: Vec<ButtonInfo>,
}

// ------------------------------------------------------------------------------------------------ text and measures

fn small() -> Style {
    Style::mono(10.0, Weight::W400).lh_px(LINE_H)
}

fn small_bold() -> Style {
    Style::mono(10.0, Weight::W700).lh_px(LINE_H)
}

fn label_style() -> Style {
    Style::mono(12.0, Weight::W700)
}

fn button_style() -> Style {
    Style::mono(10.0, Weight::W700).spacing_em(0.1).lh_px(LINE_H)
}

/// The five spans, in the order they are shown.
pub fn span_cells(u: &SpanUsageView) -> [(&'static str, String); 5] {
    let f = |n: u64| format_compact(n as f64, 3);
    [("TODAY", f(u.day)), ("WEEK", f(u.week)), ("MONTH", f(u.month)), ("YEAR", f(u.year)), ("LIFETIME", f(u.lifetime))]
}

/// Lays `widths` out left to right with `gap` between them, starting a new line when the next one would pass `max_w`.
/// Returns each item's (x, line) and the number of lines.
pub fn flow(widths: &[f32], gap: f32, max_w: f32) -> (Vec<(f32, usize)>, usize) {
    let (mut x, mut line) = (0.0f32, 0usize);
    let mut out = Vec::with_capacity(widths.len());
    for (i, w) in widths.iter().enumerate() {
        if i > 0 && x + w > max_w {
            x = 0.0;
            line += 1;
        }
        out.push((x, line));
        x += w + gap;
    }
    (out, line + 1)
}

fn lines_height(lines: usize) -> f32 {
    lines as f32 * LINE_H + lines.saturating_sub(1) as f32 * 4.0
}

/// The width of each "TODAY 232M" cell.
fn cell_widths(u: &SpanUsageView) -> Vec<f32> {
    span_cells(u).iter().map(|(l, v)| measure(&small(), l) + 5.0 + measure(&small_bold(), v)).collect()
}

fn usage_lines(u: Option<&SpanUsageView>, max_w: f32) -> usize {
    u.map_or(1, |u| flow(&cell_widths(u), CELL_GAP, max_w).1)
}

/// The sentence under an account that needs attention (never raw agent output).
fn detail_of(a: &AccountView, agent: &AgentAccountsView) -> Option<String> {
    if let Some(d) = &a.auth_detail {
        return Some(d.clone());
    }
    if a.is_default && a.auth.needs_sign_in() {
        return Some(format!("Sign in with {} itself: this app never touches an agent's own sign-in.", agent.name));
    }
    None
}

fn auth_tone(a: AuthStateView) -> Tone {
    match a {
        AuthStateView::Valid => Tone::Accent,
        AuthStateView::Expired | AuthStateView::NotLoggedIn => Tone::Warn,
        AuthStateView::Pending | AuthStateView::Unknown => Tone::Plain,
    }
}

/// Which actions an account offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offers {
    pub use_it: bool,
    pub sign_in_again: bool,
    pub remove: bool,
}

pub fn offers(a: &AccountView, agent: &AgentAccountsView) -> Offers {
    Offers {
        use_it: !a.active,
        sign_in_again: a.auth.needs_sign_in() && !a.is_default && agent.switching.supported && agent.installed,
        remove: !a.is_default,
    }
}

fn is_confirming(state: &State, agent: &AgentAccountsView, a: &AccountView) -> bool {
    state.confirm_remove.as_ref().is_some_and(|(ag, ac)| ag == &agent.agent_id && ac == &a.account_id)
}

fn account_height(a: &AccountView, agent: &AgentAccountsView, inner_w: f32, confirming: bool) -> f32 {
    let mut h = 7.0 + 18.9 + 5.0 + lines_height(usage_lines(a.usage.as_ref(), inner_w));
    if let Some(d) = detail_of(a, agent) {
        h += 5.0 + wrap(&d, &small(), inner_w, true).len() as f32 * LINE_H;
    }
    let o = offers(a, agent);
    if confirming || o.use_it || o.sign_in_again || o.remove {
        h += 5.0 + BTN_H;
    }
    h + 8.0
}

fn show_total_line(a: &AgentAccountsView) -> bool {
    a.accounts.len() > 1 || a.removed_usage.is_some()
}

fn total_height(agent: &AgentAccountsView, inner_w: f32) -> f32 {
    if show_total_line(agent) {
        7.0 + 18.9 + 5.0 + lines_height(usage_lines(agent.total.as_ref(), inner_w)) + 8.0
    } else {
        0.0
    }
}

fn footer_height(agent: &AgentAccountsView, form_open: bool) -> f32 {
    if form_open {
        FORM_H
    } else if agent.switching.supported {
        8.0 + BTN_H + 8.0
    } else {
        0.0
    }
}

fn note_lines(agent: &AgentAccountsView, inner_w: f32) -> Vec<String> {
    match (agent.switching.supported, &agent.switching.reason) {
        (false, Some(reason)) => wrap(reason, &small(), inner_w, true),
        _ => Vec::new(),
    }
}

fn agent_height(agent: &AgentAccountsView, inner_w: f32, state: &State) -> f32 {
    let notes = note_lines(agent, inner_w);
    let form_open = state.add_form.as_ref().is_some_and(|f| f.agent == agent.agent_id);
    let mut h = 2.0 * BW + 8.0 + 20.0;
    if !notes.is_empty() {
        h += 5.0 + notes.len() as f32 * LINE_H;
    }
    h += 6.0;
    for a in &agent.accounts {
        h += 1.0 + account_height(a, agent, inner_w, is_confirming(state, agent, a));
    }
    h + total_height(agent, inner_w) + footer_height(agent, form_open)
}

// ------------------------------------------------------------------------------------------------ painting

fn button_width(text: &str) -> f32 {
    measure(&button_style(), text) + 2.0 * 8.0 + 2.0 * BW
}

/// Like [`button`] but placed by its left edge. Returns (clicked, the x where the next button to its right should start).
#[allow(clippy::too_many_arguments)]
fn button_at(ui: &mut Ui, t: &Tokens, buttons: &mut Vec<ButtonInfo>, left: f32, top: f32, text: &str, danger: bool, key: &str) -> (bool, f32) {
    let (clicked, _) = button(ui, t, buttons, left + button_width(text), top, text, danger, key);
    (clicked, left + button_width(text) + 6.0)
}

/// A small bordered button whose right edge is at `right`. Records it in `buttons` and returns (was it clicked, the x its left
/// edge ended up at minus a gap - where the next button to its left should end).
#[allow(clippy::too_many_arguments)]
fn button(ui: &mut Ui, t: &Tokens, buttons: &mut Vec<ButtonInfo>, right: f32, top: f32, text: &str, danger: bool, key: &str) -> (bool, f32) {
    let st = button_style();
    let rect = Rect::from_min_size(Pos2::new(right - button_width(text), top), vec2(button_width(text), BTN_H));
    let resp = ui.interact(rect, ui.id().with(("accounts-button", key)), Sense::click());
    let color = if danger { t.alert } else { t.ink };
    let hovered = resp.hovered();
    let p = ui.painter();
    if hovered {
        p.rect_filled(rect, 0.0, color);
    }
    border(p, rect, BW, color);
    put(ui, text, &st, rect.left() + BW + 8.0, rect.top() + BW + (BTN_H - 2.0 * BW - st.line_h) / 2.0, if hovered { t.bg } else { color });
    buttons.push(ButtonInfo { rect, key: key.to_string(), label: text.to_string() });
    (resp.clicked(), rect.left() - 6.0)
}

fn paint_usage(ui: &Ui, t: &Tokens, left: f32, top: f32, max_w: f32, u: Option<&SpanUsageView>) {
    let Some(u) = u else {
        put(ui, "NO USAGE RECORDED", &small(), left, top, t.dim);
        return;
    };
    let (pos, _) = flow(&cell_widths(u), CELL_GAP, max_w);
    for ((label, value), (x, line)) in span_cells(u).iter().zip(pos) {
        let y = top + line as f32 * (LINE_H + 4.0);
        let lw = put(ui, label, &small(), left + x, y, t.dim);
        put(ui, value, &small_bold(), left + x + lw + 5.0, y, t.ink);
    }
}

#[allow(clippy::too_many_arguments)]
fn account_row(ui: &mut Ui, t: &Tokens, out: &mut Out, state: &mut State, agent: &AgentAccountsView, a: &AccountView, left: f32, top: f32, inner_w: f32) {
    let right = left + inner_w;
    let y = top + 7.0;
    let confirming = is_confirming(state, agent, a);
    let height = account_height(a, agent, inner_w, confirming);
    if a.active {
        // a solid bar down the left edge marks the account the agent is using
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(left - PAD_X, top), vec2(4.0, height)), 0.0, t.ok);
    }

    // head: [ACTIVE] label  identity ........ [SIGNED IN]
    let mut x = left;
    if a.active {
        let size = tag_size(ui.ctx(), "ACTIVE");
        x += tag_at(ui, t, Pos2::new(x, y + (18.9 - size.y) / 2.0), "ACTIVE", Tone::Accent).x + 8.0;
    }
    x += put(ui, &a.label, &label_style(), x, y + (18.9 - label_style().line_h) / 2.0, t.ink) + 8.0;
    let auth_size = tag_size(ui.ctx(), a.auth.label());
    let tag_left = right - auth_size.x;
    tag_at(ui, t, Pos2::new(tag_left, y + (18.9 - auth_size.y) / 2.0), a.auth.label(), auth_tone(a.auth));
    let room = tag_left - 8.0 - x;
    let who = a.identity.clone().or_else(|| a.is_default.then(|| "the agent's own sign-in".to_string()));
    if let Some(who) = who.filter(|w| measure(&small(), w) <= room) {
        put(ui, &who, &small(), x, y + (18.9 - LINE_H) / 2.0, t.dim);
    }

    // usage
    let usage_top = y + 18.9 + 5.0;
    paint_usage(ui, t, left, usage_top, inner_w, a.usage.as_ref());
    let mut bottom = usage_top + lines_height(usage_lines(a.usage.as_ref(), inner_w));

    if let Some(d) = detail_of(a, agent) {
        bottom += 5.0;
        let color = if a.auth.needs_sign_in() { t.alert } else { t.dim };
        for line in wrap(&d, &small(), inner_w, true) {
            put(ui, &line, &small(), left, bottom, color);
            bottom += LINE_H;
        }
    }

    // actions, right-aligned
    let o = offers(a, agent);
    let key = |what: &str| format!("{}-{}-{what}", agent.agent_id, a.account_id);
    let (agent_id, account_id) = (agent.agent_id.clone(), a.account_id.clone());
    let btn_top = bottom + 5.0;
    if confirming {
        let (cancel, r) = button(ui, t, &mut out.buttons, right, btn_top, "CANCEL", false, &key("cancel"));
        let (yes, r) = button(ui, t, &mut out.buttons, r, btn_top, "YES, REMOVE", true, &key("confirm"));
        if yes {
            out.actions.push(Action::Remove { agent: agent_id, account: account_id });
            state.confirm_remove = None;
        } else if cancel {
            state.confirm_remove = None;
        }
        let msg = "Deletes this account's sign-in folder. Its usage history stays.";
        if measure(&small(), msg) <= r - left {
            put(ui, msg, &small(), left, btn_top + (BTN_H - LINE_H) / 2.0, t.alert);
        }
    } else if o.use_it || o.sign_in_again || o.remove {
        let mut r = right;
        if o.remove {
            let (clicked, next) = button(ui, t, &mut out.buttons, r, btn_top, "REMOVE", true, &key("remove"));
            if clicked {
                state.confirm_remove = Some((agent_id.clone(), account_id.clone()));
            }
            r = next;
        }
        if o.sign_in_again {
            let (clicked, next) = button(ui, t, &mut out.buttons, r, btn_top, "SIGN IN AGAIN", false, &key("signin"));
            if clicked {
                out.actions.push(Action::Reauthenticate { agent: agent_id.clone(), account: account_id.clone() });
            }
            r = next;
        }
        if o.use_it {
            let (clicked, _) = button(ui, t, &mut out.buttons, r, btn_top, "USE", false, &key("use"));
            if clicked {
                out.actions.push(Action::Use { agent: agent_id, account: account_id });
            }
        }
    }
}

fn paint_total(ui: &Ui, t: &Tokens, agent: &AgentAccountsView, left: f32, top: f32, inner_w: f32) {
    let y = top + 7.0;
    put(ui, "ALL ACCOUNTS", &label_style(), left, y + (18.9 - label_style().line_h) / 2.0, t.ink);
    if agent.removed_usage.is_some() {
        let w = measure(&label_style(), "ALL ACCOUNTS") + 8.0;
        put(ui, "(includes removed accounts)", &small(), left + w, y + (18.9 - LINE_H) / 2.0, t.dim);
    }
    paint_usage(ui, t, left, y + 18.9 + 5.0, inner_w, agent.total.as_ref());
}

#[allow(clippy::too_many_arguments)]
fn footer(ui: &mut Ui, t: &Tokens, out: &mut Out, state: &mut State, agent: &AgentAccountsView, left: f32, top: f32) {
    // Only this agent's own form is taken out of the state; another agent's open form is left exactly where it is.
    let own_form_open = state.add_form.as_ref().is_some_and(|f| f.agent == agent.agent_id);
    let Some(mut form) = own_form_open.then(|| state.add_form.take()).flatten() else {
        if !agent.switching.supported {
            return;
        }
        let y = top + 8.0;
        if agent.installed {
            let (clicked, _) = button_at(ui, t, &mut out.buttons, left, y, "+ ADD ACCOUNT", false, &format!("{}-add", agent.agent_id));
            if clicked {
                state.add_form = Some(AddForm { agent: agent.agent_id.clone(), name: format!("Account {}", agent.accounts.len()), device_code: false, focused: false });
            }
        } else {
            put(ui, &format!("Install {} to add accounts.", agent.name), &small(), left, y + (BTN_H - LINE_H) / 2.0, t.dim);
        }
        return;
    };

    // the open form: a name field, the sign-in button, cancel, and (where offered) the device-code switch
    let y = top + 5.0;
    let field = Rect::from_min_size(Pos2::new(left, y), vec2(FIELD_W, 28.0));
    field_box(ui, t, field);
    let st = Style::mono(12.0, Weight::W400);
    let mut child = ui.new_child(UiBuilder::new().max_rect(field.shrink2(vec2(8.0, 5.0))));
    let edit = egui::TextEdit::singleline(&mut form.name)
        .font(st.font_id())
        .frame(false)
        .desired_width(FIELD_W - 16.0)
        .text_color(t.ink)
        .hint_text(egui::RichText::new("a name, e.g. Work").color(t.dim).font(st.font_id()));
    let resp = child.add(edit);
    if !form.focused {
        resp.request_focus();
        form.focused = true;
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let escape = ui.input(|i| i.key_pressed(Key::Escape));

    let btn_top = y + (28.0 - BTN_H) / 2.0;
    let mut close = escape;
    let can_submit = !form.name.trim().is_empty();
    // SIGN IN, CANCEL and the device-code switch, left to right after the field
    let (sign_in, x) = button_at(ui, t, &mut out.buttons, field.right() + 10.0, btn_top, "SIGN IN", false, "add-submit");
    let (cancel, x) = button_at(ui, t, &mut out.buttons, x, btn_top, "CANCEL", false, "add-cancel");
    if agent.login_methods.iter().any(|m| m == "device-code") {
        let text = if form.device_code { "DEVICE CODE: ON" } else { "DEVICE CODE: OFF" };
        let (toggled, _) = button_at(ui, t, &mut out.buttons, x, btn_top, text, false, "add-device");
        if toggled {
            form.device_code = !form.device_code;
        }
    }
    if (sign_in || enter) && can_submit {
        out.actions.push(Action::Add { agent: agent.agent_id.clone(), name: form.name.trim().to_string(), device_code: form.device_code });
        close = true;
    }
    if cancel {
        close = true;
    }
    put(ui, "A console window opens for the agent's own sign-in; finish it there.", &small(), left, y + 28.0 + 5.0, t.dim);
    if !close {
        state.add_form = Some(form);
    }
}

#[allow(clippy::too_many_arguments)]
fn agent_section(ui: &mut Ui, t: &Tokens, out: &mut Out, state: &mut State, agent: &AgentAccountsView, now_time_ms: u64) {
    let width = ui.available_width();
    let inner_w = width - 2.0 * BW - 2.0 * PAD_X;
    let height = agent_height(agent, inner_w, state);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let left = rect.left() + BW + PAD_X;
    let mut y = rect.top() + BW + 8.0;
    border(ui.painter(), rect, BW, t.ink);

    // head: dot, name ........ tags
    agent_dot(ui, Pos2::new(left, y + 10.0 - 4.5), agent_color(&agent.color), !agent.installed, false, now_time_ms, AnimationIntensity::Off);
    let name_style = Style::display(12.0).spacing_em(0.06);
    put(ui, &agent.name.to_uppercase(), &name_style, left + 17.0, y + (20.0 - name_style.line_h) / 2.0, if agent.installed { t.ink } else { t.dim });
    let mut tag_right = left + inner_w;
    let mut tag = |ui: &Ui, text: &str, tone: Tone| {
        let size = tag_size(ui.ctx(), text);
        tag_at(ui, t, Pos2::new(tag_right - size.x, y + (20.0 - size.y) / 2.0), text, tone);
        tag_right -= size.x + 6.0;
    };
    if agent.switching.supported {
        tag(ui, &format!("ACCOUNTS VIA {}", agent.switching.mechanism.clone().unwrap_or_default()), Tone::Plain);
    } else {
        tag(ui, "MONITOR ONLY", Tone::Warn);
    }
    if !agent.installed {
        tag(ui, "NOT INSTALLED", Tone::Plain);
    }
    y += 20.0;
    let notes = note_lines(agent, inner_w);
    if !notes.is_empty() {
        y += 5.0;
        for line in &notes {
            put(ui, line, &small(), left, y, t.dim);
            y += LINE_H;
        }
    }
    y += 6.0;

    let hairline = blend(t.bg, t.ink, 0.35);
    for a in &agent.accounts {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left() + BW, y), vec2(width - 2.0 * BW, 1.0)), 0.0, hairline);
        y += 1.0;
        account_row(ui, t, out, state, agent, a, left, y, inner_w);
        y += account_height(a, agent, inner_w, is_confirming(state, agent, a));
    }
    if show_total_line(agent) {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(rect.left() + BW, y), vec2(width - 2.0 * BW, 1.0)), 0.0, hairline);
        paint_total(ui, t, agent, left, y, inner_w);
        y += total_height(agent, inner_w);
    }
    footer(ui, t, out, state, agent, left, y);
}

/// Paints the page into the current `ui` (full available width) and returns what the user did.
pub fn show(ui: &mut Ui, p: &Props, state: &mut State) -> Out {
    let t = p.tokens;
    let mut out = Out::default();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    let width = ui.available_width();

    // ACCOUNTS ............................................ [CHECK SIGN-INS]
    let (bar, _) = ui.allocate_exact_size(vec2(width, BAR_H), Sense::hover());
    let h2 = h2_style();
    put(ui, "ACCOUNTS", &h2, bar.left(), bar.top() + (BAR_H - h2.line_h) / 2.0, t.ink);
    let (clicked, _) = button(ui, &t, &mut out.buttons, bar.right(), bar.top() + (BAR_H - BTN_H) / 2.0, if p.checking { "CHECKING..." } else { "CHECK SIGN-INS" }, false, "check-all");
    if clicked && !p.checking {
        out.actions.push(Action::CheckAll);
    }
    fineprint_limited(
        ui,
        &t,
        "Each agent keeps its accounts apart. The ACTIVE one is what `agm run <agent>` starts; switching only changes which folder the next launch uses. Usage is read from each account's own folder.",
        62.0 * measure(&hint(), "0"),
    );

    if let Some(msg) = p.notice {
        ui.add_space(4.0);
        let lines = wrap(msg, &small(), width - 2.0 * BW - 2.0 * PAD_X, true);
        let (r, _) = ui.allocate_exact_size(vec2(width, 2.0 * BW + 12.0 + lines.len() as f32 * LINE_H), Sense::hover());
        border(ui.painter(), r, BW, if p.notice_is_error { t.alert } else { t.ok });
        for (i, l) in lines.iter().enumerate() {
            put(ui, l, &small(), r.left() + BW + PAD_X, r.top() + BW + 6.0 + i as f32 * LINE_H, t.ink);
        }
        ui.add_space(8.0);
    }

    let Some(data) = p.data else {
        let msg = p.error.map_or("Reading the accounts...".to_string(), |e| format!("TOKEN DATA UNAVAILABLE: {e}"));
        ui.add_space(8.0);
        put_wrapped(ui, &t, &msg, if p.error.is_some() { t.alert } else { t.dim });
        return out;
    };

    // everything, across every agent and account
    ui.add_space(6.0);
    let inner_w = width - 2.0 * BW - 2.0 * PAD_X;
    let total_h = 2.0 * BW + 7.0 + LINE_H + 5.0 + lines_height(usage_lines(data.total.as_ref(), inner_w)) + 8.0;
    let (r, _) = ui.allocate_exact_size(vec2(width, total_h), Sense::hover());
    border(ui.painter(), r, BW, t.ink);
    put(ui, "ALL AGENTS, ALL ACCOUNTS", &small_bold(), r.left() + BW + PAD_X, r.top() + BW + 7.0, t.dim);
    paint_usage(ui, &t, r.left() + BW + PAD_X, r.top() + BW + 7.0 + LINE_H + 5.0, inner_w, data.total.as_ref());
    ui.add_space(AGENT_GAP + 8.0);

    let time_ms = ui.input(|i| (i.time * 1000.0) as u64);
    for (i, agent) in data.agents.iter().enumerate() {
        if i > 0 {
            ui.add_space(AGENT_GAP);
        }
        agent_section(ui, &t, &mut out, state, agent, time_ms);
    }
    out
}

fn put_wrapped(ui: &mut Ui, t: &Tokens, text: &str, color: egui::Color32) {
    let _ = t;
    let width = ui.available_width();
    let lines = wrap(text, &small(), width, true);
    let (r, _) = ui.allocate_exact_size(vec2(width, lines.len() as f32 * LINE_H), Sense::hover());
    for (i, l) in lines.iter().enumerate() {
        put(ui, l, &small(), r.left(), r.top() + i as f32 * LINE_H, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;
    use crate::view::Theme;
    use egui::{Event, Modifiers, PointerButton};

    fn account(id: &str, active: bool, default: bool, auth: AuthStateView) -> AccountView {
        AccountView {
            account_id: id.into(),
            label: id.to_uppercase(),
            is_default: default,
            auth,
            auth_detail: None,
            identity: None,
            active,
            checked_utc_ms: None,
            last_used_utc_ms: None,
            usage: Some(SpanUsageView { day: 1, week: 2, month: 3, year: 4, lifetime: 5 }),
        }
    }

    fn agent(installed: bool, supported: bool) -> AgentAccountsView {
        AgentAccountsView {
            agent_id: "codex".into(),
            name: "Codex".into(),
            color: "#00c2a8".into(),
            installed,
            switching: crate::view::SwitchingView { supported, mechanism: supported.then(|| "CODEX_HOME".into()), reason: (!supported).then(|| "no supported way".into()) },
            login_methods: if supported { vec!["standard".into(), "device-code".into()] } else { vec![] },
            active: Some("default".into()),
            accounts: vec![],
            removed_usage: None,
            total: None,
        }
    }

    #[test]
    fn cells_wrap_to_new_lines_only_when_they_do_not_fit() {
        let (pos, lines) = flow(&[50.0, 50.0, 50.0], 10.0, 200.0);
        assert_eq!((lines, pos), (1, vec![(0.0, 0), (60.0, 0), (120.0, 0)]));
        let (pos, lines) = flow(&[50.0, 50.0, 50.0], 10.0, 100.0);
        assert_eq!((lines, pos[2]), (3, (0.0, 2)), "50 + 10 + 50 does not fit in 100, so each gets its own line: {pos:?}");
        let (pos, lines) = flow(&[50.0, 50.0, 50.0], 10.0, 130.0);
        assert_eq!((lines, pos[1], pos[2]), (2, (60.0, 0), (0.0, 1)), "two fit, the third wraps");
        assert_eq!(flow(&[], 10.0, 100.0).1, 1);
        assert_eq!(flow(&[500.0], 10.0, 100.0).1, 1, "one cell wider than the line still gets a line");
    }

    #[test]
    fn the_five_spans_are_shown_in_the_order_of_the_brief_with_short_numbers() {
        let c = span_cells(&SpanUsageView { day: 232_300_000, week: 700_000_000, month: 2_100_000_000, year: 4_900_000_000, lifetime: 5_700_000_000 });
        let labels: Vec<&str> = c.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["TODAY", "WEEK", "MONTH", "YEAR", "LIFETIME"]);
        assert_eq!(c[0].1, "232M");
        assert_eq!(c[2].1, "2.1B");
    }

    #[test]
    fn each_account_offers_exactly_the_actions_that_make_sense() {
        let a = agent(true, true);
        let own = offers(&account("default", true, true, AuthStateView::Valid), &a);
        assert_eq!(own, Offers { use_it: false, sign_in_again: false, remove: false }, "the active default account: nothing to do");
        let other_default = offers(&account("default", false, true, AuthStateView::Unknown), &a);
        assert_eq!(other_default, Offers { use_it: true, sign_in_again: false, remove: false }, "the agent's own sign-in is never removed or re-authenticated here");
        let expired = offers(&account("work", false, false, AuthStateView::Expired), &a);
        assert_eq!(expired, Offers { use_it: true, sign_in_again: true, remove: true });
        let fine = offers(&account("work", true, false, AuthStateView::Valid), &a);
        assert_eq!(fine, Offers { use_it: false, sign_in_again: false, remove: true });
        let not_installed = offers(&account("work", false, false, AuthStateView::Expired), &agent(false, true));
        assert!(!not_installed.sign_in_again, "cannot sign in without the agent");
        let monitor_only = offers(&account("work", false, false, AuthStateView::Expired), &agent(true, false));
        assert!(!monitor_only.sign_in_again);
    }

    #[test]
    fn a_default_account_that_needs_signing_in_explains_why_the_app_will_not_do_it() {
        let a = agent(true, true);
        let d = detail_of(&account("default", true, true, AuthStateView::NotLoggedIn), &a).unwrap();
        assert!(d.contains("Sign in with Codex itself") && d.contains("never touches"), "{d}");
        assert!(detail_of(&account("default", true, true, AuthStateView::Valid), &a).is_none());
    }

    #[test]
    fn a_taller_section_is_reserved_for_a_monitor_only_note_a_confirmation_and_an_open_form() {
        let w = 700.0;
        let mut a = agent(true, true);
        a.accounts = vec![account("default", true, true, AuthStateView::Valid), account("work", false, false, AuthStateView::Valid)];
        let plain = agent_height(&a, w, &State::default());
        let confirming = agent_height(&a, w, &State { confirm_remove: Some(("codex".into(), "work".into())), add_form: None });
        assert_eq!(confirming, plain, "the work row already had a REMOVE button row; confirming replaces it");
        let form = agent_height(&a, w, &State { confirm_remove: None, add_form: Some(AddForm { agent: "codex".into(), ..Default::default() }) });
        assert!(form > plain, "the form needs more room than the add button");
        let other_agents_form = agent_height(&a, w, &State { confirm_remove: None, add_form: Some(AddForm { agent: "claude".into(), ..Default::default() }) });
        assert_eq!(other_agents_form, plain, "another agent's form does not change this section");
        let mut m = agent(true, false);
        m.accounts = vec![account("default", true, true, AuthStateView::Unknown)];
        let mut m_no_reason = m.clone();
        m_no_reason.switching.reason = None;
        assert!(agent_height(&m, w, &State::default()) > agent_height(&m_no_reason, w, &State::default()), "the reason takes lines");
    }

    // -------------------------------------------------------------------------------------------- drawing and clicking

    fn ctx() -> egui::Context {
        let c = egui::Context::default();
        crate::web::install_fonts(&c);
        let _ = c.run(egui::RawInput::default(), |_| {});
        c
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 3000.0))), events, ..Default::default() }
    }

    /// Runs `frames` (each a list of input events) over the same page state and returns every frame's output.
    fn drive(ctx: &egui::Context, data: Option<&AccountsOverview>, state: &mut State, frames: Vec<Vec<Event>>) -> Vec<Out> {
        let t = Tokens::for_theme(Theme::Dark);
        frames
            .into_iter()
            .map(|events| {
                let mut out = Out::default();
                let _ = ctx.run(input(events), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(812.0);
                        out = show(ui, &Props { tokens: t, data, error: None, notice: None, notice_is_error: false, checking: false }, state);
                    });
                });
                out
            })
            .collect()
    }

    fn click_events(at: Pos2) -> Vec<Vec<Event>> {
        vec![
            vec![Event::PointerMoved(at)],
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        ]
    }

    /// Lays the page out, then clicks the button called `key` and returns what the page asked for.
    fn click(data: &AccountsOverview, state: &mut State, key: &str) -> Vec<Action> {
        let ctx = ctx();
        let first = drive(&ctx, Some(data), state, vec![vec![]]).remove(0);
        let b = first.buttons.iter().find(|b| b.key == key).unwrap_or_else(|| panic!("no button '{key}'; there are {:?}", first.buttons.iter().map(|b| &b.key).collect::<Vec<_>>()));
        let at = b.rect.center();
        drive(&ctx, Some(data), state, click_events(at)).into_iter().flat_map(|o| o.actions).collect()
    }

    #[test]
    fn the_fixture_paints_in_both_themes_and_when_empty_or_failed() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        for theme in [Theme::Dark, Theme::Light] {
            let t = Tokens::for_theme(theme);
            let out = ctx.run(input(vec![]), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(812.0);
                    show(ui, &Props { tokens: t, data: Some(&data), error: None, notice: Some("codex/work is signed in."), notice_is_error: false, checking: true }, &mut State::default());
                });
            });
            assert!(out.shapes.len() > 400, "{theme:?}: {}", out.shapes.len());
        }
        for (data, error) in [(None, None), (None, Some("the database is locked")), (Some(&AccountsOverview::default()), None)] {
            let t = Tokens::for_theme(Theme::Dark);
            let out = ctx.run(input(vec![]), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, &Props { tokens: t, data, error, notice: None, notice_is_error: false, checking: false }, &mut State::default());
                });
            });
            assert!(out.shapes.len() > 5);
        }
    }

    #[test]
    fn it_paints_at_the_narrowest_window_without_panicking() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let t = Tokens::for_theme(Theme::Dark);
        let out = ctx.run(input(vec![]), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(380.0);
                show(ui, &Props { tokens: t, data: Some(&data), error: None, notice: None, notice_is_error: false, checking: false }, &mut State::default());
            });
        });
        assert!(out.shapes.len() > 100);
    }

    #[test]
    fn clicking_use_asks_the_host_to_switch_and_only_inactive_accounts_have_the_button() {
        let data = mock::accounts_overview();
        let actions = click(&data, &mut State::default(), "codex-personal-use");
        assert_eq!(actions, vec![Action::Use { agent: "codex".into(), account: "personal".into() }]);
        let ctx = ctx();
        let first = drive(&ctx, Some(&data), &mut State::default(), vec![vec![]]).remove(0);
        assert!(first.buttons.iter().all(|b| b.key != "codex-work-use"), "the active account has no USE button");
        assert!(first.buttons.iter().any(|b| b.key == "codex-default-use"));
    }

    #[test]
    fn signing_in_again_is_offered_for_the_expired_account_only() {
        let data = mock::accounts_overview();
        assert_eq!(click(&data, &mut State::default(), "codex-personal-signin"), vec![Action::Reauthenticate { agent: "codex".into(), account: "personal".into() }]);
        let ctx = ctx();
        let first = drive(&ctx, Some(&data), &mut State::default(), vec![vec![]]).remove(0);
        assert!(first.buttons.iter().all(|b| b.key != "codex-work-signin" && b.key != "claude-team-signin"), "signed-in accounts are not offered it");
    }

    #[test]
    fn removing_takes_two_clicks_and_the_second_one_is_explicit() {
        let data = mock::accounts_overview();
        let mut state = State::default();
        let actions = click(&data, &mut state, "codex-work-remove");
        assert!(actions.is_empty(), "the first click only asks: nothing is removed yet");
        assert_eq!(state.confirm_remove, Some(("codex".into(), "work".into())));
        // the same page state, now in the confirming state
        let actions = click(&data, &mut state, "codex-work-confirm");
        assert_eq!(actions, vec![Action::Remove { agent: "codex".into(), account: "work".into() }]);
        assert_eq!(state.confirm_remove, None);
    }

    #[test]
    fn cancelling_a_removal_changes_nothing() {
        let data = mock::accounts_overview();
        let mut state = State { confirm_remove: Some(("codex".into(), "work".into())), add_form: None };
        let actions = click(&data, &mut state, "codex-work-cancel");
        assert!(actions.is_empty());
        assert_eq!(state.confirm_remove, None);
    }

    #[test]
    fn the_default_account_has_no_remove_button_at_all() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let first = drive(&ctx, Some(&data), &mut State::default(), vec![vec![]]).remove(0);
        assert!(first.buttons.iter().all(|b| !b.key.ends_with("default-remove")), "{:?}", first.buttons.iter().map(|b| &b.key).collect::<Vec<_>>());
    }

    #[test]
    fn a_monitor_only_agent_has_no_add_button_and_an_uninstalled_one_says_to_install_it() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let first = drive(&ctx, Some(&data), &mut State::default(), vec![vec![]]).remove(0);
        let keys: Vec<&str> = first.buttons.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.contains(&"codex-add") && keys.contains(&"claude-add"));
        for gone in ["antigravity-add", "ollama-add", "aider-add"] {
            assert!(!keys.contains(&gone), "{gone}: monitor-only agents cannot get an account");
        }
        assert!(!keys.contains(&"gemini-add"), "gemini is not installed in the fixture, so there is nothing to add to");
    }

    #[test]
    fn adding_an_account_opens_a_form_prefilled_with_a_name_and_submitting_it_asks_the_host() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let mut state = State::default();
        // open the form
        let first = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0);
        let add = first.buttons.iter().find(|b| b.key == "codex-add").unwrap().rect.center();
        drive(&ctx, Some(&data), &mut state, click_events(add));
        let form = state.add_form.clone().expect("the form is open");
        assert_eq!((form.agent.as_str(), form.name.as_str(), form.device_code), ("codex", "Account 3", false));
        // submit it
        let laid_out = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0);
        let submit = laid_out.buttons.iter().find(|b| b.key == "add-submit").unwrap().rect.center();
        let actions: Vec<Action> = drive(&ctx, Some(&data), &mut state, click_events(submit)).into_iter().flat_map(|o| o.actions).collect();
        assert_eq!(actions, vec![Action::Add { agent: "codex".into(), name: "Account 3".into(), device_code: false }]);
        assert!(state.add_form.is_none(), "the form closes once submitted");
    }

    #[test]
    fn the_form_can_be_cancelled_and_will_not_submit_an_empty_name() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let mut state = State { confirm_remove: None, add_form: Some(AddForm { agent: "codex".into(), name: "   ".into(), device_code: true, focused: true }) };
        let laid_out = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0);
        let submit = laid_out.buttons.iter().find(|b| b.key == "add-submit").unwrap().rect.center();
        let actions: Vec<Action> = drive(&ctx, Some(&data), &mut state, click_events(submit)).into_iter().flat_map(|o| o.actions).collect();
        assert!(actions.is_empty(), "a blank name is not a name");
        assert!(state.add_form.is_some(), "and the form stays open so it can be fixed");
        let cancel = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0).buttons.iter().find(|b| b.key == "add-cancel").unwrap().rect.center();
        drive(&ctx, Some(&data), &mut state, click_events(cancel));
        assert!(state.add_form.is_none());
    }

    #[test]
    fn the_device_code_switch_toggles_and_is_passed_on() {
        let data = mock::accounts_overview();
        let ctx = ctx();
        let mut state = State { confirm_remove: None, add_form: Some(AddForm { agent: "codex".into(), name: "Phone".into(), device_code: false, focused: true }) };
        // the fixture offers only "standard"; give codex the device-code method too
        let mut data = data;
        data.agents[0].login_methods = vec!["standard".into(), "device-code".into()];
        let laid_out = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0);
        let toggle = laid_out.buttons.iter().find(|b| b.key == "add-device").expect("the switch is offered").rect.center();
        drive(&ctx, Some(&data), &mut state, click_events(toggle));
        assert!(state.add_form.as_ref().unwrap().device_code);
        let submit = drive(&ctx, Some(&data), &mut state, vec![vec![]]).remove(0).buttons.iter().find(|b| b.key == "add-submit").unwrap().rect.center();
        let actions: Vec<Action> = drive(&ctx, Some(&data), &mut state, click_events(submit)).into_iter().flat_map(|o| o.actions).collect();
        assert_eq!(actions, vec![Action::Add { agent: "codex".into(), name: "Phone".into(), device_code: true }]);
    }

    #[test]
    fn check_sign_ins_is_a_button_and_is_inert_while_a_check_is_running() {
        let data = mock::accounts_overview();
        assert_eq!(click(&data, &mut State::default(), "check-all"), vec![Action::CheckAll]);
        // while checking, the same button is labelled and does nothing
        let ctx = ctx();
        let t = Tokens::for_theme(Theme::Dark);
        let mut found = None;
        let mut state = State::default();
        for events in vec![vec![]].into_iter().chain(click_events(Pos2::new(800.0, 20.0))) {
            let _ = ctx.run(input(events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(812.0);
                    let out = show(ui, &Props { tokens: t, data: Some(&data), error: None, notice: None, notice_is_error: false, checking: true }, &mut state);
                    assert!(out.actions.is_empty(), "no action while a check is running");
                    found = out.buttons.iter().find(|b| b.key == "check-all").map(|b| b.label.clone());
                });
            });
        }
        assert_eq!(found.as_deref(), Some("CHECKING..."));
    }

    #[test]
    fn nothing_on_the_page_can_hold_a_secret() {
        // the page's only text input is the account name; API keys are not offered, so there is nothing to leak into UI state
        let src = include_str!("accounts.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        assert_eq!(body.matches("TextEdit::").count(), 1, "exactly one text field");
        assert!(!body.contains("password(true)") && !body.to_lowercase().contains("api_key") && !body.to_lowercase().contains("apikey"));
    }
}
