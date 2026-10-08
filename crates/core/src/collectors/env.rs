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
    fn xdg_data_defaults_under_home() {
        let env = Env::with_home("/h");
        assert_eq!(env.xdg_data(), PathBuf::from("/h").join(".local").join("share"));
        let mut env = env;
        env.xdg_data_home = Some("/custom".into());
        assert_eq!(env.xdg_data(), PathBuf::from("/custom"));
    }
}
