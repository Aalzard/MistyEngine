//! Terrain editing: GameObject ▸ 3D Object ▸ Terrain, the brush tools (Raise or Lower, Set
//! Height, Smooth, Paint Texture) driven from the Scene view, the brush circle, and undo.
//!
//! Every edit of terrain data (a brush stroke, a Settings change) is one undo step: the data is
//! copied when the edit starts and pushed onto the undo stack, then saved, once the mouse is up.

use egui::{Rect, Response, Ui};
use glam::{Vec2, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Terrain, TerrainCollider, Transform};
use misty_engine::terrain::brush::{self, Brush};
use misty_engine::terrain::{TerrainData, auto_paint, store};

use crate::gizmos::Lines;
use crate::state::EditorState;

/// Inspector tool tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainTool {
    RaiseLower,
    SetHeight,
    Smooth,
    Paint,
    Settings,
}

impl TerrainTool {
    pub const ALL: [TerrainTool; 5] = [
        TerrainTool::RaiseLower,
        TerrainTool::SetHeight,
        TerrainTool::Smooth,
        TerrainTool::Paint,
        TerrainTool::Settings,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            TerrainTool::RaiseLower => "⛰",
            TerrainTool::SetHeight => "⊟",
            TerrainTool::Smooth => "≈",
            TerrainTool::Paint => "🖌",
            TerrainTool::Settings => "⚙",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TerrainTool::RaiseLower => "Raise or Lower Terrain",
            TerrainTool::SetHeight => "Set Height",
            TerrainTool::Smooth => "Smooth Height",
            TerrainTool::Paint => "Paint Texture",
            TerrainTool::Settings => "Terrain Settings",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            TerrainTool::RaiseLower => {
                "Left click to raise the terrain. Hold Shift and click to lower it."
            }
            TerrainTool::SetHeight => "Left click to move the terrain towards the target height.",
            TerrainTool::Smooth => "Left click to average out nearby heights.",
            TerrainTool::Paint => {
                "Select a layer, then left click to paint it. Auto Paint Rules and Generate paint the whole terrain."
            }
            TerrainTool::Settings => "Size, resolution and rendering of this terrain.",
        }
    }

    /// Undo History name of one stroke with this tool.
    pub fn step_name(self) -> &'static str {
        match self {
            TerrainTool::RaiseLower => "Raise or Lower Terrain",
            TerrainTool::SetHeight => "Set Terrain Height",
            TerrainTool::Smooth => "Smooth Terrain",
            TerrainTool::Paint => "Paint Terrain",
            TerrainTool::Settings => "Change Terrain Settings",
        }
    }

    /// Paints in the Scene view.
    pub fn is_brush(self) -> bool {
        self != TerrainTool::Settings
    }
}

/// Terrain data before an edit; the undo step keeps only what changed (see `undo_terrain`).
pub struct TerrainSnapshot {
    pub path: String,
    pub data: TerrainData,
}

/// An edit in progress: the data from before it started.
struct PendingEdit {
    label: String,
    before: TerrainSnapshot,
}

pub struct TerrainTools {
    pub tool: TerrainTool,
    /// Brush diameter in metres.
    pub size: f32,
    /// Opacity / strength 0..1.
    pub strength: f32,
    /// 0 = hard edge, 1 = soft.
    pub falloff: f32,
    /// Set Height target in metres.
    pub target_height: f32,
    /// Paint Texture layer.
    pub layer: usize,
    /// Paint Texture paints the Auto Paint result (layers and grass) instead of `layer`.
    pub generate_in_brush: bool,
    /// Brush centre on the selected terrain under the mouse.
    pub hover: Option<(Entity, Vec3)>,
    pending: Option<PendingEdit>,
    /// A stroke is being painted with the left mouse button.
    painting: bool,
}

impl Default for TerrainTools {
    fn default() -> Self {
        Self {
            tool: TerrainTool::RaiseLower,
            size: 25.0,
            strength: 0.5,
            falloff: 0.7,
            target_height: 10.0,
            layer: 0,
            generate_in_brush: false,
            hover: None,
            pending: None,
            painting: false,
        }
    }
}

/// The selected object's Terrain asset path when a brush tool should paint on it.
fn brush_target(state: &EditorState) -> Option<(Entity, String)> {
    if state.play.in_play_mode() || !state.terrain_tools.tool.is_brush() {
        return None;
    }
    let e = state.selection.active()?;
    let t = state.scene.world.get::<&Terrain>(e).ok()?;
    (t.enabled && state.scene.active_in_hierarchy(e)).then(|| (e, t.terrain_data.clone()))
}

/// A terrain is selected and a brush tool is active (the transform handles hide).
pub fn brush_active(state: &EditorState) -> bool {
    brush_target(state).is_some()
}

/// Start an undoable edit of `path` (no-op while one is already open).
pub fn begin_edit(state: &mut EditorState, path: &str, label: &str) {
    if state.terrain_tools.pending.is_some() {
        return;
    }
    if let Some((data, _)) = store::get(path) {
        state.terrain_tools.pending = Some(PendingEdit {
            label: label.to_owned(),
            before: TerrainSnapshot {
                path: path.to_owned(),
                data: (*data).clone(),
            },
        });
    }
}

