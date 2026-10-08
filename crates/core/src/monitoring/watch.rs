//! Filesystem watching for the *verified* directories a collector reported — never a home-wide scan.
//! Events only nudge the worker (`Wake::Dirty`); what actually changed is decided by comparing
//! file size/mtime against saved cursors, so a missed or duplicated event can never corrupt counts.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::Wake;
use crate::collectors::WatchSpec;

pub struct Watcher {
    tx: Sender<Wake>,
    inner: Option<RecommendedWatcher>,
    watching: Vec<(PathBuf, bool)>,
}

impl Watcher {
    pub fn new(tx: Sender<Wake>) -> Watcher {
        Watcher { tx, inner: None, watching: Vec::new() }
    }

    /// (Re)configure only when the set of watched roots actually changed.
    pub fn sync(&mut self, specs: Vec<WatchSpec>) {
        let key: Vec<(PathBuf, bool)> = specs.iter().map(|s| (s.path.clone(), s.recursive)).collect();
        if key == self.watching && (self.inner.is_some() || key.is_empty()) {
            return;
        }
        self.inner = None;
        self.watching = key;
        if specs.is_empty() {
            return;
        }

        let tx = self.tx.clone();
        let filters = specs.clone();
        let handler = move |res: notify::Result<notify::Event>| match res {
            Ok(ev) => {
                if !matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any) {
                    return;
                }
                if ev.paths.iter().any(|p| is_relevant(&filters, p)) {
                    let _ = tx.send(Wake::Dirty);
                }
            }
            // Watcher errors (e.g. an OS buffer overflow) mean we may have missed events: rescan.
            Err(_) => {
                let _ = tx.send(Wake::Dirty);
            }
        };

        match notify::recommended_watcher(handler) {
            Ok(mut w) => {
                for s in &specs {
                    let mode = if s.recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
                    if let Err(e) = w.watch(&s.path, mode) {
                        tracing::warn!(path = %s.path.display(), "cannot watch (falling back to polling): {e}");
                    }
                }
                self.inner = Some(w);
            }
            Err(e) => tracing::warn!("file watching unavailable (polling only): {e}"),
        }
    }
}

fn is_relevant(specs: &[WatchSpec], path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    specs.iter().any(|s| {
        path.starts_with(&s.path) && (s.suffixes.is_empty() || s.suffixes.iter().any(|suf| lower.ends_with(&suf.to_ascii_lowercase())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(path: &str, suffixes: &'static [&'static str]) -> WatchSpec {
        WatchSpec { path: PathBuf::from(path), recursive: true, suffixes }
    }

    #[test]
    fn suffix_filters_keep_unrelated_files_from_waking_us() {
        let specs = [spec("/data", &[".jsonl"])];
        assert!(is_relevant(&specs, Path::new("/data/a/b.JSONL")));
        assert!(!is_relevant(&specs, Path::new("/data/a/b.txt")));
        assert!(!is_relevant(&specs, Path::new("/other/b.jsonl")));
    }

    #[test]
    fn empty_suffixes_match_anything_under_the_root() {
        let specs = [spec("/data", &[])];
        assert!(is_relevant(&specs, Path::new("/data/x/y.bin")));
    }

    #[test]
    fn sqlite_wal_sidecars_can_be_matched() {
        let specs = [spec("/c", &[".db", ".db-wal"])];
        assert!(is_relevant(&specs, Path::new("/c/1.db-wal")));
    }
}
