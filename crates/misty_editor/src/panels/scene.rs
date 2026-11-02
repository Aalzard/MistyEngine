//! Scene view: toolbar strip and the viewport area.

use egui::Ui;

use crate::state::EditorState;
use crate::widgets::{panel_toolbar, search_field, toggle_button};

const DRAW_MODES: [&str; 3] = ["Shaded", "Wireframe", "Shaded Wireframe"];

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    toolbar(ui, state);
    let (rect, _resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_rgb(70, 75, 82));
}

fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    let sv = &mut state.scene_view;
    panel_toolbar(ui, |ui| {
        egui::ComboBox::from_id_salt("scene_draw_mode")
            .selected_text(DRAW_MODES[sv.draw_mode])
            .width(120.0)
            .show_ui(ui, |ui| {
                for (i, m) in DRAW_MODES.iter().enumerate() {
                    ui.selectable_value(&mut sv.draw_mode, i, *m);
                }
            });
        if toggle_button(ui, sv.is_2d, "2D", "Toggle 2D view").clicked() {
            sv.is_2d = !sv.is_2d;
        }
        if toggle_button(ui, sv.lighting, "💡", "Toggle scene lighting").clicked() {
            sv.lighting = !sv.lighting;
        }
        if toggle_button(ui, sv.audio, "🔊", "Toggle audio").clicked() {
            sv.audio = !sv.audio;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut String::new(), 120.0);
            if toggle_button(ui, sv.gizmos, "Gizmos", "Toggle gizmos").clicked() {
                sv.gizmos = !sv.gizmos;
            }
        });
    });
}
