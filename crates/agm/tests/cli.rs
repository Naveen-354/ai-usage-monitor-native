//! End-to-end: the real `agm` binary, run as a process, against stand-in agents.
//!
//! The stand-ins are small scripts named `codex` and `claude` that behave like the real CLIs do (sign-in writes a file inside
//! the profile folder the environment points at; the status command reads it; output formats are the ones recorded from the
//! real tools). `PATH` holds only the stand-ins, so no test can start - or touch the sign-in of - a real agent.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ai_usage_monitor_core::database::{Batch, Database};
use ai_usage_monitor_core::model::{Accuracy, UsageEvent};

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

struct Rig {
    _dir: tempfile::TempDir,
    data: PathBuf,
    bin: PathBuf,
}

#[cfg(windows)]
const CODEX: &str = r#"@echo off
if "%~1"=="login" if "%~2"=="status" goto status
if "%~1"=="login" goto login
if "%~1"=="fail" exit /b 5
echo codex-stub args: %*
echo CODEX_HOME=[%CODEX_HOME%]
exit /b 0
:status
if exist "%CODEX_HOME%\auth.json" (echo Logged in using ChatGPT& exit /b 0)
echo Not logged in 1>&2
exit /b 1
:login
if not exist "%CODEX_HOME%" mkdir "%CODEX_HOME%"
echo stand-in sign-in> "%CODEX_HOME%\auth.json"
exit /b 0
"#;

#[cfg(windows)]
const CLAUDE: &str = r#"@echo off
if "%~1"=="auth" if "%~2"=="status" goto status
if "%~1"=="auth" if "%~2"=="login" goto login
echo claude-stub args: %*
echo CLAUDE_CONFIG_DIR=[%CLAUDE_CONFIG_DIR%]
exit /b 0
:status
if exist "%CLAUDE_CONFIG_DIR%\.credentials.json" (echo {"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com"}& exit /b 0)
echo {"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}
exit /b 1
:login
if not exist "%CLAUDE_CONFIG_DIR%" mkdir "%CLAUDE_CONFIG_DIR%"
echo stand-in sign-in> "%CLAUDE_CONFIG_DIR%\.credentials.json"
exit /b 0
"#;

#[cfg(not(windows))]
const CODEX: &str = r#"#!/bin/sh
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  if [ -f "$CODEX_HOME/auth.json" ]; then echo "Logged in using ChatGPT"; exit 0; fi
  echo "Not logged in" 1>&2; exit 1
fi
if [ "$1" = "login" ]; then mkdir -p "$CODEX_HOME"; echo stand-in > "$CODEX_HOME/auth.json"; exit 0; fi
if [ "$1" = "fail" ]; then exit 5; fi
echo "codex-stub args: $*"
echo "CODEX_HOME=[$CODEX_HOME]"
"#;

#[cfg(not(windows))]
const CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  if [ -f "$CLAUDE_CONFIG_DIR/.credentials.json" ]; then echo '{"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com"}'; exit 0; fi
  echo '{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}'; exit 1
fi
if [ "$1" = "auth" ] && [ "$2" = "login" ]; then mkdir -p "$CLAUDE_CONFIG_DIR"; echo stand-in > "$CLAUDE_CONFIG_DIR/.credentials.json"; exit 0; fi
echo "claude-stub args: $*"
echo "CLAUDE_CONFIG_DIR=[$CLAUDE_CONFIG_DIR]"
"#;

fn write_stub(bin: &Path, name: &str, body: &str) {
    let file = if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() };
    let path = bin.join(file);
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (data, bin) = (dir.path().join("data"), dir.path().join("bin"));
    std::fs::create_dir_all(&bin).unwrap();
    write_stub(&bin, "codex", CODEX);
    write_stub(&bin, "claude", CLAUDE);
    Rig { _dir: dir, data, bin }
}

