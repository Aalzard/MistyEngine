//! GPU layouts of the fog uniforms (mirrors of `fog_apply.wgsl` and `froxel_common.wgsl`)
//! and how a frame fills them.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::settings::{FogMode, FogSettings};
use super::shadow_access;
use crate::render::cascades::MAX_CASCADES;
use crate::render::shadows::{MAX_LOCAL_LAYERS, ShadowPlan};
use crate::render::{Frame, LightData, srgb_to_linear};
use crate::scene::{LightType, Shadows};

pub(super) const MAX_FOG_LIGHTS: usize = 8;
pub(super) const MAX_FOG_BOXES: usize = 16;

/// `VolumetricFog` in fog_apply.wgsl (view group, binding 3).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct ApplyUniform {
    pub mode: [f32; 4],
    pub slices: [f32; 4],
    pub forward: [f32; 4],
    pub height_fog: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub ambient: [f32; 4],
    pub albedo: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct FroxelLight {
    pub pos_range: [f32; 4],
    pub dir_type: [f32; 4],
    pub color: [f32; 4],
    pub spot: [f32; 4],
    pub shadow: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct FogBox {
    pub world_to_box: [[f32; 4]; 4],
    pub half_ext: [f32; 4],
    pub albedo: [f32; 4],
}

/// `Froxels` in froxel_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct FroxelUniform {
    /// Inverse of the view-projection without the eye translation (eye-relative points).
    pub inv_view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    /// Sun cascades, nearest first (`counts.z` of them).
    pub sun_cascades: [[[f32; 4]; 4]; MAX_CASCADES],
    pub eye: [f32; 4],
    pub forward: [f32; 4],
    pub prev_eye: [f32; 4],
    pub prev_forward: [f32; 4],
    pub grid: [f32; 4],
    pub slices: [f32; 4],
    pub jitter: [f32; 4],
    pub height_fog: [f32; 4],
    pub albedo: [f32; 4],
    pub ambient: [f32; 4],
    pub noise: [f32; 4],
    pub scroll: [f32; 4],
    pub counts: [f32; 4],
    /// x = slot of the main sun in `lights` (-1 = none), y = 1 when the sharp shafts take it.
    pub sun: [f32; 4],
    pub lights: [FroxelLight; MAX_FOG_LIGHTS],
    pub local_shadow: [[[f32; 4]; 4]; MAX_LOCAL_LAYERS],
    pub boxes: [FogBox; MAX_FOG_BOXES],
}

/// Exponential depth slices of the froxel grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Slices {
    pub near: f32,
    pub far: f32,
    pub count: u32,
}

impl Slices {
    /// Slices reaching `far` (view depth); the first slice ends a little past `far / 128`.
    pub fn new(far: f32, count: u32) -> Self {
        Self {
            near: (far / 128.0).clamp(0.05, 1.0),
            far,
            count,
        }
    }

    pub fn log_ratio(&self) -> f32 {
        (self.far / self.near).ln()
    }

    /// View depth of slice coordinate `w` (0 = the eye); CPU copy of `slice_depth`.
    #[cfg(test)]
    pub fn depth(&self, w: f32) -> f32 {
        if w <= 0.0 {
            0.0
        } else {
            self.near * (self.log_ratio() * w).exp()
        }
    }

    /// Slice coordinate of view depth `d`; CPU copy of `depth_slice`.
    #[cfg(test)]
    pub fn coord(&self, d: f32) -> f32 {
        (d.max(self.near) / self.near).ln() / self.log_ratio()
    }

    fn pack(&self) -> [f32; 4] {
        [self.near, self.log_ratio(), self.far, self.count as f32]
    }
}

fn v4(v: Vec3, w: f32) -> [f32; 4] {
    v.extend(w).to_array()
}

fn linear(c: [f32; 3]) -> Vec3 {
    Vec3::from(srgb_to_linear(c))
}

/// Height-fog vec4 shared by both uniforms.
fn height_fog(fog: &FogSettings) -> [f32; 4] {
    let v = &fog.volume;
    [
        fog.extinction(),
        v.base_height,
        fog.inv_falloff(),
        fog.anisotropy(),
    ]
}

