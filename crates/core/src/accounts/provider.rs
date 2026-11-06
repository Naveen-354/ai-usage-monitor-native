//! Everything that is specific to one agent, behind one trait, so the manager (and with it the CLI and the desktop UI) never
//! mentions an agent by name. Adding an agent means one new adapter and one line in `providers::registry()`.
//!
//! Adapters only describe what the agent *officially* offers: its own sign-in command, its own status command, and the
//! environment variable it documents for keeping a second profile apart. Where an agent has none of that, the adapter says
//! so and the manager reports "switching unavailable" - it never invents a workaround.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::runner::StatusOutput;
use crate::collectors::Env;

/// The folder a managed account lives in. `home` is a synthetic home directory: the agent is pointed at a place inside it
/// and the collectors read usage from the same place, so one folder is both the account's sign-in and its history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDirs {
    pub home: PathBuf,
}

impl ProfileDirs {
    /// `profile_dir` is the account's folder as stored (`<data>/accounts/<agent>/<id>`).
    pub fn new(profile_dir: &Path) -> ProfileDirs {
        ProfileDirs { home: profile_dir.join("home") }
    }
}

/// The environment the collectors use for one managed account: the same machine, but `home` is the profile's.
pub fn account_env(system: &Env, profile: &ProfileDirs) -> Env {
    Env { home: profile.home.clone(), xdg_data_home: None, ..system.clone() }
}

/// How a sign-in is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoginMethod {
    /// The agent's normal sign-in (a browser window, or the agent's own first-run screen).
    Standard,
    /// A code typed on another device, for a machine with no browser (`codex login --device-auth`).
    DeviceCode,
    /// An API key. Agents that store a key themselves get it on stdin; the others get it from the OS credential store as an
    /// environment variable at launch.
    ApiKey,
}

impl LoginMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            LoginMethod::Standard => "standard",
            LoginMethod::DeviceCode => "device-code",
            LoginMethod::ApiKey => "api-key",
        }
    }
}

/// What an agent can do about accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// The documented environment variable that points the agent at a separate profile - what makes a second account
    /// possible. `None` = the agent has no supported way, so only its default sign-in can be monitored.
    pub isolation: Option<&'static str>,
    /// Why not, in a sentence a user can read (shown in the CLI and the UI).
    pub unavailable_reason: Option<&'static str>,
    pub login_methods: &'static [LoginMethod],
    /// Is there a way to ask whether the account is signed in (a status command or a file whose presence tells)?
    pub can_check_auth: bool,
    /// For agents that read an API key from an environment variable: its name.
    pub api_key_env: Option<&'static str>,
}

/// Switching, as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchSupport {
    Supported { mechanism: &'static str },
    Unavailable { reason: &'static str },
}

impl Capabilities {
    pub fn switch_support(&self) -> SwitchSupport {
        match (self.isolation, self.unavailable_reason) {
            (Some(mechanism), _) => SwitchSupport::Supported { mechanism },
            (None, Some(reason)) => SwitchSupport::Unavailable { reason },
            (None, None) => SwitchSupport::Unavailable { reason: "this agent has no supported way to keep a second account" },
        }
    }
}

/// What a status check concluded. Holds only what is needed, never the raw output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Observation {
    /// `Some(true)` signed in, `Some(false)` signed out, `None` = could not tell.
    pub signed_in: Option<bool>,
    /// An e-mail or user name the agent itself reported. Display only.
    pub identity: Option<String>,
}

impl Observation {
    pub fn signed_in(identity: Option<String>) -> Observation {
        Observation { signed_in: Some(true), identity: identity.and_then(clean_identity) }
    }

    pub fn signed_out() -> Observation {
        Observation { signed_in: Some(false), identity: None }
    }

    pub fn unknown() -> Observation {
        Observation::default()
    }
}

/// A reported identity made safe to store and show: one line, printable, short.
pub fn clean_identity(s: String) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.chars().any(char::is_control) {
        return None;
    }
    Some(s.chars().take(120).collect())
}

