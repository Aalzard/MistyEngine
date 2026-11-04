//! Hub look: a calm dark (or light) theme with one blue accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

use super::config::HubTheme;

pub const ACCENT: Color32 = Color32::from_rgb(58, 128, 222);
pub const ERROR: Color32 = Color32::from_rgb(232, 96, 86);

/// Colours the Hub draws with besides egui's widgets.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub nav: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub dim: Color32,
    /// Text of projects whose folder is missing.
    pub missing: Color32,
}

pub fn palette(theme: HubTheme) -> Palette {
    match theme {
        HubTheme::Dark => Palette {
            bg: Color32::from_rgb(32, 33, 36),
            nav: Color32::from_rgb(24, 25, 27),
            card: Color32::from_rgb(42, 43, 47),
            card_hover: Color32::from_rgb(52, 54, 59),
            border: Color32::from_rgb(56, 58, 63),
            text: Color32::from_rgb(222, 224, 228),
            dim: Color32::from_rgb(140, 144, 152),
            missing: Color32::from_rgb(98, 101, 108),
        },
        HubTheme::Light => Palette {
            bg: Color32::from_rgb(246, 247, 249),
            nav: Color32::from_rgb(234, 236, 240),
            card: Color32::from_rgb(255, 255, 255),
            card_hover: Color32::from_rgb(238, 242, 249),
            border: Color32::from_rgb(214, 218, 224),
            text: Color32::from_rgb(32, 34, 38),
            dim: Color32::from_rgb(104, 110, 120),
            missing: Color32::from_rgb(168, 172, 180),
        },
    }
}

/// Apply the theme's visuals and spacing to the whole window.
pub fn apply(ctx: &egui::Context, theme: HubTheme) {
    let p = palette(theme);
    let mut v = match theme {
        HubTheme::Dark => Visuals::dark(),
        HubTheme::Light => Visuals::light(),
    };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = match theme {
        HubTheme::Dark => Color32::from_rgb(22, 23, 25),
        HubTheme::Light => Color32::WHITE,
    };
    v.window_stroke = Stroke::new(1.0, p.border);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = r;
    }
    v.widgets.inactive.weak_bg_fill = p.card;
    v.widgets.inactive.bg_fill = p.card;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.hovered.weak_bg_fill = p.card_hover;
    v.widgets.hovered.bg_fill = p.card_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        // Rows and cards take the clicks, not text selection.
        s.interaction.selectable_labels = false;
        use egui::{FontId, TextStyle};
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
    });
}

/// The filled accent button (New Project, Create).
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.to_owned())
            .color(Color32::WHITE)
            .strong(),
    )
    .fill(ACCENT)
    .stroke(Stroke::NONE)
}
