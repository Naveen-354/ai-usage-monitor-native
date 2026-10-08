//! The eframe application: compact overlay, expanded overlay and the main window, wired to the backend.
//! Everything that is drawn comes from `native_ui::web`, the port of the old web UI.

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app_api::api::{BackendEvent, SettingsPatch};
use app_api::store::AppStore;
use app_api::view::{self, AnimationIntensity, AppInfo, Corner, Settings, Theme};
use egui::{vec2, Color32, Pos2, Rect, ViewportCommand};
use fire_engine::engine::{FireEngine, FlameState};
use fire_engine::sim::{FIRE_H, FIRE_W};
use fire_engine::XorShift64;
use native_platform::platform::autostart::Autostart;
use native_platform::platform::overlay_window;
use native_platform::platform::tray::{tooltip_text, TrayAction, TrayState};
use native_ui::expanded::Page;
use native_ui::motion::counter::AnimatedCounter;
use native_ui::motion::format::{format_compact, panel_alpha, platform_alpha_floor};
use native_ui::theme;
use native_ui::web::all_agents::{self, Action as AgentsAction};
use native_ui::web::hero::{self, Body, HERO_DIGITS};
use native_ui::web::pages::{self, DiagnosticsData};
use native_ui::web::{with_alpha, Tokens};

use crate::backend::AppBackend;
use crate::bridge;
use crate::flame::{flame_state, motion_factor, next_period, period_word, pop_scale};
use crate::layout::default_compact_size;
use crate::placement::{self, Area};
use crate::shell::{HotkeyKind, OverlayShared, Shell, ShellEvent};

const EXPANDED_SIZE: (f32, f32) = (480.0, 680.0);
const POSITION_SAVE_DELAY: Duration = Duration::from_millis(700);
const STEPPED_PULSE: Duration = Duration::from_millis(500);
/// How long the floating +756K label animates (delta-float, 1.6 s).
const FLOATER_MS: f32 = 1600.0;
/// The main window of the old app: 1040 x 740, never smaller than 780 x 540.
const MAIN_SIZE: [f32; 2] = [1040.0, 740.0];
const MAIN_MIN: [f32; 2] = [780.0, 540.0];

/// `AI_USAGE_MONITOR_MAIN_SCROLL=700` opens the main window already scrolled that far (used to photograph a long page).
fn main_scroll() -> Option<f32> {
    std::env::var("AI_USAGE_MONITOR_MAIN_SCROLL").ok()?.trim().parse().ok()
}

