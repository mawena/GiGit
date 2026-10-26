//! Debounced file-system watcher for an open repository.

use std::path::Path;
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchGuard(#[allow(dead_code)] Debouncer<RecommendedWatcher>);

pub const DEBOUNCE: Duration = Duration::from_millis(400);

/// Entries of `.git` whose change alters what GiGit shows.
const GIT_STATE: &[&str] = &[
    "HEAD",
    "index",
    "refs",
    "packed-refs",
    "ORIG_HEAD",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
];

/// True when a changed path is worth a refresh: any work-tree file, but inside `.git` only the
/// entries that hold state (objects, logs, lock files and the like are noise).
pub fn relevant(repo: &Path, changed: &Path) -> bool {
    // Directory events (e.g. the watcher opening them) are noise: real changes name a file.
    if changed.extension().is_some_and(|e| e == "lock") || changed.is_dir() {
        return false;
    }
    match changed.strip_prefix(repo.join(".git")) {
        Ok(inner) => inner
            .components()
            .next()
            .is_some_and(|c| GIT_STATE.iter().any(|s| c.as_os_str() == *s)),
        Err(_) => true,
    }
}

pub fn spawn_watcher(repo: &Path, on_change: impl Fn() + Send + 'static) -> notify::Result<WatchGuard> {
    let root = repo.to_path_buf();
    let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        if let Ok(events) = res
            && events.iter().any(|e| relevant(&root, &e.path))
        {
            on_change();
        }
    })?;
    debouncer.watcher().watch(repo, RecursiveMode::Recursive)?;
    Ok(WatchGuard(debouncer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;
    use std::sync::mpsc::channel;

    #[test]
    fn relevant_filters_git_internals() {
        let r = Path::new("/r");
        assert!(relevant(r, Path::new("/r/src/main.rs")));
        assert!(relevant(r, Path::new("/r/.git/HEAD")));
        assert!(relevant(r, Path::new("/r/.git/refs/heads/main")));
        assert!(!relevant(r, Path::new("/r/.git/objects/ab/cdef")));
        assert!(!relevant(r, Path::new("/r/.git")));
        assert!(!relevant(r, Path::new("/r/.git/info")));
        assert!(!relevant(r, Path::new("/r/.git/logs/HEAD")));
        assert!(relevant(r, Path::new("/r/.git/index")));
        assert!(relevant(r, Path::new("/r/.git/MERGE_HEAD")));
        assert!(relevant(r, Path::new("/r/.git/rebase-merge/done")));
        assert!(!relevant(r, Path::new("/r/.git/index.lock")));
        assert!(!relevant(r, Path::new("/r/.git/refs/heads/main.lock")));
    }

    #[test]
    fn existing_directories_are_not_relevant() {
        let (_d, repo) = temp_repo();
        std::fs::create_dir(repo.join("sub")).unwrap();
        assert!(!relevant(&repo, &repo.join("sub")));
        assert!(!relevant(&repo, &repo.join(".git/refs/heads")));
        assert!(relevant(&repo, &repo.join("deleted-dir")));
    }

    #[test]
    fn watcher_fires_on_file_write() {
        let (_d, repo) = temp_repo();
        let (tx, rx) = channel();
        let _g = spawn_watcher(&repo, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        write(&repo, "x.txt", "hi");
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn watcher_ignores_object_writes() {
        let (_d, repo) = temp_repo();
        let (tx, rx) = channel();
        let _g = spawn_watcher(&repo, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        write(&repo, ".git/objects/ab/cdef", "x");
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    }
}
