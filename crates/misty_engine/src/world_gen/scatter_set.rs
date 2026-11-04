//! The `Scatter` component: many instances of one scatter item stored compactly on a single
//! object (positions quantised to 16 bits over the set's bounds), drawn as ordinary mesh draws
//! that share materials, so the renderer's automatic instancing batches them.

use std::sync::{Arc, OnceLock};

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::biome::ScatterShape;
use super::scatter::ScatterInstance;
use crate::render::MeshDraw;
use crate::render_scene::MaterialSource;
use crate::scene::{MeshRef, PrimitiveMesh, Scene};

/// One mesh of a scattered object, relative to the instance (before its scale).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterPart {
    pub mesh: MeshRef,
    pub material: Option<String>,
    pub offset: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for ScatterPart {
    fn default() -> Self {
        Self {
            mesh: MeshRef::Primitive(PrimitiveMesh::Cube),
            material: None,
            offset: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

impl ScatterPart {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            Quat::IDENTITY,
            Vec3::from(self.offset),
        )
    }
}

/// Which part of a primitive shape a material is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRole {
    /// Tree trunks (shared bark material).
    Trunk,
    /// Crown, rock body… (the item's colour).
    Main,
}

/// Primitive parts of a scatter shape: (mesh, offset, scale, role).
pub fn shape_parts(shape: ScatterShape) -> Vec<(PrimitiveMesh, [f32; 3], [f32; 3], PartRole)> {
    use PartRole::*;
    use PrimitiveMesh as P;
    match shape {
        ScatterShape::BroadleafTree => vec![
            (P::Cylinder, [0.0, 1.5, 0.0], [0.35, 1.5, 0.35], Trunk),
            (P::Sphere, [0.0, 4.2, 0.0], [3.4, 3.0, 3.4], Main),
        ],
        ScatterShape::PineTree => vec![
            (P::Cylinder, [0.0, 1.2, 0.0], [0.3, 1.2, 0.3], Trunk),
            (P::Capsule, [0.0, 4.4, 0.0], [2.2, 2.4, 2.2], Main),
        ],
        ScatterShape::Rock => vec![(P::Sphere, [0.0, 0.15, 0.0], [1.4, 0.8, 1.1], Main)],
        ScatterShape::Boulder => vec![(P::Sphere, [0.0, 0.4, 0.0], [2.6, 1.8, 2.1], Main)],
        ScatterShape::Bush => vec![(P::Sphere, [0.0, 0.45, 0.0], [1.7, 1.2, 1.7], Main)],
        ScatterShape::Grass => vec![(P::Capsule, [0.0, 0.25, 0.0], [0.22, 0.3, 0.22], Main)],
        ScatterShape::Cactus => vec![(P::Capsule, [0.0, 1.6, 0.0], [0.6, 1.6, 0.6], Main)],
    }
}

/// Many instances of one item on one object (the object's transform places them).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterSet {
    pub enabled: bool,
    pub cast_shadows: bool,
    pub parts: Vec<ScatterPart>,
    /// Bounds the positions are quantised over (object-local).
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub max_scale: f32,
    pub count: usize,
    /// base64 of 7 little-endian u16 per instance: x, y, z, yaw, scale, up.x, up.z.
    pub data: String,
    /// Decoded instance matrices (rebuilt after loading).
    #[serde(skip)]
    cache: Arc<OnceLock<Vec<Mat4>>>,
}

impl Default for ScatterSet {
    fn default() -> Self {
        Self {
            enabled: true,
            cast_shadows: true,
            parts: Vec::new(),
            min: [0.0; 3],
            max: [1.0; 3],
            max_scale: 1.0,
            count: 0,
            data: String::new(),
            cache: Default::default(),
        }
    }
}

impl PartialEq for ScatterSet {
    fn eq(&self, o: &Self) -> bool {
        self.enabled == o.enabled
            && self.cast_shadows == o.cast_shadows
            && self.parts == o.parts
            && self.min == o.min
            && self.max == o.max
            && self.count == o.count
            && self.data == o.data
    }
}

