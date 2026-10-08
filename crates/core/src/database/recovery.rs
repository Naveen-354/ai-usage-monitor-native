//! Self-healing for a damaged database file.
//!
//! The history in `usage.db` is *derived* data: every event can be re-read from the agents' own files. So when
//! the file is damaged the right move is to keep the broken file for inspection (never delete it), start a fresh
//! database, carry the user's settings over if they can still be read, and let the collectors rebuild the history.
//! A database written by a *newer* version of the app is not damaged and is never touched (see `AppError::is_corruption`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// How many quarantined files to keep (the oldest are deleted beyond this).
pub const KEEP_QUARANTINED: usize = 3;

/// Above this size the start-up `quick_check` is skipped (it is O(size)); runtime errors still trigger recovery.
pub const QUICK_CHECK_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// Where the damaged file was moved, if recovery happened.
    pub quarantined: Option<PathBuf>,
    pub settings_carried_over: bool,
}

impl OpenReport {
    /// One plain-language sentence for Diagnostics, or `None` when nothing happened.
    pub fn notice(&self) -> Option<String> {
        let q = self.quarantined.as_ref()?;
        Some(format!(
            "The database file was damaged, so it was set aside and rebuilt from your agents' own data{}. Damaged copy kept at: {}",
            if self.settings_carried_over { " (your settings were kept)" } else { " (settings were reset)" },
            q.display()
        ))
    }
}

fn sidecar(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Move `usage.db` and its `-wal`/`-shm` to `usage.db.corrupt-<stamp>[-wal|-shm]`. SQLite derives the WAL name
/// from the database name, so the three files stay a coherent set that can still be opened for inspection.
pub fn quarantine(path: &Path) -> io::Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let target = sidecar(path, &format!(".corrupt-{stamp}"));
    // Same-second collisions (tests, crash loops): keep every copy.
    let target = (0..100).map(|i| if i == 0 { target.clone() } else { sidecar(&target, &format!("-{i}")) }).find(|t| !t.exists()).unwrap_or(target);
    fs::rename(path, &target)?;
    for ext in ["-wal", "-shm"] {
        let from = sidecar(path, ext);
        if from.exists() {
            let _ = fs::rename(&from, sidecar(&target, ext));
        }
    }
    prune(path);
    Ok(target)
}

/// Delete all but the newest [`KEEP_QUARANTINED`] quarantined sets.
fn prune(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return };
    let prefix = format!("{}.corrupt-", name.to_string_lossy());
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut sets: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix) && !n.ends_with("-wal") && !n.ends_with("-shm"))
        .collect();
    sets.sort(); // timestamps sort chronologically
    while sets.len() > KEEP_QUARANTINED {
        let old = sets.remove(0);
        for ext in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(dir.join(format!("{old}{ext}")));
        }
    }
}

/// The user's settings document from a damaged database, if that one small table is still readable.
pub fn read_settings(damaged: &Path) -> Option<String> {
    let c = Connection::open_with_flags(damaged, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let json: Option<String> = c.query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0)).optional().ok()?;
    // Only trust it if it is still valid JSON.
    json.filter(|j| serde_json::from_str::<serde_json::Value>(j).is_ok())
}

#[cfg(test)]
mod tests {
    use super::super::Database;
    use super::*;
    use crate::database::{Batch, CursorUpdate};
    use crate::model::{Accuracy, UsageEvent};

    fn events(n: usize) -> Vec<UsageEvent> {
        (0..n)
            .map(|i| UsageEvent {
                agent: "codex",
                model: "m".into(),
                ts_utc_ms: 1_790_000_000_000 + i as i64 * 1000,
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: None,
                session_id: Some(format!("session-{}", i % 7)),
                project: None,
                source: "t",
                accuracy: Accuracy::Real,
                dedupe_key: format!("codex:a-reasonably-long-dedupe-key-to-fill-pages-quickly:{i:06}"),
            })
            .collect()
    }

    fn make_db(path: &Path, n: usize) {
        let db = Database::open(path).unwrap();
        db.with_writer(|w| {
            w.conn.execute("INSERT INTO settings (key, value) VALUES ('app', '{\"theme\":\"light\",\"alwaysOnTop\":false}')", [])?;
            Ok(())
        })
        .unwrap();
        db.commit(&Batch { agent: "codex", events: events(n), cursors: vec![CursorUpdate { path: "p".into(), size: 1, mtime_ms: 1, offset: 1, state: None }] }).unwrap();
        db.checkpoint().unwrap();
    }

