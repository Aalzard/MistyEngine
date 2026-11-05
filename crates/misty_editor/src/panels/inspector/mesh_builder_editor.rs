//! Editable Mesh Inspector: element counts, edit mode, operation amounts and buttons, and the
//! material, smoothing and UV settings of the selected faces.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::EditableMesh;
use misty_engine::scene::MeshRenderer;

use super::card::{grid, sub_header};
use crate::mesh_builder::{EditMode, Op, run};
use crate::state::EditorState;

pub const NAMES: [&str; 1] = ["Editable Mesh"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Editable Mesh").then_some("⬢")
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Some(mesh) = crate::mesh_builder::mesh_of(state, e) else {
        return false;
    };
    ui.weak(format!(
        "{} vertices · {} edges · {} faces",
        mesh.positions.len(),
        mesh.edges().len(),
        mesh.faces.len()
    ));
    ui.add_space(4.0);
    let editing_this = state.selection.active() == Some(e);
    ui.horizontal(|ui| {
        for mode in EditMode::ALL {
            let on = state.mesh_builder.mode == mode;
            let b = ui
                .add_enabled(editing_this, egui::Button::selectable(on, mode.label()))
                .on_hover_text(mode.tooltip());
            if b.clicked() {
                state.mesh_builder.mode = mode;
            }
        }
    });

    sub_header(ui, "Operations");
    let s = &mut state.mesh_builder.settings;
    grid(ui, "mesh_builder_amounts", |ui| {
        amount(ui, "Extrude Distance", &mut s.extrude, -100.0);
        amount(ui, "Inset Amount", &mut s.inset, 0.0);
        amount(ui, "Bevel Amount", &mut s.bevel, 0.0);
        amount(ui, "Weld Distance", &mut s.weld, 0.0);
    });
    ui.horizontal_wrapped(|ui| {
        for op in Op::ALL {
            if op.object_level() || op == Op::SelectAll {
                continue;
            }
            let enabled = editing_this && op.enabled(state);
            let tip = format!("{} ({})", op.tooltip(), op.shortcut_text(ui.ctx()));
            if ui
                .add_enabled(enabled, egui::Button::new(op.label()))
                .on_hover_text(tip)
                .clicked()
            {
                run(state, op);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for op in [Op::CenterPivot, Op::FreezeTransform, Op::ExportModel] {
            let enabled = editing_this && op.enabled(state);
            if ui
                .add_enabled(enabled, egui::Button::new(op.label()))
                .on_hover_text(op.tooltip())
                .clicked()
            {
                run(state, op);
            }
        }
        if ui.button("New Shape…").clicked() {
            crate::mesh_builder::open_new_shape(state);
        }
    });

    // Settings of the selected faces (the first one's values are shown).
    let faces: Vec<usize> = match state.mesh_builder.mode {
        EditMode::Face if editing_this => state.mesh_builder.selection.faces.clone(),
        _ => Vec::new(),
    };
    let Some(&first) = faces.first() else {
        if state.mesh_builder.mode == EditMode::Face && editing_this {
            ui.weak("Select faces to set their material, smoothing and UVs.");
        }
        return false;
    };
    let materials = material_names(state, e);
    let mut changed = false;
    let mut m: EditableMesh = mesh;
    let mut face = m.faces[first].clone();
    sub_header(ui, &format!("Selected Faces ({})", faces.len()));
    grid(ui, "mesh_builder_faces", |ui| {
        ui.label("Material");
        let current = materials
            .get(face.material as usize)
            .cloned()
            .unwrap_or_else(|| format!("Element {}", face.material));
        egui::ComboBox::from_id_salt("mesh_builder_face_material")
            .selected_text(current)
            .show_ui(ui, |ui| {
                // One more than the list: assigning it adds a Materials element.
                for (i, name) in materials.iter().enumerate() {
                    changed |= ui
                        .selectable_value(&mut face.material, i as u32, name)
                        .changed();
                }
                let next = materials.len() as u32;
                changed |= ui
                    .selectable_value(&mut face.material, next, "New Element")
                    .changed();
            });
        ui.end_row();
        ui.label("Smoothing Group");
        changed |= ui
            .add(egui::DragValue::new(&mut face.smoothing).range(0..=32))
            .on_hover_text("Faces sharing a group shade smoothly; 0 = hard edges")
            .changed();
        ui.end_row();
        ui.label("World Space UVs");
        changed |= ui
            .checkbox(&mut face.uv.world, "")
            .on_hover_text("Project world positions so textures line up across faces and objects")
            .changed();
        ui.end_row();
        ui.label("UV Offset");
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut face.uv.offset.x).speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(&mut face.uv.offset.y).speed(0.01))
                .changed();
        });
        ui.end_row();
        ui.label("UV Tiling");
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut face.uv.scale.x).speed(0.01))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(&mut face.uv.scale.y).speed(0.01))
                .changed();
        });
        ui.end_row();
        ui.label("UV Rotation");
        changed |= ui
            .add(
                egui::DragValue::new(&mut face.uv.rotation)
                    .speed(1.0)
                    .suffix("°"),
            )
            .changed();
        ui.end_row();
    });
    if changed {
        for f in &faces {
            if let Some(target) = m.faces.get_mut(*f) {
                target.material = face.material;
                target.smoothing = face.smoothing;
                target.uv = face.uv;
            }
        }
        let needed = m.material_count();
        let _ = state.scene.world.insert_one(e, m);
        if let Ok(mut r) = state.scene.world.get::<&mut MeshRenderer>(e) {
            while r.material_count() < needed {
                r.extra_materials.push(None);
            }
        }
    }
    changed
}

fn amount(ui: &mut Ui, label: &str, v: &mut f32, min: f32) {
    ui.label(label);
    ui.add(
        egui::DragValue::new(v)
            .speed(0.01)
            .range(min..=100.0)
            .max_decimals(3)
            .suffix(" m"),
    );
    ui.end_row();
}

/// "0: Brick" … per Materials element of the object's Mesh Renderer.
fn material_names(state: &EditorState, e: Entity) -> Vec<String> {
    let Ok(r) = state.scene.world.get::<&MeshRenderer>(e) else {
        return vec!["0: Default-Material".into()];
    };
    (0..r.material_count())
        .map(|i| {
            let name = r
                .material_at(i)
                .and_then(|p| std::path::Path::new(p).file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Default-Material".into());
            format!("{i}: {name}")
        })
        .collect()
}
