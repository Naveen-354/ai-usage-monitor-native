//! Claude Code. Official facts used here (verified against `claude 2.1` on this machine, using an empty profile folder):
//! `CLAUDE_CONFIG_DIR` relocates its config, history and sign-in; `claude auth login` signs in; `claude auth status` prints
//! a small JSON document (`loggedIn`, `authMethod`, ...) and exits 1 when signed out. An API key is read from
//! `ANTHROPIC_API_KEY`, which we keep in the OS credential store and set only for the one launch.

use std::ffi::OsString;
use std::path::PathBuf;

use serde::Deserialize;

use super::super::provider::{AccountProvider, Capabilities, LoginMethod, Observation, ProfileDirs};
use super::super::runner::StatusOutput;
use crate::collectors::Env;

pub struct ClaudeProvider;

/// The only fields we take from `claude auth status`; everything else in the document is dropped unread.
#[derive(Deserialize)]
struct Status {
    #[serde(rename = "loggedIn", default)]
    logged_in: bool,
    #[serde(default)]
    email: Option<String>,
}

impl AccountProvider for ClaudeProvider {
    fn agent_id(&self) -> &'static str {
        "claude"
    }

    fn binary_names(&self) -> &'static [&'static str] {
        &["claude"]
    }

    fn extra_search_dirs(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.home.join(".local").join("bin"), env.home.join(".claude").join("local")]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            isolation: Some("CLAUDE_CONFIG_DIR"),
            unavailable_reason: None,
            login_methods: &[LoginMethod::Standard, LoginMethod::ApiKey],
            can_check_auth: true,
            api_key_env: Some("ANTHROPIC_API_KEY"),
        }
    }

    fn profile_env(&self, profile: &ProfileDirs) -> Vec<(&'static str, OsString)> {
        vec![("CLAUDE_CONFIG_DIR", profile.home.join(".claude").into_os_string())]
    }

    fn login_args(&self, method: LoginMethod) -> Option<Vec<OsString>> {
        (method == LoginMethod::Standard).then(|| vec!["auth".into(), "login".into()])
    }

    fn status_args(&self) -> Option<Vec<OsString>> {
        Some(vec!["auth".into(), "status".into()])
    }

    fn interpret_status(&self, out: &StatusOutput) -> Observation {
        if out.timed_out {
            return Observation::unknown();
        }
        // The JSON may be surrounded by other text: take from the first `{` to the last `}`.
        let (Some(start), Some(end)) = (out.stdout.find('{'), out.stdout.rfind('}')) else {
            return Observation::unknown();
        };
        match serde_json::from_str::<Status>(&out.stdout[start..=end]) {
            Ok(s) if s.logged_in => Observation::signed_in(s.email),
            Ok(_) => Observation::signed_out(),
            Err(_) => Observation::unknown(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> StatusOutput {
        StatusOutput { exit_code: Some(code), stdout: stdout.into(), stderr: String::new(), timed_out: false }
    }

    #[test]
    fn the_real_signed_out_document_is_understood() {
        // exactly what `claude auth status` printed against an empty CLAUDE_CONFIG_DIR
        let doc = "{\n  \"loggedIn\": false,\n  \"authMethod\": \"none\",\n  \"apiProvider\": \"firstParty\"\n}\n";
        assert_eq!(ClaudeProvider.interpret_status(&out(1, doc)), Observation::signed_out());
    }

    #[test]
    fn a_signed_in_document_gives_the_identity_and_nothing_else() {
        let doc = r#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "email": "me@example.com", "orgName": "Acme", "subscriptionType": "max"}"#;
        let obs = ClaudeProvider.interpret_status(&out(0, doc));
        assert_eq!(obs, Observation { signed_in: Some(true), identity: Some("me@example.com".into()) });
    }

    #[test]
    fn text_around_the_document_does_not_matter_and_garbage_is_unknown() {
        assert_eq!(ClaudeProvider.interpret_status(&out(0, "note: update available\n{\"loggedIn\": true}\n")).signed_in, Some(true));
        assert_eq!(ClaudeProvider.interpret_status(&out(0, "not json at all")), Observation::unknown());
        assert_eq!(ClaudeProvider.interpret_status(&out(0, "{ broken")), Observation::unknown());
        assert_eq!(ClaudeProvider.interpret_status(&out(0, "")), Observation::unknown());
    }

    #[test]
    fn a_multi_line_or_empty_email_is_not_kept_as_an_identity() {
        let obs = ClaudeProvider.interpret_status(&out(0, "{\"loggedIn\": true, \"email\": \"a\\nb\"}"));
        assert_eq!(obs, Observation { signed_in: Some(true), identity: None });
    }

    #[test]
    fn the_profile_points_claude_config_dir_inside_the_accounts_own_folder() {
        let env = ClaudeProvider.profile_env(&ProfileDirs::new(std::path::Path::new("/d/accounts/claude/work")));
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        assert_eq!(PathBuf::from(&env[0].1), PathBuf::from("/d/accounts/claude/work").join("home").join(".claude"));
        assert_eq!(ClaudeProvider.capabilities().api_key_env, Some("ANTHROPIC_API_KEY"));
        assert_eq!(ClaudeProvider.login_args(LoginMethod::ApiKey), None, "an API key has no sign-in command; it is kept by us");
    }
}
