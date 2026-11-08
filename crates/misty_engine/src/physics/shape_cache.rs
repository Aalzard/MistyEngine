//! Mesh Collider shapes built ahead of time on a background thread (World Partition cells):
//! building a triangle mesh's BVH takes milliseconds, so a streamed cell's static Mesh
//! Colliders are made while the cell is read, and the main thread only picks them up.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use glam::Vec3;
use rapier3d::prelude as rp;

use crate::scene::MeshRef;

/// Shapes waiting for their collider (each is used once).
static SHAPES: LazyLock<Mutex<HashMap<String, rp::SharedShape>>> = LazyLock::new(Default::default);

/// More waiting than this (cells dropped before they were added) and the cache starts over.
const MAX: usize = 4096;

fn shapes() -> std::sync::MutexGuard<'static, HashMap<String, rp::SharedShape>> {
    SHAPES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Key of a non-convex mesh shape at a world scale.
pub fn key(mesh: &MeshRef, scale: Vec3) -> String {
    let q = |v: f32| (v * 1000.0).round() as i64;
    let name = match mesh {
        MeshRef::Asset(k) => k.as_str(),
        MeshRef::Primitive(p) => p.name(),
    };
    format!("{name}|{}|{}|{}", q(scale.x), q(scale.y), q(scale.z))
}

/// The triangle-mesh shape of `mesh` scaled by `scale` (vertices in the object's frame).
pub fn trimesh(mesh: &MeshRef, scale: Vec3) -> Option<rp::SharedShape> {
    let data = crate::models::mesh(mesh)?;
    let verts: Vec<rp::Vector> = data
        .vertices
        .iter()
        .map(|v| {
            let p = Vec3::from(v.position) * scale;
            rp::Vector::new(p.x, p.y, p.z)
        })
        .collect();
    let tris = data.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
    rp::SharedShape::trimesh(verts, tris).ok()
}

/// Build a shape now (on the calling thread) for a collider that will ask for it later.
pub fn prebuild(mesh: &MeshRef, scale: Vec3) {
    let k = key(mesh, scale);
    if shapes().contains_key(&k) {
        return;
    }
    if let Some(shape) = trimesh(mesh, scale) {
        let mut map = shapes();
        if map.len() >= MAX {
            map.clear();
        }
        map.insert(k, shape);
    }
}

/// A shape built ahead for this mesh and scale, if any.
pub fn take(mesh: &MeshRef, scale: Vec3) -> Option<rp::SharedShape> {
    shapes().remove(&key(mesh, scale))
}
