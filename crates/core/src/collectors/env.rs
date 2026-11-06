//! The slice of the machine a collector may look at. Tests build one pointing at a temp directory,
//! so no collector ever has to touch the real home folder under test.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Env {
    pub home: PathBuf,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_app_data: Option<PathBuf>,
    /// `%APPDATA%` (Windows).
    pub app_data: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    /// Directories on `PATH`.
    pub path_dirs: Vec<PathBuf>,
}

impl Env {
    pub fn from_system() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Env {
            home: dirs::home_dir().unwrap_or_default(),
            local_app_data: var("LOCALAPPDATA"),
            app_data: var("APPDATA"),
            xdg_data_home: var("XDG_DATA_HOME"),
            path_dirs: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
        }
    }

    /// The machine as it is *now*: like [`Env::from_system`], plus every directory on the system and user `PATH` that Windows
    /// stores in the registry. A running program keeps the `PATH` it was started with, so a CLI installed since then is on the
    /// registry's `PATH` and not on its own; without this a "reload" could never find it. (Elsewhere this is `from_system`.)
    pub fn reloaded() -> Env {
        let mut env = Env::from_system();
        env.path_dirs = merge_dirs(env.path_dirs, system_path_dirs());
        env
    }

    /// An environment rooted at `home` with nothing on PATH (tests).
    pub fn with_home(home: impl Into<PathBuf>) -> Env {
        Env { home: home.into(), ..Env::default() }
    }

    pub fn home_join(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    /// `~/.local/share` (or `$XDG_DATA_HOME`), the base several CLI tools use on every OS.
    pub fn xdg_data(&self) -> PathBuf {
        self.xdg_data_home.clone().unwrap_or_else(|| self.home.join(".local").join("share"))
    }

    /// Find an executable by name on PATH plus `extra` directories. Never runs it.
    pub fn which(&self, names: &[&str], extra: &[PathBuf]) -> Option<PathBuf> {
        let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat", ".ps1"] } else { &[""] };
        self.path_dirs
            .iter()
            .chain(extra.iter())
            .flat_map(|dir| names.iter().flat_map(move |n| exts.iter().map(move |e| dir.join(format!("{n}{e}")))))
            .find(|p| p.is_file())
    }
}

/// `base` followed by the directories of `extra` that it does not have yet (compared without regard to case on Windows or a
/// trailing slash), so the program's own `PATH` order is kept and nothing is searched twice.
pub fn merge_dirs(mut base: Vec<PathBuf>, extra: Vec<PathBuf>) -> Vec<PathBuf> {
    let key = |p: &Path| {
        let s = path_str(p);
        let s = s.trim_end_matches(['\\', '/']);
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let mut seen: std::collections::HashSet<String> = base.iter().map(|p| key(p)).collect();
    for p in extra {
        if !p.as_os_str().is_empty() && seen.insert(key(&p)) {
            base.push(p);
        }
    }
    base
}

/// The directories on the machine's and the user's `PATH` as the registry holds them now (machine first, as Windows builds a
/// new process's `PATH`). Empty when they cannot be read.
#[cfg(windows)]
fn system_path_dirs() -> Vec<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut dirs = registry_path(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment");
    dirs.extend(registry_path(HKEY_CURRENT_USER, "Environment"));
    dirs
}

#[cfg(not(windows))]
fn system_path_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// The `Path` value under `root\subkey`, with `%VARIABLES%` expanded by Windows, split into directories.
#[cfg(windows)]
fn registry_path(root: windows_sys::Win32::System::Registry::HKEY, subkey: &str) -> Vec<PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};

    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (sub, name) = (wide(subkey), wide("Path"));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;

    // SAFETY: `sub` and `name` are NUL-terminated and outlive both calls; the first call passes no buffer and only asks for
    // the size, the second passes a buffer of exactly the size it was given, and `got` is updated to what was written.
    let mut bytes: u32 = 0;
    let err = unsafe { RegGetValueW(root, sub.as_ptr(), name.as_ptr(), flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut bytes) };
    if err != 0 || bytes < 2 {
        return Vec::new();
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    let mut got = (buf.len() * 2) as u32;
    let err = unsafe { RegGetValueW(root, sub.as_ptr(), name.as_ptr(), flags, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut got) };
    if err != 0 {
        return Vec::new();
    }
    let len = (got as usize / 2).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    std::env::split_paths(text.trim_end_matches('\0')).filter(|p| !p.as_os_str().is_empty()).collect()
}

/// Existing directories among `candidates`.
pub fn existing_dirs(candidates: &[PathBuf]) -> Vec<PathBuf> {
    candidates.iter().filter(|p| p.is_dir()).cloned().collect()
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_executables_on_path_and_in_extra_dirs() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        let extra = d.path().join("extra");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&extra).unwrap();
        let name = if cfg!(windows) { "tool.exe" } else { "tool" };
        std::fs::write(bin.join(name), b"").unwrap();
        std::fs::write(extra.join(if cfg!(windows) { "other.cmd" } else { "other" }), b"").unwrap();

        let mut env = Env::with_home(d.path());
        env.path_dirs = vec![bin.clone()];
        assert_eq!(env.which(&["tool"], &[]), Some(bin.join(name)));
        assert!(env.which(&["other"], &[]).is_none());
        assert!(env.which(&["other"], std::slice::from_ref(&extra)).is_some());
        assert!(env.which(&["missing"], &[extra]).is_none());
    }

    #[test]
    fn merging_keeps_the_programs_own_order_and_adds_only_what_is_new() {
        let p = |s: &str| PathBuf::from(s);
        let merged = merge_dirs(vec![p("/a"), p("/b")], vec![p("/b/"), p("/c"), p("/a"), p(""), p("/c/"), p("/d")]);
        assert_eq!(merged, vec![p("/a"), p("/b"), p("/c"), p("/d")], "no repeats, no empties, own order first");
    }

    #[cfg(windows)]
    #[test]
    fn merging_ignores_case_and_trailing_slashes_on_windows() {
        let p = |s: &str| PathBuf::from(s);
        let merged = merge_dirs(vec![p(r"C:\Windows\System32")], vec![p(r"c:\windows\system32\"), p(r"C:\Tools")]);
        assert_eq!(merged, vec![p(r"C:\Windows\System32"), p(r"C:\Tools")]);
    }

    /// Every Windows machine has System32 on its system PATH, and reading it needs no particular installation.
    #[cfg(windows)]
    #[test]
    fn the_registry_path_is_read_and_expanded() {
        let dirs = system_path_dirs();
        assert!(!dirs.is_empty(), "the system PATH could not be read from the registry");
        assert!(dirs.iter().any(|d| path_str(d).to_lowercase().ends_with(r"\system32")), "System32 is on every Windows PATH: {dirs:?}");
        assert!(dirs.iter().all(|d| !path_str(d).contains('%')), "%VARIABLES% must be expanded: {dirs:?}");
    }

    #[test]
    fn a_reloaded_environment_keeps_everything_the_process_already_had() {
        let before = Env::from_system();
        let after = Env::reloaded();
        assert_eq!(after.home, before.home);
        assert_eq!(&after.path_dirs[..before.path_dirs.len()], &before.path_dirs[..], "the process's own PATH comes first, unchanged");
    }

    #[test]
    fn xdg_data_defaults_under_home() {
        let env = Env::with_home("/h");
        assert_eq!(env.xdg_data(), PathBuf::from("/h").join(".local").join("share"));
        let mut env = env;
        env.xdg_data_home = Some("/custom".into());
        assert_eq!(env.xdg_data(), PathBuf::from("/custom"));
    }
}
