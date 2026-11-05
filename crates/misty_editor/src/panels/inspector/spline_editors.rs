//! Inspector editors of the spline components: Spline Container (Draw Spline toggle, splines,
//! the knots selected in the Scene view), Spline Extrude, Spline Instantiate, Spline Animate and
//! the Spline Instance marker.

use egui::{RichText, Ui};
use glam::Vec3;
use misty_engine::camera::{euler_to_quat, quat_to_euler};
use misty_engine::hecs::Entity;
use misty_engine::scene::{PrimitiveMesh, SceneComponent};
use misty_engine::splines::components::{
    AlignMode, AnimateAlignment, AnimateMethod, Easing, ExtrudeProfile, InstantiateItem,
    InstantiateMethod, LoopMode,
};
use misty_engine::splines::{
    Spline, SplineAnimate, SplineContainer, SplineExtrude, SplineInstantiate, TangentMode, animate,
};

use super::card::{grid, sub_header};
use crate::assets::AssetDatabase;
use crate::panels::AssetPayload;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::vec3_row;

pub const NAMES: [&str; 5] = [
    "Spline Container",
    "Spline Extrude",
    "Spline Instantiate",
    "Spline Animate",
    "Spline Instance",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Spline Container" => "〰",
        "Spline Extrude" => "◍",
        "Spline Instantiate" => "⁘",
        "Spline Animate" => "➰",
        "Spline Instance" => "⁘",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Spline Extrude" => w.get::<&SplineExtrude>(e).ok().map(|c| c.enabled),
        "Spline Instantiate" => w.get::<&SplineInstantiate>(e).ok().map(|c| c.enabled),
        "Spline Animate" => w.get::<&SplineAnimate>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &state.scene.world;
    let _ = match name {
        "Spline Extrude" => w.get::<&mut SplineExtrude>(e).map(|mut c| c.enabled = on),
        "Spline Instantiate" => w
            .get::<&mut SplineInstantiate>(e)
            .map(|mut c| c.enabled = on),
        "Spline Animate" => w.get::<&mut SplineAnimate>(e).map(|mut c| c.enabled = on),
        _ => return,
    };
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Spline Container" => container(ui, state, e),
        "Spline Extrude" => edit_copy::<SplineExtrude>(state, e, |s, c| extrude(ui, s, e, c)),
        "Spline Instantiate" => {
            edit_copy::<SplineInstantiate>(state, e, |s, c| instantiate(ui, s, e, c))
        }
        "Spline Animate" => {
            edit_copy::<SplineAnimate>(state, e, |s, c| animate_editor(ui, s, e, c))
        }
        "Spline Instance" => {
            ui.weak("Made by the Spline Instantiate on the parent; replaced when it rebuilds.");
            false
        }
        _ => false,
    }
}

fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&EditorState, &mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = f(state, &mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, min: f32, max: f32) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            egui::DragValue::new(v)
                .speed(speed)
                .range(min..=max)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    all: &[T],
    name: impl Fn(T) -> &'static str,
) -> bool {
    ui.label(label);
    let mut ch = false;
    egui::ComboBox::from_id_salt(("spline_combo", label))
        .selected_text(name(*value))
        .show_ui(ui, |ui| {
            for v in all {
                ch |= ui.selectable_value(value, *v, name(*v)).changed();
            }
        });
    ui.end_row();
    ch
}

fn checkbox(ui: &mut Ui, label: &str, v: &mut bool) -> bool {
    ui.label(label);
    let ch = ui.checkbox(v, "").changed();
    ui.end_row();
    ch
}

/// Spline Container field: objects with a container, "None" meaning this object.
fn container_field(
    ui: &mut Ui,
    state: &EditorState,
    e: Entity,
    value: &mut Option<Entity>,
) -> bool {
    ui.label("Container");
    let objects: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| o)
        .filter(|o| state.scene.world.get::<&SplineContainer>(*o).is_ok())
        .map(|o| (o, state.scene.name(o)))
        .collect();
    let current = match value {
        Some(c) if state.scene.contains(*c) => state.scene.name(*c),
        _ if state.scene.world.get::<&SplineContainer>(e).is_ok() => "This object".into(),
        _ => "None (Spline Container)".into(),
    };
    let mut ch = false;
    egui::ComboBox::from_id_salt("spline_container_field")
        .selected_text(format!("〰 {current}"))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, "This object").changed();
            for (o, n) in objects {
                ch |= ui.selectable_value(value, Some(o), n).changed();
            }
        });
    ui.end_row();
    ch
}

