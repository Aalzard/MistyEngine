//! Dark editor theme with the familiar grey-panel / blue-selection look.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const PANEL_BG: Color32 = Color32::from_rgb(56, 56, 56);
pub const TAB_BAR_BG: Color32 = Color32::from_rgb(40, 40, 40);
pub const TAB_ACTIVE: Color32 = Color32::from_rgb(60, 60, 60);
pub const TAB_INACTIVE: Color32 = Color32::from_rgb(45, 45, 45);
pub const BORDER: Color32 = Color32::from_rgb(25, 25, 25);
pub const FIELD_BG: Color32 = Color32::from_rgb(42, 42, 42);
pub const BUTTON_BG: Color32 = Color32::from_rgb(88, 88, 88);
pub const HOVER_BG: Color32 = Color32::from_rgb(103, 103, 103);
pub const SELECTION: Color32 = Color32::from_rgb(44, 93, 135);
pub const TEXT: Color32 = Color32::from_rgb(210, 210, 210);
pub const TEXT_DIM: Color32 = Color32::from_rgb(130, 130, 130);
/// Warnings and problems that need the user's attention.
pub const WARN: Color32 = Color32::from_rgb(230, 180, 60);
/// Names of prefab instances in the Hierarchy (and the prefab icon).
pub const PREFAB: Color32 = Color32::from_rgb(123, 174, 250);
pub const PREFAB_DIM: Color32 = Color32::from_rgb(78, 108, 155);
pub const PLAY_TINT: Color32 = Color32::from_rgb(70, 110, 170);
pub const AXIS_X: Color32 = Color32::from_rgb(219, 62, 29);
pub const AXIS_Y: Color32 = Color32::from_rgb(139, 220, 0);
pub const AXIS_Z: Color32 = Color32::from_rgb(58, 122, 237);

/// Use the OS UI font when present, with a symbol font as glyph fallback for icons.
fn load_fonts(ctx: &egui::Context) {
    let candidates: [(&str, &str, bool); 3] = [
        ("ui", "C:/Windows/Fonts/segoeui.ttf", true),
        ("symbols", "C:/Windows/Fonts/seguisym.ttf", false),
        (
            "symbols_linux",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            false,
        ),
    ];
    let mut fonts = egui::FontDefinitions::default();
    for (name, path, primary) in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(family.clone()).or_default();
            if primary && family == egui::FontFamily::Proportional {
                list.insert(0, name.to_owned());
            } else {
                list.push(name.to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    let mut v = Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL_BG;
    v.window_fill = PANEL_BG;
    v.extreme_bg_color = FIELD_BG;
    v.faint_bg_color = Color32::from_rgb(62, 62, 62);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = SELECTION;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = Color32::from_rgb(76, 126, 255);

    let r = CornerRadius::same(3);
    v.widgets.noninteractive.bg_fill = PANEL_BG;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = BUTTON_BG;
    v.widgets.inactive.weak_bg_fill = BUTTON_BG;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(36, 36, 36));
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = HOVER_BG;
    v.widgets.hovered.weak_bg_fill = HOVER_BG;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(36, 36, 36));
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.bg_fill = Color32::from_rgb(70, 96, 124);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(70, 96, 124);
    v.widgets.active.corner_radius = r;
    v.widgets.open.bg_fill = Color32::from_rgb(70, 70, 70);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(70, 70, 70);

    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 3.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size.y = 18.0;
    });
}

/// Dock style: dark tab strip, lighter active tab, thin separators.
pub fn dock_style(ctx: &egui::Context) -> egui_dock::Style {
    let mut s = egui_dock::Style::from_egui(&ctx.global_style());
    s.main_surface_border_stroke = Stroke::NONE;
    s.separator.width = 2.0;
    s.separator.color_idle = BORDER;
    s.separator.color_hovered = Color32::from_rgb(80, 80, 80);
    s.separator.color_dragged = SELECTION;
    s.tab_bar.bg_fill = TAB_BAR_BG;
    s.tab_bar.height = 22.0;
    s.tab_bar.hline_color = BORDER;
    s.tab.active.bg_fill = TAB_ACTIVE;
    s.tab.active.text_color = Color32::WHITE;
    s.tab.focused.bg_fill = TAB_ACTIVE;
    s.tab.focused.text_color = Color32::WHITE;
    s.tab.inactive.bg_fill = TAB_INACTIVE;
    s.tab.inactive.text_color = TEXT_DIM;
    s.tab.hovered.bg_fill = Color32::from_rgb(52, 52, 52);
    s.tab.hovered.text_color = TEXT;
    s.tab.tab_body.bg_fill = PANEL_BG;
    s.tab.tab_body.stroke = Stroke::NONE;
    s
}
