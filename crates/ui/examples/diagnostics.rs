use eframe::egui;
use native_ui::diagnostics::{self, DiagnosticsData};
use native_ui::mock;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Diagnostics Example"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Diagnostics",
        options,
        Box::new(|_cc| Ok(Box::new(ExampleApp::new()))),
    )
}

struct ExampleApp {
    state: diagnostics::State,
    data: DiagnosticsData,
}

impl ExampleApp {
    fn new() -> Self {
        Self {
            state: diagnostics::State::default(),
            data: DiagnosticsData {
                app_info: mock::app_info(),
                overlay: mock::diagnostics(),
                collectors: vec![
                    diagnostics::mock_builder::collector_ok("claude"),
                    diagnostics::mock_builder::collector_ok("codex"),
                    diagnostics::mock_builder::collector_error("gemini", "no local token ledger"),
                ],
                overview: mock::overview(),
            },
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let actions = diagnostics::show(ui, &mut self.state, &self.data);
                if !actions.is_empty() {
                    println!("Actions: {:?}", actions);
                }
            });
        });
    }
}
