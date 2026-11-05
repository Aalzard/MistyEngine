//! Inspector editors for Animation Rigging: Rig Builder (rig layers), Rig (weight) and the
//! constraints, laid out as Weight, then "Constrained Objects", "Source Objects" and
//! "Settings" sections.

use egui::{DragValue, Slider, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::rigging::{
    Axis, ChainIk, ChainSolver, DampedTransform, MultiAim, OverrideSpace, OverrideTransform, Rig,
    RigBuilder, RigLayer, TwoBoneIk, WeightedSource, WorldUpType,
};
use misty_engine::scene::SceneComponent;

use super::card::{grid, sub_header};
use super::editors::combo_k;
use super::mixed;
use crate::state::EditorState;
use crate::widgets::vec3_row_mixed;

pub const NAMES: [&str; 7] = [
    RigBuilder::NAME,
    Rig::NAME,
    TwoBoneIk::NAME,
    MultiAim::NAME,
    DampedTransform::NAME,
    ChainIk::NAME,
    OverrideTransform::NAME,
];

type Objects = Vec<(Entity, String)>;

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rig Builder" => "🦴",
        "Rig" => "⛓",
        "Two Bone IK Constraint" | "Chain IK Constraint" => "⤳",
        "Multi-Aim Constraint" => "◎",
        "Damped Transform" => "〰",
        "Override Transform" => "⇲",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    (name == RigBuilder::NAME)
        .then(|| {
            state
                .scene
                .world
                .get::<&RigBuilder>(e)
                .ok()
                .map(|b| b.enabled)
        })
        .flatten()
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if name == RigBuilder::NAME
        && let Ok(mut b) = state.scene.world.get::<&mut RigBuilder>(e)
    {
        b.enabled = on;
    }
}

/// Every object, indented by depth, for the object pickers.
fn objects(state: &EditorState) -> Objects {
    state
        .scene
        .walk()
        .into_iter()
        .map(|(o, depth)| (o, format!("{}{}", "  ".repeat(depth), state.scene.name(o))))
        .collect()
}

/// Copy of component `T`, edited by `f`, written back when it reports a change.
fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&mut T) -> bool,
) -> bool {
    let Ok(mut c) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let ch = f(&mut c);
    if ch {
        let _ = state.scene.world.insert_one(e, c);
    }
    ch
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    let objs = objects(state);
    match name {
        "Rig Builder" => {
            let rigs: Objects = objs
                .iter()
                .filter(|(o, _)| state.scene.world.get::<&Rig>(*o).is_ok())
                .map(|(o, _)| (*o, state.scene.name(*o)))
                .collect();
            edit_copy::<RigBuilder>(state, e, |b| rig_builder(ui, b, &rigs))
        }
        "Rig" => {
            let n = misty_engine::rigging::rig_constraints(&state.scene, e).len();
            let ch = edit_copy::<Rig>(state, e, |r| {
                grid(ui, "rig_grid", |ui| weight_row(ui, &mut r.weight))
            });
            ui.weak(format!(
                "{n} constraint(s) on this object and its children. Add the Rig to a Rig \
                 Builder's layers (or place it below one)."
            ));
            ch
        }
        "Two Bone IK Constraint" => {
            let auto = std::cell::Cell::new(false);
            let ch = edit_copy::<TwoBoneIk>(state, e, |c| two_bone(ui, c, &objs, &auto));
            if auto.get() {
                auto_setup_two_bone(state, e);
            }
            ch
        }
        "Multi-Aim Constraint" => edit_copy::<MultiAim>(state, e, |c| multi_aim(ui, c, &objs)),
        "Damped Transform" => edit_copy::<DampedTransform>(state, e, |c| damped(ui, c, &objs)),
        "Chain IK Constraint" => edit_copy::<ChainIk>(state, e, |c| chain_ik(ui, c, &objs)),
        "Override Transform" => {
            edit_copy::<OverrideTransform>(state, e, |c| override_transform(ui, c, &objs))
        }
        _ => false,
    }
}

/// Scene object picker ("None (Transform)" clears it).
fn object_row(ui: &mut Ui, label: &str, key: &str, v: &mut Option<Entity>, objs: &Objects) -> bool {
    ui.label(label);
    let name = v
        .and_then(|t| objs.iter().find(|(e, _)| *e == t))
        .map_or("None (Transform)".to_owned(), |(_, n)| {
            n.trim_start().to_owned()
        });
    let mut ch = false;
    egui::ComboBox::from_id_salt(key)
        .selected_text(mixed::text(key, name))
        .height(320.0)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, "None (Transform)").changed();
            for (e, n) in objs {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ui.end_row();
    ch
}

