use eframe::egui;
use native_ui::motion::{State, show};

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_decorations(true),
        ..Default::default()
    };
    
    let mut state = State::new();
    
    eframe::run_simple_native("Motion Overlay", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let _actions = show(ui, &mut state, 0);
        });
    })
}
