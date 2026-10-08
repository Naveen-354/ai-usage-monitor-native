use serde::Serialize;

/// Static presentation metadata for an agent. Behaviour lives in `collectors/`; this is only
/// what the UI and database need to know that an agent exists. Adding an agent means adding a
/// collector module and one row here.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMeta {
    pub id: &'static str,
    pub name: &'static str,
    /// Short label for the compact overlay.
    pub short: &'static str,
    /// Accent colour (hex). Chosen to stay distinguishable on both themes.
    pub color: &'static str,
}

const CATALOG: &[AgentMeta] = &[
    AgentMeta { id: "codex", name: "Codex", short: "CODEX", color: "#00c2a8" },
    AgentMeta { id: "claude", name: "Claude Code", short: "CLAUDE", color: "#ff6a3d" },
    AgentMeta { id: "gemini", name: "Gemini CLI", short: "GEMINI", color: "#4d7cff" },
    AgentMeta { id: "antigravity", name: "Antigravity", short: "AGY", color: "#c14dff" },
    AgentMeta { id: "opencode", name: "OpenCode", short: "OPENCODE", color: "#f2c200" },
    AgentMeta { id: "ollama", name: "Ollama", short: "OLLAMA", color: "#8a8a8a" },
    AgentMeta { id: "aider", name: "Aider", short: "AIDER", color: "#2fd45a" },
];

pub fn catalog() -> &'static [AgentMeta] {
    CATALOG
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_colours_are_hex() {
        let ids: HashSet<_> = catalog().iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), catalog().len());
        assert!(catalog().iter().all(|a| a.color.starts_with('#') && a.color.len() == 7));
    }
}
