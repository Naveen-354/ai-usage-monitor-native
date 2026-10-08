//! Maps a working directory to the project (repository) it belongs to.
//!
//! Only `stat` calls on the directory's own ancestors — no scanning. Results are cached per
//! directory. The walk stops before the home directory so a dotfiles repo in `~` never swallows
//! every project beneath it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::model::ProjectRef;

const MAX_DEPTH: usize = 16;
const MAX_CACHE: usize = 4096;

pub struct ProjectResolver {
    enabled: bool,
    home: PathBuf,
    cache: Mutex<HashMap<String, Option<ProjectRef>>>,
}

/// `\\?\C:\x` → `C:\x`. (Canonicalisation on Windows returns verbatim paths.)
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() > 2 && rest.as_bytes()[1] == b':' => PathBuf::from(rest),
        _ => p,
    }
}

impl ProjectResolver {
    pub fn new(enabled: bool, home: PathBuf) -> Self {
        ProjectResolver { enabled, home: strip_verbatim(std::fs::canonicalize(&home).unwrap_or(home)), cache: Mutex::new(HashMap::new()) }
    }

    pub fn resolve(&self, cwd: &str) -> Option<ProjectRef> {
        if !self.enabled || cwd.trim().is_empty() {
            return None;
        }
        if let Some(hit) = self.cache.lock().unwrap_or_else(|p| p.into_inner()).get(cwd) {
            return hit.clone();
        }
        let resolved = self.compute(cwd);
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() >= MAX_CACHE {
            cache.clear();
        }
        cache.insert(cwd.to_string(), resolved.clone());
        resolved
    }

    fn compute(&self, cwd: &str) -> Option<ProjectRef> {
        // Prefer the filesystem's own spelling (true case, no `..`) when the folder still exists.
        let path = std::fs::canonicalize(cwd).map(strip_verbatim).unwrap_or_else(|_| PathBuf::from(cwd));
        if path.parent().is_none() || same_path(&path, &self.home) {
            return None; // a drive root or the home folder is not a project
        }

        let mut root: &Path = &path;
        for dir in path.ancestors().take(MAX_DEPTH) {
            if same_path(dir, &self.home) || dir.parent().is_none() {
                break;
            }
            if dir.join(".git").exists() {
                root = dir;
                break;
            }
        }
        let name = root.file_name()?.to_string_lossy().into_owned();
        Some(ProjectRef { root_path: root.to_string_lossy().into_owned(), name })
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
    if cfg!(windows) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver(enabled: bool, home: &Path) -> ProjectResolver {
        ProjectResolver::new(enabled, home.to_path_buf())
    }

    #[test]
    fn a_subfolder_resolves_to_its_git_root() {
        let d = tempfile::tempdir().unwrap();
        let repo = d.path().join("work").join("robot-simulator");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src").join("deep")).unwrap();
        let r = resolver(true, &d.path().join("home"));
        let p = r.resolve(&repo.join("src").join("deep").to_string_lossy()).unwrap();
        assert_eq!(p.name, "robot-simulator");
        assert!(same_path(Path::new(&p.root_path), &std::fs::canonicalize(&repo).map(strip_verbatim).unwrap()));
    }

    #[test]
    fn without_git_the_folder_itself_is_the_project() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("scratch-notes");
        std::fs::create_dir_all(&dir).unwrap();
        let p = resolver(true, &d.path().join("home")).resolve(&dir.to_string_lossy()).unwrap();
        assert_eq!(p.name, "scratch-notes");
    }

    #[test]
    fn a_git_worktree_file_marks_a_root_too() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("feature-wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/x").unwrap();
        assert_eq!(resolver(true, &d.path().join("home")).resolve(&wt.to_string_lossy()).unwrap().name, "feature-wt");
    }

    #[test]
    fn the_home_folder_is_not_a_project_and_a_repo_in_home_does_not_swallow_children() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("me");
        std::fs::create_dir_all(home.join(".git")).unwrap(); // dotfiles repo
        let child = home.join("play");
        std::fs::create_dir_all(&child).unwrap();
        let r = resolver(true, &home);
        assert!(r.resolve(&home.to_string_lossy()).is_none());
        assert_eq!(r.resolve(&child.to_string_lossy()).unwrap().name, "play");
    }

    #[test]
    fn disabled_or_empty_input_yields_unknown() {
        let d = tempfile::tempdir().unwrap();
        assert!(resolver(false, d.path()).resolve(&d.path().to_string_lossy()).is_none());
        assert!(resolver(true, d.path()).resolve("   ").is_none());
    }

    #[test]
    fn a_vanished_folder_still_gets_a_name_from_its_path() {
        let d = tempfile::tempdir().unwrap();
        let gone = d.path().join("deleted-project");
        assert_eq!(resolver(true, &d.path().join("home")).resolve(&gone.to_string_lossy()).unwrap().name, "deleted-project");
    }

    #[test]
    fn results_are_cached() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("cached");
        std::fs::create_dir_all(&dir).unwrap();
        let r = resolver(true, &d.path().join("home"));
        let a = r.resolve(&dir.to_string_lossy());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r.resolve(&dir.to_string_lossy()), a);
    }

    #[test]
    fn strips_the_windows_verbatim_prefix() {
        assert_eq!(strip_verbatim(PathBuf::from(r"\\?\C:\Users\x")), PathBuf::from(r"C:\Users\x"));
        assert_eq!(strip_verbatim(PathBuf::from(r"\\?\UNC\server\share")), PathBuf::from(r"\\?\UNC\server\share"));
        assert_eq!(strip_verbatim(PathBuf::from("/home/x")), PathBuf::from("/home/x"));
    }
}
