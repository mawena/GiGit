//! Diff display with hunk and line selection.

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, pos2, vec2};

use super::state::{AppState, Pending};
use crate::git::diff::{DiffSide, FileDiff, LineKind};
use crate::git::patch::LineSel;
use crate::theme::{self, Palette};
use crate::worker::Job;

const ROW_H: f32 = 20.0;
const NUM_W: f32 = 42.0;

fn is_change(file: &FileDiff, hunk: usize, line: usize) -> bool {
    file.hunks.get(hunk).and_then(|h| h.lines.get(line)).is_some_and(|l| l.kind != LineKind::Context)
}

fn hunk_changes(file: &FileDiff, hunk: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    let n = file.hunks.get(hunk).map_or(0, |h| h.lines.len());
    (0..n).filter(move |&i| is_change(file, hunk, i)).map(move |i| (hunk, i))
}

/// Selects every change line of `hunk`, or clears them if they were all selected already.
pub fn toggle_hunk(sel: &mut LineSel, file: &FileDiff, hunk: usize) {
    let lines: Vec<_> = hunk_changes(file, hunk).collect();
    if lines.iter().all(|l| sel.contains(l)) {
        for l in &lines {
            sel.remove(l);
        }
    } else {
        sel.extend(lines);
    }
}

/// Toggles one line; context lines cannot be selected.
pub fn toggle_line(sel: &mut LineSel, file: &FileDiff, hunk: usize, line: usize) {
    if is_change(file, hunk, line) && !sel.remove(&(hunk, line)) {
        sel.insert((hunk, line));
    }
}

/// Selects the change lines between two lines of the same hunk (inclusive, any order).
pub fn select_range(sel: &mut LineSel, file: &FileDiff, hunk: usize, a: usize, b: usize) {
    for i in a.min(b)..=a.max(b) {
        if is_change(file, hunk, i) {
            sel.insert((hunk, i));
        }
    }
}

fn hunk_sel(file: &FileDiff, hunk: usize) -> LineSel {
    hunk_changes(file, hunk).collect()
}

enum Row {
    Header(usize),
    Line(usize, usize),
}

/// What the user asked for in an interactive diff.
enum DiffAction {
    Stage(LineSel),
    Unstage(LineSel),
    Discard(LineSel),
}

