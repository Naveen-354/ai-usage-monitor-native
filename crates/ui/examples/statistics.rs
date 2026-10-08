use eframe::egui;
use native_ui::statistics::{show, StatisticsState};
use native_ui::view::{History, Overview, PeriodOrCustom, Range};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Statistics Module Example",
        options,
        Box::new(|_cc| Ok(Box::new(StatisticsExample::default()))),
    )
}

struct StatisticsExample {
    state: StatisticsState,
    history: History,
    overview: Overview,
}

impl Default for StatisticsExample {
    fn default() -> Self {
        Self {
            state: StatisticsState::default(),
            history: History {
                period: PeriodOrCustom::Day,
                range: Range { start_utc_ms: 0, end_utc_ms: 0 },
                buckets: vec![],
                totals: None,
                by_model: vec![],
                by_project: vec![],
            },
            overview: Overview {
                period: PeriodOrCustom::Day,
                range: Range { start_utc_ms: 0, end_utc_ms: 0 },
                generated_at_utc_ms: 0,
                totals: None,
                agents: vec![],
                unavailable_agents: 0,
                any_data: false,
                paused: false,
                count_cached_in_total: false,
                importing_agents: vec![],
            },
        }
    }
}

impl eframe::App for StatisticsExample {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let actions = show(ui, &mut self.state, &self.history, &self.overview);
            if !actions.is_empty() {
                println!("Actions: {:?}", actions);
            }
        });
    }
}
