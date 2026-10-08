//! Agents that are monitored but have no supported way to keep a second account. They still have a default account, so their
//! usage is attributed and shown like any other; switching is reported as unavailable, with the reason.

use std::path::PathBuf;

use super::super::provider::{AccountProvider, Capabilities};
use crate::collectors::Env;

pub struct PassiveProvider {
    pub id: &'static str,
    pub binaries: &'static [&'static str],
    pub reason: &'static str,
}

impl PassiveProvider {
    /// Antigravity (`agy`). Its own documentation describes a single sign-in kept in the operating system's credential store
    /// and no profile mechanism; the multi-account tools that exist work by rewriting that store entry or by overriding the
    /// user's home directory for the whole process. Neither is a supported way, so neither is used here.
    pub fn antigravity() -> PassiveProvider {
        PassiveProvider {
            id: "antigravity",
            binaries: &["agy", "antigravity"],
            reason: "Antigravity keeps one sign-in in the operating system's credential store and has no supported way to use a second profile. Switching would mean rewriting that entry, which this app will not do. The signed-in account is still monitored.",
        }
    }

    pub fn ollama() -> PassiveProvider {
        PassiveProvider { id: "ollama", binaries: &["ollama"], reason: "Ollama runs on this machine and has no accounts." }
    }

    pub fn aider() -> PassiveProvider {
        PassiveProvider {
            id: "aider",
            binaries: &["aider"],
            reason: "Aider has no sign-in of its own: it uses API keys from your environment, so there is no account to switch.",
        }
    }
}

impl AccountProvider for PassiveProvider {
    fn agent_id(&self) -> &'static str {
        self.id
    }

    fn binary_names(&self) -> &'static [&'static str] {
        self.binaries
    }

    fn extra_search_dirs(&self, env: &Env) -> Vec<PathBuf> {
        match self.id {
            "antigravity" => env.local_app_data.iter().map(|d| d.join("agy").join("bin")).chain([env.home.join(".local").join("bin")]).collect(),
            _ => vec![env.home.join(".local").join("bin")],
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { isolation: None, unavailable_reason: Some(self.reason), login_methods: &[], can_check_auth: false, api_key_env: None }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::provider::SwitchSupport;
    use super::*;

    #[test]
    fn the_monitor_only_agents_say_why_they_cannot_switch() {
        for p in [PassiveProvider::antigravity(), PassiveProvider::ollama(), PassiveProvider::aider()] {
            let caps = p.capabilities();
            assert!(caps.isolation.is_none() && caps.login_methods.is_empty() && !caps.can_check_auth);
            match caps.switch_support() {
                SwitchSupport::Unavailable { reason } => assert!(reason.len() > 20, "{}: {reason}", p.id),
                other => panic!("{} must not claim switching: {other:?}", p.id),
            }
        }
    }

    #[test]
    fn antigravitys_reason_names_the_unsafe_workaround_it_refuses() {
        let r = PassiveProvider::antigravity().reason;
        assert!(r.contains("credential store") && r.contains("will not"), "{r}");
    }

    #[test]
    fn antigravity_is_searched_where_its_installer_puts_it() {
        let mut env = Env::with_home("/h");
        env.local_app_data = Some("/local".into());
        let dirs = PassiveProvider::antigravity().extra_search_dirs(&env);
        assert!(dirs.contains(&PathBuf::from("/local").join("agy").join("bin")));
    }
}
