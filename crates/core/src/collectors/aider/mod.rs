//! Aider collector — reports **unavailable**, and says why.
//!
//! Aider is not installed on the development machine (no pip/pipx/uv package, nothing on PATH, no `~/.aider*`),
//! so its on-disk format could not be inspected, and the product rule is to never implement a collector
//! from memory. What is publicly known — a per-project `.aider.chat.history.md` that prints rounded values like
//! "1.2k sent" — would at best be `Estimated`, and finding those files would mean scanning arbitrary project
//! folders, which this app deliberately never does. TASK-032 tracks the real collector once Aider is available.

use super::{
    AgentCollector, AgentInfo, BatchSink, CollectCtx, CollectError, CollectSummary, Detection, Env, Presence, WatchSpec,
};
use crate::model::AgentId;

pub const ID: AgentId = "aider";

pub const REASON: &str = "Aider's usage format has not been inspected (it is not installed on the monitored machine). Its \
per-project history prints rounded values and is scattered across project folders, which this app does not scan. \
Shown as unavailable rather than estimated.";

pub struct AiderCollector;

/// Where pip, pipx and uv put an `aider-chat` install (directories only; nothing inside is read).
fn tool_dirs(env: &Env) -> Vec<std::path::PathBuf> {
    let mut v = vec![
        env.home.join(".local").join("pipx").join("venvs").join("aider-chat"),
        env.home.join("pipx").join("venvs").join("aider-chat"),
        env.home.join(".local").join("share").join("uv").join("tools").join("aider-chat"),
    ];
    if let Some(a) = &env.app_data {
        v.push(a.join("uv").join("tools").join("aider-chat"));
    }
    if let Some(l) = &env.local_app_data {
        v.push(l.join("uv").join("tools").join("aider-chat"));
        v.push(l.join("pipx").join("pipx").join("venvs").join("aider-chat"));
    }
    v
}

impl AgentCollector for AiderCollector {
    fn id(&self) -> AgentId {
        ID
    }

    fn get_agent_info(&self) -> AgentInfo {
        AgentInfo {
            id: ID,
            name: "Aider",
            data_sources: &["none inspected"],
            caveats: &[REASON, "A real collector is tracked as TASK-032 (needs Aider installed to inspect)."],
        }
    }

    fn detect(&self, env: &Env) -> Detection {
        let extra = vec![env.home.join(".local").join("bin")];
        let binary = env.which(&["aider"], &extra);
        let installed = binary.is_some() || tool_dirs(env).iter().any(|d| d.is_dir());
        Detection { presence: if installed { Presence::Installed } else { Presence::NotFound }, roots: vec![], binary, note: None }
    }

    fn watch_specs(&self, _d: &Detection) -> Vec<WatchSpec> {
        Vec::new()
    }

    fn collect_usage(&self, _ctx: &CollectCtx<'_>, _sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
        Err(CollectError::Unavailable(REASON.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::collectors::testkit::{Fixture, VecSink};

    #[test]
    fn absent_when_nothing_is_installed() {
        let d = tempfile::tempdir().unwrap();
        // an unrelated ~/.aider.* file must not be treated as installation or be read
        std::fs::write(d.path().join(".aider.conf.yml"), "SECRET").unwrap();
        assert_eq!(AiderCollector.detect(&Env::with_home(d.path())).presence, Presence::NotFound);
    }

    #[test]
    fn found_via_path_or_tool_directories() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "aider.exe" } else { "aider" }), b"").unwrap();
        let mut env = Env::with_home(d.path());
        env.path_dirs = vec![bin];
        assert_eq!(AiderCollector.detect(&env).presence, Presence::Installed);

        let d2 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d2.path().join(".local").join("share").join("uv").join("tools").join("aider-chat")).unwrap();
        assert_eq!(AiderCollector.detect(&Env::with_home(d2.path())).presence, Presence::Installed);
    }

    #[test]
    fn collecting_reports_unavailable_and_emits_nothing() {
        let d = tempfile::tempdir().unwrap();
        let fx = Fixture::new(d.path());
        let det = AiderCollector.detect(&fx.env);
        let cursors = HashMap::new();
        let mut sink = VecSink::default();
        assert!(matches!(AiderCollector.collect_usage(&fx.ctx(&det, &cursors), &mut sink), Err(CollectError::Unavailable(_))));
        assert!(sink.events.is_empty());
    }
}
