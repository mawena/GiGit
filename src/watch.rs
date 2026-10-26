//! Debounced file-system watcher for an open repository.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::git::{RunOpts, run};

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchGuard(#[allow(dead_code)] RecommendedWatcher);

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

/// True when a batch of changed paths contains something GiGit shows: a git state change, or a
/// work-tree file that is not ignored by `.gitignore` (build output must not refresh constantly).
fn worth_refresh<'a>(repo: &Path, changed: impl Iterator<Item = &'a Path>) -> bool {
    let git_dir = repo.join(".git");
    let mut files: Vec<u8> = Vec::new();
    let mut count = 0;
    for p in changed.filter(|p| relevant(repo, p)) {
        if p.starts_with(&git_dir) {
            return true;
        }
        if let Ok(rel) = p.strip_prefix(repo) {
            files.extend_from_slice(rel.as_os_str().as_encoded_bytes());
            files.push(0);
            count += 1;
        }
    }
    if count == 0 {
        return false;
    }
    // check-ignore rejects literal pathspecs; its paths come on stdin and are never globbed.
    let opts =
        RunOpts { background: true, read_only: true, allow_exit_1: true, stdin: Some(files), pathspec_magic: true };
    match run(repo, &["check-ignore", "--stdin", "-z"], &opts) {
        Ok(out) => out.stdout.iter().filter(|b| **b == 0).count() < count,
        Err(_) => true,
    }
}

/// Reads (open, access, close without writing) are noise: GiGit's own git calls make them.
fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

pub fn spawn_watcher(repo: &Path, on_change: impl Fn() + Send + 'static) -> notify::Result<WatchGuard> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && is_change(&ev.kind)
        {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })?;
    watcher.watch(repo, RecursiveMode::Recursive)?;
    let root = repo.to_path_buf();
    // Debounce: wait for a quiet period, then decide once for the whole batch.
    std::thread::Builder::new().name("gigit-watch".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            let deadline = Instant::now() + DEBOUNCE;
            let mut closed = false;
            loop {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(p) => batch.push(p),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
            batch.sort();
            batch.dedup();
            if worth_refresh(&root, batch.iter().map(PathBuf::as_path)) {
                on_change();
            }
            if closed {
                return;
            }
        }
    })?;
    Ok(WatchGuard(watcher))
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

    #[test]
    fn watcher_ignores_gitignored_files() {
        let (_d, repo) = temp_repo();
        write(&repo, ".gitignore", "target/\n");
        commit_all(&repo, "ignore");
        std::fs::create_dir(repo.join("target")).unwrap();
        let (tx, rx) = channel();
        let _g = spawn_watcher(&repo, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        write(&repo, "target/build.o", "x");
        assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err(), "ignored file triggered a refresh");
        write(&repo, "src.txt", "y");
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn reading_the_repo_does_not_trigger_a_refresh() {
        // GiGit's own refresh reads .git/index, HEAD and refs: that must not loop.
        let (_d, repo) = temp_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "one");
        let (tx, rx) = channel();
        let _g = spawn_watcher(&repo, move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        crate::git::status::status(&repo).unwrap();
        crate::git::log::refs(&repo).unwrap();
        crate::git::log::log(&repo, 0, 10).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err(), "a read triggered a refresh");
    }
}
