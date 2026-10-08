pub mod tray;
pub mod hotkeys;
pub mod autostart;
pub mod single_instance;
pub mod overlay_window;

pub struct State {
    pub active: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { active: true }
    }
}

pub enum Action {
    ToggleOverlay,
    ExpandOverlay,
    FocusOverlay,
    Quit,
    SetTopmost(bool),
    ShowOverlay,
    HideOverlay,
    OpenStats,
    OpenSettings,
    TogglePause,
}

pub fn show(ui: &mut egui::Ui, _state: &mut State) -> Vec<Action> {
    let actions = Vec::new();
    ui.label("Platform module is active");
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headless_smoke() {
        let mut state = State::default();
        let ctx = egui::Context::default();
        
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = show(ui, &mut state);
                assert!(actions.is_empty());
            });
        });
    }
}

