//! ungit-style graph layout.
//!
//! The first-parent chain from HEAD is the "main line": big nodes in column 0 whose commit
//! cards are always shown. Every other commit belongs to the ref that "owns" it (its
//! ideological branch) and is drawn as a small node in that branch's own column to the right.
//! Commits reachable from no ref (e.g. stashes) are left out, as in ungit.

use std::collections::HashMap;

use crate::git::log::{Commit, Ref, RefKind};

#[derive(Debug, Clone, PartialEq)]
pub struct GNode {
    /// Index into the commits the layout was built from.
    pub commit: usize,
    /// On the first-parent chain from HEAD.
    pub main: bool,
    /// Column: 0 for the main line, 1.. for other branches.
    pub slot: usize,
    /// Vertical position in layout units (main→main steps are 2, all others 1).
    pub y: f32,
    /// Name of the ref owning this commit, used for its colour.
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GEdge {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    /// y of the last node.
    pub height: f32,
    pub max_slot: usize,
    /// Commit id → node index.
    pub index: HashMap<String, usize>,
}

pub fn layout(commits: &[Commit], refs: &[Ref]) -> Layout {
    let pos: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let parents_of = |i: usize| commits[i].parents.iter().filter_map(|p| pos.get(p.as_str()).copied());

    // Ideological branches: refs claim their history in ungit's order (local before remote,
    // branches before tags, HEAD last, then oldest tip first); a claimed commit stops the walk.
    let mut order: Vec<(usize, usize)> =
        refs.iter().enumerate().filter_map(|(ri, r)| pos.get(r.target.as_str()).map(|&ci| (ri, ci))).collect();
    order.sort_by_key(|&(ri, ci)| {
        let r = &refs[ri];
        let is_branch = matches!(r.kind, RefKind::LocalBranch | RefKind::RemoteBranch);
        (r.kind == RefKind::RemoteBranch, !is_branch, r.kind == RefKind::Head, commits[ci].time, r.name.clone())
    });
    let mut owner: Vec<Option<usize>> = vec![None; commits.len()];
    let mut stack = Vec::new();
    for (ri, ci) in order {
        stack.push(ci);
        while let Some(i) = stack.pop() {
            if owner[i].is_some() {
                continue;
            }
            owner[i] = Some(ri);
            stack.extend(parents_of(i));
        }
    }

    // Main line: first-parent chain from HEAD.
    let mut main = vec![false; commits.len()];
    let head = refs.iter().find(|r| r.kind == RefKind::Head).and_then(|r| pos.get(r.target.as_str()).copied());
    let mut cur = head;
    while let Some(i) = cur {
        if main[i] {
            break;
        }
        main[i] = true;
        cur = commits[i].parents.first().and_then(|p| pos.get(p.as_str()).copied());
    }

    let kept: Vec<usize> = (0..commits.len()).filter(|&i| main[i] || owner[i].is_some()).collect();

    // Columns: each branch gets the next free column the first time it is met from the bottom.
    let mut slot_of_ref: HashMap<usize, usize> = HashMap::new();
    let mut next_slot = usize::from(head.is_some());
    let mut slots = vec![0; commits.len()];
    for &i in kept.iter().rev() {
        if main[i] {
            continue;
        }
        let r = owner[i].expect("kept side commits have an owner");
        slots[i] = *slot_of_ref.entry(r).or_insert_with(|| {
            next_slot += 1;
            next_slot - 1
        });
    }

    let mut out = Layout::default();
    let mut y = 0.0;
    for (k, &i) in kept.iter().enumerate() {
        if k > 0 {
            let prev_main = main[kept[k - 1]];
            y += if main[i] && prev_main { 2.0 } else { 1.0 };
        }
        out.index.insert(commits[i].id.clone(), k);
        out.max_slot = out.max_slot.max(slots[i]);
        out.nodes.push(GNode {
            commit: i,
            main: main[i],
            slot: slots[i],
            y,
            branch: owner[i].map(|r| refs[r].name.clone()),
        });
    }
    out.height = y;
    for (k, &i) in kept.iter().enumerate() {
        for p in &commits[i].parents {
            if let Some(&to) = out.index.get(p.as_str()) {
                out.edges.push(GEdge { from: k, to });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn c(id: &str, parents: &[&str], time: i64) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time,
            subject: String::new(),
            body: String::new(),
        }
    }

    fn r(name: &str, kind: RefKind, target: &str) -> Ref {
        Ref { name: name.into(), kind, target: target.into() }
    }

    fn head(target: &str) -> Ref {
        r("HEAD", RefKind::Head, target)
    }

    fn node<'a>(l: &'a Layout, commits: &[Commit], id: &str) -> &'a GNode {
        let i = l.index[id];
        assert_eq!(commits[l.nodes[i].commit].id, id);
        &l.nodes[i]
    }

