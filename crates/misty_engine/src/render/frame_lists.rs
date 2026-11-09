//! What each pass of one render draws: frustum culling per camera and per shadow map, and the
//! instanced batches built from what is left. The shadow maps of large frames are culled and
//! batched in parallel.

use glam::{Mat4, Vec3};
use rayon::prelude::*;

use super::batch::{Batch, Batcher, Merge};
use super::culling::{Aabb, Frustum};
use super::draw_prep::{DrawPrep, PARALLEL_MIN, map_draws};
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
    /// Sun cascades first (when planned), then the local layers.
    pub shadows: Vec<ShadowList>,
    /// Per mesh draw: inside the camera frustum (and not hidden by LOD).
    pub visible: Vec<bool>,
    /// Opaque draws in front-to-back order (GPU culling batches these itself).
    pub opaque_order: Vec<usize>,
    /// Per mesh draw: hidden by baked occlusion culling (empty = none).
    pub occluded: Vec<bool>,
    pub prep: DrawPrep,
}

impl Renderer {
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
        prep: DrawPrep,
        terrains: &[Aabb],
        stats: &mut RenderStats,
    ) -> FrameLists {
        let meshes = &prep.bounds;
        let culling = self.options.culling;
        let instancing = self.options.instancing;
        let merge = |anywhere: bool| match (instancing, anywhere) {
            (false, _) => Merge::None,
            (true, true) => Merge::Anywhere,
            (true, false) => Merge::Neighbours,
        };
        let inside = |f: &Frustum, b: &Aabb| !culling || f.intersects(b);
        let camera = Frustum::from_matrix(view_proj);

        let mut slots = std::mem::take(&mut self.slots_scratch);
        slots.clear();
        slots.extend(0..(frame.meshes.len() + frame.terrains.len()) as u32);
        let mut batcher = std::mem::take(&mut self.batcher);

        let in_frustum = map_draws(meshes.len(), |i| {
            meshes[i].is_some_and(|b| inside(&camera, &b))
        });
        // Baked occlusion culling hides draws for the camera only (shadows keep them).
        let occluded: Vec<bool> = match frame.environment.occlusion.as_ref() {
            Some(o) if culling => (0..meshes.len())
                .map(|i| o.hidden.get(i).copied().unwrap_or(false))
                .collect(),
            _ => Vec::new(),
        };
        let mut visible = in_frustum;
        for (v, o) in visible.iter_mut().zip(&occluded) {
            if *v && *o {
                *v = false;
                stats.occlusion_culled += 1;
            }
        }
        let loaded = meshes.iter().filter(|b| b.is_some()).count() as u32;
        stats.visible = visible.iter().filter(|v| **v).count() as u32;
        stats.culled = loaded - stats.visible;

        let (mut opaque, mut transparent, mut opaque_order) = (Vec::new(), Vec::new(), Vec::new());
        if frame.shaded {
            crate::profile_scope!("Render.CameraLists");
            let order = surface::draw_order(frame.meshes, super::camera_depth(frame.camera));
            let key = |i: usize| prep.main_keys[i].filter(|_| visible[i]);
            opaque = batcher.build(order.opaque.iter().copied(), key, merge(true), &mut slots);
            transparent =
                batcher.build(order.transparent.into_iter(), key, merge(false), &mut slots);
            opaque_order = order.opaque;
        }

        // Every cascade and local layer, each with its own batcher and slot run (in parallel
        // for large frames).
        let shadow_scope = crate::profiler::Scope::begin_lazy(|| "Render.ShadowLists", None);
        let matrices: Vec<Mat4> = plan.matrices().copied().collect();
        let mut batchers = std::mem::take(&mut self.shadow_batchers);
        batchers.resize_with(matrices.len(), Batcher::default);
        let candidates = prep
            .shadow_keys
            .iter()
            .zip(meshes)
            .filter(|(k, b)| k.is_some() && b.is_some())
            .count();
        let n = frame.meshes.len();
        let build = |(m, batcher): (&Mat4, &mut Batcher)| {
            crate::profile_scope!("Render.ShadowList", crate::profiler::Category::Rendering);
            let f = Frustum::from_matrix(m);
            let key = |i: usize| {
                let k = prep.shadow_keys[i]?;
                let b = meshes[i]?;
                inside(&f, &b).then_some(k)
            };
            let mut local = Vec::new();
            let batches = batcher.build(0..n, key, merge(true), &mut local);
            let terrains = frame
                .terrains
                .iter()
                .zip(terrains)
                .map(|(t, b)| t.cast_shadows && inside(&f, b))
                .collect();
            (batches, local, terrains)
        };
        // A few hundred draws take a fraction of a millisecond on this thread. A rayon job
        // injected from the render thread waits for a pool worker to wake and be scheduled,
        // which on a busy machine (a build, an asset import) measured 7 ms a frame on average
        // with 30 ms spikes for MeadowDemo's 450 draws (performance round 22).
        let per_map: Vec<(Vec<Batch>, Vec<u32>, Vec<bool>)> = if n >= PARALLEL_MIN {
            (matrices.par_iter())
                .zip(batchers.par_iter_mut())
                .map(build)
                .collect()
        } else {
            matrices
                .iter()
                .zip(batchers.iter_mut())
                .map(build)
                .collect()
        };
        self.shadow_batchers = batchers;
        drop(shadow_scope);

        let mut caster = vec![false; n];
        let shadows = per_map
            .into_iter()
            .map(|(mut batches, local, terrains)| {
                let offset = slots.len() as u32;
                for s in &local {
                    caster[*s as usize] = true;
                }
                stats.shadow_culled += (candidates - local.len()) as u32;
                slots.extend(local);
                for b in &mut batches {
                    b.first += offset;
                }
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
            visible,
            opaque_order,
            occluded,
            prep,
        }
    }
}
