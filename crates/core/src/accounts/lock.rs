//! A lock between *processes* (the desktop app and any number of `agm` commands) for changes to one agent's accounts, so
//! two sign-ins, switches or removals can never interleave and leave a half-written profile behind.
//!
//! It is an advisory OS file lock (`LockFileEx` on Windows, `flock` elsewhere) in the data folder. The OS releases it if the
//! process holding it crashes, so a dead process can never leave an agent locked for good.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

use super::error::{AccountError, AccountResult};

/// How long to wait for another change to finish before giving up.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(15);

/// Held while an agent's accounts are being changed; released when dropped.
#[derive(Debug)]
pub struct AgentLock {
    _file: File,
}

impl AgentLock {
    pub fn acquire(data_dir: &Path, agent: &str, wait: Duration) -> AccountResult<AgentLock> {
        let dir = data_dir.join("locks");
        std::fs::create_dir_all(&dir)?;
        // the agent id is one of our own constants, but never let it become a path
        let safe: String = agent.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{safe}.lock")))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(AgentLock { _file: file }),
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= wait {
                        return Err(AccountError::Busy(agent.to_string()));
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(TryLockError::Error(e)) => return Err(AccountError::Io(e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_second_holder_waits_and_then_gets_busy_if_the_first_never_lets_go() {
        let d = tempfile::tempdir().unwrap();
        let first = AgentLock::acquire(d.path(), "codex", Duration::from_secs(1)).unwrap();
        let err = AgentLock::acquire(d.path(), "codex", Duration::from_millis(150)).unwrap_err();
        assert!(matches!(err, AccountError::Busy(ref a) if a == "codex"), "{err}");
        drop(first);
        AgentLock::acquire(d.path(), "codex", Duration::from_millis(150)).expect("free again once released");
    }

    #[test]
    fn different_agents_never_block_each_other() {
        let d = tempfile::tempdir().unwrap();
        let _codex = AgentLock::acquire(d.path(), "codex", Duration::from_secs(1)).unwrap();
        AgentLock::acquire(d.path(), "claude", Duration::from_millis(100)).expect("an unrelated agent is unaffected");
    }

    #[test]
    fn a_waiter_gets_the_lock_as_soon_as_the_holder_releases_it() {
        let d = tempfile::tempdir().unwrap();
        let first = AgentLock::acquire(d.path(), "codex", Duration::from_secs(1)).unwrap();
        let path = d.path().to_path_buf();
        let waiter = std::thread::spawn(move || AgentLock::acquire(&path, "codex", Duration::from_secs(5)).is_ok());
        std::thread::sleep(Duration::from_millis(200));
        drop(first);
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn the_lock_serialises_critical_sections_across_many_threads() {
        let d = tempfile::tempdir().unwrap();
        let inside = Arc::new(AtomicUsize::new(0));
        let overlaps = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (path, inside, overlaps) = (d.path().to_path_buf(), inside.clone(), overlaps.clone());
                std::thread::spawn(move || {
                    let _g = AgentLock::acquire(&path, "codex", Duration::from_secs(20)).unwrap();
                    if inside.fetch_add(1, Ordering::SeqCst) != 0 {
                        overlaps.fetch_add(1, Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(15));
                    inside.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(overlaps.load(Ordering::SeqCst), 0, "two holders were inside at once");
    }

    #[test]
    fn an_agent_name_cannot_escape_the_lock_folder() {
        let d = tempfile::tempdir().unwrap();
        let _g = AgentLock::acquire(d.path(), "..\\..\\evil", Duration::from_secs(1)).unwrap();
        assert!(d.path().join("locks").join("evil.lock").exists());
        assert!(!d.path().parent().unwrap().join("evil.lock").exists());
    }
}
