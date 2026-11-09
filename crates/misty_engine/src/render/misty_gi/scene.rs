//! The scene as Misty GI traces it: a flat list of distance-field shapes built from the frame
//! every render (cheap: one entry per object), culled per clipmap level on the CPU.
//!
//! Shapes: analytic primitives (box, ellipsoid, capsule, cylinder), meshes with a baked mesh
//! SDF (`mesh_sdf`), terrain heightfields and terrain trees (trunk capsule + crown ellipsoid).
//! Each carries one average albedo and emission: diffuse GI needs the colour of a surface, not
//! its texture detail.

use std::hash::{Hash, Hasher};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};

use super::super::surface::RenderingMode;
use super::super::{Frame, MeshDraw, srgb_to_linear};
use crate::scene::{MeshRef, PrimitiveMesh};

/// Shape kinds (`Instance::extent.w`, `gi_common.wgsl`).
pub const KIND_BOX: f32 = 0.0;
pub const KIND_ELLIPSOID: f32 = 1.0;
pub const KIND_CAPSULE: f32 = 2.0;
pub const KIND_CYLINDER: f32 = 3.0;
pub const KIND_MESH: f32 = 4.0;
pub const KIND_TERRAIN: f32 = 5.0;

/// Average albedo of terrain (grass and soil; the layers carry no tint, see the design doc).
const TERRAIN_ALBEDO: [f32; 3] = [0.16, 0.2, 0.09];
/// Crown proxies are smaller than the leaf cloud (light gets through its gaps).
const CROWN_SHRINK: f32 = 0.75;
/// Leaf textures darken the tint about this much on average.
const LEAF_DARKEN: f32 = 0.45;
const BARK_ALBEDO: [f32; 3] = [0.12, 0.09, 0.07];

/// One shape (192 bytes, `Instance` in gi_common.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub struct Instance {
    /// World → shape space (rigid for analytic shapes, the full inverse model for meshes,
    /// a translation to the terrain's corner for terrains).
    pub inv: [[f32; 4]; 4],
    /// Shape parameters (see the kinds); w = kind.
    pub extent: [f32; 4],
    /// Meshes: atlas brick origin in texels (xyz), w = shape → world distance scale.
    pub shape: [f32; 4],
    /// Meshes: bake box min in mesh space; terrains: atlas region (x, y, columns, rows).
    pub bounds_min: [f32; 4],
    /// Meshes: bake box max; terrains: size (x, height, z).
    pub bounds_max: [f32; 4],
    /// Linear diffuse albedo (rgb).
    pub albedo: [f32; 4],
    /// Linear emitted radiance (rgb).
    pub emission: [f32; 4],
    /// World bounds (culling, early-out in the compose).
    pub aabb_min: [f32; 4],
    pub aabb_max: [f32; 4],
}

/// Where a baked mesh SDF lives in the atlas (`mesh_sdf`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brick {
    pub origin: [u32; 3],
    /// Bake box in mesh space.
    pub min: Vec3,
    pub max: Vec3,
}

/// Where a terrain's heights live in the terrain atlas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainRegion {
    pub x: u32,
    pub y: u32,
    pub columns: u32,
    pub rows: u32,
}

/// The frame's shapes.
#[derive(Default)]
pub struct SceneList {
    pub instances: Vec<Instance>,
    /// Lights as the surface cache reads them.
    pub lights: Vec<GiLight>,
}

/// A light (96 bytes, `GiLight` in gi_common.wgsl); the lit shaders' `LightItem` layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GiLight {
    pub pos_range: [f32; 4],
    /// xyz = direction the light travels, w = kind (0 directional, 1 point, 2 spot, 3 rect).
    pub dir_type: [f32; 4],
    /// Linear colour × intensity.
    pub color: [f32; 4],
    /// x = cos outer half angle, y = cos inner half angle.
    pub spot: [f32; 4],
    /// Rectangle: half-width axis (w = 1 when two-sided).
    pub area_x: [f32; 4],
    /// Rectangle: half-height axis.
    pub area_y: [f32; 4],
}

pub const MAX_LIGHTS: usize = 32;

fn v4(v: Vec3, w: f32) -> [f32; 4] {
    v.extend(w).to_array()
}

fn rigid(position: Vec3, rotation: Quat) -> [[f32; 4]; 4] {
    Mat4::from_rotation_translation(rotation, position)
        .inverse()
        .to_cols_array_2d()
}

/// World bounds of a box of half extents `half` posed by `rotation` at `position`.
fn posed_aabb(position: Vec3, rotation: Quat, half: Vec3) -> (Vec3, Vec3) {
    let m = glam::Mat3::from_quat(rotation);
    let e = m.x_axis.abs() * half.x + m.y_axis.abs() * half.y + m.z_axis.abs() * half.z;
    (position - e, position + e)
}

