mod common;

use std::path::Path;

use common::*;
use gigit::git::diff::{DiffSide, diff_file};
use gigit::git::log::log;
use gigit::git::ops::*;
use gigit::git::patch::LineSel;
use gigit::git::status::{FileState, status};

fn staged(repo: &Path) -> Vec<String> {
    status(repo).unwrap().staged().map(|e| e.path.clone()).collect()
}

fn unstaged(repo: &Path) -> Vec<String> {
    status(repo).unwrap().unstaged().map(|e| e.path.clone()).collect()
}

/// A repo with one commit on `main`, pushed to a bare `origin`.
fn repo_with_origin() -> (tempfile::TempDir, std::path::PathBuf, tempfile::TempDir, std::path::PathBuf) {
    let (d, repo) = temp_repo();
    let bare_dir = tempfile::TempDir::new().unwrap();
    let bare = bare_dir.path().to_path_buf();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    write(&repo, "a.txt", "1\n");
    commit_all(&repo, "init");
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    (d, repo, bare_dir, bare)
}

#[test]
fn stage_and_unstage_paths() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    stage_paths(&repo, &["a.txt".into()]).unwrap();
    assert_eq!(staged(&repo), ["a.txt"]);
    unstage_paths(&repo, &["a.txt".into()]).unwrap();
    assert!(staged(&repo).is_empty());
    assert_eq!(unstaged(&repo), ["a.txt"]);
}

#[test]
fn stage_path_with_spaces() {
    let (_d, repo) = temp_repo();
    write(&repo, "dir a/é b.txt", "1\n");
    stage_paths(&repo, &["dir a/é b.txt".into()]).unwrap();
    assert_eq!(staged(&repo), ["dir a/é b.txt"]);
}

#[test]
fn stage_deleted_path() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    commit_all(&repo, "init");
    std::fs::remove_file(repo.join("a.txt")).unwrap();
    stage_paths(&repo, &["a.txt".into()]).unwrap();
    let st = status(&repo).unwrap();
    assert_eq!(st.entries[0].index, FileState::Deleted);
}

#[test]
fn unstage_on_unborn_repo() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    stage_paths(&repo, &["a.txt".into()]).unwrap();
    unstage_paths(&repo, &["a.txt".into()]).unwrap();
    assert!(staged(&repo).is_empty());
}

#[test]
fn stage_lines_wrapper() {
    let (_d, repo) = temp_repo();
    write(&repo, "f", "a\nb\n");
    commit_all(&repo, "init");
    write(&repo, "f", "A\nB\n");
    let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
    let sel: LineSel = [(0, 0), (0, 2)].into_iter().collect();
    stage_lines(&repo, &f, &sel).unwrap();
    assert_eq!(git(&repo, &["show", ":f"]), "A\nb\n");
    let f = diff_file(&repo, "f", DiffSide::Staged).unwrap().unwrap();
    let sel: LineSel = [(0, 0), (0, 1)].into_iter().collect();
    unstage_lines(&repo, &f, &sel).unwrap();
    assert_eq!(git(&repo, &["show", ":f"]), "a\nb\n");
    let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
    let sel: LineSel = [(0, 1), (0, 3)].into_iter().collect();
    discard_lines(&repo, &f, &sel).unwrap();
    assert_eq!(std::fs::read_to_string(repo.join("f")).unwrap(), "A\nb\n");
}

#[test]
fn first_commit_on_empty_repo() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    stage_paths(&repo, &["a.txt".into()]).unwrap();
    commit(&repo, "Premier commit\n\nAvec un corps", false).unwrap();
    let commits = log(&repo, 0, 10).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "Premier commit");
    assert_eq!(commits[0].body, "Avec un corps");
}

#[test]
fn amend_replaces_last_commit() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    commit_all(&repo, "old");
    assert_eq!(last_commit_message(&repo).unwrap().as_deref(), Some("old"));
    commit(&repo, "new", true).unwrap();
    let commits = log(&repo, 0, 10).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "new");
}

#[test]
fn last_commit_message_on_unborn_is_none() {
    let (_d, repo) = temp_repo();
    assert_eq!(last_commit_message(&repo).unwrap(), None);
}

