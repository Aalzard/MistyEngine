//! Mesh Builder: modelling an object's Editable Mesh in the Scene view.
//!
//! Object mode is the normal editor. Vertex / Edge / Face modes (keys 1–4, or the floating
//! strip under the tools row) select elements of the active object's Editable Mesh by click,
//! Shift+click (add) and box drag; the Move / Rotate / Scale handles then act on them, and the
//! operations (Extrude, Inset, Bevel, …) run on the selection. Every operation is one undo step.

mod ops;
mod select;
mod shape_window;
mod strip;

use egui::Pos2;
use glam::{Mat3, Mat4, Quat, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::{EditableMesh, Selection};

use crate::state::EditorState;

pub use ops::{Op, convert_to_editable, run};
pub use select::{lines, paint as paint_vertices, scene_view};
#[cfg(test)]
pub use shape_window::create_shape;
pub use shape_window::{NewShape, open_new_shape, window as shape_window};
pub use strip::show as strip;

/// What clicks in the Scene view select.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditMode {
    #[default]
    Object,
    Vertex,
    Edge,
    Face,
}

impl EditMode {
    pub const ALL: [EditMode; 4] = [Self::Object, Self::Vertex, Self::Edge, Self::Face];

    pub fn label(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Vertex => "Vertex",
            Self::Edge => "Edge",
            Self::Face => "Face",
        }
    }

    /// Keyboard shortcut (plain 1–4).
    pub fn key(self) -> egui::Key {
        match self {
            Self::Object => egui::Key::Num1,
            Self::Vertex => egui::Key::Num2,
            Self::Edge => egui::Key::Num3,
            Self::Face => egui::Key::Num4,
        }
    }

    pub fn tooltip(self) -> String {
        let n = Self::ALL.iter().position(|m| *m == self).unwrap_or(0) + 1;
        match self {
            Self::Object => format!("Object selection ({n})"),
            other => format!(
                "{} selection ({n}): click, Shift+click to add, drag a box",
                other.label()
            ),
        }
    }
}

/// Amounts used by the operations (Inspector ▸ Editable Mesh ▸ Operations).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpSettings {
    pub extrude: f32,
    pub inset: f32,
    pub bevel: f32,
    pub weld: f32,
    /// Materials element Set Material gives faces.
    pub material: u32,
}

impl Default for OpSettings {
    fn default() -> Self {
        Self {
            extrude: 0.5,
            inset: 0.1,
            bevel: 0.1,
            weld: 0.01,
            material: 0,
        }
    }
}

/// Mesh Builder editor state (lives on [`EditorState`]).
#[derive(Default)]
pub struct MeshBuilder {
    pub mode: EditMode,
    /// The object whose elements `selection` refers to.
    pub target: Option<Entity>,
    pub selection: Selection,
    pub settings: OpSettings,
    /// Screen point where a box selection started.
    pub(crate) box_start: Option<Pos2>,
    /// Mesh and moved vertices when a handle drag started.
    pub(crate) drag: Option<(Entity, EditableMesh, Vec<u32>)>,
    pub new_shape: Option<NewShape>,
}

/// Does `e` have an Editable Mesh?
pub fn has_editable(state: &EditorState, e: Entity) -> bool {
    state.scene.world.get::<&EditableMesh>(e).is_ok()
}

/// The active object, if it has an Editable Mesh.
pub fn target(state: &EditorState) -> Option<Entity> {
    let e = state.selection.active()?;
    has_editable(state, e).then_some(e)
}

/// The object being edited in an element mode (Vertex, Edge or Face).
pub fn editing(state: &EditorState) -> Option<Entity> {
    if state.mesh_builder.mode == EditMode::Object || state.play.in_play_mode() {
        return None;
    }
    target(state)
}

/// Keep the element selection valid: cleared when the edited object changes, trimmed after
/// undo or topology changes. Call once per frame.
pub fn sync_frame(state: &mut EditorState) {
    let t = target(state);
    let mb = &mut state.mesh_builder;
    if t != mb.target {
        mb.target = t;
        mb.selection = Selection::default();
    }
    if let Some(e) = t
        && !mb.selection.is_empty()
        && let Ok(m) = state.scene.world.get::<&EditableMesh>(e)
    {
        mb.selection.retain_valid(&m);
    }
}

/// Copy of the target's mesh.
pub fn mesh_of(state: &EditorState, e: Entity) -> Option<EditableMesh> {
    state
        .scene
        .world
        .get::<&EditableMesh>(e)
        .ok()
        .map(|m| (*m).clone())
}

/// Vertices the current mode's selection moves.
pub fn selected_verts(state: &EditorState, m: &EditableMesh) -> Vec<u32> {
    let s = &state.mesh_builder.selection;
    let only = match state.mesh_builder.mode {
        EditMode::Vertex => Selection::verts(s.verts.clone()),
        EditMode::Edge => Selection::edges(s.edges.clone()),
        EditMode::Face => Selection::faces(s.faces.clone()),
        EditMode::Object => return Vec::new(),
    };
    only.all_verts(m)
}

