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

#[test]
fn bracket_paths_are_literal() {
    let (_d, repo) = temp_repo();
    write(&repo, "f1.txt", "one\n");
    write(&repo, "f[1].txt", "bracket\n");
    commit_all(&repo, "init");
    write(&repo, "f1.txt", "one EDITED, precious\n");
    write(&repo, "f[1].txt", "bracket edited\n");
    let st = status(&repo).unwrap();
    let target: Vec<_> = st.unstaged().filter(|e| e.path == "f[1].txt").cloned().collect();
    discard_paths(&repo, &target).unwrap();
    assert_eq!(std::fs::read_to_string(repo.join("f1.txt")).unwrap(), "one EDITED, precious\n");
    assert_eq!(std::fs::read_to_string(repo.join("f[1].txt")).unwrap(), "bracket\n");
    write(&repo, "f[1].txt", "bracket edited again\n");
    stage_paths(&repo, &["f[1].txt".into()]).unwrap();
    assert_eq!(staged(&repo), ["f[1].txt"]);
    let d = diff_file(&repo, "f[1].txt", DiffSide::Staged).unwrap().unwrap();
    assert_eq!(d.path(), "f[1].txt");
}

#[test]
fn clean_of_star_named_file_deletes_only_it() {
    let (_d, repo) = temp_repo();
    write(&repo, "base", "x\n");
    commit_all(&repo, "init");
    write(&repo, "*", "star\n");
    write(&repo, "other.txt", "keep me\n");
    let st = status(&repo).unwrap();
    let star: Vec<_> = st.unstaged().filter(|e| e.path == "*").cloned().collect();
    discard_paths(&repo, &star).unwrap();
    assert!(!repo.join("*").exists());
    assert!(repo.join("other.txt").exists());
}

fn all_lines(f: &gigit::git::diff::FileDiff) -> LineSel {
    let mut s = LineSel::new();
    for (h, hunk) in f.hunks.iter().enumerate() {
        for (i, l) in hunk.lines.iter().enumerate() {
            if l.kind != gigit::git::diff::LineKind::Context {
                s.insert((h, i));
            }
        }
    }
    s
}

fn index_bytes(repo: &Path, path: &str) -> Vec<u8> {
    let out = std::process::Command::new("git").args(["show", &format!(":{path}")]).current_dir(repo).output().unwrap();
    assert!(out.status.success());
    out.stdout
}

#[test]
fn stage_hunk_of_latin1_file_keeps_bytes() {
    let (_d, repo) = temp_repo();
    write(&repo, "l.txt", "a\nb\nc\n");
    commit_all(&repo, "init");
    std::fs::write(repo.join("l.txt"), b"a\nb\ncaf\xE9\nc\n").unwrap();
    let f = diff_file(&repo, "l.txt", DiffSide::Unstaged).unwrap().unwrap();
    stage_lines(&repo, &f, &all_lines(&f)).unwrap();
    assert_eq!(index_bytes(&repo, "l.txt"), b"a\nb\ncaf\xE9\nc\n");
}

#[test]
fn stage_hunk_of_untracked_crlf_keeps_cr() {
    let (_d, repo) = temp_repo();
    write(&repo, "base", "x\n");
    commit_all(&repo, "init");
    std::fs::write(repo.join("w.txt"), b"one\r\ntwo\r\nthree\r\n").unwrap();
    let f = diff_file(&repo, "w.txt", DiffSide::Unstaged).unwrap().unwrap();
    assert_eq!(f.hunks[0].lines[0].text, "one");
    stage_lines(&repo, &f, &all_lines(&f)).unwrap();
    assert_eq!(index_bytes(&repo, "w.txt"), b"one\r\ntwo\r\nthree\r\n");
}

#[test]
fn stage_line_of_tracked_crlf_file() {
    let (_d, repo) = temp_repo();
    std::fs::write(repo.join("t.txt"), b"a\r\nb\r\n").unwrap();
    commit_all(&repo, "init");
    std::fs::write(repo.join("t.txt"), b"A\r\nB\r\n").unwrap();
    let f = diff_file(&repo, "t.txt", DiffSide::Unstaged).unwrap().unwrap();
    let sel: LineSel = [(0, 0), (0, 2)].into_iter().collect();
    stage_lines(&repo, &f, &sel).unwrap();
    assert_eq!(index_bytes(&repo, "t.txt"), b"A\r\nb\r\n");
}

