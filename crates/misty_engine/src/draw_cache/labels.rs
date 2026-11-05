//! Object and material names of the draw list (Frame Debugger labels).

use super::DrawCache;
use crate::render::DrawLabel;
use crate::scene::{MeshRenderer, Scene};

impl DrawCache {
    /// One label per draw of the last [`DrawCache::update`] list that comes from a Mesh
    /// Renderer (in list order); merged static batches are named "Combined Mesh (Static) i".
    /// Draws after the labelled ones (sprites, tilemaps, skinned meshes, scatter) have none.
    pub fn labels(&self, scene: &Scene) -> Vec<DrawLabel> {
        let mut out = Vec::new();
        let renderers = scene.world.view::<&MeshRenderer>();
        for e in &self.order {
            let Some(entry) = self.renderers.get(e.id() as usize).and_then(|r| r.as_ref()) else {
                continue;
            };
            if entry.entity != *e || (self.batching.on && entry.batched) {
                continue;
            }
            let object = scene
                .world
                .get::<&crate::scene::GameObject>(*e)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let r = renderers.get(*e);
            for k in 0..entry.count {
                out.push(DrawLabel {
                    object: object.clone(),
                    material: r.and_then(|r| r.material_at(k as usize)).map(str::to_owned),
                    submesh: k,
                });
            }
        }
        if self.batching.on {
            for i in 0..self.batching.merged.len() {
                out.push(DrawLabel {
                    object: format!("Combined Mesh (Static) {i}"),
                    material: None,
                    submesh: 0,
                });
            }
        }
        out
    }
}
