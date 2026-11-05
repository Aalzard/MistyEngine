//! Spline Instantiate: copies of prefabs or primitives spread along a spline, by count or by
//! spacing, with random offset, rotation and scale ranges. Copies are children of the
//! component's object marked with Spline Instance; they are rebuilt (deleted and made again)
//! whenever the inputs change.

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;

use super::components::{
    AlignMode, InstantiateMethod, SplineContainer, SplineInstance, SplineInstantiate,
    container_object,
};
use super::{Fnv, Rng, SplinePath};
use crate::camera::euler_to_quat;
use crate::scene::prefab::Prefab;
use crate::scene::{MeshFilter, MeshRenderer, Scene, Transform};

/// Most copies one component makes.
pub const MAX_INSTANCES: usize = 4096;

/// One copy, in the container's local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub item: usize,
    /// Distance along the spline.
    pub distance: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// Distances along the spline where copies go.
pub fn distances(path: &SplinePath, inst: &SplineInstantiate) -> Vec<f32> {
    let len = path.length;
    if path.is_empty() {
        return Vec::new();
    }
    match inst.method {
        InstantiateMethod::InstanceCount => {
            let n = (inst.count as usize).min(MAX_INSTANCES);
            match n {
                0 => Vec::new(),
                1 => vec![0.0],
                // A loop's last copy would sit on its first.
                _ if path.closed => (0..n).map(|i| len * i as f32 / n as f32).collect(),
                _ => (0..n).map(|i| len * i as f32 / (n - 1) as f32).collect(),
            }
        }
        InstantiateMethod::SplineDistance => {
            let lo = inst.spacing[0].min(inst.spacing[1]).max(0.01);
            let hi = inst.spacing[0].max(inst.spacing[1]).max(lo);
            let mut rng = Rng::new(inst.seed as u64 ^ 0x5eed_0001);
            let mut out = Vec::new();
            let mut d = 0.0;
            let eps = 1e-4 * len.max(1.0);
            while out.len() < MAX_INSTANCES {
                if d > len + eps || (path.closed && d > len - eps && !out.is_empty()) {
                    break;
                }
                out.push(d.min(len));
                d += rng.range(lo, hi);
            }
            out
        }
    }
}

/// Where each copy goes (container-local), given the container's world rotation (for World
/// alignment).
pub fn placements(
    path: &SplinePath,
    inst: &SplineInstantiate,
    container_rotation: Quat,
) -> Vec<Placement> {
    let weights: Vec<f32> = inst.items.iter().map(|i| i.probability.max(0.0)).collect();
    let total: f32 = weights.iter().sum();
    if inst.items.is_empty() {
        return Vec::new();
    }
    let mut rng = Rng::new(inst.seed as u64);
    distances(path, inst)
        .into_iter()
        .map(|d| {
            let t = path.t_at_distance(d);
            let base = match inst.align {
                AlignMode::SplineElement => path.rotation(t),
                AlignMode::SplineObject => Quat::IDENTITY,
                AlignMode::World => container_rotation.inverse(),
            };
            let item = if total > 0.0 {
                let mut pick = rng.unit() * total;
                let mut chosen = weights.len() - 1;
                for (i, w) in weights.iter().enumerate() {
                    if pick < *w {
                        chosen = i;
                        break;
                    }
                    pick -= w;
                }
                chosen
            } else {
                0
            };
            let offset = rng.vec3(inst.position_min, inst.position_max);
            let euler = rng.vec3(inst.rotation_min, inst.rotation_max);
            let scale = rng.vec3(inst.scale_min, inst.scale_max);
            Placement {
                item,
                distance: d,
                position: path.position(t) + base * offset,
                rotation: base * euler_to_quat(euler),
                scale,
            }
        })
        .collect()
}

/// Copies currently made by the component on `owner`.
pub fn instances(scene: &Scene, owner: Entity) -> Vec<Entity> {
    scene
        .children(owner)
        .into_iter()
        .filter(|c| scene.world.get::<&SplineInstance>(*c).is_ok())
        .collect()
}

