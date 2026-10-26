//! Colour themes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    System,
    #[default]
    Ardoise,
    SombreChaleureux,
    Nuit,
    Clair,
}

use eframe::egui::{self, Color32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Ardoise,
    SombreChaleureux,
    Nuit,
    Clair,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub panel: Color32,
    pub border: Color32,
    pub selection: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub modified: Color32,
    pub added: Color32,
    pub deleted: Color32,
    pub conflict: Color32,
    /// Remote branch labels (blue in ungit) and tag labels (yellow in ungit).
    pub remote: Color32,
    pub tag: Color32,
    pub lanes: &'static [Color32],
    pub dark: bool,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Dark mode of the Vuexy/Vuetify theme (as in Cofina Report): neutral greys, lavender text,
/// violet primary, bluish cards. The primary is darkened a hair (#7367F0 → #6E63E6) so white
/// button text reaches a 4.5:1 contrast.
static ARDOISE: Palette = Palette {
    bg: rgb(0x212121),
    sidebar: rgb(0x252526),
    panel: rgb(0x2f3349),
    border: rgb(0x404146),
    selection: rgb(0x302e46),
    text: rgb(0xd0d4f1),
    muted: rgb(0xa8aaae),
    accent: rgb(0x6e63e6),
    on_accent: rgb(0xffffff),
    modified: rgb(0xff9f43),
    added: rgb(0x28c76f),
    deleted: rgb(0xea5455),
    conflict: rgb(0xea5455),
    remote: rgb(0x00cfe8),
    tag: rgb(0xffd166),
    lanes: &[
        rgb(0x7367f0),
        rgb(0x28c76f),
        rgb(0x00cfe8),
        rgb(0xff9f43),
        rgb(0xea5455),
        rgb(0xb9a6ff),
        rgb(0xffd166),
        rgb(0x4dd4ac),
    ],
    dark: true,
};

static SOMBRE_CHALEUREUX: Palette = Palette {
    bg: rgb(0x1e1e2e),
    sidebar: rgb(0x181825),
    panel: rgb(0x24273a),
    border: rgb(0x313244),
    selection: rgb(0x313244),
    text: rgb(0xcdd6f4),
    muted: rgb(0x9399b2),
    accent: rgb(0xf5a97f),
    on_accent: rgb(0x1e1e2e),
    modified: rgb(0xf9e2af),
    added: rgb(0xa6e3a1),
    deleted: rgb(0xf38ba8),
    conflict: rgb(0xfab387),
    remote: rgb(0x89b4fa),
    tag: rgb(0xf9e2af),
    lanes: &[
        rgb(0xf5a97f),
        rgb(0xcba6f7),
        rgb(0xa6e3a1),
        rgb(0x89b4fa),
        rgb(0xf9e2af),
        rgb(0xf5c2e7),
        rgb(0x89dceb),
        rgb(0xfab387),
    ],
    dark: true,
};

static NUIT: Palette = Palette {
    bg: rgb(0x0f1117),
    sidebar: rgb(0x151821),
    panel: rgb(0x1a1e29),
    border: rgb(0x262b38),
    selection: rgb(0x222839),
    text: rgb(0xe6e8ee),
    muted: rgb(0x8a90a2),
    accent: rgb(0x7c9cff),
    on_accent: rgb(0x0f1117),
    modified: rgb(0xe5b454),
    added: rgb(0x5fd3a6),
    deleted: rgb(0xf07178),
    conflict: rgb(0xff9e64),
    remote: rgb(0x5ccfe6),
    tag: rgb(0xe5b454),
    lanes: &[
        rgb(0x7c9cff),
        rgb(0xf78fb3),
        rgb(0x5fd3a6),
        rgb(0xe5b454),
        rgb(0xc39bff),
        rgb(0x5ccfe6),
        rgb(0xff9e64),
        rgb(0x9ece6a),
    ],
    dark: true,
};

static CLAIR: Palette = Palette {
    bg: rgb(0xf7f8fa),
    sidebar: rgb(0xeef0f4),
    panel: rgb(0xffffff),
    border: rgb(0xe1e4ea),
    selection: rgb(0xe8eefc),
    text: rgb(0x1d2230),
    muted: rgb(0x6b7285),
    accent: rgb(0x3b6cf6),
    on_accent: rgb(0xffffff),
    modified: rgb(0xb7791f),
    added: rgb(0x15803d),
    deleted: rgb(0xdc2626),
    conflict: rgb(0xea580c),
    remote: rgb(0x0e7490),
    tag: rgb(0x8a5a00),
    lanes: &[
        rgb(0x3b6cf6),
        rgb(0xe0457b),
        rgb(0x14a37f),
        rgb(0xc98a00),
        rgb(0x8a5cf6),
        rgb(0x0891b2),
        rgb(0xea580c),
        rgb(0x4d7c0f),
    ],
    dark: false,
};

pub fn palette(t: Theme) -> &'static Palette {
    match t {
        Theme::Ardoise => &ARDOISE,
        Theme::SombreChaleureux => &SOMBRE_CHALEUREUX,
        Theme::Nuit => &NUIT,
        Theme::Clair => &CLAIR,
    }
}

pub fn resolve(choice: ThemeChoice) -> Theme {
    match choice {
        ThemeChoice::Ardoise => Theme::Ardoise,
        ThemeChoice::SombreChaleureux => Theme::SombreChaleureux,
        ThemeChoice::Nuit => Theme::Nuit,
        ThemeChoice::Clair => Theme::Clair,
        ThemeChoice::System => match dark_light::detect() {
            Ok(dark_light::Mode::Light) => Theme::Clair,
            _ => Theme::Ardoise,
        },
    }
}

