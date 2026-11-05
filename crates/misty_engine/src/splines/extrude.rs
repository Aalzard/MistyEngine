//! Spline Extrude: a tube (Circle profile) or flat strip (Road profile) along every spline of
//! a container, written into the object's Mesh Filter as a runtime mesh.
//!
//! The mesh is in the container's local space. Its key is a hash of the inputs
//! ("runtime://spline-extrude/<hash>#0"), so a saved scene names the same mesh it rebuilds on
//! load, and an unchanged extrude costs one hash per frame.

use std::sync::Mutex;

use glam::{Vec2, Vec3};
use hecs::Entity;

use super::components::{ExtrudeProfile, SplineContainer, SplineExtrude, container_object};
use super::{Fnv, SplinePath};
use crate::models::runtime;
use crate::render::mesh::{MeshData, Vertex};
use crate::scene::{MeshFilter, MeshRef, Scene};

/// Runtime mesh name prefix of extruded meshes.
const NAME: &str = "spline-extrude";
/// Most rings one spline gets.
const MAX_RINGS: usize = 8192;

/// Replaced mesh keys waiting to be dropped (dropping clears GPU mesh caches, so it is batched).
static GARBAGE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Rings along `path` for the extrude's range: (distance, t) pairs.
fn rings(path: &SplinePath, ex: &SplineExtrude) -> Vec<(f32, f32)> {
    let len = path.length;
    if len <= 1e-6 {
        return Vec::new();
    }
    let a = ex.range[0].clamp(0.0, 1.0);
    let b = ex.range[1].clamp(0.0, 1.0);
    let (a, b) = (a.min(b), a.max(b));
    let span = (b - a) * len;
    if span <= 1e-6 {
        return Vec::new();
    }
    let segs = ((span * ex.segments_per_unit.max(0.01)).ceil() as usize)
        .max(path.curves.len())
        .clamp(1, MAX_RINGS);
    (0..=segs)
        .map(|i| {
            let d = a * len + span * i as f32 / segs as f32;
            (d, path.t_at_distance(d))
        })
        .collect()
}

fn vertex(p: Vec3, n: Vec3, uv: Vec2) -> Vertex {
    Vertex {
        position: p.into(),
        normal: n.into(),
        uv: uv.into(),
        tangent: [1.0, 0.0, 0.0, 1.0],
    }
}

/// Append the mesh of one spline.
fn extrude_path(mesh: &mut MeshData, path: &SplinePath, ex: &SplineExtrude) {
    let rings = rings(path, ex);
    if rings.len() < 2 {
        return;
    }
    let full_loop = path.closed && ex.range[0] <= 0.0 && ex.range[1] >= 1.0;
    let frame = |t: f32| {
        let tan = path.tangent(t);
        let up = path.up(t);
        (path.position(t), tan, up, up.cross(tan))
    };
    let base = mesh.vertices.len() as u32;
    match ex.profile {
        ExtrudeProfile::Circle => {
            let sides = ex.sides.clamp(3, 256) as usize;
            let r = ex.radius.max(0.0);
            let stride = sides as u32 + 1;
            for (d, t) in &rings {
                let (p, _, up, right) = frame(*t);
                for k in 0..=sides {
                    let a = k as f32 / sides as f32 * std::f32::consts::TAU;
                    let n = right * a.cos() + up * a.sin();
                    let uv = Vec2::new(k as f32 / sides as f32, d * ex.uv_tiling);
                    mesh.vertices.push(vertex(p + n * r, n, uv));
                }
            }
            for i in 0..rings.len() as u32 - 1 {
                for k in 0..sides as u32 {
                    let a = base + i * stride + k;
                    let (b, c) = (a + 1, a + stride);
                    let d = c + 1;
                    mesh.indices.extend([a, b, c, b, d, c]);
                }
            }
            if ex.cap_ends && !full_loop {
                for (end, (_, t)) in [(false, rings[0]), (true, rings[rings.len() - 1])] {
                    let (p, tan, up, right) = frame(t);
                    let n = if end { tan } else { -tan };
                    let center = mesh.vertices.len() as u32;
                    mesh.vertices.push(vertex(p, n, Vec2::splat(0.5)));
                    for k in 0..=sides {
                        let a = k as f32 / sides as f32 * std::f32::consts::TAU;
                        let (s, c) = a.sin_cos();
                        let at = p + (right * c + up * s) * r;
                        let uv = Vec2::new(0.5 + c * 0.5, 0.5 - s * 0.5);
                        mesh.vertices.push(vertex(at, n, uv));
                    }
                    for k in 0..sides as u32 {
                        let (v0, v1) = (center + 1 + k, center + 2 + k);
                        if end {
                            mesh.indices.extend([center, v0, v1]);
                        } else {
                            mesh.indices.extend([center, v1, v0]);
                        }
                    }
                }
            }
        }
        ExtrudeProfile::Road => {
            let half = ex.width.max(0.0) * 0.5;
            for (d, t) in &rings {
                let (p, _, up, right) = frame(*t);
                let v = d * ex.uv_tiling;
                mesh.vertices
                    .push(vertex(p - right * half, up, Vec2::new(0.0, v)));
                mesh.vertices
                    .push(vertex(p + right * half, up, Vec2::new(1.0, v)));
            }
            for i in 0..rings.len() as u32 - 1 {
                let a = base + i * 2;
                let (b, c, d) = (a + 1, a + 2, a + 3);
                mesh.indices.extend([a, c, b, b, c, d]);
            }
        }
    }
}

