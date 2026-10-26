//! High-level git operations.

use std::io::Write;
use std::path::Path;

use super::diff::FileDiff;
use super::patch::{LineSel, PatchMode, apply_patch, build_patch};
use super::status::{BranchInfo, FileEntry, FileState};
use super::{GitError, RunOpts, run, run_str};

fn git(repo: &Path, args: &[&str]) -> Result<(), GitError> {
    run(repo, args, &RunOpts::default()).map(|_| ())
}

fn with_paths<'a>(args: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    let mut v = args.to_vec();
    v.push("--");
    v.extend(paths.iter().map(String::as_str));
    v
}

fn has_head(repo: &Path) -> bool {
    let opts = RunOpts { read_only: true, ..Default::default() };
    run(repo, &["rev-parse", "--verify", "-q", "HEAD"], &opts).is_ok()
}

/// An error with a translation key as message, for failures GiGit detects itself.
fn own_error(command: &str, key: &str) -> GitError {
    GitError::Failed { command: command.to_string(), code: None, stderr: key.to_string() }
}

pub fn stage_paths(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    git(repo, &with_paths(&["add", "-A"], paths))
}

pub fn unstage_paths(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    if has_head(repo) {
        git(repo, &with_paths(&["restore", "--staged"], paths))
    } else {
        git(repo, &with_paths(&["rm", "--cached", "-r", "-q"], paths))
    }
}

fn apply_selection(
    repo: &Path,
    file: &FileDiff,
    sel: &LineSel,
    mode: PatchMode,
    cached: bool,
) -> Result<(), GitError> {
    match build_patch(file, sel, mode) {
        Some(patch) => apply_patch(repo, &patch, cached, mode == PatchMode::Reverse),
        None => Ok(()),
    }
}

pub fn stage_lines(repo: &Path, file: &FileDiff, sel: &LineSel) -> Result<(), GitError> {
    apply_selection(repo, file, sel, PatchMode::Forward, true)
}

pub fn unstage_lines(repo: &Path, file: &FileDiff, sel: &LineSel) -> Result<(), GitError> {
    apply_selection(repo, file, sel, PatchMode::Reverse, true)
}

pub fn discard_lines(repo: &Path, file: &FileDiff, sel: &LineSel) -> Result<(), GitError> {
    apply_selection(repo, file, sel, PatchMode::Reverse, false)
}

/// Discards unstaged changes only: the index is left untouched.
pub fn discard_paths(repo: &Path, entries: &[FileEntry]) -> Result<(), GitError> {
    let (untracked, tracked): (Vec<_>, Vec<_>) =
        entries.iter().partition(|e| e.worktree == FileState::Untracked);
    let tracked: Vec<String> = tracked.into_iter().map(|e| e.path.clone()).collect();
    let untracked: Vec<String> = untracked.into_iter().map(|e| e.path.clone()).collect();
    if !tracked.is_empty() {
        git(repo, &with_paths(&["restore", "--worktree"], &tracked))?;
    }
    if !untracked.is_empty() {
        git(repo, &with_paths(&["clean", "-f", "-q"], &untracked))?;
    }
    Ok(())
}

pub fn ignore_path(repo: &Path, path: &str) -> Result<(), GitError> {
    let file = repo.join(".gitignore");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&file)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    writeln!(f, "/{path}")?;
    Ok(())
}

pub fn commit(repo: &Path, message: &str, amend: bool) -> Result<(), GitError> {
    let mut args = vec!["commit", "-F", "-"];
    if amend {
        args.push("--amend");
    }
    let opts = RunOpts { stdin: Some(message.as_bytes().to_vec()), ..Default::default() };
    run(repo, &args, &opts).map(|_| ())
}

pub fn last_commit_message(repo: &Path) -> Result<Option<String>, GitError> {
    if !has_head(repo) {
        return Ok(None);
    }
    let opts = RunOpts { read_only: true, ..Default::default() };
    let msg = run_str(repo, &["log", "-1", "--format=%B"], &opts)?;
    Ok(Some(msg.trim_end().to_string()))
}

pub fn fetch(repo: &Path, background: bool) -> Result<(), GitError> {
    let opts = RunOpts { background, ..Default::default() };
    run(repo, &["fetch", "--all", "--prune"], &opts).map(|_| ())
}

pub fn pull(repo: &Path) -> Result<(), GitError> {
    git(repo, &["pull"])
}

fn config(repo: &Path, key: &str) -> Option<String> {
    let opts = RunOpts { read_only: true, ..Default::default() };
    run_str(repo, &["config", "--get", key], &opts).ok().map(|s| s.trim().to_string())
}

fn push_remote(repo: &Path, head: &str) -> Result<String, GitError> {
    let opts = RunOpts { read_only: true, ..Default::default() };
    let remotes: Vec<String> = run_str(repo, &["remote"], &opts)?.lines().map(str::to_string).collect();
    let candidates = [config(repo, &format!("branch.{head}.pushRemote")), config(repo, "remote.pushDefault")];
    candidates
        .into_iter()
        .flatten()
        .chain(["origin".to_string()])
        .find(|r| remotes.contains(r))
        .or_else(|| remotes.first().cloned())
        .ok_or_else(|| own_error("git push", "error.no_remote"))
}

pub fn push(repo: &Path, branch: &BranchInfo, force: bool) -> Result<(), GitError> {
    let Some(head) = branch.head.as_deref() else {
        return Err(own_error("git push", "error.push_detached"));
    };
    let mut args = vec!["push".to_string()];
    if force {
        // The lease alone compares with origin/*, which autofetch keeps fresh; --force-if-includes
        // also requires those remote commits to have been integrated locally.
        args.extend(["--force-with-lease".into(), "--force-if-includes".into()]);
    }
    if branch.upstream.is_none() {
        args.extend(["-u".into(), push_remote(repo, head)?, head.to_string()]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    git(repo, &args)
}