impl Instance {
    fn analytic(
        kind: f32,
        position: Vec3,
        rotation: Quat,
        params: Vec3,
        bound_half: Vec3,
        albedo: Vec3,
        emission: Vec3,
    ) -> Self {
        let (lo, hi) = posed_aabb(position, rotation, bound_half);
        Self {
            inv: rigid(position, rotation),
            extent: v4(params, kind),
            shape: [0.0; 4],
            bounds_min: [0.0; 4],
            bounds_max: [0.0; 4],
            albedo: v4(albedo, 0.0),
            emission: v4(emission, 0.0),
            aabb_min: v4(lo, 0.0),
            aabb_max: v4(hi, 0.0),
        }
    }

    pub fn aabb(&self) -> (Vec3, Vec3) {
        (
            Vec3::from_slice(&self.aabb_min[..3]),
            Vec3::from_slice(&self.aabb_max[..3]),
        )
    }
}

/// Whether a mesh draw takes part (opaque, lit, sends light into GI, LOD 0).
fn traced(d: &MeshDraw) -> bool {
    let m = &d.material;
    !m.unlit
        && m.contribute_gi
        && matches!(m.mode, RenderingMode::Opaque | RenderingMode::Cutout)
        && d.lod.is_none_or(|l| l.max.is_infinite())
}

/// Linear albedo and emitted radiance of a draw's material.
fn colours(d: &MeshDraw) -> (Vec3, Vec3) {
    let m = &d.material;
    let c = Vec3::from(srgb_to_linear([m.color[0], m.color[1], m.color[2]]));
    let albedo = c * (1.0 - m.metallic.clamp(0.0, 1.0));
    let emission = Vec3::from(srgb_to_linear(m.emission)) * m.emission_intensity.max(0.0);
    (albedo, emission)
}

/// The shape of one mesh draw (None for meshes without a baked SDF yet).
fn mesh_instance(
    d: &MeshDraw,
    mesh_brick: &mut dyn FnMut(&MeshRef) -> Option<Brick>,
) -> Option<Instance> {
    let (scale, rotation, position) = d.model.to_scale_rotation_translation();
    let s = scale.abs();
    let (albedo, emission) = colours(d);
    let a = |kind, params: Vec3, half: Vec3| {
        Some(Instance::analytic(
            kind, position, rotation, params, half, albedo, emission,
        ))
    };
    match &d.mesh {
        MeshRef::Primitive(p) => match p {
            PrimitiveMesh::Cube => a(KIND_BOX, s * 0.5, s * 0.5),
            PrimitiveMesh::Sphere => a(KIND_ELLIPSOID, s * 0.5, s * 0.5),
            PrimitiveMesh::Capsule => {
                let r = 0.5 * s.x.max(s.z);
                let half = (s.y - r).max(0.0);
                a(
                    KIND_CAPSULE,
                    Vec3::new(r, half, 0.0),
                    Vec3::new(r, half + r, r),
                )
            }
            PrimitiveMesh::Cylinder => {
                let r = 0.5 * s.x.max(s.z);
                a(KIND_CYLINDER, Vec3::new(r, s.y, 0.0), Vec3::new(r, s.y, r))
            }
            PrimitiveMesh::Plane => {
                let h = Vec3::new(5.0 * s.x, 0.0, 5.0 * s.z);
                a(KIND_BOX, h, h)
            }
            PrimitiveMesh::Quad => {
                let h = Vec3::new(0.5 * s.x, 0.5 * s.y, 0.0);
                a(KIND_BOX, h, h)
            }
        },
        MeshRef::Asset(_) => {
            let brick = mesh_brick(&d.mesh)?;
            let (lo, hi) = transformed_aabb(d.model, brick.min, brick.max);
            Some(Instance {
                inv: d.model.inverse().to_cols_array_2d(),
                extent: [0.0, 0.0, 0.0, KIND_MESH],
                shape: [
                    brick.origin[0] as f32,
                    brick.origin[1] as f32,
                    brick.origin[2] as f32,
                    s.min_element().max(1e-4),
                ],
                bounds_min: v4(brick.min, 0.0),
                bounds_max: v4(brick.max, 0.0),
                albedo: v4(albedo, 0.0),
                emission: v4(emission, 0.0),
                aabb_min: v4(lo, 0.0),
                aabb_max: v4(hi, 0.0),
            })
        }
    }
}

/// World AABB of a mesh-space box under `m`.
pub fn transformed_aabb(m: Mat4, lo: Vec3, hi: Vec3) -> (Vec3, Vec3) {
    let mut out = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let w = m.transform_point3(c);
        out = (out.0.min(w), out.1.max(w));
    }
    out
}