fn slider_row(ui: &mut Ui, label: &str, key: &str, v: &mut f32, tip: &str) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = ui
        .add(mixed::slider(key, Slider::new(v, 0.0..=1.0)))
        .changed();
    ui.end_row();
    ch
}

fn weight_row(ui: &mut Ui, v: &mut f32) -> bool {
    slider_row(
        ui,
        "Weight",
        "weight",
        v,
        "How much of the result is applied (0..1).",
    )
}

fn check_row(ui: &mut Ui, label: &str, key: &str, v: &mut bool, tip: &str) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = mixed::checkbox(ui, key, v, "").changed();
    ui.end_row();
    ch
}

fn axis_row(ui: &mut Ui, label: &str, key: &str, v: &mut Axis) -> bool {
    ui.label(label);
    let ch = combo_k(ui, key, key, v, &Axis::ALL, Axis::label);
    ui.end_row();
    ch
}

fn rig_builder(ui: &mut Ui, b: &mut RigBuilder, rigs: &Objects) -> bool {
    let mut ch = false;
    sub_header(ui, "Rig Layers");
    if b.layers.is_empty() {
        ui.weak("No layers: every Rig below this object is evaluated, top to bottom.");
    }
    let mut remove = None;
    grid(ui, "rig_layers", |ui| {
        for (i, layer) in b.layers.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ch |= ui
                    .checkbox(&mut layer.active, "")
                    .on_hover_text("Evaluate this layer")
                    .changed();
                ui.label(format!("Layer {i}"));
            });
            ui.horizontal(|ui| {
                let name = layer
                    .rig
                    .and_then(|r| rigs.iter().find(|(e, _)| *e == r))
                    .map_or("None (Rig)".to_owned(), |(_, n)| n.clone());
                egui::ComboBox::from_id_salt(("rig_layer", i))
                    .selected_text(name)
                    .width((ui.available_width() - 28.0).max(60.0))
                    .show_ui(ui, |ui| {
                        ch |= ui
                            .selectable_value(&mut layer.rig, None, "None (Rig)")
                            .changed();
                        for (e, n) in rigs {
                            ch |= ui.selectable_value(&mut layer.rig, Some(*e), n).changed();
                        }
                    });
                if ui.small_button("−").on_hover_text("Remove layer").clicked() {
                    remove = Some(i);
                }
            });
            ui.end_row();
        }
        false
    });
    if let Some(i) = remove {
        b.layers.remove(i);
        ch = true;
    }
    if ui.button("+ Add Layer").clicked() {
        let unused = rigs
            .iter()
            .map(|(e, _)| *e)
            .find(|r| !b.layers.iter().any(|l| l.rig == Some(*r)));
        b.layers.push(RigLayer {
            rig: unused,
            active: true,
        });
        ch = true;
    }
    ch
}

fn two_bone(ui: &mut Ui, c: &mut TwoBoneIk, objs: &Objects, auto: &std::cell::Cell<bool>) -> bool {
    let mut ch = grid(ui, "tbik_w", |ui| weight_row(ui, &mut c.weight));
    sub_header(ui, "Constrained Objects");
    ch |= grid(ui, "tbik_bones", |ui| {
        let mut ch = object_row(ui, "Root", "root", &mut c.root, objs);
        ch |= object_row(ui, "Mid", "mid", &mut c.mid, objs);
        ch |= object_row(ui, "Tip", "tip", &mut c.tip, objs);
        ch
    });
    if ui
        .add_enabled(c.tip.is_some(), egui::Button::new("Auto Setup from Tip"))
        .on_hover_text(
            "Mid and Root become the Tip's parent and grandparent; missing Target and Hint \
             objects are created under this constraint.",
        )
        .clicked()
    {
        auto.set(true);
    }
    sub_header(ui, "Source Objects");
    ch |= grid(ui, "tbik_src", |ui| {
        let mut ch = object_row(ui, "Target", "target", &mut c.target, objs);
        ch |= object_row(ui, "Hint", "hint", &mut c.hint, objs);
        ch
    });
    sub_header(ui, "Settings");
    ch |= grid(ui, "tbik_settings", |ui| {
        ui.label("Maintain Target Offset");
        let mut ch = false;
        ui.horizontal(|ui| {
            ch |= mixed::checkbox(
                ui,
                "maintain_target_position_offset",
                &mut c.maintain_target_position_offset,
                "Position",
            )
            .changed();
            ch |= mixed::checkbox(
                ui,
                "maintain_target_rotation_offset",
                &mut c.maintain_target_rotation_offset,
                "Rotation",
            )
            .changed();
        });
        ui.end_row();
        ch |= slider_row(
            ui,
            "Target Position Weight",
            "target_position_weight",
            &mut c.target_position_weight,
            "How far the tip moves towards the target.",
        );
        ch |= slider_row(
            ui,
            "Target Rotation Weight",
            "target_rotation_weight",
            &mut c.target_rotation_weight,
            "How much the tip takes the target's rotation.",
        );
        ch |= slider_row(
            ui,
            "Hint Weight",
            "hint_weight",
            &mut c.hint_weight,
            "How much the middle joint turns towards the hint.",
        );
        ch
    });
    ch
}

