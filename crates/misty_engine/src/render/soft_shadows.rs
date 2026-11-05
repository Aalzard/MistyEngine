//! Soft shadows on the CPU side: per-light source size and biases, the parameters the PCSS
//! shader include (`shaders/shadows.wgsl`) reads, and the shadow-map passes in plan order.

use bytemuck::{Pod, Zeroable};

use super::cascades::MAX_CASCADES;
use super::frame_lists::FrameLists;
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
    /// Multiplier on the penumbra (Light ▸ Shadow Softness; 1 = physical size).
    pub softness: f32,
}

impl Default for LightShadow {
    fn default() -> Self {
        Self {
            size: 0.0,
            bias: light_shadow::BIAS,
            normal_bias: light_shadow::NORMAL_BIAS,
            softness: light_shadow::SOFTNESS,
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
    /// x = widest filter radius (texels), y = narrowest, z = depth bias per metre of radius.
    filter: [f32; 4],
    /// Per light: [size, depth bias, normal bias, 0], [near, far, tan(half fov), faces].
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
    let (widest, narrowest) = settings.filter_texels();
    u.filter = [widest, narrowest, FILTER_BIAS, 0.0];
    let soft = settings.soft != crate::quality::Tier::Off;
    // The same lights in the same order as `lights_uniform`.
    let lit = frame.lights.iter().filter(|l| l.intensity > 0.0);
    for (n, l) in lit.take(MAX_LIGHTS).enumerate() {
        u.lights[2 * n] = [
            source_size(l),
            l.shadow.bias.max(0.0),
            l.shadow.normal_bias.max(0.0),
            0.0,
        ];
        let faces = shadows::rect_faces(l, soft);
        let (near, far, tan_half) = shadows::local_clip(l, faces);
        u.lights[2 * n + 1] = [near, far, tan_half, faces as f32];
    }
    u
}

/// Depth bias per metre of filter radius: wide kernels reach parts of a curved receiver that
/// bend away from its tangent plane (a 30 cm penumbra adds 6 mm).
const FILTER_BIAS: f32 = 0.02;

/// Directional: tan(half the angular diameter). Spot / point: the radius. Rectangle: just the
/// softness, since the shader projects the rectangle's own half axes. All scaled by the
/// light's softness.
fn source_size(l: &LightData) -> f32 {
    let size = l.shadow.size.max(0.0);
    let softness = l.shadow.softness.max(0.0);
    softness
        * match l.kind {
            LightType::Directional => (size.min(20.0).to_radians() * 0.5).tan(),
            LightType::Rectangle => 1.0,
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
        lists: &FrameLists,
        stats: &mut RenderStats,
    ) {
        let cascades = plan.cascades.len();
        for (slot, list) in lists.shadows.iter().enumerate() {
            let view = match slot.checked_sub(cascades) {
                None => &self.shadows.cascade_views[slot],
                Some(layer) => &self.shadows.local_layer_views[layer],
            };
            if self.capture.is_some() {
                use super::frame_debug::{self, EventKind, EventTarget, FrameEvent};
                let (name, size, display) = match slot.checked_sub(cascades) {
                    None => (
                        format!("Cascade {slot}"),
                        self.shadows.directional_size,
                        EventTarget::Shadow {
                            local: false,
                            layer: slot as u32,
                        },
                    ),
                    Some(layer) => (
                        format!("Local Light Layer {layer}"),
                        shadows::LOCAL_SIZE,
                        EventTarget::Shadow {
                            local: true,
                            layer: layer as u32,
                        },
                    ),
                };
                let target = || {
                    let t = format!("Shadow Map ({name})");
                    frame_debug::target(&t, (size, size), "Depth32Float")
                };
                frame_debug::pass(&self.capture, &["Shadows", &name], target, display);
                frame_debug::record(&self.capture, |_| {
                    FrameEvent::new(EventKind::Clear, "Clear (depth)")
                });
            }
            // Grass casts into the cascades that start within its caster range.
            let grass_range = self.terrains.grass_caster_range();
            let grass = plan.cascades.get(slot).filter(|_| {
                let start = slot.checked_sub(1).map_or(0.0, |p| plan.cascades[p].far);
                grass_range.is_some_and(|r| start < r)
            });
            let grass = grass.map(|c| &c.view_proj);
            self.shadow_pass(encoder, frame, view, slot, lists, Some(list), grass, stats);
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
        // 4 matrices, 4 cascade rows, 4 vec4s, 2 vec4s per light (shadows.wgsl).
        assert_eq!(
            std::mem::size_of::<ShadowUniform>(),
            64 * MAX_CASCADES + 16 * MAX_CASCADES + 64 + 32 * MAX_LIGHTS
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
            area: Default::default(),
            shadow: LightShadow {
                size: 2.0,
                ..Default::default()
            },
            volumetric: Default::default(),
        };
        assert!((source_size(&l) - 1f32.to_radians().tan()).abs() < 1e-6);
        l.kind = LightType::Point;
        assert_eq!(source_size(&l), 2.0);
        l.shadow.softness = 0.5;
        assert_eq!(source_size(&l), 1.0);
        l.kind = LightType::Rectangle;
        assert_eq!(source_size(&l), 0.5);
    }
}
