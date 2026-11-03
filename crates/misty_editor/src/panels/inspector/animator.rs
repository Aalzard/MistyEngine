//! Inspector editors for the Animator: the component (controller slot, speed), and a
//! `.controller` asset showing the state or transition selected in the Animator window.

use egui::{RichText, Ui};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::controller::{
    BlendChild, BlendTree, Condition, Node, ParamKind, State, Transition,
};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animator, clip_name};

use super::anim::clip_slot;
use super::editors::grid;
use crate::anim_controllers::{self, interacting};
use crate::panels::animator::{CONTROLLER_ICON, GraphItem};
use crate::state::EditorState;
use crate::theme;

/// The Animator component: Controller, Speed, Keep State On Disable.
pub fn animator(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut a) = state.scene.world.get::<&Animator>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let controllers: Vec<String> = state.assets.controllers.iter().cloned().collect();
    let mut open = false;
    let changed = grid(ui, "animator_grid", |ui| {
        let mut ch = false;
        ui.label("Controller");
        ui.horizontal(|ui| {
            ch |= controller_slot(ui, &controllers, &mut a.controller);
            open = a.controller.is_some()
                && ui
                    .small_button("Open")
                    .on_hover_text("Open in the Animator window")
                    .clicked();
        });
        ui.end_row();
        ui.label("Speed");
        ch |= ui
            .add(egui::DragValue::new(&mut a.speed).speed(0.01))
            .changed();
        ui.end_row();
        ui.label("Keep State On Disable");
        ch |= ui.checkbox(&mut a.keep_state_on_disable, "").changed();
        ui.end_row();
        ch
    });
    if open {
        state.animator.open(a.controller.clone());
        state.push(crate::actions::Action::FocusTab(
            crate::layout::Tab::Animator,
        ));
    }
    if changed {
        let _ = state.scene.world.insert_one(e, a);
    }
    changed
}

/// Controller picker: project `.controller` files, or one dragged from the Project window.
fn controller_slot(ui: &mut Ui, controllers: &[String], value: &mut Option<String>) -> bool {
    let mut ch = false;
    let none = "None (Animator Controller)";
    let current = value.as_deref().map(clip_name);
    let slot = egui::ComboBox::from_id_salt("animator_controller")
        .selected_text(format!(
            "{CONTROLLER_ICON} {}",
            current.as_deref().unwrap_or(none)
        ))
        .width(150.0)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, none).changed();
            for c in controllers {
                ch |= ui
                    .selectable_value(value, Some(c.clone()), clip_name(c))
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::animation::is_controller(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// A `.controller` asset: the Animator window's selected state / transition, or a summary.
pub fn controller_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let ctrl = match state.anim_clips.controllers.get(rel) {
        Ok(c) => c.clone(),
        Err(e) => {
            ui.colored_label(theme::AXIS_X, e);
            return;
        }
    };
    let busy = interacting(ui.ctx());
    state.anim_clips.controllers.end_interaction(busy);
    let selected = (state.animator.controller.as_deref() == Some(rel))
        .then(|| state.animator.selected.clone())
        .flatten();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| match selected {
            Some(GraphItem::Node(Node::State(name))) => {
                if let Some(st) = ctrl.state(&name) {
                    state_editor(ui, state, rel, &ctrl, st.clone(), busy);
                }
            }
            Some(GraphItem::Transition(i)) if i < ctrl.transitions.len() => {
                transition_editor(ui, state, rel, &ctrl, i, busy);
            }
            Some(GraphItem::Node(n)) => {
                header(ui, n.label(), "");
                ui.weak(match n {
                    Node::Entry => "The state machine starts here and goes to the default state.",
                    Node::AnyState => "Transitions from Any State are checked from every state.",
                    _ => "A transition to Exit returns to the default state.",
                });
            }
            _ => summary(ui, state, rel, &ctrl),
        });
}

fn header(ui: &mut Ui, title: &str, kind: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(CONTROLLER_ICON).size(22.0));
        ui.label(RichText::new(format!("{title}{kind}")).strong());
    });
    ui.separator();
}

fn summary(ui: &mut Ui, state: &mut EditorState, rel: &str, c: &AnimatorController) {
    header(ui, &clip_name(rel), " (Animator Controller)");
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    grid(ui, "controller_summary", |ui| {
        ui.label("States");
        ui.label(c.states.len().to_string());
        ui.end_row();
        ui.label("Parameters");
        ui.label(c.parameters.len().to_string());
        ui.end_row();
        ui.label("Transitions");
        ui.label(c.transitions.len().to_string());
        ui.end_row();
        ui.label("Default State");
        ui.label(c.default_state.as_deref().unwrap_or("None"));
        ui.end_row();
        false
    });
    if ui.button("Open Animator Window").clicked() {
        state.animator.open(Some(rel.to_owned()));
        state.push(crate::actions::Action::FocusTab(
            crate::layout::Tab::Animator,
        ));
    }
}