/// `AI_USAGE_MONITOR_MAIN_SIZE=1040x960` opens the main window at another size (used to photograph a page).
fn main_size() -> [f32; 2] {
    std::env::var("AI_USAGE_MONITOR_MAIN_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?])))
        .unwrap_or(MAIN_SIZE)
}

struct MainWindow {
    open: bool,
    page: Page,
    /// Diagnostics data, refreshed once a second while that page is showing.
    diag: Option<(Instant, AppInfo, view::OverlayDiagnostics)>,
    status: Option<String>,
}

pub struct App {
    backend: Arc<dyn AppBackend>,
    events: Receiver<BackendEvent>,
    store: AppStore,

    engine: FireEngine,
    rng: XorShift64,
    pixels: Vec<u8>,
    texture: Option<egui::TextureHandle>,
    fire_dirty: bool,
    last_update: Instant,
    accum_ms: f64,
    motion: f64,

    counter: Option<AnimatedCounter>,
    /// The count currently displayed (mid-animation it lags the real total).
    shown: f64,
    handled_surge: u32,
    /// (text, started at ms, blast magnitude)
    floater: Option<(String, i64, f32)>,
    /// (started at ms, peak scale) of the count's pop animation
    popping: Option<(i64, f32)>,

    main: MainWindow,

    shell: Option<Shell>,
    shared: Arc<OverlayShared>,
    /// What the user can see: false after HIDE or MIN, so the tray's Show Overlay is enabled again.
    overlay_visible: bool,
    last_tray_key: Option<(bool, bool, bool, bool, String)>,
    applied_shortcuts: Option<[String; 3]>,
    applied_autostart: bool,
    applied_theme: Option<Theme>,
    applied_size: Option<(f32, f32)>,
    applied_on_top: Option<bool>,
    /// The corner the overlay was last placed in (so a changed setting moves it, but a drag does not fight it).
    placed_corner: Option<Corner>,
    saved_pos: Option<(i32, i32)>,
    pending_pos: Option<(Instant, (i32, i32))>,
    last_diag: Instant,
    started: Instant,
}

impl App {
    pub fn new(backend: Arc<dyn AppBackend>, shell: Option<Shell>, shared: Arc<OverlayShared>, initial_size: (f32, f32), open_page: Option<Page>) -> Self {
        let events = backend.subscribe();
        let mut store = AppStore::new();
        store.init(&*backend);
        shared.visible.store(true, std::sync::atomic::Ordering::SeqCst);
        let saved_pos = store.settings.as_ref().and_then(|s| s.overlay_x.zip(s.overlay_y));
        let autostart = store.settings.as_ref().is_some_and(|s| s.start_with_system);
        let shortcuts = store.settings.as_ref().map(|s| [s.shortcut_toggle.clone(), s.shortcut_expand.clone(), s.shortcut_focus.clone()]);
        Self {
            backend,
            events,
            store,
            engine: FireEngine::new(),
            rng: XorShift64::new(0x00C0_FFEE),
            pixels: vec![0; FIRE_W * FIRE_H * 4],
            texture: None,
            fire_dirty: true,
            last_update: Instant::now(),
            accum_ms: 0.0,
            motion: 1.0,
            counter: None,
            shown: 0.0,
            handled_surge: 0,
            floater: None,
            popping: None,
            main: MainWindow { open: open_page.is_some(), page: open_page.unwrap_or(Page::Statistics), diag: None, status: None },
            shell,
            shared,
            overlay_visible: true,
            last_tray_key: None,
            applied_shortcuts: shortcuts,
            applied_autostart: autostart,
            applied_theme: None,
            applied_size: Some(initial_size),
            applied_on_top: None,
            placed_corner: None,
            saved_pos,
            pending_pos: None,
            last_diag: Instant::now() - Duration::from_secs(10),
            started: Instant::now(),
        }
    }

    // ---------------------------------------------------------------- data

    fn pump_events(&mut self) {
        let mut refresh = false;
        let mut others = Vec::new();
        for ev in self.events.try_iter() {
            match ev {
                BackendEvent::OverviewChanged | BackendEvent::HealthChanged | BackendEvent::ImportProgress { .. } => refresh = true,
                other => others.push(other),
            }
        }
        if refresh {
            // One backend notification per ingested batch can arrive several times per frame: refresh once.
            self.store.refresh(&*self.backend);
        }
        for ev in others {
            self.store.apply(ev, &*self.backend);
        }
    }

    fn patch(&mut self, patch: SettingsPatch) {
        self.store.update(patch, &*self.backend);
    }

    fn patch_json(&mut self, json: serde_json::Value) {
        if json.as_object().is_some_and(|o| o.is_empty()) {
            return;
        }
        match self.backend.update_settings_json(&json) {
            Ok(s) => {
                self.store.settings = Some(s);
                self.store.refresh(&*self.backend);
            }
            Err(e) => self.main.status = Some(e.to_string()),
        }
    }

    // ---------------------------------------------------------------- tray, hotkeys, autostart

    fn open_page(&mut self, page: Page) {
        self.main.open = true;
        self.main.page = page;
    }

    fn set_visible(&mut self, visible: bool) {
        self.overlay_visible = visible;
        self.shared.visible.store(visible, std::sync::atomic::Ordering::SeqCst);
    }

    fn show_overlay(&mut self, ctx: &egui::Context) {
        self.set_visible(true);
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Hiding is only offered while a tray icon exists to bring the overlay back; otherwise the app would be stranded.
    fn hide_overlay(&mut self, ctx: &egui::Context) {
        if self.shell.as_ref().is_some_and(|s| s.tray_available()) {
            self.set_visible(false);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        } else {
            self.main.status = Some("cannot hide: there is no tray icon to bring the overlay back".into());
        }
    }

    /// The overlay has no taskbar button, so a minimised overlay counts as hidden: the tray's `Show Overlay` restores it.
    fn minimize_overlay(&mut self, ctx: &egui::Context) {
        self.set_visible(false);
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }

    fn toggle_overlay(&mut self, ctx: &egui::Context) {
        if self.overlay_visible {
            self.hide_overlay(ctx);
        } else {
            self.show_overlay(ctx);
        }
    }

    fn toggle_pause(&mut self, settings: &Settings) {
        let r = if settings.paused { self.backend.resume() } else { self.backend.pause() };
        self.main.status = r.err().map(|e| e.to_string());
        self.store.init(&*self.backend);
    }

    fn handle_shell_events(&mut self, ctx: &egui::Context, settings: &Settings) {
        let events: Vec<ShellEvent> = self.shell.as_ref().map(|s| s.rx.try_iter().collect()).unwrap_or_default();
        for ev in events {
            match ev {
                ShellEvent::Tray(action) => match action {
                    TrayAction::ShowOverlay => self.show_overlay(ctx),
                    TrayAction::HideOverlay => self.hide_overlay(ctx),
                    TrayAction::ToggleOverlay => self.toggle_overlay(ctx),
                    TrayAction::ToggleTopmost => self.patch(SettingsPatch { always_on_top: Some(!settings.always_on_top), ..Default::default() }),
                    TrayAction::OpenStats => self.open_page(Page::Statistics),
                    TrayAction::OpenSettings => self.open_page(Page::Settings),
                    TrayAction::TogglePause => self.toggle_pause(settings),
                    TrayAction::Quit => self.backend.quit(),
                },
                ShellEvent::Hotkey(kind) => match kind {
                    HotkeyKind::Toggle => self.toggle_overlay(ctx),
                    HotkeyKind::Expand => {
                        self.show_overlay(ctx);
                        self.patch(SettingsPatch { compact_mode: Some(!settings.compact_mode), ..Default::default() });
                    }
                    HotkeyKind::Focus => self.show_overlay(ctx),
                },
            }
        }
    }

    /// Keeps the tray menu/tooltip and the registered hotkeys in step with the settings and the numbers.
    fn sync_shell(&mut self, settings: &Settings) {
        let overview = self.store.overview.as_ref();
        let state = TrayState {
            paused: overview.is_some_and(|o| o.paused),
            always_on_top: settings.always_on_top,
            overlay_visible: self.overlay_visible,
            unknown_data: overview.is_none_or(|o| o.totals.is_none()),
            total_tokens: overview.and_then(|o| o.totals).map_or(0, |t| t.total),
        };
        // The tooltip only changes when its rounded text does, so the menu is not rebuilt on every ingested event.
        let key = (state.paused, state.always_on_top, state.overlay_visible, state.unknown_data, tooltip_text(&state));
        let wanted = [settings.shortcut_toggle.clone(), settings.shortcut_expand.clone(), settings.shortcut_focus.clone()];
        let Some(shell) = &mut self.shell else { return };
        if self.last_tray_key.as_ref() != Some(&key) {
            shell.update_tray(&state);
            self.last_tray_key = Some(key);
        }
        if self.applied_shortcuts.as_ref() != Some(&wanted) {
            shell.set_shortcuts([&wanted[0], &wanted[1], &wanted[2]]);
            self.applied_shortcuts = Some(wanted);
        }
    }

    /// Only acts when the user changes the setting; starting the app never touches the system's start-up entries.
    fn sync_autostart(&mut self, settings: &Settings) {
        if settings.start_with_system == self.applied_autostart {
            return;
        }
        self.applied_autostart = settings.start_with_system;
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let entry = Autostart::new("AI Usage Monitor Native", &exe);
        let result = if settings.start_with_system { entry.enable() } else { entry.disable() };
        self.main.status = Some(match result {
            Ok(()) => format!("start with system: {}", if settings.start_with_system { "on" } else { "off" }),
            Err(e) => format!("could not change start with system: {e}"),
        });
    }

    // ---------------------------------------------------------------- window plumbing

    fn sync_window(&mut self, ctx: &egui::Context, settings: &Settings) {
        if self.applied_theme != Some(settings.theme) {
            theme::apply(ctx, settings.theme);
            // Overlay scroll bars are thin and float over the content (the old UI's `scrollbar-width: thin`).
            ctx.style_mut(|s| s.spacing.scroll = egui::style::ScrollStyle::floating());
            self.applied_theme = Some(settings.theme);
        }
        let want = self.window_size(settings);
        if self.applied_size != Some(want) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(want.0, want.1)));
            self.applied_size = Some(want);
        }
        if self.applied_on_top != Some(settings.always_on_top) {
            let level = if settings.always_on_top { egui::viewport::WindowLevel::AlwaysOnTop } else { egui::viewport::WindowLevel::Normal };
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
            self.applied_on_top = Some(settings.always_on_top);
        }
    }

    fn window_size(&self, settings: &Settings) -> (f32, f32) {
        if settings.compact_mode {
            settings.compact_size.map(|(w, h)| (w as f32, h as f32)).unwrap_or_else(|| default_compact_size(settings.token_text_size))
        } else {
            settings.expanded_size.map(|(w, h)| (w as f32, h as f32)).unwrap_or(EXPANDED_SIZE)
        }
    }

    /// The usable desktop (minus the taskbar) in egui points.
    fn work_area(&self, ctx: &egui::Context) -> Option<Area> {
        let ppp = ctx.pixels_per_point();
        if let Some((x, y, w, h)) = overlay_window::work_area() {
            return Some(Area { x: x as f32 / ppp, y: y as f32 / ppp, w: w as f32 / ppp, h: h as f32 / ppp });
        }
        ctx.input(|i| i.viewport().monitor_size).map(|m| Area { x: 0.0, y: 0.0, w: m.x, h: m.y })
    }

    /// Puts the overlay in its corner on first launch (or when the saved spot is off every screen) and whenever the
    /// "Overlay position" setting changes; a dragged position (`Custom`) is left alone.
    fn sync_placement(&mut self, ctx: &egui::Context, settings: &Settings) {
        let Some(area) = self.work_area(ctx) else { return };
        let (w, h) = self.window_size(settings);
        let saved = settings.overlay_x.zip(settings.overlay_y);
        let usable = saved.is_some_and(|(x, y)| placement::is_on_screen(x as f32, y as f32, w, h, area));
        if self.placed_corner.is_none() {
            // First look at the settings: a saved spot that is still on a screen always wins over the corner setting.
            if usable {
                self.placed_corner = Some(settings.overlay_corner);
                return;
            }
        } else if settings.overlay_corner == Corner::Custom || self.placed_corner == Some(settings.overlay_corner) {
            return; // nothing new to do: either a dragged position or the corner it is already in
        }        let (x, y) = placement::corner_position(area, settings.overlay_corner, w, h);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(x, y)));
        self.placed_corner = Some(settings.overlay_corner);
        self.saved_pos = Some((x.round() as i32, y.round() as i32));
        self.patch_json(serde_json::json!({"overlayX": x.round() as i32, "overlayY": y.round() as i32}));
    }

    /// Remembers where the user dragged the overlay (debounced so a drag is one write, not hundreds).
    fn track_position(&mut self, ctx: &egui::Context) {
        let (outer, minimized) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().minimized.unwrap_or(false)));
        let Some(r) = outer else { return };
        let pos = (r.min.x.round() as i32, r.min.y.round() as i32);
        if minimized || pos.0 < -10_000 || pos.1 < -10_000 {
            return; // a minimised window reports a parking position, not where the user put it
        }
        // A pixel or two of difference is rounding between points and pixels, not the user moving the window.
        let same = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs() <= 2 && (a.1 - b.1).abs() <= 2;
        if self.saved_pos.is_some_and(|s| same(s, pos)) {
            self.pending_pos = None;
            return;
        }
        match self.pending_pos {
            Some((_, p)) if p == pos => {}
            _ => self.pending_pos = Some((Instant::now(), pos)),
        }
        if let Some((since, p)) = self.pending_pos {
            if since.elapsed() >= POSITION_SAVE_DELAY {
                self.patch_json(serde_json::json!({"overlayX": p.0, "overlayY": p.1, "overlayCorner": "custom"}));
                self.saved_pos = Some(p);
                self.placed_corner = Some(Corner::Custom);
                self.pending_pos = None;
            } else {
                ctx.request_repaint_after(POSITION_SAVE_DELAY);
            }
        }
    }

    fn report_overlay(&mut self, ctx: &egui::Context, frame: &eframe::Frame, settings: &Settings) {
        if self.last_diag.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_diag = Instant::now();
        let (outer, inner, minimized, ppp) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.minimized.unwrap_or(false), i.pixels_per_point())
        });
        self.backend.set_overlay_diagnostics(view::OverlayDiagnostics {
            visible: !minimized,
            minimized,
            always_on_top_setting: settings.always_on_top,
            decorated: Some(false),
            os_reports_topmost: os_topmost(frame),
            x: outer.map(|r| r.min.x.round() as i32),
            y: outer.map(|r| r.min.y.round() as i32),
            width: inner.map(|r| r.width().round() as u32),
            height: inner.map(|r| r.height().round() as u32),
            scale_factor: Some(ppp),
            monitors: 1,
            platform_note: crate::backend::platform_note().into(),
        });
    }

    // ---------------------------------------------------------------- fire + number

    fn step_fire_and_counter(&mut self, now: Instant, now_ms: i64, settings: &Settings) {
        let elapsed = (now - self.last_update).as_secs_f64() * 1000.0;
        self.last_update = now;

        let motion = motion_factor(settings.animation_intensity);
        let want = flame_state(self.store.overview.as_ref(), now_ms);
        if want != self.engine.mode || (motion - self.motion).abs() > f64::EPSILON {
            self.motion = motion;
            self.engine.set_state(want, motion, &mut self.rng);
            self.fire_dirty = true;
        }

        // A sudden jump in the total fires a burst scaled to its size.
        if let Some(s) = self.store.surge.clone() {
            if s.id != self.handled_surge {
                self.handled_surge = s.id;
                if let Some(b) = self.engine.blast(s.delta as f64, &mut self.rng) {
                    self.floater = Some((format_compact(s.delta as f64, 3), now_ms, b.magnitude as f32));
                    self.popping = Some((now_ms, b.pop as f32));
                    self.fire_dirty = true;
                }
            }
        }

        if self.engine.needs_loop() {
            self.accum_ms += elapsed.min(250.0);
            let frame = self.engine.frame_ms();
            let mut ticks = 0;
            while self.accum_ms >= frame && ticks < 4 {
                self.engine.tick(frame, &mut self.rng);
                self.accum_ms -= frame;
                ticks += 1;
                self.fire_dirty = true;
            }
            if ticks == 4 {
                self.accum_ms = 0.0;
            }
        } else {
            self.accum_ms = 0.0;
        }

        // The big number counts up to the new total instead of jumping.
        let total = self.store.overview.as_ref().and_then(|o| o.totals).map(|t| t.total as f64);
        if let Some(total) = total {
            let c = self.counter.get_or_insert_with(|| AnimatedCounter::new(total));
            c.set_target(total, now_ms, settings.animation_intensity, 700.0);
            self.shown = c.tick(now_ms);
        }
        if self.floater.as_ref().is_some_and(|(_, at, _)| (now_ms - at) as f32 >= FLOATER_MS) {
            self.floater = None;
        }
        if self.popping.is_some_and(|(at, _)| now_ms - at >= 600) {
            self.popping = None;
        }
    }

    fn upload_fire(&mut self, ctx: &egui::Context) {
        if !self.fire_dirty && self.texture.is_some() {
            return;
        }
        self.engine.paint(&mut self.pixels);
        let image = egui::ColorImage::from_rgba_unmultiplied([FIRE_W, FIRE_H], &self.pixels);
        match &mut self.texture {
            Some(t) => t.set(image, egui::TextureOptions::NEAREST),
            None => self.texture = Some(ctx.load_texture("fire", image, egui::TextureOptions::NEAREST)),
        }
        self.fire_dirty = false;
    }

    fn schedule_repaint(&self, ctx: &egui::Context, settings: &Settings) {
        let mut next: Option<Duration> = None;
        let mut want = |d: Duration| next = Some(next.map_or(d, |n| n.min(d)));
        if let Some(ms) = self.engine.next_repaint() {
            want(Duration::from_millis(ms as u64));
        }
        if self.counter.as_ref().is_some_and(|c| c.is_animating()) || self.floater.is_some() || self.popping.is_some() {
            want(Duration::from_millis(33));
        }
        if self.engine.mode == FlameState::Live && settings.animation_intensity != AnimationIntensity::Off {
            want(STEPPED_PULSE);
        }
        if self.main.open && self.main.page == Page::Diagnostics {
            want(Duration::from_secs(1));
        }
        if let Some(d) = next {
            ctx.request_repaint_after(d);
        }
    }

    // ---------------------------------------------------------------- hero (compact overlay and top of the expanded one)

    /// What the big-number slot shows. Zero and "unknown" are different: never print 0 for no data.
    fn hero_body(&self) -> Body {
        match (&self.store.overview, &self.store.error) {
            (Some(o), _) if o.totals.is_none() => Body::Unavailable,
            (Some(_), _) => Body::Number(format_compact(self.shown, HERO_DIGITS)),
            (None, Some(_)) => Body::Error,
            (None, None) => Body::Pending,
        }
    }

    fn hero_props<'a>(&'a self, settings: &'a Settings, now_ms: i64, compact: bool) -> hero::Props<'a> {
        let overview = self.store.overview.as_ref();
        let paused = overview.is_some_and(|o| o.paused);
        let importing = overview.is_some_and(|o| !o.importing_agents.is_empty());
        let tag = if paused {
            Some("PAUSED")
        } else if importing {
            Some("IMPORTING HISTORY")
        } else {
            None
        };
        let floater = self.floater.as_ref().and_then(|(text, at, magnitude)| {
            let age = (now_ms - at) as f32 / FLOATER_MS;
            (0.0..1.0).contains(&age).then(|| hero::Floater { text: text.clone(), age, magnitude: *magnitude })
        });
        hero::Props {
            tokens: Tokens::for_theme(settings.theme),
            compact,
            num_scale: settings.token_text_size,
            body: self.hero_body(),
            live: self.engine.mode == FlameState::Live,
            period_label: period_word(settings.period),
            agents: overview.map(|o| o.agents.as_slice()).unwrap_or(&[]),
            now_ms,
            time_ms: self.started.elapsed().as_millis() as u64,
            intensity: settings.animation_intensity,
            tag,
            pin_on: settings.always_on_top,
            fire: self.texture.as_ref().map(|t| t.id()),
            floater,
            pop: self.pop_scale(now_ms),
        }
    }

    /// The `pop` animation: the count swells to `pop` over the first 18 % of 0.6 s and settles back.
    fn pop_scale(&self, now_ms: i64) -> f32 {
        self.popping.map_or(1.0, |(at, pop)| pop_scale((now_ms - at) as f32 / 600.0, pop))
    }

    fn apply_hero(&mut self, ctx: &egui::Context, settings: &Settings, out: hero::Out) {
        if out.start_drag {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if out.pin {
            self.patch(SettingsPatch { always_on_top: Some(!settings.always_on_top), ..Default::default() });
        }
        if out.minimize {
            self.minimize_overlay(ctx);
        }
        if out.hide {
            self.hide_overlay(ctx);
        }
        if out.cycle_period {
            self.patch(SettingsPatch { period: Some(next_period(settings.period)), ..Default::default() });
        }
        if out.toggle {
            self.patch(SettingsPatch { compact_mode: Some(!settings.compact_mode), ..Default::default() });
        }
    }

    /// `.overlay { background: rgb(var(--bg) / var(--panel-alpha)) }`: the user's "Overlay background" setting.
    fn backing_alpha(settings: &Settings) -> f32 {
        panel_alpha(settings.overlay_opacity, platform_alpha_floor())
    }

    fn compact_ui(&mut self, ctx: &egui::Context, settings: &Settings, now_ms: i64) {
        let mut out = hero::Out::default();
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let rect = ui.max_rect();
            let alpha = Self::backing_alpha(settings);
            if alpha > 0.0 {
                ui.painter().rect_filled(rect, 0.0, with_alpha(Tokens::for_theme(settings.theme).bg, alpha));
            }
            let props = self.hero_props(settings, now_ms, true);
            out = hero::show(ui, rect, &props);
        });
        self.apply_hero(ctx, settings, out);
    }

    // ---------------------------------------------------------------- expanded overlay

    /// The hero on top (see-through) and, below it, the scrolling "ALL AGENTS" panel on a backing of at least 90 %.
    fn expanded_ui(&mut self, ctx: &egui::Context, settings: &Settings, now_ms: i64) {
        let t = Tokens::for_theme(settings.theme);
        let mut out = hero::Out::default();
        let mut actions: Vec<AgentsAction> = Vec::new();
        let mut open: Option<&'static str> = None;
        let alpha = Self::backing_alpha(settings);
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let full = ui.max_rect();
            if alpha > 0.0 {
                ui.painter().rect_filled(full, 0.0, with_alpha(t.bg, alpha));
            }
            let props = self.hero_props(settings, now_ms, false);
            let hero_rect = Rect::from_min_size(full.min, vec2(full.width(), hero::height(ui.ctx(), &props, full.width())));
            out = hero::show(ui, hero_rect, &props);

            let more = Rect::from_min_max(Pos2::new(full.min.x, hero_rect.max.y), full.max);
            ui.painter().rect_filled(more, 0.0, with_alpha(t.bg, alpha.max(0.9)));
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(more));
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut inner, |ui| {
                egui::Frame::none().inner_margin(egui::Margin { left: 14.0, right: 14.0, top: 14.0, bottom: 16.0 }).show(ui, |ui| {
                    let props = all_agents::Props { tokens: t, overview: self.store.overview.as_ref(), settings, now_ms, time_ms: self.started.elapsed().as_millis() as u64, hide_headline: true };
                    actions = all_agents::show(ui, &props);
                    open = all_agents::overlay_actions(ui, &t);
                });
            });
        });
        self.apply_hero(ctx, settings, out);
        for a in actions {
            match a {
                AgentsAction::Period(p) => self.patch(SettingsPatch { period: Some(p), ..Default::default() }),
                AgentsAction::OpenAgent(_) => self.open_page(Page::Statistics),
            }
        }
        match open {
            Some("statistics") => self.open_page(Page::Statistics),
            Some("settings") => self.open_page(Page::Settings),
            _ => {}
        }
    }

    // ---------------------------------------------------------------- main window

    /// Diagnostics read the app info and the OS view of the overlay; once a second is plenty (the old page used 5 s).
    fn refresh_diagnostics(&mut self) {
        if self.main.page != Page::Diagnostics {
            return;
        }
        if self.main.diag.as_ref().is_some_and(|d| d.0.elapsed() < Duration::from_secs(1)) {
            return;
        }
        if let (Ok(info), Ok(overlay)) = (self.backend.app_info(), self.backend.overlay_diagnostics()) {
            self.main.diag = Some((Instant::now(), info, overlay));
        }
    }

    fn main_window(&mut self, ctx: &egui::Context, settings: &Settings) {
        let t = Tokens::for_theme(settings.theme);
        let mut page = self.main.page;
        let mut close = false;
        let status = self.main.status.clone();
        let overview = self.store.overview.clone();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let time_ms = self.started.elapsed().as_millis() as u64;
        self.refresh_diagnostics();
        let diag = self.main.diag.clone();
        let mut edit: Option<Settings> = None;
        let mut quit = false;
        let mut period: Option<view::PeriodKey> = None;

        let builder = egui::ViewportBuilder::default().with_title("AI Usage Monitor").with_inner_size(main_size()).with_min_inner_size(MAIN_MIN);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("main-window"), builder, |ctx, _class| {
            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            if let Some(s) = &status {
                egui::TopBottomPanel::bottom("status").frame(egui::Frame::none().fill(t.bg)).show_separator_line(false).show(ctx, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), egui::Sense::hover());
                    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0.0, t.ink);
                    native_ui::web::widgets::put(ui, s, &native_ui::web::widgets::hint(), r.left() + 24.0, r.top() + 6.0, t.dim);
                });
            }
            egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg)).show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                if let Some(p) = pages::header(ui, &t, page) {
                    page = p;
                }
                let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if let Some(y) = main_scroll() {
                    scroll = scroll.vertical_scroll_offset(y);
                }
                scroll.show(ui, |ui| match page {
                    Page::Statistics => pages::page(ui, |ui| {
                        let props = all_agents::Props { tokens: t, overview: overview.as_ref(), settings, now_ms, time_ms, hide_headline: false };
                        for a in all_agents::show(ui, &props) {
                            if let AgentsAction::Period(p) = a {
                                period = Some(p);
                            }
                        }
                    }),
                    Page::Settings => {
                        let agents = overview.as_ref().map(|o| o.agents.as_slice()).unwrap_or(&[]);
                        let o = pages::settings_page(ui, &t, settings, agents);
                        edit = o.changed;
                        quit = o.quit;
                    }
                    Page::Diagnostics => {
                        let agents = overview.as_ref().map(|o| o.agents.as_slice()).unwrap_or(&[]);
                        pages::diagnostics_page(ui, &t, &DiagnosticsData { overlay: diag.as_ref().map(|d| &d.2), agents, info: diag.as_ref().map(|d| &d.1), error: None });
                    }
                    Page::Privacy => pages::privacy_page(ui, &t),
                });
            });
        });

        self.main.page = page;
        if close {
            self.main.open = false;
        }
        if let Some(p) = period {
            self.patch(SettingsPatch { period: Some(p), ..Default::default() });
        }
        if let Some(new) = edit {
            self.patch_json(bridge::settings_diff(settings, &new));
        }
        if quit {
            self.backend.quit();
        }
    }
}

