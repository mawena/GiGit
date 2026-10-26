//! Unified diff parser.

use std::path::Path;

use super::{GitError, RunOpts, run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// Followed by "\ No newline at end of file".
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub section: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// `None` means /dev/null (file added).
    pub old_path: Option<String>,
    /// `None` means /dev/null (file deleted).
    pub new_path: Option<String>,
    /// Raw header lines from `diff --git` up to the first hunk, kept verbatim to rebuild patches.
    pub header: Vec<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn path(&self) -> &str {
        self.new_path.as_deref().or(self.old_path.as_deref()).unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSide {
    Unstaged,
    Staged,
}

/// Undo git's C-style quoting (`"a\\tb"`), used for paths with unusual characters.
fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let mut bytes = Vec::new();
    let mut it = inner.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'r') => bytes.push(b'\r'),
            Some(d @ b'0'..=b'7') => {
                let mut v = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&n @ b'0'..=b'7') = it.peek() {
                        v = v * 8 + u32::from(n - b'0');
                        it.next();
                    }
                }
                bytes.push(v as u8);
            }
            Some(o) => bytes.push(o),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Path from a `---`/`+++` line value: `None` for /dev/null, prefix `a/` or `b/` removed.
fn side_path(v: &str) -> Option<String> {
    let v = unquote(v.trim_end_matches('\t'));
    if v == "/dev/null" {
        return None;
    }
    Some(v.strip_prefix("a/").or_else(|| v.strip_prefix("b/")).unwrap_or(&v).to_string())
}

/// Best-effort path from `diff --git a/X b/X` when there are no ---/+++ lines (binary, mode-only).
fn git_line_path(rest: &str) -> Option<String> {
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find("\" ")? + 1;
        return side_path(&rest[..=end]);
    }
    let rest = rest.strip_prefix("a/")?;
    let n = rest.len().checked_sub(3)? / 2;
    (rest.get(n..n + 3) == Some(" b/") && rest[..n] == rest[n + 3..]).then(|| rest[..n].to_string())
}

fn parse_range(r: &str) -> (u32, u32) {
    match r.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (r.parse().unwrap_or(0), 1),
    }
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_len) = parse_range(old);
    let (new_start, new_len) = parse_range(new);
    Some(Hunk { old_start, old_len, new_start, new_len, section: section.trim().to_string(), lines: Vec::new() })
}

pub fn parse_diff(raw: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let p = git_line_path(rest);
            files.push(FileDiff {
                old_path: p.clone(),
                new_path: p,
                header: vec![line.to_string()],
                binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if file.hunks.is_empty() && !line.starts_with("@@") {
            file.header.push(line.to_string());
            if let Some(v) = line.strip_prefix("--- ") {
                file.old_path = side_path(v);
            } else if let Some(v) = line.strip_prefix("+++ ") {
                file.new_path = side_path(v);
            } else if line.starts_with("new file mode") {
                file.old_path = None;
            } else if line.starts_with("deleted file mode") {
                file.new_path = None;
            } else if let Some(v) = line.strip_prefix("rename from ") {
                file.old_path = Some(unquote(v));
            } else if let Some(v) = line.strip_prefix("rename to ") {
                file.new_path = Some(unquote(v));
            } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
                file.binary = true;
            }
            continue;
        }
        if let Some(h) = parse_hunk_header(line) {
            old_no = h.old_start;
            new_no = h.new_start;
            file.hunks.push(h);
            continue;
        }
        let Some(hunk) = file.hunks.last_mut() else { continue };
        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            Some(b'\\') => {
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_newline = true;
                }
                continue;
            }
            None => (LineKind::Context, ""),
            _ => continue,
        };
        let (o, n) = match kind {
            LineKind::Context => (Some(old_no), Some(new_no)),
            LineKind::Removed => (Some(old_no), None),
            LineKind::Added => (None, Some(new_no)),
        };
        old_no += u32::from(o.is_some());
        new_no += u32::from(n.is_some());
        hunk.lines.push(DiffLine { kind, text: text.to_string(), old_no: o, new_no: n, no_newline: false });
    }
    files
}

