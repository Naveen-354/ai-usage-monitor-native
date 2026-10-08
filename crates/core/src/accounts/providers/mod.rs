//! One adapter per agent. To support a new agent: write its adapter here, add it to [`registry`], and give it a row in the
//! agent catalogue - nothing in the manager, the CLI or the UI changes.

mod claude;
mod codex;
mod gemini;
mod opencode;
mod passive;

use std::collections::HashMap;
use std::sync::Arc;

use super::provider::AccountProvider;

pub use claude::ClaudeProvider;
pub use codex::CodexProvider;
pub use gemini::GeminiProvider;
pub use opencode::OpenCodeProvider;
pub use passive::PassiveProvider;

pub type SharedProvider = Arc<dyn AccountProvider>;

/// Every adapter the app ships, in catalogue order.
pub fn registry() -> Vec<SharedProvider> {
    vec![
        Arc::new(CodexProvider),
        Arc::new(ClaudeProvider),
        Arc::new(GeminiProvider),
        Arc::new(PassiveProvider::antigravity()),
        Arc::new(OpenCodeProvider),
        Arc::new(PassiveProvider::ollama()),
        Arc::new(PassiveProvider::aider()),
    ]
}

/// Adapters by agent id.
pub fn by_id(providers: Vec<SharedProvider>) -> HashMap<&'static str, SharedProvider> {
    providers.into_iter().map(|p| (p.agent_id(), p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::provider::SwitchSupport;
    use crate::model::catalog;

    #[test]
    fn every_agent_in_the_catalogue_has_exactly_one_adapter() {
        let ids: Vec<&str> = registry().iter().map(|p| p.agent_id()).collect();
        for a in catalog() {
            assert_eq!(ids.iter().filter(|i| **i == a.id).count(), 1, "adapter for {}", a.id);
        }
        assert_eq!(ids.len(), catalog().len(), "no adapter for an agent that does not exist");
    }

    #[test]
    fn the_agents_that_can_switch_are_exactly_those_with_a_documented_profile_variable() {
        let mut supported: Vec<(&str, &str)> = registry()
            .iter()
            .filter_map(|p| match p.capabilities().switch_support() {
                SwitchSupport::Supported { mechanism } => Some((p.agent_id(), mechanism)),
                SwitchSupport::Unavailable { .. } => None,
            })
            .collect();
        supported.sort();
        assert_eq!(
            supported,
            [("claude", "CLAUDE_CONFIG_DIR"), ("codex", "CODEX_HOME"), ("gemini", "GEMINI_CLI_HOME"), ("opencode", "XDG_DATA_HOME")]
        );
    }

    #[test]
    fn a_supported_agent_always_has_a_way_to_sign_in_and_a_profile_environment() {
        for p in registry() {
            let caps = p.capabilities();
            if caps.isolation.is_some() {
                assert!(!caps.login_methods.is_empty(), "{} can switch but cannot sign in", p.agent_id());
                let profile = crate::accounts::provider::ProfileDirs::new(std::path::Path::new("/x"));
                let env = p.profile_env(&profile);
                assert!(env.iter().any(|(k, _)| Some(*k) == caps.isolation), "{} does not set {:?}", p.agent_id(), caps.isolation);
            }
        }
    }
}
