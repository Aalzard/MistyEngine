//! Skinned Mesh Renderer posing: bone matrices and blend shape weights for each renderer, the
//! posed bounds, the CPU reference deformation, and the draws the renderer deforms on the GPU.
//!
//! Each frame a view's [`crate::draw_cache::DrawCache`] poses its Skinned Mesh Renderers and
//! publishes the result as a [`SkinPose`] under a mesh key `"skinned://…#0"`. The draw then
//! names that key like any imported mesh; the renderer finds the pose, runs the skinning compute
//! pass once per changed pose and draws its output buffer in every pass (shadows, depth prepass,
//! main, outlines). Meshes without skins never touch any of this.
//!
//! Skin matrices are in the renderer object's space: `inverse(renderer) * bone * bind_pose`, so
//! the draw keeps the renderer's world matrix like an ordinary mesh.

pub mod sample;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::models::{self, BlendShape, MeshSkin, Model};
use crate::render::MeshDraw;
use crate::render::mesh::{MeshData, Vertex};
use crate::render_scene::MaterialSource;
use crate::scene::{MeshRef, Scene, SkinnedMeshRenderer};

/// Mesh key prefix of posed skinned meshes.
pub const PREFIX: &str = "skinned://";

pub fn is_pose_key(key: &str) -> bool {
    key.starts_with(PREFIX)
}

/// One posed Skinned Mesh Renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinPose {
    /// The source mesh (bind pose).
    pub mesh: MeshRef,
    /// Skin matrix per joint, in the renderer's space.
    pub matrices: Vec<Mat4>,
    /// Blend shape weights, 0–1.
    pub weights: Vec<f32>,
    /// Posed bounds in the renderer's space.
    pub bounds: (Vec3, Vec3),
    /// Bumped whenever the matrices or weights change (the GPU re-skins only then).
    pub version: u64,
}

struct Published {
    pose: Arc<SkinPose>,
    touched: Instant,
}

#[derive(Default)]
struct Registry {
    poses: HashMap<String, Published>,
    last_prune: Option<Instant>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Default::default);

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Seconds an unrefreshed pose is kept (views that stopped drawing drop theirs).
const KEEP_SECONDS: f32 = 10.0;

/// Store a pose under `key`; its version only changes when the matrices or weights did.
pub fn publish(key: &str, mut pose: SkinPose) -> Arc<SkinPose> {
    let now = Instant::now();
    let mut reg = registry();
    if reg
        .last_prune
        .is_none_or(|t| now.duration_since(t).as_secs_f32() > 1.0)
    {
        reg.last_prune = Some(now);
        reg.poses
            .retain(|_, p| now.duration_since(p.touched).as_secs_f32() < KEEP_SECONDS);
    }
    if let Some(old) = reg.poses.get_mut(key) {
        old.touched = now;
        let same = old.pose.mesh == pose.mesh
            && old.pose.matrices == pose.matrices
            && old.pose.weights == pose.weights;
        if same {
            if old.pose.bounds != pose.bounds {
                pose.version = old.pose.version;
                old.pose = Arc::new(pose);
            }
            return old.pose.clone();
        }
        pose.version = old.pose.version + 1;
    }
    let pose = Arc::new(pose);
    reg.poses.insert(
        key.to_owned(),
        Published {
            pose: pose.clone(),
            touched: now,
        },
    );
    pose
}

/// The pose published under `key`.
pub fn pose(key: &str) -> Option<Arc<SkinPose>> {
    registry().poses.get(key).map(|p| p.pose.clone())
}

/// The imported model and mesh index of a mesh reference.
pub fn model_mesh(r: &MeshRef) -> Option<(Arc<Model>, usize)> {
    let (path, index) = r.split_asset()?;
    if is_pose_key(path) {
        return None;
    }
    let model = models::model(path).ok()?;
    (index < model.meshes.len()).then_some((model, index))
}

/// Blend shape names of a mesh (`None` if it isn't an imported mesh).
pub fn blend_shape_names(r: &MeshRef) -> Option<Vec<String>> {
    let (model, i) = model_mesh(r)?;
    Some(
        model.meshes[i]
            .blend_shapes
            .iter()
            .map(|s| s.name.clone())
            .collect(),
    )
}

/// Corners of a box transformed by `m`, as a new axis-aligned box.
pub fn transform_box(m: &Mat4, (lo, hi): (Vec3, Vec3)) -> (Vec3, Vec3) {
    let mut out = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let p = m.transform_point3(c);
        out.0 = out.0.min(p);
        out.1 = out.1.max(p);
    }
    out
}

