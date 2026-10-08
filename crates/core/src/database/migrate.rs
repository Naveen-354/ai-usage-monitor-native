//! Forward-only migrations. Applied versions are recorded; a database written by a *newer* build is
//! refused rather than silently downgraded, so user history is never damaged.

use rusqlite::Connection;

use crate::error::{AppError, Result};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    sql: include_str!("../../migrations/0001_init.sql"),
}];

pub const LATEST_VERSION: i64 = 1;

pub fn current_version(conn: &Connection) -> Result<i64> {
    ensure_table(conn)?;
    Ok(conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?)
}

fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_utc_ms INTEGER NOT NULL
        )",
    )?;
    Ok(())
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_with(conn, MIGRATIONS)
}

fn migrate_with(conn: &mut Connection, migrations: &[Migration]) -> Result<()> {
    let current = current_version(conn)?;
    let newest_known = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    if current > newest_known {
        return Err(AppError::Migration(format!(
            "database schema v{current} is newer than this build supports (v{newest_known}); \
             refusing to open it to protect your history"
        )));
    }
    for m in migrations.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql).map_err(|e| {
            AppError::Migration(format!("migration {:04}_{} failed: {e}", m.version, m.name))
        })?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_utc_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![m.version, m.name, chrono::Utc::now().timestamp_millis()],
        )?;
        tx.commit()?;
        tracing::info!(version = m.version, name = m.name, "applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        c
    }

    #[test]
    fn fresh_database_migrates_to_latest() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        assert_eq!(current_version(&c).unwrap(), LATEST_VERSION);
        for table in [
            "agents", "models", "projects", "sessions", "usage_events", "usage_buckets",
            "file_cursors", "collector_health", "settings",
        ] {
            let n: i64 = c
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }

    #[test]
    fn reopening_is_a_no_op() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        migrate(&mut c).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn upgrade_preserves_existing_rows() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO agents (id, display_name) VALUES ('codex', 'Codex')", []).unwrap();
        c.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();

        // Simulate a future release shipping 0002 on top of 0001.
        let upgraded = [
            Migration { version: 1, name: "init", sql: MIGRATIONS[0].sql },
            Migration { version: 2, name: "add_note", sql: "ALTER TABLE agents ADD COLUMN note TEXT;" },
        ];
        migrate_with(&mut c, &upgraded).unwrap();

        assert_eq!(current_version(&c).unwrap(), 2);
        let name: String = c.query_row("SELECT display_name FROM agents WHERE id='codex'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Codex");
        let v: String = c.query_row("SELECT value FROM settings WHERE key='k'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "1");
    }

    #[test]
    fn newer_database_is_refused_not_downgraded() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO schema_migrations VALUES (99, 'future', 0)", []).unwrap();
        let err = migrate(&mut c).unwrap_err();
        assert!(err.to_string().contains("newer"), "{err}");
    }

    #[test]
    fn a_failing_migration_rolls_back_completely() {
        let mut c = mem();
        let bad = [Migration { version: 1, name: "bad", sql: "CREATE TABLE ok_table (a); CREATE TABLE ok_table (b);" }];
        assert!(migrate_with(&mut c, &bad).is_err());
        let n: i64 = c
            .query_row("SELECT count(*) FROM sqlite_master WHERE name='ok_table'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "partial migration must not persist");
        assert_eq!(current_version(&c).unwrap(), 0);
    }

    #[test]
    fn the_reserved_unknown_project_exists() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        let name: String = c.query_row("SELECT name FROM projects WHERE id=0", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "UNKNOWN PROJECT");
    }

    #[test]
    fn unavailable_accuracy_is_rejected_by_the_schema() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO agents (id, display_name) VALUES ('a','A')", []).unwrap();
        c.execute("INSERT INTO models (id, agent_id, name) VALUES (1,'a','m')", []).unwrap();
        let r = c.execute(
            "INSERT INTO usage_events (dedupe_key, agent_id, model_id, ts_utc_ms, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, accuracy, source) VALUES ('k','a',1,0,0,0,0,0,'unavailable','s')",
            [],
        );
        assert!(r.is_err(), "'unavailable' must never be storable");
    }
}
