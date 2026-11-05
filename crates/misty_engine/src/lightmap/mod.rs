//! Baked global illumination (the lightmapper).
//!
//! - [`uv`]: lightmap UVs (second UV set) for primitives and imported models.
//! - [`atlas`]: renderers' rectangles packed into lightmaps (scale-offset per renderer).
//! - [`bake`]: a multi-threaded CPU path tracer over a BVH of the static triangles: direct light
//!   of Baked lights, indirect bounces of every baked light, the sky and emissive surfaces;
//!   then an edge-aware denoise and seam dilation.
//! - [`data`] / [`store`]: `Lightmap-N.lightmap` files next to the scene and their cache.
//!
//! The renderer samples lightmaps (rgba16f texture array) through `MeshRenderer::lightmap_index`
//! and `lightmap_scale_offset`: lightmapped surfaces take their diffuse ambient / indirect light
//! from the lightmap instead of the realtime ambient and probe diffuse.

pub mod atlas;
pub mod bake;
pub mod data;
pub mod store;
pub mod uv;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use data::LightmapData;

use crate::scene::Scene;

/// File extension of baked lightmaps.
pub const EXTENSION: &str = "lightmap";

/// Project path of the `index`-th lightmap of a scene: `Assets/Scenes/Level.scene` →
/// `Assets/Scenes/Level/Lightmap-0.lightmap`.
pub fn asset_path_for(scene_path: &str, index: usize) -> String {
    let stem = scene_path.strip_suffix(".scene").unwrap_or(scene_path);
    format!("{stem}/Lightmap-{index}.{EXTENSION}")
}

pub fn is_lightmap(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

/// Scene view draw modes that show lightmaps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightmapDebug {
    #[default]
    Off,
    /// Lightmap texels only (Baked Lightmap).
    Baked,
    /// Lightmap texel grid and charts (UV Charts).
    Charts,
}

impl LightmapDebug {
    pub fn index(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Baked => 1.0,
            Self::Charts => 2.0,
        }
    }
}

/// Shared lightmap compared by identity (what the GPU cache keys on).
#[derive(Clone)]
pub struct LightmapHandle(pub Arc<LightmapData>);

impl PartialEq for LightmapHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for LightmapHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightmapHandle({}×{})", self.0.width, self.0.height)
    }
}

/// The lightmaps of one view (index = `MeshRenderer::lightmap_index`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightmapSet {
    /// `None` where a file is missing (those renderers draw unlightmapped).
    pub maps: Vec<Option<LightmapHandle>>,
    pub debug: LightmapDebug,
}

impl LightmapSet {
    pub fn is_empty(&self) -> bool {
        self.maps.iter().all(Option::is_none)
    }

    /// Is lightmap `index` loaded?
    pub fn has(&self, index: i32) -> bool {
        index >= 0 && self.maps.get(index as usize).is_some_and(Option::is_some)
    }
}

/// The scene's baked lightmaps (loaded through [`store`]).
pub fn scene_lightmaps(scene: &Scene) -> LightmapSet {
    let s = &scene.render_settings.lightmapping;
    if !s.baked_gi {
        return LightmapSet::default();
    }
    LightmapSet {
        maps: s
            .lightmaps
            .iter()
            .map(|rel| store::load(&crate::assets::absolute(rel)).map(LightmapHandle))
            .collect(),
        debug: LightmapDebug::Off,
    }
}

/// Does the scene have baked lightmaps (then Baked lights stop lighting in realtime)?
pub fn scene_has_lightmaps(scene: &Scene) -> bool {
    scene.render_settings.lightmapping.baked_gi
        && !scene.render_settings.lightmapping.lightmaps.is_empty()
}

/// Per-draw lightmap of a renderer: lightmap index and uv2 scale-offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawLightmap {
    pub index: u32,
    pub scale_offset: [f32; 4],
}
