//! The OS-facing shell around the window: tray icon, global hotkeys and their event forwarding.
//!
//! Events from the OS arrive on library-owned channels. Two small threads forward them into one channel the UI
//! drains, and wake the UI (a hidden or idle window would otherwise never run `update`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use fire_engine::engine::{FireEngine, FlameState};
use fire_engine::sim::{FIRE_H, FIRE_W};
use fire_engine::XorShift64;
use native_platform::global_hotkey::hotkey::{Code, HotKey, Modifiers};
use native_platform::global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use native_platform::platform::overlay_window;
use native_platform::platform::tray::{action_for, generate_flame_icon, TrayAction, TrayHandle, TrayState};
use native_platform::tray_icon::menu::MenuEvent;
use native_platform::tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconEvent};
use native_ui::settings::shortcut::{parse_shortcut, Shortcut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyKind {
    Toggle,
    Expand,
    Focus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Tray(TrayAction),
    Hotkey(HotkeyKind),
}

// ------------------------------------------------------------------------------------------------ window state shared with the event threads

/// What the OS-event threads need to know about the overlay window. A hidden window runs no frame loop, so these
/// threads bring it back themselves instead of waiting for an `update` that would never come.
#[derive(Default)]
pub struct OverlayShared {
    /// Win32 window handle (0 until the first frame has run).
    pub hwnd: AtomicIsize,
    /// What the user can currently see.
    pub visible: AtomicBool,
}

impl OverlayShared {
    /// If the overlay is hidden or minimised, shows it again right now.
    pub fn restore_if_hidden(&self) {
        if self.visible.load(Ordering::SeqCst) {
            return;
        }
        let h = self.hwnd.load(Ordering::SeqCst);
        if h != 0 {
            overlay_window::restore_visible(h as *mut std::ffi::c_void);
            self.visible.store(true, Ordering::SeqCst);
        }
    }
}

// ------------------------------------------------------------------------------------------------ shortcuts

/// `"A"` -> `Code::KeyA`, `"7"` -> `Digit7`, `"Up"` -> `ArrowUp`; every other canonical name is already a W3C code name.
fn code_for(key: &str) -> Option<Code> {
    let name = match key {
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphabetic()) => format!("Key{}", k.to_ascii_uppercase()),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()) => format!("Digit{k}"),
        "Up" => "ArrowUp".to_string(),
        "Down" => "ArrowDown".to_string(),
        "Left" => "ArrowLeft".to_string(),
        "Right" => "ArrowRight".to_string(),
        other => other.to_string(),
    };
    name.parse::<Code>().ok()
}

/// Turns an already-validated shortcut into an OS hotkey. A shortcut without a modifier never gets here
/// (the validating parser rejects it), so a bare letter can never be hijacked system-wide.
pub fn to_hotkey(sc: &Shortcut) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    if sc.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if sc.alt {
        mods |= Modifiers::ALT;
    }
    if sc.shift {
        mods |= Modifiers::SHIFT;
    }
    if sc.meta {
        mods |= Modifiers::SUPER;
    }
    if mods.is_empty() {
        return None;
    }
    Some(HotKey::new(Some(mods), code_for(&sc.key)?))
}

// ------------------------------------------------------------------------------------------------ tray icon

/// A real pixel flame (the same simulation as the overlay), cropped to a 32x32 tray icon.
pub fn flame_icon_rgba() -> (Vec<u8>, u32, u32) {
    let mut engine = FireEngine::new();
    let mut rng = XorShift64::new(7);
    engine.set_state(FlameState::Live, 1.0, &mut rng);
    for _ in 0..24 {
        engine.tick(83.0, &mut rng);
    }
    let mut full = vec![0u8; FIRE_W * FIRE_H * 4];
    engine.paint(&mut full);

    const SIZE: usize = 32;
    let (x0, y0) = ((FIRE_W - SIZE) / 2, FIRE_H - SIZE);
    let mut out = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        let row = ((y0 + y) * FIRE_W + x0) * 4;
        out.extend_from_slice(&full[row..row + SIZE * 4]);
    }
    (out, SIZE as u32, SIZE as u32)
}