/// Renders a diff. With `sel`, lines and hunks are selectable and actions are offered for `side`.
fn render(ui: &mut egui::Ui, state: &AppState, file: &FileDiff, mut sel: Option<&mut LineSel>, side: DiffSide) -> Option<DiffAction> {
    let p = theme::palette(state.theme);
    if file.binary {
        ui.label(RichText::new(state.tr("diff.binary")).color(p.muted));
        return None;
    }
    let rows: Vec<Row> = file
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| std::iter::once(Row::Header(h)).chain((0..hunk.lines.len()).map(move |i| Row::Line(h, i))))
        .collect();
    let anchor_id = egui::Id::unique(("gigit-diff-anchor", file.path()));
    let mut action = None;
    let mono = FontId::monospace(state.config.font_size * 0.92);
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, range| {
        for row in &rows[range] {
            match *row {
                Row::Header(h) => {
                    let hunk = &file.hunks[h];
                    egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
                        ui.set_height(ROW_H);
                        ui.horizontal_centered(|ui| {
                            let title = format!("@@ -{},{} +{},{} @@ {}", hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len, hunk.section);
                            let title = ui.add(egui::Label::new(RichText::new(title).font(mono.clone()).color(p.muted)).sense(Sense::click()));
                            if let Some(s) = sel.as_deref_mut()
                                && title.clicked()
                            {
                                toggle_hunk(s, file, h);
                            }
                            if sel.is_some() {
                                ui.add_space(12.0);
                                match side {
                                    DiffSide::Unstaged => {
                                        if ui.small_button(state.tr("diff.stage_hunk")).clicked() {
                                            action = Some(DiffAction::Stage(hunk_sel(file, h)));
                                        }
                                        if ui.small_button(RichText::new(state.tr("diff.discard_hunk")).color(p.deleted)).clicked() {
                                            action = Some(DiffAction::Discard(hunk_sel(file, h)));
                                        }
                                    }
                                    DiffSide::Staged => {
                                        if ui.small_button(state.tr("diff.unstage_hunk")).clicked() {
                                            action = Some(DiffAction::Unstage(hunk_sel(file, h)));
                                        }
                                    }
                                }
                            }
                        });
                    });
                }
                Row::Line(h, i) => {
                    let line = &file.hunks[h].lines[i];
                    let galley = ui.painter().layout_no_wrap(line.text.clone(), mono.clone(), p.text);
                    let width = (NUM_W * 2.0 + 24.0 + galley.size().x + 16.0).max(ui.available_width());
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                    let selected = sel.as_ref().is_some_and(|s| s.contains(&(h, i)));
                    let (bg, sign, sign_col) = match line.kind {
                        LineKind::Added => (p.added.gamma_multiply(0.14), "+", p.added),
                        LineKind::Removed => (p.deleted.gamma_multiply(0.14), "-", p.deleted),
                        LineKind::Context => (Color32::TRANSPARENT, " ", p.muted),
                    };
                    let painter = ui.painter();
                    painter.rect_filled(rect, 0.0, if selected { p.accent.gamma_multiply(0.35) } else { bg });
                    if resp.hovered() && sel.is_some() && line.kind != LineKind::Context {
                        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, p.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                    }
                    let cy = rect.center().y;
                    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                    let small = FontId::monospace(state.config.font_size * 0.8);
                    painter.text(pos2(rect.left() + NUM_W - 6.0, cy), Align2::RIGHT_CENTER, num(line.old_no), small.clone(), p.muted);
                    painter.text(pos2(rect.left() + NUM_W * 2.0 - 6.0, cy), Align2::RIGHT_CENTER, num(line.new_no), small, p.muted);
                    painter.text(pos2(rect.left() + NUM_W * 2.0 + 6.0, cy), Align2::LEFT_CENTER, sign, mono.clone(), sign_col);
                    painter.galley(pos2(rect.left() + NUM_W * 2.0 + 22.0, cy - galley.size().y / 2.0), galley, p.text);
                    if line.no_newline {
                        painter.text(pos2(rect.right() - 8.0, cy), Align2::RIGHT_CENTER, "⏎̸", mono.clone(), p.muted);
                    }
                    if let Some(s) = sel.as_deref_mut()
                        && resp.clicked()
                    {
                        let shift = ui.input(|i| i.modifiers.shift);
                        let anchor: Option<(usize, usize)> = ui.data(|d| d.get_temp(anchor_id));
                        match anchor {
                            Some((ah, ai)) if shift && ah == h => select_range(s, file, h, ai, i),
                            _ => toggle_line(s, file, h, i),
                        }
                        ui.data_mut(|d| d.insert_temp(anchor_id, (h, i)));
                    }
                }
            }
        }
    });
    action
}

fn header_bar(ui: &mut egui::Ui, p: &Palette, back_label: &str, title: &str, sub: &str) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        back = ui.button(back_label).clicked();
        ui.label(RichText::new(title).strong().color(p.text));
        ui.label(RichText::new(sub).small().color(p.muted));
    });
    ui.add_space(4.0);
    back
}

