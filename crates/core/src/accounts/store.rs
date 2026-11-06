//! Persistence of accounts and of "which account each agent is using", in the same SQLite database the app and the CLI
//! share. That shared file *is* the single source of truth: the desktop UI and `agm` read and write the same rows, so
//! they can never disagree, and a restart loses nothing.
//!
//! Every change runs in an immediate transaction (SQLite takes the write lock up front), so two processes changing
//! accounts at the same moment are serialised by the database itself.

use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row, Transaction, TransactionBehavior};

use super::error::{AccountError, AccountResult};
use super::model::{slugify, Account, AccountKind, AuthState};
use crate::database::Database;
use crate::model::DEFAULT_ACCOUNT;

/// Longest label we accept.
pub const MAX_LABEL: usize = 40;

const COLUMNS: &str = "agent_id, account_id, label, kind, profile_dir, identity, auth_state, auth_detail,
                       auth_checked_utc_ms, created_utc_ms, last_used_utc_ms, removed_utc_ms";

fn row_to_account(r: &Row<'_>) -> rusqlite::Result<Account> {
    let kind: String = r.get(3)?;
    let auth: String = r.get(6)?;
    let profile: Option<String> = r.get(4)?;
    Ok(Account {
        agent_id: r.get(0)?,
        account_id: r.get(1)?,
        label: r.get(2)?,
        kind: AccountKind::parse(&kind).unwrap_or(AccountKind::Managed),
        profile_dir: profile.map(PathBuf::from),
        identity: r.get(5)?,
        auth: AuthState::parse(&auth).unwrap_or(AuthState::Unknown),
        auth_detail: r.get(7)?,
        auth_checked_utc_ms: r.get(8)?,
        created_utc_ms: r.get(9)?,
        last_used_utc_ms: r.get(10)?,
        removed_utc_ms: r.get(11)?,
    })
}

/// A label the user typed, made safe to store and show: trimmed, one line, no control characters, not too long.
pub fn clean_label(label: &str) -> AccountResult<String> {
    let label = label.trim();
    if label.is_empty() {
        return Err(AccountError::InvalidLabel("give the account a name".into()));
    }
    if label.chars().count() > MAX_LABEL {
        return Err(AccountError::InvalidLabel(format!("the name can be at most {MAX_LABEL} characters")));
    }
    if label.chars().any(char::is_control) {
        return Err(AccountError::InvalidLabel("the name cannot contain control characters".into()));
    }
    Ok(label.to_string())
}

#[derive(Clone)]
pub struct AccountStore {
    db: Arc<Database>,
}

impl AccountStore {
    pub fn new(db: Arc<Database>) -> Self {
        AccountStore { db }
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    fn write<T>(&self, f: impl FnOnce(&Transaction) -> AccountResult<T>) -> AccountResult<T> {
        let out = self
            .db
            .with_writer(|w| {
                let tx = w.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let out = f(&tx);
                if out.is_ok() {
                    tx.commit()?;
                } // on an error the transaction is dropped and rolled back
                Ok(out)
            })
            .map_err(AccountError::from)?;
        out
    }

    fn read<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> AccountResult<T>) -> AccountResult<T> {
        self.db.with_reader(|c| Ok(f(c))).map_err(AccountError::from)?
    }

