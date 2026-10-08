use serde::{Serialize, Serializer};

pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// Crate-wide error. Serialises to a plain string so Tauri commands can return it to the UI.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Is the *database file itself* damaged (as opposed to a normal failure or a newer-version file)?
    pub fn is_corruption(&self) -> bool {
        matches!(
            self,
            AppError::Sqlite(rusqlite::Error::SqliteFailure(e, _))
                if matches!(e.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)
        )
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl From<String> for AppError {
    fn from(v: String) -> Self {
        AppError::Other(v)
    }
}
