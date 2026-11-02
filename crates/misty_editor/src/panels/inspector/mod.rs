//! Inspector panel: GameObject header and one foldout per component.

mod editors;

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::registry;

use crate::actions::Action;
use crate::state::{EditorState, object_icon};
use crate::theme;

pub const TAGS: [&str; 7] = [
    "Untagged",
    "Respawn",
    "Finish",
    "EditorOnly",
    "MainCamera",
    "Player",
    "GameController",
];
pub const LAYERS: [&str; 6] = [
    "Default",
    "TransparentFX",
    "Ignore Raycast",
    "Water",
    "UI",
    "Player",
];

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    lock_row(ui, state);

    // A locked Inspector keeps showing the object it was locked on.
    let target = if state.inspector_locked {
        state.inspector_target.filter(|e| state.scene.contains(*e))
    } else {
        state.selection.active()
    };
    state.inspector_target = target;
    let Some(e) = target else { return };

    let count = state.selection.list().len();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            header(ui, state, e);
            if count > 1 && !state.inspector_locked {
                ui.label(
                    RichText::new(format!("{count} objects selected — editing the active one"))
                        .color(theme::TEXT_DIM),
                );
            }
            ui.separator();
            for name in state.scene.component_names(e) {
                component_section(ui, state, e, &name);
            }
            ui.add_space(8.0);
            editors::add_component_button(ui, state, e);
        });
}

fn lock_row(ui: &mut Ui, state: &mut EditorState) {
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
}

/// Icon, active toggle, name, Static; then Tag and Layer.
fn header(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let icon = object_icon(&state.scene, e);
    let mut changed = false;
    let mut go = (*state.scene.game_object(e)).clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(22.0));
        changed |= ui.checkbox(&mut go.active, "").changed();
        let name_w = ui.available_width() - 70.0;
        changed |= ui
            .add(egui::TextEdit::singleline(&mut go.name).desired_width(name_w))
            .changed();
        changed |= ui.checkbox(&mut go.is_static, "Static").changed();
    });
    ui.horizontal(|ui| {
        ui.label("Tag");
        egui::ComboBox::from_id_salt("insp_tag")
            .selected_text(go.tag.as_str())
            .width(110.0)
            .show_ui(ui, |ui| {
                for t in TAGS {
                    if ui.selectable_label(go.tag == t, t).clicked() {
                        go.tag = t.to_owned();
                        changed = true;
                    }
                }
            });
        ui.label("Layer");
        let layer_name = LAYERS.get(go.layer as usize).copied().unwrap_or("Default");
        egui::ComboBox::from_id_salt("insp_layer")
            .selected_text(layer_name)
            .width(110.0)
            .show_ui(ui, |ui| {
                for (i, l) in LAYERS.iter().enumerate() {
                    changed |= ui.selectable_value(&mut go.layer, i as u8, *l).changed();
                }
            });
    });
    if changed {
        *state.scene.game_object_mut(e) = go;
        state.mark_edited("GameObject");
    }
}

/// Foldout header with enable toggle and ⋮ menu, then the component's fields.
fn component_section(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) {
    let id = ui.make_persistent_id(("component", name));
    let removable = registry::find(name).is_some_and(|i| i.removable);
    let open_state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

    let header = open_state.show_header(ui, |ui| {
        if let Some(enabled) = editors::enabled_flag(state, e, name) {
            let mut on = enabled;
            if ui.checkbox(&mut on, "").changed() {
                editors::set_enabled(state, e, name, on);
                state.mark_edited(name);
            }
        }
        ui.label(RichText::new(format!("{} {name}", editors::icon(name))).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⋮", |ui| {
                let mut act = |ui: &mut Ui, label: &str, enabled: bool, action: Action| {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        state.push(action);
                        ui.close();
                    }
                };
                let n = name.to_owned();
                act(
                    ui,
                    "Reset",
                    true,
                    Action::ResetComponent {
                        entity: e,
                        name: n.clone(),
                    },
                );
                ui.separator();
                act(
                    ui,
                    "Remove Component",
                    removable,
                    Action::RemoveComponent {
                        entity: e,
                        name: n.clone(),
                    },
                );
                act(
                    ui,
                    "Move Up",
                    removable,
                    Action::MoveComponent {
                        entity: e,
                        name: n.clone(),
                        dir: -1,
                    },
                );
                act(
                    ui,
                    "Move Down",
                    removable,
                    Action::MoveComponent {
                        entity: e,
                        name: n,
                        dir: 1,
                    },
                );
            });
        });
    });
    header.body(|ui| {
        if editors::edit(ui, state, e, name) {
            state.mark_edited(name);
        }
    });
    ui.separator();
}
