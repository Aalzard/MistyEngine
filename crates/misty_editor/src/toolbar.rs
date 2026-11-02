//! Main toolbar: transform tools, pivot/space toggles, Play/Pause/Step, Layers and Layout.

use egui::{Align, Layout, RichText, Ui};

use crate::actions::Action;
use crate::state::{EditorState, PlayState, Tool};
use crate::theme;
use crate::widgets::toggle_button;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.horizontal(|ui| {
        // Left: transform tools.
        for tool in Tool::ALL {
            if toggle_button(ui, state.tool == tool, tool.icon(), tool.tooltip()).clicked() {
                state.tool = tool;
            }
        }
        ui.add_space(8.0);
        let pivot = if state.pivot_center {
            "◎ Center"
        } else {
            "◉ Pivot"
        };
        if ui
            .button(pivot)
            .on_hover_text("Toggle Tool Handle Position")
            .clicked()
        {
            state.pivot_center = !state.pivot_center;
        }
        let space = if state.global_space {
            "🌐 Global"
        } else {
            "⬚ Local"
        };
        if ui
            .button(space)
            .on_hover_text("Toggle Tool Handle Rotation")
            .clicked()
        {
            state.global_space = !state.global_space;
        }

        // Right: Layout and Layers dropdowns.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            egui::ComboBox::from_id_salt("layout_dropdown")
                .selected_text("Layout")
                .width(80.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(false, "Default").clicked() {
                        state.push(Action::ResetLayout);
                    }
                });
            egui::ComboBox::from_id_salt("layers_dropdown")
                .selected_text("Layers")
                .width(80.0)
                .show_ui(ui, |ui| {
                    for layer in ["Default", "TransparentFX", "Ignore Raycast", "Water", "UI"] {
                        ui.label(format!("👁 {layer}"));
                    }
                });
        });
    });

    // Centre: Play / Pause / Step, drawn on top of the full toolbar width.
    let center = egui::pos2(full.center().x, full.center().y);
    let size = egui::vec2(100.0, 22.0);
    let rect = egui::Rect::from_center_size(center, size);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| play_controls(ui, state));
    });
}

fn play_controls(ui: &mut Ui, state: &mut EditorState) {
    let playing = state.play.in_play_mode();
    let paused = state.play == PlayState::Paused;
    let play_text = RichText::new("▶").color(if playing {
        theme::PLAY_TINT
    } else {
        theme::TEXT
    });
    if toggle_button(ui, playing, play_text, "Play (Ctrl+P)").clicked() {
        state.push(Action::TogglePlay);
    }
    if toggle_button(ui, paused, "⏸", "Pause (Ctrl+Shift+P)").clicked() {
        state.push(Action::TogglePause);
    }
    if toggle_button(ui, false, "⏭", "Step (Ctrl+Alt+P)").clicked() {
        state.push(Action::Step);
    }
}