fn q16(v: f32, lo: f32, hi: f32) -> u16 {
    (((v - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn dq16(q: u16, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * q as f32 / 65535.0
}

/// Rotation of an instance: yaw about +Y, then tilt +Y onto `up`.
pub fn instance_rotation(yaw: f32, up: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, up.normalize_or(Vec3::Y)) * Quat::from_rotation_y(yaw)
}

impl ScatterSet {
    /// Pack `instances` (object-local positions).
    pub fn new(parts: Vec<ScatterPart>, instances: &[ScatterInstance]) -> Self {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut max_scale = 0.01f32;
        for i in instances {
            lo = lo.min(i.position);
            hi = hi.max(i.position);
            max_scale = max_scale.max(i.scale);
        }
        if instances.is_empty() {
            (lo, hi) = (Vec3::ZERO, Vec3::ONE);
        }
        let hi = hi.max(lo + Vec3::splat(0.01));
        let tau = std::f32::consts::TAU;
        let mut bytes = Vec::with_capacity(instances.len() * 14);
        for i in instances {
            let v = [
                q16(i.position.x, lo.x, hi.x),
                q16(i.position.y, lo.y, hi.y),
                q16(i.position.z, lo.z, hi.z),
                q16(i.yaw.rem_euclid(tau), 0.0, tau),
                q16(i.scale, 0.0, max_scale),
                q16(i.up.x, -1.0, 1.0),
                q16(i.up.z, -1.0, 1.0),
            ];
            for x in v {
                bytes.extend(x.to_le_bytes());
            }
        }
        Self {
            parts,
            min: lo.into(),
            max: hi.into(),
            max_scale,
            count: instances.len(),
            data: base64::encode(&bytes),
            ..Default::default()
        }
    }

    /// Instances as stored (object-local).
    pub fn instances(&self) -> Vec<ScatterInstance> {
        let bytes = base64::decode(&self.data).unwrap_or_default();
        let (lo, hi) = (Vec3::from(self.min), Vec3::from(self.max));
        let tau = std::f32::consts::TAU;
        bytes
            .as_chunks::<14>()
            .0
            .iter()
            .take(self.count)
            .map(|c| {
                let u = |k: usize| u16::from_le_bytes([c[2 * k], c[2 * k + 1]]);
                let (ux, uz) = (dq16(u(5), -1.0, 1.0), dq16(u(6), -1.0, 1.0));
                let uy = (1.0 - ux * ux - uz * uz).max(0.0).sqrt();
                ScatterInstance {
                    position: Vec3::new(
                        dq16(u(0), lo.x, hi.x),
                        dq16(u(1), lo.y, hi.y),
                        dq16(u(2), lo.z, hi.z),
                    ),
                    yaw: dq16(u(3), 0.0, tau),
                    scale: dq16(u(4), 0.0, self.max_scale),
                    up: Vec3::new(ux, uy, uz),
                }
            })
            .collect()
    }

    /// Object-local instance matrices (decoded once).
    pub fn matrices(&self) -> &[Mat4] {
        self.cache.get_or_init(|| {
            self.instances()
                .iter()
                .map(|i| {
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(i.scale),
                        instance_rotation(i.yaw, i.up),
                        i.position,
                    )
                })
                .collect()
        })
    }
}

/// Mesh draws of every enabled Scatter on active objects whose layer `keep` accepts.
pub fn draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    keep: impl Fn(Entity) -> bool,
    out: &mut Vec<MeshDraw>,
) {
    for (e, s) in scene.world.query::<(Entity, &ScatterSet)>().iter() {
        if !s.enabled || !scene.active_in_hierarchy(e) || !keep(e) {
            continue;
        }
        let world = scene.world_matrix(e);
        for part in &s.parts {
            let material = materials.params(part.material.as_deref());
            let local = part.matrix();
            for m in s.matrices() {
                out.push(MeshDraw {
                    mesh: part.mesh.clone(),
                    model: world * *m * local,
                    material: material.clone(),
                    cast_shadows: s.cast_shadows,
                    receive_shadows: true,
                    outline: false,
                    sorting: Default::default(),
                });
            }
        }
    }
}

/// Standard base64 (padding), for the packed instance data.
mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                out.push(if i <= chunk.len() {
                    ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char
                } else {
                    '='
                });
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let (mut acc, mut bits) = (0u32, 0u32);
        for c in text.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' | b'\n' | b'\r' | b' ' => continue,
                _ => return None,
            };
            acc = acc << 6 | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        Some(out)
    }
}
