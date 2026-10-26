//! Central panel: the commit graph.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

use super::state::AppState;
use crate::git::log::{Commit, Ref, RefKind};
use crate::graph::GNode;
use crate::i18n::{self, Lang};
use crate::theme::{self, Palette};
use crate::worker::Job;

/// "il y a 5 min" / "5 min ago" style age of `t` relative to `now` (unix seconds).
pub fn relative_time(lang: Lang, now: i64, t: i64) -> String {
    let secs = (now - t).max(0);
    let (key, n) = match secs {
        0..60 => return i18n::tr(lang, "time.just_now").to_string(),
        60..3600 => ("time.minutes", secs / 60),
        3600..86400 => ("time.hours", secs / 3600),
        _ => ("time.days", secs / 86400),
    };
    i18n::trf(lang, key, &[("n", &n.to_string())])
}

/// Stable colour index for a branch or author name (FNV-1a).
pub fn name_hash(name: &str) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h ^ (h >> 32)) as usize
}

// Geometry, after ungit (scaled to ~0.85): main nodes r=30, side r=15, 60px per step,
// 90px per branch column, commit cards on the left of the main line.
const UNIT: f32 = 52.0;
const R_MAIN: f32 = 26.0;
const R_SIDE: f32 = 13.0;
const SLOT_W: f32 = 78.0;
const TOP: f32 = 46.0;
const CARD_H: f32 = 66.0;
const CARD_GAP: f32 = 22.0;
const EDGE_W: f32 = 7.0;

fn branch_color(p: &Palette, name: Option<&str>) -> Color32 {
    match name {
        Some(n) => p.lanes[name_hash(n) % p.lanes.len()],
        None => p.muted,
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase()
}

struct Geometry {
    origin: Pos2,
    card_w: f32,
    line_x: f32,
}

impl Geometry {
    fn center(&self, n: &GNode) -> Pos2 {
        pos2(self.line_x + n.slot as f32 * SLOT_W, self.origin.y + TOP + n.y * UNIT)
    }
    fn radius(n: &GNode) -> f32 {
        if n.main { R_MAIN } else { R_SIDE }
    }
}

/// Commit card on the left of the main line, with an arrow pointing at the node.
#[allow(clippy::too_many_arguments)]
fn card(painter: &egui::Painter, p: &Palette, lang: Lang, now: i64, g: &Geometry, center: Pos2, c: &Commit, selected: bool, faded: bool) -> Rect {
    let rect = Rect::from_min_size(pos2(g.origin.x + 10.0, center.y - CARD_H / 2.0), vec2(g.card_w, CARD_H));
    let alpha = if faded { 0.85 } else { 1.0 };
    let fill = p.panel.gamma_multiply(alpha);
    painter.rect_filled(rect, CornerRadius::same(6), fill);
    let border = if selected { p.accent } else { p.border };
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(if selected { 2.0 } else { 1.0 }, border), egui::StrokeKind::Inside);
    // arrow
    let tip = pos2(rect.right() + 9.0, center.y);
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(rect.right() - 1.0, center.y - 9.0), tip, pos2(rect.right() - 1.0, center.y + 9.0)],
        fill,
        Stroke::NONE,
    ));
    // avatar
    let av = pos2(rect.left() + 28.0, rect.center().y);
    painter.circle_filled(av, 17.0, p.lanes[name_hash(&c.author) % p.lanes.len()]);
    painter.text(av, Align2::CENTER_CENTER, initials(&c.author), FontId::new(13.0, FontFamily::Name("semibold".into())), p.on_accent);
    let text_x = rect.left() + 56.0;
    let clip = Rect::from_min_max(pos2(text_x, rect.top()), pos2(rect.right() - 10.0, rect.bottom()));
    let tp = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
    let title = tp.layout_no_wrap(c.subject.clone(), FontId::new(14.5, FontFamily::Name("semibold".into())), p.text);
    let title_w = title.size().x;
    tp.galley(pos2(text_x, rect.top() + 12.0), title, p.text);
    let by = format!("  {} {}", if lang == Lang::Fr { "par" } else { "by" }, c.author);
    tp.text(pos2(text_x + title_w, rect.top() + 12.0), Align2::LEFT_TOP, by, FontId::proportional(13.0), p.muted);
    let meta = format!("{}  |  {}", relative_time(lang, now, c.time), &c.id[..c.id.len().min(8)]);
    tp.text(pos2(text_x, rect.top() + 38.0), Align2::LEFT_TOP, meta, FontId::proportional(12.5), p.muted);
    rect
}

/// Ref labels to the right of a node; returns nothing, purely decorative and clickable via the node.
fn labels(painter: &egui::Painter, p: &Palette, at: Pos2, refs: &[&Ref], current: Option<&str>) {
    let mut x = at.x;
    for r in refs {
        let is_current = r.kind == RefKind::LocalBranch && Some(r.name.as_str()) == current;
        let (icon, color, size) = match r.kind {
            RefKind::LocalBranch if is_current => ("⎇", p.text, 19.0),
            RefKind::LocalBranch => ("⎇", p.text.gamma_multiply(0.65), 15.0),
            RefKind::RemoteBranch => ("⎇", p.lanes[3 % p.lanes.len()].gamma_multiply(0.9), 15.0),
            RefKind::Tag => ("🏷", p.modified.gamma_multiply(0.9), 15.0),
            RefKind::Head => ("➤", p.accent, 15.0),
        };
        let font = if is_current { FontId::new(size, FontFamily::Name("semibold".into())) } else { FontId::proportional(size) };
        let galley = painter.layout_no_wrap(format!("{icon} {}", r.name), font, color);
        let w = galley.size().x;
        painter.galley(pos2(x, at.y - galley.size().y / 2.0), galley, color);
        x += w + 16.0;
    }
}

