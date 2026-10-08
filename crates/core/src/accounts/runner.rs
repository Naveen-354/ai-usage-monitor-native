//! The only place processes are started. Everything above it talks to [`ProcessRunner`], so tests replace it with a script
//! and no test ever launches a real agent or touches a real sign-in.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::collectors::Env;

/// Output kept from a status command: more than this is cut off (a status line is a few hundred bytes).
const CAPTURE_LIMIT: usize = 64 * 1024;

/// A process to start. `Debug` shows names only - never environment *values* or the stdin payload, either of which can be
/// a secret.
#[derive(Clone, Default)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub env_remove: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Bytes written to the child's standard input, then closed (used to hand an API key to `codex login --with-api-key`).
    pub stdin: Option<Vec<u8>>,
}

impl fmt::Debug for ProcessSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &self.env.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .field("env_remove", &self.env_remove)
            .field("cwd", &self.cwd)
            .field("stdin", &self.stdin.as_ref().map(|b| format!("<{} bytes>", b.len())))
            .finish()
    }
}

/// What a quiet run printed. Only used to read a status; never stored or shown as-is.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct StatusOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl fmt::Debug for StatusOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusOutput")
            .field("exit_code", &self.exit_code)
            .field("stdout", &format!("<{} bytes>", self.stdout.len()))
            .field("stderr", &format!("<{} bytes>", self.stderr.len()))
            .field("timed_out", &self.timed_out)
            .finish()
    }
}

pub trait ProcessRunner: Send + Sync {
    /// Runs without showing anything and captures the output; killed after `timeout`.
    fn capture(&self, spec: &ProcessSpec, timeout: Duration) -> io::Result<StatusOutput>;
    /// Runs on the user's own terminal (standard streams inherited) and waits. Returns the exit code.
    fn run_attached(&self, spec: &ProcessSpec) -> io::Result<i32>;
    /// Runs in a console window of its own and waits - for sign-ins started from the desktop app. Returns the exit code.
    fn run_in_new_window(&self, spec: &ProcessSpec) -> io::Result<i32>;
}

/// The real thing.
#[derive(Default)]
pub struct SystemRunner;

fn command(spec: &ProcessSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args);
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }
    cmd
}

#[cfg(windows)]
fn creation_flags(cmd: &mut Command, flags: u32) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(flags);
}
#[cfg(not(windows))]
fn creation_flags(_cmd: &mut Command, _flags: u32) {}

#[cfg_attr(not(windows), allow(dead_code))]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg_attr(not(windows), allow(dead_code))]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

fn drain(mut r: impl Read + Send + 'static, into: Arc<Mutex<Vec<u8>>>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = r.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut buf) = into.lock() {
                let room = CAPTURE_LIMIT.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..n.min(room)]); // keep reading past the cap so the child never blocks on a full pipe
            }
        }
    })
}

impl ProcessRunner for SystemRunner {
    fn capture(&self, spec: &ProcessSpec, timeout: Duration) -> io::Result<StatusOutput> {
        let mut cmd = command(spec);
        cmd.stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
        creation_flags(&mut cmd, CREATE_NO_WINDOW); // no console window flashing up from the desktop app
        let mut child = cmd.spawn()?;
        if let (Some(data), Some(mut stdin)) = (&spec.stdin, child.stdin.take()) {
            let _ = stdin.write_all(data); // dropped right after: the child sees end-of-input
        }
        let (out_buf, err_buf) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
        let readers = [child.stdout.take().map(|o| drain(o, out_buf.clone())), child.stderr.take().map(|e| drain(e, err_buf.clone()))];

        let started = Instant::now();
        let (exit_code, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status.code(), false);
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        // A grandchild that inherited the pipes can keep them open after the child is gone: give the readers a moment, then go on.
        let grace = Instant::now();
        for r in readers.into_iter().flatten() {
            while !r.is_finished() && grace.elapsed() < Duration::from_millis(400) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let text = |b: &Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&b.lock().map(|g| g.clone()).unwrap_or_default()).into_owned();
        Ok(StatusOutput { exit_code, stdout: text(&out_buf), stderr: text(&err_buf), timed_out })
    }

    fn run_attached(&self, spec: &ProcessSpec) -> io::Result<i32> {
        let mut cmd = command(spec);
        if spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn()?;
        if let (Some(data), Some(mut stdin)) = (&spec.stdin, child.stdin.take()) {
            let _ = stdin.write_all(data);
        }
        Ok(child.wait()?.code().unwrap_or(-1))
    }

