//! The heat map's OPEN CMD button: a command prompt that starts one agent's command-line tool.
//!
//! The command comes from a fixed table, never from user input or from the database, because it is handed to `cmd.exe`.

use std::ffi::OsStr;
use std::path::PathBuf;

/// The program that starts `agent_id` in a terminal, or `None` for an agent that has no command-line tool to start.
///
/// Antigravity is an editor and Ollama needs a model name, so there is nothing sensible to run for them. Every id of the
/// catalogue is either here or in [`NO_COMMAND`] (a test enforces it), so a new agent gets a deliberate answer.
pub fn command_for(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "gemini" => Some("gemini"),
        "opencode" => Some("opencode"),
        "aider" => Some("aider"),
        _ => None,
    }
}

/// Agents the table above deliberately leaves out.
#[cfg(test)]
const NO_COMMAND: [&str; 2] = ["antigravity", "ollama"];

/// What Windows tries to run for a bare command name when `PATHEXT` is not set.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The file `cmd.exe` would run for `program`: the first directory of `path` that holds `program` plus one of the
/// `pathext` extensions (`.exe`, `.cmd`, ...). A file without an extension does not count: `cmd.exe` cannot run it, and
/// npm leaves one of those beside every `.cmd` it installs.
pub fn find_on_path(program: &str, path: &OsStr, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').map(str::trim).filter(|e| !e.is_empty()).collect();
    std::env::split_paths(path).find_map(|dir| exts.iter().map(|ext| dir.join(format!("{program}{ext}"))).find(|candidate| candidate.is_file()))
}

/// Opens a new command prompt window, in the user's home folder, that runs `agent_id`'s tool and stays open when the
/// tool exits. Fails, without opening anything, when the agent has no command-line tool or it is not on `PATH`.
pub fn open(agent_id: &str) -> Result<(), String> {
    let program = command_for(agent_id).ok_or_else(|| format!("{agent_id} has no command-line tool to start"))?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_string());
    if find_on_path(program, &path, &pathext).is_none() {
        return Err(format!("`{program}` was not found on PATH (is it installed?)"));
    }
    spawn(program)
}

#[cfg(windows)]
fn spawn(program: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let mut cmd = std::process::Command::new("cmd.exe");
    // `/K` runs the command and keeps the prompt, so the window does not vanish when the tool exits.
    cmd.arg("/K").arg(program).creation_flags(CREATE_NEW_CONSOLE);
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("could not start cmd.exe: {e}"))
}

#[cfg(not(windows))]
fn spawn(_program: &str) -> Result<(), String> {
    Err("opening a command prompt is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_monitor_core::model::catalog;

    #[test]
    fn every_agent_of_the_catalogue_is_either_launchable_or_deliberately_not() {
        for meta in catalog() {
            let launchable = command_for(meta.id).is_some();
            let excluded = NO_COMMAND.contains(&meta.id);
            assert!(launchable != excluded, "{}: it must be in exactly one of the table and NO_COMMAND", meta.id);
        }
        for id in NO_COMMAND {
            assert!(catalog().iter().any(|m| m.id == id), "NO_COMMAND names an agent that does not exist: {id}");
        }
    }

    #[test]
    fn a_command_is_a_bare_program_name_so_nothing_can_be_smuggled_into_cmd() {
        for meta in catalog() {
            if let Some(c) = command_for(meta.id) {
                assert!(!c.is_empty() && c.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'), "{c:?}");
            }
        }
        assert_eq!(command_for("claude"), Some("claude"));
        assert_eq!(command_for("gemini"), Some("gemini"));
        assert_eq!(command_for("claude & calc"), None, "only the fixed ids are accepted");
        assert_eq!(command_for(""), None);
    }

    fn touch(dir: &std::path::Path, name: &str) {
        std::fs::write(dir.join(name), b"").unwrap();
    }

    /// A path as text, lower-cased: the result is spelled with the extension as PATHEXT writes it (`.EXE`), which on a
    /// case-insensitive file system is the same file as `.exe`.
    fn text(p: impl Into<Option<PathBuf>>) -> Option<String> {
        p.into().map(|p| p.to_string_lossy().to_lowercase())
    }

    #[test]
    fn the_first_directory_with_a_runnable_file_wins_and_extensionless_files_do_not_count() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        touch(a.path(), "claude"); // npm's shell shim: cmd.exe cannot run it
        touch(a.path(), "codex.cmd");
        touch(b.path(), "claude.exe");
        touch(b.path(), "codex.exe");
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();

        assert_eq!(text(find_on_path("claude", &path, ".COM;.EXE;.BAT;.CMD")), text(b.path().join("claude.exe")), "the bare `claude` in the first folder is skipped");
        assert_eq!(text(find_on_path("codex", &path, ".COM;.EXE;.BAT;.CMD")), text(a.path().join("codex.cmd")), "the earlier folder wins");
        assert_eq!(find_on_path("gemini", &path, ".COM;.EXE;.BAT;.CMD"), None);
        assert_eq!(find_on_path("claude", &path, ".CMD"), None, "only the extensions PATHEXT lists");
        assert_eq!(find_on_path("claude", &path, ""), None, "no extensions, nothing runnable");
        assert_eq!(find_on_path("claude", OsStr::new(""), ".EXE"), None, "an empty PATH finds nothing");
    }

    #[test]
    fn extension_lists_tolerate_stray_separators_and_spaces() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "aider.bat");
        let path = std::env::join_paths([d.path()]).unwrap();
        assert_eq!(text(find_on_path("aider", &path, ";.EXE; .BAT ;")), text(d.path().join("aider.bat")));
    }

    #[test]
    fn an_agent_without_a_tool_is_refused_before_anything_is_started() {
        let e = open("ollama").unwrap_err();
        assert!(e.contains("no command-line tool"), "{e}");
        assert!(open("no-such-agent").is_err());
    }
}
