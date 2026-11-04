//! Soft shadows on the CPU side: per-light source size and biases, the parameters the PCSS
//! shader include (`shaders/shadows.wgsl`) reads, and the shadow-map passes in plan order.

use bytemuck::{Pod, Zeroable};

use super::cascades::MAX_CASCADES;
use super::frame_lists::ShadowList;
use super::shadows::{self, ShadowPlan};
use super::{Frame, LightData, MAX_LIGHTS, RenderStats, Renderer, ViewUniform};
use crate::quality::ShadowSettings;
use crate::scene::{LightType, light_shadow};

/// A light's shadow shape (Light â–¸ Shadows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightShadow {
    /// Directional: angular diameter in degrees. Spot / point: source radius in metres.
    /// 0 = the narrowest soft filter.
    pub size: f32,
    /// Depth bias in shadow-map texels.
    pub bias: f32,
    /// Normal offset in shadow-map texels.
    pub normal_bias: f32,
}

impl Default for LightShadow {
    fn default() -> Self {
        Self {
            size: 0.0,
            bias: light_shadow::BIAS,
            normal_bias: light_shadow::NORMAL_BIAS,
        }
    }
}

/// `ShadowParams` in shadows.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct ShadowUniform {
    cascades: [[[f32; 4]; 4]; MAX_CASCADES],
    /// x = far split (view depth), y = texel (world), z = depth range (world), w = map width.
    cascade_info: [[f32; 4]; MAX_CASCADES],
    /// x = cascade count, y = blend fraction, z = blocker samples, w = filter samples.
    settings: [f32; 4],
    /// xyz = camera forward, w = shadow distance.
    view_axis: [f32; 4],
    /// xy = target size (pixels), z = 1 to tint cascades, w = camera near.
    misc: [f32; 4],
    /// Per light: [size, depth bias, normal bias, 0], [near, far, tan(half fov), 0].
    lights: [[f32; 4]; 2 * MAX_LIGHTS],
}

fn pack(
    frame: &Frame,
    plan: &ShadowPlan,
    settings: &ShadowSettings,
    size: (u32, u32),
    debug: bool,
) -> ShadowUniform {
    let mut u = ShadowUniform::zeroed();
    for (i, c) in plan.cascades.iter().enumerate() {
        u.cascades[i] = c.view_proj.to_cols_array_2d();
        u.cascade_info[i] = [c.far, c.texel, c.depth_range, 2.0 * c.radius];
    }
    let (blocker, filter) = settings.samples();
    let count = plan.cascades.len() as f32;
    u.settings = [count, settings.cascade_blend, blocker as f32, filter as f32];
    let forward = frame.camera.forward();
    let distance = plan.cascades.last().map_or(0.0, |c| c.far);
    u.view_axis = forward.extend(distance).to_array();
    let debug = if debug { 1.0 } else { 0.0 };
    u.misc = [
        size.0 as f32,
        size.1 as f32,
        debug,
        frame.camera.near.max(0.01),
    ];
    // The same lights in the same order as `lights_uniform`.
    let lit = frame.lights.iter().filter(|l| l.intensity > 0.0);
    for (n, l) in lit.take(MAX_LIGHTS).enumerate() {
        u.lights[2 * n] = [
            source_size(l),
            l.shadow.bias.max(0.0),
            l.shadow.normal_bias.max(0.0),
            0.0,
        ];
        let (near, far, tan_half) = shadows::local_clip(l);
        u.lights[2 * n + 1] = [near, far, tan_half, 0.0];
    }
    u
}

/// Directional: tan(half the angular diameter). Spot / point: the radius.
fn source_size(l: &LightData) -> f32 {
    let size = l.shadow.size.max(0.0);
    match l.kind {
        LightType::Directional => (size.min(20.0).to_radians() * 0.5).tan(),
        _ => size,
    }
}

impl Renderer {
    /// Tint each sun cascade in the next render only (Scene view â–¸ Shadow Cascades).
    pub fn set_cascade_debug(&mut self, on: bool) {
        self.shadows.debug_cascades = on;
    }

    /// Upload one view slot per shadow map and the soft-shadow parameters.
    pub(super) fn write_shadow_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        frame: &Frame,
        plan: &ShadowPlan,
        settings: &ShadowSettings,
        size: (u32, u32),
    ) {
        for (slot, m) in plan.matrices().enumerate() {
            let mut v = ViewUniform::zeroed();
            v.view_proj = m.to_cols_array_2d();
            let offset = slot as u64 * shadows::VIEW_STRIDE;
            queue.write_buffer(&self.shadows.view_buffer, offset, bytemuck::bytes_of(&v));
        }
        let debug = std::mem::take(&mut self.shadows.debug_cascades);
        let u = pack(frame, plan, settings, size, debug);
        queue.write_buffer(&self.shadows.params_buffer, 0, bytemuck::bytes_of(&u));
    }

    /// Render every planned shadow map: cascades, then the local layers.
    pub(super) fn shadow_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        plan: &ShadowPlan,
        lists: &[ShadowList],
        stats: &mut RenderStats,
    ) {
        let cascades = plan.cascades.len();
        for (slot, list) in lists.iter().enumerate() {
            let view = match slot.checked_sub(cascades) {
                None => &self.shadows.cascade_views[slot],
                Some(layer) => &self.shadows.local_layer_views[layer],
            };
            self.shadow_pass(encoder, frame, view, slot, Some(list), stats);
        }
    }
}

#[cfg(test)]
#[path = "pcss_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_matches_wgsl_and_sizes_convert() {
        // 4 matrices, 4 cascade rows, 3 vec4s, 2 vec4s per light (shadows.wgsl).
        assert_eq!(
            std::mem::size_of::<ShadowUniform>(),
            64 * MAX_CASCADES + 16 * MAX_CASCADES + 48 + 32 * MAX_LIGHTS
        );
        let mut l = LightData {
            kind: LightType::Directional,
            position: glam::Vec3::ZERO,
            direction: glam::Vec3::NEG_Y,
            color: [1.0; 3],
            intensity: 1.0,
            range: 10.0,
            spot_angle: 30.0,
            shadows: crate::scene::Shadows::Soft,
            contact_shadows: false,
            shadow: LightShadow {
                size: 2.0,
                ..Default::default()
            },
            volumetric: Default::default(),
        };
        assert!((source_size(&l) - 1f32.to_radians().tan()).abs() < 1e-6);
        l.kind = LightType::Point;
        assert_eq!(source_size(&l), 2.0);
    }
}