/// Bilinear normalised height of a terrain at `u, v` (0..1 across it).
fn terrain_height(d: &crate::terrain::TerrainData, u: f32, v: f32) -> f32 {
    let r = d.heightmap_resolution.max(2);
    let x = (u.clamp(0.0, 1.0) * (r - 1) as f32).min((r - 1) as f32 - 1e-3);
    let z = (v.clamp(0.0, 1.0) * (r - 1) as f32).min((r - 1) as f32 - 1e-3);
    let (ix, iz) = (x as usize, z as usize);
    let (fx, fz) = (x.fract(), z.fract());
    let h = |a, b| d.height(a, b);
    let top = h(ix, iz) * (1.0 - fx) + h(ix + 1, iz) * fx;
    let bottom = h(ix, iz + 1) * (1.0 - fx) + h(ix + 1, iz + 1) * fx;
    top * (1.0 - fz) + bottom * fz
}

/// Trunk and crown proxies of a terrain's trees (those that can reach `region`, the world box
/// the clipmap levels cover: the rest would be culled from every level anyway, and a forest of
/// a hundred thousand trees made the per-frame list and its upload enormous).
fn tree_instances(
    t: &crate::terrain::TerrainDraw,
    region: Option<(Vec3, Vec3)>,
    out: &mut Vec<Instance>,
) {
    let d = &t.data;
    let protos: Vec<Option<std::sync::Arc<crate::trees::store::Built>>> = d
        .trees
        .prototypes
        .iter()
        .map(|p| crate::trees::store::get(&p.tree))
        .collect();
    for inst in &d.trees.instances {
        let Some(Some(built)) = protos.get(inst.prototype as usize) else {
            continue;
        };
        let a = &built.asset;
        let foot = t.position
            + Vec3::new(
                inst.position.x * d.size.x,
                terrain_height(d, inst.position.x, inst.position.y) * d.size.y,
                inst.position.y * d.size.z,
            );
        let height = a.height * inst.height_scale;
        let width = a.crown_width * a.height * inst.width_scale;
        if let Some((lo, hi)) = region {
            let r = Vec3::new(width, 0.0, width) * 0.5 + Vec3::splat(1.0);
            let (a0, a1) = (foot - r, foot + r + Vec3::Y * height);
            if a0.cmpgt(hi).any() || a1.cmplt(lo).any() {
                continue;
            }
        }
        let crown_base = a.crown_base.clamp(0.0, 0.95) * height;
        let trunk_r = (a.trunk_radius * inst.width_scale).max(0.05);
        // Trunk up to the middle of the crown.
        let trunk_top = crown_base + (height - crown_base) * 0.5;
        let half = trunk_top * 0.5;
        out.push(Instance::analytic(
            KIND_CAPSULE,
            foot + Vec3::Y * half,
            Quat::IDENTITY,
            Vec3::new(trunk_r, (half - trunk_r).max(0.0), 0.0),
            Vec3::new(trunk_r, half, trunk_r),
            Vec3::from(BARK_ALBEDO),
            Vec3::ZERO,
        ));
        let radii = Vec3::new(width * 0.5, (height - crown_base) * 0.5, width * 0.5) * CROWN_SHRINK;
        let leaf = Vec3::from(srgb_to_linear(a.leaf_color)) * LEAF_DARKEN;
        out.push(Instance::analytic(
            KIND_ELLIPSOID,
            foot + Vec3::Y * (crown_base + (height - crown_base) * 0.5),
            Quat::IDENTITY,
            radii,
            radii,
            leaf,
            Vec3::ZERO,
        ));
    }
}

/// The frame's shapes and lights. `terrain_region` places (and uploads) a terrain's heights;
/// `mesh_brick` returns a mesh's baked SDF (baking it when needed).
pub fn build(
    frame: &Frame,
    mesh_brick: &mut dyn FnMut(&MeshRef) -> Option<Brick>,
    terrain_region: &mut dyn FnMut(&crate::terrain::TerrainDraw) -> Option<TerrainRegion>,
) -> SceneList {
    build_within(frame, None, mesh_brick, terrain_region)
}