/// Close the open edit once the mouse is released: one undo step, then save the asset.
pub fn end_frame(state: &mut EditorState, interacting: bool) {
    if interacting {
        return;
    }
    state.terrain_tools.painting = false;
    let Some(edit) = state.terrain_tools.pending.take() else {
        return;
    };
    let path = edit.before.path.clone();
    let changed = store::get(&path).is_some_and(|(d, _)| *d != edit.before.data);
    if !changed {
        return;
    }
    state
        .undo
        .push_terrain(&edit.label, &state.scene, &state.selection, edit.before);
    if let Err(e) = store::save(&path) {
        state.console.warn(format!("Could not save {path}: {e}"));
    }
}

/// Scene view: track the brush under the mouse and paint while the left button is held.
/// Returns true when the view's left click belongs to the brush (no picking, no handles).
pub fn scene_view(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    response: &Response,
) -> bool {
    let Some((e, path)) = brush_target(state) else {
        state.terrain_tools.hover = None;
        return false;
    };
    let pos = state.scene.world_position(e);
    let hover = response
        .hover_pos()
        .filter(|_| response.hovered() || state.terrain_tools.painting)
        .and_then(|at| {
            let (origin, dir) = crate::picking::ray(camera, rect, at);
            let (data, _) = store::get(&path)?;
            misty_engine::terrain::raycast_data(&data, origin - pos, dir).map(|p| p + pos)
        });
    state.terrain_tools.hover = hover.map(|p| (e, p));

    let (pressed, down, alt, shift, dt) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.alt,
            i.modifiers.shift,
            i.stable_dt.min(0.1),
        )
    });
    if pressed && response.hovered() && !alt && hover.is_some() {
        state.terrain_tools.painting = true;
        let label = state.terrain_tools.tool.step_name();
        begin_edit(state, &path, label);
    }
    if !down {
        state.terrain_tools.painting = false;
    }
    if state.terrain_tools.painting
        && let Some(p) = hover
    {
        apply(state, &path, p - pos, shift, dt);
        ui.ctx().request_repaint();
    }
    hover.is_some() || state.terrain_tools.painting
}

/// One frame of the current tool at local point `at`.
fn apply(state: &mut EditorState, path: &str, at: Vec3, shift: bool, dt: f32) {
    let t = &state.terrain_tools;
    let b = Brush {
        center: Vec2::new(at.x, at.z),
        size: t.size,
        strength: t.strength,
        falloff: t.falloff,
    };
    let (tool, target, layer, generated) = (t.tool, t.target_height, t.layer, t.generate_in_brush);
    misty_engine::profile_scope!("Terrain.Brush");
    // Only the brush's square is re-uploaded by the renderer.
    let Some(region) = store::get(path).map(|(d, _)| b.region(&d)) else {
        return;
    };
    store::modify_region(path, region, |d| match tool {
        TerrainTool::RaiseLower => brush::raise(d, &b, dt, shift),
        TerrainTool::SetHeight => brush::set_height(d, &b, dt, target),
        TerrainTool::Smooth => brush::smooth(d, &b, dt),
        TerrainTool::Paint if generated => auto_paint::generate_in_brush(d, &b, dt),
        TerrainTool::Paint => brush::paint(d, &b, dt, layer),
        TerrainTool::Settings => {}
    });
}

/// A short stroke of `tool` across the middle of the selected terrain, as one undo step
/// (tutorial demos and tests).
pub fn demo_stroke(state: &mut EditorState, tool: TerrainTool) {
    state.terrain_tools.tool = tool;
    let Some((_, path)) = brush_target(state) else {
        return;
    };
    let Some(size) = store::get(&path).map(|(d, _)| d.size) else {
        return;
    };
    begin_edit(state, &path, tool.step_name());
    for i in 0..12 {
        let at = Vec3::new(size.x * (0.35 + i as f32 * 0.025), 0.0, size.z * 0.5);
        apply(state, &path, at, false, 0.1);
    }
    end_frame(state, false);
}

/// Brush circle following the terrain under the mouse.
pub fn brush_lines(lines: &mut Lines, state: &EditorState) {
    let Some((e, centre)) = state.terrain_tools.hover else {
        return;
    };
    let Some(data) = misty_engine::terrain::data_of(&state.scene, e) else {
        return;
    };
    let pos = state.scene.world_position(e);
    let r = state.terrain_tools.size * 0.5;
    let color = [0.35, 0.65, 1.0, 1.0];
    let at = |a: f32, r: f32| {
        let p = centre + Vec3::new(a.cos(), 0.0, a.sin()) * r;
        let l = p - pos;
        Vec3::new(p.x, pos.y + data.sample_height(l.x, l.z) + 0.05, p.z)
    };
    const SEGMENTS: usize = 64;
    // Rim, plus a fainter inner ring where the falloff starts.
    let inner = r * (1.0 - state.terrain_tools.falloff.clamp(0.0, 1.0));
    for (radius, c) in [(r, color), (inner, [color[0], color[1], color[2], 0.45])] {
        if radius < 0.05 {
            continue;
        }
        for i in 0..SEGMENTS {
            let a0 = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            lines.line(at(a0, radius), at(a1, radius), c);
        }
    }
}