fn visuals(p: &Palette) -> egui::Visuals {
    use egui::{CornerRadius, Stroke};
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let radius = CornerRadius::same(6);
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.muted);
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = if p.dark { p.sidebar } else { p.panel };
    v.faint_bg_color = p.panel;
    v.code_bg_color = p.sidebar;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.modified;
    v.error_fg_color = p.deleted;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for (state, fill) in [
        (&mut w.inactive, p.panel),
        (&mut w.hovered, p.selection),
        (&mut w.active, p.selection),
        (&mut w.open, p.selection),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, p.border);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
    }
    w.noninteractive.corner_radius = radius;
    // Slider rails and check boxes use `bg_fill`: give them contrast against panels.
    w.inactive.bg_fill = p.border;
    v.slider_trailing_fill = true;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    v
}

pub fn apply(ctx: &egui::Context, p: &Palette, font_size: f32) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let vis = visuals(p);
    ctx.all_styles_mut(|style| {
        style.visuals = vis.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = font_size + 10.0;
        style.text_styles = [
            (TextStyle::Small, FontId::new(font_size * 0.8, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(font_size, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(font_size, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(font_size * 1.35, FontFamily::Name("semibold".into()))),
            (TextStyle::Monospace, FontId::new(font_size * 0.92, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// Embeds Inter (OFL) as the proportional font and a `semibold` family for headings.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    use std::sync::Arc;
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))),
    );
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    let mut semibold = vec!["inter-semibold".to_string()];
    semibold.extend(fallbacks);
    fonts.families.insert(FontFamily::Name("semibold".into()), semibold);
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Theme; 4] = [Theme::Ardoise, Theme::SombreChaleureux, Theme::Nuit, Theme::Clair];

    #[test]
    fn ardoise_exact_values() {
        // Dark mode of the Vuexy/Vuetify theme used by Cofina Report.
        let p = palette(Theme::Ardoise);
        let got: Vec<String> =
            [p.bg, p.sidebar, p.panel, p.border, p.selection, p.text, p.muted, p.accent].map(hex).to_vec();
        assert_eq!(got, ["#212121", "#252526", "#2f3349", "#404146", "#302e46", "#d0d4f1", "#a8aaae", "#6e63e6"]);
        assert_eq!([p.added, p.modified, p.deleted].map(hex), ["#28c76f", "#ff9f43", "#ea5455"]);
        assert_eq!([p.remote, p.tag].map(hex), ["#00cfe8", "#ffd166"]);
        assert!(p.dark);
    }

    #[test]
    fn ref_label_colours_are_readable() {
        for t in ALL {
            let p = palette(t);
            assert!(contrast(p.remote, p.bg) >= 4.5, "{t:?} remote");
            assert!(contrast(p.tag, p.bg) >= 4.5, "{t:?} tag");
        }
    }

    #[test]
    fn ardoise_is_the_default() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Ardoise);
        assert_eq!(resolve(ThemeChoice::Ardoise), Theme::Ardoise);
    }

    fn hex(c: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
    }

    fn luminance(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn sombre_chaleureux_exact_values() {
        let p = palette(Theme::SombreChaleureux);
        let got: Vec<String> =
            [p.bg, p.sidebar, p.panel, p.border, p.selection, p.text, p.muted, p.accent].map(hex).to_vec();
        assert_eq!(got, ["#1e1e2e", "#181825", "#24273a", "#313244", "#313244", "#cdd6f4", "#9399b2", "#f5a97f"]);
        let lanes: Vec<String> = p.lanes.iter().copied().map(hex).collect();
        assert_eq!(lanes, ["#f5a97f", "#cba6f7", "#a6e3a1", "#89b4fa", "#f9e2af", "#f5c2e7", "#89dceb", "#fab387"]);
        assert!(p.dark);
    }

    #[test]
    fn each_palette_has_8_lanes() {
        for t in ALL {
            assert_eq!(palette(t).lanes.len(), 8, "{t:?}");
        }
    }

    #[test]
    fn contrast_text_on_bg_ge_7() {
        for t in ALL {
            let p = palette(t);
            assert!(contrast(p.text, p.bg) >= 7.0, "{t:?}");
            assert!(contrast(p.text, p.panel) >= 7.0, "{t:?}");
        }
    }

    #[test]
    fn contrast_muted_on_bg_ge_4_5() {
        for t in ALL {
            let p = palette(t);
            assert!(contrast(p.muted, p.bg) >= 4.5, "{t:?} {}", contrast(p.muted, p.bg));
            assert!(contrast(p.muted, p.panel) >= 4.5, "{t:?} {}", contrast(p.muted, p.panel));
        }
    }

    #[test]
    fn accent_button_text_readable() {
        for t in ALL {
            let p = palette(t);
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "{t:?} {}", contrast(p.on_accent, p.accent));
        }
    }

    #[test]
    fn explicit_choices_resolve() {
        assert_eq!(resolve(ThemeChoice::Nuit), Theme::Nuit);
        assert_eq!(resolve(ThemeChoice::Clair), Theme::Clair);
        assert_eq!(resolve(ThemeChoice::SombreChaleureux), Theme::SombreChaleureux);
    }

    #[test]
    fn apply_sets_visuals() {
        let ctx = egui::Context::default();
        let p = palette(Theme::Clair);
        apply(&ctx, p, 15.0);
        let style = ctx.global_style();
        assert!(!style.visuals.dark_mode);
        assert_eq!(style.visuals.panel_fill, p.bg);
        assert_eq!(style.text_styles[&egui::TextStyle::Body].size, 15.0);
        assert_eq!(style.visuals.widgets.inactive.bg_fill, p.border);
        assert!(style.visuals.slider_trailing_fill);
    }
}