    /// Makes sure every listed agent has its `default` account and an active account. Safe to call any time, from any
    /// process: it never changes an existing row.
    pub fn ensure_defaults(&self, agent_ids: &[&str], now_ms: i64) -> AccountResult<()> {
        self.write(|tx| {
            for agent in agent_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO accounts (agent_id, account_id, label, kind, auth_state, created_utc_ms)
                     VALUES (?1, ?2, 'Default', 'default', 'unknown', ?3)",
                    params![agent, DEFAULT_ACCOUNT, now_ms],
                )?;
                tx.execute(
                    "INSERT OR IGNORE INTO active_accounts (agent_id, account_id, switched_utc_ms) VALUES (?1, ?2, ?3)",
                    params![agent, DEFAULT_ACCOUNT, now_ms],
                )?;
            }
            Ok(())
        })
    }

    /// Accounts, default first then in the order they were added. Removed ones only when asked for.
    pub fn list(&self, agent: Option<&str>, include_removed: bool) -> AccountResult<Vec<Account>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM accounts
                 WHERE (?1 IS NULL OR agent_id = ?1) AND (?2 = 1 OR removed_utc_ms IS NULL)
                 ORDER BY agent_id, (kind = 'default') DESC, created_utc_ms, account_id"
            );
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params![agent, include_removed], row_to_account)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn get(&self, agent: &str, account: &str) -> AccountResult<Option<Account>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM accounts WHERE agent_id = ?1 AND account_id = ?2");
            Ok(c.prepare_cached(&sql)?.query_row(params![agent, account], row_to_account).optional()?)
        })
    }

    /// Like [`get`](Self::get) but an error when the account does not exist or has been removed.
    pub fn require(&self, agent: &str, account: &str) -> AccountResult<Account> {
        match self.get(agent, account)? {
            Some(a) if !a.is_removed() => Ok(a),
            _ => Err(AccountError::UnknownAccount { agent: agent.to_string(), account: account.to_string() }),
        }
    }

    /// Finds a live account by id or by label (case-insensitive), the way a user types it.
    pub fn find(&self, agent: &str, id_or_label: &str) -> AccountResult<Option<Account>> {
        let wanted = id_or_label.trim();
        let all = self.list(Some(agent), false)?;
        Ok(all
            .iter()
            .find(|a| a.account_id == wanted)
            .or_else(|| all.iter().find(|a| a.label.eq_ignore_ascii_case(wanted)))
            .cloned())
    }

    /// The account the agent is using now.
    pub fn active(&self, agent: &str) -> AccountResult<Option<Account>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {} FROM accounts a JOIN active_accounts s ON s.agent_id = a.agent_id AND s.account_id = a.account_id
                 WHERE a.agent_id = ?1 AND a.removed_utc_ms IS NULL",
                COLUMNS.replace('\n', " ").split(',').map(|c| format!("a.{}", c.trim())).collect::<Vec<_>>().join(", ")
            );
            Ok(c.prepare_cached(&sql)?.query_row([agent], row_to_account).optional()?)
        })
    }

    /// (agent, account) for every agent that has an active account.
    pub fn all_active(&self) -> AccountResult<Vec<(String, String)>> {
        self.read(|c| {
            let mut st = c.prepare_cached("SELECT agent_id, account_id FROM active_accounts ORDER BY agent_id")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Adds a managed account. `profile_dir_for` receives the id that was chosen and returns the folder for it.
    pub fn insert_managed(
        &self,
        agent: &str,
        label: &str,
        profile_dir_for: impl FnOnce(&str) -> PathBuf,
        now_ms: i64,
    ) -> AccountResult<Account> {
        let label = clean_label(label)?;
        self.write(|tx| {
            let taken: i64 = tx.query_row(
                "SELECT count(*) FROM accounts WHERE agent_id = ?1 AND lower(label) = lower(?2) AND removed_utc_ms IS NULL",
                params![agent, label],
                |r| r.get(0),
            )?;
            if taken > 0 {
                return Err(AccountError::DuplicateLabel(label.clone()));
            }
            // an id is never reused, not even one that belonged to a removed account (its history is still filed under it)
            let base = slugify(&label);
            let mut id = base.clone();
            let mut n = 1;
            while tx.query_row("SELECT count(*) FROM accounts WHERE agent_id = ?1 AND account_id = ?2", params![agent, id], |r| r.get::<_, i64>(0))? > 0 {
                n += 1;
                id = format!("{base}-{n}");
            }
            let dir = profile_dir_for(&id);
            tx.execute(
                "INSERT INTO accounts (agent_id, account_id, label, kind, profile_dir, auth_state, created_utc_ms)
                 VALUES (?1, ?2, ?3, 'managed', ?4, 'pending', ?5)",
                params![agent, id, label, dir.to_string_lossy(), now_ms],
            )?;
            let sql = format!("SELECT {COLUMNS} FROM accounts WHERE agent_id = ?1 AND account_id = ?2");
            Ok(tx.query_row(&sql, params![agent, id], row_to_account)?)
        })
    }

    pub fn rename(&self, agent: &str, account: &str, label: &str) -> AccountResult<()> {
        let label = clean_label(label)?;
        self.write(|tx| {
            let taken: i64 = tx.query_row(
                "SELECT count(*) FROM accounts WHERE agent_id = ?1 AND lower(label) = lower(?2) AND removed_utc_ms IS NULL AND account_id <> ?3",
                params![agent, label, account],
                |r| r.get(0),
            )?;
            if taken > 0 {
                return Err(AccountError::DuplicateLabel(label.clone()));
            }
            let n = tx.execute(
                "UPDATE accounts SET label = ?3 WHERE agent_id = ?1 AND account_id = ?2 AND removed_utc_ms IS NULL",
                params![agent, account, label],
            )?;
            if n == 0 {
                return Err(AccountError::UnknownAccount { agent: agent.into(), account: account.into() });
            }
            Ok(())
        })
    }

    /// Records the result of an authentication check.
    pub fn set_auth(&self, agent: &str, account: &str, state: AuthState, identity: Option<&str>, detail: Option<&str>, now_ms: i64) -> AccountResult<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE accounts SET auth_state = ?3, identity = COALESCE(?4, identity), auth_detail = ?5, auth_checked_utc_ms = ?6
                 WHERE agent_id = ?1 AND account_id = ?2 AND removed_utc_ms IS NULL",
                params![agent, account, state.as_str(), identity, detail, now_ms],
            )?;
            Ok(())
        })
    }

    /// Makes `account` the one `agent` uses. Only that agent's row changes.
    pub fn set_active(&self, agent: &str, account: &str, now_ms: i64) -> AccountResult<()> {
        self.write(|tx| {
            let exists: i64 = tx.query_row(
                "SELECT count(*) FROM accounts WHERE agent_id = ?1 AND account_id = ?2 AND removed_utc_ms IS NULL",
                params![agent, account],
                |r| r.get(0),
            )?;
            if exists == 0 {
                return Err(AccountError::UnknownAccount { agent: agent.into(), account: account.into() });
            }
            tx.execute(
                "INSERT INTO active_accounts (agent_id, account_id, switched_utc_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT (agent_id) DO UPDATE SET account_id = excluded.account_id, switched_utc_ms = excluded.switched_utc_ms",
                params![agent, account, now_ms],
            )?;
            Ok(())
        })
    }

    pub fn touch_used(&self, agent: &str, account: &str, now_ms: i64) -> AccountResult<()> {
        self.write(|tx| {
            tx.execute("UPDATE accounts SET last_used_utc_ms = ?3 WHERE agent_id = ?1 AND account_id = ?2", params![agent, account, now_ms])?;
            Ok(())
        })
    }

    /// Removes an account from view: its sign-in is forgotten (the profile folder is cleared by the caller) but its usage
    /// history stays, filed under the same id. If it was the active account the agent goes back to its default one.
    /// Returns the profile folder the account had.
    pub fn soft_remove(&self, agent: &str, account: &str, now_ms: i64) -> AccountResult<Option<PathBuf>> {
        if account == DEFAULT_ACCOUNT {
            return Err(AccountError::CannotRemoveDefault);
        }
        self.write(|tx| {
            let dir: Option<Option<String>> = tx
                .query_row(
                    "SELECT profile_dir FROM accounts WHERE agent_id = ?1 AND account_id = ?2 AND removed_utc_ms IS NULL",
                    params![agent, account],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(dir) = dir else {
                return Err(AccountError::UnknownAccount { agent: agent.into(), account: account.into() });
            };
            tx.execute(
                "UPDATE accounts SET removed_utc_ms = ?3, profile_dir = NULL, identity = NULL, auth_state = 'not_logged_in',
                        auth_detail = NULL WHERE agent_id = ?1 AND account_id = ?2",
                params![agent, account, now_ms],
            )?;
            tx.execute(
                "UPDATE active_accounts SET account_id = ?3, switched_utc_ms = ?4 WHERE agent_id = ?1 AND account_id = ?2",
                params![agent, account, DEFAULT_ACCOUNT, now_ms],
            )?;
            Ok(dir.map(PathBuf::from))
        })
    }

    /// Deletes an account whose sign-in never completed (still `pending`). Anything else is left alone. If the account had
    /// already been chosen as the active one, the agent goes back to its default account first.
    pub fn discard_pending(&self, agent: &str, account: &str) -> AccountResult<bool> {
        if account == DEFAULT_ACCOUNT {
            return Ok(false);
        }
        let now = chrono::Utc::now().timestamp_millis();
        self.write(|tx| {
            let pending: i64 = tx.query_row(
                "SELECT count(*) FROM accounts WHERE agent_id = ?1 AND account_id = ?2 AND auth_state = 'pending' AND kind = 'managed'",
                params![agent, account],
                |r| r.get(0),
            )?;
            if pending == 0 {
                return Ok(false);
            }
            tx.execute(
                "UPDATE active_accounts SET account_id = ?3, switched_utc_ms = ?4 WHERE agent_id = ?1 AND account_id = ?2",
                params![agent, account, DEFAULT_ACCOUNT, now],
            )?;
            tx.execute("DELETE FROM accounts WHERE agent_id = ?1 AND account_id = ?2", params![agent, account])?;
            Ok(true)
        })
    }

    /// Deletes the usage history filed under an account (events and rollup) and the read positions inside its profile.
    pub fn purge_usage(&self, agent: &str, account: &str, profile_dir: Option<&str>) -> AccountResult<()> {
        if account == DEFAULT_ACCOUNT {
            return Err(AccountError::CannotRemoveDefault);
        }
        self.write(|tx| {
            tx.execute("DELETE FROM usage_events WHERE agent_id = ?1 AND account_id = ?2", params![agent, account])?;
            tx.execute("DELETE FROM usage_buckets WHERE agent_id = ?1 AND account_id = ?2", params![agent, account])?;
            if let Some(dir) = profile_dir {
                tx.execute(
                    "DELETE FROM file_cursors WHERE agent_id = ?1 AND substr(path, 1, ?3) = ?2",
                    params![agent, dir, dir.chars().count() as i64],
                )?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::testutil::temp_db;
    use crate::model::catalog;

    fn store() -> (tempfile::TempDir, AccountStore) {
        let (d, db) = temp_db();
        let s = AccountStore::new(db);
        let ids: Vec<&str> = catalog().iter().map(|a| a.id).collect();
        s.ensure_defaults(&ids, 1).unwrap();
        (d, s)
    }

    fn add(s: &AccountStore, agent: &str, label: &str) -> Account {
        s.insert_managed(agent, label, |id| PathBuf::from(format!("/profiles/{agent}/{id}")), 10).unwrap()
    }

    #[test]
    fn every_agent_starts_with_its_default_account_active() {
        let (_d, s) = store();
        for a in catalog() {
            let active = s.active(a.id).unwrap().expect("an active account");
            assert_eq!(active.account_id, DEFAULT_ACCOUNT);
            assert!(active.is_default() && active.profile_dir.is_none());
        }
        assert_eq!(s.all_active().unwrap().len(), catalog().len());
    }

    #[test]
    fn ensuring_defaults_twice_changes_nothing() {
        let (_d, s) = store();
        add(&s, "codex", "Work");
        s.set_active("codex", "work", 5).unwrap();
        let ids: Vec<&str> = catalog().iter().map(|a| a.id).collect();
        s.ensure_defaults(&ids, 999).unwrap();
        assert_eq!(s.active("codex").unwrap().unwrap().account_id, "work", "an existing choice is never reset");
        assert_eq!(s.list(Some("codex"), false).unwrap().len(), 2);
    }

    #[test]
    fn adding_accounts_keeps_each_agents_list_separate_and_default_first() {
        let (_d, s) = store();
        add(&s, "codex", "Personal");
        add(&s, "codex", "Work");
        add(&s, "claude", "Work");
        let codex = s.list(Some("codex"), false).unwrap();
        let ids: Vec<&str> = codex.iter().map(|a| a.account_id.as_str()).collect();
        assert_eq!(ids, ["default", "personal", "work"]);
        assert_eq!(s.list(Some("claude"), false).unwrap().len(), 2);
        assert_eq!(codex[1].auth, AuthState::Pending, "a new account is pending until its sign-in is confirmed");
        assert_eq!(codex[1].profile_dir.as_deref(), Some(std::path::Path::new("/profiles/codex/personal")));
    }

    #[test]
    fn labels_are_validated_and_unique_per_agent_ignoring_case() {
        let (_d, s) = store();
        add(&s, "codex", "Work");
        assert!(matches!(s.insert_managed("codex", "  work ", |_| PathBuf::new(), 1), Err(AccountError::DuplicateLabel(_))));
        assert!(s.insert_managed("claude", "work", |_| PathBuf::new(), 1).is_ok(), "another agent may use the same name");
        assert!(matches!(s.insert_managed("codex", "   ", |_| PathBuf::new(), 1), Err(AccountError::InvalidLabel(_))));
        assert!(matches!(s.insert_managed("codex", "bad\nname", |_| PathBuf::new(), 1), Err(AccountError::InvalidLabel(_))));
        assert!(matches!(s.insert_managed("codex", &"x".repeat(41), |_| PathBuf::new(), 1), Err(AccountError::InvalidLabel(_))));
    }

    #[test]
    fn ids_come_from_the_label_and_are_never_reused() {
        let (_d, s) = store();
        let a = add(&s, "codex", "Work");
        assert_eq!(a.account_id, "work");
        s.soft_remove("codex", "work", 20).unwrap();
        let b = add(&s, "codex", "Work");
        assert_eq!(b.account_id, "work-2", "history filed under `work` must not attach itself to the new account");
        assert!(matches!(s.insert_managed("codex", "default", |_| PathBuf::new(), 1), Err(AccountError::DuplicateLabel(_))), "the agent's own account already carries that name");
    }

    #[test]
    fn switching_one_agent_never_touches_another() {
        let (_d, s) = store();
        add(&s, "codex", "Work");
        add(&s, "claude", "Personal");
        let before: Vec<_> = s.all_active().unwrap().into_iter().filter(|(a, _)| a != "codex").collect();
        s.set_active("codex", "work", 5).unwrap();
        let after: Vec<_> = s.all_active().unwrap().into_iter().filter(|(a, _)| a != "codex").collect();
        assert_eq!(before, after, "every other agent keeps its active account");
        assert_eq!(s.active("codex").unwrap().unwrap().account_id, "work");
        assert_eq!(s.active("claude").unwrap().unwrap().account_id, DEFAULT_ACCOUNT);
    }

    #[test]
    fn an_account_of_another_agent_cannot_be_made_active() {
        let (_d, s) = store();
        add(&s, "claude", "Personal");
        assert!(matches!(s.set_active("codex", "personal", 5), Err(AccountError::UnknownAccount { .. })));
        assert_eq!(s.active("codex").unwrap().unwrap().account_id, DEFAULT_ACCOUNT);
        assert!(matches!(s.set_active("codex", "ghost", 5), Err(AccountError::UnknownAccount { .. })));
    }

    #[test]
    fn removing_the_active_account_falls_back_to_default_and_keeps_the_row_for_history() {
        let (_d, s) = store();
        add(&s, "codex", "Work");
        s.set_active("codex", "work", 5).unwrap();
        let dir = s.soft_remove("codex", "work", 30).unwrap();
        assert_eq!(dir, Some(PathBuf::from("/profiles/codex/work")));
        assert_eq!(s.active("codex").unwrap().unwrap().account_id, DEFAULT_ACCOUNT);
        assert!(s.list(Some("codex"), false).unwrap().iter().all(|a| a.account_id != "work"));
        let kept = s.get("codex", "work").unwrap().unwrap();
        assert!(kept.is_removed() && kept.profile_dir.is_none(), "the sign-in location is forgotten");
        assert!(matches!(s.require("codex", "work"), Err(AccountError::UnknownAccount { .. })));
        assert!(s.list(Some("codex"), true).unwrap().iter().any(|a| a.account_id == "work"));
    }

    #[test]
    fn the_default_account_cannot_be_removed() {
        let (_d, s) = store();
        assert!(matches!(s.soft_remove("codex", DEFAULT_ACCOUNT, 1), Err(AccountError::CannotRemoveDefault)));
        assert!(matches!(s.purge_usage("codex", DEFAULT_ACCOUNT, None), Err(AccountError::CannotRemoveDefault)));
    }

    #[test]
    fn auth_checks_are_recorded_and_the_identity_is_kept_when_a_later_check_has_none() {
        let (_d, s) = store();
        add(&s, "claude", "Work");
        s.set_auth("claude", "work", AuthState::Valid, Some("me@example.com"), None, 100).unwrap();
        s.set_auth("claude", "work", AuthState::Expired, None, Some("the session ended"), 200).unwrap();
        let a = s.get("claude", "work").unwrap().unwrap();
        assert_eq!((a.auth, a.identity.as_deref(), a.auth_detail.as_deref(), a.auth_checked_utc_ms), (AuthState::Expired, Some("me@example.com"), Some("the session ended"), Some(200)));
    }

    #[test]
    fn accounts_are_found_by_id_or_by_label_in_any_case() {
        let (_d, s) = store();
        add(&s, "codex", "My Work");
        assert_eq!(s.find("codex", "my-work").unwrap().unwrap().label, "My Work");
        assert_eq!(s.find("codex", "MY WORK").unwrap().unwrap().account_id, "my-work");
        assert!(s.find("codex", "nope").unwrap().is_none());
        assert!(s.find("claude", "my-work").unwrap().is_none(), "another agent's account is not visible");
    }

    #[test]
    fn purging_deletes_only_that_accounts_usage_and_cursors() {
        let (_d, s) = store();
        add(&s, "codex", "Work");
        let db = s.database().clone();
        let ev = |k: &str| crate::model::UsageEvent {
            agent: "codex",
            model: "m".into(),
            ts_utc_ms: 1_000_000,
            input_tokens: 10,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: None,
            session_id: None,
            project: None,
            source: "t",
            accuracy: crate::model::Accuracy::Real,
            dedupe_key: k.into(),
        };
        let cur = |p: &str| crate::database::CursorUpdate { path: p.into(), size: 1, mtime_ms: 1, offset: 1, state: None };
        db.commit_for("work", &crate::database::Batch { agent: "codex", events: vec![ev("a")], cursors: vec![cur("/profiles/codex/work/home/.codex/sessions/x.jsonl")] }).unwrap();
        db.commit_for("default", &crate::database::Batch { agent: "codex", events: vec![ev("a")], cursors: vec![cur("/home/me/.codex/sessions/x.jsonl")] }).unwrap();
        s.purge_usage("codex", "work", Some("/profiles/codex/work")).unwrap();
        let counts: (i64, i64, i64) = db
            .with_reader(|c| {
                Ok((
                    c.query_row("SELECT count(*) FROM usage_events WHERE account_id = 'work'", [], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM usage_events WHERE account_id = 'default'", [], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM file_cursors", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(counts, (0, 1, 1), "the default account's event and cursor survive");
    }

    #[test]
    fn state_survives_closing_and_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        {
            let s = AccountStore::new(Database::open(&path).unwrap());
            s.ensure_defaults(&["codex", "claude"], 1).unwrap();
            add(&s, "codex", "Work");
            s.set_active("codex", "work", 5).unwrap();
            s.set_auth("codex", "work", AuthState::Valid, Some("a@b.c"), None, 6).unwrap();
        }
        let s = AccountStore::new(Database::open(&path).unwrap());
        let a = s.active("codex").unwrap().unwrap();
        assert_eq!((a.account_id.as_str(), a.auth, a.identity.as_deref()), ("work", AuthState::Valid, Some("a@b.c")));
        assert_eq!(s.active("claude").unwrap().unwrap().account_id, DEFAULT_ACCOUNT);
    }

    #[test]
    fn two_processes_share_one_truth() {
        // Two separate Database handles on the same file stand in for the desktop app and the CLI.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let app = AccountStore::new(Database::open(&path).unwrap());
        let cli = AccountStore::new(Database::open(&path).unwrap());
        app.ensure_defaults(&["codex"], 1).unwrap();
        add(&cli, "codex", "Work");
        cli.set_active("codex", "work", 5).unwrap();
        assert_eq!(app.active("codex").unwrap().unwrap().account_id, "work", "the app sees what the CLI did");
        app.set_active("codex", DEFAULT_ACCOUNT, 6).unwrap();
        assert_eq!(cli.active("codex").unwrap().unwrap().account_id, DEFAULT_ACCOUNT, "and the other way round");
    }
}