/// GameObject ▸ 3D Object ▸ Terrain: a new terrain data asset in `Assets` and an object
/// with Terrain + Terrain Collider, centred on the Scene view pivot.
pub fn create(s: &mut EditorState, parent: Option<Entity>) {
    let parent = parent.filter(|p| s.scene.contains(*p));
    let dir = s.project.root.join("Assets");
    let file = crate::asset_ops::unique_path(&dir, "New Terrain", misty_engine::terrain::EXTENSION);
    let data = TerrainData::default();
    if let Err(e) = data.save(&file) {
        s.console
            .warn(format!("Could not create {}: {e}", file.display()));
        return;
    }
    let rel = file
        .strip_prefix(&s.project.root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| misty_engine::assets::relative(&file));
    let size = data.size;
    store::insert(&rel, data);
    s.record("Create Terrain");
    let e = s.scene.spawn("Terrain", parent);
    let mut t = Transform::default();
    if parent.is_none() {
        t.position = (s.scene_view.camera.pivot() - Vec3::new(size.x, 0.0, size.z) * 0.5).round();
        t.position.y = 0.0;
    }
    s.scene.set_transform(e, t);
    s.scene.add(
        e,
        Terrain {
            terrain_data: rel.clone(),
            ..Default::default()
        },
    );
    s.scene.add(e, TerrainCollider::default());
    s.select(e);
    s.refresh_assets();
    s.console.info(format!("Created 'Terrain' with {rel}."));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> EditorState {
        let root =
            std::env::temp_dir().join(format!("misty_terrain_{name}_{}", std::process::id()));
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    #[test]
    fn create_makes_an_asset_and_a_collider() {
        let mut s = state("create");
        create(&mut s, None);
        let e = s.selection.active().unwrap();
        let t = (*s.scene.world.get::<&Terrain>(e).unwrap()).clone();
        assert_eq!(t.terrain_data, "Assets/New Terrain.terrain");
        assert!(s.project.root.join(&t.terrain_data).exists());
        assert!(s.scene.world.get::<&TerrainCollider>(e).is_ok());
        assert_eq!(s.undo.undo_label(), Some("Create Terrain"));
        // A second terrain gets its own asset.
        create(&mut s, None);
        let e2 = s.selection.active().unwrap();
        let t2 = s
            .scene
            .world
            .get::<&Terrain>(e2)
            .unwrap()
            .terrain_data
            .clone();
        assert_eq!(t2, "Assets/New Terrain 1.terrain");
        let _ = std::fs::remove_dir_all(&s.project.root);
    }

    #[test]
    fn a_settings_edit_is_one_undo_step() {
        let dir = std::env::temp_dir().join(format!("misty_terrain_undo_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("U.terrain").to_string_lossy().into_owned();
        store::insert(&path, TerrainData::new(33, Vec3::splat(10.0), 32));
        let mut s = state("undo");
        // Three frames of dragging, then release.
        for i in 0..3 {
            begin_edit(&mut s, &path, "Change Terrain Settings");
            store::modify(&path, |d| d.set_size(Vec3::splat(11.0 + i as f32)));
            end_frame(&mut s, true);
        }
        end_frame(&mut s, false);
        assert_eq!(s.undo.undo_label(), Some("Change Terrain Settings"));
        assert!(
            TerrainData::load(std::path::Path::new(&path))
                .unwrap()
                .size
                .x
                == 13.0,
            "saved"
        );
        s.undo_step();
        assert_eq!(store::get(&path).unwrap().0.size.x, 10.0);
        s.redo_step();
        assert_eq!(store::get(&path).unwrap().0.size.x, 13.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A small stroke on a big terrain keeps only a few tiles, and undo/redo restore it.
    #[test]
    fn brush_stroke_undo_stores_only_changed_tiles() {
        let dir = std::env::temp_dir().join(format!("misty_terrain_tiles_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("T.terrain").to_string_lossy().into_owned();
        let flat = TerrainData::new(513, Vec3::new(200.0, 50.0, 200.0), 256);
        store::insert(&path, flat.clone());
        let mut s = state("tiles");
        begin_edit(&mut s, &path, TerrainTool::RaiseLower.step_name());
        store::modify(&path, |d| {
            for z in 100..140 {
                for x in 200..240 {
                    d.heights[z * 513 + x] = 0.5;
                }
            }
        });
        end_frame(&mut s, false);
        assert_eq!(s.undo.undo_label(), Some("Raise or Lower Terrain"));
        // A 40×40 patch touches at most 4 tiles (16 KB), far below the 1 MB heightmap.
        assert!(s.undo.bytes() < 200 * 1024, "{} bytes", s.undo.bytes());
        let edited = (*store::get(&path).unwrap().0).clone();
        s.undo_step();
        assert_eq!(*store::get(&path).unwrap().0, flat);
        s.redo_step();
        assert_eq!(*store::get(&path).unwrap().0, edited);
        let _ = std::fs::remove_dir_all(dir);
    }
}
