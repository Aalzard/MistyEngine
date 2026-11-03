//! Per-view fog values (from the Post-process Volume's Fog / Sun Shafts overrides) and the
//! local Fog Volumes gathered from the scene.

use glam::{Mat4, Vec3};

use crate::render::srgb_to_linear;
use crate::scene::{FogOverride, FogQuality, FogVolume, SunShaftsOverride};

/// Scene-view debug output of the fog (Scene view draw-mode menu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FogDebug {
    #[default]
    Off,
    /// 1 - transmittance (how much fog is in front of each pixel).
    Density,
    /// In-scattered light only.
    Lighting,
}

impl FogDebug {
    pub(super) fn index(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Density => 1.0,
            Self::Lighting => 2.0,
        }
    }
}

/// Fog of one view. `Default` = no fog (the baseline every scene gets without an override).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FogSettings {
    /// The override's values; only used when `active` and `enabled` are both set.
    pub volume: FogOverride,
    pub debug: FogDebug,
}

/// How the fog is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FogMode {
    Off,
    Analytic,
    Volumetric,
}

impl FogSettings {
    pub fn from_override(o: &FogOverride) -> Self {
        Self {
            volume: *o,
            debug: FogDebug::Off,
        }
    }

    pub fn is_on(&self) -> bool {
        self.volume.active && self.volume.enabled
    }

    pub(crate) fn mode(&self) -> FogMode {
        match (self.is_on(), self.volume.volumetric) {
            (false, _) => FogMode::Off,
            (true, false) => FogMode::Analytic,
            (true, true) => FogMode::Volumetric,
        }
    }

    /// Extinction (1/m) at and below the base height.
    pub(crate) fn extinction(&self) -> f32 {
        1.0 / self.volume.attenuation_distance.max(0.01)
    }

    /// 1 / height scale of the exponential falloff above the base height: the density drops to
    /// e^-2 (about an eighth) at the maximum height.
    pub(crate) fn inv_falloff(&self) -> f32 {
        let span = (self.volume.maximum_height - self.volume.base_height).max(0.01);
        2.0 / span
    }

    /// Linear albedo (scattering / extinction).
    pub(crate) fn albedo(&self) -> Vec3 {
        Vec3::from(srgb_to_linear(self.volume.albedo)).clamp(Vec3::ZERO, Vec3::ONE)
    }

    pub(crate) fn anisotropy(&self) -> f32 {
        self.volume.anisotropy.clamp(-0.95, 0.95)
    }

    /// Reach of the froxel volume (never past the fog's own max distance).
    pub(crate) fn volumetric_distance(&self) -> f32 {
        self.volume
            .volumetric_distance
            .clamp(1.0, self.max_distance())
    }

    pub(crate) fn max_distance(&self) -> f32 {
        self.volume.max_fog_distance.max(1.0)
    }
}

/// Froxel grid size (width, height, depth slices) per quality.
pub(crate) fn grid_size(q: FogQuality) -> [u32; 3] {
    match q {
        FogQuality::Low => [96, 54, 48],
        FogQuality::Medium => [128, 72, 64],
        FogQuality::High => [160, 90, 64],
    }
}

/// A light's volumetric values (Light Inspector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumetricLight {
    /// Scales the light inside the fog.
    pub multiplier: f32,
    /// 0..1: how dark its shadows are inside the fog.
    pub shadow_dimmer: f32,
}

impl Default for VolumetricLight {
    fn default() -> Self {
        Self {
            multiplier: 1.0,
            shadow_dimmer: 1.0,
        }
    }
}

/// Screen-space sun shafts of one view (inactive = off).
pub type SunShaftSettings = SunShaftsOverride;

/// A Fog Volume ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalFog {
    /// World → box space, where the box is [-1, 1]³.
    pub world_to_box: Mat4,
    /// Half size of the box in world units along its own axes.
    pub half_extents: Vec3,
    /// 1 / mean free path.
    pub extinction: f32,
    /// Linear.
    pub albedo: Vec3,
    /// Soft edge width (metres).
    pub blend: f32,
}

impl LocalFog {
    /// `world` = the object's world matrix.
    pub fn new(v: &FogVolume, world: Mat4) -> Self {
        let half = Vec3::from(v.size).abs().max(Vec3::splat(1e-3)) * 0.5;
        let model = world * Mat4::from_scale(half);
        Self {
            world_to_box: model.inverse(),
            half_extents: Vec3::new(
                model.x_axis.truncate().length(),
                model.y_axis.truncate().length(),
                model.z_axis.truncate().length(),
            ),
            extinction: 1.0 / v.fog_distance.max(0.01),
            albedo: Vec3::from(srgb_to_linear(v.albedo)).clamp(Vec3::ZERO, Vec3::ONE),
            blend: v.blend_distance.max(0.0),
        }
    }

    /// 0..1 density weight at a world point (1 inside, fading over the blend width).
    pub fn weight(&self, p: Vec3) -> f32 {
        let b = self.world_to_box.transform_point3(p).abs();
        let inside = (Vec3::ONE - b) * self.half_extents;
        let f = (inside / self.blend.max(1e-3)).clamp(Vec3::ZERO, Vec3::ONE);
        if b.max_element() > 1.0 {
            0.0
        } else {
            f.x * f.y * f.z
        }
    }
}