/// Sky ambient the fog scatters (average over the sphere of the scene's ambient light).
fn ambient(frame: &Frame, fog: &FogSettings) -> Vec3 {
    let [sky, equator, ground] = frame.environment.ambient();
    let intensity = frame
        .environment
        .settings
        .environment_lighting
        .intensity
        .max(0.0);
    (sky + equator * 2.0 + ground) * 0.25 * intensity * fog.volume.ambient_dimmer.max(0.0)
}

/// The brightest directional light (the sun of the analytic fog).
fn sun(lights: &[LightData]) -> Option<&LightData> {
    lights
        .iter()
        .filter(|l| l.kind == LightType::Directional && l.intensity > 0.0)
        .max_by(|a, b| a.intensity.total_cmp(&b.intensity))
}

pub(super) fn apply_uniform(
    frame: &Frame,
    fog: &FogSettings,
    mode: FogMode,
    slices: &Slices,
) -> ApplyUniform {
    let cam = frame.camera;
    let mode_index = match mode {
        FogMode::Off => 0.0,
        FogMode::Analytic => 1.0,
        FogMode::Volumetric => 2.0,
    };
    let (sun_dir, sun_color) = match sun(frame.lights) {
        Some(l) => (
            v4(-l.direction.normalize_or(Vec3::NEG_Y), 1.0),
            v4(
                linear(l.color) * l.intensity * l.volumetric.multiplier.max(0.0),
                0.0,
            ),
        ),
        None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4]),
    };
    ApplyUniform {
        mode: [
            mode_index,
            fog.debug.index(),
            slices.far,
            fog.max_distance(),
        ],
        slices: [slices.near, slices.log_ratio(), slices.count as f32, 0.0],
        forward: v4(cam.forward(), 0.0),
        height_fog: height_fog(fog),
        sun_dir,
        sun_color,
        ambient: v4(ambient(frame, fog), fog.volume.sky_haze.clamp(0.0, 1.0)),
        albedo: v4(fog.albedo(), 0.0),
    }
}

/// Last frame's camera, for reprojection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PrevView {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub forward: Vec3,
    pub slices: Slices,
    pub size: [u32; 3],
}

/// Halton(2, 3, 5) sample `i` (1-based) in 0..1.
pub(super) fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// Per-render froxel values; `history` = blend weight of last frame (0 = none).
pub(super) struct FroxelInputs<'a> {
    pub frame: &'a Frame<'a>,
    pub fog: &'a FogSettings,
    pub plan: &'a ShadowPlan,
    pub view_proj: Mat4,
    pub slices: Slices,
    pub size: [u32; 3],
    pub frame_index: u32,
    pub prev: Option<PrevView>,
    pub history: f32,
    pub time: f32,
    /// The sharp sun shafts march the main sun this frame (the froxels skip it).
    pub sharp: bool,
}

/// Index in `frame.lights` of the main sun: the directional light that owns the cascades,
/// else the brightest directional light.
pub(super) fn main_sun(frame: &Frame, plan: &ShadowPlan) -> Option<usize> {
    let owner = (0..frame.lights.len()).find(|i| shadow_access::light_layer(plan, *i) == Some(-1));
    owner.or_else(|| {
        let s = sun(frame.lights)?;
        frame.lights.iter().position(|l| std::ptr::eq(l, s))
    })
}

