//! Right panel: staged / unstaged files and the commit box.

use eframe::egui::{self, RichText};

use super::state::AppState;
use crate::git::diff::DiffSide;
use crate::git::log::RefKind;
use crate::git::status::{FileEntry, FileState};
use crate::theme::{self, Palette};
use crate::worker::Job;

fn state_color(p: &Palette, e: &FileEntry, side: DiffSide) -> egui::Color32 {
    let st = if side == DiffSide::Staged { e.index } else { e.worktree };
    match (e.conflicted, st) {
        (true, _) => p.conflict,
        (_, FileState::Added | FileState::Untracked) => p.added,
        (_, FileState::Deleted) => p.deleted,
        _ => p.modified,
    }
}

fn state_letter(e: &FileEntry, side: DiffSide) -> &'static str {
    if e.conflicted {
        return "!";
    }
    match if side == DiffSide::Staged { e.index } else { e.worktree } {
        FileState::Added => "A",
        FileState::Untracked => "?",
        FileState::Deleted => "D",
        FileState::Renamed => "R",
        FileState::Copied => "C",
        FileState::TypeChanged => "T",
        _ => "M",
    }
}

/// Paths to pass to stage/unstage for an entry (both sides of a rename).
fn entry_paths(e: &FileEntry) -> Vec<String> {
    let mut v = vec![e.path.clone()];
    v.extend(e.orig_path.clone());
    v
}

enum Action {
    Select(String, DiffSide),
    Job(Job),
    Confirm(Job),
}

fn file_list(ui: &mut egui::Ui, state: &AppState, entries: &[FileEntry], side: DiffSide, actions: &mut Vec<Action>) {
    let p = theme::palette(state.theme);
    let selected = state.open.as_ref().and_then(|v| v.selected_file.clone());
    for e in entries {
        let is_sel = selected.as_ref() == Some(&(e.path.clone(), side));
        let frame = egui::Frame::new()
            .fill(if is_sel { p.selection } else { egui::Color32::TRANSPARENT })
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(4, 1));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut checked = side == DiffSide::Staged;
                if ui.checkbox(&mut checked, "").changed() {
                    let paths = entry_paths(e);
                    actions.push(Action::Job(if checked { Job::Stage(paths) } else { Job::Unstage(paths) }));
                }
                ui.label(RichText::new(state_letter(e, side)).monospace().strong().color(state_color(p, e, side)));
                let (dir, name) = match e.path.rsplit_once('/') {
                    Some((d, n)) => (format!("{d}/"), n.to_string()),
                    None => (String::new(), e.path.clone()),
                };
                let text = egui::text::LayoutJob::default();
                let mut job = text;
                job.append(&name, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.5), p.text));
                if !dir.is_empty() {
                    job.append(&format!("  {dir}"), 0.0, egui::TextFormat::simple(egui::FontId::proportional(12.0), p.muted));
                }
                let resp = ui.add(egui::Label::new(job).truncate().sense(egui::Sense::click()));
                let resp = resp.on_hover_text(&e.path);
                if resp.clicked() {
                    actions.push(Action::Select(e.path.clone(), side));
                }
                if side == DiffSide::Unstaged {
                    resp.context_menu(|ui| {
                        if ui.button(RichText::new(state.tr("changes.discard")).color(p.deleted)).clicked() {
                            actions.push(Action::Confirm(Job::Discard(vec![e.clone()])));
                            ui.close();
                        }
                        if ui.button(state.tr("changes.ignore")).clicked() {
                            actions.push(Action::Job(Job::Ignore(e.path.clone())));
                            ui.close();
                        }
                    });
                }
            });
        });
    }
}

fn group_header(ui: &mut egui::Ui, p: &Palette, title: &str, n: usize, button: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}  {n}", title.to_uppercase())).small().strong().color(p.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if n > 0 {
                clicked = ui.small_button(button).clicked();
            }
        });
    });
    clicked
}