/// A state: name, motion (clip or 1D blend tree), speed, loop and its outgoing transitions.
fn state_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    mut st: State,
    busy: bool,
) {
    let is_default = c.default_state.as_deref() == Some(st.name.as_str());
    header(ui, &st.name, if is_default { "  (Default)" } else { "" });
    let old_name = st.name.clone();
    let clips: Vec<String> = state.assets.anims.iter().cloned().collect();
    let floats: Vec<String> = c
        .parameters
        .iter()
        .filter(|p| p.kind == ParamKind::Float)
        .map(|p| p.name.clone())
        .collect();
    let mut rename = None;
    let mut changed = grid(ui, "anim_state_grid", |ui| {
        let mut ch = false;
        ui.label("Name");
        let buf = ui.id().with(("state_name", &old_name));
        let mut text: String = ui.data(|d| d.get_temp(buf)).unwrap_or(old_name.clone());
        let r = ui.text_edit_singleline(&mut text);
        if r.lost_focus() {
            if text != old_name {
                rename = Some(text.clone());
            }
            ui.data_mut(|d| d.remove::<String>(buf));
        } else if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(buf, text));
        }
        ui.end_row();
        ui.label("Motion Type");
        let mut tree = st.blend_tree.is_some();
        egui::ComboBox::from_id_salt("state_motion_type")
            .selected_text(if tree { "1D Blend Tree" } else { "Clip" })
            .show_ui(ui, |ui| {
                ch |= ui.selectable_value(&mut tree, false, "Clip").changed();
                ch |= ui
                    .selectable_value(&mut tree, true, "1D Blend Tree")
                    .changed();
            });
        if tree != st.blend_tree.is_some() {
            st.blend_tree = tree.then(|| BlendTree {
                parameter: floats.first().cloned().unwrap_or_default(),
                children: st
                    .motion
                    .iter()
                    .map(|m| BlendChild {
                        motion: Some(m.clone()),
                        threshold: 0.0,
                    })
                    .collect(),
            });
        }
        ui.end_row();
        if st.blend_tree.is_none() {
            ui.label("Motion");
            ch |= clip_slot(ui, "state_motion", &clips, &mut st.motion);
            ui.end_row();
        }
        ui.label("Speed");
        ch |= ui
            .add(egui::DragValue::new(&mut st.speed).speed(0.01))
            .changed();
        ui.end_row();
        ui.label("Loop");
        ch |= ui.checkbox(&mut st.looping, "").changed();
        ui.end_row();
        ch
    });
    if let Some(tree) = st.blend_tree.as_mut() {
        changed |= blend_tree_editor(ui, tree, &floats, &clips);
    }
    if changed {
        let name = old_name.clone();
        anim_controllers::edit(state, rel, "Animator State", busy, |c| {
            if let Some(s) = c.state_mut(&name) {
                *s = st.clone();
                s.name = name.clone();
            }
        });
    }
    if let Some(new) = rename {
        let mut final_name = None;
        anim_controllers::edit(state, rel, "Rename State", false, |c| {
            final_name = c.rename_state(&old_name, &new);
        });
        if let Some(n) = final_name {
            state.animator.selected = Some(GraphItem::Node(Node::State(n)));
        }
    }
    ui.separator();
    ui.label(RichText::new("Transitions").strong());
    let here = Node::state(&old_name);
    let outgoing: Vec<(usize, String)> = c
        .transitions
        .iter()
        .enumerate()
        .filter(|(_, t)| t.from == here)
        .map(|(i, t)| (i, t.label()))
        .collect();
    if outgoing.is_empty() {
        ui.weak("None. Right-click the state in the Animator window ▸ Make Transition.");
    }
    for (i, label) in outgoing {
        if ui.selectable_label(false, label).clicked() {
            state.animator.selected = Some(GraphItem::Transition(i));
        }
    }
}

/// Parameter and children (clip + threshold) of a 1D blend tree.
fn blend_tree_editor(
    ui: &mut Ui,
    tree: &mut BlendTree,
    floats: &[String],
    clips: &[String],
) -> bool {
    let mut ch = false;
    ui.separator();
    ui.label(RichText::new("Blend Tree").strong());
    grid(ui, "blend_tree_grid", |ui| {
        ui.label("Parameter");
        egui::ComboBox::from_id_salt("blend_param")
            .selected_text(if tree.parameter.is_empty() {
                "None"
            } else {
                tree.parameter.as_str()
            })
            .show_ui(ui, |ui| {
                for f in floats {
                    ch |= ui
                        .selectable_value(&mut tree.parameter, f.clone(), f)
                        .changed();
                }
            });
        ui.end_row();
        false
    });
    if floats.is_empty() {
        ui.weak("Add a Float parameter to drive the blend.");
    }
    let mut remove = None;
    for (i, child) in tree.children.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ch |= clip_slot(ui, ("blend_child", i), clips, &mut child.motion);
            ui.label("Threshold");
            ch |= ui
                .add(egui::DragValue::new(&mut child.threshold).speed(0.01))
                .changed();
            if ui
                .small_button("−")
                .on_hover_text("Remove motion")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        tree.children.remove(i);
        ch = true;
    }
    if ui.small_button("+ Add Motion Field").clicked() {
        let threshold = tree.children.last().map_or(0.0, |c| c.threshold + 1.0);
        tree.children.push(BlendChild {
            motion: None,
            threshold,
        });
        ch = true;
    }
    ch
}