#[test]
fn commit_with_nothing_staged_fails() {
    let (_d, repo) = temp_repo();
    write(&repo, "a.txt", "1\n");
    commit_all(&repo, "init");
    assert!(commit(&repo, "nothing", false).is_err());
}

#[test]
fn discard_tracked_and_untracked() {
    let (_d, repo) = temp_repo();
    write(&repo, "t.txt", "1\n");
    write(&repo, "s.txt", "1\n");
    commit_all(&repo, "init");
    write(&repo, "t.txt", "2\n");
    write(&repo, "s.txt", "staged\n");
    git(&repo, &["add", "s.txt"]);
    write(&repo, "s.txt", "staged then edited\n");
    write(&repo, "u dir/u.txt", "new\n");
    let st = status(&repo).unwrap();
    let entries: Vec<_> = st.unstaged().cloned().collect();
    discard_paths(&repo, &entries).unwrap();
    assert_eq!(std::fs::read_to_string(repo.join("t.txt")).unwrap(), "1\n");
    assert_eq!(std::fs::read_to_string(repo.join("s.txt")).unwrap(), "staged\n");
    assert!(!repo.join("u dir/u.txt").exists());
    assert_eq!(staged(&repo), ["s.txt"]);
}

#[test]
fn ignore_path_appends_line() {
    let (_d, repo) = temp_repo();
    write(&repo, ".gitignore", "old");
    ignore_path(&repo, "build").unwrap();
    assert_eq!(std::fs::read_to_string(repo.join(".gitignore")).unwrap(), "old\n/build\n");
    ignore_path(&repo, "x y.log").unwrap();
    assert_eq!(std::fs::read_to_string(repo.join(".gitignore")).unwrap(), "old\n/build\n/x y.log\n");
}

#[test]
fn ignore_path_creates_file() {
    let (_d, repo) = temp_repo();
    ignore_path(&repo, "a.log").unwrap();
    assert_eq!(std::fs::read_to_string(repo.join(".gitignore")).unwrap(), "/a.log\n");
}

#[test]
fn push_without_upstream_sets_it() {
    let (_d, repo, _b, _bare) = repo_with_origin();
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    write(&repo, "b.txt", "1\n");
    commit_all(&repo, "feat");
    let br = status(&repo).unwrap().branch;
    assert_eq!(br.upstream, None);
    push(&repo, &br, false).unwrap();
    assert_eq!(status(&repo).unwrap().branch.upstream.as_deref(), Some("origin/feature"));
}

#[test]
fn push_detached_fails() {
    let (_d, repo, _b, _bare) = repo_with_origin();
    git(&repo, &["checkout", "-q", "--detach"]);
    let br = status(&repo).unwrap().branch;
    let err = push(&repo, &br, false).unwrap_err();
    assert_eq!(err.to_string(), "error.push_detached");
}

#[test]
fn fetch_then_behind_count() {
    let (_d, repo, _b, bare) = repo_with_origin();
    let other_dir = tempfile::TempDir::new().unwrap();
    let other = other_dir.path().join("clone");
    git(other_dir.path(), &["clone", "-q", bare.to_str().unwrap(), "clone"]);
    git(&other, &["config", "user.name", "O"]);
    git(&other, &["config", "user.email", "o@x"]);
    write(&other, "c.txt", "1\n");
    commit_all(&other, "remote change");
    git(&other, &["push", "-q"]);
    fetch(&repo, true).unwrap();
    assert_eq!(status(&repo).unwrap().branch.behind, 1);
    pull(&repo).unwrap();
    assert_eq!(status(&repo).unwrap().branch.behind, 0);
    assert!(repo.join("c.txt").exists());
}

#[test]
fn force_push_with_lease() {
    let (_d, repo, _b, _bare) = repo_with_origin();
    write(&repo, "a.txt", "2\n");
    commit_all(&repo, "second");
    let br = status(&repo).unwrap().branch;
    push(&repo, &br, false).unwrap();
    commit(&repo, "second amended", true).unwrap();
    let br = status(&repo).unwrap().branch;
    assert!(push(&repo, &br, false).is_err());
    push(&repo, &br, true).unwrap();
    let br = status(&repo).unwrap().branch;
    assert_eq!((br.ahead, br.behind), (0, 0));
}