pub fn show(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let lang = state.lang;
    let Some(v) = state.open.as_mut() else { return };
    let Some(snap) = v.snapshot.as_ref() else {
        ui.centered_and_justified(|ui| ui.spinner());
        return;
    };
    let lay = &snap.layout;
    if lay.nodes.is_empty() {
        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(i18n::tr(lang, "graph.empty")).color(p.muted)));
        return;
    }
    let mut by_commit: HashMap<&str, Vec<&Ref>> = HashMap::new();
    for r in &snap.refs {
        by_commit.entry(r.target.as_str()).or_default().push(r);
    }
    let current = snap.status.branch.head.as_deref();
    let detached = current.is_none();
    let head_id = snap.refs.iter().find(|r| r.kind == RefKind::Head).map(|r| r.target.clone());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let total_h = TOP + lay.height * UNIT + TOP + 40.0;
    let selected_id = v.selected_commit.clone();
    let mut clicked: Option<String> = None;
    let mut near_end = false;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let width = ui.available_width();
        let card_w = (width * 0.48).clamp(300.0, 560.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, total_h), Sense::click());
        let g = Geometry { origin: rect.min, card_w, line_x: rect.min.x + 10.0 + card_w + CARD_GAP + R_MAIN };
        let painter = ui.painter();
        let visible = viewport.translate(rect.min.to_vec2()).expand(UNIT * 2.0);
        near_end = viewport.max.y > total_h - 900.0;
        let in_view = |y: f32| y >= visible.top() && y <= visible.bottom();
        let edge_col = p.muted.gamma_multiply(0.45);

        // Edges first, straight like ungit.
        for e in &lay.edges {
            let (a, b) = (g.center(&lay.nodes[e.from]), g.center(&lay.nodes[e.to]));
            if b.y < visible.top() || a.y > visible.bottom() {
                continue;
            }
            painter.line_segment([a, b], Stroke::new(EDGE_W, edge_col));
        }

        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut hovered: Option<usize> = None;
        for (k, n) in lay.nodes.iter().enumerate() {
            let c = g.center(n);
            if !in_view(c.y) {
                continue;
            }
            let r = Geometry::radius(n);
            if pointer.is_some_and(|pt| pt.distance(c) <= r + 3.0) && resp.hovered() {
                hovered = Some(k);
            }
        }

        // Cards: main line always, others on hover or when selected.
        for (k, n) in lay.nodes.iter().enumerate() {
            let c = g.center(n);
            if !in_view(c.y) {
                continue;
            }
            let commit = &snap.commits[n.commit];
            let selected = selected_id.as_deref() == Some(commit.id.as_str());
            if n.main || selected || hovered == Some(k) {
                let card_rect = card(painter, p, lang, now, &g, pos2(c.x, c.y), commit, selected, !n.main);
                if resp.clicked() && pointer.is_some_and(|pt| card_rect.contains(pt)) {
                    clicked = Some(commit.id.clone());
                }
                if !n.main {
                    // connector from the card arrow to a side node
                    painter.line_segment([pos2(card_rect.right() + 9.0, c.y), c], Stroke::new(1.0, p.border));
                }
            }
        }

        // Nodes and labels.
        for (k, n) in lay.nodes.iter().enumerate() {
            let c = g.center(n);
            if !in_view(c.y) {
                continue;
            }
            let commit = &snap.commits[n.commit];
            let r = Geometry::radius(n);
            let col = branch_color(p, n.branch.as_deref());
            painter.circle_filled(c, r, col);
            let selected = selected_id.as_deref() == Some(commit.id.as_str());
            if selected || hovered == Some(k) {
                painter.circle_stroke(c, r - 4.0, Stroke::new(4.0, p.bg));
            }
            if head_id.as_deref() == Some(commit.id.as_str()) {
                painter.circle_stroke(c, r + 4.0, Stroke::new(2.5, p.accent));
            }
            if let Some(refs) = by_commit.get(commit.id.as_str()) {
                let shown: Vec<&Ref> =
                    refs.iter().copied().filter(|r| r.kind != RefKind::Head || detached).collect();
                labels(painter, p, pos2(c.x + r + 14.0, c.y), &shown, current);
            }
            if resp.clicked() && hovered == Some(k) {
                clicked = Some(commit.id.clone());
            }
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    });

    if let Some(id) = clicked {
        if v.selected_commit.as_deref() == Some(id.as_str()) {
            v.selected_commit = None;
            v.detail = None;
        } else {
            v.selected_commit = Some(id.clone());
            v.detail = None;
            v.worker.send(Job::Show { id });
        }
    }
    if near_end && snap.has_more && !v.loading_more {
        v.loading_more = true;
        v.worker.send(Job::LoadMore { skip: snap.commits.len() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_buckets() {
        let now = 1_000_000;
        assert_eq!(relative_time(Lang::Fr, now, now - 30), "à l'instant");
        assert_eq!(relative_time(Lang::Fr, now, now - 5 * 60), "il y a 5 min");
        assert_eq!(relative_time(Lang::En, now, now - 3 * 3600), "3 h ago");
        assert_eq!(relative_time(Lang::Fr, now, now - 2 * 86400), "il y a 2 j");
        // future timestamps (clock skew) read as "just now"
        assert_eq!(relative_time(Lang::En, now, now + 100), "just now");
    }

    #[test]
    fn name_hash_is_stable_and_spreads() {
        assert_eq!(name_hash("main"), name_hash("main"));
        let distinct: std::collections::HashSet<usize> =
            ["main", "develop", "feature/a", "feature/b", "origin/main", "v1"].iter().map(|n| name_hash(n) % 8).collect();
        assert!(distinct.len() >= 4, "{distinct:?}");
    }
}
