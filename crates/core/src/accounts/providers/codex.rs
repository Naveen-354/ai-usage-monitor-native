//! Codex CLI. Official facts used here (verified against `codex 0.147` on this machine, using an empty profile folder):
//! `CODEX_HOME` is the folder Codex keeps its sign-in and sessions in; `codex login` signs in (browser), `--device-auth` uses
//! a code, `--with-api-key` reads a key from stdin and stores it itself; `codex login status` prints "Not logged in" and
//! exits 1 when signed out.

use std::ffi::OsString;

use super::super::provider::{AccountProvider, Capabilities, LoginMethod, Observation, ProfileDirs};
use super::super::runner::StatusOutput;

pub struct CodexProvider;

impl AccountProvider for CodexProvider {
    fn agent_id(&self) -> &'static str {
        "codex"
    }

    fn binary_names(&self) -> &'static [&'static str] {
        &["codex"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            isolation: Some("CODEX_HOME"),
            unavailable_reason: None,
            login_methods: &[LoginMethod::Standard, LoginMethod::DeviceCode, LoginMethod::ApiKey],
            can_check_auth: true,
            api_key_env: None, // Codex stores a key itself (`login --with-api-key`); nothing for us to keep
        }
    }

    fn profile_env(&self, profile: &ProfileDirs) -> Vec<(&'static str, OsString)> {
        vec![("CODEX_HOME", profile.home.join(".codex").into_os_string())]
    }

    fn login_args(&self, method: LoginMethod) -> Option<Vec<OsString>> {
        Some(match method {
            LoginMethod::Standard => vec!["login".into()],
            LoginMethod::DeviceCode => vec!["login".into(), "--device-auth".into()],
            LoginMethod::ApiKey => vec!["login".into(), "--with-api-key".into()],
        })
    }

    fn login_reads_key_from_stdin(&self, method: LoginMethod) -> bool {
        method == LoginMethod::ApiKey
    }

    fn status_args(&self) -> Option<Vec<OsString>> {
        Some(vec!["login".into(), "status".into()])
    }

    fn interpret_status(&self, out: &StatusOutput) -> Observation {
        if out.timed_out {
            return Observation::unknown();
        }
        let text = format!("{}\n{}", out.stdout, out.stderr).to_lowercase();
        if text.contains("not logged in") {
            Observation::signed_out()
        } else if out.exit_code == Some(0) && text.contains("logged in") {
            Observation::signed_in(None) // the status line names the method, not the person
        } else {
            Observation::unknown()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> StatusOutput {
        StatusOutput { exit_code: Some(code), stdout: stdout.into(), stderr: stderr.into(), timed_out: false }
    }

    #[test]
    fn the_profile_points_codex_home_inside_the_accounts_own_folder() {
        let env = CodexProvider.profile_env(&ProfileDirs::new(std::path::Path::new("/d/accounts/codex/work")));
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CODEX_HOME");
        assert_eq!(std::path::PathBuf::from(&env[0].1), std::path::PathBuf::from("/d/accounts/codex/work").join("home").join(".codex"));
    }

    #[test]
    fn the_real_signed_out_message_is_understood() {
        // exactly what `codex login status` printed against an empty CODEX_HOME
        assert_eq!(CodexProvider.interpret_status(&out(1, "", "Not logged in\n")), Observation::signed_out());
        assert_eq!(CodexProvider.interpret_status(&out(1, "Not logged in", "")), Observation::signed_out());
    }

    #[test]
    fn a_logged_in_message_needs_a_clean_exit() {
        assert_eq!(CodexProvider.interpret_status(&out(0, "Logged in using ChatGPT\n", "")).signed_in, Some(true));
        assert_eq!(CodexProvider.interpret_status(&out(0, "Logged in using an API key", "")).signed_in, Some(true));
        assert_eq!(CodexProvider.interpret_status(&out(2, "Logged in using ChatGPT", "")).signed_in, None, "a failed run proves nothing");
    }

    #[test]
    fn a_crash_or_timeout_is_not_mistaken_for_either_state() {
        assert_eq!(CodexProvider.interpret_status(&out(101, "", "thread panicked")).signed_in, None);
        let timed_out = StatusOutput { timed_out: true, ..Default::default() };
        assert_eq!(CodexProvider.interpret_status(&timed_out), Observation::unknown());
    }

    #[test]
    fn the_api_key_goes_to_the_agent_on_stdin_never_on_the_command_line() {
        assert!(CodexProvider.login_reads_key_from_stdin(LoginMethod::ApiKey));
        assert!(!CodexProvider.login_reads_key_from_stdin(LoginMethod::Standard));
        let args = CodexProvider.login_args(LoginMethod::ApiKey).unwrap();
        assert_eq!(args, vec![OsString::from("login"), OsString::from("--with-api-key")], "no key among the arguments");
    }
}
