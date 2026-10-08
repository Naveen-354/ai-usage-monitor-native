//! Ollama collector — reports **unavailable**, and says why.
//!
//! Ollama keeps no persistent usage history. Token counts (`prompt_eval_count`, `eval_count`) exist only in
//! the live HTTP responses of `/api/generate` and `/api/chat`; the server log records requests but not counts.
//! There is therefore nothing on disk to read, and inventing numbers would break the product's core rule.
//! (TASK-031 tracks an opt-in local proxy that could capture *real* counts; it cannot be built or verified
//! until Ollama is installed.)
//!
//! Detection touches nothing but file metadata and one TCP connect to localhost: the socket is closed
//! immediately, with no bytes sent or read.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence,
};
use crate::model::AgentId;

pub const ID: AgentId = "ollama";
const DEFAULT_ADDR: &str = "127.0.0.1:11434";
const PROBE_TIMEOUT: Duration = Duration::from_millis(150);

pub const REASON: &str = "Ollama keeps no usage history: token counts exist only in live API responses, so there is nothing \
on disk to read. Shown as unavailable rather than estimated.";

pub struct OllamaCollector {
    addr: SocketAddr,
}

impl OllamaCollector {
    pub fn new() -> Self {
        OllamaCollector { addr: DEFAULT_ADDR.parse().expect("valid default address") }
    }

    /// For tests: probe another local address.
    #[cfg(test)]
    fn at(addr: SocketAddr) -> Self {
        OllamaCollector { addr }
    }

    /// Is something listening where Ollama's server would be? Connect only; send and read nothing.
    fn daemon_running(&self) -> bool {
        self.addr.ip().is_loopback() && TcpStream::connect_timeout(&self.addr, PROBE_TIMEOUT).is_ok()
    }
}

impl Default for OllamaCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentCollector for OllamaCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Ollama",
            data_sources: &["none: no usage ledger exists"],
            caveats: &[REASON, "An opt-in local proxy that records real counts is planned (TASK-031)."],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let mut extra = vec![env.home.join(".local").join("bin"), "/usr/local/bin".into(), "/opt/homebrew/bin".into(), "/usr/bin".into()];
        if let Some(l) = &env.local_app_data {
            extra.push(l.join("Programs").join("Ollama"));
        }
        let binary = env.which(&["ollama"], &extra);
        let running = self.daemon_running();
        let has_models = std::fs::read_dir(env.home.join(".ollama").join("models")).map(|mut d| d.next().is_some()).unwrap_or(false);
        let presence = if binary.is_some() || running {
            Presence::Installed
        } else if has_models {
            Presence::DataOnly
        } else {
            Presence::NotFound // stray config/keys in ~/.ollama do not make it "installed"
        };
        Detection {
            presence,
            roots: vec![],
            note: running.then(|| "server is running on localhost:11434".to_string()),
            binary,
        }
    }

    fn watch_specs(&self, _d: &Detection) -> Vec<super::WatchSpec> {
        Vec::new() // nothing to watch
    }

    fn collect_usage(&self, _ctx: &CollectCtx<'_>, _sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        Err(CollectError::Unavailable(REASON.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::TcpListener;

    use super::*;
    use crate::collectors::testkit::{Fixture, VecSink};
    use crate::collectors::RunOutcome;
    use crate::model::Availability;

    fn closed_port() -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        a
    }

    #[test]
    fn absent_when_nothing_is_installed_running_or_downloaded() {
        let d = tempfile::tempdir().unwrap();
        // leftover config and keys, as on the author's machine, must not count as "installed"
        std::fs::create_dir_all(d.path().join(".ollama")).unwrap();
        std::fs::write(d.path().join(".ollama").join("config.json"), "{}").unwrap();
        let det = OllamaCollector::at(closed_port()).detect(&Env::with_home(d.path()));
        assert_eq!(det.presence, Presence::NotFound);
    }

    #[test]
    fn a_running_server_means_installed() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let d = tempfile::tempdir().unwrap();
        let det = OllamaCollector::at(l.local_addr().unwrap()).detect(&Env::with_home(d.path()));
        assert_eq!(det.presence, Presence::Installed);
        assert!(det.note.unwrap().contains("running"));
    }

    #[test]
    fn downloaded_models_without_a_binary_mean_data_only() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".ollama").join("models").join("blobs")).unwrap();
        assert_eq!(OllamaCollector::at(closed_port()).detect(&Env::with_home(d.path())).presence, Presence::DataOnly);
    }

    #[test]
    fn a_non_local_address_is_never_probed() {
        let c = OllamaCollector::at("192.0.2.1:11434".parse().unwrap());
        assert!(!c.daemon_running(), "only loopback addresses may ever be contacted");
    }

    #[test]
    fn collecting_is_unavailable_with_the_reason_and_emits_nothing() {
        let d = tempfile::tempdir().unwrap();
        let fx = Fixture::new(d.path());
        let c = OllamaCollector::at(closed_port());
        let det = c.detect(&fx.env);
        let cursors = HashMap::new();
        let mut sink = VecSink::default();
        match c.collect_usage(&fx.ctx(&det, &cursors), &mut sink) {
            Err(CollectError::Unavailable(r)) => assert!(r.contains("no usage history")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
        assert!(sink.events.is_empty() && sink.cursors.is_empty());
    }

    #[test]
    fn health_maps_to_unavailable_when_present_and_not_installed_when_absent() {
        let present = Detection { presence: Presence::Installed, roots: vec![], binary: None, note: None };
        let h = OllamaCollector::new().get_health(&present, Some(&RunOutcome::Unavailable(REASON.into())), 0, None);
        assert!(matches!(h.availability, Availability::Unavailable { .. }));
        let h = OllamaCollector::new().get_health(&Detection::not_found(), None, 0, None);
        assert_eq!(h.availability, Availability::NotInstalled);
    }
}
