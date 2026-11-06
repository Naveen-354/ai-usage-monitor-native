//! Gemini CLI. Official fact used here (Gemini CLI configuration reference): `GEMINI_CLI_HOME` is "the root directory for
//! the CLI's user-level configuration and storage"; the CLI creates its `.gemini` folder inside it. Gemini CLI has no
//! separate sign-in command - it asks on first start - and no status command, so a sign-in is recognised by its file's
//! *existence* (the file is never read). A key can be given instead through `GEMINI_API_KEY`.
//!
//! Not verified on this machine: the CLI is not installed here. The behaviour below follows its documentation.

use std::ffi::OsString;
use std::path::PathBuf;

use super::super::provider::{AccountProvider, Capabilities, LoginMethod, ProfileDirs};

pub struct GeminiProvider;

impl AccountProvider for GeminiProvider {
    fn agent_id(&self) -> &'static str {
        "gemini"
    }

    fn binary_names(&self) -> &'static [&'static str] {
        &["gemini"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            isolation: Some("GEMINI_CLI_HOME"),
            unavailable_reason: None,
            login_methods: &[LoginMethod::Standard, LoginMethod::ApiKey],
            can_check_auth: true, // by the presence of its sign-in file
            api_key_env: Some("GEMINI_API_KEY"),
        }
    }

    fn profile_env(&self, profile: &ProfileDirs) -> Vec<(&'static str, OsString)> {
        vec![("GEMINI_CLI_HOME", profile.home.clone().into_os_string())]
    }

    fn login_args(&self, method: LoginMethod) -> Option<Vec<OsString>> {
        // No arguments: start Gemini CLI itself and sign in on its first screen, then leave with /quit.
        (method == LoginMethod::Standard).then(Vec::new)
    }

    fn credential_markers(&self, profile: &ProfileDirs) -> Vec<PathBuf> {
        vec![profile.home.join(".gemini").join("oauth_creds.json")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_is_the_cli_home_and_the_sign_in_marker_is_inside_its_dot_gemini() {
        let p = ProfileDirs::new(std::path::Path::new("/d/accounts/gemini/work"));
        let env = GeminiProvider.profile_env(&p);
        assert_eq!(env[0].0, "GEMINI_CLI_HOME");
        assert_eq!(PathBuf::from(&env[0].1), p.home);
        assert_eq!(GeminiProvider.credential_markers(&p), vec![p.home.join(".gemini").join("oauth_creds.json")]);
    }

    #[test]
    fn signing_in_is_the_cli_itself_and_an_api_key_is_kept_by_us() {
        assert_eq!(GeminiProvider.login_args(LoginMethod::Standard), Some(vec![]));
        assert_eq!(GeminiProvider.login_args(LoginMethod::ApiKey), None);
        assert_eq!(GeminiProvider.capabilities().api_key_env, Some("GEMINI_API_KEY"));
        assert!(GeminiProvider.status_args().is_none(), "no status command exists");
    }
}
