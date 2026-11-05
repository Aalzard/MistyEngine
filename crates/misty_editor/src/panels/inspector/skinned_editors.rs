//! Inspector editor of the Skinned Mesh Renderer: bounds, quality switches, mesh, root bone and
//! bones, a slider (0–100) per blend shape under a bold "BlendShapes" sub-header, materials and
//! lighting.

use egui::{CollapsingHeader, Slider, Ui};
use misty_engine::hecs::Entity;
use misty_engine::models;
use misty_engine::scene::{MeshRef, SkinnedMeshRenderer};

use super::card::{grid, sub_header};
use super::mixed;
use crate::assets::AssetDatabase;
use crate::panels::AssetPayload;
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Skinned Mesh Renderer"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Skinned Mesh Renderer").then_some("🦴")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&SkinnedMeshRenderer>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut SkinnedMeshRenderer>(e)
        .map(|mut c| c.enabled = on);
}

/// Name of a bone object ("None (Transform)" when missing).
fn object_name(state: &EditorState, e: Option<Entity>) -> String {
    match e.filter(|e| state.scene.contains(*e)) {
        Some(e) => state.scene.name(e),
        None => "None (Transform)".into(),
    }
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut r) = state
        .scene
        .world
        .get::<&SkinnedMeshRenderer>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = false;

    // Bounds (root bone space).
    let mut center = r.bounds_center.to_array();
    if vec3_row(ui, "Bounds Center", &mut center, 0.01) {
        r.bounds_center = center.into();
        ch = true;
    }
    let mut extent = r.bounds_extents.to_array();
    if vec3_row(ui, "Bounds Extent", &mut extent, 0.01) {
        r.bounds_extents = glam::Vec3::from(extent).abs();
        ch = true;
    }

    ch |= grid(ui, "smr_main_grid", |ui| {
        let mut ch = false;
        ui.label("Update When Offscreen").on_hover_text(
            "Compute the bounds from the posed bones every frame instead of using the box above.",
        );
        ch |= mixed::checkbox(
            ui,
            "update_when_offscreen",
            &mut r.update_when_offscreen,
            "",
        )
        .changed();
        ui.end_row();

        ui.label("Mesh");
        let current = r
            .mesh
            .as_ref()
            .map(models::display_name)
            .unwrap_or_else(|| "None (Mesh)".into());
        let slot = ui.add(egui::Button::new(format!("▦ {current}")).truncate());
        if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
            && models::is_model(&p.0)
        {
            r.mesh = Some(MeshRef::asset(&p.0, 0));
            ch = true;
        }
        ui.end_row();

        ui.label("Root Bone");
        ui.add(egui::Label::new(format!("⬚ {}", object_name(state, r.root_bone))).truncate());
        ui.end_row();
        ch
    });

    CollapsingHeader::new(format!("Bones ({})", r.bones.len()))
        .id_salt("smr_bones")
        .default_open(false)
        .show(ui, |ui| {
            grid(ui, "smr_bones_grid", |ui| {
                for (i, b) in r.bones.iter().enumerate() {
                    ui.label(format!("Element {i}"));
                    ui.add(egui::Label::new(format!("⬚ {}", object_name(state, *b))).truncate());
                    ui.end_row();
                }
                false
            })
        });

    // Blend shapes, one 0–100 slider each.
    let names = r
        .mesh
        .as_ref()
        .and_then(misty_engine::skinning::blend_shape_names)
        .unwrap_or_default();
    if !names.is_empty() {
        sub_header(ui, "BlendShapes");
        ch |= grid(ui, "smr_blend_shapes", |ui| {
            let mut ch = false;
            for (i, name) in names.iter().enumerate() {
                ui.label(name);
                let mut w = r.blend_shape_weight(i);
                let s = Slider::new(&mut w, 0.0..=100.0).clamping(egui::SliderClamping::Never);
                if ui
                    .add(mixed::slider(&format!("blend_shape_{i}"), s))
                    .changed()
                {
                    r.set_blend_shape_weight(i, w);
                    ch = true;
                }
                ui.end_row();
            }
            ch
        });
    }

    sub_header(ui, "Materials");
    let materials: Vec<String> = state.assets.materials.keys().cloned().collect();
    ch |= grid(ui, "smr_materials", |ui| {
        let mut ch = false;
        ui.label("Size");
        let mut size = r.materials.len().max(1);
        if ui
            .add(egui::DragValue::new(&mut size).range(1..=8))
            .changed()
        {
            r.materials.resize(size, None);
            ch = true;
        }
        ui.end_row();
        for i in 0..r.materials.len() {
            ui.label(format!("Element {i}"));
            ch |= material_slot(ui, &mut r.materials[i], i, &materials);
            ui.end_row();
        }
        ch
    });

    sub_header(ui, "Lighting");
    ch |= grid(ui, "smr_lighting", |ui| {
        let mut ch = false;
        ui.label("Cast Shadows");
        egui::ComboBox::from_id_salt("smr_cast")
            .selected_text(mixed::text(
                "cast_shadows",
                if r.cast_shadows { "On" } else { "Off" },
            ))
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, true, "On")
                    .changed();
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, false, "Off")
                    .changed();
            });
        ui.end_row();
        ui.label("Receive Shadows");
        ch |= mixed::checkbox(ui, "receive_shadows", &mut r.receive_shadows, "").changed();
        ui.end_row();
        ch
    });

    if ch {
        let _ = state.scene.world.insert_one(e, r);
    }
    ch
}

/// One Materials element: a dropdown of the project's materials (or a dropped `.mat`).
fn material_slot(ui: &mut Ui, value: &mut Option<String>, i: usize, names: &[String]) -> bool {
    let mut ch = false;
    let current = AssetDatabase::material_name(value.as_deref());
    let slot = egui::ComboBox::from_id_salt(("smr_material", i))
        .selected_text(format!("◍ {current}"))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(value, None, crate::assets::DEFAULT_MATERIAL)
                .changed();
            for n in names {
                let label = AssetDatabase::material_name(Some(n));
                ch |= ui.selectable_value(value, Some(n.clone()), label).changed();
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<AssetPayload>()
        && path.0.ends_with(".mat")
    {
        *value = Some(path.0.clone());
        ch = true;
    }
    ch
}
