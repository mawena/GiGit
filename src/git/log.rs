//! Commit history and references.

use std::path::Path;

use super::{GitError, RunOpts, run, run_str};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Short name: `main`, `origin/main`, `v1.0`, `HEAD`.
    pub name: String,
    pub kind: RefKind,
    /// Commit id (annotated tags are peeled).
    pub target: String,
}

pub const LOG_FORMAT: &str = "%H%x00%P%x00%an%x00%ae%x00%at%x00%s%x00%b%x1e";

pub fn parse_log(raw: &[u8]) -> Vec<Commit> {
    String::from_utf8_lossy(raw)
        .split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let f: Vec<&str> = rec.splitn(7, '\0').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Commit {
                id: f[0].to_string(),
                parents: f[1].split_whitespace().map(str::to_string).collect(),
                author: f[2].to_string(),
                email: f[3].to_string(),
                time: f[4].parse().unwrap_or(0),
                subject: f[5].to_string(),
                body: f[6].trim_end().to_string(),
            })
        })
        .collect()
}

pub fn log(repo: &Path, skip: usize, limit: usize) -> Result<Vec<Commit>, GitError> {
    let format = format!("--format={LOG_FORMAT}");
    let skip = format!("--skip={skip}");
    let limit = format!("-n{limit}");
    let opts = RunOpts { read_only: true, ..Default::default() };
    match run(repo, &["log", "--all", "--date-order", &format, &skip, &limit], &opts) {
        Ok(out) => Ok(parse_log(&out.stdout)),
        // An unborn repository has no history: that is not an error.
        Err(_) if refs(repo)?.is_empty() => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn parse_refs(raw: &str) -> Vec<Ref> {
    raw.lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let (name, oid, peeled) = (f.next()?, f.next()?, f.next().unwrap_or(""));
            let (kind, short) = if let Some(n) = name.strip_prefix("refs/heads/") {
                (RefKind::LocalBranch, n)
            } else if let Some(n) = name.strip_prefix("refs/remotes/") {
                if n.ends_with("/HEAD") {
                    return None;
                }
                (RefKind::RemoteBranch, n)
            } else if let Some(n) = name.strip_prefix("refs/tags/") {
                (RefKind::Tag, n)
            } else {
                return None;
            };
            let target = if peeled.is_empty() { oid } else { peeled };
            Some(Ref { name: short.to_string(), kind, target: target.to_string() })
        })
        .collect()
}

pub fn refs(repo: &Path) -> Result<Vec<Ref>, GitError> {
    let opts = RunOpts { read_only: true, ..Default::default() };
    let raw = run_str(
        repo,
        &["for-each-ref", "--format=%(refname)%00%(objectname)%00%(*objectname)"],
        &opts,
    )?;
    let mut refs = parse_refs(&raw);
    if let Ok(head) = run_str(repo, &["rev-parse", "--verify", "-q", "HEAD"], &opts) {
        refs.insert(0, Ref { name: "HEAD".into(), kind: RefKind::Head, target: head.trim().to_string() });
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_records() {
        let raw = "m1\0p1 p2\0Ann\0ann@x.org\01700000000\0Merge it\0line one\nline two\n\x1e\nc2\0\0Bob\0b@x\01600000000\0Root\0\x1e\n";
        let commits = parse_log(raw.as_bytes());
        assert_eq!(commits.len(), 2);
        let m = &commits[0];
        assert_eq!(m.id, "m1");
        assert_eq!(m.parents, ["p1", "p2"]);
        assert_eq!((m.author.as_str(), m.email.as_str(), m.time), ("Ann", "ann@x.org", 1_700_000_000));
        assert_eq!(m.subject, "Merge it");
        assert_eq!(m.body, "line one\nline two");
    }

    #[test]
    fn parses_root_commit() {
        let raw = "c2\0\0Bob\0b@x\01600000000\0Root\0\x1e\n";
        let c = &parse_log(raw.as_bytes())[0];
        assert!(c.parents.is_empty());
        assert_eq!(c.body, "");
    }

    #[test]
    fn parses_refs_kinds() {
        let raw = "refs/heads/main\0aaa\0\nrefs/remotes/origin/main\0bbb\0\nrefs/remotes/origin/HEAD\0bbb\0\nrefs/tags/v1\0tagobj\0ccc\nrefs/tags/light\0ddd\0\nrefs/stash\0eee\0\n";
        let refs = parse_refs(raw);
        assert_eq!(
            refs,
            vec![
                Ref { name: "main".into(), kind: RefKind::LocalBranch, target: "aaa".into() },
                Ref { name: "origin/main".into(), kind: RefKind::RemoteBranch, target: "bbb".into() },
                Ref { name: "v1".into(), kind: RefKind::Tag, target: "ccc".into() },
                Ref { name: "light".into(), kind: RefKind::Tag, target: "ddd".into() },
            ]
        );
    }

    use crate::git::testutil::*;

    #[test]
    fn log_on_empty_repo_is_empty() {
        let (_d, repo) = temp_repo();
        assert_eq!(log(&repo, 0, 500).unwrap(), vec![]);
        assert_eq!(refs(&repo).unwrap(), vec![]);
    }

    #[test]
    fn log_pagination_skip_limit() {
        let (_d, repo) = temp_repo();
        for i in 0..7 {
            write(&repo, "f.txt", &i.to_string());
            commit_all(&repo, &format!("c{i}"));
        }
        let all = log(&repo, 0, 7).unwrap();
        assert_eq!(all.len(), 7);
        assert_eq!(log(&repo, 0, 3).unwrap(), all[0..3]);
        assert_eq!(log(&repo, 3, 3).unwrap(), all[3..6]);
        assert_eq!(all[0].subject, "c6");
    }

    #[test]
    fn refs_include_head_and_branch() {
        let (_d, repo) = temp_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "one");
        let refs = refs(&repo).unwrap();
        assert!(refs.iter().any(|r| r.kind == RefKind::Head));
        assert!(refs.iter().any(|r| r.kind == RefKind::LocalBranch && r.name == "main"));
    }
}