impl Rig {
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_agm"));
        cmd.arg("--data-dir").arg(&self.data).args(args);
        let system = if cfg!(windows) { format!("{}\\System32", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())) } else { "/usr/bin:/bin".into() };
        let sep = if cfg!(windows) { ";" } else { ":" };
        cmd.env("PATH", format!("{}{sep}{system}", self.bin.display()));
        // nothing the test does may reach a real agent's sign-in
        for var in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "GEMINI_CLI_HOME", "XDG_DATA_HOME", "ANTHROPIC_API_KEY", "GEMINI_API_KEY", "AI_USAGE_MONITOR_DATA_DIR"] {
            cmd.env_remove(var);
        }
        cmd
    }

    fn agm(&self, args: &[&str]) -> Out {
        let o = self.command(args).stdin(Stdio::null()).output().unwrap();
        Out { code: o.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() }
    }

    fn profile(&self, agent: &str, account: &str) -> PathBuf {
        self.data.join("accounts").join(agent).join(account)
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let o = self.agm(args);
        assert_eq!(o.code, 0, "{}", o.stderr);
        serde_json::from_str(&o.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", o.stdout))
    }

    fn active(&self, agent: &str) -> String {
        let v = self.json(&["current", "--json"]);
        v["current"].as_array().unwrap().iter().find(|c| c["agent"] == agent).unwrap()["account"].as_str().unwrap().to_string()
    }
}

#[test]
fn help_lists_every_command_in_the_brief() {
    let r = rig();
    let o = r.agm(&["--help"]);
    assert_eq!(o.code, 0);
    for cmd in ["accounts", "login", "use", "current", "usage", "run", "remove"] {
        assert!(o.stdout.contains(cmd), "`agm --help` does not mention {cmd}:\n{}", o.stdout);
    }
}

