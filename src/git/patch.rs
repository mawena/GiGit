//! Builds partial patches from a selection of diff lines (hunk / line staging).

use std::collections::HashSet;
use std::path::Path;

use super::diff::{DiffLine, FileDiff, LineKind};
use super::{GitError, RunOpts, run};

/// Selected `(hunk index, line index)` pairs.
pub type LineSel = HashSet<(usize, usize)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchMode {
    /// Stage selected lines from an unstaged diff (`apply --cached`).
    Forward,
    /// Unstage from a staged diff (`apply --cached --reverse`) or discard from an unstaged diff (`apply --reverse`).
    Reverse,
}

const NO_NEWLINE: &[u8] = b"\\ No newline at end of file\n";

/// One emitted patch line: sign (' ', '+', '-'), exact bytes, followed by the no-newline marker.
type Out<'a> = (char, &'a [u8], bool);

/// Lays out one block of consecutive change lines.
///
/// The old side (context + removals) must keep the old file's order and the new side
/// (context + additions) the new file's order. Forward: kept removals become context and the
/// selected additions go right after the last selected removal. Reverse: kept additions become
/// context and the selected removals go right before the first selected addition.
fn layout_block<'a>(lines: &[(&'a DiffLine, bool)], mode: PatchMode, out: &mut Vec<Out<'a>>) {
    let (fixed_kind, moving_kind) = match mode {
        PatchMode::Forward => (LineKind::Removed, LineKind::Added),
        PatchMode::Reverse => (LineKind::Added, LineKind::Removed),
    };
    let sign = |k: LineKind| if k == LineKind::Added { '+' } else { '-' };
    let fixed: Vec<Out> = lines
        .iter()
        .filter(|(l, _)| l.kind == fixed_kind)
        .map(|(l, sel)| (if *sel { sign(l.kind) } else { ' ' }, l.raw.as_slice(), l.no_newline))
        .collect();
    let moving: Vec<Out> = lines
        .iter()
        .filter(|(l, sel)| l.kind == moving_kind && *sel)
        .map(|(l, _)| (sign(l.kind), l.raw.as_slice(), l.no_newline))
        .collect();
    let split = match mode {
        PatchMode::Forward => fixed.iter().rposition(|o| o.0 != ' ').map_or(fixed.len(), |k| k + 1),
        PatchMode::Reverse => fixed.iter().position(|o| o.0 != ' ').unwrap_or(0),
    };
    out.extend_from_slice(&fixed[..split]);
    out.extend(moving);
    out.extend_from_slice(&fixed[split..]);
}

/// A context line flagged "no newline at end of file" must stay last on both sides. When lines
/// of one side follow it, it is split into a removal and an addition so only the side where it
/// really is last keeps the marker.
fn fix_final_newline<'a>(out: Vec<Out<'a>>) -> Vec<Out<'a>> {
    let mut fixed = Vec::with_capacity(out.len() + 2);
    let mut tail = Vec::new();
    for (i, &(sign, text, nonl)) in out.iter().enumerate() {
        if sign != ' ' || !nonl {
            fixed.push((sign, text, nonl));
            continue;
        }
        let later_old = out[i + 1..].iter().any(|o| o.0 == '-');
        let later_new = out[i + 1..].iter().any(|o| o.0 == '+');
        if later_old {
            fixed.push(('-', text, false));
            tail.push(('+', text, true));
        } else if later_new {
            fixed.push(('-', text, true));
            fixed.push(('+', text, false));
        } else {
            fixed.push((sign, text, nonl));
        }
    }
    fixed.extend(tail);
    fixed
}

fn modification_header(header: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let value = |prefix: &[u8]| {
        header.iter().find_map(|l| l.strip_prefix(prefix)).filter(|v| *v != b"/dev/null").map(<[u8]>::to_vec)
    };
    let (old, new) = (value(b"--- "), value(b"+++ "));
    let swap = |v: &[u8], from: &[u8], to: &[u8]| -> Vec<u8> {
        let (quote, rest) = match v.strip_prefix(b"\"") {
            Some(q) => (&b"\""[..], q),
            None => (&b""[..], v),
        };
        [quote, to, rest.strip_prefix(from).unwrap_or(rest)].concat()
    };
    let old = old.unwrap_or_else(|| swap(new.as_deref().unwrap_or_default(), b"b/", b"a/"));
    let new = new.unwrap_or_else(|| swap(&old, b"a/", b"b/"));
    header
        .iter()
        .filter(|l| !(l.starts_with(b"new file mode") || l.starts_with(b"deleted file mode") || l.starts_with(b"index ")))
        .map(|l| {
            if l.starts_with(b"--- ") {
                [&b"--- "[..], &old].concat()
            } else if l.starts_with(b"+++ ") {
                [&b"+++ "[..], &new].concat()
            } else {
                l.clone()
            }
        })
        .collect()
}

