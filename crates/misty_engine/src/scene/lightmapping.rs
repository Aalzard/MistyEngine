//! Lightmapping settings of a scene (Lighting window ▸ Scene) and the per-light / per-renderer
//! values the lightmapper reads: Light Mode, Contribute GI, Receive GI, Scale In Lightmap and
//! the baked lightmap index / scale-offset (see [`crate::lightmap`]).

use serde::{Deserialize, Serialize};

/// How a light takes part in baking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightMode {
    /// Lit every frame; not baked.
    #[default]
    Realtime,
    /// Realtime direct light, baked indirect (bounce) light.
    Mixed,
    /// Direct and indirect light baked into lightmaps; no realtime light once the scene has
    /// baked lightmaps.
    Baked,
}

impl LightMode {
    pub const ALL: [LightMode; 3] = [Self::Realtime, Self::Mixed, Self::Baked];

    pub fn label(self) -> &'static str {
        match self {
            Self::Realtime => "Realtime",
            Self::Mixed => "Mixed",
            Self::Baked => "Baked",
        }
    }

    pub fn is_realtime(m: &LightMode) -> bool {
        *m == Self::Realtime
    }
}

/// Where a renderer that contributes GI gets its own indirect light from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiveGi {
    /// Baked into lightmaps (static objects).
    #[default]
    Lightmaps,
    /// Lit like a dynamic object (ambient / probes); still bounces light onto others.
    LightProbes,
}

impl ReceiveGi {
    pub const ALL: [ReceiveGi; 2] = [Self::Lightmaps, Self::LightProbes];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lightmaps => "Lightmaps",
            Self::LightProbes => "Light Probes",
        }
    }

    pub fn is_lightmaps(r: &ReceiveGi) -> bool {
        *r == Self::Lightmaps
    }
}

/// What Mixed lights bake.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MixedLightingMode {
    /// Realtime direct light and shadows, baked indirect light.
    #[default]
    BakedIndirect,
}

impl MixedLightingMode {
    pub const ALL: [MixedLightingMode; 1] = [Self::BakedIndirect];

    pub fn label(self) -> &'static str {
        match self {
            Self::BakedIndirect => "Baked Indirect",
        }
    }
}

/// Lighting window ▸ Scene ▸ Lightmapping Settings, plus the lightmaps of the last bake.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightmapSettings {
    /// Baked Global Illumination on (Generate Lighting bakes lightmaps).
    pub baked_gi: bool,
    pub mixed_mode: MixedLightingMode,
    /// Lightmap texels per world unit.
    pub resolution: f32,
    /// Largest lightmap side in texels (more lightmaps are made when the scene doesn't fit).
    pub max_size: u32,
    /// Texels between renderers in a lightmap.
    pub padding: u32,
    /// Paths traced per texel.
    pub samples: u32,
    /// Indirect bounces (1–4).
    pub bounces: u32,
    /// Multiplier on baked indirect light.
    pub indirect_intensity: f32,
    /// Project paths of the baked lightmaps (`Assets/Scenes/<Scene>/Lightmap-N.lightmap`),
    /// indexed by `MeshRenderer::lightmap_index`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lightmaps: Vec<String>,
}

pub const MAX_SIZES: [u32; 6] = [64, 128, 256, 512, 1024, 2048];

impl Default for LightmapSettings {
    fn default() -> Self {
        Self {
            baked_gi: true,
            mixed_mode: MixedLightingMode::BakedIndirect,
            resolution: 10.0,
            max_size: 1024,
            padding: 2,
            samples: 64,
            bounces: 2,
            indirect_intensity: 1.0,
            lightmaps: Vec::new(),
        }
    }
}

impl LightmapSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Values in their working ranges.
    pub fn sanitized(&self) -> Self {
        let mut s = self.clone();
        if !s.resolution.is_finite() {
            s.resolution = 10.0;
        }
        s.resolution = s.resolution.clamp(0.1, 200.0);
        s.max_size = s.max_size.clamp(32, 4096);
        s.padding = s.padding.min(16);
        s.samples = s.samples.clamp(1, 4096);
        s.bounces = s.bounces.clamp(0, 4);
        if !s.indirect_intensity.is_finite() {
            s.indirect_intensity = 1.0;
        }
        s.indirect_intensity = s.indirect_intensity.clamp(0.0, 10.0);
        s
    }
}

pub(crate) fn is_one(v: &f32) -> bool {
    *v == 1.0
}

pub(crate) fn is_no_lightmap(v: &i32) -> bool {
    *v < 0
}

pub(crate) fn is_identity_st(v: &[f32; 4]) -> bool {
    *v == IDENTITY_ST
}

pub(crate) fn is_yes(v: &bool) -> bool {
    *v
}

/// Scale-offset that maps uv2 onto a whole lightmap.
pub const IDENTITY_ST: [f32; 4] = [1.0, 1.0, 0.0, 0.0];