/// [`build`] with the trees limited to those that can reach `region` (the levels' world box,
/// margins included).
pub fn build_within(
    frame: &Frame,
    reach: Option<(Vec3, Vec3)>,
    mesh_brick: &mut dyn FnMut(&MeshRef) -> Option<Brick>,
    terrain_region: &mut dyn FnMut(&crate::terrain::TerrainDraw) -> Option<TerrainRegion>,
) -> SceneList {
    let mut list = SceneList::default();
    for d in frame.meshes.iter().filter(|d| traced(d)) {
        if let Some(i) = mesh_instance(d, mesh_brick) {
            list.instances.push(i);
        }
    }
    for t in frame.terrains {
        let Some(region) = terrain_region(t) else {
            continue;
        };
        let size = t.data.size;
        let (lo, hi) = height_range(&t.data);
        list.instances.push(Instance {
            inv: Mat4::from_translation(-t.position).to_cols_array_2d(),
            extent: [0.0, 0.0, 0.0, KIND_TERRAIN],
            shape: [0.0; 4],
            bounds_min: [
                region.x as f32,
                region.y as f32,
                region.columns as f32,
                region.rows as f32,
            ],
            bounds_max: v4(size, 0.0),
            albedo: v4(Vec3::from(TERRAIN_ALBEDO), 0.0),
            emission: [0.0; 4],
            // Everything below the surface is inside: the bounds reach well under it.
            aabb_min: v4(t.position + Vec3::new(0.0, lo - 1000.0, 0.0), 0.0),
            aabb_max: v4(t.position + Vec3::new(size.x, hi, size.z), 0.0),
        });
        tree_instances(t, reach, &mut list.instances);
    }
    list.lights = lights(frame);
    list
}

/// Lowest and highest terrain height in metres above its corner.
fn height_range(d: &crate::terrain::TerrainData) -> (f32, f32) {
    let (lo, hi) = d
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
    if lo > hi {
        return (0.0, 0.0);
    }
    (lo * d.size.y, hi * d.size.y)
}

fn lights(frame: &Frame) -> Vec<GiLight> {
    use crate::scene::LightType;
    frame
        .lights
        .iter()
        .filter(|l| l.intensity > 0.0)
        .take(MAX_LIGHTS)
        .map(|l| {
            let c = Vec3::from(srgb_to_linear(l.color)) * l.intensity;
            let kind = match l.kind {
                LightType::Directional => 0.0,
                LightType::Point => 1.0,
                LightType::Spot => 2.0,
                LightType::Rectangle => 3.0,
            };
            let half = (l.spot_angle * 0.5).to_radians();
            let (area_x, area_y) = if l.kind == LightType::Rectangle {
                let (hx, hy) = l.area.half_axes();
                (
                    v4(hx, if l.area.two_sided { 1.0 } else { 0.0 }),
                    v4(hy, 0.0),
                )
            } else {
                ([0.0; 4], [0.0; 4])
            };
            GiLight {
                pos_range: v4(l.position, l.range),
                dir_type: v4(l.direction.normalize_or(Vec3::Z), kind),
                color: v4(c, 0.0),
                spot: [half.cos(), (half * 0.8).cos(), 0.0, 0.0],
                area_x,
                area_y,
            }
        })
        .collect()
}

/// Indices of the instances a clipmap box (`lo..hi`, already grown by the margin) overlaps.
pub fn cull(instances: &[Instance], lo: Vec3, hi: Vec3) -> Vec<u32> {
    instances
        .iter()
        .enumerate()
        .filter(|(_, i)| {
            let (a, b) = i.aabb();
            a.cmple(hi).all() && b.cmpge(lo).all()
        })
        .map(|(n, _)| n as u32)
        .collect()
}

/// Hash of what a level's compose depends on (its box and its instances).
pub fn level_hash(instances: &[Instance], indices: &[u32], origin: Vec3, extra: u64) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytemuck::bytes_of(&origin.to_array()).hash(&mut h);
    extra.hash(&mut h);
    for &i in indices {
        bytemuck::bytes_of(&instances[i as usize]).hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_shader() {
        assert_eq!(std::mem::size_of::<Instance>(), 192);
        assert_eq!(std::mem::size_of::<GiLight>(), 96);
    }

    #[test]
    fn culling_keeps_overlapping_shapes() {
        let a = Instance::analytic(
            KIND_BOX,
            Vec3::new(10.0, 0.0, 0.0),
            Quat::IDENTITY,
            Vec3::ONE,
            Vec3::ONE,
            Vec3::ONE,
            Vec3::ZERO,
        );
        let b = Instance::analytic(
            KIND_BOX,
            Vec3::ZERO,
            Quat::from_rotation_y(0.7),
            Vec3::ONE,
            Vec3::ONE,
            Vec3::ONE,
            Vec3::ZERO,
        );
        let list = [a, b];
        assert_eq!(cull(&list, Vec3::splat(-2.0), Vec3::splat(2.0)), vec![1]);
        assert_eq!(
            cull(&list, Vec3::splat(-20.0), Vec3::splat(20.0)),
            vec![0, 1]
        );
        let h0 = level_hash(&list, &[0, 1], Vec3::ZERO, 0);
        let h1 = level_hash(&list, &[0, 1], Vec3::X, 0);
        assert_ne!(h0, h1, "moving the level recomposes it");
    }
}