/// Interactive diff of the selected work-tree file.
pub fn show_file(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let Some(v) = state.open.as_ref() else { return };
    let Some((path, side)) = v.selected_file.clone() else { return };
    let side_label = state.tr(if side == DiffSide::Staged { "changes.staged" } else { "changes.unstaged" }).to_string();
    let mut back = false;
    let mut action = None;
    let mut clear = false;
    ui.horizontal(|ui| {
        back = header_bar(ui, p, state.tr("graph.back"), &path, &side_label);
    });
    let mut sel = v.line_sel.clone();
    // Always laid out, so selecting the first line does not shift the diff under the pointer.
    let any = !sel.is_empty();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} ✓", sel.len())).color(if any { p.accent } else { p.muted }));
        match side {
            DiffSide::Unstaged => {
                if ui.add_enabled(any, egui::Button::new(state.tr("diff.stage_lines"))).clicked() {
                    action = Some(DiffAction::Stage(sel.clone()));
                }
                let discard = egui::Button::new(RichText::new(state.tr("diff.discard_lines")).color(p.deleted));
                if ui.add_enabled(any, discard).clicked() {
                    action = Some(DiffAction::Discard(sel.clone()));
                }
            }
            DiffSide::Staged => {
                if ui.add_enabled(any, egui::Button::new(state.tr("diff.unstage_lines"))).clicked() {
                    action = Some(DiffAction::Unstage(sel.clone()));
                }
            }
        }
        if ui.add_enabled(any, egui::Button::new(state.tr("diff.clear_selection"))).clicked() {
            clear = true;
        }
    });
    ui.add_space(4.0);
    match v.diff.clone() {
        None => {
            ui.label(RichText::new(state.tr("diff.empty")).color(p.muted));
        }
        Some(diff) => {
            if let Some(a) = render(ui, state, &diff, Some(&mut sel), side) {
                action = Some(a);
            }
            let job = match action {
                Some(DiffAction::Stage(s)) => Some(Job::StageLines(diff.clone(), s)),
                Some(DiffAction::Unstage(s)) => Some(Job::UnstageLines(diff.clone(), s)),
                Some(DiffAction::Discard(s)) => {
                    state.pending = Some(Pending::Confirm { text_key: "confirm.discard", action: Job::DiscardLines(diff.clone(), s) });
                    None
                }
                None => None,
            };
            if let Some(j) = job {
                state.send(j);
                sel.clear();
            }
        }
    }
    let Some(v) = state.open.as_mut() else { return };
    v.line_sel = if clear { LineSel::new() } else { sel };
    if back {
        v.selected_file = None;
        v.diff = None;
        v.line_sel.clear();
    }
}

/// Read-only diff of a file of the selected commit.
pub fn show_commit_file(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let Some(v) = state.open.as_ref() else { return };
    let (Some((commit, files)), Some(idx)) = (v.detail.as_ref(), v.detail_file) else { return };
    let Some(file) = files.get(idx).cloned() else { return };
    let short = commit.id[..commit.id.len().min(7)].to_string();
    let back = header_bar(ui, p, state.tr("graph.back"), file.path(), &short);
    render(ui, state, &file, None, DiffSide::Unstaged);
    if back && let Some(v) = state.open.as_mut() {
        v.detail_file = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::parse_diff;

    fn file() -> FileDiff {
        parse_diff("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -10,2 +10,3 @@\n x\n+y\n z\n")
            .remove(0)
    }

    #[test]
    fn toggle_hunk_selects_only_change_lines() {
        let f = file();
        let mut s = LineSel::new();
        toggle_hunk(&mut s, &f, 0);
        assert_eq!(s, [(0, 1), (0, 2)].into_iter().collect());
    }

    #[test]
    fn toggle_hunk_twice_clears() {
        let f = file();
        let mut s: LineSel = [(1, 1)].into_iter().collect();
        toggle_hunk(&mut s, &f, 0);
        toggle_hunk(&mut s, &f, 0);
        assert_eq!(s, [(1, 1)].into_iter().collect());
    }

    #[test]
    fn toggle_hunk_partial_selects_rest() {
        let f = file();
        let mut s: LineSel = [(0, 1)].into_iter().collect();
        toggle_hunk(&mut s, &f, 0);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn toggle_line_ignores_context() {
        let f = file();
        let mut s = LineSel::new();
        toggle_line(&mut s, &f, 0, 0);
        assert!(s.is_empty());
        toggle_line(&mut s, &f, 0, 2);
        toggle_line(&mut s, &f, 1, 1);
        assert_eq!(s, [(0, 2), (1, 1)].into_iter().collect());
        toggle_line(&mut s, &f, 0, 2);
        assert_eq!(s, [(1, 1)].into_iter().collect());
    }

    #[test]
    fn select_range_any_order() {
        let f = file();
        let mut s = LineSel::new();
        select_range(&mut s, &f, 0, 3, 0);
        assert_eq!(s, [(0, 1), (0, 2)].into_iter().collect());
    }
}
