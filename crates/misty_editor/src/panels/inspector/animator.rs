//! Inspector editors for the Animator: the component (controller slot, speed, root motion,
//! humanoid avatar), and
//! a `.controller` asset showing what is selected in the Animator window on its shown layer:
//! a state (clip or blend tree with its diagram, speed, multiplier, tag), a sub-state machine,
//! a transition (exit time, duration, offset, interruption, conditions) or a summary.

use egui::{RichText, Ui};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::controller::{
    BlendChild, BlendTree, BlendType, Condition, InterruptionSource, Node, ParamKind, State,
    StateMachine, Transition, machine_within,
};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animator, clip_name};

use super::anim::clip_slot;
use super::editors::grid;
use crate::anim_controllers::{self, interacting};
use crate::panels::animator::blend_view;
use crate::panels::animator::{CONTROLLER_ICON, GraphItem};
use crate::state::EditorState;
use crate::theme;

/// The Animator component: Controller, Speed, Apply Root Motion, Root Motion Bone, Keep State On
/// Disable.
pub fn animator(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut a) = state.scene.world.get::<&Animator>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let controllers: Vec<String> = state.assets.controllers.iter().cloned().collect();
    let bones = misty_engine::animation::AvatarMask::skeleton(&state.scene, e);
    let avatars: Vec<String> = state.assets.avatars.iter().cloned().collect();
    let mut open = false;
    let mut create_avatar = false;
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
        ui.label("Apply Root Motion");
        ch |= ui
            .checkbox(&mut a.apply_root_motion, "")
            .on_hover_text(
                "Move the object by its root's animation (OnAnimatorMove in a script takes over)",
            )
            .changed();
        ui.end_row();
        ui.add_enabled_ui(a.apply_root_motion, |ui| ui.label("Root Motion Bone"));
        let current = if a.root_motion_bone.is_empty() {
            "(This Object)".to_owned()
        } else {
            a.root_motion_bone.clone()
        };
        ui.add_enabled_ui(a.apply_root_motion, |ui| {
            egui::ComboBox::from_id_salt("root_motion_bone")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for b in &bones {
                        let label = if b.is_empty() { "(This Object)" } else { b };
                        ch |= ui
                            .selectable_value(&mut a.root_motion_bone, b.clone(), label)
                            .changed();
                    }
                });
        });
        ui.end_row();
        ui.label("Avatar");
        ui.horizontal(|ui| {
            let none = "None (Avatar)";
            egui::ComboBox::from_id_salt("animator_avatar")
                .selected_text(match &a.avatar {
                    Some(p) => format!("{} {}", crate::panels::animator::AVATAR_ICON, clip_name(p)),
                    None => none.to_owned(),
                })
                .show_ui(ui, |ui| {
                    ch |= ui.selectable_value(&mut a.avatar, None, none).changed();
                    for p in &avatars {
                        ch |= ui
                            .selectable_value(&mut a.avatar, Some(p.clone()), clip_name(p))
                            .changed();
                    }
                });
            create_avatar = ui
                .small_button("Create")
                .on_hover_text("Map this rig's bones to a humanoid Avatar asset")
                .clicked();
        });
        ui.end_row();
        ui.label("Keep State On Disable");
        ch |= ui.checkbox(&mut a.keep_state_on_disable, "").changed();
        ui.end_row();
        ch
    });
    if create_avatar {
        let avatar = misty_engine::animation::Avatar::auto_map(&state.scene, e);
        let dir = state.assets.absolute("Assets");
        let name = state.scene.name(e);
        let path = crate::asset_ops::unique_path(&dir, &name, "avatar");
        match avatar.save(&path) {
            Ok(()) => {
                let rel = state.assets.relative(&path);
                let missing = avatar.missing();
                if !missing.is_empty() {
                    state.console.warn(format!(
                        "Avatar {rel}: no bone found for {}.",
                        missing
                            .iter()
                            .map(|b| b.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                state.assets.avatars.insert(rel.clone());
                a.avatar = Some(rel);
                let _ = state.scene.world.insert_one(e, a.clone());
                return true;
            }
            Err(err) => state
                .console
                .warn(format!("Could not create the Avatar: {err}")),
        }
    }
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

/// Change the machine of `layer` with undo.
fn edit_layer(
    state: &mut EditorState,
    rel: &str,
    layer: usize,
    label: &str,
    busy: bool,
    f: impl FnOnce(&mut StateMachine),
) {
    anim_controllers::edit(state, rel, label, busy, |c| {
        if let Some(m) = c.machine_mut(layer) {
            f(m);
        }
    });
}

/// A `.controller` asset: the Animator window's selection on its shown layer, or a summary.
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
    let shown = state.animator.controller.as_deref() == Some(rel);
    let selected = shown.then(|| state.animator.selected.clone()).flatten();
    let layer = if shown {
        state.animator.layer.min(ctrl.layer_count() - 1)
    } else {
        0
    };
    let ml = ctrl.machine_layer(layer);
    let synced = ml != layer;
    let Some(m) = ctrl.machine(ml).cloned() else {
        return;
    };
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| match selected {
            Some(GraphItem::Node(Node::State(name))) => {
                if let Some(st) = m.state(&name) {
                    if synced {
                        synced_state_editor(ui, state, rel, &ctrl, layer, st, busy);
                    } else {
                        state_editor(ui, state, rel, &ctrl, layer, st.clone(), busy);
                    }
                }
            }
            Some(GraphItem::Node(Node::Machine(path))) if m.sub(&path).is_some() => {
                sub_machine_editor(ui, state, rel, &m, layer, &path, synced);
            }
            Some(GraphItem::Transition(i)) if i < m.transitions.len() => {
                if synced {
                    header(ui, &m.transitions[i].label(), "  (Transition)");
                    ui.weak("A synced layer uses its source layer's transitions.");
                } else {
                    transition_editor(ui, state, rel, &ctrl, &m, layer, i, busy);
                }
            }
            Some(GraphItem::Node(n)) => {
                header(ui, n.label(), "");
                ui.weak(match n {
                    Node::Entry => "The state machine starts here and goes to the default state (or the first Entry transition whose conditions hold).",
                    Node::AnyState => "Transitions from Any State are checked from every state of the layer, and may interrupt any transition.",
                    Node::Up => "The parent state machine. End Make Transition here to pick a state outside this sub-state machine.",
                    _ => "A transition to Exit leaves this state machine (at the top: back to the default state).",
                });
            }
            _ => summary(ui, state, rel, &ctrl),
        });
}

fn header(ui: &mut Ui, title: &str, kind: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(CONTROLLER_ICON).size(22.0));
        ui.label(crate::theme::bold(ui.ctx(), format!("{title}{kind}")));
    });
    ui.separator();
}

