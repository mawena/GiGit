//! Error banner, confirmation modals and the empty state.

use eframe::egui::{self, RichText};

use super::GigitApp;
use super::state::{AppState, Pending};
use crate::theme;

pub fn error_banner(state: &mut AppState, ui: &mut egui::Ui) {
    if state.errors.is_empty() {
        return;
    }
    let p = theme::palette(state.theme);
    let mut dismiss = None;
    egui::Panel::top("errors")
        .frame(egui::Frame::new().fill(p.deleted.gamma_multiply(0.18)).inner_margin(10))
        .show(ui, |ui| {
            for (i, e) in state.errors.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠").color(p.deleted));
                    ui.label(RichText::new(&e.message).color(p.text).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(state.tr("error.dismiss")).clicked() {
                            dismiss = Some(i);
                        }
                    });
                });
                egui::CollapsingHeader::new(RichText::new(state.tr("error.details")).small().color(p.muted))
                    .id_salt(("error", i))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&e.details).monospace().color(p.muted));
                    });
            }
        });
    if let Some(i) = dismiss {
        state.errors.remove(i);
    }
}

pub fn empty_state(app: &GigitApp, ui: &mut egui::Ui) {
    let p = theme::palette(app.state.theme);
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.label(RichText::new("GiGit").size(32.0).color(p.accent).strong());
        ui.add_space(8.0);
        ui.label(RichText::new(app.state.tr("app.empty")).color(p.muted));
        ui.add_space(14.0);
        let btn = egui::Button::new(RichText::new(format!("+  {}", app.state.tr("repos.add"))).color(p.on_accent))
            .fill(p.accent);
        if ui.add(btn).clicked() {
            app.pick_folder(ui.ctx());
        }
    });
}

/// Shows the pending confirmation, if any, and runs its action when confirmed.
pub fn pending(state: &mut AppState, ctx: &egui::Context) {
    let Some(pending) = state.pending.clone() else { return };
    let p = theme::palette(state.theme);
    let text = match &pending {
        Pending::Confirm { text_key, .. } => state.tr(text_key).to_string(),
        Pending::InitRepo(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            crate::i18n::trf(state.lang, "repos.init_prompt", &[("name", &name)])
        }
    };
    let (yes_key, danger) = match &pending {
        Pending::Confirm { .. } => ("confirm.yes", true),
        Pending::InitRepo(_) => ("repos.init", false),
    };
    let mut decision = None;
    let modal = egui::Modal::new(egui::Id::unique("gigit-confirm")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(RichText::new(state.tr("confirm.title")).heading());
        ui.add_space(8.0);
        ui.label(text);
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let fill = if danger { p.deleted } else { p.accent };
            if ui.add(egui::Button::new(RichText::new(state.tr(yes_key)).color(p.on_accent)).fill(fill)).clicked() {
                decision = Some(true);
            }
            if ui.button(state.tr("confirm.cancel")).clicked() {
                decision = Some(false);
            }
        });
    });
    if modal.should_close() && decision.is_none() {
        decision = Some(false);
    }
    if let Some(yes) = decision {
        state.answer_pending(yes);
    }
}