#[test]
fn user_diff_config_does_not_break_patches() {
    let (_d, repo) = temp_repo();
    git(&repo, &["config", "diff.noprefix", "true"]);
    git(&repo, &["config", "color.diff", "always"]);
    git(&repo, &["config", "diff.mnemonicPrefix", "true"]);
    write(&repo, "src/x.rs", "a\n");
    commit_all(&repo, "init");
    write(&repo, "src/x.rs", "b\n");
    let f = diff_file(&repo, "src/x.rs", DiffSide::Unstaged).unwrap().expect("diff parsed despite color.diff=always");
    assert_eq!(f.path(), "src/x.rs");
    stage_lines(&repo, &f, &all_lines(&f)).unwrap();
    assert_eq!(staged(&repo), ["src/x.rs"]);
    assert_eq!(index_bytes(&repo, "src/x.rs"), b"b\n");
}

#[test]
fn show_ignores_signature_and_color_config() {
    let (_d, repo) = temp_repo();
    git(&repo, &["config", "log.showSignature", "true"]);
    git(&repo, &["config", "color.ui", "always"]);
    write(&repo, "a", "1\n");
    commit_all(&repo, "one");
    let (c, files) = gigit::git::log::show(&repo, "HEAD").unwrap();
    assert_eq!(c.subject, "one");
    assert_eq!(files.len(), 1);
    assert_eq!(log(&repo, 0, 5).unwrap()[0].subject, "one");
}

fn numbered(prefix: &str, n: usize) -> String {
    (0..n).map(|i| format!("{prefix}{i}\n")).collect()
}

/// Two identical blocks; the work tree adds lines on top and `NEW` inside both blocks.
fn repeated_blocks_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let (d, repo) = temp_repo();
    let block = numbered("x", 7);
    let base = format!("{}{block}{}{block}", numbered("a", 10), numbered("m", 10));
    write(&repo, "r.txt", &base);
    commit_all(&repo, "init");
    let with_new = format!("{}NEW\n{}", numbered("x", 4), (4..7).map(|i| format!("x{i}\n")).collect::<String>());
    let changed = format!("{}{}{with_new}{}{with_new}", numbered("top", 20), numbered("a", 10), numbered("m", 10));
    write(&repo, "r.txt", &changed);
    (d, repo)
}

fn new_line_of_hunk(f: &gigit::git::diff::FileDiff, h: usize) -> LineSel {
    let i = f.hunks[h].lines.iter().position(|l| l.text == "NEW").unwrap();
    [(h, i)].into_iter().collect()
}

#[test]
fn discard_line_in_repeated_block_hits_the_right_one() {
    let (_d, repo) = repeated_blocks_repo();
    let f = diff_file(&repo, "r.txt", DiffSide::Unstaged).unwrap().unwrap();
    assert_eq!(f.hunks.len(), 3);
    discard_lines(&repo, &f, &new_line_of_hunk(&f, 2)).unwrap();
    let content = std::fs::read_to_string(repo.join("r.txt")).unwrap();
    let first_new = content.find("NEW").unwrap();
    let m0 = content.find("m0").unwrap();
    assert!(first_new < m0, "the first block must keep its NEW");
    assert_eq!(content.matches("NEW").count(), 1);
}

#[test]
fn unstage_line_in_repeated_block_hits_the_right_one() {
    let (_d, repo) = repeated_blocks_repo();
    git(&repo, &["add", "r.txt"]);
    let f = diff_file(&repo, "r.txt", DiffSide::Staged).unwrap().unwrap();
    unstage_lines(&repo, &f, &new_line_of_hunk(&f, 2)).unwrap();
    let staged = git(&repo, &["show", ":r.txt"]);
    assert!(staged.find("NEW").unwrap() < staged.find("m0").unwrap());
    assert_eq!(staged.matches("NEW").count(), 1);
}

#[test]
fn force_push_keeps_fetched_but_unseen_remote_commits() {
    let (_d, repo, _b, bare) = repo_with_origin();
    // A collaborator pushes a commit; autofetch brings it into origin/main without merging it.
    let other_dir = tempfile::TempDir::new().unwrap();
    let other = other_dir.path().join("clone");
    git(other_dir.path(), &["clone", "-q", bare.to_str().unwrap(), "clone"]);
    git(&other, &["config", "user.name", "O"]);
    git(&other, &["config", "user.email", "o@x"]);
    write(&other, "theirs.txt", "precious\n");
    commit_all(&other, "collaborator work");
    git(&other, &["push", "-q"]);
    fetch(&repo, true).unwrap();
    commit(&repo, "rewritten", true).unwrap();
    let br = status(&repo).unwrap().branch;
    assert!(push(&repo, &br, true).is_err(), "force push must not overwrite unseen remote commits");
    let remote_log = git(&bare, &["log", "--format=%s", "main"]);
    assert!(remote_log.contains("collaborator work"));
}
