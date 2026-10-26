//! Application state and its transitions, independent from rendering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::config::{self, Config, RepoProbe};
use crate::git::{GitError, status};
use crate::git::diff::{DiffSide, FileDiff};
use crate::git::log::Commit;
use crate::git::patch::LineSel;
use crate::i18n::{self, Lang};
use crate::theme::{self, Theme};
use crate::watch::{self, WatchGuard};
use crate::worker::{Job, JobResult, RepoWorker, Snapshot, Wake};

pub struct RepoView {
    pub path: PathBuf,
    pub worker: RepoWorker,
    pub watch: Option<WatchGuard>,
    pub snapshot: Option<Snapshot>,
    pub selected_commit: Option<String>,
    pub selected_file: Option<(String, DiffSide)>,
    pub diff: Option<FileDiff>,
    pub line_sel: LineSel,
    pub detail: Option<(Commit, Vec<FileDiff>)>,
    /// File of the selected commit shown in the central panel.
    pub detail_file: Option<usize>,
    pub commit_msg: String,
    pub amend: bool,
    pub last_fetch: Instant,
    pub fetch_failed: bool,
    pub loading_more: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoBadge {
    pub probe: Option<RepoProbe>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone)]
pub enum Pending {
    /// `repo` is the repository the action was confirmed for; it never runs anywhere else.
    Confirm { repo: PathBuf, text_key: &'static str, action: Job },
    InitRepo(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownError {
    pub message: String,
    pub details: String,
}

/// Results of work moved off the UI thread.
enum Background {
    Probed(PathBuf, RepoProbe),
    Inited(PathBuf, Result<(), GitError>),
    Watching(PathBuf, Option<WatchGuard>),
}

pub struct AppState {
    pub config: Config,
    pub config_path: PathBuf,
    pub lang: Lang,
    pub theme: Theme,
    pub open: Option<RepoView>,
    pub badges: HashMap<PathBuf, RepoBadge>,
    pub errors: Vec<ShownError>,
    pub pending: Option<Pending>,
    pub show_settings: bool,
    /// Probes, inits and watcher setups still running off the UI thread.
    pub background_tasks: usize,
    pub wake: Wake,
    results_tx: Sender<(PathBuf, JobResult)>,
    results_rx: Receiver<(PathBuf, JobResult)>,
    badges_tx: Sender<(PathBuf, RepoBadge)>,
    badges_rx: Receiver<(PathBuf, RepoBadge)>,
    background_tx: Sender<Background>,
    background_rx: Receiver<Background>,
}

/// Branch, dirtiness and ahead/behind of a listed repository, read from local data only.
pub fn compute_badge(p: &Path) -> RepoBadge {
    let probe = config::probe(p);
    if !matches!(probe, RepoProbe::Ok { .. }) {
        return RepoBadge { probe: Some(probe), ..Default::default() };
    }
    match status::status(p) {
        Ok(st) => RepoBadge {
            probe: Some(probe),
            dirty: !st.entries.is_empty(),
            branch: st.branch.head.clone(),
            ahead: st.branch.ahead,
            behind: st.branch.behind,
        },
        Err(_) => RepoBadge { probe: Some(probe), ..Default::default() },
    }
}

impl AppState {
    pub fn new(config: Config, config_path: PathBuf, wake: Wake) -> Self {
        let (results_tx, results_rx) = channel();
        let (badges_tx, badges_rx) = channel();
        let (background_tx, background_rx) = channel();
        AppState {
            lang: i18n::resolve(config.language),
            theme: theme::resolve(config.theme),
            config,
            config_path,
            open: None,
            badges: HashMap::new(),
            errors: Vec::new(),
            pending: None,
            show_settings: false,
            background_tasks: 0,
            wake,
            results_tx,
            results_rx,
            badges_tx,
            badges_rx,
            background_tx,
            background_rx,
        }
    }

    pub fn save_config(&mut self) {
        if let Err(e) = self.config.save(&self.config_path) {
            self.errors.push(ShownError { message: e.to_string(), details: self.config_path.display().to_string() });
        }
    }

    pub fn open_repo(&mut self, p: PathBuf) {
        // A confirmation belongs to the repository it was asked for.
        if matches!(self.pending, Some(Pending::Confirm { .. })) {
            self.pending = None;
        }
        let worker = RepoWorker::spawn(p.clone(), self.results_tx.clone(), self.wake.clone());
        let jobs = worker.sender();
        let root = p.clone();
        // A recursive watch walks the whole tree: never on the UI thread.
        self.spawn_background(move || {
            let watch = watch::spawn_watcher(&root, move || {
                let _ = jobs.send(Job::Refresh);
            })
            .ok();
            Background::Watching(root, watch)
        });
        worker.send(Job::Refresh);
        self.open = Some(RepoView {
            path: p.clone(),
            worker,
            watch: None,
            snapshot: None,
            selected_commit: None,
            selected_file: None,
            diff: None,
            line_sel: LineSel::new(),
            detail: None,
            detail_file: None,
            commit_msg: String::new(),
            amend: false,
            last_fetch: Instant::now(),
            fetch_failed: false,
            loading_more: false,
        });
        if self.config.autofetch_minutes > 0 {
            self.send(Job::Fetch { background: true });
        }
        self.config.last_open = Some(p);
        self.save_config();
        self.refresh_badges();
    }

    /// Runs `task` on a thread; its result is handled by `poll`.
    fn spawn_background(&mut self, task: impl FnOnce() -> Background + Send + 'static) {
        self.background_tasks += 1;
        let tx = self.background_tx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let _ = tx.send(task());
            wake();
        });
    }

    /// Probes `p` in the background, then opens it, offers `git init`, or reports it missing.
    pub fn add_folder(&mut self, p: PathBuf) {
        self.spawn_background(move || {
            let probe = config::probe(&p);
            Background::Probed(p, probe)
        });
    }

    pub fn init_and_add(&mut self, p: PathBuf) {
        self.spawn_background(move || {
            let result = config::init_repo(&p);
            Background::Inited(p, result)
        });
    }

    fn handle_background(&mut self, b: Background) {
        self.background_tasks = self.background_tasks.saturating_sub(1);
        match b {
            Background::Probed(_, RepoProbe::Ok { root }) => {
                self.config.add_repo(root.clone());
                self.open_repo(root);
            }
            Background::Probed(p, RepoProbe::NotARepo) => self.pending = Some(Pending::InitRepo(p)),
            Background::Probed(p, RepoProbe::Missing) => self.errors.push(ShownError {
                message: self.tr("repos.missing").to_string(),
                details: p.display().to_string(),
            }),
            Background::Inited(p, Ok(())) => self.add_folder(p),
            Background::Inited(_, Err(e)) => self.push_error(&e),
            Background::Watching(p, guard) => {
                if let Some(v) = self.open.as_mut().filter(|v| v.path == p) {
                    v.watch = guard;
                }
            }
        }
    }

    pub fn remove_repo(&mut self, p: &Path) {
        self.config.remove_repo(p);
        self.badges.remove(p);
        if self.open.as_ref().is_some_and(|v| v.path == p) {
            self.open = None;
        }
        self.save_config();
    }

    /// Recomputes the sidebar badges on a background thread.
    pub fn refresh_badges(&self) {
        let repos = self.config.repos.clone();
        let tx = self.badges_tx.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            for r in repos {
                let b = compute_badge(&r);
                if tx.send((r, b)).is_err() {
                    return;
                }
            }
            wake();
        });
    }

    /// Sends a job to the open repository's worker.
    pub fn send(&self, job: Job) {
        if let Some(v) = &self.open {
            v.worker.send(job);
        }
    }

    /// Drains worker and badge results. Called every frame.
    pub fn poll(&mut self) {
        while let Ok((path, r)) = self.results_rx.try_recv() {
            self.handle_result(&path, r);
        }
        while let Ok(b) = self.background_rx.try_recv() {
            self.handle_background(b);
        }
        while let Ok((path, b)) = self.badges_rx.try_recv() {
            self.badges.insert(path, b);
        }
    }

    pub fn handle_result(&mut self, path: &Path, r: JobResult) {
        if let JobResult::Error { error, background: false, .. } = &r {
            if self.open.as_ref().is_some_and(|v| v.path == path) {
                self.push_error(error);
            }
            return;
        }
        let Some(v) = self.open.as_mut().filter(|v| v.path == path) else { return };
        match r {
            JobResult::Snapshot(snap) => {
                if let Some(b) = self.badges.get_mut(path) {
                    b.branch = snap.status.branch.head.clone();
                    b.dirty = !snap.status.entries.is_empty();
                    b.ahead = snap.status.branch.ahead;
                    b.behind = snap.status.branch.behind;
                }
                // Keep the open diff in sync with the work tree, or close it when the file
                // no longer has changes on that side.
                if let Some((file, side)) = v.selected_file.clone() {
                    let still_there = match side {
                        DiffSide::Staged => snap.status.staged().any(|e| e.path == file),
                        DiffSide::Unstaged => snap.status.unstaged().any(|e| e.path == file),
                    };
                    if still_there {
                        v.worker.send(Job::Diff { path: file, side });
                    } else {
                        v.selected_file = None;
                        v.diff = None;
                        v.line_sel.clear();
                    }
                }
                v.snapshot = Some(snap);
                v.loading_more = false;
            }
            JobResult::Diff { path: file, side, diff } => {
                if v.selected_file.as_ref() == Some(&(file, side)) {
                    if v.diff != diff {
                        v.line_sel.clear();
                    }
                    v.diff = diff;
                }
            }
            JobResult::Show { commit, files } => {
                if v.selected_commit.as_deref() == Some(commit.id.as_str()) {
                    v.detail = Some((commit, files));
                    v.detail_file = None;
                }
            }
            JobResult::Done { job } => match job {
                "commit" => {
                    v.commit_msg.clear();
                    v.amend = false;
                }
                "fetch" => v.fetch_failed = false,
                _ => {}
            },
            JobResult::Error { job, .. } => {
                if job == "fetch" {
                    v.fetch_failed = true;
                }
            }
        }
    }

    pub fn push_error(&mut self, e: &GitError) {
        let (message, details) = match e {
            GitError::Failed { command, stderr, .. } => {
                let first = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(command);
                let message =
                    if first.starts_with("error.") { self.tr(first).to_string() } else { first.to_string() };
                (message, format!("{command}\n\n{}", stderr.trim_end()))
            }
            GitError::Spawn(io) => (e.to_string(), io.to_string()),
        };
        self.errors.push(ShownError { message, details });
    }

    pub fn set_theme(&mut self, choice: crate::theme::ThemeChoice) {
        self.config.theme = choice;
        self.theme = theme::resolve(choice);
        self.save_config();
    }

    pub fn set_language(&mut self, choice: crate::i18n::LangChoice) {
        self.config.language = choice;
        self.lang = i18n::resolve(choice);
        self.save_config();
    }

    /// Asks for confirmation before running `action` on the open repository.
    pub fn confirm(&mut self, text_key: &'static str, action: Job) {
        if let Some(v) = &self.open {
            self.pending = Some(Pending::Confirm { repo: v.path.clone(), text_key, action });
        }
    }

    /// Resolves the pending dialog. A confirmed action only runs on the repository it was
    /// confirmed for.
    pub fn answer_pending(&mut self, yes: bool) {
        let Some(pending) = self.pending.take() else { return };
        if !yes {
            return;
        }
        match pending {
            Pending::Confirm { repo, action, .. } => {
                if self.open.as_ref().is_some_and(|v| v.path == repo) {
                    self.send(action);
                }
            }
            Pending::InitRepo(path) => self.init_and_add(path),
        }
    }

    pub fn tr<'a>(&self, key: &'a str) -> &'a str {
        i18n::tr(self.lang, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn state() -> (tempfile::TempDir, AppState) {
        let d = tempfile::TempDir::new().unwrap();
        let cfg = Config { language: crate::i18n::LangChoice::Fr, ..Default::default() };
        let s = AppState::new(cfg, d.path().join("config.toml"), Arc::new(|| {}));
        (d, s)
    }

    /// Polls until no background task (probe, init, watcher setup) is in flight.
    fn settle(s: &mut AppState) {
        for _ in 0..250 {
            s.poll();
            if s.background_tasks == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("background tasks never finished");
    }

    fn add(s: &mut AppState, p: impl Into<PathBuf>) {
        s.add_folder(p.into());
        settle(s);
    }

    #[test]
    fn add_folder_is_asynchronous() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        s.add_folder(repo);
        assert!(s.open.is_none(), "probing must not happen on the UI thread");
        settle(&mut s);
        assert!(s.open.is_some());
        assert!(s.open.as_ref().unwrap().watch.is_some());
    }

    fn wait_snapshot(s: &mut AppState) {
        for _ in 0..200 {
            s.poll();
            if s.open.as_ref().is_some_and(|v| v.snapshot.is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no snapshot");
    }

    #[test]
    fn add_folder_repo_opens_and_saves() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        std::fs::create_dir(repo.join("sub")).unwrap();
        add(&mut s, repo.join("sub"));
        let root = repo.canonicalize().unwrap();
        assert_eq!(s.config.repos, vec![root.clone()]);
        assert_eq!(s.open.as_ref().unwrap().path, root);
        assert_eq!(s.config.last_open.as_ref(), Some(&root));
        assert_eq!(Config::load(&s.config_path).repos, vec![root]);
        wait_snapshot(&mut s);
    }

    #[test]
    fn add_folder_non_repo_sets_pending_init() {
        let (_d, mut s) = state();
        let dir = tempfile::TempDir::new().unwrap();
        add(&mut s, dir.path().to_path_buf());
        assert!(matches!(&s.pending, Some(Pending::InitRepo(p)) if p == dir.path()));
        assert!(s.config.repos.is_empty());
        s.init_and_add(dir.path().to_path_buf());
        settle(&mut s);
        assert_eq!(s.config.repos.len(), 1);
        assert!(s.open.is_some());
    }

    #[test]
    fn add_missing_folder_shows_error() {
        let (_d, mut s) = state();
        add(&mut s, "/definitely/missing");
        assert_eq!(s.errors.len(), 1);
        assert!(s.config.repos.is_empty());
    }

    #[test]
    fn handle_error_result_pushes_shown_error() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        add(&mut s, repo.clone());
        let path = s.open.as_ref().unwrap().path.clone();
        let err = GitError::Failed { command: "git push".into(), code: Some(1), stderr: "fatal: nope\nmore".into() };
        s.handle_result(&path, JobResult::Error { job: "push", error: err, background: false });
        assert_eq!(s.errors, vec![ShownError { message: "fatal: nope".into(), details: "git push\n\nfatal: nope\nmore".into() }]);
        let err = GitError::Failed { command: "git fetch".into(), code: Some(1), stderr: "x".into() };
        s.handle_result(&path, JobResult::Error { job: "fetch", error: err, background: true });
        assert_eq!(s.errors.len(), 1);
        assert!(s.open.as_ref().unwrap().fetch_failed);
    }

    #[test]
    fn own_error_keys_are_translated() {
        let (_d, mut s) = state();
        let err = GitError::Failed { command: "git push".into(), code: None, stderr: "error.no_remote".into() };
        s.push_error(&err);
        assert_eq!(s.errors[0].message, "Aucun dépôt distant n'est configuré.");
    }

    #[test]
    fn results_for_other_repo_are_ignored() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        add(&mut s, repo);
        s.handle_result(Path::new("/elsewhere"), JobResult::Snapshot(Snapshot::default()));
        assert!(s.open.as_ref().unwrap().snapshot.is_none());
    }

    #[test]
    fn commit_done_clears_message() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        add(&mut s, repo);
        let path = s.open.as_ref().unwrap().path.clone();
        let v = s.open.as_mut().unwrap();
        v.commit_msg = "msg".into();
        v.amend = true;
        s.handle_result(&path, JobResult::Done { job: "commit" });
        let v = s.open.as_ref().unwrap();
        assert!(v.commit_msg.is_empty());
        assert!(!v.amend);
    }

    #[test]
    fn remove_open_repo_closes_view() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        add(&mut s, repo.clone());
        let root = repo.canonicalize().unwrap();
        s.remove_repo(&root);
        assert!(s.open.is_none());
        assert!(s.config.repos.is_empty());
        assert!(repo.exists());
    }

    #[test]
    fn compute_badge_reads_state() {
        let (_r, repo) = temp_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "one");
        write(&repo, "a", "2");
        let b = compute_badge(&repo);
        assert!(matches!(b.probe, Some(RepoProbe::Ok { .. })));
        assert_eq!(b.branch.as_deref(), Some("main"));
        assert!(b.dirty);
        assert_eq!(compute_badge(Path::new("/nope/x")).probe, Some(RepoProbe::Missing));
    }

    #[test]
    fn selected_file_cleared_when_it_leaves_the_side() {
        let (_d, mut s) = state();
        let (_r, repo) = temp_repo();
        add(&mut s, repo);
        let path = s.open.as_ref().unwrap().path.clone();
        let v = s.open.as_mut().unwrap();
        v.selected_file = Some(("gone.txt".into(), DiffSide::Unstaged));
        v.line_sel.insert((0, 1));
        s.handle_result(&path, JobResult::Snapshot(Snapshot::default()));
        let v = s.open.as_ref().unwrap();
        assert_eq!(v.selected_file, None);
        assert!(v.line_sel.is_empty());
    }

    #[test]
    fn changing_theme_updates_state_and_config() {
        let (_d, mut s) = state();
        s.set_theme(crate::theme::ThemeChoice::Clair);
        assert_eq!(s.theme, Theme::Clair);
        assert_eq!(Config::load(&s.config_path).theme, crate::theme::ThemeChoice::Clair);
    }

    #[test]
    fn changing_language_updates_lang() {
        let (_d, mut s) = state();
        s.set_language(crate::i18n::LangChoice::En);
        assert_eq!(s.lang, Lang::En);
        assert_eq!(s.tr("repos.add"), "Add a folder");
        assert_eq!(Config::load(&s.config_path).language, crate::i18n::LangChoice::En);
    }

    fn modified_repo() -> (tempfile::TempDir, PathBuf) {
        let (d, repo) = temp_repo();
        write(&repo, "f", "1\n");
        commit_all(&repo, "init");
        write(&repo, "f", "2\n");
        (d, repo)
    }

    fn discard_f() -> Job {
        Job::Discard(vec![crate::git::status::FileEntry {
            path: "f".into(),
            orig_path: None,
            index: crate::git::status::FileState::Unmodified,
            worktree: crate::git::status::FileState::Modified,
            conflicted: false,
        }])
    }

    #[test]
    fn switching_repo_drops_pending_confirmation() {
        let (_d, mut s) = state();
        let (_a, a) = modified_repo();
        let (_b, b) = modified_repo();
        add(&mut s, a);
        s.confirm("confirm.discard", discard_f());
        assert!(matches!(&s.pending, Some(Pending::Confirm { .. })));
        add(&mut s, b);
        assert!(s.pending.is_none());
    }

    #[test]
    fn confirmed_action_never_runs_on_another_repo() {
        let (_d, mut s) = state();
        let (_a, a) = modified_repo();
        let (_b, b) = modified_repo();
        add(&mut s, a.clone());
        let a_root = s.open.as_ref().unwrap().path.clone();
        add(&mut s, b.clone());
        // A confirmation created for A while B is open (e.g. a switch raced the dialog).
        s.pending = Some(Pending::Confirm { repo: a_root, text_key: "confirm.discard", action: discard_f() });
        s.answer_pending(true);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(std::fs::read_to_string(b.join("f")).unwrap(), "2\n");
        assert_eq!(std::fs::read_to_string(a.join("f")).unwrap(), "2\n");
        assert!(s.pending.is_none());
    }

    #[test]
    fn confirmed_action_runs_on_its_repo() {
        let (_d, mut s) = state();
        let (_a, a) = modified_repo();
        add(&mut s, a.clone());
        s.confirm("confirm.discard", discard_f());
        s.answer_pending(true);
        for _ in 0..100 {
            if std::fs::read_to_string(a.join("f")).unwrap() == "1\n" {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("discard did not run");
    }
}
