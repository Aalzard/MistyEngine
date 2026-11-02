//! Inspector panel: GameObject header and one foldout per component.
//! With several objects selected, edits apply to all of them (only shared components are shown).

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

    // A locked Inspector keeps showing the objects it was locked on.
    if !state.inspector_locked {
        state.inspector_targets = state.selection.list().to_vec();
        if let Some(a) = state.selection.active() {
            // Active object first: its values are the ones displayed.
            state.inspector_targets.retain(|e| *e != a);
            state.inspector_targets.insert(0, a);
        }
    }
    let scene = &state.scene;
    state.inspector_targets.retain(|e| scene.contains(*e));
    let targets = state.inspector_targets.clone();
    let Some(&e) = targets.first() else { return };

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            header(ui, state, &targets);
            if targets.len() > 1 {
                ui.label(
                    RichText::new(format!("{} objects selected", targets.len()))
                        .color(theme::TEXT_DIM),
                );
            }
            ui.separator();
            for name in shared_components(state, &targets) {
                component_section(ui, state, &targets, &name);
            }
            ui.add_space(8.0);
            editors::add_component_button(ui, state, e);
        });
}

/// Components present on every target, in the active object's order.
fn shared_components(state: &EditorState, targets: &[Entity]) -> Vec<String> {
    let names = state.scene.component_names(targets[0]);
    names
        .into_iter()
        .filter(|n| {
            registry::find(n)
                .is_some_and(|info| targets.iter().all(|t| (info.has)(&state.scene.world, *t)))
        })
        .collect()
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

/// Icon, active toggle, name, Static; then Tag and Layer. Name edits only the active object.
fn header(ui: &mut Ui, state: &mut EditorState, targets: &[Entity]) {
    let e = targets[0];
    let icon = object_icon(&state.scene, e);
    let before = (*state.scene.game_object(e)).clone();
    let mut go = before.clone();
    let multi = targets.len() > 1;
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(22.0));
        changed |= ui.checkbox(&mut go.active, "").changed();
        let name_w = ui.available_width() - 70.0;
        if multi {
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut "—".to_owned()).desired_width(name_w),
            );
        } else {
            changed |= ui
                .add(egui::TextEdit::singleline(&mut go.name).desired_width(name_w))
                .changed();
        }
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
    if !changed {
        return;
    }
    for &t in targets {
        let mut g = state.scene.game_object_mut(t);
        if go.active != before.active {
            g.active = go.active;
        }
        if go.is_static != before.is_static {
            g.is_static = go.is_static;
        }
        if go.tag != before.tag {
            g.tag = go.tag.clone();
        }
        if go.layer != before.layer {
            g.layer = go.layer;
        }
        if t == e && go.name != before.name {
            g.name = go.name.clone();
        }
    }
    state.mark_edited("GameObject");
}

/// Foldout header with enable toggle and ⋮ menu, then the component's fields.
fn component_section(ui: &mut Ui, state: &mut EditorState, targets: &[Entity], name: &str) {
    let e = targets[0];
    let id = ui.make_persistent_id(("component", name));
    let removable = registry::find(name).is_some_and(|i| i.removable);
    let open_state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

    let header = open_state.show_header(ui, |ui| {
        if let Some(enabled) = editors::enabled_flag(state, e, name) {
            let mut on = enabled;
            if ui.checkbox(&mut on, "").changed() {
                for &t in targets {
                    editors::set_enabled(state, t, name, on);
                }
                state.mark_edited(name);
            }
        }
        ui.label(RichText::new(format!("{} {name}", editors::icon(name))).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⋮", |ui| component_menu(ui, state, e, name, removable));
        });
    });
    header.body(|ui| {
        let before = to_json(state, e, name);
        if editors::edit(ui, state, targets, name) {
            if name != "Transform" {
                propagate(state, targets, name, before);
            }
            state.mark_edited(name);
        }
    });
    ui.separator();
}

