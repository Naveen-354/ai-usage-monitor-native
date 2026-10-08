use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem, CheckMenuItem},
    TrayIcon, TrayIconBuilder, Icon,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    ToggleOverlay,
    ShowOverlay,
    HideOverlay,
    ToggleTopmost,
    OpenStats,
    OpenSettings,
    TogglePause,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayState {
    pub paused: bool,
    pub always_on_top: bool,
    pub overlay_visible: bool,
    pub unknown_data: bool,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemKind {
    Normal,
    Check(bool),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemSpec {
    pub id: String,
    pub label: String,
    pub kind: MenuItemKind,
    pub enabled: bool,
}

pub fn build_menu(state: &TrayState) -> Vec<MenuItemSpec> {
    let status = if state.paused { "○ Paused" } else { "● Monitoring" };
    let pause_label = if state.paused { "Resume Monitoring" } else { "Pause Monitoring" };

    vec![
        MenuItemSpec { id: "title".into(), label: "AI Usage Monitor".into(), kind: MenuItemKind::Normal, enabled: false },
        MenuItemSpec { id: "status".into(), label: status.into(), kind: MenuItemKind::Normal, enabled: false },
        MenuItemSpec { id: "sep1".into(), label: "".into(), kind: MenuItemKind::Separator, enabled: true },
        MenuItemSpec { id: "show".into(), label: "Show Overlay".into(), kind: MenuItemKind::Normal, enabled: !state.overlay_visible },
        MenuItemSpec { id: "hide".into(), label: "Hide Overlay".into(), kind: MenuItemKind::Normal, enabled: state.overlay_visible },
        MenuItemSpec { id: "sep2".into(), label: "".into(), kind: MenuItemKind::Separator, enabled: true },
        MenuItemSpec { id: "topmost".into(), label: "Always On Top".into(), kind: MenuItemKind::Check(state.always_on_top), enabled: true },
        MenuItemSpec { id: "sep3".into(), label: "".into(), kind: MenuItemKind::Separator, enabled: true },
        MenuItemSpec { id: "stats".into(), label: "Statistics".into(), kind: MenuItemKind::Normal, enabled: true },
        MenuItemSpec { id: "settings".into(), label: "Settings".into(), kind: MenuItemKind::Normal, enabled: true },
        MenuItemSpec { id: "sep4".into(), label: "".into(), kind: MenuItemKind::Separator, enabled: true },
        MenuItemSpec { id: "pause".into(), label: pause_label.into(), kind: MenuItemKind::Normal, enabled: true },
        MenuItemSpec { id: "sep5".into(), label: "".into(), kind: MenuItemKind::Separator, enabled: true },
        MenuItemSpec { id: "quit".into(), label: "Quit".into(), kind: MenuItemKind::Normal, enabled: true },
    ]
}

pub fn action_for(id: &str) -> Option<TrayAction> {
    match id {
        "show" => Some(TrayAction::ShowOverlay),
        "hide" => Some(TrayAction::HideOverlay),
        "topmost" => Some(TrayAction::ToggleTopmost),
        "stats" => Some(TrayAction::OpenStats),
        "settings" => Some(TrayAction::OpenSettings),
        "pause" => Some(TrayAction::TogglePause),
        "quit" => Some(TrayAction::Quit),
        _ => None,
    }
}

pub fn generate_flame_icon() -> Icon {
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let dy = height as f32 - y as f32;
            let dx = (width / 2) as f32 - x as f32;
            let d = f32::sqrt(dx*dx + dy*dy);
            if d < 12.0 {
                rgba.extend_from_slice(&[255, 100, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(rgba, width, height).unwrap()
}

pub fn tooltip_text(state: &TrayState) -> String {
    if state.unknown_data {
        "TOKEN DATA UNAVAILABLE".into()
    } else {
        let t = state.total_tokens;
        if t >= 1_000_000_000_000 {
            format!("{:.1}T", t as f64 / 1_000_000_000_000.0)
        } else if t >= 1_000_000_000 {
            format!("{:.1}B", t as f64 / 1_000_000_000.0)
        } else if t >= 1_000_000 {
            format!("{:.1}M", t as f64 / 1_000_000.0)
        } else if t >= 1_000 {
            format!("{:.1}K", t as f64 / 1_000.0)
        } else {
            format!("{}", t)
        }
    }
}

/// The system tray icon. Menu items carry the ids from [`build_menu`], so a click maps back to its [`TrayAction`]
/// through [`action_for`] (items with random ids could never be told apart).
pub struct TrayHandle {
    icon: Option<TrayIcon>,
}

impl TrayHandle {
    fn menu_for(state: &TrayState) -> Menu {
        let menu = Menu::new();
        for item in build_menu(state) {
            let _ = match item.kind {
                MenuItemKind::Normal => menu.append(&MenuItem::with_id(item.id.as_str(), &item.label, item.enabled, None)),
                MenuItemKind::Check(checked) => menu.append(&CheckMenuItem::with_id(item.id.as_str(), &item.label, item.enabled, checked, None)),
                MenuItemKind::Separator => menu.append(&PredefinedMenuItem::separator()),
            };
        }
        menu
    }

    fn tooltip(state: &TrayState) -> String {
        format!("AI Usage Monitor: {}", tooltip_text(state))
    }

    /// Creates the tray icon. Must run on the thread that owns the window event loop.
    pub fn new(state: &TrayState, icon: Icon) -> Self {
        let built = TrayIconBuilder::new()
            .with_menu(Box::new(Self::menu_for(state)))
            .with_tooltip(Self::tooltip(state))
            .with_icon(icon)
            .build();
        Self { icon: built.ok() }
    }

    /// `false` when the OS refused the icon: the overlay must then never be hidden, or it could not be brought back.
    pub fn is_available(&self) -> bool {
        self.icon.is_some()
    }

    /// Refreshes the menu labels/checks and the tooltip.
    pub fn update(&self, state: &TrayState) {
        if let Some(t) = &self.icon {
            t.set_menu(Some(Box::new(Self::menu_for(state))));
            let _ = t.set_tooltip(Some(Self::tooltip(state)));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_model() {
        let state = TrayState {
            paused: false,
            always_on_top: true,
            overlay_visible: true,
            unknown_data: false,
            total_tokens: 0,
        };
        let menu = build_menu(&state);
        assert_eq!(menu[1].label, "● Monitoring");
        assert!(!menu[3].enabled); // show is disabled when visible
        assert!(menu[4].enabled); // hide is enabled when visible
    }

    #[test]
    fn test_tooltip_formatting() {
        let mut state = TrayState {
            paused: false,
            always_on_top: true,
            overlay_visible: true,
            unknown_data: false,
            total_tokens: 500,
        };
        assert_eq!(tooltip_text(&state), "500");
        state.total_tokens = 1500;
        assert_eq!(tooltip_text(&state), "1.5K");
        state.total_tokens = 2_500_000;
        assert_eq!(tooltip_text(&state), "2.5M");
        state.unknown_data = true;
        assert_eq!(tooltip_text(&state), "TOKEN DATA UNAVAILABLE");
    }
}
