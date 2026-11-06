//! What an account *is*: metadata only. Nothing in this file (or in the table it maps to) can hold a password, token,
//! cookie or API key - the sign-in itself lives where the agent keeps it, or in the OS credential store.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::DEFAULT_ACCOUNT;

/// The account that already exists on every machine (the agent's own sign-in) versus one this app set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// The agent's normal sign-in, outside any profile this app manages. It is never created, moved or deleted by us.
    Default,
    /// An account with its own profile folder; the agent is pointed at that folder to use it.
    Managed,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Default => "default",
            AccountKind::Managed => "managed",
        }
    }

    pub fn parse(s: &str) -> Option<AccountKind> {
        match s {
            "default" => Some(AccountKind::Default),
            "managed" => Some(AccountKind::Managed),
            _ => None,
        }
    }
}

/// What we last learned about whether the account can be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    /// The agent says it is signed in.
    Valid,
    /// It *was* signed in and the agent now says it is not (session expired or revoked): sign in again.
    Expired,
    /// There is no sign-in (never completed, or the agent's data is missing).
    NotLoggedIn,
    /// A sign-in has been started and has not finished.
    Pending,
    /// Not checked yet, or the agent offers no way to check.
    Unknown,
}

impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthState::Valid => "valid",
            AuthState::Expired => "expired",
            AuthState::NotLoggedIn => "not_logged_in",
            AuthState::Pending => "pending",
            AuthState::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<AuthState> {
        match s {
            "valid" => Some(AuthState::Valid),
            "expired" => Some(AuthState::Expired),
            "not_logged_in" => Some(AuthState::NotLoggedIn),
            "pending" => Some(AuthState::Pending),
            "unknown" => Some(AuthState::Unknown),
            _ => None,
        }
    }

    /// Short upper-case label for tables and tags.
    pub fn label(self) -> &'static str {
        match self {
            AuthState::Valid => "SIGNED IN",
            AuthState::Expired => "EXPIRED",
            AuthState::NotLoggedIn => "NOT SIGNED IN",
            AuthState::Pending => "SIGN-IN PENDING",
            AuthState::Unknown => "NOT CHECKED",
        }
    }

    /// Can the account be used to run the agent as far as we know? (`Unknown` is given the benefit of the doubt: the agent
    /// itself will say if it cannot.)
    pub fn is_usable(self) -> bool {
        matches!(self, AuthState::Valid | AuthState::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    pub agent_id: String,
    /// Short slug, unique within the agent and never reused (`default` for the agent's own sign-in).
    pub account_id: String,
    /// What the user calls it.
    pub label: String,
    pub kind: AccountKind,
    /// A managed account's profile folder (the synthetic home the agent is pointed at).
    pub profile_dir: Option<PathBuf>,
    /// E-mail or user name, when the agent's own status command tells us. Display only.
    pub identity: Option<String>,
    pub auth: AuthState,
    /// A short plain-language reason; never raw agent output.
    pub auth_detail: Option<String>,
    pub auth_checked_utc_ms: Option<i64>,
    pub created_utc_ms: i64,
    pub last_used_utc_ms: Option<i64>,
    pub removed_utc_ms: Option<i64>,
}

impl Account {
    pub fn is_default(&self) -> bool {
        self.account_id == DEFAULT_ACCOUNT
    }

    pub fn is_removed(&self) -> bool {
        self.removed_utc_ms.is_some()
    }

    /// `agent/account`, the form the CLI and logs use.
    pub fn display_ref(&self) -> String {
        format!("{}/{}", self.agent_id, self.account_id)
    }
}

/// An account's address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountRef {
    pub agent: String,
    pub account: String,
}

impl AccountRef {
    pub fn new(agent: impl Into<String>, account: impl Into<String>) -> Self {
        AccountRef { agent: agent.into(), account: account.into() }
    }
}

/// Turns a label into an id: lower-case ASCII letters and digits separated by single dashes ("My Work!" -> "my-work").
/// Never empty and never the reserved `default`.
pub fn slugify(label: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in label.trim().chars() {
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    if out.len() > 32 {
        out.truncate(32);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() || out == DEFAULT_ACCOUNT {
        out = if out.is_empty() { "account".to_string() } else { "default-2".to_string() };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_safe_ascii_and_never_the_reserved_name() {
        assert_eq!(slugify("Work"), "work");
        assert_eq!(slugify("  My Work Account!  "), "my-work-account");
        assert_eq!(slugify("a//b..c"), "a-b-c");
        assert_eq!(slugify("..\\..\\evil"), "evil", "path tricks cannot survive");
        assert_eq!(slugify("名前"), "account", "nothing usable left");
        assert_eq!(slugify(""), "account");
        assert_eq!(slugify("default"), "default-2", "`default` is reserved for the agent's own sign-in");
        assert_eq!(slugify("Default"), "default-2");
        assert!(slugify(&"x".repeat(100)).len() <= 32);
        assert!(!slugify("a".repeat(31).as_str().to_owned().as_str()).is_empty());
    }

    #[test]
    fn states_round_trip_through_their_stored_text() {
        for s in [AuthState::Valid, AuthState::Expired, AuthState::NotLoggedIn, AuthState::Pending, AuthState::Unknown] {
            assert_eq!(AuthState::parse(s.as_str()), Some(s));
        }
        for k in [AccountKind::Default, AccountKind::Managed] {
            assert_eq!(AccountKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(AuthState::parse("whatever"), None);
    }

    #[test]
    fn only_a_known_good_or_unchecked_account_is_called_usable() {
        assert!(AuthState::Valid.is_usable() && AuthState::Unknown.is_usable());
        assert!(!AuthState::Expired.is_usable() && !AuthState::NotLoggedIn.is_usable() && !AuthState::Pending.is_usable());
    }
}