fn component_menu(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str, removable: bool) {
    let n = name.to_owned();
    let act = |ui: &mut Ui, state: &mut EditorState, label: &str, enabled: bool, action: Action| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            state.push(action);
            ui.close();
        }
    };
    act(
        ui,
        state,
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
        state,
        "Remove Component",
        removable,
        Action::RemoveComponent {
            entity: e,
            name: n.clone(),
        },
    );
    act(
        ui,
        state,
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
        state,
        "Move Down",
        removable,
        Action::MoveComponent {
            entity: e,
            name: n.clone(),
            dir: 1,
        },
    );
    ui.separator();
    if ui.button("Copy Component").clicked() {
        state.component_clipboard = to_json(state, e, name).map(|v| (n.clone(), v));
        ui.close();
    }
    let can_paste = state
        .component_clipboard
        .as_ref()
        .is_some_and(|(c, _)| c == name);
    act(
        ui,
        state,
        "Paste Component Values",
        can_paste,
        Action::PasteComponentValues { entity: e, name: n },
    );
}

fn to_json(state: &EditorState, e: Entity, name: &str) -> Option<serde_json::Value> {
    registry::find(name).and_then(|info| (info.to_json)(&state.scene.world, e))
}

/// Copy the fields that changed on the active object to the other targets.
fn propagate(
    state: &mut EditorState,
    targets: &[Entity],
    name: &str,
    before: Option<serde_json::Value>,
) {
    let Some(info) = registry::find(name) else {
        return;
    };
    let (Some(serde_json::Value::Object(before)), Some(serde_json::Value::Object(after))) =
        (before, to_json(state, targets[0], name))
    else {
        return;
    };
    let changed: Vec<(&String, &serde_json::Value)> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .collect();
    for &t in &targets[1..] {
        let Some(serde_json::Value::Object(mut obj)) = (info.to_json)(&state.scene.world, t) else {
            continue;
        };
        for (k, v) in &changed {
            obj.insert((*k).clone(), (*v).clone());
        }
        let _ = (info.from_json)(&mut state.scene.world, t, serde_json::Value::Object(obj));
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::Camera;

    use super::*;
    use crate::commands;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(std::env::temp_dir().join("misty_insp_test")))
    }

    fn apply_actions(s: &mut EditorState) {
        for a in std::mem::take(&mut s.actions) {
            commands::apply(s, a);
        }
    }

    #[test]
    fn add_component_popup_adds_light() {
        let mut s = state();
        let cam = s.scene.roots()[0];
        s.selection.set(cam);
        let mut h = Harness::builder()
            .with_size([400.0, 700.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h.get_by_label("Add Component").click();
        h.run();
        h.get_by_label("Light").click();
        h.run();
        apply_actions(h.state_mut());
        assert_eq!(h.state().scene.component_names(cam), ["Transform", "Camera", "Light"]);
        assert!(!h.state().inspector_add_open, "popup closes after choosing");
    }

    #[test]
    fn multi_edit_propagates_only_changed_fields() {
        let mut s = state();
        let a = s.scene.spawn("A", None);
        let b = s.scene.spawn("B", None);
        s.scene.add(a, Camera::default());
        s.scene.add(b, Camera { near: 2.0, ..Default::default() });
        let before = to_json(&s, a, "Camera");
        s.scene.world.get::<&mut Camera>(a).unwrap().fov = 90.0;
        propagate(&mut s, &[a, b], "Camera", before);
        let cb = s.scene.world.get::<&Camera>(b).unwrap();
        assert_eq!(cb.fov, 90.0);
        assert_eq!(cb.near, 2.0, "unchanged fields keep their own values");
    }

    #[test]
    fn copy_paste_component_values() {
        let mut s = state();
        let (a, b) = (s.scene.spawn("A", None), s.scene.spawn("B", None));
        s.scene.add(a, Camera { fov: 30.0, ..Default::default() });
        s.scene.add(b, Camera::default());
        s.component_clipboard = to_json(&s, a, "Camera").map(|v| ("Camera".into(), v));
        s.push(Action::PasteComponentValues { entity: b, name: "Camera".into() });
        apply_actions(&mut s);
        assert_eq!(s.scene.world.get::<&Camera>(b).unwrap().fov, 30.0);
        s.push(Action::Undo);
        apply_actions(&mut s);
        assert_eq!(s.scene.world.get::<&Camera>(b).unwrap().fov, 60.0);
    }
}