/// Message of the HEAD commit, taken from the loaded history (no git call on the UI thread).
fn head_message(v: &super::state::RepoView) -> Option<String> {
    let snap = v.snapshot.as_ref()?;
    let head = snap.refs.iter().find(|r| r.kind == RefKind::Head)?;
    let c = snap.commits.iter().find(|c| c.id == head.target)?;
    Some(if c.body.is_empty() { c.subject.clone() } else { format!("{}\n\n{}", c.subject, c.body) })
}

fn commit_box(ui: &mut egui::Ui, state: &mut AppState, staged_count: usize) {
    let p = theme::palette(state.theme);
    let lang_amend = state.tr("commit.amend").to_string();
    let placeholder = state.tr("commit.message_placeholder").to_string();
    let button_label = state.tr("commit.button").to_string();
    let Some(v) = state.open.as_mut() else { return };
    let edit = egui::TextEdit::multiline(&mut v.commit_msg)
        .hint_text(placeholder)
        .desired_rows(4)
        .desired_width(f32::INFINITY);
    let resp = ui.add(edit);
    let mut amend = v.amend;
    if ui.checkbox(&mut amend, lang_amend).changed() {
        v.amend = amend;
        if amend && v.commit_msg.trim().is_empty() {
            v.commit_msg = head_message(v).unwrap_or_default();
        }
    }
    let can_commit = !v.commit_msg.trim().is_empty() && (staged_count > 0 || v.amend);
    let shortcut = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let btn = egui::Button::new(RichText::new(format!("✔ {button_label}")).color(p.on_accent).strong())
        .fill(p.accent)
        .min_size(egui::vec2(ui.available_width(), 32.0));
    let clicked = ui.add_enabled(can_commit, btn).on_hover_text("Ctrl+Entrée").clicked();
    if can_commit && (clicked || shortcut) {
        let job = Job::Commit { message: v.commit_msg.trim().to_string(), amend: v.amend };
        v.worker.send(job);
    }
}

pub fn show(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    if state.open.is_none() {
        return;
    }
    egui::Panel::right("changes")
        .resizable(true)
        .default_size(340.0)
        .min_size(260.0)
        .frame(egui::Frame::new().fill(p.sidebar).inner_margin(12))
        .show(ui, |ui| {
            let status = state.open.as_ref().and_then(|v| v.snapshot.as_ref()).map(|s| s.status.clone()).unwrap_or_default();
            let staged: Vec<FileEntry> = status.staged().cloned().collect();
            let unstaged: Vec<FileEntry> = status.unstaged().cloned().collect();
            let mut actions = Vec::new();
            egui::Panel::bottom("commit-box")
                .frame(egui::Frame::new().inner_margin(egui::Margin { top: 10, ..Default::default() }))
                .show_separator_line(true)
                .show(ui, |ui| commit_box(ui, state, staged.len()));
            super::commit_detail::show(state, ui);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if group_header(ui, p, state.tr("changes.staged"), staged.len(), state.tr("changes.unstage_all")) {
                    actions.push(Action::Job(Job::Unstage(staged.iter().flat_map(entry_paths).collect())));
                }
                file_list(ui, state, &staged, DiffSide::Staged, &mut actions);
                ui.add_space(10.0);
                if group_header(ui, p, state.tr("changes.unstaged"), unstaged.len(), state.tr("changes.stage_all")) {
                    actions.push(Action::Job(Job::Stage(unstaged.iter().flat_map(entry_paths).collect())));
                }
                file_list(ui, state, &unstaged, DiffSide::Unstaged, &mut actions);
                if staged.is_empty() && unstaged.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new(state.tr("changes.empty")).color(p.muted)));
                }
            });
            for a in actions {
                match a {
                    Action::Select(path, side) => {
                        if let Some(v) = state.open.as_mut() {
                            v.selected_file = Some((path.clone(), side));
                            v.diff = None;
                            v.line_sel.clear();
                            v.detail_file = None;
                            v.worker.send(Job::Diff { path, side });
                        }
                    }
                    Action::Job(j) => state.send(j),
                    Action::Confirm(j) => state.confirm("confirm.discard", j),
                }
            }
        });
}
