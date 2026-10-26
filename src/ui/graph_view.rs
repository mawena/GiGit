//! Central panel: the commit graph.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use eframe::epaint::CubicBezierShape;

use super::state::AppState;
use crate::git::log::{Ref, RefKind};
use crate::i18n::{self, Lang};
use crate::theme::{self, Palette};
use crate::worker::Job;

const ROW_H: f32 = 30.0;
const LANE_W: f32 = 16.0;
const GUTTER_PAD: f32 = 14.0;

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

fn lane_x(rect: Rect, lane: usize) -> f32 {
    rect.left() + GUTTER_PAD + lane as f32 * LANE_W
}

fn lane_color(p: &Palette, color: usize) -> Color32 {
    p.lanes[color % p.lanes.len()]
}

fn edge_shape(from: Pos2, to: Pos2, stroke: Stroke) -> egui::Shape {
    if (from.x - to.x).abs() < 0.5 {
        egui::Shape::line_segment([from, to], stroke)
    } else {
        let mid = (to.y - from.y) * 0.5;
        let pts = [from, pos2(from.x, from.y + mid), pos2(to.x, to.y - mid), to];
        CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, stroke).into()
    }
}

/// Paints a ref label and returns its width.
fn chip(painter: &egui::Painter, p: &Palette, at: Pos2, r: &Ref, lane_col: Color32) -> f32 {
    let font = FontId::proportional(11.5);
    let (fill, text_col, outline) = match r.kind {
        RefKind::LocalBranch => (lane_col, p.on_accent, None),
        RefKind::RemoteBranch => (Color32::TRANSPARENT, lane_col, Some(lane_col)),
        RefKind::Tag => (p.muted.gamma_multiply(0.35), p.text, None),
        RefKind::Head => (p.accent, p.on_accent, None),
    };
    let label = if r.kind == RefKind::Tag { format!("🏷 {}", r.name) } else { r.name.clone() };
    let galley = painter.layout_no_wrap(label, font, text_col);
    let rect = Rect::from_min_size(pos2(at.x, at.y - 9.0), vec2(galley.size().x + 12.0, 18.0));
    painter.rect_filled(rect, CornerRadius::same(5), fill);
    if let Some(c) = outline {
        painter.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, c), egui::StrokeKind::Inside);
    }
    painter.galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, text_col);
    rect.width()
}

pub fn show(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let lang = state.lang;
    let Some(v) = state.open.as_mut() else { return };
    let Some(snap) = v.snapshot.as_ref() else {
        ui.centered_and_justified(|ui| ui.spinner());
        return;
    };
    if snap.commits.is_empty() {
        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(i18n::tr(lang, "graph.empty")).color(p.muted)));
        return;
    }
    let mut by_commit: HashMap<&str, Vec<&Ref>> = HashMap::new();
    for r in &snap.refs {
        by_commit.entry(r.target.as_str()).or_default().push(r);
    }
    let head = snap.refs.iter().find(|r| r.kind == RefKind::Head).map(|r| r.target.as_str());
    let detached = snap.status.branch.head.is_none();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let gutter = (snap.layout.max_lanes as f32 * LANE_W + GUTTER_PAD).min(260.0);
    let n = snap.commits.len();
    let mut clicked: Option<String> = None;
    let mut reached_end = false;

    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, n, |ui, range| {
        reached_end = range.end >= n;
        let mut rects = Vec::with_capacity(range.len());
        // Pass 1: row backgrounds and text.
        for i in range.clone() {
            let c = &snap.commits[i];
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
            let painter = ui.painter();
            let selected = v.selected_commit.as_deref() == Some(c.id.as_str());
            if selected {
                painter.rect_filled(rect, CornerRadius::same(4), p.selection);
            } else if resp.hovered() {
                painter.rect_filled(rect, CornerRadius::same(4), p.selection.gamma_multiply(0.5));
            }
            let row = &snap.layout.rows[i];
            let col = lane_color(p, row.color);
            let mut x = rect.left() + gutter + 6.0;
            let cy = rect.center().y;
            for r in by_commit.get(c.id.as_str()).into_iter().flatten() {
                if r.kind == RefKind::Head && !detached {
                    continue;
                }
                x += chip(painter, p, pos2(x, cy), r, col) + 6.0;
            }
            let right = format!("{}   {}", relative_time(lang, now, c.time), &c.id[..c.id.len().min(7)]);
            let right_galley = painter.layout_no_wrap(right, FontId::proportional(12.0), p.muted);
            let right_x = rect.right() - right_galley.size().x - 10.0;
            painter.galley(pos2(right_x, cy - right_galley.size().y / 2.0), right_galley, p.muted);
            let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right_x - 12.0, rect.bottom()));
            painter.with_clip_rect(clip.intersect(painter.clip_rect())).text(
                pos2(x, cy),
                Align2::LEFT_CENTER,
                &c.subject,
                FontId::proportional(13.5),
                if selected { p.text } else { p.text.gamma_multiply(0.92) },
            );
            let resp = resp.on_hover_text(format!("{}\n{} <{}>", c.subject, c.author, c.email));
            if resp.clicked() {
                clicked = Some(c.id.clone());
            }
            rects.push(rect);
        }
        // Pass 2: edges and nodes on top of every background.
        let painter = ui.painter();
        if let (Some(first), true) = (rects.first(), range.start > 0) {
            let prev = &snap.layout.rows[range.start - 1];
            let y0 = first.center().y - ROW_H;
            for e in &prev.edges {
                let stroke = Stroke::new(2.0, lane_color(p, e.color));
                painter.add(edge_shape(pos2(lane_x(*first, e.from_lane), y0), pos2(lane_x(*first, e.to_lane), first.center().y), stroke));
            }
        }
        for (k, i) in range.clone().enumerate() {
            let rect = rects[k];
            let row = &snap.layout.rows[i];
            let cy = rect.center().y;
            if i + 1 < n {
                for e in &row.edges {
                    let stroke = Stroke::new(2.0, lane_color(p, e.color));
                    painter.add(edge_shape(pos2(lane_x(rect, e.from_lane), cy), pos2(lane_x(rect, e.to_lane), cy + ROW_H), stroke));
                }
            }
        }
        for (k, i) in range.clone().enumerate() {
            let rect = rects[k];
            let row = &snap.layout.rows[i];
            let center = pos2(lane_x(rect, row.lane), rect.center().y);
            let col = lane_color(p, row.color);
            if head == Some(snap.commits[i].id.as_str()) {
                painter.circle_filled(center, 6.0, p.bg);
                painter.circle_stroke(center, 6.5, Stroke::new(2.0, p.accent));
                painter.circle_filled(center, 3.5, col);
            } else {
                painter.circle_filled(center, 4.5, col);
                painter.circle_stroke(center, 4.5, Stroke::new(1.5, p.bg));
            }
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
    if reached_end && snap.has_more && !v.loading_more {
        v.loading_more = true;
        v.worker.send(Job::LoadMore { skip: n });
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
}
