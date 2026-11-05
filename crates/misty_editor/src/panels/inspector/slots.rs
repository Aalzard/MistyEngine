//! Asset slots in the Inspector: texture pickers for materials and the Mesh Filter mesh list.
//! Both accept assets dragged from the Project window.

use std::path::Path;

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::models;
use misty_engine::scene::{MeshFilter, MeshRef, PrimitiveMesh};

use crate::panels::AssetPayload;
use crate::state::EditorState;

const NO_TEXTURE: &str = "None (Texture)";

fn file_name(rel: &str) -> String {
    Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_owned())
}

/// Texture dropdown (project textures) that also takes a texture dropped from the Project window.
pub fn texture_slot(
    ui: &mut Ui,
    id: &str,
    value: &mut Option<String>,
    textures: &[String],
) -> bool {
    let mut ch = false;
    let current = value.as_deref().map(file_name);
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(format!("🖼 {}", current.as_deref().unwrap_or(NO_TEXTURE)))
        // Leaves room for the colour / strength that often follows on the same row.
        .width((ui.available_width() * 0.55).max(100.0))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, NO_TEXTURE).changed();
            for t in textures {
                ch |= ui
                    .selectable_value(value, Some(t.clone()), file_name(t))
                    .on_hover_text(t)
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
        && misty_engine::assets::is_texture(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// Every mesh that can go in a Mesh Filter: primitives, then each imported model's meshes.
fn mesh_choices(state: &EditorState) -> Vec<(MeshRef, String)> {
    let mut out: Vec<(MeshRef, String)> = PrimitiveMesh::ALL
        .iter()
        .map(|p| (MeshRef::Primitive(*p), p.name().to_owned()))
        .collect();
    for (rel, names) in &state.assets.models {
        let Ok(names) = names else { continue };
        let file = file_name(rel);
        for (i, n) in names.iter().enumerate() {
            out.push((MeshRef::asset(rel, i), format!("{file} ▸ {n}")));
        }
    }
    out
}

/// Mesh Filter: mesh dropdown; a model dropped from the Project window uses its first mesh.
pub fn mesh_filter(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut f) = state
        .scene
        .world
        .get::<&MeshFilter>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    // Baked from an Editable Mesh: the Mesh Builder owns the mesh.
    if crate::mesh_builder::has_editable(state, e) {
        crate::panels::inspector::card::PropertyGrid::new("meshfilter_grid").show(ui, |ui| {
            ui.label("Mesh");
            ui.weak("▦ Editable Mesh (Mesh Builder)");
            ui.end_row();
        });
        return false;
    }
    let choices = mesh_choices(state);
    let mut ch = false;
    crate::panels::inspector::card::PropertyGrid::new("meshfilter_grid").show(ui, |ui| {
        ui.label("Mesh");
        let current = f
            .mesh
            .as_ref()
            .map(models::display_name)
            .unwrap_or_else(|| "None (Mesh)".into());
        let slot = egui::ComboBox::from_id_salt("mesh_filter_mesh")
            .selected_text(format!("▦ {current}"))
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut f.mesh, None, "None (Mesh)")
                    .changed();
                for (i, (r, label)) in choices.iter().enumerate() {
                    if i == PrimitiveMesh::ALL.len() {
                        ui.separator();
                    }
                    ch |= ui
                        .selectable_value(&mut f.mesh, Some(r.clone()), label)
                        .changed();
                }
            })
            .response;
        if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
            && models::is_model(&p.0)
        {
            f.mesh = Some(MeshRef::asset(&p.0, 0));
            ch = true;
        }
        ui.end_row();
    });
    if f.mesh.is_some()
        && !ch
        && ui
            .button("Convert to Editable Mesh")
            .on_hover_text("Model this mesh with the Mesh Builder (an editable copy)")
            .clicked()
    {
        // Its own undo step (not an Inspector edit).
        crate::mesh_builder::convert_to_editable(state, e);
        return false;
    }
    if ch {
        let _ = state.scene.world.insert_one(e, f);
    }
    ch
}