/// Pose renderer `e`: `world(x)` = world matrix of object `x`. `None` when the mesh is missing
/// or the renderer's matrix can't be inverted.
pub fn compute_pose(
    scene: &Scene,
    e: Entity,
    r: &SkinnedMeshRenderer,
    world: &dyn Fn(Entity) -> Mat4,
) -> Option<SkinPose> {
    let mesh = r.mesh.clone()?;
    let (model, index) = model_mesh(&mesh)?;
    let m = &model.meshes[index];
    let renderer = world(e);
    if renderer.determinant().abs() < 1e-12 {
        return None;
    }
    let inv = renderer.inverse();
    let bone = |j: usize| {
        r.bones
            .get(j)
            .copied()
            .flatten()
            .filter(|b| scene.contains(*b))
    };
    let matrices: Vec<Mat4> = match &m.skin {
        Some(skin) => skin
            .bind_poses
            .iter()
            .enumerate()
            .map(|(j, bind)| match bone(j) {
                Some(b) => inv * world(b) * *bind,
                None => Mat4::IDENTITY,
            })
            .collect(),
        None => Vec::new(),
    };
    let weights: Vec<f32> = (0..m.blend_shapes.len())
        .map(|s| r.blend_shape_weight(s) / 100.0)
        .collect();
    let root = r.root_bone.filter(|b| scene.contains(*b));
    let bounds = match (&m.skin, root) {
        (Some(skin), _) if r.update_when_offscreen || root.is_none() => {
            posed_bounds(skin, &matrices).unwrap_or(m.bounds)
        }
        (_, Some(root)) => {
            let half = r.bounds_extents.abs();
            let local = (r.bounds_center - half, r.bounds_center + half);
            transform_box(&(inv * world(root)), local)
        }
        _ => {
            let pad = m
                .blend_shapes
                .iter()
                .map(BlendShape::max_offset)
                .sum::<f32>();
            (m.bounds.0 - Vec3::splat(pad), m.bounds.1 + Vec3::splat(pad))
        }
    };
    Some(SkinPose {
        mesh,
        matrices,
        weights,
        bounds,
        version: 0,
    })
}

/// Union of every joint's vertex bounds carried by its skin matrix.
pub fn posed_bounds(skin: &MeshSkin, matrices: &[Mat4]) -> Option<(Vec3, Vec3)> {
    let mut out: Option<(Vec3, Vec3)> = None;
    for (b, m) in skin.joint_bounds.iter().zip(matrices) {
        let Some(b) = b else {
            continue;
        };
        let (lo, hi) = transform_box(m, *b);
        let o = out.get_or_insert((lo, hi));
        o.0 = o.0.min(lo);
        o.1 = o.1.max(hi);
    }
    out
}

/// Root-bone-space bounds that hold the mesh however the bones rotate about the root: a cube
/// around the root as big as the farthest vertex (the import default). Returns (centre, extents).
pub fn default_bounds(m: &models::ModelMesh, root_bind: Option<Mat4>) -> (Vec3, Vec3) {
    let pad = m
        .blend_shapes
        .iter()
        .map(BlendShape::max_offset)
        .sum::<f32>();
    let Some(root) = root_bind else {
        let (lo, hi) = m.bounds;
        return ((lo + hi) * 0.5, (hi - lo) * 0.5 + Vec3::splat(pad));
    };
    let reach = m
        .data
        .vertices
        .iter()
        .map(|v| root.transform_point3(Vec3::from(v.position)).length())
        .fold(0.0, f32::max);
    (Vec3::ZERO, Vec3::splat(reach + pad))
}

/// Deform `data` on the CPU exactly as the GPU pass does: blend shapes first (weights 0–1),
/// then linear blend skinning with `matrices`. The reference for tests and picking.
pub fn deform(
    data: &MeshData,
    skin: Option<&MeshSkin>,
    shapes: &[BlendShape],
    matrices: &[Mat4],
    weights: &[f32],
) -> Vec<Vertex> {
    data.vertices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut p = Vec3::from(v.position);
            let mut n = Vec3::from(v.normal);
            let mut t = Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2]);
            for (s, w) in shapes.iter().zip(weights) {
                if *w != 0.0 {
                    p += Vec3::from(s.positions.get(i).copied().unwrap_or_default()) * *w;
                    n += Vec3::from(s.normals.get(i).copied().unwrap_or_default()) * *w;
                }
            }
            if let Some(skin) = skin.filter(|_| !matrices.is_empty()) {
                let (j, w) = (skin.joints[i], skin.weights[i]);
                let last = matrices.len() - 1;
                let mut m = Mat4::ZERO;
                for k in 0..4 {
                    m += matrices[(j[k] as usize).min(last)] * w[k];
                }
                p = m.transform_point3(p);
                n = m.transform_vector3(n);
                t = m.transform_vector3(t);
            }
            Vertex {
                position: p.into(),
                normal: n.normalize_or(Vec3::Y).into(),
                uv: v.uv,
                tangent: [
                    t.normalize_or(Vec3::X).x,
                    t.normalize_or(Vec3::X).y,
                    t.normalize_or(Vec3::X).z,
                    v.tangent[3],
                ],
            }
        })
        .collect()
}