fn inputs_hash(container: &SplineContainer, inst: &SplineInstantiate, rel: Option<Mat4>) -> u64 {
    let mut h = Fnv::default();
    if let Some(s) = container.splines.get(inst.spline) {
        h.spline(s);
    }
    h.u64(inst.spline as u64);
    for item in &inst.items {
        h.str(item.prefab.as_deref().unwrap_or(""));
        h.u64(item.primitive as u64);
        h.str(item.material.as_deref().unwrap_or(""));
        h.f32(item.probability);
    }
    h.u64(inst.method as u64);
    h.u64(inst.count as u64);
    h.f32(inst.spacing[0]);
    h.f32(inst.spacing[1]);
    h.u64(inst.align as u64);
    for v in [
        inst.position_min,
        inst.position_max,
        inst.rotation_min,
        inst.rotation_max,
        inst.scale_min,
        inst.scale_max,
    ] {
        h.vec3(v);
    }
    h.u64(inst.seed as u64);
    if let Some(m) = rel {
        for v in m.to_cols_array() {
            h.f32(v);
        }
    }
    // Never 0 (0 means "not built").
    h.0.max(1)
}

/// Rebuild the copies of Spline Instantiate components whose inputs changed.
pub fn update(scene: &mut Scene) {
    let owners: Vec<Entity> = scene
        .world
        .query::<(Entity, &SplineInstantiate)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in owners {
        rebuild_if_changed(scene, e);
    }
}

/// Rebuild one component's copies if its inputs changed. True if it rebuilt.
pub fn rebuild_if_changed(scene: &mut Scene, e: Entity) -> bool {
    let Some(inst) = scene
        .world
        .get::<&SplineInstantiate>(e)
        .ok()
        .map(|i| (*i).clone())
    else {
        return false;
    };
    let container_e = container_object(scene, e, inst.container);
    let container = container_e.and_then(|c| {
        scene
            .world
            .get::<&SplineContainer>(c)
            .ok()
            .map(|c| (*c).clone())
    });
    // Copies sit relative to this object; a container elsewhere adds its relative transform.
    let rel = container_e
        .filter(|c| *c != e)
        .map(|c| scene.world_matrix(e).inverse() * scene.world_matrix(c));
    let hash = match (&container, inst.enabled) {
        (Some(c), true) => inputs_hash(c, &inst, rel),
        _ => 1,
    };
    if hash == inst.built {
        return false;
    }
    for old in instances(scene, e) {
        scene.despawn(old);
    }
    if let (Some(container), Some(ce), true) = (&container, container_e, inst.enabled) {
        let path = container.path(inst.spline);
        let container_rot = scene.world_rotation(ce);
        let rel = rel.unwrap_or(Mat4::IDENTITY);
        let mut prefabs: Vec<Option<Prefab>> = Vec::new();
        for item in &inst.items {
            prefabs.push(
                item.prefab
                    .as_deref()
                    .and_then(|p| Prefab::load(&crate::assets::absolute(p)).ok()),
            );
        }
        for (i, p) in placements(&path, &inst, container_rot).iter().enumerate() {
            let item = &inst.items[p.item];
            let local =
                rel * Mat4::from_scale_rotation_translation(p.scale, p.rotation, p.position);
            let (scale, rotation, position) = local.to_scale_rotation_translation();
            let child = match (&prefabs[p.item], &item.prefab) {
                (Some(prefab), Some(path)) => scene.instantiate_prefab(prefab, Some(path), Some(e)),
                _ => {
                    let name = format!("{} ({i})", item.primitive.name());
                    let c = scene.spawn(&name, Some(e));
                    scene.add(c, MeshFilter::primitive(item.primitive));
                    scene.add(
                        c,
                        MeshRenderer {
                            material: item.material.clone(),
                            ..Default::default()
                        },
                    );
                    c
                }
            };
            scene.set_transform(
                child,
                Transform {
                    position,
                    rotation,
                    scale,
                    ..Default::default()
                },
            );
            scene.add(child, SplineInstance {});
        }
    }
    if let Ok(mut c) = scene.world.get::<&mut SplineInstantiate>(e) {
        c.built = hash;
    }
    true
}