pub fn build_patch(file: &FileDiff, sel: &LineSel, mode: PatchMode) -> Option<Vec<u8>> {
    let mut body: Vec<u8> = Vec::new();
    let mut offset: i64 = 0;
    for (h, hunk) in file.hunks.iter().enumerate() {
        let selected = |i: usize| hunk.lines[i].kind != LineKind::Context && sel.contains(&(h, i));
        if !(0..hunk.lines.len()).any(selected) {
            continue;
        }
        let mut out: Vec<Out> = Vec::new();
        let mut i = 0;
        while i < hunk.lines.len() {
            let l = &hunk.lines[i];
            if l.kind == LineKind::Context {
                out.push((' ', l.raw.as_slice(), l.no_newline));
                i += 1;
                continue;
            }
            let start = i;
            while i < hunk.lines.len() && hunk.lines[i].kind != LineKind::Context {
                i += 1;
            }
            let block: Vec<_> = (start..i).map(|j| (&hunk.lines[j], selected(j))).collect();
            layout_block(&block, mode, &mut out);
        }
        let out = fix_final_newline(out);
        let old_len = out.iter().filter(|o| o.0 != '+').count() as i64;
        let new_len = out.iter().filter(|o| o.0 != '-').count() as i64;
        // Anchor on the side git will search: the old side when applying forward, the new side
        // (the real current content, including unselected earlier hunks) when applying in reverse.
        let (old_start, new_start) = match mode {
            PatchMode::Forward => (i64::from(hunk.old_start), i64::from(hunk.old_start) + offset),
            PatchMode::Reverse => (i64::from(hunk.new_start) - offset, i64::from(hunk.new_start)),
        };
        body.extend_from_slice(format!("@@ -{old_start},{old_len} +{new_start},{new_len} @@\n").as_bytes());
        for (sign, text, nonl) in out {
            body.push(sign as u8);
            body.extend_from_slice(text);
            body.push(b'\n');
            if nonl {
                body.extend_from_slice(NO_NEWLINE);
            }
        }
        offset += new_len - old_len;
    }
    if body.is_empty() {
        return None;
    }
    let complete = file.hunks.iter().enumerate().all(|(h, hunk)| {
        hunk.lines.iter().enumerate().all(|(i, l)| l.kind == LineKind::Context || sel.contains(&(h, i)))
    });
    // A partial patch of a created (reverse) or deleted (forward) file leaves the file in place on
    // both sides, so it must be expressed as a plain modification.
    let as_modification = !complete
        && match mode {
            PatchMode::Forward => file.new_path.is_none(),
            PatchMode::Reverse => file.old_path.is_none(),
        };
    let header = if as_modification { modification_header(&file.header) } else { file.header.clone() };
    let mut patch = header.join(&b'\n');
    patch.push(b'\n');
    patch.extend_from_slice(&body);
    Some(patch)
}