    fn event_count(db: &Database) -> i64 {
        db.with_reader(|c| Ok(c.query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn a_healthy_database_opens_untouched() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        make_db(&p, 50);
        let (db, report) = Database::open_with_report(&p).unwrap();
        assert_eq!(report, OpenReport::default());
        assert_eq!(event_count(&db), 50);
        assert!(fs::read_dir(d.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("corrupt")));
    }

    #[test]
    fn a_file_that_is_not_a_database_is_set_aside_and_replaced() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        fs::write(&p, b"this is definitely not an sqlite database, just text".repeat(100)).unwrap();
        let (db, report) = Database::open_with_report(&p).unwrap();
        let q = report.quarantined.clone().expect("must be quarantined");
        assert!(q.exists(), "the damaged file is kept, never deleted");
        assert!(!report.settings_carried_over);
        assert_eq!(event_count(&db), 0);
        // the fresh database is fully usable
        db.commit(&Batch { agent: "codex", events: events(3), cursors: vec![] }).unwrap();
        assert_eq!(event_count(&db), 3);
        assert!(report.notice().unwrap().contains("settings were reset"));
    }

    #[test]
    fn damaged_pages_are_detected_and_settings_are_carried_over() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        make_db(&p, 1500);
        // Trash the newest pages (event data), leaving the schema and the small settings table intact.
        let mut bytes = fs::read(&p).unwrap();
        assert!(bytes.len() > 4096 * 12, "fixture should span many pages, got {}", bytes.len());
        let n = bytes.len();
        for b in &mut bytes[n - 4096 * 3..] {
            *b = 0xA7;
        }
        fs::write(&p, &bytes).unwrap();
        for ext in ["-wal", "-shm"] {
            let _ = fs::remove_file(sidecar(&p, ext));
        }

        let (db, report) = Database::open_with_report(&p).unwrap();
        assert!(report.quarantined.is_some(), "quick_check must notice the damage");
        assert!(report.settings_carried_over, "the settings table was untouched and must survive");
        let settings: String = db.with_reader(|c| Ok(c.query_row("SELECT value FROM settings WHERE key='app'", [], |r| r.get(0))?)).unwrap();
        assert!(settings.contains("light"));
        assert_eq!(event_count(&db), 0, "history is rebuilt from the agents, not trusted from a damaged file");
    }

    #[test]
    fn a_database_from_a_newer_version_is_never_quarantined() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        make_db(&p, 10);
        {
            let c = Connection::open(&p).unwrap();
            c.execute("INSERT INTO schema_migrations VALUES (99, 'future', 0)", []).unwrap();
        }
        let err = Database::open_with_report(&p).err().expect("must refuse, not wipe");
        assert!(err.to_string().contains("newer"), "{err}");
        assert!(p.exists(), "the newer database must be left exactly where it was");
        assert!(fs::read_dir(d.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("corrupt")));
    }

    #[test]
    fn only_the_newest_quarantined_copies_are_kept() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        for _ in 0..(KEEP_QUARANTINED + 3) {
            fs::write(&p, b"junk").unwrap();
            quarantine(&p).unwrap();
        }
        let kept = fs::read_dir(d.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".corrupt-")).count();
        assert_eq!(kept, KEEP_QUARANTINED);
    }

    #[test]
    fn a_wal_that_belongs_to_the_damaged_file_moves_with_it() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("usage.db");
        fs::write(&p, b"junk").unwrap();
        fs::write(sidecar(&p, "-wal"), b"wal").unwrap();
        fs::write(sidecar(&p, "-shm"), b"shm").unwrap();
        let q = quarantine(&p).unwrap();
        assert!(sidecar(&q, "-wal").exists() && sidecar(&q, "-shm").exists());
        assert!(!sidecar(&p, "-wal").exists(), "a stale WAL must not be paired with the fresh database");
    }

    #[test]
    fn the_notice_is_plain_language_and_absent_when_nothing_happened() {
        assert!(OpenReport::default().notice().is_none());
        let r = OpenReport { quarantined: Some(PathBuf::from("/x/usage.db.corrupt-1")), settings_carried_over: true };
        let n = r.notice().unwrap();
        assert!(n.contains("rebuilt from your agents") && n.contains("settings were kept") && n.contains("usage.db.corrupt-1"));
    }
}