fn container(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut c) = state
        .scene
        .world
        .get::<&SplineContainer>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = false;
    ui.horizontal(|ui| {
        let drawing = state.splines.drawing;
        let label = if drawing {
            "✔ Done Drawing"
        } else {
            "✏ Draw Spline"
        };
        if ui
            .selectable_label(drawing, label)
            .on_hover_text("Click in the Scene view to place knots, drag for tangents")
            .clicked()
        {
            state.splines.set_drawing(!drawing);
        }
        if ui.button("Add Spline").clicked() {
            c.splines.push(Spline::default());
            ch = true;
        }
    });
    let mut remove = None;
    let count = c.splines.len();
    for (i, s) in c.splines.iter_mut().enumerate() {
        let path = s.path();
        sub_header(ui, &format!("Spline {i}"));
        ch |= grid(ui, ("spline_info", i), |ui| {
            ui.label("Knots");
            ui.label(s.knots.len().to_string());
            ui.end_row();
            ui.label("Length");
            ui.label(format!("{:.2}", path.length));
            ui.end_row();
            checkbox(ui, "Closed", &mut s.closed)
        });
        ui.horizontal(|ui| {
            if ui.button("Add Knot").clicked() {
                let mut tmp = SplineContainer {
                    splines: vec![s.clone()],
                };
                crate::spline_tools::append_knot(&mut tmp, 0);
                *s = tmp.splines.remove(0);
                ch = true;
            }
            if ui.button("Reverse").clicked() {
                s.knots.reverse();
                for k in &mut s.knots {
                    std::mem::swap(&mut k.tangent_in, &mut k.tangent_out);
                    k.rotation *= glam::Quat::from_rotation_y(std::f32::consts::PI);
                    k.tangent_in = glam::Quat::from_rotation_y(std::f32::consts::PI) * k.tangent_in;
                    k.tangent_out =
                        glam::Quat::from_rotation_y(std::f32::consts::PI) * k.tangent_out;
                }
                ch = true;
            }
            if count > 1 && ui.button("Remove Spline").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        c.splines.remove(i);
        ch = true;
    }

    // Knots selected in the Scene view.
    let selected: Vec<_> = state
        .splines
        .selected
        .iter()
        .copied()
        .filter(|r| state.splines.entity == Some(e) && c.knot_mut(r.spline, r.knot).is_some())
        .collect();
    if let Some(r) = selected.first() {
        sub_header(
            ui,
            &if selected.len() == 1 {
                format!("Knot {} (Spline {})", r.knot, r.spline)
            } else {
                format!("{} Knots (editing the first)", selected.len())
            },
        );
        let k = c.knot_mut(r.spline, r.knot).expect("filtered above");
        let mut pos = k.position.to_array();
        if vec3_row(ui, "Position", &mut pos, 0.05) {
            k.position = Vec3::from(pos);
            ch = true;
        }
        let mut euler = quat_to_euler(k.rotation).to_array();
        if vec3_row(ui, "Rotation", &mut euler, 1.0) {
            k.rotation = euler_to_quat(Vec3::from(euler));
            ch = true;
        }
        let mut mode = k.mode;
        ch |= grid(ui, "knot_mode", |ui| {
            let mut ch = combo(ui, "Tangent Mode", &mut mode, &TangentMode::ALL, |m| {
                m.label()
            });
            if mode == TangentMode::AutoSmooth {
                ch |= number(ui, "Tension", &mut k.tension, 0.01, 0.0, 1.0);
            }
            ch
        });
        if mode != k.mode {
            if mode.is_bezier() && !k.mode.is_bezier() {
                // Start from the curve's current shape so switching doesn't jump.
                let spline = &c.splines[r.spline];
                let (tin, tout) = spline.tangents(r.knot);
                let k = c.knot_mut(r.spline, r.knot).expect("exists");
                let inv = k.rotation.inverse();
                k.tangent_in = inv * tin;
                k.tangent_out = inv * tout;
            }
            let k = c.knot_mut(r.spline, r.knot).expect("exists");
            k.mode = mode;
            k.enforce(false);
            ch = true;
        }
        let k = c.knot_mut(r.spline, r.knot).expect("exists");
        if k.mode.is_bezier() {
            let mut tin = k.tangent_in.to_array();
            if vec3_row(ui, "Tangent In", &mut tin, 0.05) {
                k.tangent_in = Vec3::from(tin);
                k.enforce(true);
                ch = true;
            }
            let mut tout = k.tangent_out.to_array();
            if vec3_row(ui, "Tangent Out", &mut tout, 0.05) {
                k.tangent_out = Vec3::from(tout);
                k.enforce(false);
                ch = true;
            }
        }
    } else {
        ui.label(
            RichText::new("Select knots in the Scene view to edit them here.")
                .color(theme::TEXT_DIM),
        );
    }
    if ch {
        let _ = state.scene.world.insert_one(e, c);
    }
    ch
}

fn extrude(ui: &mut Ui, state: &EditorState, e: Entity, c: &mut SplineExtrude) -> bool {
    grid(ui, "spline_extrude", |ui| {
        let mut ch = container_field(ui, state, e, &mut c.container);
        ch |= combo(
            ui,
            "Shape",
            &mut c.profile,
            &[ExtrudeProfile::Circle, ExtrudeProfile::Road],
            |p| match p {
                ExtrudeProfile::Circle => "Circle",
                ExtrudeProfile::Road => "Road",
            },
        );
        match c.profile {
            ExtrudeProfile::Circle => {
                ch |= number(ui, "Radius", &mut c.radius, 0.01, 0.0, 1e4);
                ui.label("Sides");
                ch |= ui
                    .add(egui::DragValue::new(&mut c.sides).range(3..=256))
                    .changed();
                ui.end_row();
                ch |= checkbox(ui, "Cap Ends", &mut c.cap_ends);
            }
            ExtrudeProfile::Road => {
                ch |= number(ui, "Width", &mut c.width, 0.05, 0.0, 1e4);
            }
        }
        ch |= number(
            ui,
            "Segments Per Unit",
            &mut c.segments_per_unit,
            0.05,
            0.01,
            100.0,
        );
        ui.label("Range");
        ui.horizontal(|ui| {
            ch |= ui
                .add(
                    egui::DragValue::new(&mut c.range[0])
                        .speed(0.005)
                        .range(0.0..=1.0),
                )
                .changed();
            ch |= ui
                .add(
                    egui::DragValue::new(&mut c.range[1])
                        .speed(0.005)
                        .range(0.0..=1.0),
                )
                .changed();
        });
        ui.end_row();
        ch |= number(ui, "UV Tiling", &mut c.uv_tiling, 0.01, 0.0, 1000.0);
        ch
    })
}

/// Prefab slot: drop a `.prefab` here; the menu clears it.
fn prefab_field(ui: &mut Ui, i: usize, prefab: &mut Option<String>) -> bool {
    let mut ch = false;
    let current = prefab
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "None (Prefab)".into());
    let slot = egui::ComboBox::from_id_salt(("spline_prefab", i))
        .selected_text(format!("⬢ {current}"))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(prefab, None, "None (use the primitive)")
                .changed();
            ui.weak("Drop a prefab from the Project window here.");
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
        && p.0.ends_with(".prefab")
    {
        *prefab = Some(p.0.clone());
        ch = true;
    }
    ch
}

