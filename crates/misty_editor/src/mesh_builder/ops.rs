//! Mesh Builder operations on the selected elements, each one undo step.

use egui::{Key, KeyboardShortcut, Modifiers};
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::Selection;
use misty_engine::mesh_builder::ops as mops;
use misty_engine::scene::{MeshFilter, MeshRef, MeshRenderer, Transform};

use super::{EditMode, editing, mesh_of, target};
use crate::state::EditorState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Extrude,
    Inset,
    Bevel,
    Subdivide,
    Connect,
    Bridge,
    Merge,
    Weld,
    Detach,
    FlipNormals,
    SetMaterial,
    Delete,
    SelectAll,
    CenterPivot,
    FreezeTransform,
    ExportModel,
}

impl Op {
    pub const ALL: [Op; 16] = [
        Op::Extrude,
        Op::Inset,
        Op::Bevel,
        Op::Subdivide,
        Op::Connect,
        Op::Bridge,
        Op::Merge,
        Op::Weld,
        Op::Detach,
        Op::FlipNormals,
        Op::SetMaterial,
        Op::Delete,
        Op::SelectAll,
        Op::CenterPivot,
        Op::FreezeTransform,
        Op::ExportModel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Op::Extrude => "Extrude",
            Op::Inset => "Inset",
            Op::Bevel => "Bevel",
            Op::Subdivide => "Subdivide",
            Op::Connect => "Connect",
            Op::Bridge => "Bridge",
            Op::Merge => "Merge",
            Op::Weld => "Weld",
            Op::Detach => "Detach",
            Op::FlipNormals => "Flip Normals",
            Op::SetMaterial => "Set Material",
            Op::Delete => "Delete",
            Op::SelectAll => "Select All",
            Op::CenterPivot => "Center Pivot",
            Op::FreezeTransform => "Freeze Transform",
            Op::ExportModel => "Export to Model",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Op::Extrude => "Push faces out along their normals, or grow open edges outward",
            Op::Inset => "Make a smaller copy of each face inside it, joined by a ring of faces",
            Op::Bevel => "Cut a sloped strip along edges",
            Op::Subdivide => "Split faces into four-sided pieces around their centres",
            Op::Connect => {
                "One edge: cut an edge loop through the ring of quads. Several: join their midpoints across faces"
            }
            Op::Bridge => "Join two open edges with a new face",
            Op::Merge => "Collapse the selection to a single vertex at its centre",
            Op::Weld => "Merge selected vertices closer than the weld distance",
            Op::Detach => "Give faces their own vertices so they come loose",
            Op::FlipNormals => "Turn faces to face the other way",
            Op::SetMaterial => "Draw the faces with the chosen Materials element",
            Op::Delete => "Remove the selected elements' faces",
            Op::SelectAll => "Select every element of the current mode",
            Op::CenterPivot => "Move the object's pivot to the centre of its mesh",
            Op::FreezeTransform => {
                "Bake position, rotation and scale into the mesh and reset the Transform"
            }
            Op::ExportModel => {
                "Write the mesh to Assets/Models as an .obj and use that model instead"
            }
        }
    }

    /// Modes the operation works in (object-level ones work in every mode).
    fn modes(self) -> &'static [EditMode] {
        use EditMode::*;
        match self {
            Op::Extrude => &[Edge, Face],
            Op::Inset | Op::Subdivide | Op::Detach | Op::FlipNormals | Op::SetMaterial => &[Face],
            Op::Bevel | Op::Connect | Op::Bridge => &[Edge],
            Op::Merge | Op::Weld | Op::Delete => &[Vertex, Edge, Face],
            Op::SelectAll => &[Vertex, Edge, Face],
            Op::CenterPivot | Op::FreezeTransform | Op::ExportModel => {
                &[Object, Vertex, Edge, Face]
            }
        }
    }

    /// Object-level (no element selection needed).
    pub fn object_level(self) -> bool {
        matches!(
            self,
            Op::CenterPivot | Op::FreezeTransform | Op::ExportModel
        )
    }

    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let alt = Modifiers::ALT;
        let (mods, key) = match self {
            Op::Extrude => (Modifiers::COMMAND, Key::E),
            Op::Inset => (alt, Key::I),
            Op::Bevel => (alt | Modifiers::SHIFT, Key::B),
            Op::Subdivide => (alt, Key::S),
            Op::Connect => (alt, Key::E),
            Op::Bridge => (alt, Key::B),
            Op::Merge => (alt, Key::C),
            Op::Weld => (alt, Key::V),
            Op::Detach => (alt, Key::D),
            Op::FlipNormals => (alt, Key::N),
            Op::SelectAll => (alt, Key::A),
            _ => return None,
        };
        Some(KeyboardShortcut::new(mods, key))
    }

    /// Shortcut text for menus ("Ctrl+E").
    pub fn shortcut_text(self, ctx: &egui::Context) -> String {
        match self {
            Op::Delete => "Del".into(),
            op => op
                .shortcut()
                .map(|s| ctx.format_shortcut(&s))
                .unwrap_or_default(),
        }
    }

    /// Can it run on the current selection?
    pub fn enabled(self, state: &EditorState) -> bool {
        let mb = &state.mesh_builder;
        if target(state).is_none() || !self.modes().contains(&mb.mode) {
            return false;
        }
        if self.object_level() {
            return true;
        }
        if editing(state).is_none() {
            return false;
        }
        let s = &mb.selection;
        match self {
            Op::SelectAll => true,
            Op::Bridge => s.edges.len() == 2,
            Op::Merge => match mb.mode {
                EditMode::Vertex => s.verts.len() >= 2,
                _ => !s.is_empty(),
            },
            _ => !s.is_empty(),
        }
    }
}

