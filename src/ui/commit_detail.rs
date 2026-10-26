//! Detail of the commit selected in the graph, shown above the changes list.

use eframe::egui::{self, RichText};

use super::state::AppState;
use crate::git::diff::FileDiff;
use crate::theme;
use crate::worker::Job;

/// Local date and time as `YYYY-MM-DD HH:MM`.
pub fn format_local(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time: libc::time_t = t;
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return t.to_string();
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

fn file_color(p: &theme::Palette, f: &FileDiff) -> egui::Color32 {
    match (&f.old_path, &f.new_path) {
        (None, _) => p.added,
        (_, None) => p.deleted,
        _ => p.modified,
    }
}

pub fn show(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let lang = state.lang;
    let Some(v) = state.open.as_ref() else { return };
    if v.selected_commit.is_none() {
        return;
    }
    let Some((commit, files)) = v.detail.clone() else {
        ui.horizontal(|ui| {
            ui.spinner();
        });
        return;
    };
    let mut select_parent = None;
    let mut open_file = None;
    let mut close = false;
    egui::Frame::new().fill(p.panel).corner_radius(8).inner_margin(10).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(state.tr("commit.title").to_uppercase()).small().strong().color(p.muted));
            ui.label(RichText::new(&commit.id[..commit.id.len().min(10)]).small().monospace().color(p.accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    close = true;
                }
            });
        });
        ui.label(RichText::new(&commit.subject).strong().color(p.text));
        if !commit.body.is_empty() {
            ui.label(RichText::new(&commit.body).color(p.text.gamma_multiply(0.85)));
        }
        ui.add_space(4.0);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        ui.horizontal(|ui| {
            let avatar =
                state.config.avatars.then(|| state.avatars.get(ui.ctx(), &commit.email)).flatten().map(|t| t.id());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
            super::graph_view::avatar_circle(ui.painter(), p, rect.center(), 14.0, &commit.author, avatar);
            ui.label(RichText::new(format!("{} <{}>", commit.author, commit.email)).small().color(p.muted));
        });
        ui.label(
            RichText::new(format!("{}  ·  {}", format_local(commit.time), super::graph_view::relative_time(lang, now, commit.time)))
                .small()
                .color(p.muted),
        );
        if !commit.parents.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(state.tr("detail.parents")).small().color(p.muted));
                for parent in &commit.parents {
                    if ui.link(RichText::new(&parent[..parent.len().min(7)]).small().monospace()).clicked() {
                        select_parent = Some(parent.clone());
                    }
                }
            });
        }
        ui.add_space(4.0);
        ui.label(RichText::new(format!("{} ({})", state.tr("detail.files"), files.len())).small().color(p.muted));
        egui::ScrollArea::vertical().id_salt("detail-files").max_height(160.0).show(ui, |ui| {
            for (i, f) in files.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(file_color(p, f)));
                    if ui.add(egui::Label::new(RichText::new(f.path()).color(p.text)).truncate().sense(egui::Sense::click())).clicked() {
                        open_file = Some(i);
                    }
                });
            }
        });
    });
    ui.add_space(8.0);
    let Some(v) = state.open.as_mut() else { return };
    if close {
        v.selected_commit = None;
        v.detail = None;
        v.detail_file = None;
    }
    if let Some(i) = open_file {
        v.detail_file = Some(i);
        v.selected_file = None;
    }
    if let Some(id) = select_parent {
        v.selected_commit = Some(id.clone());
        v.detail = None;
        v.detail_file = None;
        v.worker.send(Job::Show { id });
    }
}