fn tray_icon() -> Icon {
    let (rgba, w, h) = flame_icon_rgba();
    Icon::from_rgba(rgba, w, h).unwrap_or_else(|_| generate_flame_icon())
}

// ------------------------------------------------------------------------------------------------ the shell

type HotkeyMap = Arc<Mutex<HashMap<u32, HotkeyKind>>>;

pub struct Shell {
    tray: TrayHandle,
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    map: HotkeyMap,
    pub rx: Receiver<ShellEvent>,
    /// Sender into the same queue (used by the self-test and by tests).
    pub tx: Sender<ShellEvent>,
    /// Hotkeys that could not be registered (typo, already taken by another program).
    pub warnings: Vec<String>,
}

impl Shell {
    /// Creates the tray icon and registers the hotkeys. Call on the window's event-loop thread.
    pub fn start(state: &TrayState, shortcuts: [&str; 3], shared: Arc<OverlayShared>, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        let tray = TrayHandle::new(state, tray_icon());
        let map: HotkeyMap = Arc::default();

        {
            let (tx, repaint, shared) = (tx.clone(), repaint.clone(), shared.clone());
            std::thread::Builder::new()
                .name("tray-menu-events".into())
                .spawn(move || {
                    for ev in MenuEvent::receiver().iter() {
                        if let Some(action) = action_for(&ev.id.0) {
                            if matches!(action, TrayAction::ShowOverlay | TrayAction::ToggleOverlay) {
                                shared.restore_if_hidden();
                            }
                            let _ = tx.send(ShellEvent::Tray(action));
                            repaint();
                        }
                    }
                })
                .ok();
        }
        {
            let (tx, repaint, shared) = (tx.clone(), repaint.clone(), shared.clone());
            std::thread::Builder::new()
                .name("tray-click-events".into())
                .spawn(move || {
                    for ev in TrayIconEvent::receiver().iter() {
                        // A plain left click brings the overlay back (the menu is on the right click).
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                            shared.restore_if_hidden();
                            let _ = tx.send(ShellEvent::Tray(TrayAction::ShowOverlay));
                            repaint();
                        }
                    }
                })
                .ok();
        }
        {
            let (tx, map, shared) = (tx.clone(), map.clone(), shared.clone());
            std::thread::Builder::new()
                .name("hotkey-events".into())
                .spawn(move || {
                    for ev in GlobalHotKeyEvent::receiver().iter() {
                        if ev.state != HotKeyState::Pressed {
                            continue;
                        }
                        let kind = map.lock().ok().and_then(|m| m.get(&ev.id).copied());
                        if let Some(kind) = kind {
                            // Every hotkey wants the overlay on screen (toggle only hides it when it is already visible).
                            shared.restore_if_hidden();
                            let _ = tx.send(ShellEvent::Hotkey(kind));
                            repaint();
                        }
                    }
                })
                .ok();
        }

