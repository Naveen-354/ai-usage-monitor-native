#[derive(Default)]
pub struct State {}

pub enum PrivacyAction {}

pub fn show(ui: &mut egui::Ui, _state: &mut State) -> Vec<PrivacyAction> {
    ui.heading("LOCAL FIRST");
    ui.label("AI Usage Monitor reads token counters that your AI coding agents already wrote to your own disk, and stores aggregated usage in a SQLite file on your own machine. Nothing is uploaded, and there is no telemetry.");

    ui.add_space(8.0);
    ui.heading("WHAT IT READS");
    ui.label("• Token counters, model name, timestamp, session id and working-folder path from each agent's session data.");
    ui.label("• The folder name is used to group usage by project.");

    ui.add_space(8.0);
    ui.heading("WHAT IT NEVER READS OR STORES");
    ui.label("• Prompts, responses, code, tool output, file contents or conversation titles.");
    ui.label("• API keys, OAuth tokens or any credential file.");

    ui.add_space(8.0);
    ui.heading("NETWORK");
    ui.label("• The application contains no networking code and its content-security policy forbids outside connections.");
    ui.label("• There is no auto-updater and no analytics.");

    ui.add_space(8.0);
    ui.small("The full audit, including exactly which fields each collector reads, is in PRIVACY.md in the repository.");

    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_smoke_test() {
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
