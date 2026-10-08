//! Example executable for the expanded overlay view.
//!
//! Run with: `cargo run --example expanded` (do not run during automated test suite).

use eframe::egui;
use native_ui::expanded::{self, ExpandedAction, ExpandedState};
use native_ui::mock;

struct ExpandedApp {
    state: ExpandedState,
    overview: native_ui::view::Overview,
    settings: native_ui::view::Settings,
    now_ms: i64,
}

impl Default for ExpandedApp {
    fn default() -> Self {
        Self {
            state: ExpandedState::default(),
            overview: mock::overview(),
            settings: mock::settings(),
            now_ms: mock::NOW_MS,
        }
    }
}

impl eframe::App for ExpandedApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let actions = expanded::show(
                ui,
                &mut self.state,
                &self.overview,
                &self.settings,
                self.now_ms,
            );

            for action in actions {
                match action {
                    ExpandedAction::SetPeriod(pk) => {
                        self.settings.period = pk;
                        self.overview.period = match pk {
                            native_ui::view::PeriodKey::Day => native_ui::view::PeriodOrCustom::Day,
                            native_ui::view::PeriodKey::Week => native_ui::view::PeriodOrCustom::Week,
                            native_ui::view::PeriodKey::Month => native_ui::view::PeriodOrCustom::Month,
                            native_ui::view::PeriodKey::Year => native_ui::view::PeriodOrCustom::Year,
                        };
                    }
                    ExpandedAction::TogglePin => {
                        self.settings.always_on_top = !self.settings.always_on_top;
                    }
                    ExpandedAction::Collapse => {
                        println!("Action: Collapse requested");
                    }
                    ExpandedAction::Minimize => {
                        println!("Action: Minimize requested");
                    }
                    ExpandedAction::Hide => {
                        println!("Action: Hide requested");
                    }
                    ExpandedAction::OpenWindow(page) => {
                        println!("Action: OpenWindow({:?}) requested", page);
                    }
                    ExpandedAction::SelectAgent(id) => {
                        println!("Action: SelectAgent({}) requested", id);
                    }
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 680.0])
            .with_title("AI Usage Monitor - Expanded View"),
        ..Default::default()
    };

    eframe::run_native(
        "AI Usage Monitor - Expanded View",
        options,
        Box::new(|_cc| Ok(Box::new(ExpandedApp::default()))),
    )
}