/// `git apply [--cached] [--reverse] --recount -` with the patch on stdin.
pub fn apply_patch(repo: &Path, patch: &[u8], cached: bool, reverse: bool) -> Result<(), GitError> {
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    if cached {
        args.push("--cached");
    }
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    let opts = RunOpts { stdin: Some(patch.to_vec()), ..Default::default() };
    run(repo, &args, &opts).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::{DiffSide, diff_file, parse_diff};
    use crate::git::testutil::*;

    const HDR: &str = "diff --git a/f b/f\n--- a/f\n+++ b/f\n";

    fn file(body: &str) -> FileDiff {
        parse_diff(format!("{HDR}{body}").as_bytes()).remove(0)
    }

    fn text(p: Option<Vec<u8>>) -> String {
        String::from_utf8(p.expect("a patch")).unwrap()
    }

    fn sel(pairs: &[(usize, usize)]) -> LineSel {
        pairs.iter().copied().collect()
    }

    fn all_changes(f: &FileDiff) -> LineSel {
        let mut s = LineSel::new();
        for (h, hunk) in f.hunks.iter().enumerate() {
            for (i, l) in hunk.lines.iter().enumerate() {
                if l.kind != crate::git::diff::LineKind::Context {
                    s.insert((h, i));
                }
            }
        }
        s
    }

    #[test]
    fn whole_hunk_selection_reproduces_hunk() {
        let body = "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        let f = file(body);
        let p = text(build_patch(&f, &all_changes(&f), PatchMode::Forward));
        assert_eq!(p, format!("{HDR}{body}"));
    }

    #[test]
    fn forward_unselected_removed_becomes_context() {
        let f = file("@@ -1 +1 @@\n-a\n+b\n");
        let p = text(build_patch(&f, &sel(&[(0, 1)]), PatchMode::Forward));
        assert_eq!(p, format!("{HDR}@@ -1,1 +1,2 @@\n a\n+b\n"));
    }

    #[test]
    fn reverse_unselected_added_becomes_context() {
        let f = file("@@ -1 +1 @@\n-a\n+b\n");
        let p = text(build_patch(&f, &sel(&[(0, 0)]), PatchMode::Reverse));
        assert_eq!(p, format!("{HDR}@@ -1,2 +1,1 @@\n-a\n b\n"));
    }

    #[test]
    fn forward_unselected_added_is_dropped() {
        let f = file("@@ -1,1 +1,3 @@\n a\n+b\n+c\n");
        let p = text(build_patch(&f, &sel(&[(0, 2)]), PatchMode::Forward));
        assert_eq!(p, format!("{HDR}@@ -1,1 +1,2 @@\n a\n+c\n"));
    }

    #[test]
    fn second_hunk_new_start_uses_offset() {
        let f = file("@@ -1,1 +1,3 @@\n a\n+x\n+y\n@@ -10,2 +12,2 @@\n p\n-q\n+Q\n");
        let p = text(build_patch(&f, &sel(&[(0, 1), (0, 2), (1, 2)]), PatchMode::Forward));
        assert!(p.contains("@@ -1,1 +1,3 @@\n"), "{p}");
        // q stays (unselected removal ⇒ context), Q is added: old 2 lines, new 3 lines, shifted by +2.
        assert!(p.contains("@@ -10,2 +12,3 @@\n p\n q\n+Q\n"), "{p}");
    }

    #[test]
    fn unselected_hunk_is_skipped_and_does_not_shift() {
        let f = file("@@ -1,1 +1,3 @@\n a\n+x\n+y\n@@ -10,2 +12,2 @@\n p\n-q\n+Q\n");
        let p = text(build_patch(&f, &sel(&[(1, 1), (1, 2)]), PatchMode::Forward));
        assert!(!p.contains("@@ -1,"), "{p}");
        assert!(p.contains("@@ -10,2 +10,2 @@\n p\n-q\n+Q\n"), "{p}");
    }

    #[test]
    fn nothing_selected_is_none() {
        let f = file("@@ -1 +1 @@\n-a\n+b\n");
        assert_eq!(build_patch(&f, &LineSel::new(), PatchMode::Forward), None);
        // Selecting only context lines counts as nothing.
        let f = file("@@ -1,2 +1,2 @@\n c\n-a\n+b\n");
        assert_eq!(build_patch(&f, &sel(&[(0, 0)]), PatchMode::Forward), None);
    }

    #[test]
    fn no_newline_marker_kept() {
        let f = file("@@ -1 +1 @@\n-a\n+x\n\\ No newline at end of file\n");
        let p = text(build_patch(&f, &all_changes(&f), PatchMode::Forward));
        assert!(p.ends_with("+x\n\\ No newline at end of file\n"), "{p}");
    }

    // --- integration with real git ---

    fn staged_content(repo: &Path, path: &str) -> String {
        git(repo, &["show", &format!(":{path}")])
    }

    #[test]
    fn stage_one_line_of_two() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\nb\n");
        commit_all(&repo, "init");
        write(&repo, "f", "A\nB\n");
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        // lines: -a -b +A +B ; stage a→A only
        let p = build_patch(&f, &sel(&[(0, 0), (0, 2)]), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(staged_content(&repo, "f"), "A\nb\n");
        assert_eq!(std::fs::read_to_string(repo.join("f")).unwrap(), "A\nB\n");
    }

    #[test]
    fn unstage_one_line() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\nb\n");
        commit_all(&repo, "init");
        write(&repo, "f", "A\nB\n");
        git(&repo, &["add", "f"]);
        let f = diff_file(&repo, "f", DiffSide::Staged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 1), (0, 3)]), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, true, true).unwrap();
        assert_eq!(staged_content(&repo, "f"), "A\nb\n");
    }

    #[test]
    fn discard_one_line() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\nb\n");
        commit_all(&repo, "init");
        write(&repo, "f", "A\nB\n");
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 1), (0, 3)]), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("f")).unwrap(), "A\nb\n");
    }

    #[test]
    fn stage_part_of_untracked_file() {
        let (_d, repo) = temp_repo();
        write(&repo, "base", "x\n");
        commit_all(&repo, "init");
        write(&repo, "n.txt", "one\ntwo\nthree\n");
        let f = diff_file(&repo, "n.txt", DiffSide::Unstaged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 0), (0, 2)]), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(staged_content(&repo, "n.txt"), "one\nthree\n");
    }

    #[test]
    fn stage_line_after_missing_final_newline() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a");
        commit_all(&repo, "init");
        write(&repo, "f", "a\nb\n");
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        // -a (no newline) +a +b ; stage only +b
        let p = build_patch(&f, &sel(&[(0, 2)]), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(staged_content(&repo, "f"), "a\nb\n");
    }

    #[test]
    fn discard_line_before_missing_final_newline() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\n");
        commit_all(&repo, "init");
        write(&repo, "f", "a\nb\nc");
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        // -a +a +b +c(no newline) : discard only "+b"
        let idx = f.hunks[0].lines.iter().position(|l| l.text == "b").unwrap();
        let p = build_patch(&f, &sel(&[(0, idx)]), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("f")).unwrap(), "a\nc");
    }

    #[test]
    fn unstage_part_of_new_file() {
        let (_d, repo) = temp_repo();
        write(&repo, "base", "x\n");
        commit_all(&repo, "init");
        write(&repo, "n.txt", "one\ntwo\nthree\n");
        git(&repo, &["add", "n.txt"]);
        let f = diff_file(&repo, "n.txt", DiffSide::Staged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 1)]), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, true, true).unwrap();
        assert_eq!(staged_content(&repo, "n.txt"), "one\nthree\n");
    }

    #[test]
    fn discard_part_of_untracked_file() {
        let (_d, repo) = temp_repo();
        write(&repo, "base", "x\n");
        commit_all(&repo, "init");
        write(&repo, "n.txt", "one\ntwo\nthree\n");
        let f = diff_file(&repo, "n.txt", DiffSide::Unstaged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 1)]), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, false, true).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("n.txt")).unwrap(), "one\nthree\n");
    }

    #[test]
    fn stage_part_of_deletion() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "one\ntwo\nthree\n");
        commit_all(&repo, "init");
        std::fs::remove_file(repo.join("f")).unwrap();
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        let p = build_patch(&f, &sel(&[(0, 1)]), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(staged_content(&repo, "f"), "one\nthree\n");
        assert!(!repo.join("f").exists());
    }

    #[test]
    fn full_selection_of_deletion_stages_deletion() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "one\ntwo\n");
        write(&repo, "keep", "k\n");
        commit_all(&repo, "init");
        std::fs::remove_file(repo.join("f")).unwrap();
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        let p = build_patch(&f, &all_changes(&f), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(git(&repo, &["diff", "--cached", "--name-status"]), "D\tf\n");
    }

    #[test]
    fn stage_line_inside_pure_addition_hunk() {
        let (_d, repo) = temp_repo();
        write(&repo, "f", "a\nb\n");
        commit_all(&repo, "init");
        write(&repo, "f", "a\nb\nc\nd\n");
        let f = diff_file(&repo, "f", DiffSide::Unstaged).unwrap().unwrap();
        let d = f.hunks[0].lines.iter().position(|l| l.text == "d").unwrap();
        let p = build_patch(&f, &sel(&[(0, d)]), PatchMode::Forward).unwrap();
        apply_patch(&repo, &p, true, false).unwrap();
        assert_eq!(staged_content(&repo, "f"), "a\nb\nd\n");
        // and unstage it again
        let f = diff_file(&repo, "f", DiffSide::Staged).unwrap().unwrap();
        let p = build_patch(&f, &all_changes(&f), PatchMode::Reverse).unwrap();
        apply_patch(&repo, &p, true, true).unwrap();
        assert_eq!(staged_content(&repo, "f"), "a\nb\n");
    }
}
