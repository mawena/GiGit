//! Lane layout for the commit graph.

use crate::git::log::Commit;

/// A segment drawn from this row's node centre (or lane) to the next row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from_lane: usize,
    pub to_lane: usize,
    pub color: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub lane: usize,
    pub color: usize,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub rows: Vec<Row>,
    pub max_lanes: usize,
}

/// Assigns each commit (children before parents) a lane and computes the segments to the next row.
///
/// `lanes[j]` holds the commit id lane `j` is waiting for and its colour. When several lanes wait
/// for the same commit they converge onto the one the commit is drawn on: the previous row's
/// edges pointing at the other lanes are redirected to it.
pub fn layout(commits: &[Commit]) -> Layout {
    let mut lanes: Vec<Option<(&str, usize)>> = Vec::new();
    let mut next_color = 0;
    let mut out = Layout::default();
    for c in commits {
        let targets: Vec<usize> =
            (0..lanes.len()).filter(|&j| lanes[j].is_some_and(|(id, _)| id == c.id)).collect();
        let (lane, color) = match targets.first() {
            Some(&j) => (j, lanes[j].map_or(0, |(_, col)| col)),
            None => {
                let j = lanes.iter().position(Option::is_none).unwrap_or_else(|| {
                    lanes.push(None);
                    lanes.len() - 1
                });
                next_color += 1;
                (j, next_color - 1)
            }
        };
        if let Some(prev) = out.rows.last_mut() {
            for e in &mut prev.edges {
                if targets.contains(&e.to_lane) {
                    e.to_lane = lane;
                }
            }
        }
        for &j in &targets {
            lanes[j] = None;
        }
        lanes[lane] = c.parents.first().map(|p| (p.as_str(), color));

        let mut edges = Vec::new();
        let mut from_node = vec![lane];
        for p in c.parents.iter().skip(1) {
            match lanes.iter().position(|l| l.is_some_and(|(id, _)| id == p)) {
                Some(k) => edges.push(Edge { from_lane: lane, to_lane: k, color: lanes[k].map_or(0, |l| l.1) }),
                None => {
                    let k = lanes.iter().position(Option::is_none).unwrap_or_else(|| {
                        lanes.push(None);
                        lanes.len() - 1
                    });
                    lanes[k] = Some((p.as_str(), next_color));
                    edges.push(Edge { from_lane: lane, to_lane: k, color: next_color });
                    next_color += 1;
                    from_node.push(k);
                }
            }
        }
        if lanes[lane].is_some() {
            edges.push(Edge { from_lane: lane, to_lane: lane, color });
        }
        for (j, l) in lanes.iter().enumerate() {
            if let Some((_, col)) = l
                && !from_node.contains(&j)
            {
                edges.push(Edge { from_lane: j, to_lane: j, color: *col });
            }
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        out.max_lanes = out.max_lanes.max(lanes.len()).max(lane + 1);
        out.rows.push(Row { lane, color, edges });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn c(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            subject: String::new(),
            body: String::new(),
        }
    }

    fn has_edge(row: &Row, from: usize, to: usize) -> bool {
        row.edges.iter().any(|e| e.from_lane == from && e.to_lane == to)
    }

    #[test]
    fn linear_history_single_lane() {
        let l = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(l.rows.iter().all(|r| r.lane == 0));
        assert_eq!(l.max_lanes, 1);
        assert!(has_edge(&l.rows[0], 0, 0));
    }

    #[test]
    fn branch_and_merge() {
        let l = layout(&[c("m", &["a", "b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(l.rows[0].lane, 0);
        assert!(has_edge(&l.rows[0], 0, 0));
        assert!(has_edge(&l.rows[0], 0, 1));
        assert_eq!(l.rows[1].lane, 1);
        assert!(has_edge(&l.rows[1], 1, 0), "{:?}", l.rows[1]);
        assert_eq!(l.rows[2].lane, 0);
        assert_eq!(l.max_lanes, 2);
    }

    #[test]
    fn two_heads_converge() {
        let l = layout(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(l.rows[0].lane, 0);
        assert_eq!(l.rows[1].lane, 1);
        assert_eq!(l.rows[2].lane, 0);
        // x's line passes through row y on lane 0, y joins a on lane 0.
        assert!(has_edge(&l.rows[1], 0, 0));
        assert!(has_edge(&l.rows[1], 1, 0), "{:?}", l.rows[1]);
    }

    #[test]
    fn root_ends_lane() {
        let l = layout(&[c("b", &["a"]), c("a", &[])]);
        assert!(l.rows[1].edges.is_empty());
    }

    #[test]
    fn unrelated_parent_not_in_page_keeps_lane_open() {
        // Page cut: parent "z" is not loaded yet; the lane continues downward.
        let l = layout(&[c("b", &["z"])]);
        assert!(has_edge(&l.rows[0], 0, 0));
    }

    #[test]
    fn colors_stable_along_lane() {
        let l = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(l.rows.iter().all(|r| r.color == l.rows[0].color));
    }

    #[test]
    fn side_branch_gets_its_own_color() {
        let l = layout(&[c("m", &["a", "b"]), c("b", &["a"]), c("a", &[])]);
        assert_ne!(l.rows[0].color, l.rows[1].color);
        let merge_edge = l.rows[0].edges.iter().find(|e| e.to_lane == 1).unwrap();
        assert_eq!(merge_edge.color, l.rows[1].color);
    }

    #[test]
    fn freed_lane_is_reused() {
        // m merges b; after b joins a, a later branch reuses lane 1 rather than opening lane 2.
        let l = layout(&[
            c("m", &["a", "b"]),
            c("b", &["a"]),
            c("a", &["r"]),
            c("q", &["r"]),
            c("r", &[]),
        ]);
        assert_eq!(l.rows[3].lane, 1);
        assert_eq!(l.max_lanes, 2);
    }

    #[test]
    fn layout_10k_linear_under_200ms() {
        let ids: Vec<String> = (0..10_000).map(|i| format!("{i}")).collect();
        let commits: Vec<Commit> = (0..10_000)
            .map(|i| if i + 1 < 10_000 { c(&ids[i], &[&ids[i + 1]]) } else { c(&ids[i], &[]) })
            .collect();
        let start = Instant::now();
        let l = layout(&commits);
        assert_eq!(l.rows.len(), 10_000);
        assert!(start.elapsed().as_millis() < 200, "{:?}", start.elapsed());
    }
}