        let mut shell = Shell { tray, manager: None, registered: Vec::new(), map, rx, tx, warnings: Vec::new() };
        match GlobalHotKeyManager::new() {
            Ok(m) => shell.manager = Some(m),
            Err(e) => shell.warnings.push(format!("global hotkeys are unavailable: {e}")),
        }
        shell.set_shortcuts(shortcuts);
        shell
    }

    pub fn tray_available(&self) -> bool {
        self.tray.is_available()
    }

    pub fn update_tray(&self, state: &TrayState) {
        self.tray.update(state);
    }

    /// (Re)registers the three global shortcuts: toggle, expand, focus. A bad or taken shortcut becomes a warning and
    /// never stops the others.
    pub fn set_shortcuts(&mut self, shortcuts: [&str; 3]) {
        let Some(manager) = &self.manager else { return };
        for hk in self.registered.drain(..) {
            let _ = manager.unregister(hk);
        }
        let mut map = HashMap::new();
        self.warnings.retain(|w| !w.starts_with("shortcut"));
        for (text, kind) in shortcuts.into_iter().zip([HotkeyKind::Toggle, HotkeyKind::Expand, HotkeyKind::Focus]) {
            let parsed = parse_shortcut(text).map_err(|e| e.to_string()).and_then(|sc| to_hotkey(&sc).ok_or_else(|| "key not supported".to_string()));
            match parsed {
                Ok(hk) => match manager.register(hk) {
                    Ok(()) => {
                        map.insert(hk.id(), kind);
                        self.registered.push(hk);
                    }
                    Err(e) => self.warnings.push(format!("shortcut '{text}' could not be registered (is another program using it?): {e}")),
                },
                Err(e) => self.warnings.push(format!("shortcut '{text}' is not valid: {e}")),
            }
        }
        if let Ok(mut m) = self.map.lock() {
            *m = map;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(text: &str) -> Option<HotKey> {
        parse_shortcut(text).ok().and_then(|sc| to_hotkey(&sc))
    }

    #[test]
    fn the_backends_default_shortcuts_all_become_os_hotkeys() {
        let s = ai_usage_monitor_core::settings::Settings::default();
        for text in [&s.shortcut_toggle, &s.shortcut_expand, &s.shortcut_focus] {
            assert!(hotkey(text).is_some(), "default shortcut '{text}' must register");
        }
    }

    #[test]
    fn letters_digits_function_and_named_keys_map_to_the_right_codes() {
        let code = |t: &str| hotkey(t).map(|h| h.key);
        assert_eq!(code("Ctrl+Alt+T"), Some(Code::KeyT));
        assert_eq!(code("Ctrl+Alt+7"), Some(Code::Digit7));
        assert_eq!(code("Ctrl+F5"), Some(Code::F5));
        assert_eq!(code("Alt+Up"), Some(Code::ArrowUp));
        assert_eq!(code("Ctrl+Space"), Some(Code::Space));
        assert_eq!(code("Ctrl+Shift+`"), Some(Code::Backquote));
        assert_eq!(code_for("BracketLeft"), Some(Code::BracketLeft));
        assert_eq!(code_for("Nonsense"), None);
    }

    #[test]
    fn modifiers_are_carried_over() {
        let h = hotkey("Ctrl+Alt+Shift+Win+U").unwrap();
        assert_eq!(h.mods, Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::SUPER);
    }

    #[test]
    fn a_shortcut_that_could_hijack_a_bare_key_never_becomes_a_hotkey() {
        // The platform crate's own parser would have accepted "Foo+T" as a bare T; the validating one does not.
        for bad in ["T", "Foo+T", "Ctrl", "", "Ctrl+Alt", "Ctrl+Nope", "Ctrl+A+B"] {
            assert!(hotkey(bad).is_none(), "'{bad}' must not register");
        }
    }

    #[test]
    fn the_tray_icon_is_a_real_flame_not_a_disc() {
        let (rgba, w, h) = flame_icon_rgba();
        assert_eq!((w, h), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let opaque = |x: usize, y: usize| rgba[(y * 32 + x) * 4 + 3] > 0;
        let total: usize = (0..32).flat_map(|y| (0..32).map(move |x| (x, y))).filter(|&(x, y)| opaque(x, y)).count();
        assert!((40..700).contains(&total), "{total} opaque pixels: neither empty nor a solid blob");
        // A flame is rooted at the bottom centre and narrower near the top than at the base.
        let width_of = |y: usize| (0..32).filter(|&x| opaque(x, y)).count();
        let top = (0..16).map(width_of).max().unwrap_or(0);
        let base = (20..32).map(width_of).max().unwrap_or(0);
        assert!(base > top, "base {base} should be wider than the upper half {top}");
    }

    #[test]
    fn the_flame_icon_is_accepted_by_the_tray_library() {
        let (rgba, w, h) = flame_icon_rgba();
        assert!(Icon::from_rgba(rgba, w, h).is_ok());
    }
}
