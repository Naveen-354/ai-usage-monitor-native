//! Brutalist Theme and Widget Gallery example.
//!
//! Run with: `cargo run --example theme` (interactive window for a human to view).

use eframe::egui;
use native_ui::theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 720.0])
            .with_title("Brutalist Theme Gallery"),
        ..Default::default()
    };

    eframe::run_native(
        "Brutalist Theme Gallery",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, native_ui::view::Theme::Dark);
            Ok(Box::new(ThemeGalleryApp::default()))
        }),
    )
}

#[derive(Default)]
struct ThemeGalleryApp {
    state: theme::State,
}

impl eframe::App for ThemeGalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                theme::show(ui, &mut self.state);
            });
        });
    }
}
