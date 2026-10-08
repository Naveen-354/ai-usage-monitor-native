use eframe::egui;
use native_ui::privacy;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("Privacy Example"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Privacy",
        options,
        Box::new(|_cc| Ok(Box::new(ExampleApp::new()))),
    )
}

struct ExampleApp {
    state: privacy::State,
}

impl ExampleApp {
    fn new() -> Self {
        Self {
            state: privacy::State::default(),
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let _ = privacy::show(ui, &mut self.state);
            });
        });
    }
}