#[test]
fn the_whole_story_sign_in_list_switch_current_run_and_back() {
    let r = rig();

    // sign in: the agent's own sign-in runs, inside the new account's own folder
    let o = r.agm(&["login", "codex", "--name", "Work"]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    assert!(o.stdout.contains("codex/work is signed in"), "{}", o.stdout);
    assert!(o.stdout.contains("agm use codex work"), "{}", o.stdout);
    assert!(r.profile("codex", "work").join("home").join(".codex").join("auth.json").is_file(), "the (stand-in) agent saved its sign-in in the account's folder");

    // list: the new account is there and signed in; the default account is still the active one
    let v = r.json(&["accounts", "codex", "--json"]);
    let accounts = v["agents"][0]["accounts"].as_array().unwrap();
    let work = accounts.iter().find(|a| a["id"] == "work").unwrap();
    assert_eq!((work["signIn"].as_str(), work["active"].as_bool()), (Some("valid"), Some(false)));
    assert_eq!(accounts.iter().find(|a| a["id"] == "default").unwrap()["active"], true);
    assert_eq!(v["agents"][0]["switching"]["mechanism"], "CODEX_HOME");

    // switch
    let o = r.agm(&["use", "codex", "work"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("codex now uses work"), "{}", o.stdout);
    assert_eq!(r.active("codex"), "work");

    // run: the agent is started pointed at the account's folder, with its own arguments and exit code
    let o = r.agm(&["run", "codex", "exec", "--flag", "hello"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("codex-stub args: exec --flag hello"), "{}", o.stdout);
    let expected_home = r.profile("codex", "work").join("home").join(".codex");
    assert!(o.stdout.contains(&format!("CODEX_HOME=[{}]", expected_home.display())), "the agent ran with the account's folder:\n{}", o.stdout);
    assert_eq!(r.agm(&["run", "codex", "fail"]).code, 5, "the agent's own exit code is passed on");

    // and back to the default account: the agent is started with *no* override
    assert_eq!(r.agm(&["use", "codex", "default"]).code, 0);
    let o = r.agm(&["run", "codex"]);
    assert!(o.stdout.contains("CODEX_HOME=[]"), "the default account gets no override:\n{}", o.stdout);
}

#[test]
fn switching_one_agent_never_changes_another() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    assert_eq!(r.agm(&["login", "claude", "--name", "Work"]).code, 0);
    assert_eq!(r.agm(&["use", "claude", "work"]).code, 0);
    assert_eq!((r.active("claude"), r.active("codex")), ("work".to_string(), "default".to_string()));
    assert_eq!(r.agm(&["use", "codex", "work"]).code, 0);
    assert_eq!(r.agm(&["use", "claude", "default"]).code, 0);
    assert_eq!((r.active("claude"), r.active("codex")), ("default".to_string(), "work".to_string()));
    // every other agent is untouched throughout
    for other in ["gemini", "antigravity", "opencode", "ollama", "aider"] {
        assert_eq!(r.active(other), "default", "{other}");
    }
}

#[test]
fn the_same_name_for_two_agents_is_two_separate_accounts_with_their_own_folders() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    assert_eq!(r.agm(&["login", "claude", "--name", "Work"]).code, 0);
    assert!(r.profile("codex", "work").join("home").join(".codex").is_dir());
    assert!(r.profile("claude", "work").join("home").join(".claude").join(".credentials.json").is_file());
    assert!(!r.profile("codex", "work").join("home").join(".claude").exists(), "no leakage between agents");
}

#[test]
fn an_agent_without_a_supported_profile_mechanism_is_monitored_but_refuses_to_pretend() {
    let r = rig();
    let o = r.agm(&["login", "antigravity", "--name", "Second"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("antigravity") && o.stderr.contains("credential store"), "the reason is given: {}", o.stderr);
    let o = r.agm(&["accounts", "antigravity"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("monitor only - switching unavailable") && o.stdout.contains("why:"), "{}", o.stdout);
    assert!(!r.data.join("accounts").join("antigravity").exists(), "nothing was created");
}

#[test]
fn an_expired_session_is_detected_refused_for_launch_and_fixed_by_signing_in_again() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    assert_eq!(r.agm(&["use", "codex", "work"]).code, 0);
    // the session goes away (the stand-in's sign-in file is deleted, as expiry would do)
    std::fs::remove_file(r.profile("codex", "work").join("home").join(".codex").join("auth.json")).unwrap();

    let v = r.json(&["accounts", "codex", "--check", "--json"]);
    let work = v["agents"][0]["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "work").unwrap().clone();
    assert_eq!(work["signIn"], "expired", "was signed in, now is not");

    let o = r.agm(&["run", "codex"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("not signed in") && o.stderr.contains("agm login codex"), "{}", o.stderr);

    let o = r.agm(&["use", "codex", "work"]);
    assert_eq!(o.code, 0, "choosing an expired account still works, with a warning");
    assert!(o.stderr.contains("expired"), "{}", o.stderr);

    // re-authenticate through the same flow; the account is reused, not duplicated
    let o = r.agm(&["login", "codex", "--account", "work"]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    let o = r.agm(&["run", "codex", "ok"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let v = r.json(&["accounts", "codex", "--json"]);
    assert_eq!(v["agents"][0]["accounts"].as_array().unwrap().len(), 2, "default + work, no duplicate");
}

#[test]
fn an_account_can_be_removed_and_its_sign_in_folder_goes_with_it() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    assert_eq!(r.agm(&["use", "codex", "work"]).code, 0);
    let dir = r.profile("codex", "work");
    assert!(dir.exists());

    // without --yes and with "no" on stdin nothing happens
    let mut child = r.command(&["remove", "codex", "work"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(dir.exists(), "declined: still there");

    let o = r.agm(&["remove", "codex", "work", "--yes"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(!dir.exists(), "the sign-in folder is gone");
    assert_eq!(r.active("codex"), "default", "the agent fell back to its own account");
    let v = r.json(&["accounts", "codex", "--json"]);
    assert_eq!(v["agents"][0]["accounts"].as_array().unwrap().len(), 1);
}

#[test]
fn the_default_account_cannot_be_removed_and_mistakes_are_clear_errors() {
    let r = rig();
    let o = r.agm(&["remove", "codex", "default", "--yes"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("cannot be removed"), "{}", o.stderr);
    for (args, needle) in [
        (vec!["accounts", "nope"], "unknown agent 'nope'"),
        (vec!["use", "codex", "ghost"], "no account 'ghost'"),
        (vec!["login", "nope", "--name", "x"], "unknown agent"),
        (vec!["run", "nope"], "unknown agent"),
    ] {
        let o = r.agm(&args);
        assert_eq!(o.code, 1, "{args:?}");
        assert!(o.stderr.contains(needle), "{args:?}: {}", o.stderr);
    }
}

#[test]
fn a_sign_in_that_does_not_complete_leaves_no_account_behind() {
    let r = rig();
    // a stand-in `codex` whose sign-in fails
    write_stub(&r.bin, "codex", if cfg!(windows) { "@echo off\r\nexit /b 3\r\n" } else { "#!/bin/sh\nexit 3\n" });
    let o = r.agm(&["login", "codex", "--name", "Broken"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("did not complete"), "{}", o.stderr);
    assert!(!r.profile("codex", "broken").exists());
    let v = r.json(&["accounts", "codex", "--json"]);
    assert_eq!(v["agents"][0]["accounts"].as_array().unwrap().len(), 1);
}

fn seed_usage(r: &Rig, account: &str, key: &str, input: u64, when_ms: i64) {
    let db = Database::open(&r.data.join("usage.db")).unwrap();
    let event = UsageEvent {
        agent: "codex",
        model: "m".into(),
        ts_utc_ms: when_ms,
        input_tokens: input,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: None,
        session_id: None,
        project: None,
        source: "test",
        accuracy: Accuracy::Real,
        dedupe_key: key.into(),
    };
    db.commit_for(account, &Batch { agent: "codex", events: vec![event], cursors: vec![] }).unwrap();
}

#[test]
fn usage_is_shown_per_account_and_adds_up_per_agent_and_overall() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    let now = chrono_now_ms();
    seed_usage(&r, "default", "a", 1_000, now);
    seed_usage(&r, "work", "b", 250, now);
    seed_usage(&r, "work", "c", 4_000, now - 400 * 86_400_000); // over a year ago: lifetime only

    let v = r.json(&["usage", "--all", "--json"]);
    let rows = v["usage"].as_array().unwrap();
    let find = |needle: &str| rows.iter().find(|row| row["account"].as_str().unwrap().starts_with(needle)).unwrap();
    assert_eq!(find("default")["spans"]["today"]["total"], 1_000);
    assert_eq!(find("work")["spans"]["today"]["total"], 250);
    assert_eq!(find("work")["spans"]["lifetime"]["total"], 4_250, "the old event is lifetime only");
    assert_eq!(find("work")["spans"]["year"]["total"], 250);

    // by default only the active account is shown (the default one here)
    let v = r.json(&["usage", "codex", "--json"]);
    let rows = v["usage"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["account"].as_str().unwrap().starts_with("default"));

    let o = r.agm(&["usage", "--all"]);
    assert!(o.stdout.contains("ALL ACCOUNTS") && o.stdout.contains("1.25K"), "1,000 + 250 today: {}", o.stdout);
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

#[test]
fn several_real_processes_switching_at_once_never_corrupt_the_state() {
    let r = rig();
    for n in ["A", "B", "C"] {
        assert_eq!(r.agm(&["login", "codex", "--name", n]).code, 0);
    }
    let targets = ["a", "b", "c", "default"];
    let children: Vec<_> = (0..12).map(|i| r.command(&["use", "codex", targets[i % 4]]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()).collect();
    for c in children {
        let o = c.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let now = r.active("codex");
    assert!(targets.contains(&now.as_str()), "{now}");
    let v = r.json(&["accounts", "codex", "--json"]);
    let active_count = v["agents"][0]["accounts"].as_array().unwrap().iter().filter(|a| a["active"] == true).count();
    assert_eq!(active_count, 1, "exactly one active account");
}

#[test]
fn state_survives_between_separate_runs_of_the_tool() {
    let r = rig();
    assert_eq!(r.agm(&["login", "claude", "--name", "Personal"]).code, 0);
    assert_eq!(r.agm(&["use", "claude", "personal"]).code, 0);
    // each call above was a fresh process; a later one still sees everything
    let o = r.agm(&["current"]);
    let line = o.stdout.lines().find(|l| l.contains("claude")).unwrap();
    assert!(line.contains("personal") && line.contains("SIGNED IN"), "{line}");
    let v = r.json(&["accounts", "claude", "--json"]);
    let personal = v["agents"][0]["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "personal").unwrap().clone();
    assert_eq!(personal["identity"], "me@example.com", "the identity the agent reported was kept");
}

#[test]
fn nothing_the_tool_prints_or_stores_contains_a_secret() {
    let r = rig();
    assert_eq!(r.agm(&["login", "codex", "--name", "Work"]).code, 0);
    let mut printed = String::new();
    for args in [vec!["accounts"], vec!["accounts", "--json"], vec!["current"], vec!["usage", "--all"], vec!["use", "codex", "work"]] {
        let o = r.agm(&args);
        printed.push_str(&o.stdout);
        printed.push_str(&o.stderr);
    }
    assert!(!printed.contains("stand-in sign-in"), "the contents of the agent's sign-in file are never read or shown");
    for name in ["usage.db", "usage.db-wal"] {
        if let Ok(bytes) = std::fs::read(r.data.join(name)) {
            let needle = b"stand-in sign-in";
            assert!(!bytes.windows(needle.len()).any(|w| w == needle), "{name} holds the sign-in contents");
        }
    }
}