fn summary(ui: &mut Ui, state: &mut EditorState, rel: &str, c: &AnimatorController) {
    header(ui, &clip_name(rel), " (Animator Controller)");
    crate::asset_guids::path_label(ui, rel);
    let states: usize = (0..c.layer_count())
        .filter_map(|l| c.machine(l))
        .map(|m| m.states.len())
        .sum();
    grid(ui, "controller_summary", |ui| {
        ui.label("Layers");
        ui.label(c.layer_count().to_string());
        ui.end_row();
        ui.label("States");
        ui.label(states.to_string());
        ui.end_row();
        ui.label("Parameters");
        ui.label(c.parameters.len().to_string());
        ui.end_row();
        ui.label("Transitions");
        ui.label(c.base.transitions.len().to_string());
        ui.end_row();
        ui.label("Default State");
        ui.label(c.base.default_state.as_deref().unwrap_or("None"));
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

fn float_params(c: &AnimatorController) -> Vec<String> {
    c.parameters
        .iter()
        .filter(|p| p.kind == ParamKind::Float)
        .map(|p| p.name.clone())
        .collect()
}

/// A float parameter picker ("None" allowed when `none` is set).
fn param_combo(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    floats: &[String],
    none: bool,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(if value.is_empty() {
            "None"
        } else {
            value.as_str()
        })
        .show_ui(ui, |ui| {
            if none {
                ch |= ui.selectable_value(value, String::new(), "None").changed();
            }
            for f in floats {
                ch |= ui.selectable_value(value, f.clone(), f).changed();
            }
        });
    ch
}

/// A state: name, motion (clip or blend tree), speed and multiplier, loop, tag and its outgoing
/// transitions.
fn state_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    layer: usize,
    mut st: State,
    busy: bool,
) {
    let m = c.machine(layer).expect("layer exists");
    let is_default = m.default_of(&st.machine) == Some(st.name.as_str());
    header(ui, &st.name, if is_default { "  (Default)" } else { "" });
    let old_name = st.name.clone();
    let clips: Vec<String> = state.assets.anims.iter().cloned().collect();
    let floats = float_params(c);
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
        ui.label("Tag");
        ch |= ui.text_edit_singleline(&mut st.tag).changed();
        ui.end_row();
        ui.label("Motion Type");
        let mut tree = st.blend_tree.is_some();
        egui::ComboBox::from_id_salt("state_motion_type")
            .selected_text(if tree { "Blend Tree" } else { "Clip" })
            .show_ui(ui, |ui| {
                ch |= ui.selectable_value(&mut tree, false, "Clip").changed();
                ch |= ui.selectable_value(&mut tree, true, "Blend Tree").changed();
            });
        if tree != st.blend_tree.is_some() {
            st.blend_tree = tree.then(|| BlendTree {
                parameter: floats.first().cloned().unwrap_or_default(),
                children: st.motion.iter().map(|m| BlendChild::clip(m)).collect(),
                ..Default::default()
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
        ui.label("Multiplier");
        ch |= param_combo(
            ui,
            "state_speed_param",
            &mut st.speed_parameter,
            &floats,
            true,
        );
        ui.end_row();
        ui.label("Loop");
        ch |= ui.checkbox(&mut st.looping, "").changed();
        ui.end_row();
        ch
    });
    if let Some(tree) = st.blend_tree.as_mut() {
        changed |= blend_tree_editor(ui, state, rel, c, tree, &floats, &clips, busy);
        if ui
            .small_button("Open in Animator Window")
            .on_hover_text("Show the blend tree view")
            .clicked()
        {
            state.animator.blend_view = Some(old_name.clone());
        }
    }
    if changed {
        let name = old_name.clone();
        edit_layer(state, rel, layer, "Animator State", busy, |m| {
            if let Some(s) = m.state_mut(&name) {
                *s = st.clone();
                s.name = name.clone();
            }
        });
    }
    if let Some(new) = rename {
        let mut final_name = None;
        anim_controllers::edit(state, rel, "Rename State", false, |c| {
            final_name = c.rename_state(layer, &old_name, &new);
        });
        if let Some(n) = final_name {
            state.animator.selected = Some(GraphItem::Node(Node::State(n)));
        }
    }
    transitions_list(ui, state, m, &Node::state(&old_name));
}

/// Transitions leaving `from`, clickable.
fn transitions_list(ui: &mut Ui, state: &mut EditorState, m: &StateMachine, from: &Node) {
    super::card::sub_header(ui, "Transitions");
    let outgoing: Vec<(usize, String)> = m
        .transitions
        .iter()
        .enumerate()
        .filter(|(_, t)| t.from == *from)
        .map(|(i, t)| (i, t.label()))
        .collect();
    if outgoing.is_empty() {
        ui.weak("None. Right-click the node in the Animator window ▸ Make Transition.");
    }
    for (i, label) in outgoing {
        if ui.selectable_label(false, label).clicked() {
            state.animator.selected = Some(GraphItem::Transition(i));
        }
    }
}

/// A state of a synced layer: only the clip this layer plays for it.
fn synced_state_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    layer: usize,
    st: &State,
    busy: bool,
) {
    header(ui, &st.name, "  (Synced)");
    let source = c.layer_name(c.machine_layer(layer)).to_owned();
    ui.weak(format!(
        "This layer reuses the states of '{source}'; pick the clip it plays for this one."
    ));
    let clips: Vec<String> = state.assets.anims.iter().cloned().collect();
    let mut motion = c
        .layer(layer)
        .and_then(|l| l.sync_motions.get(&st.name).cloned());
    let changed = grid(ui, "synced_state_grid", |ui| {
        ui.label("Motion");
        let ch = clip_slot(ui, "synced_motion", &clips, &mut motion);
        ui.end_row();
        ch
    });
    if changed {
        let name = st.name.clone();
        anim_controllers::edit(state, rel, "Synced Motion", busy, |c| {
            if let Some(l) = c.layer_mut(layer) {
                match motion {
                    Some(m) => {
                        l.sync_motions.insert(name, m);
                    }
                    None => {
                        l.sync_motions.remove(&name);
                    }
                }
            }
        });
    }
}

/// Blend type, parameters, the diagram and the motions of a blend tree.
#[allow(clippy::too_many_arguments)]
fn blend_tree_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    tree: &mut BlendTree,
    floats: &[String],
    clips: &[String],
    busy: bool,
) -> bool {
    let mut ch = false;
    super::card::sub_header(ui, "Blend Tree");
    grid(ui, "blend_tree_grid", |ui| {
        ui.label("Blend Type");
        egui::ComboBox::from_id_salt("blend_type")
            .selected_text(tree.blend_type.label())
            .show_ui(ui, |ui| {
                for t in BlendType::ALL {
                    ch |= ui
                        .selectable_value(&mut tree.blend_type, t, t.label())
                        .changed();
                }
            });
        ui.end_row();
        match tree.blend_type {
            BlendType::Direct => {
                ui.label("Normalized Blend Values");
                ch |= ui.checkbox(&mut tree.normalize, "").changed();
                ui.end_row();
            }
            t => {
                ui.label(if t.is_2d() {
                    "Parameter X"
                } else {
                    "Parameter"
                });
                ch |= param_combo(ui, "blend_param", &mut tree.parameter, floats, false);
                ui.end_row();
                if t.is_2d() {
                    ui.label("Parameter Y");
                    ch |= param_combo(ui, "blend_param_y", &mut tree.parameter_y, floats, false);
                    ui.end_row();
                }
            }
        }
        false
    });
    if floats.is_empty() {
        ui.weak("Add a Float parameter to drive the blend.");
    }
    // The diagram, with the parameter values as the red marker (drag it to change them).
    let live_status = state
        .runtime
        .as_ref()
        .zip(state.animator.target)
        .and_then(|(rt, e)| rt.animator_status(&state.scene, e));
    let live = live_status.as_ref();
    let param = |n: &str| blend_view::param_value(c, live, n);
    let value = [param(&tree.parameter), param(&tree.parameter_y)];
    let w = ui.available_width().min(320.0);
    let h = match tree.blend_type {
        BlendType::Simple1D => 90.0,
        BlendType::Direct => 22.0 * tree.children.len().max(1) as f32 + 8.0,
        _ => w,
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let selected = state.animator.blend_selected;
    if let Some(v) = blend_view::diagram(ui, rect, tree, value, &param, selected) {
        let names = [tree.parameter.clone(), tree.parameter_y.clone()];
        for (i, name) in names.iter().enumerate() {
            if (v[i] - value[i]).abs() > 1e-6 && floats.contains(name) {
                blend_view::set_param(state, rel, name, v[i], live.is_some(), true);
            }
        }
    }
    let weights = tree.weights_at(value[0], value[1], &param);
    ui.add_space(4.0);
    let mut remove = None;
    for (i, child) in tree.children.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let pct = format!("{:>3.0}%", weights.get(i).copied().unwrap_or(0.0) * 100.0);
            if ui
                .selectable_label(selected == Some(i), pct)
                .on_hover_text("Weight now (click to show its influence)")
                .clicked()
            {
                state.animator.blend_selected = Some(i);
            }
            ch |= clip_slot(ui, ("blend_child", i), clips, &mut child.motion);
            match tree.blend_type {
                BlendType::Simple1D => {
                    ui.label("Threshold");
                    ch |= ui
                        .add(egui::DragValue::new(&mut child.threshold).speed(0.01))
                        .changed();
                }
                BlendType::Direct => {
                    ch |= param_combo(
                        ui,
                        ("blend_direct", i),
                        &mut child.direct_parameter,
                        floats,
                        false,
                    );
                }
                _ => {
                    ui.label("Pos X");
                    ch |= ui
                        .add(egui::DragValue::new(&mut child.position[0]).speed(0.01))
                        .changed();
                    ui.label("Y");
                    ch |= ui
                        .add(egui::DragValue::new(&mut child.position[1]).speed(0.01))
                        .changed();
                }
            }
            ui.label("Speed");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut child.time_scale)
                        .speed(0.01)
                        .range(0.01..=100.0),
                )
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
        state.animator.blend_selected = None;
        ch = true;
    }
    if ui.small_button("+ Add Motion Field").clicked() {
        let last = tree.children.last();
        tree.children.push(BlendChild {
            threshold: last.map_or(0.0, |c| c.threshold + 1.0),
            position: last.map_or([0.0, 0.0], |c| [c.position[0] + 1.0, c.position[1]]),
            ..Default::default()
        });
        ch = true;
    }
    let _ = busy;
    ch
}