fn item(ui: &mut Ui, i: usize, it: &mut InstantiateItem, materials: &[String]) -> bool {
    grid(ui, ("spline_item", i), |ui| {
        ui.label("Prefab");
        let mut ch = prefab_field(ui, i, &mut it.prefab);
        ui.end_row();
        if it.prefab.is_none() {
            ch |= combo(
                ui,
                "Primitive",
                &mut it.primitive,
                &PrimitiveMesh::ALL,
                |p| p.name(),
            );
            ui.label("Material");
            egui::ComboBox::from_id_salt(("spline_item_mat", i))
                .selected_text(format!(
                    "◍ {}",
                    AssetDatabase::material_name(it.material.as_deref())
                ))
                .show_ui(ui, |ui| {
                    ch |= ui
                        .selectable_value(&mut it.material, None, crate::assets::DEFAULT_MATERIAL)
                        .changed();
                    for m in materials {
                        let label = AssetDatabase::material_name(Some(m));
                        ch |= ui
                            .selectable_value(&mut it.material, Some(m.clone()), label)
                            .changed();
                    }
                });
            ui.end_row();
        }
        ch |= number(ui, "Probability", &mut it.probability, 0.01, 0.0, 1000.0);
        ch
    })
}

fn range_rows(ui: &mut Ui, label: &str, lo: &mut Vec3, hi: &mut Vec3, speed: f32) -> bool {
    let mut a = lo.to_array();
    let mut b = hi.to_array();
    let mut ch = vec3_row(ui, &format!("{label} Min"), &mut a, speed);
    ch |= vec3_row(ui, &format!("{label} Max"), &mut b, speed);
    *lo = Vec3::from(a);
    *hi = Vec3::from(b);
    ch
}

