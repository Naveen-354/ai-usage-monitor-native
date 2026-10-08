use eframe::egui;
use native_platform::platform::{State, show};

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_decorations(true),
        ..Default::default()
    };

    let mut state = State::default();

    eframe::run_simple_native("Platform Module Example", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            show(ui, &mut state);
        });
    })
}