/// A sub-state machine: name, default state, and transitions leaving it (checked when it is
/// left through its Exit).
fn sub_machine_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    m: &StateMachine,
    layer: usize,
    path: &str,
    synced: bool,
) {
    header(
        ui,
        misty_engine::animation::controller::machine_name(path),
        "  (Sub-State Machine)",
    );
    let sub = m.sub(path).expect("checked").clone();
    let inside: Vec<String> = m
        .states
        .iter()
        .filter(|s| machine_within(&s.machine, path))
        .map(|s| s.name.clone())
        .collect();
    let mut rename = None;
    let mut default = sub.default_state.clone();
    let changed = ui
        .add_enabled_ui(!synced, |ui| {
            grid(ui, "sub_machine_grid", |ui| {
                ui.label("Name");
                let buf = ui.id().with(("machine_name", path));
                let old = misty_engine::animation::controller::machine_name(path).to_owned();
                let mut text: String = ui.data(|d| d.get_temp(buf)).unwrap_or(old.clone());
                let r = ui.text_edit_singleline(&mut text);
                if r.lost_focus() {
                    if text != old {
                        rename = Some(text.clone());
                    }
                    ui.data_mut(|d| d.remove::<String>(buf));
                } else if r.has_focus() {
                    ui.data_mut(|d| d.insert_temp(buf, text));
                }
                ui.end_row();
                ui.label("Default State");
                let mut ch = false;
                egui::ComboBox::from_id_salt("sub_machine_default")
                    .selected_text(default.as_deref().unwrap_or("None"))
                    .show_ui(ui, |ui| {
                        for n in &inside {
                            ch |= ui
                                .selectable_value(&mut default, Some(n.clone()), n)
                                .changed();
                        }
                    });
                ui.end_row();
                ch
            })
        })
        .inner;
    if changed {
        let (p, d) = (path.to_owned(), default.clone());
        edit_layer(state, rel, layer, "Sub-State Machine Default", false, |m| {
            m.set_default(&p, d)
        });
    }
    if let Some(new) = rename {
        let mut to = None;
        let p = path.to_owned();
        edit_layer(state, rel, layer, "Rename Sub-State Machine", false, |m| {
            to = m.rename_sub_machine(&p, &new);
        });
        if let Some(to) = to {
            state.animator.selected = Some(GraphItem::Node(Node::Machine(to)));
        }
    }
    ui.add_space(4.0);
    if ui.button("Open Sub-State Machine").clicked() {
        state.animator.enter_machine(path.to_owned());
    }
    transitions_list(ui, state, m, &Node::Machine(path.to_owned()));
}

