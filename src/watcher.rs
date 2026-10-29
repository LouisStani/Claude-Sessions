//! Wakes the session list up when Claude Code or a mailbox writes something, instead of
//! polling `claude agents --json` (about 0.55 s of CPU per account and call).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// What to watch in one account directory (`CLAUDE_CONFIG_DIR`):
/// `sessions/` gets a file per running session, rewritten when its state changes;
/// `daemon/` holds the roster of background sessions; `jobs/<id>/` their state.
/// `.claude.json` is left out on purpose: Claude Code rewrites it all the time.
pub fn targets(account_dirs: &[PathBuf], mailbox_root: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let mut targets: Vec<(PathBuf, RecursiveMode)> = account_dirs
        .iter()
        .flat_map(|dir| {
            [
                (dir.join("sessions"), RecursiveMode::NonRecursive),
                (dir.join("daemon"), RecursiveMode::NonRecursive),
                (dir.join("jobs"), RecursiveMode::Recursive),
            ]
        })
        .collect();
    targets.push((mailbox_root.to_path_buf(), RecursiveMode::Recursive));
    targets
}

pub struct ChangeWatcher {
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    changes: Receiver<()>,
}

pub enum Wake {
    Changed,
    TimedOut,
    Closed,
}

impl ChangeWatcher {
    pub fn new() -> notify::Result<Self> {
        let (sender, changes) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            // Reads (open/close events) are ignored: `claude agents --json` reads these files.
            let wrote = result.is_ok_and(|event| event.kind.is_create() || event.kind.is_modify() || event.kind.is_remove());
            if wrote {
                // The receiver only disappears when the refresher stops.
                let _ = sender.send(());
            }
        })?;
        Ok(Self { watcher, watched: HashSet::new(), changes })
    }

    /// Starts watching the targets not watched yet. Missing directories are skipped
    /// silently (an account that never ran a background session has no `jobs/`);
    /// other failures are returned.
    pub fn ensure(&mut self, targets: &[(PathBuf, RecursiveMode)]) -> Vec<String> {
        let mut errors = Vec::new();
        for (path, mode) in targets {
            if self.watched.contains(path) || !path.is_dir() {
                continue;
            }
            match self.watcher.watch(path, *mode) {
                Ok(()) => {
                    self.watched.insert(path.clone());
                },
                Err(error) => errors.push(format!("cannot watch {}: {error}", path.display())),
            }
        }
        errors
    }

    /// Waits for a change, then lets a burst of writes settle so it counts as one.
    pub fn wait(&self, fallback: Duration, settle: Duration) -> Wake {
        match self.changes.recv_timeout(fallback) {
            Ok(()) => {},
            Err(RecvTimeoutError::Timeout) => return Wake::TimedOut,
            Err(RecvTimeoutError::Disconnected) => return Wake::Closed,
        }
        while self.changes.recv_timeout(settle).is_ok() {}
        Wake::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    #[test]
    fn targets_cover_each_account_and_the_mailbox() {
        let dirs = [PathBuf::from("/a/louis"), PathBuf::from("/a/pro")];
        let targets = targets(&dirs, Path::new("/m"));
        assert_eq!(targets.len(), 7);
        assert!(targets.contains(&(PathBuf::from("/a/pro/jobs"), RecursiveMode::Recursive)));
        assert!(targets.contains(&(PathBuf::from("/a/louis/sessions"), RecursiveMode::NonRecursive)));
        assert_eq!(targets.last(), Some(&(PathBuf::from("/m"), RecursiveMode::Recursive)));
        assert!(!targets.iter().any(|(path, _)| path.ends_with(".claude.json")));
    }

    #[test]
    fn a_write_wakes_up_and_a_burst_counts_once() {
        let root = std::env::temp_dir().join(format!("claude-sessions-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("jobs/abc")).unwrap();
        let mut watcher = ChangeWatcher::new().unwrap();
        let errors = watcher.ensure(&[
            (root.join("jobs"), RecursiveMode::Recursive),
            (root.join("missing"), RecursiveMode::NonRecursive),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
        for index in 0..5 {
            fs::write(root.join(format!("jobs/abc/state-{index}.json")), b"{}").unwrap();
        }
        let started = Instant::now();
        assert!(matches!(watcher.wait(Duration::from_secs(5), Duration::from_millis(200)), Wake::Changed));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(matches!(watcher.wait(Duration::from_millis(300), Duration::from_millis(50)), Wake::TimedOut));
        fs::remove_dir_all(root).unwrap();
    }

    /// `claude agents --json` reads the watched files: reading must not wake us up,
    /// or every refresh triggers the next one.
    #[test]
    fn reading_a_watched_file_does_not_wake_up() {
        let root = std::env::temp_dir().join(format!("claude-sessions-watch-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("roster.json"), b"{}").unwrap();
        let mut watcher = ChangeWatcher::new().unwrap();
        assert!(watcher.ensure(&[(root.clone(), RecursiveMode::Recursive)]).is_empty());
        fs::read(root.join("roster.json")).unwrap();
        fs::read_dir(&root).unwrap().for_each(drop);
        assert!(matches!(watcher.wait(Duration::from_millis(500), Duration::from_millis(50)), Wake::TimedOut));
        fs::remove_dir_all(root).unwrap();
    }
}
