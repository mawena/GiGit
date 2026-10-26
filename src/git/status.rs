//! Parser for `git status --porcelain=v2 -z --branch`.

use std::path::Path;

use super::{GitError, RunOpts, run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub orig_path: Option<String>,
    pub index: FileState,
    pub worktree: FileState,
    pub conflicted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// `None` when HEAD is detached.
    pub head: Option<String>,
    /// `None` when the branch has no commit yet.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: BranchInfo,
    pub entries: Vec<FileEntry>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| {
            e.conflicted || !matches!(e.index, FileState::Unmodified | FileState::Untracked | FileState::Ignored)
        })
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries
            .iter()
            .filter(|e| e.conflicted || !matches!(e.worktree, FileState::Unmodified | FileState::Ignored))
    }
}

fn state(c: u8) -> FileState {
    match c {
        b'M' => FileState::Modified,
        b'A' => FileState::Added,
        b'D' => FileState::Deleted,
        b'R' => FileState::Renamed,
        b'C' => FileState::Copied,
        b'T' => FileState::TypeChanged,
        _ => FileState::Unmodified,
    }
}

fn entry(xy: &str, path: &str, orig_path: Option<String>, conflicted: bool) -> Option<FileEntry> {
    let xy = xy.as_bytes();
    if xy.len() != 2 {
        return None;
    }
    Some(FileEntry {
        path: path.to_string(),
        orig_path,
        index: state(xy[0]),
        worktree: state(xy[1]),
        conflicted,
    })
}

pub fn parse_status(raw: &[u8]) -> Status {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0');
    let mut st = Status::default();
    while let Some(rec) = fields.next() {
        if let Some(header) = rec.strip_prefix("# ") {
            let (key, val) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => st.branch.oid = (val != "(initial)").then(|| val.to_string()),
                "branch.head" => st.branch.head = (val != "(detached)").then(|| val.to_string()),
                "branch.upstream" => st.branch.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            st.branch.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            st.branch.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let e = match rec.as_bytes().first() {
            Some(b'1') => {
                let parts: Vec<&str> = rec.splitn(9, ' ').collect();
                (parts.len() == 9).then(|| entry(parts[1], parts[8], None, false)).flatten()
            }
            Some(b'2') => {
                let parts: Vec<&str> = rec.splitn(10, ' ').collect();
                let orig = fields.next().map(str::to_string);
                (parts.len() == 10).then(|| entry(parts[1], parts[9], orig, false)).flatten()
            }
            Some(b'u') => {
                let parts: Vec<&str> = rec.splitn(11, ' ').collect();
                (parts.len() == 11).then(|| entry(parts[1], parts[10], None, true)).flatten()
            }
            Some(b'?') | Some(b'!') if rec.len() > 2 => Some(FileEntry {
                path: rec[2..].to_string(),
                orig_path: None,
                index: FileState::Unmodified,
                worktree: if rec.starts_with('?') { FileState::Untracked } else { FileState::Ignored },
                conflicted: false,
            }),
            _ => None,
        };
        st.entries.extend(e);
    }
    st
}

pub fn status(repo: &Path) -> Result<Status, GitError> {
    let out = run(
        repo,
        &["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"],
        &RunOpts { read_only: true, ..Default::default() },
    )?;
    Ok(parse_status(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Status {
        parse_status(s.replace('|', "\0").as_bytes())
    }

    #[test]
    fn parses_branch_ahead_behind() {
        let s = p("# branch.oid abc|# branch.head main|# branch.upstream origin/main|# branch.ab +2 -1|");
        assert_eq!(s.branch.head.as_deref(), Some("main"));
        assert_eq!(s.branch.oid.as_deref(), Some("abc"));
        assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.branch.ahead, s.branch.behind), (2, 1));
    }

    #[test]
    fn parses_detached_head() {
        let s = p("# branch.oid abc|# branch.head (detached)|");
        assert_eq!(s.branch.head, None);
    }

    #[test]
    fn parses_branch_without_upstream() {
        let s = p("# branch.oid abc|# branch.head main|");
        assert_eq!(s.branch.upstream, None);
        assert_eq!((s.branch.ahead, s.branch.behind), (0, 0));
    }

    #[test]
    fn parses_initial_oid() {
        let s = p("# branch.oid (initial)|# branch.head main|");
        assert_eq!(s.branch.oid, None);
        assert_eq!(s.branch.head.as_deref(), Some("main"));
    }

    #[test]
    fn parses_ordinary_modified() {
        let s = p("1 .M N... 100644 100644 100644 h1 h2 src/a.rs|");
        assert_eq!(s.entries.len(), 1);
        let e = &s.entries[0];
        assert_eq!(e.path, "src/a.rs");
        assert_eq!(e.index, FileState::Unmodified);
        assert_eq!(e.worktree, FileState::Modified);
        assert!(!e.conflicted);
    }

    #[test]
    fn parses_rename() {
        let s = p("2 R. N... 100644 100644 100644 h1 h2 R100 new.rs|old.rs|");
        let e = &s.entries[0];
        assert_eq!(e.path, "new.rs");
        assert_eq!(e.orig_path.as_deref(), Some("old.rs"));
        assert_eq!(e.index, FileState::Renamed);
    }

    #[test]
    fn parses_unmerged() {
        let s = p("u UU N... 100644 100644 100644 100644 h1 h2 h3 c.rs|");
        assert_eq!(s.entries[0].path, "c.rs");
        assert!(s.entries[0].conflicted);
    }

    #[test]
    fn parses_untracked() {
        let s = p("? notes/é t.md|");
        assert_eq!(s.entries[0].path, "notes/é t.md");
        assert_eq!(s.entries[0].worktree, FileState::Untracked);
        assert_eq!(s.entries[0].index, FileState::Unmodified);
    }

    #[test]
    fn parses_paths_with_spaces_and_unicode() {
        let path = "dir with space/fïchier \"q\".txt";
        let s = p(&format!("1 A. N... 000000 100644 100644 h1 h2 {path}|? autre\nligne.txt|"));
        assert_eq!(s.entries[0].path, path);
        assert_eq!(s.entries[0].index, FileState::Added);
        assert_eq!(s.entries[1].path, "autre\nligne.txt");
    }

    #[test]
    fn unknown_records_are_skipped() {
        let s = p("! ignored.txt|zzz weird|1 .M N... 100644 100644 100644 h1 h2 a|");
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0].worktree, FileState::Ignored);
        assert_eq!(s.entries[1].path, "a");
    }

    #[test]
    fn staged_and_unstaged_split() {
        let s = p("1 MM N... 100644 100644 100644 h1 h2 both|1 M. N... 100644 100644 100644 h1 h2 idx|? new|");
        let staged: Vec<_> = s.staged().map(|e| e.path.as_str()).collect();
        let unstaged: Vec<_> = s.unstaged().map(|e| e.path.as_str()).collect();
        assert_eq!(staged, ["both", "idx"]);
        assert_eq!(unstaged, ["both", "new"]);
    }
}
