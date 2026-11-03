//! Navigation gizmos: the selected agent's cylinder, obstacle shapes, the translucent blue nav
//! mesh while the Navigation window is open, and the selected agent's path in Play mode.

use glam::{Mat4, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{NavMeshAgent, NavMeshObstacle, ObstacleShape};
use misty_engine::render::LineVertex;
use misty_engine::scene::Scene;

use super::Lines;
use crate::state::EditorState;

const AGENT: [f32; 4] = [0.55, 0.85, 1.0, 0.9];
const OBSTACLE: [f32; 4] = [0.95, 0.55, 0.3, 0.9];
const MESH_FILL: [f32; 4] = [0.3, 0.62, 1.0, 0.38];
const MESH_EDGE: [f32; 4] = [0.45, 0.75, 1.0, 0.55];
const PATH: [f32; 4] = [1.0, 0.85, 0.2, 1.0];
/// The overlay floats a little above the surface so it doesn't fight with it.
const LIFT: f32 = 0.03;

impl Lines {
    /// Agent cylinder (from the surface up, ignores rotation) and obstacle shapes of `e`.
    pub(super) fn nav_shapes(&mut self, scene: &Scene, e: Entity) {
        if let Ok(a) = scene.world.get::<&NavMeshAgent>(e) {
            let base = scene.world_position(e) - Vec3::Y * a.base_offset;
            let m = Mat4::from_translation(base);
            self.circle(&m, Vec3::ZERO, a.radius, 1, AGENT);
            self.circle(&m, Vec3::Y * a.height, a.radius, 1, AGENT);
            for d in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                let p = base + d * a.radius;
                self.line(p, p + Vec3::Y * a.height, AGENT);
            }
        }
        if let Ok(o) = scene.world.get::<&NavMeshObstacle>(e) {
            let m = scene.world_matrix(e);
            match o.shape {
                ObstacleShape::Box => self.wire_box(&m, o.center, o.size * 0.5, OBSTACLE),
                ObstacleShape::Capsule => {
                    let half = (o.height * 0.5 - o.radius).max(0.0);
                    let m = m * Mat4::from_translation(o.center);
                    self.capsule(&m, 1, o.radius, half, OBSTACLE);
                }
            }
        }
    }
}

/// Nav mesh triangles (returned, for the translucent overlay pass) and edges + agent paths
/// (added to `lines`).
pub fn nav_overlay(state: &EditorState, ctx: &egui::Context, lines: &mut Lines) -> Vec<LineVertex> {
    let mut tris = Vec::new();
    let open = crate::panels::navigation::is_open(state, ctx);
    if open
        && state.navigation.show_mesh
        && let Some(mesh) = crate::panels::navigation::current_mesh(state)
    {
        let lift = Vec3::Y * LIFT;
        for t in mesh.triangles() {
            for p in t {
                tris.push(LineVertex {
                    position: (p + lift).into(),
                    color: MESH_FILL,
                });
            }
        }
        for poly in &mesh.polys {
            let v = &poly.vertices;
            for i in 0..v.len() {
                lines.line(v[i] + lift, v[(i + 1) % v.len()] + lift, MESH_EDGE);
            }
        }
    }
    // Paths of selected agents while playing.
    if let Some(rt) = &state.runtime {
        for &e in state.selection.list() {
            if let Some(path) = rt.nav_path(e) {
                for w in path.windows(2) {
                    lines.line(w[0] + Vec3::Y * LIFT, w[1] + Vec3::Y * LIFT, PATH);
                }
                for c in &path[1..] {
                    lines.line(*c, *c + Vec3::Y * 0.4, PATH);
                }
            }
        }
    }
    tris
}
