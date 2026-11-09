//! The only place the fog reads the shadow system: map views and per-light matrices. Keep it
//! small so changes to `shadows.rs` (cascades, soft shadows) only need this file adapted.

use glam::Mat4;

use crate::render::shadows::{ShadowMaps, ShadowPlan};

/// Sun cascades (2D array, one layer per cascade) and the local spot/point layers (2D array).
pub(super) fn views(maps: &ShadowMaps) -> (&wgpu::TextureView, &wgpu::TextureView) {
    (&maps.cascade_array_view, &maps.local_array_view)
}

/// World → clip space of each sun cascade, nearest first (empty = no sun shadows this frame).
pub(super) fn sun_cascades(plan: &ShadowPlan) -> Vec<Mat4> {
    // With the virtual shadow map the layers are filled from it (render/vsm/fog.rs).
    let cascades = match plan.vsm {
        Some(_) => &plan.fog_cascades,
        None => &plan.cascades,
    };
    cascades.iter().map(|c| c.view_proj).collect()
}

/// World size of a texel of each sun cascade, nearest first (`None` with the virtual shadow
/// map, whose fog layers are filled from its pages).
pub(super) fn sun_cascade_texels(plan: &ShadowPlan) -> Option<Vec<f32>> {
    plan.vsm
        .is_none()
        .then(|| plan.cascades.iter().map(|c| c.texel).collect())
}

/// World → clip space of each local layer.
pub(super) fn local_matrices(plan: &ShadowPlan) -> &[Mat4] {
    &plan.local
}

/// Shadow layer of `Frame::lights[i]`: -1 = sun cascades, n = first local layer.
pub(super) fn light_layer(plan: &ShadowPlan, i: usize) -> Option<i32> {
    plan.light_layers.get(i).copied().flatten()
}
