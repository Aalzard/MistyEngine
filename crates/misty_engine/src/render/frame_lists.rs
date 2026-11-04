//! What each pass of one render draws: world bounds, frustum culling per camera and per
//! shadow map, and the instanced batches built from what is left.

use glam::{Mat4, Vec3};

use super::batch::{Batch, BatchKey, Merge, addr};
use super::culling::{Aabb, Frustum};
use super::shadows::ShadowPlan;
use super::{Frame, RenderStats, Renderer, surface};

/// Casters of one shadow map.
pub(super) struct ShadowList {
    pub batches: Vec<Batch>,
    /// Per terrain: inside the light's frustum.
    pub terrains: Vec<bool>,
}

pub(super) struct FrameLists {
    /// Instance → draw slot. Entries `0..draw count` are the identity (single draws such as
    /// terrains and the selection mask); the batches' runs follow.
    pub slots: Vec<u32>,
    /// Opaque then Cutout batches, roughly front to back.
    pub opaque: Vec<Batch>,
    /// Fade / Transparent, back to front.
    pub transparent: Vec<Batch>,
    /// Per terrain / particle system: inside the camera frustum.
    pub terrains: Vec<bool>,
    pub particles: Vec<bool>,
    /// Directional map first (when planned), then the local layers.
    pub shadows: Vec<ShadowList>,
}

impl Renderer {
    /// World box of each mesh draw (`None` = its mesh isn't loaded, so it isn't drawn).
    pub(super) fn mesh_bounds(&self, frame: &Frame) -> Vec<Option<Aabb>> {
        frame
            .meshes
            .iter()
            .map(|d| Some(self.gpu_mesh(&d.mesh)?.bounds.transformed(&d.model)))
            .collect()
    }

    /// World box of each terrain (cached per terrain version).
    pub(super) fn terrain_bounds(&self, frame: &Frame) -> Vec<Aabb> {
        frame
            .terrains
            .iter()
            .map(|t| {
                let (lo, hi) = self.terrains.world_bounds(t);
                Aabb::new(lo, hi)
            })
            .collect()
    }

    /// Bounds of the shadow casters, for fitting the directional shadow box.
    pub(super) fn caster_bounds(
        frame: &Frame,
        meshes: &[Option<Aabb>],
        terrains: &[Aabb],
    ) -> Vec<(Vec3, Vec3)> {
        let meshes = frame
            .meshes
            .iter()
            .zip(meshes)
            .filter(|(d, _)| d.cast_shadows)
            .filter_map(|(_, b)| b.map(|b| (b.min, b.max)));
        let terrains = frame
            .terrains
            .iter()
            .zip(terrains)
            .filter(|(t, _)| t.cast_shadows)
            .map(|(_, b)| (b.min, b.max));
        meshes.chain(terrains).collect()
    }

    /// Cull and batch every pass of the frame.
    pub(super) fn frame_lists(
        &mut self,
        frame: &Frame,
        view_proj: &Mat4,
        plan: &ShadowPlan,
        bounds: (&[Option<Aabb>], &[Aabb]),
        stats: &mut RenderStats,
    ) -> FrameLists {
        let (meshes, terrains) = bounds;
        let culling = self.options.culling;
        let instancing = self.options.instancing;
        let merge = |anywhere: bool| match (instancing, anywhere) {
            (false, _) => Merge::None,
            (true, true) => Merge::Anywhere,
            (true, false) => Merge::Neighbours,
        };
        let inside = |f: &Frustum, b: &Aabb| !culling || f.intersects(b);
        let camera = Frustum::from_matrix(view_proj);

        // Batch keys per draw, resolved once.
        let main_keys: Vec<Option<BatchKey>> =
            frame.meshes.iter().map(|d| self.main_key(d)).collect();
        let shadow_keys: Vec<Option<BatchKey>> =
            frame.meshes.iter().map(|d| self.shadow_key(d)).collect();

        let mut slots = std::mem::take(&mut self.slots_scratch);
        slots.clear();
        slots.extend(0..(frame.meshes.len() + frame.terrains.len()) as u32);
        let mut batcher = std::mem::take(&mut self.batcher);

        let visible: Vec<bool> = meshes
            .iter()
            .map(|b| b.is_some_and(|b| inside(&camera, &b)))
            .collect();
        let loaded = meshes.iter().filter(|b| b.is_some()).count() as u32;
        stats.visible = visible.iter().filter(|v| **v).count() as u32;
        stats.culled = loaded - stats.visible;

        let (mut opaque, mut transparent) = (Vec::new(), Vec::new());
        if frame.shaded {
            let order = surface::draw_order(frame.meshes, super::camera_depth(frame.camera));
            let key = |i: usize| main_keys[i].filter(|_| visible[i]);
            opaque = batcher.build(order.opaque.into_iter(), key, merge(true), &mut slots);
            transparent =
                batcher.build(order.transparent.into_iter(), key, merge(false), &mut slots);
        }

        let mut caster = vec![false; frame.meshes.len()];
        let matrices = plan.directional.iter().chain(&plan.local);
        let shadows = matrices
            .map(|m| {
                let f = Frustum::from_matrix(m);
                let key = |i: usize| {
                    let k = shadow_keys[i]?;
                    let b = meshes[i]?;
                    inside(&f, &b).then_some(k)
                };
                let batches = batcher.build(0..frame.meshes.len(), key, merge(true), &mut slots);
                for b in &batches {
                    for s in &slots[b.first as usize..(b.first + b.count) as usize] {
                        caster[*s as usize] = true;
                    }
                }
                let drawn: usize = batches.iter().map(|b| b.count as usize).sum();
                let candidates = shadow_keys
                    .iter()
                    .zip(meshes)
                    .filter(|(k, b)| k.is_some() && b.is_some())
                    .count();
                stats.shadow_culled += (candidates - drawn) as u32;
                let terrains = frame
                    .terrains
                    .iter()
                    .zip(terrains)
                    .map(|(t, b)| t.cast_shadows && inside(&f, b))
                    .collect();
                ShadowList { batches, terrains }
            })
            .collect();
        stats.shadow_casters = caster.iter().filter(|c| **c).count() as u32;

        let terrains = terrains.iter().map(|b| inside(&camera, b)).collect();
        let particles = frame
            .particles
            .iter()
            .map(|p| {
                let points = p.particles.iter().map(|i| (Vec3::from(i.position), i.size));
                Aabb::around(points).is_some_and(|b| inside(&camera, &b))
            })
            .collect();
        self.batcher = batcher;
        FrameLists {
            slots,
            opaque,
            transparent,
            terrains,
            particles,
            shadows,
        }
    }

    /// Main pass key: pipeline (variant or `.shader`), material group and mesh.
    fn main_key(&self, d: &super::MeshDraw) -> Option<BatchKey> {
        let (pipeline, group) = self.material_resources(&d.material)?;
        let mesh = self.gpu_mesh(&d.mesh)?;
        Some([addr(pipeline), addr(group), addr(mesh)])
    }

    /// Shadow caster key (`None` = casts no shadow): caster pipeline, mesh and, for cutout
    /// casters, the textures they clip with.
    fn shadow_key(&self, d: &super::MeshDraw) -> Option<BatchKey> {
        let key = d.material.shadow_key().filter(|_| d.cast_shadows)?;
        let pipeline = self.pipelines.shadow_variant(key)?;
        let group = match key.cutout {
            true => addr(self.textures.get_set(&d.material.textures())?),
            false => 0,
        };
        let mesh = self.gpu_mesh(&d.mesh)?;
        Some([addr(pipeline), addr(mesh), group])
    }
}