/// Run `op` on the active Editable Mesh (one undo step).
pub fn run(state: &mut EditorState, op: Op) {
    let Some(e) = target(state) else { return };
    if !op.enabled(state) {
        return;
    }
    match op {
        Op::CenterPivot => return center_pivot(state, e),
        Op::FreezeTransform => return freeze_transform(state, e),
        Op::ExportModel => return export_model(state, e),
        Op::SelectAll => {
            if let Some(m) = mesh_of(state, e) {
                state.mesh_builder.selection = match state.mesh_builder.mode {
                    EditMode::Vertex => Selection::verts((0..m.positions.len() as u32).collect()),
                    EditMode::Edge => Selection::edges(m.edges()),
                    _ => Selection::faces((0..m.faces.len()).collect()),
                };
            }
            return;
        }
        _ => {}
    }
    let Some(mut m) = mesh_of(state, e) else {
        return;
    };
    let mode = state.mesh_builder.mode;
    let sel = state.mesh_builder.selection.clone();
    let set = state.mesh_builder.settings;
    let all_verts = super::selected_verts(state, &m);
    let what = match mode {
        EditMode::Vertex => "Vertices",
        EditMode::Edge => "Edges",
        _ => "Faces",
    };
    let (label, result, new_mode) = match op {
        Op::Extrude if mode == EditMode::Edge => {
            let edges = mops::extrude_edges(&mut m, &sel.edges, set.extrude);
            ("Extrude Edges", Selection::edges(edges), mode)
        }
        Op::Extrude => {
            let faces = mops::extrude_faces(&mut m, &sel.faces, set.extrude);
            ("Extrude Faces", Selection::faces(faces), mode)
        }
        Op::Inset => {
            let faces = mops::inset_faces(&mut m, &sel.faces, set.inset);
            ("Inset Faces", Selection::faces(faces), mode)
        }
        Op::Bevel => {
            let faces = mops::bevel_edges(&mut m, &sel.edges, set.bevel);
            ("Bevel Edges", Selection::faces(faces), EditMode::Face)
        }
        Op::Subdivide => {
            let faces = mops::subdivide_faces(&mut m, &sel.faces);
            ("Subdivide Faces", Selection::faces(faces), mode)
        }
        Op::Connect => {
            let edges = mops::connect_edges(&mut m, &sel.edges);
            ("Connect Edges", Selection::edges(edges), mode)
        }
        Op::Bridge => {
            let f = mops::bridge_edges(&mut m, sel.edges[0], sel.edges[1]);
            let Some(f) = f else {
                state
                    .console
                    .warn("Bridge needs two open (border) edges.".to_owned());
                return;
            };
            ("Bridge Edges", Selection::faces(vec![f]), EditMode::Face)
        }
        Op::Merge => {
            let v = mops::merge_vertices(&mut m, &all_verts);
            let label = match mode {
                EditMode::Vertex => "Merge Vertices",
                EditMode::Edge => "Merge Edges",
                _ => "Merge Faces",
            };
            (
                label,
                Selection::verts(v.into_iter().collect()),
                EditMode::Vertex,
            )
        }
        Op::Weld => {
            let removed = mops::weld_vertices(&mut m, &all_verts, set.weld);
            if removed == 0 {
                state
                    .console
                    .info("Weld: no selected vertices were close enough.".to_owned());
                return;
            }
            ("Weld Vertices", Selection::default(), mode)
        }
        Op::Detach => {
            let faces = mops::detach_faces(&mut m, &sel.faces);
            ("Detach Faces", Selection::faces(faces), mode)
        }
        Op::FlipNormals => {
            mops::flip_faces(&mut m, &sel.faces);
            ("Flip Normals", sel.clone(), mode)
        }
        Op::SetMaterial => {
            mops::set_material(&mut m, &sel.faces, set.material);
            ("Set Face Material", sel.clone(), mode)
        }
        Op::Delete => {
            let faces = match mode {
                EditMode::Face => sel.faces.clone(),
                _ => mops::faces_with_verts(&m, &all_verts, false),
            };
            mops::delete_faces(&mut m, &faces);
            let label = match what {
                "Vertices" => "Delete Vertices",
                "Edges" => "Delete Edges",
                _ => "Delete Faces",
            };
            (label, Selection::default(), mode)
        }
        Op::SelectAll | Op::CenterPivot | Op::FreezeTransform | Op::ExportModel => return,
    };
    state.record(label);
    let needed = m.material_count();
    let _ = state.scene.world.insert_one(e, m);
    if op == Op::SetMaterial {
        ensure_material_slots(state, e, needed);
    }
    state.mesh_builder.mode = new_mode;
    state.mesh_builder.selection = result;
}

