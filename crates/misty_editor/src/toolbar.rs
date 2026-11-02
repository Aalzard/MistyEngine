//! Main toolbar: Play/Pause/Step in the centre, Layers and Layout on the right.
//! Transform tools and handle toggles live in the Scene view overlay.

use egui::{Align, Layout, RichText, Ui};

use crate::actions::Action;
use crate::state::{EditorState, PlayState};
use crate::theme;
use crate::widgets::toggle_button;

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.horizontal(|ui| {
        ui.label(RichText::new("◆ Misty").strong());
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
        });
    });

    // Centre: Play / Pause / Step, drawn on top of the full toolbar width.
    let rect = egui::Rect::from_center_size(full.center(), egui::vec2(100.0, 22.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| play_controls(ui, state));
    });
}

fn play_controls(ui: &mut Ui, state: &mut EditorState) {
    let playing = state.play.in_play_mode();
    let paused = state.play == PlayState::Paused || (!playing && state.pause_on_play);
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