/// The posed vertices of a published pose (CPU, for picking and tests).
pub fn deform_pose(pose: &SkinPose) -> Option<Vec<Vertex>> {
    let (model, i) = model_mesh(&pose.mesh)?;
    let m = &model.meshes[i];
    Some(deform(
        &m.data,
        m.skin.as_ref(),
        &m.blend_shapes,
        &pose.matrices,
        &pose.weights,
    ))
}

/// Pose every visible Skinned Mesh Renderer and list its draws (one per Materials element).
/// `keep(e)` = layer filter, `world(e)` = world matrices, `outline(e)` = selection outline,
/// `view` = key namespace of the calling view (poses of different scenes never mix).
pub fn draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    view: u64,
    keep: impl Fn(Entity) -> bool,
    world: &dyn Fn(Entity) -> Mat4,
    outline: impl Fn(Entity) -> bool,
    out: &mut Vec<MeshDraw>,
) {
    let renderers: Vec<(Entity, SkinnedMeshRenderer)> = scene
        .world
        .query::<(Entity, &SkinnedMeshRenderer)>()
        .iter()
        .filter(|(_, r)| r.enabled && r.mesh.is_some())
        .map(|(e, r)| (e, r.clone()))
        .collect();
    for (e, r) in renderers {
        if !scene.active_in_hierarchy(e) || !keep(e) {
            continue;
        }
        let Some(pose) = compute_pose(scene, e, &r, world) else {
            continue;
        };
        let key = pose_key(view, e);
        publish(&key, pose);
        let model = world(e);
        let outlined = outline(e);
        for i in 0..r.materials.len().max(1) {
            let path = r.material_at(i);
            let material = match path.and_then(|p| scene.shared_materials.get(p)) {
                Some(m) => m.params(),
                None => materials.params(path),
            };
            out.push(MeshDraw {
                mesh: MeshRef::Asset(key.clone()),
                model,
                material,
                cast_shadows: r.cast_shadows,
                receive_shadows: r.receive_shadows,
                outline: outlined,
                sorting: Default::default(),
                lod: None,
            });
        }
    }
}

/// Mesh key of renderer `e`'s pose in view namespace `view`.
pub fn pose_key(view: u64, e: Entity) -> String {
    format!("{PREFIX}{view}/{}#0", e.to_bits().get())
}

/// A fresh view namespace for [`draws`].
pub fn next_view_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests;

/// The Skinned Mesh Renderer an import gives node `node`'s mesh `mesh`: bones are the objects
/// made for the skin's joint nodes (`objects[i]` = object of model node `i`), the bounds hold
/// the mesh however the bones turn about the root, weights start at the file's defaults.
pub fn renderer_for(
    model: &Model,
    rel: &str,
    node: usize,
    mesh: usize,
    objects: &[Entity],
    material: Option<String>,
) -> SkinnedMeshRenderer {
    let m = &model.meshes[mesh];
    let skin = model.nodes[node].skin.and_then(|s| model.skins.get(s));
    let object = |i: usize| objects.get(i).copied();
    let (bones, root_bone, root_bind) = match skin {
        Some(s) => {
            let root_bind = s
                .root
                .and_then(|r| s.joints.iter().position(|j| *j == r))
                .and_then(|k| s.inverse_bind.get(k).copied());
            (
                s.joints.iter().map(|j| object(*j)).collect(),
                s.root.and_then(object),
                root_bind,
            )
        }
        None => (Vec::new(), None, None),
    };
    let (center, extents) = default_bounds(m, root_bind);
    SkinnedMeshRenderer {
        mesh: Some(MeshRef::asset(rel, mesh)),
        materials: vec![material],
        root_bone,
        bones,
        bounds_center: center,
        bounds_extents: extents,
        blend_shape_weights: m.default_weights.clone(),
        ..Default::default()
    }
}

/// Whether model node `node` should get a Skinned Mesh Renderer (skinned or morphing meshes).
pub fn is_skinned_node(model: &Model, node: usize) -> bool {
    let n = &model.nodes[node];
    n.skin.is_some()
        || n.meshes
            .iter()
            .any(|m| !model.meshes[*m].blend_shapes.is_empty())
}
