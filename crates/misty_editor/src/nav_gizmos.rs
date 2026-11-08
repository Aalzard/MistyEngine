//! Navigation gizmos: the selected agent's cylinder, obstacle shapes (carving ones dashed red),
//! NavMesh Link arcs, Modifier Volume and Surface volume boxes; the translucent nav meshes
//! coloured by area (tile borders on request) while the Navigation window is open or a
//! navigation component is selected — in Play mode the live meshes with carved holes — and
//! agent paths in Play mode.

use glam::{Mat4, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::navigation::{
    CollectObjects, NavMesh, NavMeshAgent, NavMeshLink, NavMeshModifierVolume, NavMeshObstacle,
    NavMeshSurface, ObstacleShape, area_color,
};
use misty_engine::render::LineVertex;
use misty_engine::scene::Scene;

use super::Lines;
use crate::state::EditorState;

const AGENT: [f32; 4] = [0.55, 0.85, 1.0, 0.9];
const OBSTACLE: [f32; 4] = [0.95, 0.55, 0.3, 0.9];
const CARVE: [f32; 4] = [1.0, 0.3, 0.25, 0.95];
const VOLUME: [f32; 4] = [0.4, 1.0, 0.6, 0.9];
const LINK: [f32; 4] = [1.0, 0.85, 0.3, 1.0];
const TILE: [f32; 4] = [0.15, 0.2, 0.3, 0.7];
const PATH: [f32; 4] = [1.0, 0.85, 0.2, 1.0];
/// Fill and edge opacity of the overlay.
const FILL_ALPHA: f32 = 0.4;
const EDGE_ALPHA: f32 = 0.55;
/// The overlay floats a little above the surface so it doesn't fight with it.
const LIFT: f32 = 0.03;

fn with_alpha(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

impl Lines {
    /// Agent cylinder (from the surface up, ignores rotation), obstacle shapes, links and
    /// volumes of `e`.
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
            let color = if o.carve { CARVE } else { OBSTACLE };
            match o.shape {
                ObstacleShape::Box => self.wire_box(&m, o.center, o.size * 0.5, color),
                ObstacleShape::Capsule => {
                    let half = (o.height * 0.5 - o.radius).max(0.0);
                    let m = m * Mat4::from_translation(o.center);
                    self.capsule(&m, 1, o.radius, half, color);
                }
            }
        }
        if let Ok(v) = scene.world.get::<&NavMeshModifierVolume>(e) {
            let m = scene.world_matrix(e);
            let c = with_alpha(area_color(v.area), 0.95);
            self.wire_box(&m, v.center, v.size * 0.5, c);
            self.wire_box(&m, v.center, v.size * 0.5 * 0.98, VOLUME);
        }
        if let Ok(s) = scene.world.get::<&NavMeshSurface>(e)
            && s.collect == CollectObjects::Volume
        {
            self.wire_box(&scene.world_matrix(e), s.center, s.size * 0.5, VOLUME);
        }
        if let Ok(l) = scene.world.get::<&NavMeshLink>(e) {
            let m = scene.world_matrix(e);
            self.link_arc(
                m.transform_point3(l.start),
                m.transform_point3(l.end),
                l.width,
            );
        }
    }

    /// An arc from `a` to `b` with end markers (and the width as bars across).
    fn link_arc(&mut self, a: Vec3, b: Vec3, width: f32) {
        let len = a.distance(b).max(0.01);
        let h = (len * 0.25).max(0.3);
        let mut prev = a;
        for i in 1..=16 {
            let t = i as f32 / 16.0;
            let p = a.lerp(b, t) + Vec3::Y * 4.0 * h * t * (1.0 - t);
            self.line(prev, p, LINK);
            prev = p;
        }
        let side = (b - a).cross(Vec3::Y).normalize_or_zero() * (width * 0.5).max(0.15);
        for p in [a, b] {
            self.line(p - side, p + side, LINK);
            self.circle(&Mat4::from_translation(p), Vec3::ZERO, 0.12, 1, LINK);
        }
    }
}

/// Add `mesh`'s triangles (area colours) to `tris` and its edges / tile borders to `lines`.
fn draw_mesh(mesh: &NavMesh, tiles: bool, tris: &mut Vec<LineVertex>, lines: &mut Lines) {
    let lift = Vec3::Y * LIFT;
    for (_, poly) in mesh.polys() {
        let fill = with_alpha(area_color(poly.area), FILL_ALPHA);
        for t in poly.detail_triangles() {
            for p in t {
                tris.push(LineVertex {
                    position: (p + lift).into(),
                    color: fill,
                });
            }
        }
        let edge = with_alpha(
            area_color(poly.area).map(|c| (c * 1.2).min(1.0)),
            EDGE_ALPHA,
        );
        let v = &poly.vertices;
        for i in 0..v.len() {
            lines.line(v[i] + lift, v[(i + 1) % v.len()] + lift, edge);
        }
    }
    if tiles && mesh.settings.tile_size > 0 {
        for t in mesh.tiles() {
            if t.polys.is_empty() {
                continue;
            }
            let (lo, hi) = mesh.settings.tile_bounds(t.x, t.z);
            let y = t
                .polys
                .iter()
                .flat_map(|p| p.vertices.iter().map(|v| v.y))
                .fold(f32::MIN, f32::max)
                + LIFT * 2.0;
            let c = [
                Vec3::new(lo.x, y, lo.y),
                Vec3::new(hi.x, y, lo.y),
                Vec3::new(hi.x, y, hi.y),
                Vec3::new(lo.x, y, hi.y),
            ];
            for i in 0..4 {
                lines.line(c[i], c[(i + 1) % 4], TILE);
            }
        }
    }
    for l in &mesh.off_links {
        lines.link_arc(l.start_on_mesh, l.end_on_mesh, l.width);
    }
}

/// Whether a navigation component is selected (the overlay shows then too).
fn nav_selected(state: &EditorState) -> bool {
    state.selection.list().iter().any(|&e| {
        let w = &state.scene.world;
        w.get::<&NavMeshSurface>(e).is_ok()
            || w.get::<&NavMeshModifierVolume>(e).is_ok()
            || w.get::<&NavMeshLink>(e).is_ok()
    })
}

/// Nav mesh triangles (returned, for the translucent overlay pass) and edges, tiles, links and
/// agent paths (added to `lines`).
pub fn nav_overlay(state: &EditorState, ctx: &egui::Context, lines: &mut Lines) -> Vec<LineVertex> {
    let mut tris = Vec::new();
    let open = crate::panels::navigation::is_open(state, ctx) || nav_selected(state);
    let tiles = state.navigation.show_tiles;
    if open && state.navigation.show_mesh {
        let mut live = false;
        if let Some(rt) = &state.runtime {
            rt.with_navigation(|n| {
                for (m, _) in n.meshes() {
                    live = true;
                    draw_mesh(m, tiles, &mut tris, lines);
                }
            });
        }
        if !live {
            if let Some(mesh) = crate::panels::navigation::current_mesh(state) {
                draw_mesh(&mesh, tiles, &mut tris, lines);
            }
            for (_, mesh) in crate::panels::navigation::surface_meshes(state) {
                draw_mesh(&mesh, tiles, &mut tris, lines);
            }
        }
    }
    // Paths of selected (or all) agents while playing.
    if let Some(rt) = &state.runtime {
        let all = state.navigation.show_all_paths;
        let agents: Vec<Entity> = if all {
            rt.with_navigation(|n| n.agent_entities())
        } else {
            state.selection.list().to_vec()
        };
        for e in agents {
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
