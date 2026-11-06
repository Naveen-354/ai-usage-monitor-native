//! Errors of the account layer. Messages are written for people and never contain a secret: nothing here ever sees one.

use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("unknown agent '{0}'")]
    UnknownAgent(String),
    #[error("agent '{agent}' has no account '{account}'")]
    UnknownAccount { agent: String, account: String },
    #[error("there is already an account called '{0}' for this agent")]
    DuplicateLabel(String),
    #[error("{0}")]
    InvalidLabel(String),
    /// The agent has no supported way to keep a second sign-in, so we do not pretend to switch.
    #[error("{agent}: {reason}")]
    SwitchingUnavailable { agent: String, reason: String },
    #[error("the default account is the agent's own sign-in and cannot be removed here")]
    CannotRemoveDefault,
    #[error("'{0}' is not installed (it was not found on PATH)")]
    NotInstalled(String),
    #[error("sign-in did not complete: {0}")]
    LoginFailed(String),
    #[error("the {0} account is not signed in; run `agm login {0}` again")]
    NeedsLogin(String),
    /// Another `agm` command or the app is changing this agent's accounts right now.
    #[error("another change to {0}'s accounts is in progress; try again in a moment")]
    Busy(String),
    #[error("credential store: {0}")]
    Secret(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Db(#[from] AppError),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type AccountResult<T> = Result<T, AccountError>;