/// A transition: exit time, duration, offset, interruption settings and conditions.
#[allow(clippy::too_many_arguments)]
fn transition_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    c: &AnimatorController,
    m: &StateMachine,
    layer: usize,
    i: usize,
    busy: bool,
) {
    let mut t: Transition = m.transitions[i].clone();
    header(ui, &t.label(), "  (Transition)");
    let mut changed = grid(ui, "anim_transition_grid", |ui| {
        let mut ch = false;
        ui.label("Mute");
        ch |= ui.checkbox(&mut t.mute, "").changed();
        ui.end_row();
        ui.label("Solo");
        ch |= ui.checkbox(&mut t.solo, "").changed();
        ui.end_row();
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
        ui.label("Interruption Source");
        egui::ComboBox::from_id_salt("interruption_source")
            .selected_text(t.interruption_source.label())
            .show_ui(ui, |ui| {
                for src in InterruptionSource::ALL {
                    ch |= ui
                        .selectable_value(&mut t.interruption_source, src, src.label())
                        .changed();
                }
            });
        ui.end_row();
        let uses_current = t.interruption_source.order().contains(&false);
        ui.add_enabled_ui(uses_current, |ui| ui.label("Ordered Interruption"));
        ch |= ui
            .add_enabled(
                uses_current,
                egui::Checkbox::new(&mut t.ordered_interruption, ""),
            )
            .on_hover_text("Only transitions listed before this one may interrupt it")
            .changed();
        ui.end_row();
        if t.from == Node::AnyState {
            ui.label("Can Transition To Self");
            ch |= ui.checkbox(&mut t.can_transition_to_self, "").changed();
            ui.end_row();
        }
        ch
    });
    super::card::sub_header(ui, "Conditions");
    changed |= conditions_editor(ui, c, &mut t.conditions);
    if !t.has_exit_time && t.conditions.is_empty() && !matches!(t.from, Node::Machine(_)) {
        ui.colored_label(
            egui::Color32::from_rgb(230, 190, 60),
            "A transition needs at least one condition or an Exit Time.",
        );
    }
    if changed {
        edit_layer(state, rel, layer, "Animator Transition", busy, |m| {
            if let Some(slot) = m.transitions.get_mut(i) {
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