pub fn diff_file(repo: &Path, path: &str, side: DiffSide) -> Result<Option<FileDiff>, GitError> {
    let ro = RunOpts { read_only: true, ..Default::default() };
    let out = match side {
        DiffSide::Staged => run(repo, &["diff", "--cached", "--no-ext-diff", "--", path], &ro)?,
        DiffSide::Unstaged => {
            let out = run(repo, &["diff", "--no-ext-diff", "--", path], &ro)?;
            let untracked = out.stdout.is_empty()
                && !run(repo, &["ls-files", "--others", "--exclude-standard", "--", path], &ro)?
                    .stdout
                    .is_empty();
            if untracked {
                let opts = RunOpts { allow_exit_1: true, ..ro };
                run(repo, &["diff", "--no-ext-diff", "--no-index", "--", "/dev/null", path], &opts)?
            } else {
                out
            }
        }
    };
    Ok(parse_diff(&String::from_utf8_lossy(&out.stdout)).into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;

    const ONE: &str = "diff --git a/src/x.rs b/src/x.rs
index 111..222 100644
--- a/src/x.rs
+++ b/src/x.rs
@@ -3,4 +3,5 @@ fn x
 a
-b
+B
+C
 c
 d
";

    #[test]
    fn parses_single_hunk_with_line_numbers() {
        let files = parse_diff(ONE);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.old_path.as_deref(), Some("src/x.rs"));
        assert_eq!(f.new_path.as_deref(), Some("src/x.rs"));
        assert_eq!(f.header.len(), 4);
        assert_eq!(f.header[0], "diff --git a/src/x.rs b/src/x.rs");
        let h = &f.hunks[0];
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len), (3, 4, 3, 5));
        assert_eq!(h.section, "fn x");
        let nums: Vec<_> = h.lines.iter().map(|l| (l.kind, l.text.as_str(), l.old_no, l.new_no)).collect();
        assert_eq!(
            nums,
            vec![
                (LineKind::Context, "a", Some(3), Some(3)),
                (LineKind::Removed, "b", Some(4), None),
                (LineKind::Added, "B", None, Some(4)),
                (LineKind::Added, "C", None, Some(5)),
                (LineKind::Context, "c", Some(5), Some(6)),
                (LineKind::Context, "d", Some(6), Some(7)),
            ]
        );
    }

    #[test]
    fn parses_header_omitted_len() {
        let f = &parse_diff("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n")[0];
        let h = &f.hunks[0];
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len), (1, 1, 1, 1));
        assert_eq!(h.section, "");
    }

    #[test]
    fn parses_no_newline_marker() {
        let f = &parse_diff(
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+a\n",
        )[0];
        let lines = &f.hunks[0].lines;
        assert_eq!(lines.len(), 2);
        assert!(lines[0].no_newline);
        assert!(!lines[1].no_newline);
    }

    #[test]
    fn parses_new_and_deleted_files() {
        let raw = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1 @@\n+x\ndiff --git a/d b/d\ndeleted file mode 100644\n--- a/d\n+++ /dev/null\n@@ -1 +0,0 @@\n-y\n";
        let files = parse_diff(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].old_path, None);
        assert_eq!(files[0].new_path.as_deref(), Some("n"));
        assert_eq!(files[1].new_path, None);
        assert_eq!(files[1].path(), "d");
    }

    #[test]
    fn parses_binary() {
        let raw = "diff --git a/x.png b/x.png\nindex 1..2 100644\nBinary files a/x.png and b/x.png differ\n";
        let f = &parse_diff(raw)[0];
        assert!(f.binary);
        assert!(f.hunks.is_empty());
        assert_eq!(f.path(), "x.png");
    }

    #[test]
    fn parses_two_files() {
        let raw = format!("{ONE}{}", ONE.replace("src/x.rs", "y.rs"));
        let files = parse_diff(&raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].path(), "y.rs");
        assert_eq!(files[1].header[0], "diff --git a/y.rs b/y.rs");
    }

    #[test]
    fn parses_path_with_spaces() {
        let raw = "diff --git a/a b/c d.txt b/a b/c d.txt\n--- a/a b/c d.txt\n+++ b/a b/c d.txt\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(parse_diff(raw)[0].path(), "a b/c d.txt");
    }

    #[test]
    fn diff_untracked_file_is_all_added() {
        let (_d, repo) = temp_repo();
        write(&repo, "new file.txt", "one\ntwo\n");
        let f = diff_file(&repo, "new file.txt", DiffSide::Unstaged).unwrap().unwrap();
        assert_eq!(f.hunks.len(), 1);
        assert!(f.hunks[0].lines.iter().all(|l| l.kind == LineKind::Added));
        assert_eq!(f.hunks[0].lines.len(), 2);
    }

    #[test]
    fn diff_staged_and_unstaged_sides() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\n");
        commit_all(&repo, "init");
        write(&repo, "f", "b\n");
        git(&repo, &["add", "f"]);
        write(&repo, "f", "c\n");
        let staged = diff_file(&repo, "f", DiffSide::Staged).unwrap().unwrap();
        let unstaged = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        assert_eq!(staged.hunks[0].lines[1].text, "b");
        assert_eq!(unstaged.hunks[0].lines[0].text, "b");
        assert_eq!(unstaged.hunks[0].lines[1].text, "c");
    }

    #[test]
    fn diff_clean_file_is_none() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\n");
        commit_all(&repo, "init");
        assert_eq!(diff_file(&repo, "f", DiffSide::Unstaged).unwrap(), None);
    }
}
