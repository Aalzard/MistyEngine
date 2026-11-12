//! Rectangle area lights: shape data, the LTC lookup table the shaders integrate GGX and
//! Lambert over the rectangle with (`area_light.wgsl`), and the optional emissive quad.
//!
//! Units: a rectangle light's colour × intensity is the radiance of its surface. A surface
//! facing an infinitely large one receives colour × intensity, like a directional light
//! of that intensity shining straight at it; the visible emitter quad has that brightness too.

mod ggx;
pub mod ltc_fit;
mod nelder_mead;
pub mod polygon;
pub mod table;

use glam::{Mat3, Mat4, Vec2, Vec3};

use super::{LightData, MaterialParams, MeshDraw};
use crate::scene::{LightGlow, LightType, PrimitiveMesh};

/// Size and orientation of a rectangle light (unused by other light types).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaShape {
    /// Width along `right` and height along `up`, in metres.
    pub size: Vec2,
    /// World axes of the rectangle (unit); right × up is the emitting direction.
    pub right: Vec3,
    pub up: Vec3,
    /// Emits from both faces (else only towards the light's forward axis).
    pub two_sided: bool,
    /// Draw the rectangle as an emissive quad.
    pub show_emitter: bool,
    /// Light ▸ Glow of the emitter quad (its bloom, spread halo and soft core).
    pub glow: LightGlow,
}

impl Default for AreaShape {
    fn default() -> Self {
        Self {
            size: Vec2::ONE,
            right: Vec3::X,
            up: Vec3::Y,
            two_sided: false,
            show_emitter: false,
            glow: LightGlow::default(),
        }
    }
}

impl AreaShape {
    /// Half axes scaled by the half width and height.
    pub fn half_axes(&self) -> (Vec3, Vec3) {
        (
            self.right * (self.size.x * 0.5).max(0.0),
            self.up * (self.size.y * 0.5).max(0.0),
        )
    }

    /// Radius used for soft shadows: half the mean side.
    pub fn shadow_radius(&self) -> f32 {
        0.25 * (self.size.x + self.size.y).max(0.0)
    }
}

/// The storage buffer of the LTC table (mesh group binding 7).
pub fn table_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("misty ltc table"),
        contents: bytemuck::cast_slice(table::texels()),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

/// Lambert (diffuse) and LTC (specular) integrals of a rectangle seen from `p` with normal `n`
/// and view `v`, and the lobe's Fresnel scale and bias. CPU copy of `rect_light` (tests).
pub fn integrals(
    light: &LightData,
    p: Vec3,
    n: Vec3,
    v: Vec3,
    perceptual: f32,
) -> (f32, f32, [f32; 2]) {
    let (hx, hy) = light.area.half_axes();
    let frame = shading_frame_corners(light.position, hx, hy, p, n, v);
    let side = |x: f32| {
        if light.area.two_sided {
            x.abs()
        } else {
            x.max(0.0)
        }
    };
    let diffuse = side(polygon::integrate(Mat3::IDENTITY, frame));
    let t = (1.0 - n.dot(v).clamp(0.0, 1.0)).sqrt();
    let m = table::fetch(0, perceptual, t);
    let minv = Mat3::from_cols(
        Vec3::new(m[0], 0.0, m[2]),
        Vec3::Y,
        Vec3::new(m[1], 0.0, m[3]),
    );
    let spec = side(polygon::integrate(minv, frame));
    let s = table::fetch(ltc_fit::SIZE * ltc_fit::SIZE, perceptual, t);
    (diffuse, spec, [s[0], s[1]])
}

fn shading_frame_corners(center: Vec3, hx: Vec3, hy: Vec3, p: Vec3, n: Vec3, v: Vec3) -> [Vec3; 4] {
    let frame = polygon::shading_frame(n, v);
    polygon::rect_corners(center, hx, hy).map(|c| frame * (c - p))
}

/// Emissive quads of the rectangle lights with Show Emitter on (no shadows, no outline).
pub fn emitter_draws(lights: &[LightData]) -> Vec<MeshDraw> {
    lights
        .iter()
        .filter(|l| l.kind == LightType::Rectangle && l.area.show_emitter && l.intensity > 0.0)
        .map(|l| {
            let a = &l.area;
            let g = a.glow.clamped();
            let forward = a.right.cross(a.up).normalize_or(Vec3::Z);
            // The quad faces -Z: turn it half a turn about up so it faces the light's forward.
            let model = Mat4::from_cols(
                (-a.right * a.size.x.max(1e-3)).extend(0.0),
                (a.up * a.size.y.max(1e-3)).extend(0.0),
                (-forward).extend(0.0),
                l.position.extend(1.0),
            );
            MeshDraw {
                mesh: PrimitiveMesh::Quad.into(),
                model,
                material: MaterialParams {
                    color: [0.0, 0.0, 0.0, 1.0],
                    smoothness: 0.0,
                    emission: l.color,
                    emission_intensity: l.intensity,
                    double_sided: a.two_sided,
                    light_glow: [1.0 - super::glow::emitter_feed(&g), g.soft_core],
                    ..Default::default()
                },
                cast_shadows: false,
                receive_shadows: false,
                outline: false,
                sorting: Default::default(),
                lod: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;