/// A transition: exit time, duration, offset and conditions.
fn transition_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    i: usize,
    busy: bool,
) {
    let mut t: Transition = c.transitions[i].clone();
    header(ui, &t.label(), "  (Transition)");
    let mut changed = grid(ui, "anim_transition_grid", |ui| {
        let mut ch = false;
        ui.label("Has Exit Time");
        ch |= ui.checkbox(&mut t.has_exit_time, "").changed();
        ui.end_row();
        ui.add_enabled_ui(t.has_exit_time, |ui| ui.label("Exit Time"));
        ch |= ui
            .add_enabled(
                t.has_exit_time,
                egui::DragValue::new(&mut t.exit_time)
                    .speed(0.01)
                    .range(0.0..=100.0),
            )
            .changed();
        ui.end_row();
        ui.label("Fixed Duration");
        ch |= ui.checkbox(&mut t.fixed_duration, "").changed();
        ui.end_row();
        ui.label(if t.fixed_duration {
            "Transition Duration (s)"
        } else {
            "Transition Duration (%)"
        });
        ch |= ui
            .add(
                egui::DragValue::new(&mut t.duration)
                    .speed(0.01)
                    .range(0.0..=100.0),
            )
            .changed();
        ui.end_row();
        ui.label("Transition Offset");
        ch |= ui
            .add(
                egui::DragValue::new(&mut t.offset)
                    .speed(0.01)
                    .range(0.0..=1.0),
            )
            .changed();
        ui.end_row();
        ch
    });
    ui.separator();
    ui.label(RichText::new("Conditions").strong());
    changed |= conditions_editor(ui, c, &mut t.conditions);
    if !t.has_exit_time && t.conditions.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(230, 190, 60),
            "A transition needs at least one condition or an Exit Time.",
        );
    }
    if changed {
        anim_controllers::edit(state, rel, "Animator Transition", busy, |c| {
            if let Some(slot) = c.transitions.get_mut(i) {
                *slot = t;
            }
        });
    }
}

/// Condition rows: parameter, mode, threshold; "+" adds one, "−" removes it.
fn conditions_editor(ui: &mut Ui, c: &AnimatorController, list: &mut Vec<Condition>) -> bool {
    let mut ch = false;
    if list.is_empty() {
        ui.weak("List is Empty");
    }
    let mut remove = None;
    for (i, cond) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("cond_param", i))
                .width(110.0)
                .selected_text(cond.parameter.as_str())
                .show_ui(ui, |ui| {
                    for p in &c.parameters {
                        ch |= ui
                            .selectable_value(&mut cond.parameter, p.name.clone(), &p.name)
                            .changed();
                    }
                });
            let kind = c
                .parameter(&cond.parameter)
                .map_or(ParamKind::Float, |p| p.kind);
            if !kind.modes().contains(&cond.mode) {
                cond.mode = kind.modes()[0];
                ch = true;
            }
            if kind != ParamKind::Trigger {
                egui::ComboBox::from_id_salt(("cond_mode", i))
                    .width(80.0)
                    .selected_text(cond.mode.label())
                    .show_ui(ui, |ui| {
                        for m in kind.modes() {
                            ch |= ui.selectable_value(&mut cond.mode, *m, m.label()).changed();
                        }
                    });
            }
            if cond.mode.uses_threshold() {
                let speed = if kind == ParamKind::Int { 1.0 } else { 0.01 };
                ch |= ui
                    .add(egui::DragValue::new(&mut cond.threshold).speed(speed))
                    .changed();
            }
            if ui
                .small_button("−")
                .on_hover_text("Remove condition")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
        ch = true;
    }
    match c.parameters.first() {
        Some(p) => {
            if ui
                .small_button("+")
                .on_hover_text("Add condition")
                .clicked()
            {
                list.push(Condition {
                    parameter: p.name.clone(),
                    mode: p.kind.modes()[0],
                    threshold: 0.0,
                });
                ch = true;
            }
        }
        None => {
            ui.weak("Add parameters in the Animator window to use conditions.");
        }
    }
    ch
}