/// Handle placement while editing: `None` = not editing (normal object handles);
/// `Some(None)` = editing with nothing selected (no handles); otherwise the centre of the
/// selected vertices and the handle rotation.
pub fn handle_pivot(state: &EditorState, local: bool) -> Option<Option<(Vec3, Quat)>> {
    let e = editing(state)?;
    let m = mesh_of(state, e)?;
    let verts = selected_verts(state, &m);
    if verts.is_empty() {
        return Some(None);
    }
    let world = state.scene.world_matrix(e);
    let center = verts.iter().map(|v| m.position(*v)).sum::<Vec3>() / verts.len() as f32;
    let rot = if local {
        state.scene.world_rotation(e)
    } else {
        Quat::IDENTITY
    };
    Some(Some((world.transform_point3(center), rot)))
}

/// A handle drag starts: remember the mesh so each frame applies the whole drag to it.
pub fn begin_drag(state: &mut EditorState) {
    state.mesh_builder.drag = editing(state).and_then(|e| {
        let m = mesh_of(state, e)?;
        let verts = selected_verts(state, &m);
        Some((e, m, verts))
    });
}

/// Apply a handle drag (a world-space transform of the selection since the drag started).
/// Returns false when no element drag is active (the handles then move objects).
pub fn drag(state: &mut EditorState, world_delta: Mat4) -> bool {
    let Some((e, start, verts)) = state.mesh_builder.drag.clone() else {
        return false;
    };
    if editing(state) != Some(e) {
        return false;
    }
    let w = state.scene.world_matrix(e);
    let local = w.inverse() * world_delta * w;
    let mut m = start;
    misty_engine::mesh_builder::ops::transform_vertices(&mut m, &verts, &local);
    let _ = state.scene.world.insert_one(e, m);
    true
}

/// Transform about `origin` in the frame `axes` (scale handles).
pub fn scale_about(origin: Vec3, axes: [Vec3; 3], factor: Vec3) -> Mat4 {
    let r = Mat4::from_mat3(Mat3::from_cols(axes[0], axes[1], axes[2]));
    Mat4::from_translation(origin)
        * r
        * Mat4::from_scale(factor)
        * r.inverse()
        * Mat4::from_translation(-origin)
}

/// "Vertices" / "Edges" / "Faces": step names for handle drags while editing.
pub fn drag_label(state: &EditorState) -> Option<&'static str> {
    editing(state)?;
    state.mesh_builder.drag.as_ref()?;
    Some(match state.mesh_builder.mode {
        EditMode::Vertex => "Vertices",
        EditMode::Edge => "Edges",
        _ => "Faces",
    })
}

/// Mode keys (1–4) and operation shortcuts; consumed before the global shortcuts so Delete
/// removes elements rather than the object. Skipped while typing.
pub fn shortcuts(ctx: &egui::Context, state: &mut EditorState, typing: bool) {
    if typing || target(state).is_none() || state.play.in_play_mode() {
        return;
    }
    for mode in EditMode::ALL {
        if ctx.input(|i| i.modifiers.is_none() && i.key_pressed(mode.key())) {
            state.mesh_builder.mode = mode;
        }
    }
    if editing(state).is_none() {
        return;
    }
    // Most modifiers first: Alt+B would also match Alt+Shift+B.
    let mut ops: Vec<(Op, egui::KeyboardShortcut)> = Op::ALL
        .into_iter()
        .filter_map(|op| Some((op, op.shortcut()?)))
        .collect();
    ops.sort_by_key(|(_, s)| std::cmp::Reverse(s.modifiers.shift as u8 + s.modifiers.alt as u8));
    for (op, sc) in ops {
        if op.enabled(state) && ctx.input_mut(|i| i.consume_shortcut(&sc)) {
            run(state, op);
        }
    }
    // Delete / Backspace remove the selected elements (not the object).
    if !state.mesh_builder.selection.is_empty()
        && ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        })
    {
        run(state, Op::Delete);
    }
}

/// Screenshot hook: "face" / "edge" / "vertex", optionally ":i,j,…" element indices to select
/// (edges by index into the mesh's sorted edge list).
pub fn apply_env(state: &mut EditorState, value: &str) {
    let (mode, list) = value.split_once(':').unwrap_or((value, ""));
    let Some(mode) = EditMode::ALL
        .into_iter()
        .find(|m| m.label().eq_ignore_ascii_case(mode.trim()))
    else {
        return;
    };
    state.mesh_builder.mode = mode;
    sync_frame(state);
    let ids: Vec<usize> = list
        .split(',')
        .filter_map(|n| n.trim().parse().ok())
        .collect();
    let Some(m) = target(state).and_then(|e| mesh_of(state, e)) else {
        return;
    };
    let sel = &mut state.mesh_builder.selection;
    match mode {
        EditMode::Vertex => sel.verts = ids.iter().map(|i| *i as u32).collect(),
        EditMode::Edge => {
            let edges = m.edges();
            sel.edges = ids.iter().filter_map(|i| edges.get(*i).copied()).collect();
        }
        EditMode::Face => sel.faces = ids,
        EditMode::Object => {}
    }
    sel.retain_valid(&m);
}

#[cfg(test)]
mod tests;