/// Removes ANSI colour/cursor sequences so a coloured status line can be read as text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub trait AccountProvider: Send + Sync {
    fn agent_id(&self) -> &'static str;

    /// Names of the agent's executable, for finding it on PATH.
    fn binary_names(&self) -> &'static [&'static str];

    /// Where else the agent's executable is commonly installed.
    fn extra_search_dirs(&self, _env: &Env) -> Vec<PathBuf> {
        Vec::new()
    }

    fn capabilities(&self) -> Capabilities;

    /// The environment that makes the agent use `profile` instead of the machine's own sign-in. Only called when
    /// `capabilities().isolation` is set.
    fn profile_env(&self, _profile: &ProfileDirs) -> Vec<(&'static str, OsString)> {
        Vec::new()
    }

    /// The agent's own sign-in command line for `method` (arguments only). Empty arguments mean "start the agent itself",
    /// for agents that sign in on first start. `None` = this method has no command (API keys held by us).
    fn login_args(&self, _method: LoginMethod) -> Option<Vec<OsString>> {
        None
    }

    /// Does this sign-in command read the API key from stdin (and store it itself)?
    fn login_reads_key_from_stdin(&self, _method: LoginMethod) -> bool {
        false
    }

    /// The agent's own "am I signed in" command (arguments only).
    fn status_args(&self) -> Option<Vec<OsString>> {
        None
    }

    /// Reads what that command printed.
    fn interpret_status(&self, _out: &StatusOutput) -> Observation {
        Observation::unknown()
    }

    /// Files whose mere *existence* says a sign-in is present, for agents without a status command. Never read.
    fn credential_markers(&self, _profile: &ProfileDirs) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes_are_removed_from_status_text() {
        assert_eq!(strip_ansi("\u{1b}[90mC:\\x\u{1b}[0m \u{1b}[1m0 credentials\u{1b}[0m"), "C:\\x 0 credentials");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn identities_are_one_printable_line_and_short() {
        assert_eq!(clean_identity("  me@example.com ".into()).as_deref(), Some("me@example.com"));
        assert_eq!(clean_identity("a\nb".into()), None);
        assert_eq!(clean_identity("   ".into()), None);
        assert_eq!(clean_identity("x".repeat(500)).unwrap().len(), 120);
    }

    #[test]
    fn a_collector_sees_the_profile_as_its_home_but_keeps_the_machines_path() {
        let mut sys = Env::with_home("/home/me");
        sys.path_dirs = vec!["/usr/bin".into()];
        sys.xdg_data_home = Some("/data".into());
        let env = account_env(&sys, &ProfileDirs::new(Path::new("/accounts/codex/work")));
        assert_eq!(env.home, PathBuf::from("/accounts/codex/work").join("home"));
        assert_eq!(env.path_dirs, vec![PathBuf::from("/usr/bin")], "the agent's executable is still found");
        assert!(env.xdg_data_home.is_none(), "so OpenCode's data folder is inside the profile too");
        assert_eq!(env.xdg_data(), PathBuf::from("/accounts/codex/work").join("home").join(".local").join("share"));
    }

    #[test]
    fn switch_support_explains_itself() {
        let on = Capabilities { isolation: Some("X_HOME"), unavailable_reason: None, login_methods: &[], can_check_auth: false, api_key_env: None };
        assert_eq!(on.switch_support(), SwitchSupport::Supported { mechanism: "X_HOME" });
        let off = Capabilities { isolation: None, unavailable_reason: Some("because"), ..on.clone() };
        assert_eq!(off.switch_support(), SwitchSupport::Unavailable { reason: "because" });
        let silent = Capabilities { isolation: None, unavailable_reason: None, ..on };
        assert!(matches!(silent.switch_support(), SwitchSupport::Unavailable { .. }), "unknown means unavailable, never supported");
    }
}
