//! Bounded directory listing for *known* agent data folders. Never follows symlinks, never recurses
//! deeper than asked, and silently skips what it cannot read (permissions, files vanishing mid-scan).

use std::fs::Metadata;
use std::path::{Path, PathBuf};

/// Regular files under `root` whose name ends with `suffix` (case-insensitive), at most `max_depth`
/// directory levels down (`root` itself is depth 0). Sorted for deterministic ingestion order.
pub fn files_with_suffix(root: &Path, suffix: &str, max_depth: usize) -> Vec<(PathBuf, Metadata)> {
    let suffix = suffix.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                if depth < max_depth {
                    stack.push((path, depth + 1));
                }
            } else if ft.is_file() && path.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(&suffix)) {
                if let Ok(meta) = entry.metadata() {
                    out.push((path, meta));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Immediate subdirectories of `root`.
pub fn subdirs(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_matching_files_within_the_depth_limit_only() {
        let d = tempfile::tempdir().unwrap();
        let deep = d.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(d.path().join("root.jsonl"), "").unwrap();
        std::fs::write(d.path().join("a").join("one.JSONL"), "").unwrap();
        std::fs::write(d.path().join("a").join("b").join("two.jsonl"), "").unwrap();
        std::fs::write(deep.join("three.jsonl"), "").unwrap();
        std::fs::write(d.path().join("a").join("skip.txt"), "").unwrap();

        let names = |depth| -> Vec<String> {
            files_with_suffix(d.path(), ".jsonl", depth).iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
        };
        assert_eq!(names(0), vec!["root.jsonl"]);
        assert_eq!(names(2).len(), 3);
        assert_eq!(names(3).len(), 4);
    }

    #[test]
    fn a_missing_root_is_empty_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(files_with_suffix(&d.path().join("nope"), ".jsonl", 3).is_empty());
        assert!(subdirs(&d.path().join("nope")).is_empty());
    }
}
