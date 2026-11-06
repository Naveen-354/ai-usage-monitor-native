//! Multi-account management.
//!
//! Every agent has a **default** account (the sign-in it already uses on this machine) and can have any number of
//! **managed** accounts when it has a supported way to be pointed at a separate profile. The pieces:
//!
//! * [`model`], [`store`] - what an account is and where that is kept (the shared database).
//! * [`provider`], [`providers`] - what is specific to one agent: how to sign in, how to ask whether it is signed in,
//!   which environment makes it use a profile, and whether switching is possible at all.
//! * [`runner`] - the only place processes are started (and the seam tests replace).
//! * [`secrets`] - the OS credential store, for the few secrets we may hold.
//! * [`manager`] - the operations (`login`, `use`, `remove`, `run`, ...) shared by the CLI and the desktop UI.
//!
//! Security rules this module keeps: it never reads a credential file (only whether one exists), never puts a secret
//! in the database, a log line or an error message, and never rewrites an agent's own configuration or sign-in -
//! "switching" only changes which profile folder the *next* launch of the agent is pointed at.

mod error;
pub mod lock;
pub mod manager;
pub mod model;
pub mod provider;
pub mod providers;
pub mod runner;
pub mod secrets;
pub mod store;

pub use error::{AccountError, AccountResult};
pub use model::{slugify, Account, AccountKind, AccountRef, AuthState};
pub use lock::AgentLock;
pub use manager::{account_collector_envs, AccountManager, AgentAccounts, LoginPlan, UseOutcome};
pub use provider::{account_env, AccountProvider, Capabilities, LoginMethod, Observation, ProfileDirs, SwitchSupport};
pub use runner::{ProcessRunner, ProcessSpec, StatusOutput, SystemRunner};
pub use secrets::{KeyringSecretStore, MemorySecretStore, Secret, SecretKey, SecretStore};
pub use store::{clean_label, AccountStore, MAX_LABEL};

pub use crate::model::DEFAULT_ACCOUNT;
