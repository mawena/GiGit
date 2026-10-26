//! Left panel: the list of repositories.

use eframe::egui::{self, Align2, CornerRadius, FontId, Sense, Stroke, vec2};

use super::GigitApp;
use crate::config::RepoProbe;
use crate::theme;

const ROW_H: f32 = 40.0;

pub fn show(app: &mut GigitApp, ui: &mut egui::Ui) {
    let p = theme::palette(app.state.theme);
    egui::Panel::left("repos")
        .resizable(true)
        .default_size(230.0)
        .min_size(170.0)
        .frame(egui::Frame::new().fill(p.sidebar).inner_margin(10))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                bottom_buttons(app, ui);
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.label(
                        egui::RichText::new(app.state.tr("repos.title").to_uppercase())
                            .small()
                            .color(p.muted)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| repo_list(app, ui));
                });
            });
        });
}

fn bottom_buttons(app: &mut GigitApp, ui: &mut egui::Ui) {
    let p = theme::palette(app.state.theme);
    ui.horizontal(|ui| {
        let label = egui::RichText::new(format!("⚙  {}", app.state.tr("toolbar.settings"))).color(p.muted);
        let gear = ui.add(egui::Button::new(label).frame(false));
        if gear.clicked() {
            app.state.show_settings = !app.state.show_settings;
        }
    });
    let label = format!("+  {}", app.state.tr("repos.add"));
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let color = if resp.hovered() { p.accent } else { p.muted };
    dashed_rect(ui.painter(), rect, Stroke::new(1.0, color));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), color);
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.pick_folder(ui.ctx());
    }
}

fn dashed_rect(painter: &egui::Painter, r: egui::Rect, stroke: Stroke) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in pts.windows(2) {
        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, 5.0, 4.0));
    }
}

fn repo_list(app: &mut GigitApp, ui: &mut egui::Ui) {
    let p = theme::palette(app.state.theme);
    let repos = app.state.config.repos.clone();
    let open = app.state.open.as_ref().map(|v| v.path.clone());
    let mut action: Option<Action> = None;
    for (i, path) in repos.iter().enumerate() {
        let badge = app.state.badges.get(path).cloned().unwrap_or_default();
        let broken = matches!(badge.probe, Some(RepoProbe::Missing | RepoProbe::NotARepo));
        let selected = open.as_ref() == Some(path);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        resp.dnd_set_drag_payload(i);
        let painter = ui.painter();
        if selected || resp.hovered() {
            let fill = if selected { p.selection } else { p.selection.gamma_multiply(0.5) };
            painter.rect_filled(rect, CornerRadius::same(7), fill);
        }
        if let Some(from) = resp.dnd_hover_payload::<usize>()
            && *from != i
        {
            let y = if *from < i { rect.bottom() } else { rect.top() };
            painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(2.0, p.accent));
        }
        if let Some(from) = resp.dnd_release_payload::<usize>() {
            action = Some(Action::Move(*from, i));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let name_color = if broken {
            p.muted.gamma_multiply(0.6)
        } else if selected {
            p.accent
        } else {
            p.text
        };
        let font = if selected { FontId::new(14.0, egui::FontFamily::Name("semibold".into())) } else { FontId::proportional(14.0) };
        let left = rect.left() + 10.0;
        painter.text(egui::pos2(left, rect.center().y - 7.0), Align2::LEFT_CENTER, &name, font, name_color);
        let sub = if broken {
            app.state.tr(if matches!(badge.probe, Some(RepoProbe::Missing)) { "repos.missing" } else { "repos.not_a_repo" }).to_string()
        } else {
            let mut s = badge.branch.clone().unwrap_or_default();
            if badge.ahead > 0 {
                s.push_str(&format!("  ↑{}", badge.ahead));
            }
            if badge.behind > 0 {
                s.push_str(&format!("  ↓{}", badge.behind));
            }
            s
        };
        painter.text(egui::pos2(left, rect.center().y + 9.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.5), p.muted);
        if badge.dirty {
            painter.circle_filled(egui::pos2(rect.right() - 12.0, rect.center().y), 3.5, p.modified);
        }
        let resp = resp.on_hover_text(path.display().to_string());
        if resp.clicked() && !selected && !broken {
            action = Some(Action::Open(path.clone()));
        }
        resp.context_menu(|ui| {
            if ui.button(app.state.tr("repos.remove")).on_hover_text(app.state.tr("repos.remove_hint")).clicked() {
                action = Some(Action::Remove(path.clone()));
                ui.close();
            }
        });
        ui.add_space(2.0);
    }
    match action {
        Some(Action::Open(p)) => app.state.open_repo(p),
        Some(Action::Remove(p)) => app.state.remove_repo(&p),
        Some(Action::Move(from, to)) => {
            app.state.config.move_repo(from, to);
            app.state.save_config();
        }
        None => {}
    }
}

enum Action {
    Open(std::path::PathBuf),
    Remove(std::path::PathBuf),
    Move(usize, usize),
}
