//! Occlusion culling for a view's draw list: which renderer owns each draw, Occludee Static
//! objects kept out of static batches (a merged batch can't be culled per object), and the
//! occludee map, rebuilt only when the draw list changed.

use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use super::DrawCache;
use crate::occlusion::OcclusionData;
use crate::occlusion::runtime::{OccludeeMap, OcclusionResult, compute};
use crate::scene::Scene;

#[derive(Default)]
pub(super) struct State {
    /// Keep Occludee Static objects out of static batches.
    pub split: bool,
    /// Data the map was built for (by address) and the call it was built in.
    data: usize,
    built: u64,
    map: OccludeeMap,
}

impl DrawCache {
    /// Keep Occludee Static objects out of static batches (set while occlusion data is used).
    pub fn set_occlusion_split(&mut self, on: bool) {
        if on != self.occlusion.split {
            self.occlusion.split = on;
            self.batching.dirty = true;
        }
    }

    /// The renderer of each draw of the last [`DrawCache::update`] (`None` for static batches,
    /// sprites, skinned meshes, tilemaps and scatter).
    pub fn draw_owners(&self) -> Vec<Option<Entity>> {
        let target = if self.batching.on {
            &self.batching.out
        } else {
            &self.out
        };
        let mut owners = Vec::with_capacity(target.len());
        for e in &self.order {
            let Some(r) = self.renderers.get(e.id() as usize).and_then(|r| r.as_ref()) else {
                continue;
            };
            if r.entity != *e || (self.batching.on && r.batched) {
                continue;
            }
            owners.extend(std::iter::repeat_n(Some(*e), r.count as usize));
        }
        owners.resize(target.len(), None);
        owners
    }

    /// Occlusion of the last update's draws seen from `eye`.
    pub fn occlusion(
        &mut self,
        scene: &Scene,
        data: &Arc<OcclusionData>,
        eye: Vec3,
        dynamic: bool,
    ) -> OcclusionResult {
        let address = Arc::as_ptr(data) as usize;
        let changed = self.stats.relaid || self.stats.rebuilt > 0;
        if address != self.occlusion.data || changed || self.occlusion.built == 0 {
            self.occlusion.map = OccludeeMap::build(scene, data);
            self.occlusion.data = address;
            self.occlusion.built = self.calls;
        }
        compute(data, &self.occlusion.map, &self.draw_owners(), eye, dynamic)
    }
}