/// Grow the Mesh Renderer's Materials list to `count` elements.
fn ensure_material_slots(state: &mut EditorState, e: Entity, count: usize) {
    if let Ok(mut r) = state.scene.world.get::<&mut MeshRenderer>(e) {
        while r.material_count() < count {
            r.extra_materials.push(None);
        }
    }
}

fn center_pivot(state: &mut EditorState, e: Entity) {
    let Some(mut m) = mesh_of(state, e) else {
        return;
    };
    state.record("Center Pivot");
    let c = mops::center_pivot(&mut m);
    let mut t = state.scene.transform(e);
    t.position += t.rotation * (t.scale * c);
    state.scene.set_transform(e, t);
    let _ = state.scene.world.insert_one(e, m);
}

fn freeze_transform(state: &mut EditorState, e: Entity) {
    let Some(mut m) = mesh_of(state, e) else {
        return;
    };
    state.record("Freeze Transform");
    let t = state.scene.transform(e);
    let matrix = t.local_matrix();
    let all: Vec<u32> = (0..m.positions.len() as u32).collect();
    mops::transform_vertices(&mut m, &all, &matrix);
    // A mirroring scale turns the faces inside out; flip them back.
    if matrix.determinant() < 0.0 {
        let faces: Vec<usize> = (0..m.faces.len()).collect();
        mops::flip_faces(&mut m, &faces);
    }
    state.scene.set_transform(e, Transform::default());
    let _ = state.scene.world.insert_one(e, m);
}

/// Write the mesh to `Assets/Models/<name>.obj` and swap the Editable Mesh for that model
/// (one child object per extra material, since a model mesh has one material).
fn export_model(state: &mut EditorState, e: Entity) {
    let Some(m) = mesh_of(state, e) else {
        return;
    };
    if m.faces.is_empty() {
        return;
    }
    let base = state.scene.name(e);
    let mut rel = format!("Assets/Models/{base}.obj");
    let mut n = 1;
    while state.assets.absolute(&rel).exists() {
        rel = format!("Assets/Models/{base} {n}.obj");
        n += 1;
    }
    let (text, materials) = misty_engine::mesh_builder::export::to_obj(&m, &base);
    let abs = state.assets.absolute(&rel);
    if let Some(dir) = abs.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&abs, text) {
        state.console.warn(format!("Export to Model failed: {err}"));
        return;
    }
    state.record(&format!("Export {base} to Model"));
    let renderer = state
        .scene
        .world
        .get::<&MeshRenderer>(e)
        .map(|r| (*r).clone())
        .unwrap_or_default();
    let element = |k: u32| renderer.material_at(k as usize).map(str::to_owned);
    state.scene.remove_by_name(e, "Editable Mesh");
    for (i, k) in materials.iter().enumerate() {
        let r = MeshRenderer {
            material: element(*k),
            extra_materials: Vec::new(),
            overrides: Default::default(),
            ..renderer.clone()
        };
        let filter = MeshFilter {
            mesh: Some(MeshRef::asset(&rel, i)),
        };
        if i == 0 {
            state.scene.add(e, filter);
            state.scene.add(e, r);
        } else {
            let name = format!("{base} ({})", element(*k).as_deref().unwrap_or("Default"));
            let child = state.scene.spawn(&name, Some(e));
            state.scene.add(child, filter);
            state.scene.add(child, r);
        }
    }
    state.mesh_builder.mode = EditMode::Object;
    state.refresh_assets();
    state.console.info(format!("Exported {base} to {rel}."));
}

/// Mesh Filter ▸ Convert to Editable Mesh: an editable copy of the object's mesh.
pub fn convert_to_editable(state: &mut EditorState, e: Entity) -> bool {
    let Some(mesh) = state
        .scene
        .world
        .get::<&MeshFilter>(e)
        .ok()
        .and_then(|f| f.mesh.clone())
    else {
        return false;
    };
    let Some(editable) = misty_engine::mesh_builder::convert::from_mesh_ref(&mesh) else {
        return false;
    };
    state.record("Convert to Editable Mesh");
    if state.scene.world.get::<&MeshRenderer>(e).is_err() {
        state.scene.add(e, MeshRenderer::default());
    }
    state.scene.add(e, editable);
    misty_engine::mesh_builder::sync(&state.scene);
    true
}