    fn run_in_new_window(&self, spec: &ProcessSpec) -> io::Result<i32> {
        if !cfg!(windows) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "a separate window is only offered on Windows; run `agm login` in a terminal"));
        }
        let mut cmd = command(spec);
        creation_flags(&mut cmd, CREATE_NEW_CONSOLE);
        if spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn()?;
        if let (Some(data), Some(mut stdin)) = (&spec.stdin, child.stdin.take()) {
            let _ = stdin.write_all(data);
        }
        Ok(child.wait()?.code().unwrap_or(-1))
    }
}

/// Finds a program that can actually be *started*. On Windows that means `.exe`/`.com`/`.cmd`/`.bat` - not the extension-less
/// shell script or `.ps1` an npm install also leaves next to it (`Env::which` would return those first). Never runs it.
pub fn resolve_executable(env: &Env, names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".com", ".cmd", ".bat"] } else { &[""] };
    env.path_dirs
        .iter()
        .chain(extra_dirs.iter())
        .flat_map(|dir| names.iter().flat_map(move |n| exts.iter().map(move |e| dir.join(format!("{n}{e}")))))
        .find(|p| p.is_file())
}

/// Is `inner` strictly inside `outer`? Both are resolved first, so `..` and symlinks cannot fool it. Used before anything
/// is deleted: only ever a profile folder under our own `accounts/` folder.
pub fn is_inside(inner: &Path, outer: &Path) -> bool {
    match (inner.canonicalize(), outer.canonicalize()) {
        (Ok(i), Ok(o)) => i != o && i.starts_with(&o),
        _ => false,
    }
}

/// A runner that records what it was asked to do and answers from a script. For tests (here and in other crates).
#[cfg(any(test, feature = "testing"))]
pub mod fake {
    use super::*;

    /// One scripted answer for a call, matched by the whole argument list joined with spaces (e.g. "login status").
    #[derive(Clone)]
    pub struct Reply {
        pub when_args: &'static str,
        pub output: StatusOutput,
        pub attached_code: i32,
        /// A file to create when this call runs (simulates the agent writing its own sign-in): the path is the value of
        /// the environment variable named first, joined with the relative path named second.
        pub create_under_env: Option<(&'static str, &'static str)>,
    }

    impl Reply {
        /// A sign-in style call that runs on the terminal and exits with `code`.
        pub fn attached(when_args: &'static str, code: i32) -> Reply {
            Reply { when_args, output: StatusOutput::default(), attached_code: code, create_under_env: None }
        }

        /// A status-style call that prints something and exits with `code`.
        pub fn status(when_args: &'static str, code: i32, stdout: &str, stderr: &str) -> Reply {
            Reply {
                when_args,
                output: StatusOutput { exit_code: Some(code), stdout: stdout.into(), stderr: stderr.into(), timed_out: false },
                attached_code: code,
                create_under_env: None,
            }
        }

        /// Also writes `rel` under the folder named by environment variable `var` (the agent saving its own sign-in).
        pub fn writing(mut self, var: &'static str, rel: &'static str) -> Reply {
            self.create_under_env = Some((var, rel));
            self
        }
    }

    #[derive(Default)]
    pub struct FakeRunner {
        pub replies: Mutex<Vec<Reply>>,
        pub calls: Mutex<Vec<ProcessSpec>>,
    }

    impl FakeRunner {
        pub fn new() -> Arc<FakeRunner> {
            Arc::new(FakeRunner::default())
        }

        pub fn script(&self, reply: Reply) {
            self.replies.lock().unwrap().push(reply);
        }

        pub fn calls(&self) -> Vec<ProcessSpec> {
            self.calls.lock().unwrap().clone()
        }

        fn answer(&self, spec: &ProcessSpec) -> Option<Reply> {
            self.calls.lock().unwrap().push(spec.clone());
            let joined = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
            let found = self.replies.lock().unwrap().iter().rev().find(|r| r.when_args == joined).cloned();
            if let Some(Reply { create_under_env: Some((var, rel)), .. }) = &found {
                if let Some((_, base)) = spec.env.iter().find(|(k, _)| k == *var) {
                    let path = Path::new(base).join(rel);
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(path, b"stand-in for the agent's own sign-in file");
                }
            }
            found
        }
    }

    impl ProcessRunner for FakeRunner {
        fn capture(&self, spec: &ProcessSpec, _timeout: Duration) -> io::Result<StatusOutput> {
            self.answer(spec).map(|r| r.output).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no scripted reply"))
        }

        fn run_attached(&self, spec: &ProcessSpec) -> io::Result<i32> {
            self.answer(spec).map(|r| r.attached_code).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no scripted reply"))
        }