/// The extruded mesh of every spline in `container` (container-local space).
pub fn build_mesh(container: &SplineContainer, ex: &SplineExtrude) -> MeshData {
    let mut mesh = MeshData::default();
    for s in &container.splines {
        extrude_path(&mut mesh, &s.path(), ex);
    }
    if !mesh.indices.is_empty() {
        mesh.compute_tangents();
    }
    mesh
}

/// Hash of everything the mesh depends on.
fn inputs_hash(container: &SplineContainer, ex: &SplineExtrude) -> u64 {
    let mut h = Fnv::default();
    for s in &container.splines {
        h.spline(s);
    }
    h.u64(ex.profile as u64);
    h.f32(ex.radius);
    h.u64(ex.sides as u64);
    h.f32(ex.width);
    h.f32(ex.segments_per_unit);
    h.u64(ex.cap_ends as u64);
    h.f32(ex.range[0]);
    h.f32(ex.range[1]);
    h.f32(ex.uv_tiling);
    h.0
}

/// Mesh key the extrude on `e` should show right now (None: nothing to extrude).
pub fn mesh_key(container: &SplineContainer, ex: &SplineExtrude) -> String {
    format!(
        "{}{NAME}/{:016x}#0",
        runtime::PREFIX,
        inputs_hash(container, ex)
    )
}

fn is_extrude_key(key: &str) -> bool {
    key.starts_with(&format!("{}{NAME}/", runtime::PREFIX))
}

/// Rebuild the meshes of Spline Extrudes whose inputs changed (or whose mesh was dropped).
pub fn update(scene: &mut Scene) {
    let owners: Vec<Entity> = scene
        .world
        .query::<(Entity, &SplineExtrude)>()
        .iter()
        .filter(|(_, ex)| ex.enabled)
        .map(|(e, _)| e)
        .collect();
    let mut rebuilt = false;
    for e in owners {
        let Some(ex) = scene
            .world
            .get::<&SplineExtrude>(e)
            .ok()
            .map(|x| (*x).clone())
        else {
            continue;
        };
        let container = container_object(scene, e, ex.container).and_then(|c| {
            scene
                .world
                .get::<&SplineContainer>(c)
                .ok()
                .map(|c| (*c).clone())
        });
        let Some(container) = container else {
            continue;
        };
        let key = mesh_key(&container, &ex);
        let current = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone());
        let shows = matches!(&current, Some(MeshRef::Asset(k)) if *k == key);
        if shows && runtime::is_registered(&key) {
            continue;
        }
        let mesh = build_mesh(&container, &ex);
        let new = if mesh.indices.is_empty() {
            None
        } else {
            let name = key
                .trim_start_matches(runtime::PREFIX)
                .trim_end_matches("#0")
                .to_owned();
            Some(MeshRef::Asset(runtime::register(&name, mesh)))
        };
        rebuilt = true;
        if let Some(MeshRef::Asset(old)) = &current
            && is_extrude_key(old)
            && Some(old)
                != new.as_ref().and_then(|n| match n {
                    MeshRef::Asset(k) => Some(k),
                    MeshRef::Primitive(_) => None,
                })
        {
            garbage().push(old.clone());
        }
        if let Ok(mut f) = scene.world.get::<&mut MeshFilter>(e) {
            f.mesh = new.clone();
        } else {
            scene.add(e, MeshFilter { mesh: new.clone() });
        }
        if let Ok(mut x) = scene.world.get::<&mut SplineExtrude>(e) {
            x.built = new.and_then(|m| match m {
                MeshRef::Asset(k) => Some(k),
                MeshRef::Primitive(_) => None,
            });
        }
    }
    collect(rebuilt, scene);
}

fn garbage() -> std::sync::MutexGuard<'static, Vec<String>> {
    GARBAGE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drop replaced meshes once editing pauses (or when many piled up), except ones still shown.
fn collect(rebuilt: bool, scene: &Scene) {
    let mut g = garbage();
    if g.is_empty() || (rebuilt && g.len() < 64) {
        return;
    }
    let shown: Vec<String> = scene
        .world
        .query::<(Entity, &MeshFilter)>()
        .iter()
        .filter_map(|(_, f)| match &f.mesh {
            Some(MeshRef::Asset(k)) => Some(k.clone()),
            _ => None,
        })
        .collect();
    g.retain(|k| !shown.contains(k));
    let keys = std::mem::take(&mut *g);
    drop(g);
    runtime::unregister_keys(&keys);
}
