//! SQLite persistence. One writer connection (serialised by a mutex) and one read-only connection,
//! both in WAL mode so UI reads never block ingestion.

mod migrate;
pub mod queries;
mod recovery;
mod writer;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags};

use crate::error::Result;

pub use migrate::{current_version, LATEST_VERSION};
pub use recovery::OpenReport;
pub use writer::{project_key, Batch, CommitStats, CursorUpdate, IdCache, WriterState};

pub struct Database {
    path: PathBuf,
    writer: Mutex<WriterState>,
    reader: Mutex<Connection>,
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL: readers never block the writer. NORMAL sync is safe under WAL and much cheaper.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Keep the page cache small: the monitor must stay well under its RAM budget.
    conn.pragma_update(None, "cache_size", -8192)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// A fast structural check at start-up. `quick_check` is O(file size), so very large files skip it (damage found
/// later, at runtime, still triggers recovery on the next start).
fn verify(conn: &Connection, path: &Path) -> Result<()> {
    if fs::metadata(path).map(|m| m.len()).unwrap_or(0) > recovery::QUICK_CHECK_MAX_BYTES {
        return Ok(());
    }
    let first: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if first == "ok" {
        return Ok(());
    }
    Err(crate::error::AppError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
        Some(first),
    )))
}

impl Database {
    pub fn open(path: &Path) -> Result<Arc<Database>> {
        Self::open_with_report(path).map(|(db, _)| db)
    }

    /// Open (and, if the file is damaged, quarantine + recreate) the database. See `recovery`.
    pub fn open_with_report(path: &Path) -> Result<(Arc<Database>, OpenReport)> {
        match Self::open_inner(path) {
            Ok(db) => Ok((db, OpenReport::default())),
            Err(e) if e.is_corruption() => {
                tracing::error!("database is damaged ({e}); setting it aside and rebuilding");
                let quarantined = recovery::quarantine(path)?;
                let settings = recovery::read_settings(&quarantined);
                let db = Self::open_inner(path)?;
                if let Some(json) = &settings {
                    db.with_writer(|w| {
                        w.conn.execute(
                            "INSERT INTO settings (key, value) VALUES ('app', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                            [json],
                        )?;
                        Ok(())
                    })?;
                }
                Ok((db, OpenReport { quarantined: Some(quarantined), settings_carried_over: settings.is_some() }))
            }
            Err(e) => Err(e),
        }
    }

    fn open_inner(path: &Path) -> Result<Arc<Database>> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        verify(&conn, path)?;
        migrate::migrate(&mut conn)?;
        writer::ensure_agents(&conn)?;

        let reader = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        reader.busy_timeout(std::time::Duration::from_secs(5))?;
        reader.pragma_update(None, "query_only", "ON")?;
        reader.pragma_update(None, "cache_size", -4096)?;

        Ok(Arc::new(Database {
            path: path.to_path_buf(),
            writer: Mutex::new(WriterState { conn, cache: IdCache::default() }),
            reader: Mutex::new(reader),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fold the WAL back into the main file and truncate it: called on a clean exit so the database is a single
    /// self-contained file whenever the app is not running.
    pub fn checkpoint(&self) -> Result<()> {
        self.with_writer(|w| {
            w.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            Ok(())
        })
    }

    fn lock_writer(&self) -> MutexGuard<'_, WriterState> {
        // A panic while holding the lock must not brick the app: recover the guard.
        self.writer.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn with_writer<T>(&self, f: impl FnOnce(&mut WriterState) -> Result<T>) -> Result<T> {
        let mut guard = self.lock_writer();
        f(&mut guard)
    }

    pub fn with_reader<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.reader.lock().unwrap_or_else(|p| p.into_inner());
        f(&guard)
    }

    /// Atomically store a batch of events plus the cursor positions that produced them.
    pub fn commit(&self, batch: &Batch) -> Result<CommitStats> {
        self.with_writer(|state| writer::commit(state, batch))
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// Opens a fresh database in a temp dir. The returned guard must outlive the database.
    pub fn temp_db() -> (tempfile::TempDir, Arc<Database>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("usage.db")).unwrap();
        (dir, db)
    }
}