/// Two Bone IK ▸ Auto Setup from Tip.
fn auto_setup_two_bone(state: &mut EditorState, e: Entity) {
    let Some(tip) = state
        .scene
        .world
        .get::<&TwoBoneIk>(e)
        .ok()
        .and_then(|c| c.tip)
    else {
        return;
    };
    let Some(mid) = state.scene.parent(tip) else {
        state
            .console
            .warn("Auto Setup: the Tip needs a parent (Mid) and a grandparent (Root).");
        return;
    };
    let Some(root) = state.scene.parent(mid) else {
        state
            .console
            .warn("Auto Setup: the Tip needs a parent (Mid) and a grandparent (Root).");
        return;
    };
    state.record("Auto Setup Two Bone IK");
    let tip_name = state.scene.name(tip);
    let mut c: TwoBoneIk = (*state.scene.world.get::<&TwoBoneIk>(e).unwrap()).clone();
    c.root = Some(root);
    c.mid = Some(mid);
    if c.target.is_none() {
        let t = state.scene.spawn(&format!("{tip_name}_target"), Some(e));
        let (p, r) = (
            state.scene.world_position(tip),
            state.scene.world_rotation(tip),
        );
        state.scene.set_world_position(t, p);
        state.scene.set_world_rotation(t, r);
        c.target = Some(t);
    }
    if c.hint.is_none() {
        let h = state.scene.spawn(&format!("{tip_name}_hint"), Some(e));
        let (a, b, t) = (
            state.scene.world_position(root),
            state.scene.world_position(mid),
            state.scene.world_position(tip),
        );
        // Out from the elbow, away from the root → tip line.
        let along = (t - a).normalize_or_zero();
        let out = ((b - a) - along * (b - a).dot(along))
            .try_normalize()
            .unwrap_or_else(|| along.any_orthonormal_vector());
        let len = (b - a).length().max(0.1);
        state.scene.set_world_position(h, b + out * len * 0.5);
        c.hint = Some(h);
    }
    let _ = state.scene.world.insert_one(e, c);
}

