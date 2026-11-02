//! Inspector panel: header (active, name, static, tag, layer) and component foldouts.

use egui::{RichText, Ui};

use crate::actions::Action;
use crate::state::EditorState;
use crate::widgets::vec3_row;

const TAGS: [&str; 7] = [
    "Untagged",
    "Respawn",
    "Finish",
    "EditorOnly",
    "MainCamera",
    "Player",
    "GameController",
];
const LAYERS: [&str; 5] = ["Default", "TransparentFX", "Ignore Raycast", "Water", "UI"];

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lock = if state.inspector_locked {
                "🔒"
            } else {
                "🔓"
            };
            if ui
                .add(egui::Button::new(lock).frame(false))
                .on_hover_text("Lock Inspector")
                .clicked()
            {
                state.inspector_locked = !state.inspector_locked;
            }
        });
    });

    let Some(index) = state.selected else {
        return;
    };
    let Some(obj) = state.objects.get_mut(index) else {
        return;
    };
    let mut changed = false;
    let mut add_component = false;

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Header row 1: active toggle, name, static.
            ui.horizontal(|ui| {
                ui.label(RichText::new(obj.icon).size(22.0));
                changed |= ui.checkbox(&mut obj.active, "").changed();
                let name_w = ui.available_width() - 70.0;
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut obj.name).desired_width(name_w))
                    .changed();
                changed |= ui.checkbox(&mut obj.is_static, "Static").changed();
            });
            // Header row 2: tag and layer.
            ui.horizontal(|ui| {
                ui.label("Tag");
                changed |= dropdown(ui, "tag", &mut obj.tag, &TAGS);
                ui.label("Layer");
                changed |= dropdown(ui, "layer", &mut obj.layer, &LAYERS);
            });
            ui.separator();

            // Transform foldout (always present, cannot be disabled).
            egui::CollapsingHeader::new(RichText::new("⬚ Transform").strong())
                .default_open(true)
                .show(ui, |ui| {
                    changed |= vec3_row(ui, "Position", &mut obj.position, 0.1);
                    changed |= vec3_row(ui, "Rotation", &mut obj.rotation, 1.0);
                    changed |= vec3_row(ui, "Scale", &mut obj.scale, 0.01);
                });
            ui.separator();

            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                add_component = ui
                    .add_sized([220.0, 22.0], egui::Button::new("Add Component"))
                    .clicked();
            });
        });

    if changed {
        state.scene_dirty = true;
    }
    if add_component {
        state.push(Action::NotImplemented("Add Component"));
    }
}

fn dropdown(ui: &mut Ui, id: &str, value: &mut String, options: &[&str]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.as_str())
        .width(110.0)
        .show_ui(ui, |ui| {
            for opt in options {
                if ui.selectable_label(value == opt, *opt).clicked() {
                    *value = (*opt).to_owned();
                    changed = true;
                }
            }
        });
    changed
}
