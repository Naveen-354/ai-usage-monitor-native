use crate::view::PeriodKey;

pub mod format;
pub mod counter;
pub mod surge;
pub mod rollover;
pub mod drag;
pub mod frame;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    SetPeriod(PeriodKey),
    UpdateSettings, 
}

#[derive(Default)]
pub struct State {}

impl State {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn show(ui: &mut egui::Ui, _state: &mut State, _now_ms: i64) -> Vec<Action> {
    ui.label("Motion logic built and unit-tested, not launched");
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, RawInput, CentralPanel};

    #[test]
    fn headless_smoke_test() {
        let mut state = State::new();
        let _ = Context::default().run(RawInput::default(), |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let actions = show(ui, &mut state, 0);
                assert!(actions.is_empty());
            });
        });
    }
}
