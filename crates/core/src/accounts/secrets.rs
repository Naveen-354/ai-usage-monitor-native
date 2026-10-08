//! The OS credential store (Windows Credential Manager, macOS Keychain, Linux keyutils) for the few secrets this app may
//! hold: an API key the user gives us for an agent that reads its key from an environment variable.
//!
//! A sign-in obtained through an agent's own flow (OAuth, device code, `codex login`, ...) is *not* handled here: the agent
//! keeps it in its own profile folder and we never read it. Secrets are wrapped in [`Secret`], which cannot be printed.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use super::error::{AccountError, AccountResult};

/// A value that must never reach a log, an error message, a database or the screen. `Debug` and `Display` both redact it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The one way to get at the value: used when handing it to the OS credential store or to a child process.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Which secret: the API key of one account of one agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey {
    pub agent: String,
    pub account: String,
}

impl SecretKey {
    pub fn api_key(agent: &str, account: &str) -> SecretKey {
        SecretKey { agent: agent.into(), account: account.into() }
    }

    /// The name the entry has in the OS credential store.
    fn user(&self) -> String {
        format!("{}/{}/api-key", self.agent, self.account)
    }
}

pub const SERVICE: &str = "ai-usage-monitor";

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &SecretKey, value: &Secret) -> AccountResult<()>;
    fn get(&self, key: &SecretKey) -> AccountResult<Option<Secret>>;
    /// Deleting something that is not there is not an error.
    fn delete(&self, key: &SecretKey) -> AccountResult<()>;
    /// Does this store really persist secrets in the OS? (`false` for the in-memory test store.)
    fn is_persistent(&self) -> bool;
}

/// The real thing. On a platform without a native credential store the library would silently keep secrets in memory,
/// so there [`KeyringSecretStore::available`] is `false` and the manager refuses API-key accounts instead.
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    pub fn available() -> bool {
        cfg!(any(windows, target_os = "macos", target_os = "linux"))
    }

    fn entry(key: &SecretKey) -> AccountResult<keyring::Entry> {
        keyring::Entry::new(SERVICE, &key.user()).map_err(|e| AccountError::Secret(e.to_string()))
    }
}

impl SecretStore for KeyringSecretStore {
    fn set(&self, key: &SecretKey, value: &Secret) -> AccountResult<()> {
        if !Self::available() {
            return Err(AccountError::Secret("this platform has no OS credential store".into()));
        }
        Self::entry(key)?.set_password(value.expose()).map_err(|e| AccountError::Secret(e.to_string()))
    }

    fn get(&self, key: &SecretKey) -> AccountResult<Option<Secret>> {
        match Self::entry(key)?.get_password() {
            Ok(v) => Ok(Some(Secret::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AccountError::Secret(e.to_string())),
        }
    }

    fn delete(&self, key: &SecretKey) -> AccountResult<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AccountError::Secret(e.to_string())),
        }
    }

    fn is_persistent(&self) -> bool {
        Self::available()
    }
}

/// For tests: remembers secrets in memory only.
#[derive(Default)]
pub struct MemorySecretStore {
    items: Mutex<HashMap<SecretKey, Secret>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &SecretKey, value: &Secret) -> AccountResult<()> {
        self.items.lock().map_err(|_| AccountError::Secret("poisoned".into()))?.insert(key.clone(), value.clone());
        Ok(())
    }

    fn get(&self, key: &SecretKey) -> AccountResult<Option<Secret>> {
        Ok(self.items.lock().map_err(|_| AccountError::Secret("poisoned".into()))?.get(key).cloned())
    }

    fn delete(&self, key: &SecretKey) -> AccountResult<()> {
        self.items.lock().map_err(|_| AccountError::Secret("poisoned".into()))?.remove(key);
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_cannot_be_printed_by_accident() {
        let s = Secret::new("sk-very-secret-value");
        assert_eq!(format!("{s}"), "[redacted]");
        assert_eq!(format!("{s:?}"), "Secret([redacted])");
        assert!(!format!("{:?}", Some(s.clone())).contains("very-secret"));
        assert_eq!(s.expose(), "sk-very-secret-value");
    }

    #[test]
    fn the_memory_store_round_trips_and_deleting_twice_is_fine() {
        let store = MemorySecretStore::new();
        let k = SecretKey::api_key("claude", "work");
        assert!(store.get(&k).unwrap().is_none());
        store.set(&k, &Secret::new("abc")).unwrap();
        assert_eq!(store.get(&k).unwrap().unwrap().expose(), "abc");
        assert!(store.get(&SecretKey::api_key("claude", "other")).unwrap().is_none(), "another account's key is separate");
        store.delete(&k).unwrap();
        store.delete(&k).unwrap();
        assert!(store.is_empty());
        assert!(!store.is_persistent());
    }

    #[test]
    fn the_store_names_do_not_collide_between_agents_or_accounts() {
        assert_ne!(SecretKey::api_key("claude", "work").user(), SecretKey::api_key("codex", "work").user());
        assert_ne!(SecretKey::api_key("claude", "a").user(), SecretKey::api_key("claude", "b").user());
    }

    /// Talks to the real OS credential store (one temporary entry, removed at the end). Ignored by default because a
    /// headless machine may have no credential store; run it with `cargo test -- --ignored os_credential_store`.
    #[test]
    #[ignore]
    fn os_credential_store_round_trip() {
        let store = KeyringSecretStore;
        let k = SecretKey::api_key("test-agent", &format!("round-trip-{}", std::process::id()));
        store.set(&k, &Secret::new("not-a-real-key")).unwrap();
        assert_eq!(store.get(&k).unwrap().unwrap().expose(), "not-a-real-key");
        store.delete(&k).unwrap();
        assert!(store.get(&k).unwrap().is_none());
    }
}
