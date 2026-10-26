//! One background thread per open repository: runs git jobs serially, never on the UI thread.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use crate::git::{GitError, diff, log, ops, status};
use crate::git::diff::{DiffSide, FileDiff};
use crate::git::log::{Commit, Ref};
use crate::git::patch::LineSel;
use crate::git::status::{FileEntry, Status};
use crate::graph::Layout;

/// Commits loaded per page.
pub const PAGE: usize = 500;

#[derive(Debug, Clone)]
pub enum Job {
    /// Status, refs and the first page of history.
    Refresh,
    LoadMore { skip: usize },
    Diff { path: String, side: DiffSide },
    Show { id: String },
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageLines(FileDiff, LineSel),
    UnstageLines(FileDiff, LineSel),
    DiscardLines(FileDiff, LineSel),
    Discard(Vec<FileEntry>),
    Ignore(String),
    Commit { message: String, amend: bool },
    Fetch { background: bool },
    Pull,
    Push { force: bool },
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub status: Status,
    pub refs: Vec<Ref>,
    pub commits: Vec<Commit>,
    pub layout: Layout,
    pub has_more: bool,
}

#[derive(Debug)]
pub enum JobResult {
    Snapshot(Snapshot),
    More { commits: Vec<Commit> },
    Diff { path: String, side: DiffSide, diff: Option<FileDiff> },
    Show { commit: Commit, files: Vec<FileDiff> },
    Done { job: &'static str },
    Error { job: &'static str, error: GitError, background: bool },
}

pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct RepoWorker {
    pub repo: PathBuf,
    tx: Sender<Job>,
    busy: Arc<AtomicBool>,
}

impl RepoWorker {
    /// Starts the worker thread. It stops when the `RepoWorker` is dropped.
    pub fn spawn(repo: PathBuf, out: Sender<(PathBuf, JobResult)>, wake: Wake) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let busy = Arc::new(AtomicBool::new(false));
        let (r, b) = (repo.clone(), busy.clone());
        std::thread::Builder::new()
            .name("gigit-worker".into())
            .spawn(move || worker_loop(r, rx, out, b, wake))
            .expect("spawn worker thread");
        RepoWorker { repo, tx, busy }
    }

    /// A handle other threads (e.g. the file watcher) can use to queue jobs.
    pub fn sender(&self) -> Sender<Job> {
        self.tx.clone()
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

fn snapshot(repo: &std::path::Path) -> Result<Snapshot, GitError> {
    let status = status::status(repo)?;
    let refs = log::refs(repo)?;
    let commits = log::log(repo, 0, PAGE)?;
    let layout = crate::graph::layout(&commits);
    let has_more = commits.len() == PAGE;
    Ok(Snapshot { status, refs, commits, layout, has_more })
}

/// Runs one job. `Ok(None)` means a mutating job succeeded and a refresh must follow.
fn run_job(repo: &std::path::Path, job: Job) -> Result<Option<JobResult>, GitError> {
    Ok(Some(match job {
        Job::Refresh => JobResult::Snapshot(snapshot(repo)?),
        Job::LoadMore { skip } => JobResult::More { commits: log::log(repo, skip, PAGE)? },
        Job::Diff { path, side } => {
            let diff = diff::diff_file(repo, &path, side)?;
            JobResult::Diff { path, side, diff }
        }
        Job::Show { id } => {
            let (commit, files) = log::show(repo, &id)?;
            JobResult::Show { commit, files }
        }
        Job::Stage(p) => return ops::stage_paths(repo, &p).map(|_| None),
        Job::Unstage(p) => return ops::unstage_paths(repo, &p).map(|_| None),
        Job::StageLines(f, s) => return ops::stage_lines(repo, &f, &s).map(|_| None),
        Job::UnstageLines(f, s) => return ops::unstage_lines(repo, &f, &s).map(|_| None),
        Job::DiscardLines(f, s) => return ops::discard_lines(repo, &f, &s).map(|_| None),
        Job::Discard(e) => return ops::discard_paths(repo, &e).map(|_| None),
        Job::Ignore(p) => return ops::ignore_path(repo, &p).map(|_| None),
        Job::Commit { message, amend } => return ops::commit(repo, &message, amend).map(|_| None),
        Job::Fetch { background } => return ops::fetch(repo, background).map(|_| None),
        Job::Pull => return ops::pull(repo).map(|_| None),
        Job::Push { force } => {
            let branch = status::status(repo)?.branch;
            return ops::push(repo, &branch, force).map(|_| None);
        }
    }))
}

fn job_name(job: &Job) -> &'static str {
    match job {
        Job::Refresh => "refresh",
        Job::LoadMore { .. } => "load_more",
        Job::Diff { .. } => "diff",
        Job::Show { .. } => "show",
        Job::Stage(_) => "stage",
        Job::Unstage(_) => "unstage",
        Job::StageLines(..) => "stage_lines",
        Job::UnstageLines(..) => "unstage_lines",
        Job::DiscardLines(..) => "discard_lines",
        Job::Discard(_) => "discard",
        Job::Ignore(_) => "ignore",
        Job::Commit { .. } => "commit",
        Job::Fetch { .. } => "fetch",
        Job::Pull => "pull",
        Job::Push { .. } => "push",
    }
}

fn worker_loop(repo: PathBuf, rx: Receiver<Job>, out: Sender<(PathBuf, JobResult)>, busy: Arc<AtomicBool>, wake: Wake) {
    let mut queue: VecDeque<Job> = VecDeque::new();
    let send = |r: JobResult| {
        let ok = out.send((repo.clone(), r)).is_ok();
        wake();
        ok
    };
    loop {
        let job = match queue.pop_front() {
            Some(j) => j,
            None => match rx.recv() {
                Ok(j) => j,
                Err(_) => return,
            },
        };
        queue.extend(rx.try_iter());
        let mut job = Some(job);
        if matches!(job, Some(Job::Refresh)) {
            // A refresh about to run makes every queued refresh redundant.
            queue.retain(|j| !matches!(j, Job::Refresh));
        }
        busy.store(true, Ordering::Relaxed);
        while let Some(j) = job.take() {
            let name = job_name(&j);
            let background = matches!(j, Job::Fetch { background: true });
            let result = match run_job(&repo, j) {
                Ok(Some(r)) => r,
                Ok(None) => {
                    queue.retain(|j| !matches!(j, Job::Refresh));
                    job = Some(Job::Refresh);
                    JobResult::Done { job: name }
                }
                Err(error) => JobResult::Error { job: name, error, background },
            };
            if !send(result) {
                return;
            }
        }
        busy.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn spawn(repo: &std::path::Path) -> (RepoWorker, Receiver<(PathBuf, JobResult)>) {
        let (tx, rx) = channel();
        (RepoWorker::spawn(repo.to_path_buf(), tx, Arc::new(|| {})), rx)
    }

    fn next(rx: &Receiver<(PathBuf, JobResult)>) -> JobResult {
        rx.recv_timeout(Duration::from_secs(10)).expect("worker result").1
    }

    #[test]
    fn refresh_returns_snapshot() {
        let (_d, repo) = temp_repo();
        write(&repo, "a", "1\n");
        commit_all(&repo, "one");
        write(&repo, "b", "2\n");
        let (w, rx) = spawn(&repo);
        w.send(Job::Refresh);
        match next(&rx) {
            JobResult::Snapshot(s) => {
                assert_eq!(s.commits.len(), 1);
                assert_eq!(s.layout.rows.len(), 1);
                assert!(!s.has_more);
                assert_eq!(s.status.unstaged().count(), 1);
                assert!(!s.refs.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stage_then_auto_refresh() {
        let (_d, repo) = temp_repo();
        write(&repo, "a", "1\n");
        let (w, rx) = spawn(&repo);
        w.send(Job::Stage(vec!["a".into()]));
        assert!(matches!(next(&rx), JobResult::Done { job: "stage" }));
        match next(&rx) {
            JobResult::Snapshot(s) => assert_eq!(s.status.staged().count(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn error_is_reported_not_panicked() {
        let (_d, repo) = temp_repo();
        let (w, rx) = spawn(&repo);
        w.send(Job::Commit { message: "x".into(), amend: false });
        assert!(matches!(next(&rx), JobResult::Error { job: "commit", background: false, .. }));
        // the worker is still alive
        w.send(Job::Refresh);
        assert!(matches!(next(&rx), JobResult::Snapshot(_)));
    }

    #[test]
    fn background_fetch_error_is_flagged() {
        let (_d, repo) = temp_repo();
        git(&repo, &["remote", "add", "origin", "/nonexistent/repo.git"]);
        let (w, rx) = spawn(&repo);
        w.send(Job::Fetch { background: true });
        assert!(matches!(next(&rx), JobResult::Error { job: "fetch", background: true, .. }));
    }

    #[test]
    fn refresh_coalesced() {
        let (_d, repo) = temp_repo();
        for i in 0..30 {
            write(&repo, "f", &i.to_string());
            commit_all(&repo, "c");
        }
        let (w, rx) = spawn(&repo);
        for _ in 0..10 {
            w.send(Job::Refresh);
        }
        let mut snapshots = 0;
        while let Ok((_, r)) = rx.recv_timeout(Duration::from_millis(1500)) {
            assert!(matches!(r, JobResult::Snapshot(_)));
            snapshots += 1;
        }
        assert!((1..=2).contains(&snapshots), "{snapshots}");
    }

    #[test]
    fn load_more_and_diff_and_show() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\n");
        commit_all(&repo, "one");
        write(&repo, "f", "b\n");
        let (w, rx) = spawn(&repo);
        w.send(Job::LoadMore { skip: 0 });
        assert!(matches!(next(&rx), JobResult::More { commits } if commits.len() == 1));
        w.send(Job::Diff { path: "f".into(), side: DiffSide::Unstaged });
        assert!(matches!(next(&rx), JobResult::Diff { diff: Some(_), .. }));
        w.send(Job::Show { id: "HEAD".into() });
        assert!(matches!(next(&rx), JobResult::Show { commit, .. } if commit.subject == "one"));
    }
}