pub(super) fn froxel_uniform(i: &FroxelInputs) -> FroxelUniform {
    let mut u = FroxelUniform::zeroed();
    let (frame, fog) = (i.frame, i.fog);
    let cam = frame.camera;
    let ortho = matches!(
        cam.projection,
        crate::camera::Projection::Orthographic { .. }
    );
    // Without the eye translation (the shaders add the eye back): a full inverse
    // view-projection loses the far plane's precision with a large near / far ratio, and the
    // froxel rays would wobble by pixels from one camera orientation to the next.
    u.inv_view_proj = (i.view_proj * Mat4::from_translation(cam.position))
        .inverse()
        .to_cols_array_2d();
    u.eye = v4(cam.position, if ortho { 1.0 } else { 0.0 });
    u.forward = v4(cam.forward(), 0.0);
    let prev = i.prev.unwrap_or(PrevView {
        view_proj: i.view_proj,
        eye: cam.position,
        forward: cam.forward(),
        slices: i.slices,
        size: i.size,
    });
    u.prev_view_proj = prev.view_proj.to_cols_array_2d();
    u.prev_eye = v4(prev.eye, 0.0);
    u.prev_forward = v4(prev.forward, i.history);
    u.grid = [
        i.size[0] as f32,
        i.size[1] as f32,
        i.size[2] as f32,
        i.frame_index as f32,
    ];
    u.slices = i.slices.pack();
    // Halton jitter inside each froxel; the temporal blend averages it out.
    u.jitter = if fog.volume.denoising {
        let n = i.frame_index % 16 + 1;
        [halton(n, 2), halton(n, 3), halton(n, 5), 0.0]
    } else {
        [0.5, 0.5, 0.5, 0.0]
    };
    u.height_fog = height_fog(fog);
    u.albedo = v4(fog.albedo(), 1.0);
    u.ambient = v4(ambient(frame, fog), 0.0);
    let v = &fog.volume;
    u.noise = [
        v.noise_intensity.clamp(0.0, 1.0),
        1.0 / v.noise_scale.max(0.01),
        i.time,
        0.0,
    ];
    u.scroll = [v.noise_scroll[0], v.noise_scroll[1], v.noise_scroll[2], 0.0];

    let cascades = shadow_access::sun_cascades(i.plan);
    for (slot, m) in cascades.iter().enumerate() {
        u.sun_cascades[slot] = m.to_cols_array_2d();
    }
    u.counts[2] = cascades.len() as f32;
    for (slot, m) in shadow_access::local_matrices(i.plan)
        .iter()
        .take(MAX_LOCAL_LAYERS)
        .enumerate()
    {
        u.local_shadow[slot] = m.to_cols_array_2d();
    }
    let main = main_sun(frame, i.plan);
    u.sun = [-1.0, 0.0, 0.0, 0.0];
    let mut lights = 0;
    for (index, l) in frame.lights.iter().enumerate() {
        if lights == MAX_FOG_LIGHTS {
            break;
        }
        if let Some(item) = froxel_light(l, shadow_access::light_layer(i.plan, index)) {
            if main == Some(index) {
                u.sun = [lights as f32, if i.sharp { 1.0 } else { 0.0 }, 0.0, 0.0];
            }
            u.lights[lights] = item;
            lights += 1;
        }
    }
    let mut boxes = 0;
    for b in frame.environment.fog_volumes.iter().take(MAX_FOG_BOXES) {
        u.boxes[boxes] = FogBox {
            world_to_box: b.world_to_box.to_cols_array_2d(),
            half_ext: v4(b.half_extents, b.extinction),
            albedo: v4(b.albedo, b.blend),
        };
        boxes += 1;
    }
    u.counts[0] = lights as f32;
    u.counts[1] = boxes as f32;
    u
}

/// One light of the froxel pass (`None` when it adds nothing to the fog).
fn froxel_light(l: &LightData, layer: Option<i32>) -> Option<FroxelLight> {
    let strength = l.intensity * l.volumetric.multiplier;
    if strength <= 0.0 {
        return None;
    }
    let kind = crate::render::light_kind_index(l.kind);
    let half = (l.spot_angle * 0.5).to_radians();
    let spot = if l.kind == LightType::Rectangle {
        // Area and sides of the rectangle (froxel_light.wgsl).
        let a = &l.area;
        let sides = if a.two_sided { 1.0 } else { 0.0 };
        [a.size.x.max(0.0) * a.size.y.max(0.0), sides, 0.0, 0.0]
    } else {
        [half.cos(), (half * 0.8).cos(), 0.0, 0.0]
    };
    let shadowed = layer.is_some() && l.shadows != Shadows::None;
    Some(FroxelLight {
        pos_range: v4(l.position, l.range),
        dir_type: v4(l.direction.normalize_or(Vec3::Z), kind),
        color: v4(linear(l.color) * strength, 0.0),
        spot,
        shadow: [
            if shadowed { 1.0 } else { 0.0 },
            layer.unwrap_or(0) as f32,
            l.volumetric.shadow_dimmer.clamp(0.0, 1.0),
            0.0,
        ],
    })
}