fn multi_aim(ui: &mut Ui, c: &mut MultiAim, objs: &Objects) -> bool {
    let mut ch = grid(ui, "maim_w", |ui| weight_row(ui, &mut c.weight));
    sub_header(ui, "Constrained Object");
    ch |= grid(ui, "maim_obj", |ui| {
        let mut ch = object_row(
            ui,
            "Object",
            "constrained_object",
            &mut c.constrained_object,
            objs,
        );
        ch |= axis_row(ui, "Aim Axis", "aim_axis", &mut c.aim_axis);
        ch |= axis_row(ui, "Up Axis", "up_axis", &mut c.up_axis);
        ui.label("World Up Type");
        ch |= combo_k(
            ui,
            "world_up_type",
            "world_up_type",
            &mut c.world_up_type,
            &WorldUpType::ALL,
            WorldUpType::label,
        );
        ui.end_row();
        if matches!(
            c.world_up_type,
            WorldUpType::ObjectUp | WorldUpType::ObjectRotationUp
        ) {
            ch |= object_row(
                ui,
                "World Up Object",
                "world_up_object",
                &mut c.world_up_object,
                objs,
            );
        }
        ch
    });
    if matches!(
        c.world_up_type,
        WorldUpType::Vector | WorldUpType::ObjectRotationUp
    ) {
        let mut v = c.world_up_vector.to_array();
        if vec3_row_mixed(
            ui,
            "World Up Vector",
            &mut v,
            0.01,
            mixed::axes("world_up_vector"),
        ) {
            c.world_up_vector = Vec3::from(v);
            ch = true;
        }
    }
    if c.aim_axis
        .vector()
        .cross(c.up_axis.vector())
        .length_squared()
        < 0.5
        && c.world_up_type != WorldUpType::None
    {
        ui.weak("Aim and Up axes are parallel: the up direction is ignored.");
    }
    sub_header(ui, "Source Objects");
    ch |= sources(ui, &mut c.sources, objs);
    sub_header(ui, "Settings");
    ch |= grid(ui, "maim_settings", |ui| {
        check_row(
            ui,
            "Maintain Offset",
            "maintain_offset",
            &mut c.maintain_offset,
            "Keep the starting rotation difference instead of snapping to the aim.",
        )
    });
    let mut off = c.offset.to_array();
    if vec3_row_mixed(ui, "Offset", &mut off, 0.5, mixed::axes("offset")) {
        c.offset = Vec3::from(off);
        ch = true;
    }
    ch |= grid(ui, "maim_limits", |ui| {
        ui.label("Constrained Axes");
        let mut ch = false;
        ui.horizontal(|ui| {
            for (i, l) in ["X", "Y", "Z"].into_iter().enumerate() {
                ch |= ui.checkbox(&mut c.constrained_axes[i], l).changed();
            }
        });
        ui.end_row();
        ui.label("Min Limit")
            .on_hover_text("Degrees, per constrained axis.");
        ch |= ui
            .add(mixed::drag(
                "min_limit",
                DragValue::new(&mut c.min_limit)
                    .speed(0.5)
                    .range(-180.0..=180.0)
                    .suffix("°"),
            ))
            .changed();
        ui.end_row();
        ui.label("Max Limit")
            .on_hover_text("Degrees, per constrained axis.");
        ch |= ui
            .add(mixed::drag(
                "max_limit",
                DragValue::new(&mut c.max_limit)
                    .speed(0.5)
                    .range(-180.0..=180.0)
                    .suffix("°"),
            ))
            .changed();
        ui.end_row();
        ch
    });
    ch
}

/// Weighted source list with add / remove buttons.
fn sources(ui: &mut Ui, list: &mut Vec<WeightedSource>, objs: &Objects) -> bool {
    let mut ch = false;
    let mut remove = None;
    if list.is_empty() {
        ui.weak("No sources: add the objects to aim at.");
    }
    for (i, s) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let name = s
                .object
                .and_then(|t| objs.iter().find(|(e, _)| *e == t))
                .map_or("None (Transform)".to_owned(), |(_, n)| {
                    n.trim_start().to_owned()
                });
            let w = ui.available_width();
            egui::ComboBox::from_id_salt(("aim_source", i))
                .selected_text(name)
                .width((w * 0.5).max(80.0))
                .height(320.0)
                .show_ui(ui, |ui| {
                    ch |= ui
                        .selectable_value(&mut s.object, None, "None (Transform)")
                        .changed();
                    for (e, n) in objs {
                        ch |= ui.selectable_value(&mut s.object, Some(*e), n).changed();
                    }
                });
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(40.0);
            ch |= ui.add(Slider::new(&mut s.weight, 0.0..=1.0)).changed();
            if ui
                .small_button("−")
                .on_hover_text("Remove source")
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
    if ui.button("+ Add Source").clicked() {
        list.push(WeightedSource::default());
        ch = true;
    }
    ch
}

fn damped(ui: &mut Ui, c: &mut DampedTransform, objs: &Objects) -> bool {
    let mut ch = grid(ui, "damped_w", |ui| weight_row(ui, &mut c.weight));
    sub_header(ui, "Constrained Object");
    ch |= grid(ui, "damped_obj", |ui| {
        object_row(
            ui,
            "Object",
            "constrained_object",
            &mut c.constrained_object,
            objs,
        )
    });
    sub_header(ui, "Source Objects");
    ch |= grid(ui, "damped_src", |ui| {
        object_row(ui, "Source", "source", &mut c.source, objs)
    });
    sub_header(ui, "Settings");
    ch |= grid(ui, "damped_settings", |ui| {
        let mut ch = slider_row(
            ui,
            "Damp Position",
            "damp_position",
            &mut c.damp_position,
            "0 = follows rigidly, 1 = does not move.",
        );
        ch |= slider_row(
            ui,
            "Damp Rotation",
            "damp_rotation",
            &mut c.damp_rotation,
            "0 = turns with the source, 1 = does not turn.",
        );
        ch |= check_row(
            ui,
            "Maintain Aim",
            "maintain_aim",
            &mut c.maintain_aim,
            "Turn so the object keeps pointing away from the source as it lags.",
        );
        ch
    });
    ch
}

