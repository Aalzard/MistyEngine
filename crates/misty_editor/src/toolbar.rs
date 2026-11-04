//! Main toolbar: Play/Pause/Step in the centre (flat icon buttons), Layers and Layout on the
//! right. The logo and menus are in the title bar above (`title_bar`).
//! Transform tools and handle toggles live in the Scene view overlay.

use egui::{
    Align, Color32, Layout, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType,
};

use crate::actions::Action;
use crate::state::{EditorState, PlayState};
use crate::theme;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            egui::ComboBox::from_id_salt("layout_dropdown")
                .selected_text(state.layouts.current_name.clone())
                .width(80.0)
                .show_ui(ui, |ui| crate::menu_bar::layouts_menu(ui, state));
            egui::ComboBox::from_id_salt("layers_dropdown")
                .selected_text("Layers")
                .width(80.0)
                .show_ui(ui, |ui| {
                    for layer in ["Default", "TransparentFX", "Ignore Raycast", "Water", "UI"] {
                        ui.label(format!("👁 {layer}"));
                    }
                });
            // Green "⬆ Update" pill, only while an update is available.
            if crate::updater::ui::update_button(ui, &state.updater) {
                crate::updater::editor::request_install(state);
            }
        });
    });

    // Centre: Play / Pause / Step, drawn on top of the full toolbar width.
    let size = egui::vec2(3.0 * PLAY_BUTTON.x + 2.0 * 2.0, PLAY_BUTTON.y);
    let rect = egui::Rect::from_center_size(full.center(), size);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal_centered(|ui| play_controls(ui, state));
    });
}

/// Size of one play-control button.
const PLAY_BUTTON: Vec2 = egui::vec2(32.0, 24.0);

#[derive(Clone, Copy)]
enum PlayIcon {
    Play,
    Pause,
    Step,
}

fn play_controls(ui: &mut Ui, state: &mut EditorState) {
    let playing = state.play.in_play_mode();
    let paused = state.play == PlayState::Paused || (!playing && state.pause_on_play);
    let play = play_button(ui, PlayIcon::Play, playing, "Play", "Play (Ctrl+P)");
    crate::tutorials::targets::mark_response(ui, "toolbar.play", &play);
    if play.clicked() {
        state.push(Action::TogglePlay);
    }
    if play_button(ui, PlayIcon::Pause, paused, "Pause", "Pause (Ctrl+Shift+P)").clicked() {
        state.push(Action::TogglePause);
    }
    if play_button(ui, PlayIcon::Step, false, "Step", "Step (Ctrl+Alt+P)").clicked() {
        state.push(Action::Step);
    }
}

/// A flat play-control button: no frame or outline, a soft highlight on hover, and the accent
/// colour (icon and a faint fill) while `active`.
fn play_button(ui: &mut Ui, icon: PlayIcon, active: bool, name: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(PLAY_BUTTON, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, name));
    let accent = crate::branding::icons::accent(ui.visuals());
    let fill = if active {
        Some(accent.gamma_multiply(0.22))
    } else if resp.is_pointer_button_down_on() {
        Some(Color32::from_white_alpha(26))
    } else if resp.hovered() {
        Some(Color32::from_white_alpha(14))
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    let color = if active { accent } else { theme::TEXT };
    paint_play_icon(ui, rect.center(), icon, color);
    resp.on_hover_text(tip)
}

/// The icons as filled shapes, so they stay crisp at any scale.
fn paint_play_icon(ui: &Ui, c: Pos2, icon: PlayIcon, color: Color32) {
    let p = ui.painter();
    let triangle = |left: f32, w: f32, h: f32| {
        let pts = vec![
            egui::pos2(left, c.y - h),
            egui::pos2(left + w, c.y),
            egui::pos2(left, c.y + h),
        ];
        egui::Shape::convex_polygon(pts, color, Stroke::NONE)
    };
    match icon {
        PlayIcon::Play => {
            p.add(triangle(c.x - 4.0, 9.0, 5.5));
        }
        PlayIcon::Pause => {
            for x in [c.x - 4.0, c.x + 1.0] {
                let bar = Rect::from_min_size(egui::pos2(x, c.y - 5.5), egui::vec2(3.0, 11.0));
                p.rect_filled(bar, 0.5, color);
            }
        }
        PlayIcon::Step => {
            p.add(triangle(c.x - 5.0, 7.5, 5.0));
            let bar = Rect::from_min_size(egui::pos2(c.x + 3.0, c.y - 5.0), egui::vec2(2.0, 10.0));
            p.rect_filled(bar, 0.5, color);
        }
    }
}
