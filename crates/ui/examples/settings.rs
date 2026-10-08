use eframe::egui;
use native_ui::mock;
use native_ui::settings::page::{show, SettingsState};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 800.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Settings Example",
        options,
        Box::new(|_cc| Ok(Box::new(SettingsExample::default()))),
    )
}

struct SettingsExample {
    state: SettingsState,
    settings: native_ui::view::Settings,
    overview: native_ui::view::Overview,
}

impl Default for SettingsExample {
    fn default() -> Self {
        Self {
            state: SettingsState::default(),
            settings: mock::settings(),
            overview: mock::overview(),
        }
    }
}

impl eframe::App for SettingsExample {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let actions = show(ui, &mut self.state, &self.settings, &self.overview);
            for action in actions {
                if let native_ui::settings::page::SettingsAction::Patch(patch) = action {
                    patch.apply_to(&mut self.settings);
                }
            }
        });
    }
}