/// What Windows reports about the overlay's always-on-top bit (read back, not assumed).
fn os_topmost(frame: &eframe::Frame) -> Option<bool> {
    overlay_window::is_topmost(window_hwnd(frame)? as *mut std::ffi::c_void)
}

/// The overlay's Win32 window handle (None on other platforms).
fn window_hwnd(frame: &eframe::Frame) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // fully transparent: the desktop shows through
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.pump_events();

        let settings = match &self.store.settings {
            Some(s) => s.clone(),
            None => {
                // The backend could not be read at all: say so instead of drawing an empty window.
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.colored_label(Color32::RED, format!("DATA ERROR: {}", self.store.error.clone().unwrap_or_default()));
                });
                return;
            }
        };

        if self.shared.hwnd.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            if let Some(h) = window_hwnd(frame) {
                self.shared.hwnd.store(h, std::sync::atomic::Ordering::SeqCst);
            }
        }
        self.handle_shell_events(ctx, &settings);
        let settings = self.store.settings.clone().unwrap_or(settings);
        self.sync_window(ctx, &settings);
        self.sync_placement(ctx, &settings);
        self.sync_shell(&settings);
        self.sync_autostart(&settings);
        self.step_fire_and_counter(now, now_ms, &settings);
        self.upload_fire(ctx);
        let settings = self.store.settings.clone().unwrap_or(settings);
        if settings.compact_mode {
            self.compact_ui(ctx, &settings, now_ms);
        } else {
            self.expanded_ui(ctx, &settings, now_ms);
        }
        if self.main.open {
            let settings = self.store.settings.clone().unwrap_or(settings.clone());
            self.main_window(ctx, &settings);
        }
        self.track_position(ctx);
        self.report_overlay(ctx, frame, &settings);
        if self.backend.quit_requested() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.schedule_repaint(ctx, &settings);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.backend.shutdown();
    }
}
