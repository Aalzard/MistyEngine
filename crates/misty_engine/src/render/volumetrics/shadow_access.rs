//! The only place the fog reads the shadow system: map views and per-light matrices. Keep it
//! small so changes to `shadows.rs` (cascades, soft shadows) only need this file adapted.

use glam::Mat4;

use crate::render::shadows::{ShadowMaps, ShadowPlan};

/// Directional map (2D) and the local spot/point layers (2D array).
pub(super) fn views(maps: &ShadowMaps) -> (&wgpu::TextureView, &wgpu::TextureView) {
    (&maps.directional_view, &maps.local_array_view)
}

/// World → directional shadow clip space, when a directional light has a map this frame.
pub(super) fn sun_matrix(plan: &ShadowPlan) -> Option<Mat4> {
    plan.directional
}

/// World → clip space of each local layer.
pub(super) fn local_matrices(plan: &ShadowPlan) -> &[Mat4] {
    &plan.local
}

/// Shadow layer of `Frame::lights[i]`: -1 = directional map, n = first local layer.
pub(super) fn light_layer(plan: &ShadowPlan, i: usize) -> Option<i32> {
    plan.light_layers.get(i).copied().flatten()
}
