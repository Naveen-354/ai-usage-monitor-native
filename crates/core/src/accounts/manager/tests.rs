use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::accounts::runner::fake::{FakeRunner, Reply};
use crate::accounts::secrets::MemorySecretStore;
use crate::database::{Batch, Database};
use crate::model::{Accuracy, UsageEvent};

/// A machine in a temp folder: fake agent executables on PATH, a scripted runner, an in-memory credential store.
struct Fx {
    _dir: tempfile::TempDir,
    data: PathBuf,
    env: Env,
    runner: Arc<FakeRunner>,
    secrets: Arc<MemorySecretStore>,
    mgr: AccountManager,
}

fn exe_name(n: &str) -> String {
    if cfg!(windows) {
        format!("{n}.cmd")
    } else {
        n.to_string()
    }
}

fn fx_with(installed: &[&str]) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let (data, bin, home) = (dir.path().join("data"), dir.path().join("bin"), dir.path().join("home"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    for n in installed {
        std::fs::write(bin.join(exe_name(n)), b"").unwrap();
    }
    let mut env = Env::with_home(&home);
    env.path_dirs = vec![bin];
    let runner = FakeRunner::new();
    let secrets = Arc::new(MemorySecretStore::new());
    let db = Database::open(&data.join("usage.db")).unwrap();
    let mgr = AccountManager::new(AccountStore::new(db), runner.clone(), secrets.clone(), env.clone(), data.clone()).with_lock_wait(Duration::from_secs(3));
    mgr.init().unwrap();
    Fx { _dir: dir, data, env, runner, secrets, mgr }
}

fn fx() -> Fx {
    fx_with(&["codex", "claude", "gemini", "opencode", "agy"])
}

/// A second manager over the same data folder - what a second process (the CLI next to the app) would be.
fn second_manager(f: &Fx) -> AccountManager {
    let db = Database::open(&f.data.join("usage.db")).unwrap();
    AccountManager::new(AccountStore::new(db), f.runner.clone(), f.secrets.clone(), f.env.clone(), f.data.clone()).with_lock_wait(Duration::from_secs(3))
}

fn script_codex_login(f: &Fx) {
    f.runner.script(Reply::attached("login", 0).writing("CODEX_HOME", "auth.json"));
    f.runner.script(Reply::status("login status", 0, "Logged in using ChatGPT\n", ""));
}

fn script_claude_login(f: &Fx) {
    f.runner.script(Reply::attached("auth login", 0).writing("CLAUDE_CONFIG_DIR", ".credentials.json"));
    f.runner.script(Reply::status("auth status", 0, r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com"}"#, ""));
}

fn add_codex(f: &Fx, label: &str) -> Account {
    script_codex_login(f);
    f.mgr.login("codex", label, LoginMethod::Standard, None).unwrap()
}

fn add_claude(f: &Fx, label: &str) -> Account {
    script_claude_login(f);
    f.mgr.login("claude", label, LoginMethod::Standard, None).unwrap()
}

fn ids(f: &Fx, agent: &str) -> Vec<String> {
    f.mgr.store().list(Some(agent), false).unwrap().into_iter().map(|a| a.account_id).collect()
}

fn active(f: &Fx, agent: &str) -> String {
    f.mgr.store().active(agent).unwrap().unwrap().account_id
}

fn files_under(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push((p.clone(), e.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    out.sort();
    out
}

fn event(agent: &'static str, key: &str, ts: i64, input: u64) -> UsageEvent {
    UsageEvent {
        agent,
        model: "m".into(),
        ts_utc_ms: ts,
        input_tokens: input,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: None,
        session_id: None,
        project: None,
        source: "t",
        accuracy: Accuracy::Real,
        dedupe_key: key.into(),
    }
}

fn env_of(spec: &ProcessSpec, name: &str) -> Option<OsString> {
    spec.env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

// ------------------------------------------------------------------------------------------------ listing and defaults

#[test]
fn every_agent_has_a_default_account_that_is_active_and_the_list_follows_the_catalogue() {
    let f = fx_with(&["codex"]);
    let agents = f.mgr.agents().unwrap();
    assert_eq!(agents.iter().map(|a| a.agent_id.as_str()).collect::<Vec<_>>(), catalog().iter().map(|a| a.id).collect::<Vec<_>>());
    for a in &agents {
        assert_eq!(a.active.as_deref(), Some(DEFAULT_ACCOUNT), "{}", a.agent_id);
        assert_eq!(a.accounts.len(), 1);
        assert_eq!(a.active_account().unwrap().kind, AccountKind::Default);
    }
    let codex = f.mgr.agent("codex").unwrap();
    assert!(codex.installed && codex.binary.is_some());
    assert!(!f.mgr.agent("claude").unwrap().installed, "not on PATH here");
    assert_eq!(f.mgr.agent("codex").unwrap().switch, SwitchSupport::Supported { mechanism: "CODEX_HOME" });
}

#[test]
fn unknown_agents_are_reported_as_such_everywhere() {
    let f = fx();
    assert!(matches!(f.mgr.agent("nope"), Err(AccountError::UnknownAgent(_))));
    assert!(matches!(f.mgr.plan_login("nope", "x", LoginMethod::Standard, None), Err(AccountError::UnknownAgent(_))));
    assert!(matches!(f.mgr.use_account("nope", "x"), Err(AccountError::UnknownAgent(_))));
    assert!(matches!(f.mgr.remove("nope", "x", false), Err(AccountError::UnknownAgent(_))));
    assert!(matches!(f.mgr.check_auth("nope", "x"), Err(AccountError::UnknownAgent(_))));
    assert!(matches!(f.mgr.launch_spec("nope", &[]), Err(AccountError::UnknownAgent(_))));
}

// ------------------------------------------------------------------------------------------------ signing in

#[test]
fn a_codex_sign_in_gets_its_own_profile_and_is_confirmed_with_codexs_own_status_command() {
    let f = fx();
    let a = add_codex(&f, "Work");
    assert_eq!((a.account_id.as_str(), a.kind, a.auth), ("work", AccountKind::Managed, AuthState::Valid));
    let profile = f.data.join("accounts").join("codex").join("work");
    assert_eq!(a.profile_dir.as_deref(), Some(profile.as_path()));
    assert!(profile.join("home").join(".codex").join("auth.json").is_file(), "the agent saved its sign-in inside the account's own folder");

    let calls = f.runner.calls();
    assert_eq!(calls.len(), 2, "sign-in, then the status check");
    let codex_home = profile.join("home").join(".codex");
    for c in &calls {
        assert_eq!(env_of(c, "CODEX_HOME"), Some(codex_home.clone().into_os_string()), "every call is pointed at the account's folder");
    }
    assert_eq!(calls[0].args, vec![OsString::from("login")]);
    assert_eq!(calls[1].args, vec![OsString::from("login"), OsString::from("status")]);
    assert_eq!(active(&f, "codex"), DEFAULT_ACCOUNT, "adding an account does not switch to it");
}

#[test]
fn the_identity_the_agent_reports_is_kept_for_display() {
    let f = fx();
    let a = add_claude(&f, "Work");
    assert_eq!(a.identity.as_deref(), Some("me@example.com"));
    assert_eq!(a.auth, AuthState::Valid);
}

#[test]
fn a_sign_in_that_fails_leaves_nothing_behind() {
    let f = fx();
    f.runner.script(Reply::attached("login", 1));
    let err = f.mgr.login("codex", "Work", LoginMethod::Standard, None).unwrap_err();
    assert!(matches!(err, AccountError::LoginFailed(_)), "{err}");
    assert_eq!(ids(&f, "codex"), ["default"], "no half-made account");
    assert!(!f.data.join("accounts").join("codex").join("work").exists(), "its folder is gone");
    f.mgr.login("codex", "Work", LoginMethod::Standard, None).unwrap_err(); // and the name is free again, not 'work-2'
    assert!(f.mgr.store().get("codex", "work").unwrap().is_none());
}

#[test]
fn a_sign_in_the_agent_does_not_confirm_is_not_kept() {
    let f = fx();
    f.runner.script(Reply::attached("login", 0));
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n"));
    let err = f.mgr.login("codex", "Work", LoginMethod::Standard, None).unwrap_err();
    assert!(matches!(err, AccountError::LoginFailed(ref m) if m.contains("not signed in")), "{err}");
    assert_eq!(ids(&f, "codex"), ["default"]);
}

#[test]
fn a_sign_in_whose_status_cannot_be_read_is_kept_but_marked_unchecked() {
    let f = fx();
    f.runner.script(Reply::attached("login", 0));
    f.runner.script(Reply::status("login status", 101, "", "thread panicked"));
    let a = f.mgr.login("codex", "Work", LoginMethod::Standard, None).unwrap();
    assert_eq!(a.auth, AuthState::Unknown);
    assert!(a.auth_detail.as_deref().unwrap().contains("no way to confirm"));
}

#[test]
fn an_agent_that_is_not_installed_cannot_get_an_account() {
    let f = fx_with(&["claude"]);
    let err = f.mgr.plan_login("codex", "Work", LoginMethod::Standard, None).unwrap_err();
    assert!(matches!(err, AccountError::NotInstalled(ref a) if a == "codex"), "{err}");
    assert_eq!(ids(&f, "codex"), ["default"]);
    assert!(f.runner.calls().is_empty(), "nothing was started");
}

#[test]
fn an_agent_without_a_supported_profile_mechanism_reports_switching_unavailable_and_changes_nothing() {
    let f = fx();
    for agent in ["antigravity", "ollama", "aider"] {
        let err = f.mgr.plan_login(agent, "Second", LoginMethod::Standard, None).unwrap_err();
        match err {
            AccountError::SwitchingUnavailable { agent: a, reason } => {
                assert_eq!(a, agent);
                assert!(reason.len() > 20, "{reason}");
            }
            other => panic!("{agent}: {other}"),
        }
        assert_eq!(ids(&f, agent), ["default"], "{agent} is still monitor-only");
    }
    assert!(f.runner.calls().is_empty());
    assert!(!f.data.join("accounts").exists() || files_under(&f.data.join("accounts")).is_empty(), "no folder was created either");
    // its one account can still be "used" - a no-op, not an error
    assert_eq!(f.mgr.use_account("antigravity", "default").unwrap().account.account_id, DEFAULT_ACCOUNT);
    assert!(matches!(f.mgr.agent("antigravity").unwrap().switch, SwitchSupport::Unavailable { .. }));
}

#[test]
fn a_method_the_agent_does_not_offer_is_refused_before_anything_is_created() {
    let f = fx();
    let err = f.mgr.plan_login("opencode", "Work", LoginMethod::DeviceCode, None).unwrap_err();
    assert!(matches!(err, AccountError::Unsupported(ref m) if m.contains("opencode")), "{err}");
    assert_eq!(ids(&f, "opencode"), ["default"]);
    let err = f.mgr.plan_login("codex", "Work", LoginMethod::ApiKey, None).unwrap_err();
    assert!(matches!(err, AccountError::Unsupported(ref m) if m.contains("API key")), "an API key sign-in needs a key: {err}");
}

#[test]
fn a_duplicate_name_is_refused_and_does_not_start_a_sign_in() {
    let f = fx();
    add_codex(&f, "Work");
    let calls_before = f.runner.calls().len();
    let err = f.mgr.plan_login("codex", "work", LoginMethod::Standard, None).unwrap_err();
    assert!(matches!(err, AccountError::DuplicateLabel(_)), "{err}");
    assert_eq!(f.runner.calls().len(), calls_before);
}

// ------------------------------------------------------------------------------------------------ secrets

#[test]
fn an_api_key_for_claude_lives_in_the_credential_store_and_never_in_the_database() {
    let f = fx();
    let key = "sk-ant-THE-SECRET-1234567890";
    let a = f.mgr.login("claude", "Api", LoginMethod::ApiKey, Some(Secret::new(key))).unwrap();
    assert_eq!(a.auth, AuthState::Unknown, "we cannot check a key without using it, and we say so");
    assert!(a.auth_detail.as_deref().unwrap().contains("credential store"));
    assert!(f.runner.calls().is_empty(), "no command ran: the key is only kept for launch");
    assert_eq!(f.secrets.len(), 1);

    // the secret is in none of the database's files
    for name in ["usage.db", "usage.db-wal", "usage.db-shm"] {
        if let Ok(bytes) = std::fs::read(f.data.join(name)) {
            assert!(!bytes.windows(key.len()).any(|w| w == key.as_bytes()), "{name} contains the key");
        }
    }
    // nor in anything printed
    assert!(!format!("{a:?}").contains(key));

    // at launch it is handed to the agent through its environment, for that one process
    f.mgr.use_account("claude", "api").unwrap();
    let (spec, _) = f.mgr.launch_spec("claude", &[]).unwrap();
    assert_eq!(env_of(&spec, "ANTHROPIC_API_KEY"), Some(OsString::from(key)));
    assert!(!format!("{spec:?}").contains(key), "the spec's Debug output redacts it");
}

#[test]
fn an_api_key_account_is_never_declared_signed_out_just_because_the_agent_has_no_oauth_sign_in() {
    let f = fx();
    f.mgr.login("claude", "Api", LoginMethod::ApiKey, Some(Secret::new("sk-x"))).unwrap();
    // the agent, asked, would say "not logged in" - but that is the wrong question for a key account
    f.runner.script(Reply::status("auth status", 1, r#"{"loggedIn": false}"#, ""));
    let a = f.mgr.check_auth("claude", "api").unwrap();
    assert_eq!(a.auth, AuthState::Unknown);
    assert!(f.runner.calls().is_empty(), "and it was not even asked");
    f.mgr.use_account("claude", "api").unwrap();
    f.mgr.launch_spec("claude", &[]).expect("an unchecked key account can be launched");
}

#[test]
fn an_api_key_for_codex_goes_to_codex_on_stdin_and_we_keep_nothing() {
    let f = fx();
    let key = "sk-proj-CODEX-SECRET-abcdef";
    f.runner.script(Reply::attached("login --with-api-key", 0).writing("CODEX_HOME", "auth.json"));
    f.runner.script(Reply::status("login status", 0, "Logged in using an API key", ""));
    let a = f.mgr.login("codex", "Key", LoginMethod::ApiKey, Some(Secret::new(key))).unwrap();
    assert_eq!(a.auth, AuthState::Valid);
    let login = &f.runner.calls()[0];
    assert_eq!(login.stdin.as_deref(), Some(format!("{key}\n").as_bytes()), "the key is the child's input");
    assert!(login.args.iter().all(|a| !a.to_string_lossy().contains(key)), "never on the command line, where other programs can see it");
    assert!(login.env.iter().all(|(_, v)| !v.to_string_lossy().contains(key)), "nor in the environment");
    assert!(f.secrets.is_empty(), "Codex stores its own key; we hold nothing");
    for name in ["usage.db", "usage.db-wal"] {
        if let Ok(bytes) = std::fs::read(f.data.join(name)) {
            assert!(!bytes.windows(key.len()).any(|w| w == key.as_bytes()), "{name}");
        }
    }
}

#[test]
fn a_credential_store_that_refuses_the_key_leaves_no_account_behind() {
    struct Refusing;
    impl SecretStore for Refusing {
        fn set(&self, _: &SecretKey, _: &Secret) -> AccountResult<()> {
            Err(AccountError::Secret("no OS credential store".into()))
        }
        fn get(&self, _: &SecretKey) -> AccountResult<Option<Secret>> {
            Ok(None)
        }
        fn delete(&self, _: &SecretKey) -> AccountResult<()> {
            Ok(())
        }
        fn is_persistent(&self) -> bool {
            false
        }
    }
    let f = fx();
    let db = Database::open(&f.data.join("usage.db")).unwrap();
    let mgr = AccountManager::new(AccountStore::new(db), f.runner.clone(), Arc::new(Refusing), f.env.clone(), f.data.clone());
    let err = mgr.login("claude", "Api", LoginMethod::ApiKey, Some(Secret::new("k"))).unwrap_err();
    assert!(matches!(err, AccountError::LoginFailed(ref m) if m.contains("credential store")), "{err}");
    assert_eq!(ids(&f, "claude"), ["default"]);
}

// ------------------------------------------------------------------------------------------------ choosing

#[test]
fn switching_one_agent_never_changes_another_and_never_rewrites_an_agents_files() {
    let f = fx();
    add_codex(&f, "Work");
    add_codex(&f, "Personal");
    add_claude(&f, "Work");
    let before_files = files_under(&f.data.join("accounts"));
    let before_calls = f.runner.calls().len();
    let others_before: Vec<_> = f.mgr.store().all_active().unwrap().into_iter().filter(|(a, _)| a != "codex").collect();

    f.mgr.use_account("codex", "work").unwrap();
    assert_eq!(active(&f, "codex"), "work");
    f.mgr.use_account("codex", "Personal").unwrap();
    assert_eq!(active(&f, "codex"), "personal", "found by name too");
    f.mgr.use_account("codex", "default").unwrap();
    f.mgr.use_account("codex", "work").unwrap();

    let others_after: Vec<_> = f.mgr.store().all_active().unwrap().into_iter().filter(|(a, _)| a != "codex").collect();
    assert_eq!(others_before, others_after, "every other agent keeps its active account");
    assert_eq!(active(&f, "claude"), DEFAULT_ACCOUNT);
    assert_eq!(files_under(&f.data.join("accounts")), before_files, "not one file was created, changed or removed by switching");
    assert_eq!(f.runner.calls().len(), before_calls, "switching starts no process");
}

#[test]
fn choosing_an_account_that_does_not_exist_or_belongs_to_another_agent_is_an_error() {
    let f = fx();
    add_claude(&f, "Work");
    assert!(matches!(f.mgr.use_account("codex", "work"), Err(AccountError::UnknownAccount { .. })), "claude's account is not codex's");
    assert!(matches!(f.mgr.use_account("codex", "ghost"), Err(AccountError::UnknownAccount { .. })));
    assert_eq!(active(&f, "codex"), DEFAULT_ACCOUNT);
}

#[test]
fn choosing_an_expired_account_works_but_says_so() {
    let f = fx();
    add_codex(&f, "Work");
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n"));
    assert_eq!(f.mgr.check_auth("codex", "work").unwrap().auth, AuthState::Expired);
    let out = f.mgr.use_account("codex", "work").unwrap();
    assert_eq!(active(&f, "codex"), "work");
    assert!(out.warning.as_deref().unwrap().contains("expired"), "{:?}", out.warning);
    assert!(f.mgr.use_account("codex", "default").unwrap().warning.is_none());
}

// ------------------------------------------------------------------------------------------------ authentication state

#[test]
fn a_session_that_was_signed_in_and_no_longer_is_expired_but_one_never_signed_in_is_not() {
    let f = fx();
    add_codex(&f, "Work");
    assert_eq!(f.mgr.store().get("codex", "work").unwrap().unwrap().auth, AuthState::Valid);
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n"));
    let a = f.mgr.check_auth("codex", "work").unwrap();
    assert_eq!(a.auth, AuthState::Expired);
    assert!(a.auth_detail.as_deref().unwrap().contains("sign in again"));
    assert!(a.auth_checked_utc_ms.is_some());
    // still signed out on the next check: stays expired
    assert_eq!(f.mgr.check_auth("codex", "work").unwrap().auth, AuthState::Expired);
    // signing in again (re-authentication) brings it back
    f.runner.script(Reply::attached("login", 0));
    f.runner.script(Reply::status("login status", 0, "Logged in using ChatGPT", ""));
    let again = f.mgr.relogin("codex", "work", LoginMethod::Standard, None).unwrap();
    assert_eq!(again.auth, AuthState::Valid);
    assert_eq!(ids(&f, "codex"), ["default", "work"], "re-authenticating reuses the account, it does not add another");
}

#[test]
fn a_status_check_that_fails_does_not_flip_the_state() {
    let f = fx();
    add_codex(&f, "Work");
    f.runner.script(Reply::status("login status", 101, "", "thread panicked"));
    let a = f.mgr.check_auth("codex", "work").unwrap();
    assert_eq!(a.auth, AuthState::Valid, "a failed check proves nothing");
    assert!(a.auth_detail.as_deref().unwrap().contains("could not check"));
}

#[test]
fn an_agent_with_no_status_command_is_judged_by_whether_its_sign_in_file_exists_and_the_file_is_never_read() {
    let f = fx();
    // Gemini: no status command; the marker is <profile>/home/.gemini/oauth_creds.json
    let plan = f.mgr.plan_login("gemini", "Work", LoginMethod::Standard, None).unwrap();
    let marker = plan.account.profile_dir.clone().unwrap().join("home").join(".gemini").join("oauth_creds.json");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    // an unreadable stand-in: if the manager opened it as text the invalid bytes would show; it only checks existence
    std::fs::write(&marker, [0xff, 0xfe, 0x00, 0x01]).unwrap();
    let a = f.mgr.finish_login(plan, Ok(0)).unwrap();
    assert_eq!(a.auth, AuthState::Valid);
    std::fs::remove_file(&marker).unwrap();
    assert_eq!(f.mgr.check_auth("gemini", "work").unwrap().auth, AuthState::Expired, "the sign-in file disappeared");
}

#[test]
fn check_all_checks_every_checkable_account_and_one_failure_does_not_stop_the_rest() {
    let f = fx();
    add_codex(&f, "Work");
    add_claude(&f, "Work");
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n")); // codex: expired
    f.runner.script(Reply::status("auth status", 0, r#"{"loggedIn": true}"#, "")); // claude: fine
    let results = f.mgr.check_all();
    let state = |agent: &str, acc: &str| results.iter().find(|(a, i, _)| a == agent && i == acc).and_then(|(_, _, r)| r.as_ref().ok().map(|a| a.auth));
    assert_eq!(state("codex", "work"), Some(AuthState::Expired));
    assert_eq!(state("claude", "work"), Some(AuthState::Valid));
    assert!(results.iter().all(|(a, _, _)| a != "antigravity"), "monitor-only agents are not checked");
}

#[test]
fn a_status_command_that_cannot_even_start_is_unknown_not_a_crash() {
    let f = fx();
    add_codex(&f, "Work");
    // no reply scripted for the next status call -> the fake runner reports an i/o error, like a missing executable
    let a = f.mgr.check_auth("codex", "work").unwrap();
    assert_eq!(a.auth, AuthState::Valid);
}

// ------------------------------------------------------------------------------------------------ removing

#[test]
fn removing_an_account_deletes_its_folder_and_key_and_falls_back_to_default_but_keeps_its_history() {
    let f = fx();
    add_codex(&f, "Work");
    f.mgr.use_account("codex", "work").unwrap();
    let dir = f.data.join("accounts").join("codex").join("work");
    assert!(dir.exists());
    f.mgr.store().database().commit_for("work", &Batch { agent: "codex", events: vec![event("codex", "e1", 5_000_000, 42)], cursors: vec![] }).unwrap();

    f.mgr.remove("codex", "work", false).unwrap();
    assert!(!dir.exists(), "its sign-in is gone from the disk");
    assert_eq!(active(&f, "codex"), DEFAULT_ACCOUNT);
    assert_eq!(ids(&f, "codex"), ["default"]);
    let kept: i64 = f.mgr.store().database().with_reader(|c| Ok(c.query_row("SELECT SUM(input_tokens) FROM usage_buckets WHERE account_id = 'work'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(kept, 42, "the usage already recorded stays part of the totals");
}

#[test]
fn removing_with_purge_also_deletes_the_usage_history() {
    let f = fx();
    add_codex(&f, "Work");
    f.mgr.store().database().commit_for("work", &Batch { agent: "codex", events: vec![event("codex", "e1", 5_000_000, 42)], cursors: vec![] }).unwrap();
    f.mgr.remove("codex", "work", true).unwrap();
    let left: i64 = f.mgr.store().database().with_reader(|c| Ok(c.query_row("SELECT count(*) FROM usage_events WHERE account_id = 'work'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn removing_an_account_deletes_the_api_key_we_held_for_it() {
    let f = fx();
    f.mgr.login("claude", "Api", LoginMethod::ApiKey, Some(Secret::new("sk-x"))).unwrap();
    assert_eq!(f.secrets.len(), 1);
    f.mgr.remove("claude", "api", false).unwrap();
    assert!(f.secrets.is_empty(), "not left behind in the credential store");
}

#[test]
fn the_default_account_cannot_be_removed() {
    let f = fx();
    assert!(matches!(f.mgr.remove("codex", "default", false), Err(AccountError::CannotRemoveDefault)));
    assert!(matches!(f.mgr.remove("codex", "ghost", false), Err(AccountError::UnknownAccount { .. })));
}

#[test]
fn nothing_outside_the_accounts_folder_is_ever_deleted_even_if_the_database_says_so() {
    let f = fx();
    add_codex(&f, "Work");
    let outside = f.data.parent().unwrap().join("precious");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), b"mine").unwrap();
    // a corrupted or hand-edited database points the account at somebody's real folder
    f.mgr
        .store()
        .database()
        .with_writer(|w| Ok(w.conn.execute("UPDATE accounts SET profile_dir = ?1 WHERE agent_id = 'codex' AND account_id = 'work'", [outside.to_string_lossy().as_ref()])?))
        .unwrap();
    let err = f.mgr.remove("codex", "work", false).unwrap_err();
    assert!(matches!(err, AccountError::Unsupported(_)), "{err}");
    assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"mine", "untouched");
    assert!(ids(&f, "codex").contains(&"work".to_string()), "and the account is still listed, so nothing is half done");
}

// ------------------------------------------------------------------------------------------------ launching

#[test]
fn launching_uses_the_active_accounts_profile_and_the_default_account_gets_no_overrides() {
    let f = fx();
    add_codex(&f, "Work");
    let (spec, acc) = f.mgr.launch_spec("codex", &["--version".into()]).unwrap();
    assert_eq!(acc.account_id, DEFAULT_ACCOUNT);
    assert!(spec.env.is_empty() && spec.env_remove.is_empty(), "the agent's own sign-in is used exactly as the user has it");
    assert_eq!(spec.args, vec![OsString::from("--version")]);

    f.mgr.use_account("codex", "work").unwrap();
    let (spec, acc) = f.mgr.launch_spec("codex", &[]).unwrap();
    assert_eq!(acc.account_id, "work");
    let home = f.data.join("accounts").join("codex").join("work").join("home").join(".codex");
    assert_eq!(env_of(&spec, "CODEX_HOME"), Some(home.into_os_string()));
    assert!(f.mgr.store().get("codex", "work").unwrap().unwrap().last_used_utc_ms.is_some());
}

#[test]
fn an_api_key_in_the_shell_cannot_override_a_signed_in_accounts_own_sign_in() {
    let f = fx();
    add_claude(&f, "Work");
    f.mgr.use_account("claude", "work").unwrap();
    let (spec, _) = f.mgr.launch_spec("claude", &[]).unwrap();
    assert!(spec.env_remove.iter().any(|k| k == "ANTHROPIC_API_KEY"), "the variable is removed from the child's environment");
    assert!(env_of(&spec, "CLAUDE_CONFIG_DIR").is_some());
}

#[test]
fn launching_with_an_account_that_needs_signing_in_again_says_how() {
    let f = fx();
    add_codex(&f, "Work");
    f.mgr.use_account("codex", "work").unwrap();
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n"));
    f.mgr.check_auth("codex", "work").unwrap();
    let err = f.mgr.launch_spec("codex", &[]).unwrap_err();
    assert!(matches!(err, AccountError::NeedsLogin(ref a) if a == "codex/work"), "{err}");
    assert!(err.to_string().contains("agm login codex"), "{err}");
}

#[test]
fn launching_an_agent_that_is_not_installed_is_a_clear_error() {
    let f = fx_with(&[]);
    assert!(matches!(f.mgr.launch_spec("codex", &[]), Err(AccountError::NotInstalled(_))));
}

#[test]
fn run_starts_the_agent_on_the_terminal_with_the_active_account_and_returns_its_exit_code() {
    let f = fx();
    add_codex(&f, "Work");
    f.mgr.use_account("codex", "work").unwrap();
    f.runner.script(Reply::attached("exec hello", 7));
    let code = f.mgr.run("codex", &["exec".into(), "hello".into()]).unwrap();
    assert_eq!(code, 7);
    assert!(env_of(f.runner.calls().last().unwrap(), "CODEX_HOME").is_some());
}

// ------------------------------------------------------------------------------------------------ concurrency

#[test]
fn many_threads_switching_one_agent_leave_exactly_one_consistent_active_account() {
    let f = Arc::new(fx());
    for l in ["A", "B", "C"] {
        add_codex(&f, l);
    }
    let handles: Vec<_> = (0..12)
        .map(|i| {
            let f = f.clone();
            std::thread::spawn(move || {
                let target = ["a", "b", "c", "default"][i % 4];
                f.mgr.use_account("codex", target).map(|o| o.account.account_id)
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap().expect("every switch succeeds; none is lost to a lock error");
    }
    let rows = f.mgr.store().all_active().unwrap().into_iter().filter(|(a, _)| a == "codex").count();
    assert_eq!(rows, 1, "still exactly one active row for the agent");
    assert!(["a", "b", "c", "default"].contains(&active(&f, "codex").as_str()));
    assert_eq!(active(&f, "claude"), DEFAULT_ACCOUNT, "and another agent was never touched");
}

#[test]
fn two_processes_switching_the_same_agent_do_not_corrupt_each_other() {
    let f = fx();
    add_codex(&f, "A");
    add_codex(&f, "B");
    let other = second_manager(&f); // the CLI, next to the app
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 0..15 {
                f.mgr.use_account("codex", if i % 2 == 0 { "a" } else { "b" }).unwrap();
            }
        });
        s.spawn(|| {
            for i in 0..15 {
                other.use_account("codex", if i % 2 == 0 { "b" } else { "a" }).unwrap();
            }
        });
    });
    let final_choice = active(&f, "codex");
    assert!(final_choice == "a" || final_choice == "b");
    assert_eq!(other.store().active("codex").unwrap().unwrap().account_id, final_choice, "both see the same truth");
}

#[test]
fn a_change_in_progress_on_one_agent_makes_that_agent_busy_but_not_the_others() {
    let f = fx();
    add_claude(&f, "Work");
    let short = second_manager(&f).with_lock_wait(Duration::from_millis(200));
    let held = f.mgr.lock("codex").unwrap(); // somebody is in the middle of changing codex
    let err = short.use_account("codex", "default").unwrap_err();
    assert!(matches!(err, AccountError::Busy(ref a) if a == "codex"), "{err}");
    assert!(err.to_string().contains("in progress"));
    short.use_account("claude", "work").expect("another agent is not blocked");
    drop(held);
    short.use_account("codex", "default").expect("free again");
}

#[test]
fn a_sign_in_and_a_removal_of_the_same_agent_are_serialised() {
    let f = Arc::new(fx());
    add_codex(&f, "Keep");
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let f = f.clone();
            std::thread::spawn(move || {
                script_codex_login(&f);
                let label = format!("Temp {i}");
                let a = f.mgr.login("codex", &label, LoginMethod::Standard, None).unwrap();
                f.mgr.remove("codex", &a.account_id, false).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(ids(&f, "codex"), ["default", "keep"], "every temporary account came and went; nothing was left half-made");
    assert_eq!(files_under(&f.data.join("accounts").join("codex")).iter().filter(|(p, _)| p.to_string_lossy().contains("temp")).count(), 0);
}

// ------------------------------------------------------------------------------------------------ restart recovery

#[test]
fn everything_survives_a_restart() {
    let f = fx();
    add_codex(&f, "Work");
    add_claude(&f, "Personal");
    f.mgr.use_account("codex", "work").unwrap();
    f.mgr.use_account("claude", "personal").unwrap();

    let restarted = second_manager(&f); // a fresh process over the same folder
    restarted.init().unwrap();
    let current: Vec<(String, String)> = restarted.current().unwrap().into_iter().map(|(a, acc)| (a, acc.account_id)).collect();
    assert!(current.contains(&("codex".into(), "work".into())));
    assert!(current.contains(&("claude".into(), "personal".into())));
    assert!(current.contains(&("gemini".into(), DEFAULT_ACCOUNT.into())), "an agent nobody touched is still on its default");
    let work = restarted.store().get("codex", "work").unwrap().unwrap();
    assert_eq!(work.auth, AuthState::Valid);
    assert!(work.profile_dir.unwrap().join("home").join(".codex").join("auth.json").is_file());
}

#[test]
fn init_twice_changes_nothing() {
    let f = fx();
    add_codex(&f, "Work");
    f.mgr.use_account("codex", "work").unwrap();
    f.mgr.init().unwrap();
    f.mgr.init().unwrap();
    assert_eq!(active(&f, "codex"), "work");
    assert_eq!(ids(&f, "codex"), ["default", "work"]);
}

#[test]
fn a_sign_in_interrupted_by_a_crash_is_settled_at_the_next_start() {
    let f = fx();
    // two sign-ins were started and the process died before either was settled
    let finished_in_the_browser = f.mgr.plan_login("codex", "Finished", LoginMethod::Standard, None).unwrap();
    let abandoned = f.mgr.plan_login("claude", "Abandoned", LoginMethod::Standard, None).unwrap();
    std::fs::write(finished_in_the_browser.account.profile_dir.clone().unwrap().join("home").join(".codex").join("auth.json"), b"x").unwrap();
    drop((finished_in_the_browser, abandoned));
    assert_eq!(f.mgr.store().get("codex", "finished").unwrap().unwrap().auth, AuthState::Pending);

    f.runner.script(Reply::status("login status", 0, "Logged in using ChatGPT", ""));
    f.runner.script(Reply::status("auth status", 1, r#"{"loggedIn": false}"#, ""));
    let restarted = second_manager(&f);
    restarted.init().unwrap();

    assert_eq!(restarted.store().get("codex", "finished").unwrap().unwrap().auth, AuthState::Valid, "the sign-in did complete: kept");
    assert!(restarted.store().get("claude", "abandoned").unwrap().is_none(), "the one that never completed is cleaned up");
    assert!(!f.data.join("accounts").join("claude").join("abandoned").exists());
}

#[test]
fn an_interrupted_sign_in_that_was_made_active_does_not_strand_the_agent() {
    let f = fx();
    let plan = f.mgr.plan_login("codex", "Half", LoginMethod::Standard, None).unwrap();
    f.mgr.use_account("codex", "half").unwrap(); // chosen while still pending
    drop(plan);
    f.runner.script(Reply::status("login status", 1, "", "Not logged in\n"));
    second_manager(&f).init().unwrap();
    assert_eq!(active(&f, "codex"), DEFAULT_ACCOUNT, "falls back to the default account");
    assert_eq!(ids(&f, "codex"), ["default"]);
}

// ------------------------------------------------------------------------------------------------ what the monitor reads

#[test]
fn the_monitor_reads_each_managed_accounts_usage_from_that_accounts_own_folder() {
    let f = fx();
    add_codex(&f, "Work");
    add_claude(&f, "Work");
    let envs = account_collector_envs(f.mgr.store(), &f.env);
    assert_eq!(envs.len(), 2, "one per managed account; the default account uses the machine's own folders");
    let (agent, account, env) = envs.iter().find(|(a, _, _)| a == "codex").unwrap();
    assert_eq!((agent.as_str(), account.as_str()), ("codex", "work"));
    assert_eq!(env.home, f.data.join("accounts").join("codex").join("work").join("home"));
    assert_eq!(env.path_dirs, f.env.path_dirs, "the agent's executable is still found");
    f.mgr.remove("codex", "work", false).unwrap();
    assert_eq!(account_collector_envs(f.mgr.store(), &f.env).len(), 1, "a removed account is no longer read");
}

// ------------------------------------------------------------------------------------------------ the real agents

/// Runs the *real* `codex`, `claude` and `opencode` against empty scratch profile folders and checks that what they print is
/// understood: an account that was never signed in must come out as "not signed in". Each agent is pointed at a folder inside
/// the temp directory with its own documented variable, so no real sign-in is read or changed. Needs the tools installed, so it
/// is ignored by default: `cargo test -p ai_usage_monitor_core real_agents -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_agents_report_an_empty_profile_as_not_signed_in() {
    use crate::accounts::runner::SystemRunner;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let db = Database::open(&data.join("usage.db")).unwrap();
    let mgr = AccountManager::new(AccountStore::new(db), Arc::new(SystemRunner), Arc::new(MemorySecretStore::new()), Env::from_system(), data.clone());
    mgr.init().unwrap();
    let mut checked = 0;
    for agent in ["codex", "claude", "opencode"] {
        if !mgr.agent(agent).unwrap().installed {
            eprintln!("{agent}: not installed here, skipped");
            continue;
        }
        let acc = mgr.store().insert_managed(agent, "Probe", |id| data.join("accounts").join(agent).join(id), 1).unwrap();
        std::fs::create_dir_all(acc.profile_dir.as_ref().unwrap().join("home")).unwrap();
        let after = mgr.check_auth(agent, &acc.account_id).unwrap();
        eprintln!("{agent}: {} ({:?})", after.auth.label(), after.auth_detail);
        assert_eq!(after.auth, AuthState::NotLoggedIn, "{agent} should report an empty profile as not signed in");
        checked += 1;
    }
    assert!(checked > 0, "none of the agents is installed, so nothing was checked");
}

#[test]
fn a_profile_folder_that_has_gone_missing_is_recreated_before_the_agent_is_asked_anything() {
    // Real Codex fails with a configuration error when CODEX_HOME names a folder that does not exist.
    let f = fx();
    add_codex(&f, "Work");
    let agent_dir = f.data.join("accounts").join("codex").join("work").join("home").join(".codex");

    std::fs::remove_dir_all(&agent_dir).unwrap();
    f.runner.script(Reply::status("login status", 0, "Logged in using ChatGPT", ""));
    f.mgr.check_auth("codex", "work").unwrap();
    assert!(agent_dir.is_dir(), "recreated for the status check");

    f.mgr.use_account("codex", "work").unwrap();
    std::fs::remove_dir_all(&agent_dir).unwrap();
    f.mgr.launch_spec("codex", &[]).unwrap();
    assert!(agent_dir.is_dir(), "and recreated for a launch");
}

#[test]
fn a_profile_path_outside_the_accounts_folder_is_never_created() {
    let f = fx();
    add_codex(&f, "Work");
    let outside = f.data.parent().unwrap().join("not-ours");
    f.mgr
        .store()
        .database()
        .with_writer(|w| Ok(w.conn.execute("UPDATE accounts SET profile_dir = ?1 WHERE agent_id = 'codex' AND account_id = 'work'", [outside.to_string_lossy().as_ref()])?))
        .unwrap();
    f.runner.script(Reply::status("login status", 1, "", "Not logged in
"));
    f.mgr.check_auth("codex", "work").unwrap();
    assert!(!outside.exists(), "nothing is created outside the accounts folder, whatever the database says");
    let climbing = f.data.join("accounts").join("..").join("..").join("escaped");
    assert!(!f.mgr.is_managed_path(&climbing), "`..` cannot climb out");
}
