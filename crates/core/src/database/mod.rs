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

/// Is this a "somebody else has the file right now" error? (Windows reports a sharing violation as an I/O error.)
fn is_contention(e: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode::{DatabaseBusy, DatabaseLocked, SystemIoFailure};
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if matches!(f.code, DatabaseBusy | DatabaseLocked | SystemIoFailure))
}

fn configure(conn: &Connection) -> Result<()> {
    // The wait comes FIRST: the app and several `agm` processes may open the same file at the same moment, and every statement
    // below - the switch to WAL included - must wait for the others instead of failing at once.
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    // WAL: readers never block the writer. NORMAL sync is safe under WAL and much cheaper. Switching the file to WAL needs a
    // moment of exclusive access, which the busy timeout does not cover on every platform, so it is retried briefly.
    let mut attempt = 0;
    loop {
        match conn.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => break,
            Err(e) if is_contention(&e) && attempt < 60 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 + 10 * attempt.min(20)));
            }
            Err(e) => return Err(e.into()),
        }
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Keep the page cache small: the monitor must stay well under its RAM budget.
    conn.pragma_update(None, "cache_size", -8192)?;
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

/// Before a database that already holds history is upgraded to a newer schema, a consistent snapshot of it is kept next to it
/// (`usage.db.v1.bak` for a version-1 file). The upgrade is itself transactional and lossless, but an older build cannot read
/// a newer file, and a user who goes back should find their history exactly as it was. Never fatal: a failure is logged and the
/// upgrade carries on. One snapshot per old version is kept, and an existing one is never overwritten.
fn backup_before_upgrade(conn: &Connection, path: &Path) {
    let Ok(version) = migrate::current_version(conn) else { return };
    if version == 0 || version >= LATEST_VERSION {
        return; // a brand-new file, or already current
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "usage.db".into());
    let backup = path.with_file_name(format!("{name}.v{version}.bak"));
    if backup.exists() {
        return;
    }
    match conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()]) {
        Ok(_) => tracing::info!(from = version, "kept a copy of the database before upgrading it"),
        Err(e) => tracing::warn!("could not keep a copy of the database before upgrading it: {e}"),
    }
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
        backup_before_upgrade(&conn, path);
        migrate::migrate(&mut conn)?;
        writer::ensure_agents(&conn)?;

        let reader = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        reader.busy_timeout(std::time::Duration::from_secs(10))?;
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

    /// Atomically store a batch of events plus the cursor positions that produced them, filed under the agent's
    /// default account.
    pub fn commit(&self, batch: &Batch) -> Result<CommitStats> {
        self.commit_for(crate::model::DEFAULT_ACCOUNT, batch)
    }

    /// Like [`Database::commit`], but the events belong to `account` of the batch's agent.
    pub fn commit_for(&self, account: &str, batch: &Batch) -> Result<CommitStats> {
        self.with_writer(|state| writer::commit(state, batch, account))
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

#[cfg(test)]
mod concurrency_tests {
    use super::*;

    /// The desktop app and any number of `agm` commands open the same database file, often at the same moment (a script that
    /// switches several agents; a login finishing while the app starts). Every one of them must get a working database.
    #[test]
    fn many_connections_opening_a_new_database_at_the_same_moment_all_succeed() {
        for round in 0..4 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("usage.db");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    let (path, barrier) = (path.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        Database::open(&path).map(|db| db.with_reader(|c| Ok(c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get::<_, i64>(0))?)).unwrap())
                    })
                })
                .collect();
            for h in handles {
                let version = h.join().unwrap().unwrap_or_else(|e| panic!("round {round}: a connection could not open the database: {e}"));
                assert_eq!(version, LATEST_VERSION);
            }
        }
    }

    /// A database written by the previous release is upgraded, and a snapshot of it as it was is kept.
    #[test]
    fn upgrading_an_existing_database_keeps_a_snapshot_of_it_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(include_str!("../../migrations/0001_init.sql")).unwrap();
            c.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_utc_ms INTEGER NOT NULL); INSERT INTO schema_migrations VALUES (1, 'init', 0);").unwrap();
            c.execute("INSERT INTO agents (id, display_name) VALUES ('codex', 'Codex')", []).unwrap();
            c.execute("INSERT INTO models (id, agent_id, name) VALUES (1, 'codex', 'm')", []).unwrap();
            c.execute(
                "INSERT INTO usage_events (dedupe_key, agent_id, model_id, ts_utc_ms, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, accuracy, source)
                 VALUES ('k', 'codex', 1, 1000, 42, 0, 0, 0, 'actual', 's')",
                [],
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        let version: i64 = db.with_reader(|c| Ok(c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))?)).unwrap();
        assert_eq!(version, LATEST_VERSION, "the real database was upgraded");

        let backup = dir.path().join("usage.db.v1.bak");
        assert!(backup.is_file(), "a snapshot was kept");
        let old = Connection::open(&backup).unwrap();
        let (v, tokens): (i64, i64) = old
            .query_row("SELECT (SELECT MAX(version) FROM schema_migrations), (SELECT SUM(input_tokens) FROM usage_events)", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((v, tokens), (1, 42), "the snapshot is the old schema with the old history");
        assert!(old.prepare("SELECT account_id FROM usage_events").is_err(), "and it has none of the new columns");
    }

    #[test]
    fn no_snapshot_is_made_for_a_new_or_an_already_current_database_and_an_existing_one_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        drop(Database::open(&path).unwrap()); // brand new
        drop(Database::open(&path).unwrap()); // already current
        assert!(std::fs::read_dir(dir.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".bak")));
        // an existing snapshot is never overwritten
        let bak = dir.path().join("usage.db.v1.bak");
        std::fs::write(&bak, b"precious").unwrap();
        let c = Connection::open(dir.path().join("old.db")).unwrap();
        c.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_utc_ms INTEGER NOT NULL); INSERT INTO schema_migrations VALUES (1, 'init', 0);").unwrap();
        backup_before_upgrade(&c, &dir.path().join("usage.db"));
        assert_eq!(std::fs::read(&bak).unwrap(), b"precious");
    }
}
