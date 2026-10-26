//! Top bar of the open repository: name, branch and remote actions.

use eframe::egui::{self, RichText};

use super::state::{AppState, Pending};
use crate::theme;
use crate::worker::Job;

pub fn show(state: &mut AppState, ui: &mut egui::Ui) {
    let p = theme::palette(state.theme);
    let Some(v) = state.open.as_ref() else { return };
    let name = v.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let branch = v.snapshot.as_ref().map(|s| s.status.branch.clone()).unwrap_or_default();
    let busy = v.worker.busy();
    let fetch_failed = v.fetch_failed;
    let mut job = None;
    let mut force = false;
    egui::Panel::top("toolbar")
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(14, 10)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).heading().color(p.text));
                let label = branch.head.clone().unwrap_or_else(|| state.tr("toolbar.detached").to_string());
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, p.accent))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| ui.label(RichText::new(format!("⎇ {label}")).color(p.accent)));
                if let Some(up) = &branch.upstream {
                    ui.label(RichText::new(format!("→ {up}")).small().color(p.muted));
                }
                if busy {
                    ui.spinner();
                }
                if fetch_failed {
                    ui.label(RichText::new("⚠").color(p.modified)).on_hover_text(state.tr("toolbar.fetch_failed"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let push_label = if branch.ahead > 0 {
                        format!("⬆ {}  {}", state.tr("toolbar.push"), branch.ahead)
                    } else {
                        format!("⬆ {}", state.tr("toolbar.push"))
                    };
                    let mut push_btn = egui::Button::new(RichText::new(push_label).color(if branch.ahead > 0 { p.on_accent } else { p.text }));
                    if branch.ahead > 0 {
                        push_btn = push_btn.fill(p.accent);
                    }
                    let push = ui.add(push_btn);
                    if push.clicked() {
                        job = Some(Job::Push { force: false });
                    }
                    push.context_menu(|ui| {
                        if ui.button(state.tr("toolbar.force_push")).clicked() {
                            force = true;
                            ui.close();
                        }
                    });
                    let pull_label = if branch.behind > 0 {
                        format!("⬇ {}  {}", state.tr("toolbar.pull"), branch.behind)
                    } else {
                        format!("⬇ {}", state.tr("toolbar.pull"))
                    };
                    if ui.button(pull_label).clicked() {
                        job = Some(Job::Pull);
                    }
                    if ui.button(format!("⟳ {}", state.tr("toolbar.fetch"))).clicked() {
                        job = Some(Job::Fetch { background: false });
                    }
                    if ui.small_button("↻").on_hover_text(state.tr("toolbar.refresh")).clicked() {
                        job = Some(Job::Refresh);
                    }
                });
            });
        });
    if let Some(j) = job {
        state.send(j);
    }
    if force {
        state.pending = Some(Pending::Confirm { text_key: "confirm.force_push", action: Job::Push { force: true } });
    }
}
