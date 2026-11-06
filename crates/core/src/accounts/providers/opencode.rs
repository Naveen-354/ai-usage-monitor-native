//! OpenCode. Official facts used here (verified against `opencode 1.18` on this machine, using an empty profile folder):
//! it keeps credentials and its session database under `$XDG_DATA_HOME/opencode`; `opencode auth login` adds a provider
//! credential; `opencode auth list` prints the folder and "N credentials" and exits 0 either way.

use std::ffi::OsString;

use super::super::provider::{strip_ansi, AccountProvider, Capabilities, LoginMethod, Observation, ProfileDirs};
use super::super::runner::StatusOutput;

pub struct OpenCodeProvider;

/// The number in "N credential(s)", from the coloured text `opencode auth list` prints.
fn credential_count(text: &str) -> Option<u32> {
    let plain = strip_ansi(text);
    let words: Vec<&str> = plain.split_whitespace().collect();
    words.windows(2).find_map(|w| w[1].to_lowercase().starts_with("credential").then(|| w[0].parse().ok()).flatten())
}

impl AccountProvider for OpenCodeProvider {
    fn agent_id(&self) -> &'static str {
        "opencode"
    }

    fn binary_names(&self) -> &'static [&'static str] {
        &["opencode"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            isolation: Some("XDG_DATA_HOME"),
            unavailable_reason: None,
            login_methods: &[LoginMethod::Standard],
            can_check_auth: true,
            api_key_env: None, // `opencode auth login` lets you paste a provider key and stores it itself
        }
    }

    fn profile_env(&self, profile: &ProfileDirs) -> Vec<(&'static str, OsString)> {
        vec![("XDG_DATA_HOME", profile.home.join(".local").join("share").into_os_string())]
    }

    fn login_args(&self, method: LoginMethod) -> Option<Vec<OsString>> {
        (method == LoginMethod::Standard).then(|| vec!["auth".into(), "login".into()])
    }

    fn status_args(&self) -> Option<Vec<OsString>> {
        Some(vec!["auth".into(), "list".into()])
    }

    fn interpret_status(&self, out: &StatusOutput) -> Observation {
        if out.timed_out || out.exit_code != Some(0) {
            return Observation::unknown();
        }
        match credential_count(&format!("{}\n{}", out.stdout, out.stderr)) {
            Some(0) => Observation::signed_out(),
            Some(_) => Observation::signed_in(None),
            None => Observation::unknown(),
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
    fn the_real_empty_listing_means_signed_out() {
        // exactly the text (with its colour codes) `opencode auth list` printed against an empty XDG_DATA_HOME
        let text = "\u{1b}[0m\nT  Credentials \u{1b}[90mC:\\scratch\\opencode\\auth.json\u{1b}[0m\n|\n\u{2014}  0 credentials\n";
        assert_eq!(OpenCodeProvider.interpret_status(&out(0, "", text)), Observation::signed_out());
    }

    #[test]
    fn one_or_more_credentials_means_signed_in_and_names_nobody() {
        assert_eq!(OpenCodeProvider.interpret_status(&out(0, "|\n\u{2014}  2 credentials\n", "")), Observation::signed_in(None));
        assert_eq!(OpenCodeProvider.interpret_status(&out(0, "1 credential", "")).signed_in, Some(true));
    }

    #[test]
    fn unreadable_or_failed_output_is_unknown() {
        assert_eq!(OpenCodeProvider.interpret_status(&out(0, "something else entirely", "")), Observation::unknown());
        assert_eq!(OpenCodeProvider.interpret_status(&out(1, "0 credentials", "")), Observation::unknown());
    }

    #[test]
    fn the_profile_points_the_data_folder_inside_the_accounts_own_folder() {
        let env = OpenCodeProvider.profile_env(&ProfileDirs::new(std::path::Path::new("/d/accounts/opencode/work")));
        assert_eq!(env[0].0, "XDG_DATA_HOME");
        assert_eq!(std::path::PathBuf::from(&env[0].1), std::path::PathBuf::from("/d/accounts/opencode/work").join("home").join(".local").join("share"));
    }
}