fn instantiate(ui: &mut Ui, state: &EditorState, e: Entity, c: &mut SplineInstantiate) -> bool {
    let materials: Vec<String> = state.assets.materials.keys().cloned().collect();
    let mut ch = grid(ui, "spline_inst", |ui| {
        let mut ch = container_field(ui, state, e, &mut c.container);
        ui.label("Spline");
        ch |= ui
            .add(egui::DragValue::new(&mut c.spline).range(0..=63))
            .changed();
        ui.end_row();
        ch |= combo(
            ui,
            "Instantiate Method",
            &mut c.method,
            &[
                InstantiateMethod::InstanceCount,
                InstantiateMethod::SplineDistance,
            ],
            |m| match m {
                InstantiateMethod::InstanceCount => "Instance Count",
                InstantiateMethod::SplineDistance => "Spline Distance",
            },
        );
        match c.method {
            InstantiateMethod::InstanceCount => {
                ui.label("Count");
                ch |= ui
                    .add(egui::DragValue::new(&mut c.count).range(0..=4096))
                    .changed();
                ui.end_row();
            }
            InstantiateMethod::SplineDistance => {
                ch |= number(ui, "Spacing Min", &mut c.spacing[0], 0.05, 0.01, 1e4);
                ch |= number(ui, "Spacing Max", &mut c.spacing[1], 0.05, 0.01, 1e4);
            }
        }
        ch |= combo(ui, "Align To", &mut c.align, &AlignMode::ALL, |a| a.label());
        ui.label("Seed");
        ch |= ui.add(egui::DragValue::new(&mut c.seed)).changed();
        ui.end_row();
        ch
    });
    sub_header(ui, "Offsets");
    ch |= range_rows(
        ui,
        "Position",
        &mut c.position_min,
        &mut c.position_max,
        0.05,
    );
    ch |= range_rows(
        ui,
        "Rotation",
        &mut c.rotation_min,
        &mut c.rotation_max,
        1.0,
    );
    ch |= range_rows(ui, "Scale", &mut c.scale_min, &mut c.scale_max, 0.01);
    sub_header(ui, "Items");
    let mut remove = None;
    let n = c.items.len();
    for (i, it) in c.items.iter_mut().enumerate() {
        ch |= item(ui, i, it, &materials);
        if n > 1 && ui.small_button("Remove Item").clicked() {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        c.items.remove(i);
        ch = true;
    }
    ui.horizontal(|ui| {
        if ui.button("Add Item").clicked() {
            c.items.push(InstantiateItem::default());
            ch = true;
        }
        if ui.button("Randomize").clicked() {
            c.seed = c.seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
            ch = true;
        }
    });
    let made = misty_engine::splines::instantiate::instances(&state.scene, e).len();
    ui.label(RichText::new(format!("{made} instances")).color(theme::TEXT_DIM));
    ch
}

fn animate_editor(ui: &mut Ui, state: &EditorState, e: Entity, c: &mut SplineAnimate) -> bool {
    let ch = grid(ui, "spline_anim", |ui| {
        let mut ch = container_field(ui, state, e, &mut c.container);
        ui.label("Spline");
        ch |= ui
            .add(egui::DragValue::new(&mut c.spline).range(0..=63))
            .changed();
        ui.end_row();
        ch |= checkbox(ui, "Play On Awake", &mut c.play_on_awake);
        ch |= combo(
            ui,
            "Method",
            &mut c.method,
            &[AnimateMethod::Time, AnimateMethod::Speed],
            |m| match m {
                AnimateMethod::Time => "Time",
                AnimateMethod::Speed => "Speed",
            },
        );
        match c.method {
            AnimateMethod::Time => ch |= number(ui, "Duration", &mut c.duration, 0.05, 0.001, 1e5),
            AnimateMethod::Speed => ch |= number(ui, "Speed", &mut c.speed, 0.05, 0.001, 1e5),
        }
        ch |= combo(ui, "Easing", &mut c.easing, &Easing::ALL, |x| x.label());
        ch |= combo(ui, "Loop Mode", &mut c.loop_mode, &LoopMode::ALL, |x| {
            x.label()
        });
        ch |= combo(
            ui,
            "Alignment",
            &mut c.alignment,
            &AnimateAlignment::ALL,
            |x| x.label(),
        );
        ch |= number(ui, "Start Offset", &mut c.start_offset, 0.005, 0.0, 1.0);
        ch
    });
    if state.play.in_play_mode() {
        let len = animate::length(&state.scene, e);
        let t = animate::normalized_time(c, animate::duration(c, len));
        ui.label(
            RichText::new(format!(
                "{} · normalized time {t:.3}",
                if c.playing { "Playing" } else { "Paused" }
            ))
            .color(theme::TEXT_DIM),
        );
    }
    ch
}
