//! Settings window: language, theme, text size, automatic fetch.

use eframe::egui::{self, RichText};

use super::state::AppState;
use crate::i18n::{self, LangChoice};
use crate::theme::{self, ThemeChoice};

pub fn show(state: &mut AppState, ctx: &egui::Context) {
    if !state.show_settings {
        return;
    }
    let p = theme::palette(state.theme);
    let mut open = true;
    let title = state.tr("settings.title").to_string();
    egui::Window::new(RichText::new(title).strong())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("settings-grid").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                ui.label(RichText::new(state.tr("settings.language")).color(p.muted));
                let mut lang = state.config.language;
                let lang_label = |l: LangChoice| match l {
                    LangChoice::System => "lang.system",
                    LangChoice::Fr => "lang.fr",
                    LangChoice::En => "lang.en",
                };
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(state.tr(lang_label(lang)))
                    .show_ui(ui, |ui| {
                        for l in [LangChoice::System, LangChoice::Fr, LangChoice::En] {
                            ui.selectable_value(&mut lang, l, i18n::tr(state.lang, lang_label(l)));
                        }
                    });
                if lang != state.config.language {
                    state.set_language(lang);
                }
                ui.end_row();

                ui.label(RichText::new(state.tr("settings.theme")).color(p.muted));
                let mut th = state.config.theme;
                let theme_label = |t: ThemeChoice| match t {
                    ThemeChoice::System => "theme.system",
                    ThemeChoice::Ardoise => "theme.ardoise",
                    ThemeChoice::SombreChaleureux => "theme.sombre_chaleureux",
                    ThemeChoice::Nuit => "theme.nuit",
                    ThemeChoice::Clair => "theme.clair",
                };
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(state.tr(theme_label(th)))
                    .show_ui(ui, |ui| {
                        for t in [
                            ThemeChoice::System,
                            ThemeChoice::Ardoise,
                            ThemeChoice::SombreChaleureux,
                            ThemeChoice::Nuit,
                            ThemeChoice::Clair,
                        ] {
                            ui.selectable_value(&mut th, t, i18n::tr(state.lang, theme_label(t)));
                        }
                    });
                if th != state.config.theme {
                    state.set_theme(th);
                }
                ui.end_row();

                ui.label(RichText::new(state.tr("settings.font_size")).color(p.muted));
                let mut size = state.config.font_size;
                let slider = ui.add(egui::Slider::new(&mut size, 11.0..=20.0).step_by(1.0).suffix(" px"));
                if slider.changed() {
                    state.config.font_size = size;
                }
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    state.save_config();
                }
                ui.end_row();

                ui.label(RichText::new(state.tr("settings.autofetch")).color(p.muted));
                let mut minutes = state.config.autofetch_minutes;
                let label = |m: u32| {
                    if m == 0 {
                        i18n::tr(state.lang, "settings.autofetch_off").to_string()
                    } else {
                        i18n::trf(state.lang, "settings.autofetch_every", &[("n", &m.to_string())])
                    }
                };
                egui::ComboBox::from_id_salt("autofetch").selected_text(label(minutes)).show_ui(ui, |ui| {
                    for m in [0, 1, 5, 15, 30] {
                        ui.selectable_value(&mut minutes, m, label(m));
                    }
                });
                if minutes != state.config.autofetch_minutes {
                    state.config.autofetch_minutes = minutes;
                    state.save_config();
                }
                ui.end_row();

                ui.label(RichText::new(state.tr("settings.avatars")).color(p.muted));
                let mut avatars = state.config.avatars;
                let hint = state.tr("settings.avatars_hint").to_string();
                if ui.checkbox(&mut avatars, "").on_hover_text(hint).changed() {
                    state.config.avatars = avatars;
                    state.save_config();
                }
                ui.end_row();
            });
        });
    if !open {
        state.show_settings = false;
    }
}