fn chain_ik(ui: &mut Ui, c: &mut ChainIk, objs: &Objects) -> bool {
    let mut ch = grid(ui, "chain_w", |ui| weight_row(ui, &mut c.weight));
    sub_header(ui, "Constrained Objects");
    ch |= grid(ui, "chain_bones", |ui| {
        let mut ch = object_row(ui, "Root", "root", &mut c.root, objs);
        ch |= object_row(ui, "Tip", "tip", &mut c.tip, objs);
        ch
    });
    sub_header(ui, "Source Objects");
    ch |= grid(ui, "chain_src", |ui| {
        object_row(ui, "Target", "target", &mut c.target, objs)
    });
    sub_header(ui, "Settings");
    ch |= grid(ui, "chain_settings", |ui| {
        ui.label("Solver");
        let mut ch = combo_k(
            ui,
            "solver",
            "solver",
            &mut c.solver,
            &ChainSolver::ALL,
            ChainSolver::label,
        );
        ui.end_row();
        ui.label("Maintain Target Offset");
        ui.horizontal(|ui| {
            ch |= mixed::checkbox(
                ui,
                "maintain_target_position_offset",
                &mut c.maintain_target_position_offset,
                "Position",
            )
            .changed();
            ch |= mixed::checkbox(
                ui,
                "maintain_target_rotation_offset",
                &mut c.maintain_target_rotation_offset,
                "Rotation",
            )
            .changed();
        });
        ui.end_row();
        ch |= slider_row(
            ui,
            "Chain Rotation Weight",
            "chain_rotation_weight",
            &mut c.chain_rotation_weight,
            "How much each bone turns towards the solved pose.",
        );
        ch |= slider_row(
            ui,
            "Tip Rotation Weight",
            "tip_rotation_weight",
            &mut c.tip_rotation_weight,
            "How much the tip takes the target's rotation.",
        );
        ui.label("Max Iterations");
        ch |= ui
            .add(mixed::drag(
                "max_iterations",
                DragValue::new(&mut c.max_iterations).range(1..=200),
            ))
            .changed();
        ui.end_row();
        ui.label("Tolerance")
            .on_hover_text("Stop once the tip is this close to the target.");
        ch |= ui
            .add(mixed::drag(
                "tolerance",
                DragValue::new(&mut c.tolerance)
                    .speed(0.0001)
                    .range(0.0..=1.0)
                    .max_decimals(5)
                    .suffix(" m"),
            ))
            .changed();
        ui.end_row();
        ch
    });
    if let ChainSolver::Ccd = c.solver {
        ui.weak("CCD bends the joints nearest the tip the most.");
    }
    ch
}

fn override_transform(ui: &mut Ui, c: &mut OverrideTransform, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ovr_w", |ui| weight_row(ui, &mut c.weight));
    sub_header(ui, "Constrained Object");
    ch |= grid(ui, "ovr_obj", |ui| {
        object_row(
            ui,
            "Object",
            "constrained_object",
            &mut c.constrained_object,
            objs,
        )
    });
    sub_header(ui, "Source Objects");
    ch |= grid(ui, "ovr_src", |ui| {
        object_row(ui, "Override Source", "source", &mut c.source, objs)
    });
    sub_header(ui, "Settings");
    ch |= grid(ui, "ovr_space", |ui| {
        ui.label("Space");
        let ch = combo_k(
            ui,
            "space",
            "space",
            &mut c.space,
            &OverrideSpace::ALL,
            OverrideSpace::label,
        );
        ui.end_row();
        ch
    });
    ui.add_enabled_ui(c.source.is_none(), |ui| {
        let mut p = c.position.to_array();
        if vec3_row_mixed(
            ui,
            "Override Position",
            &mut p,
            0.01,
            mixed::axes("position"),
        ) {
            c.position = Vec3::from(p);
            ch = true;
        }
        let mut r = c.rotation.to_array();
        if vec3_row_mixed(
            ui,
            "Override Rotation",
            &mut r,
            0.5,
            mixed::axes("rotation"),
        ) {
            c.rotation = Vec3::from(r);
            ch = true;
        }
    });
    ch |= grid(ui, "ovr_weights", |ui| {
        let mut ch = slider_row(
            ui,
            "Position Weight",
            "position_weight",
            &mut c.position_weight,
            "Share of the override position.",
        );
        ch |= slider_row(
            ui,
            "Rotation Weight",
            "rotation_weight",
            &mut c.rotation_weight,
            "Share of the override rotation.",
        );
        ch
    });
    ch
}
