//! Forward-only migrations. Applied versions are recorded; a database written by a *newer* build is
//! refused rather than silently downgraded, so user history is never damaged.

use rusqlite::Connection;

use crate::error::{AppError, Result};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("../../migrations/0001_init.sql") },
    Migration { version: 2, name: "accounts", sql: include_str!("../../migrations/0002_accounts.sql") },
];

pub const LATEST_VERSION: i64 = 2;

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
            "file_cursors", "collector_health", "settings", "accounts", "active_accounts",
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
        assert_eq!(n, LATEST_VERSION, "each migration is recorded exactly once");
    }

    #[test]
    fn upgrade_preserves_existing_rows() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO agents (id, display_name) VALUES ('codex', 'Codex')", []).unwrap();
        c.execute("INSERT INTO settings (key, value) VALUES ('k', '1')", []).unwrap();

        // Simulate a future release shipping a migration on top of the latest one.
        let next = LATEST_VERSION + 1;
        let upgraded = [
            Migration { version: 1, name: "init", sql: MIGRATIONS[0].sql },
            Migration { version: 2, name: "accounts", sql: MIGRATIONS[1].sql },
            Migration { version: next, name: "add_note", sql: "ALTER TABLE agents ADD COLUMN note TEXT;" },
        ];
        migrate_with(&mut c, &upgraded).unwrap();

        assert_eq!(current_version(&c).unwrap(), next);
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

    /// A database written by the previous release (schema v1, with real usage in it) upgrades without losing a token,
    /// and everything already recorded becomes the agent's default account.
    #[test]
    fn upgrading_from_v1_keeps_every_row_and_files_it_under_the_default_account() {
        let mut c = mem();
        migrate_with(&mut c, &MIGRATIONS[..1]).unwrap();
        assert_eq!(current_version(&c).unwrap(), 1);
        c.execute("INSERT INTO agents (id, display_name) VALUES ('codex','Codex')", []).unwrap();
        c.execute("INSERT INTO models (id, agent_id, name) VALUES (1,'codex','m')", []).unwrap();
        c.execute(
            "INSERT INTO usage_events (dedupe_key, agent_id, model_id, ts_utc_ms, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, accuracy, source) VALUES ('k','codex',1,1000,10,5,3,1,'actual','s')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO usage_buckets (bucket_utc_s, agent_id, model_id, project_id, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_tokens, events)
             VALUES (900,'codex',1,0,10,5,3,1,2,0,1), (1800,'codex',1,0,20,10,6,2,4,0,2)",
            [],
        )
        .unwrap();

        migrate(&mut c).unwrap();

        assert_eq!(current_version(&c).unwrap(), LATEST_VERSION);
        let (n, input, output, cache_read, events): (i64, i64, i64, i64, i64) = c
            .query_row(
                "SELECT count(*), SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(events) FROM usage_buckets WHERE account_id = 'default'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!((n, input, output, cache_read, events), (2, 30, 15, 9, 3), "no bucket lost or altered");
        let account: String = c.query_row("SELECT account_id FROM usage_events WHERE dedupe_key = 'k'", [], |r| r.get(0)).unwrap();
        assert_eq!(account, "default");
    }

    #[test]
    fn the_rebuilt_bucket_table_keeps_accounts_apart() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO agents (id, display_name) VALUES ('codex','Codex')", []).unwrap();
        c.execute("INSERT INTO models (id, agent_id, name) VALUES (1,'codex','m')", []).unwrap();
        for account in ["default", "work"] {
            c.execute(
                "INSERT INTO usage_buckets (bucket_utc_s, agent_id, account_id, model_id, project_id, input_tokens) VALUES (900,'codex',?1,1,0,7)",
                [account],
            )
            .unwrap();
        }
        let n: i64 = c.query_row("SELECT count(*) FROM usage_buckets WHERE bucket_utc_s = 900", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "the same bucket for two accounts is two rows");
        let dup = c.execute("INSERT INTO usage_buckets (bucket_utc_s, agent_id, account_id, model_id, project_id) VALUES (900,'codex','work',1,0)", []);
        assert!(dup.is_err(), "but one account has one row per bucket/model/project");
    }

    #[test]
    fn account_labels_are_unique_per_agent_ignoring_case_but_not_across_agents_or_after_removal() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        for a in ["codex", "claude"] {
            c.execute("INSERT INTO agents (id, display_name) VALUES (?1, ?1)", [a]).unwrap();
        }
        let add = |c: &Connection, agent: &str, id: &str, label: &str| {
            c.execute(
                "INSERT INTO accounts (agent_id, account_id, label, kind, created_utc_ms) VALUES (?1, ?2, ?3, 'managed', 0)",
                [agent, id, label],
            )
        };
        add(&c, "codex", "work", "Work").unwrap();
        assert!(add(&c, "codex", "work-2", "WORK").is_err(), "same label, different case");
        add(&c, "claude", "work", "Work").unwrap();
        c.execute("UPDATE accounts SET removed_utc_ms = 1 WHERE agent_id = 'codex' AND account_id = 'work'", []).unwrap();
        add(&c, "codex", "work-2", "Work").unwrap();
    }

    #[test]
    fn the_active_account_must_exist_and_each_agent_has_at_most_one() {
        let mut c = mem();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO agents (id, display_name) VALUES ('codex','Codex')", []).unwrap();
        let ghost = c.execute("INSERT INTO active_accounts (agent_id, account_id, switched_utc_ms) VALUES ('codex','nobody',0)", []);
        assert!(ghost.is_err(), "pointing at an account that does not exist is refused");
        c.execute("INSERT INTO accounts (agent_id, account_id, label, kind, created_utc_ms) VALUES ('codex','default','Default','default',0)", []).unwrap();
        c.execute("INSERT INTO active_accounts (agent_id, account_id, switched_utc_ms) VALUES ('codex','default',0)", []).unwrap();
        let again = c.execute("INSERT INTO active_accounts (agent_id, account_id, switched_utc_ms) VALUES ('codex','default',1)", []);
        assert!(again.is_err(), "one active account per agent");
    }
}