        fn run_in_new_window(&self, spec: &ProcessSpec) -> io::Result<i32> {
            self.run_attached(spec)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_shows_environment_values_or_the_stdin_payload() {
        let spec = ProcessSpec {
            program: "agent".into(),
            args: vec!["login".into()],
            env: vec![("API_KEY".into(), "sk-super-secret".into())],
            stdin: Some(b"sk-also-secret".to_vec()),
            ..Default::default()
        };
        let shown = format!("{spec:?}");
        assert!(shown.contains("API_KEY"), "the variable *name* is useful for diagnosis");
        assert!(!shown.contains("sk-super-secret") && !shown.contains("sk-also-secret"), "{shown}");
        let out = StatusOutput { stdout: "Logged in as someone@example.com".into(), ..Default::default() };
        assert!(!format!("{out:?}").contains("someone@example.com"));
    }

    #[test]
    fn a_runnable_program_is_found_not_the_script_next_to_it() {
        let d = tempfile::tempdir().unwrap();
        let (win, other) = (cfg!(windows), cfg!(not(windows)));
        for name in ["codex", "codex.ps1", "codex.cmd"] {
            std::fs::write(d.path().join(name), b"").unwrap();
        }
        let mut env = Env::with_home(d.path());
        env.path_dirs = vec![d.path().to_path_buf()];
        let found = resolve_executable(&env, &["codex"], &[]).unwrap();
        if win {
            assert_eq!(found.file_name().unwrap(), "codex.cmd", "not the shell script or the .ps1");
        }
        if other {
            assert_eq!(found.file_name().unwrap(), "codex");
        }
        assert!(resolve_executable(&env, &["missing"], &[]).is_none());
        assert!(resolve_executable(&Env::with_home(d.path()), &["codex"], &[d.path().to_path_buf()]).is_some(), "extra directories are searched too");
    }

    #[test]
    fn deletion_is_only_allowed_strictly_inside_the_accounts_folder() {
        let d = tempfile::tempdir().unwrap();
        let accounts = d.path().join("accounts");
        let profile = accounts.join("codex").join("work");
        std::fs::create_dir_all(&profile).unwrap();
        assert!(is_inside(&profile, &accounts));
        assert!(!is_inside(&accounts, &accounts), "the folder itself is not 'inside' itself");
        assert!(!is_inside(d.path(), &accounts), "a parent is not inside");
        let sneaky = profile.join("..").join("..").join("..");
        assert!(!is_inside(&sneaky, &accounts), "`..` is resolved before the check");
        assert!(!is_inside(&d.path().join("nope"), &accounts), "a path that does not exist is refused");
    }

    #[cfg(windows)]
    #[test]
    fn a_real_process_is_run_quietly_and_its_output_captured() {
        let spec = ProcessSpec { program: "cmd".into(), args: vec!["/c".into(), "echo hello && echo oops 1>&2 && exit 3".into()], ..Default::default() };
        let out = SystemRunner.capture(&spec, Duration::from_secs(10)).unwrap();
        assert_eq!(out.exit_code, Some(3));
        assert!(out.stdout.contains("hello") && out.stderr.contains("oops"), "{out:?}");
        assert!(!out.timed_out);
    }

    #[cfg(windows)]
    #[test]
    fn a_process_that_never_ends_is_killed_at_the_timeout() {
        let spec = ProcessSpec { program: "cmd".into(), args: vec!["/c".into(), "ping -n 30 127.0.0.1 > nul".into()], ..Default::default() };
        let started = Instant::now();
        let out = SystemRunner.capture(&spec, Duration::from_millis(400)).unwrap();
        assert!(out.timed_out && out.exit_code.is_none());
        assert!(started.elapsed() < Duration::from_secs(10), "it did not wait for the whole command");
    }

    #[cfg(windows)]
    #[test]
    fn stdin_reaches_the_child_and_the_environment_is_set_and_removed() {
        // `findstr` echoes what it reads on stdin; `set` shows what the child sees.
        let spec = ProcessSpec { program: "findstr".into(), args: vec![".".into()], stdin: Some(b"from-stdin\r\n".to_vec()), ..Default::default() };
        assert!(SystemRunner.capture(&spec, Duration::from_secs(10)).unwrap().stdout.contains("from-stdin"));
        let spec = ProcessSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), "echo [%AGM_TEST_SET%][%AGM_TEST_GONE%]".into()],
            env: vec![("AGM_TEST_SET".into(), "yes".into())],
            env_remove: vec!["AGM_TEST_GONE".into()],
            ..Default::default()
        };
        std::env::set_var("AGM_TEST_GONE", "should-not-leak");
        let out = SystemRunner.capture(&spec, Duration::from_secs(10)).unwrap();
        assert!(out.stdout.contains("[yes]") && !out.stdout.contains("should-not-leak"), "{out:?}");
    }
}