    #[test]
    fn linear_history_is_main_line() {
        let cs = [c("c", &["b"], 3), c("b", &["a"], 2), c("a", &[], 1)];
        let l = layout(&cs, &[head("c"), r("main", RefKind::LocalBranch, "c")]);
        assert_eq!(l.nodes.len(), 3);
        assert!(l.nodes.iter().all(|n| n.main && n.slot == 0));
        let ys: Vec<f32> = l.nodes.iter().map(|n| n.y).collect();
        assert_eq!(ys, [0.0, 2.0, 4.0]);
        assert_eq!(l.edges, vec![GEdge { from: 0, to: 1 }, GEdge { from: 1, to: 2 }]);
        assert_eq!(l.nodes[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn unmerged_branch_goes_to_its_own_column() {
        // main: m2 - m1 - base ; feature: f2 - f1 - base ; HEAD on main
        let cs = [
            c("f2", &["f1"], 6),
            c("m2", &["m1"], 5),
            c("f1", &["base"], 4),
            c("m1", &["base"], 3),
            c("base", &[], 1),
        ];
        let refs = [head("m2"), r("main", RefKind::LocalBranch, "m2"), r("feature", RefKind::LocalBranch, "f2")];
        let l = layout(&cs, &refs);
        let f2 = node(&l, &cs, "f2");
        assert!(!f2.main);
        assert_eq!(f2.slot, 1);
        assert_eq!(f2.branch.as_deref(), Some("feature"));
        assert!(node(&l, &cs, "m2").main);
        assert!(node(&l, &cs, "base").main);
        assert_eq!(l.max_slot, 1);
    }

    #[test]
    fn y_spacing_follows_ungit() {
        let cs = [
            c("f2", &["f1"], 6),
            c("m2", &["m1"], 5),
            c("f1", &["base"], 4),
            c("m1", &["base"], 3),
            c("base", &[], 1),
        ];
        let refs = [head("m2"), r("main", RefKind::LocalBranch, "m2"), r("feature", RefKind::LocalBranch, "f2")];
        let l = layout(&cs, &refs);
        // f2 first (side) 0; m2 main after side +1; f1 side +1; m1 main after side +1; base main after main +2
        let ys: Vec<f32> = l.nodes.iter().map(|n| n.y).collect();
        assert_eq!(ys, [0.0, 1.0, 2.0, 3.0, 5.0]);
        assert_eq!(l.height, 5.0);
    }

    #[test]
    fn unreferenced_commits_are_dropped() {
        // "s" (a stash-like commit) is reachable from no listed ref.
        let cs = [c("s", &["b"], 9), c("b", &["a"], 2), c("a", &[], 1)];
        let l = layout(&cs, &[head("b"), r("main", RefKind::LocalBranch, "b")]);
        assert_eq!(l.nodes.len(), 2);
        assert!(!l.index.contains_key("s"));
    }

    #[test]
    fn older_branch_claims_shared_history_first() {
        // As in ungit, refs are processed oldest tip first, so "old" owns the shared base.
        let cs = [c("n", &["base"], 5), c("o", &["base"], 3), c("base", &[], 1)];
        let refs = [r("new", RefKind::LocalBranch, "n"), r("old", RefKind::LocalBranch, "o")];
        let l = layout(&cs, &refs);
        assert_eq!(node(&l, &cs, "base").branch.as_deref(), Some("old"));
        assert_eq!(node(&l, &cs, "n").branch.as_deref(), Some("new"));
    }

    #[test]
    fn local_branches_win_over_remote_and_tags() {
        let cs = [c("a", &[], 1)];
        let refs = [
            r("v1", RefKind::Tag, "a"),
            r("origin/main", RefKind::RemoteBranch, "a"),
            r("main", RefKind::LocalBranch, "a"),
        ];
        let l = layout(&cs, &refs);
        assert_eq!(l.nodes[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn merge_edges_reach_both_parents() {
        let cs = [c("m", &["a", "b"], 3), c("b", &["a"], 2), c("a", &[], 1)];
        let refs = [head("m"), r("main", RefKind::LocalBranch, "m")];
        let l = layout(&cs, &refs);
        let (im, ia, ib) = (l.index["m"], l.index["a"], l.index["b"]);
        assert!(l.edges.contains(&GEdge { from: im, to: ia }));
        assert!(l.edges.contains(&GEdge { from: im, to: ib }));
        assert!(l.edges.contains(&GEdge { from: ib, to: ia }));
        assert!(!node(&l, &cs, "b").main);
    }

    #[test]
    fn no_head_means_no_main_line() {
        let cs = [c("a", &[], 1)];
        let l = layout(&cs, &[r("x", RefKind::LocalBranch, "a")]);
        assert!(!l.nodes[0].main);
        assert_eq!(l.nodes[0].slot, 0);
    }

    #[test]
    fn layout_10k_linear_under_200ms() {
        let ids: Vec<String> = (0..10_000).map(|i| format!("{i}")).collect();
        let commits: Vec<Commit> = (0..10_000)
            .map(|i| if i + 1 < 10_000 { c(&ids[i], &[&ids[i + 1]], 10_000 - i as i64) } else { c(&ids[i], &[], 0) })
            .collect();
        let refs = [head("0"), r("main", RefKind::LocalBranch, "0")];
        let start = Instant::now();
        let l = layout(&commits, &refs);
        assert_eq!(l.nodes.len(), 10_000);
        assert!(start.elapsed().as_millis() < 200, "{:?}", start.elapsed());
    }
}
