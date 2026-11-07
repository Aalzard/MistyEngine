//! Project Settings ▸ Quality: named levels (Low … Ultra) of rendering settings, one of them the
//! default, plus the level the running game or editor preview uses right now.
//!
//! The renderer reads the active level through [`active`] (or the copy handed to
//! `Renderer::set_quality`); the accessor methods on [`QualityLevel`] turn the menu choices into
//! the numbers a render pass needs.

use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Shadow map resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowResolution {
    Low,
    Medium,
    #[default]
    High,
    VeryHigh,
}

impl ShadowResolution {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::VeryHigh];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low (512)",
            Self::Medium => "Medium (1024)",
            Self::High => "High (2048)",
            Self::VeryHigh => "Very High (4096)",
        }
    }

    pub fn pixels(self) -> u32 {
        match self {
            Self::Low => 512,
            Self::Medium => 1024,
            Self::High => 2048,
            Self::VeryHigh => 4096,
        }
    }
}

/// Off / Low / Medium / High for soft shadows and screen effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

impl Tier {
    pub const ALL: [Self; 4] = [Self::Off, Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// 0 (Off) to 3 (High).
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// Multisample anti-aliasing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiAliasing {
    Off,
    X2,
    #[default]
    X4,
}

impl AntiAliasing {
    pub const ALL: [Self; 3] = [Self::Off, Self::X2, Self::X4];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Disabled",
            Self::X2 => "2x Multi Sampling",
            Self::X4 => "4x Multi Sampling",
        }
    }

    pub fn samples(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::X2 => 2,
            Self::X4 => 4,
        }
    }
}

/// How the image rendered at Render Scale is stretched to the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpscaleFilter {
    /// Smooth.
    #[default]
    Bilinear,
    /// Hard pixels (pixel-art look at low Render Scale).
    Nearest,
    /// Bilinear after a contrast-adaptive sharpening pass.
    Sharpen,
}

impl UpscaleFilter {
    pub const ALL: [Self; 3] = [Self::Bilinear, Self::Nearest, Self::Sharpen];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bilinear => "Bilinear",
            Self::Nearest => "Nearest (pixelated)",
            Self::Sharpen => "Sharpen",
        }
    }

    /// Whether the stretch samples the nearest texel instead of blending four.
    pub fn nearest(self) -> bool {
        self == Self::Nearest
    }
}

/// Temporal upscaling (Quality ▸ Upscaling): the scene renders at a share of the output
/// resolution with a sub-pixel jitter, and the temporal resolve rebuilds the full resolution
/// from the jittered frames (render/taa.rs). Post-processing and the UI run at the output
/// resolution. Turns temporal anti-aliasing on (the upscaler is TAA at a lower resolution).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upscaling {
    /// Native resolution.
    #[default]
    Off,
    /// 67 % of the output resolution per side (≈ 45 % of the pixels).
    Quality,
    /// 58 % per side (≈ 34 % of the pixels).
    Balanced,
    /// 50 % per side (25 % of the pixels).
    Performance,
    /// [`QualityLevel::upscaling_scale`] per side.
    Custom,
}

impl Upscaling {
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Quality => "Quality (67%)",
            Self::Balanced => "Balanced (58%)",
            Self::Performance => "Performance (50%)",
            Self::Custom => "Custom",
        }
    }

    /// Share of the output resolution rendered per side (`custom` for Custom; 1 = Off).
    pub fn scale(self, custom: f32) -> f32 {
        match self {
            Self::Off => 1.0,
            Self::Quality => 0.67,
            Self::Balanced => 0.58,
            Self::Performance => 0.5,
            Self::Custom => custom.clamp(UPSCALING_MIN, 1.0),
        }
    }
}

/// Smallest Custom upscaling scale.
pub const UPSCALING_MIN: f32 = 0.33;

/// Which upscaler rebuilds the output resolution. Only the built-in temporal upscaler exists;
/// vendor upscalers (FSR, DLSS, XeSS) would be further variants taking the same inputs
/// (jittered colour, depth, motion vectors, jitter, reset), see render/taa.rs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpscalerBackend {
    /// Misty's temporal upscaler (render/taa.rs, `fs_taau`).
    #[default]
    Temporal,
}

impl UpscalerBackend {
    pub const ALL: [Self; 1] = [Self::Temporal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Temporal => "Misty Temporal",
        }
    }
}

/// Texture Quality (the global mipmap limit): how many top mip levels are skipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureQuality {
    #[default]
    Full,
    Half,
    Quarter,
    Eighth,
}

impl TextureQuality {
    pub const ALL: [Self; 4] = [Self::Full, Self::Half, Self::Quarter, Self::Eighth];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full Resolution",
            Self::Half => "Half Resolution",
            Self::Quarter => "Quarter Resolution",
            Self::Eighth => "Eighth Resolution",
        }
    }

    /// Mip levels dropped (each one halves the width and height).
    pub fn mip_limit(self) -> u32 {
        self as u32
    }
}

/// Anisotropic Textures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anisotropic {
    Disabled,
    #[default]
    PerTexture,
    ForcedOn,
}

impl Anisotropic {
    pub const ALL: [Self; 3] = [Self::Disabled, Self::PerTexture, Self::ForcedOn];

    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::PerTexture => "Per Texture",
            Self::ForcedOn => "Forced On",
        }
    }
}

/// Default share of the newest frame in the TAA blend (about ten frames of history).
pub const TAA_BLEND: f32 = 0.1;
/// Default sharpening of the TAA history.
pub const TAA_SHARPNESS: f32 = 0.4;

/// One quality level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityLevel {
    pub name: String,
    pub shadow_resolution: ShadowResolution,
    /// Shadows are drawn up to this many metres from the camera.
    pub shadow_distance: f32,
    /// Directional shadow cascades: 1, 2 or 4.
    pub shadow_cascades: u32,
    /// Where cascades split: 0 = even, 1 = logarithmic (more resolution near the camera).
    pub cascade_split: f32,
    /// Fraction of each cascade blended into the next, hiding the seam.
    pub cascade_blend: f32,
    /// Soft shadow filtering (Off = hard edges).
    pub soft_shadows: Tier,
    /// Soft shadows accumulate over frames (a sample pattern that moves every frame, blended
    /// with the reprojected history): many more effective samples, no visible pattern.
    pub shadow_temporal: bool,
    /// Edge-aware screen-space blur of soft shadows, as wide as half their penumbra.
    pub shadow_denoise: bool,
    pub anti_aliasing: AntiAliasing,
    /// FXAA post pass on the final image (smooths edges MSAA misses: alpha cut-outs, shading).
    pub fxaa: bool,
    /// Temporal anti-aliasing: jittered frames blended over time (smooth edges, thin grass
    /// and foliage that stop crawling), reprojected through motion vectors.
    pub temporal_aa: bool,
    /// TAA: share of the newest frame in the blend (0.02–0.5; lower = smoother and slower to
    /// react, higher = crisper and noisier).
    pub taa_blend: f32,
    /// TAA: how much the reprojected history is sharpened (0–1) against the softening of
    /// blending and resampling.
    pub taa_sharpness: f32,
    pub texture_quality: TextureQuality,
    pub anisotropic_textures: Anisotropic,
    /// Largest texture side on the GPU, whatever the textures' own Max Size.
    pub max_texture_size: u32,
    /// The 3D image is rendered at this fraction of the window size (0.25–2).
    pub render_scale: f32,
    /// How the Render Scale image is stretched to the window.
    pub upscale_filter: UpscaleFilter,
    /// Temporal upscaling: the scene renders at a share of the output resolution and the
    /// temporal resolve rebuilds the rest (Off = native).
    pub upscaling: Upscaling,
    /// Share of the output resolution per side for Upscaling ▸ Custom (0.33–1).
    pub upscaling_scale: f32,
    /// Which upscaler runs (only the built-in one so far).
    pub upscaler: UpscalerBackend,
    /// Multiplier on LOD Group switch distances (`RenderOptions::lod_bias`).
    pub lod_bias: f32,
    pub ambient_occlusion: Tier,
    pub volumetric_fog: Tier,
    pub reflection_probes: Tier,
    /// Lens Flare components draw (light halos always do: they are cheap).
    pub lens_flares: bool,
    /// Volumetric clouds: trace resolution and step counts (Off = no clouds).
    pub volumetric_clouds: Tier,
    /// Sharp volumetric sun shafts: resolution and steps of the per-pixel sun march (Off =
    /// the sun stays in the froxel fog).
    pub sun_shafts: Tier,
    /// Terrain grass casts shadows near the camera: how far and how many blades (Off = no
    /// grass shadow casters; grass still receives shadows).
    pub grass_shadows: Tier,
    /// A dedicated sharp shadow map of the grass around the camera (Off, 1024, 2048, 4096 px
    /// over the nearest 12 m; full blade density).
    pub grass_shadow_map: Tier,
    /// Share of each terrain's grass blades drawn (0.1..1); the blades left out make the
    /// others wider, so the ground stays covered.
    pub grass_density: f32,
}

impl Default for QualityLevel {
    fn default() -> Self {
        Self::high()
    }
}

impl QualityLevel {
    pub fn low() -> Self {
        Self {
            name: "Low".into(),
            shadow_resolution: ShadowResolution::Low,
            shadow_distance: 40.0,
            shadow_cascades: 1,
            cascade_split: 0.75,
            cascade_blend: 0.1,
            soft_shadows: Tier::Off,
            shadow_temporal: false,
            shadow_denoise: false,
            anti_aliasing: AntiAliasing::Off,
            fxaa: false,
            temporal_aa: false,
            taa_blend: TAA_BLEND,
            taa_sharpness: TAA_SHARPNESS,
            texture_quality: TextureQuality::Half,
            anisotropic_textures: Anisotropic::Disabled,
            max_texture_size: 1024,
            render_scale: 0.75,
            upscale_filter: UpscaleFilter::Bilinear,
            upscaling: Upscaling::Off,
            upscaling_scale: 0.75,
            upscaler: UpscalerBackend::Temporal,
            lod_bias: 0.5,
            ambient_occlusion: Tier::Off,
            volumetric_fog: Tier::Off,
            reflection_probes: Tier::Low,
            lens_flares: false,
            volumetric_clouds: Tier::Low,
            sun_shafts: Tier::Off,
            grass_shadows: Tier::Off,
            grass_shadow_map: Tier::Off,
            grass_density: 0.5,
        }
    }

    pub fn medium() -> Self {
        Self {
            name: "Medium".into(),
            shadow_resolution: ShadowResolution::Medium,
            shadow_distance: 70.0,
            shadow_cascades: 2,
            cascade_split: 0.75,
            cascade_blend: 0.1,
            soft_shadows: Tier::Low,
            shadow_temporal: false,
            shadow_denoise: false,
            anti_aliasing: AntiAliasing::X2,
            fxaa: false,
            temporal_aa: true,
            taa_blend: TAA_BLEND,
            taa_sharpness: TAA_SHARPNESS,
            texture_quality: TextureQuality::Full,
            anisotropic_textures: Anisotropic::PerTexture,
            max_texture_size: 2048,
            render_scale: 1.0,
            upscale_filter: UpscaleFilter::Bilinear,
            upscaling: Upscaling::Off,
            upscaling_scale: 0.75,
            upscaler: UpscalerBackend::Temporal,
            lod_bias: 1.0,
            ambient_occlusion: Tier::Low,
            volumetric_fog: Tier::Low,
            reflection_probes: Tier::Medium,
            lens_flares: true,
            volumetric_clouds: Tier::Medium,
            sun_shafts: Tier::Medium,
            grass_shadows: Tier::Low,
            grass_shadow_map: Tier::Low,
            grass_density: 0.75,
        }
    }

    pub fn high() -> Self {
        Self {
            name: "High".into(),
            shadow_resolution: ShadowResolution::High,
            shadow_distance: 150.0,
            shadow_cascades: 4,
            cascade_split: 0.75,
            cascade_blend: 0.1,
            soft_shadows: Tier::Medium,
            shadow_temporal: true,
            shadow_denoise: true,
            anti_aliasing: AntiAliasing::X4,
            fxaa: false,
            temporal_aa: true,
            taa_blend: TAA_BLEND,
            taa_sharpness: TAA_SHARPNESS,
            texture_quality: TextureQuality::Full,
            anisotropic_textures: Anisotropic::ForcedOn,
            max_texture_size: 4096,
            render_scale: 1.0,
            upscale_filter: UpscaleFilter::Bilinear,
            upscaling: Upscaling::Off,
            upscaling_scale: 0.75,
            upscaler: UpscalerBackend::Temporal,
            lod_bias: 1.5,
            ambient_occlusion: Tier::Medium,
            volumetric_fog: Tier::Medium,
            reflection_probes: Tier::High,
            lens_flares: true,
            volumetric_clouds: Tier::Medium,
            sun_shafts: Tier::Medium,
            grass_shadows: Tier::Medium,
            grass_shadow_map: Tier::Medium,
            grass_density: 1.0,
        }
    }

    pub fn ultra() -> Self {
        Self {
            name: "Ultra".into(),
            shadow_resolution: ShadowResolution::VeryHigh,
            shadow_distance: 250.0,
            soft_shadows: Tier::High,
            max_texture_size: 8192,
            lod_bias: 2.0,
            ambient_occlusion: Tier::High,
            volumetric_fog: Tier::High,
            volumetric_clouds: Tier::High,
            sun_shafts: Tier::High,
            grass_shadows: Tier::High,
            grass_shadow_map: Tier::High,
            grass_density: 1.0,
            ..Self::high()
        }
    }

    /// Keep values in the ranges the renderer supports.
    pub fn sanitize(&mut self) {
        if self.name.trim().is_empty() {
            self.name = "Level".into();
        }
        self.shadow_distance = finite_clamp(self.shadow_distance, 1.0, 10000.0, 150.0);
        self.shadow_cascades = match self.shadow_cascades {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        self.cascade_split = finite_clamp(self.cascade_split, 0.0, 1.0, 0.75);
        self.cascade_blend = finite_clamp(self.cascade_blend, 0.0, 0.5, 0.1);
        self.max_texture_size = self.max_texture_size.clamp(32, 8192);
        self.render_scale = finite_clamp(self.render_scale, 0.25, 2.0, 1.0);
        self.upscaling_scale = finite_clamp(self.upscaling_scale, UPSCALING_MIN, 1.0, 0.75);
        self.lod_bias = finite_clamp(self.lod_bias, 0.01, 10.0, 1.0);
        self.taa_blend = finite_clamp(self.taa_blend, 0.02, 0.5, TAA_BLEND);
        self.taa_sharpness = finite_clamp(self.taa_sharpness, 0.0, 1.0, TAA_SHARPNESS);
    }

    // ---- what the renderer reads ------------------------------------------------------------

    /// Side of the directional shadow map in pixels.
    pub fn shadow_map_size(&self) -> u32 {
        self.shadow_resolution.pixels()
    }

    /// Side of each spot/point light shadow map (half the directional one).
    pub fn local_shadow_map_size(&self) -> u32 {
        (self.shadow_map_size() / 2).max(256)
    }

    /// What the shadow pass reads: cascades, distance, split, blend, map size and soft-shadow taps.
    pub fn shadow_settings(&self) -> ShadowSettings {
        ShadowSettings {
            soft: self.soft_shadows,
            cascades: self.shadow_cascades,
            distance: self.shadow_distance,
            split_lambda: self.cascade_split,
            cascade_blend: self.cascade_blend,
            map_size: self.shadow_map_size(),
            temporal: self.shadow_temporal,
            denoise: self.shadow_denoise,
        }
    }

    /// Share of the output resolution the 3D views render at before temporal upscaling
    /// (1 = native, no upscaler).
    pub fn upscale_ratio(&self) -> f32 {
        self.upscaling.scale(self.upscaling_scale)
    }

    /// The temporal resolve runs: TAA on, or the upscaler (which is TAA at a lower
    /// resolution).
    pub fn temporal(&self) -> bool {
        self.temporal_aa || self.upscaling != Upscaling::Off
    }

    pub fn msaa_samples(&self) -> u32 {
        self.anti_aliasing.samples()
    }

    /// Largest texture side after Texture Quality and Max Texture Size, for an image whose own
    /// import Max Size is `import_max`.
    pub fn texture_size_limit(&self, import_max: u32) -> u32 {
        (import_max.min(self.max_texture_size) >> self.texture_quality.mip_limit()).max(1)
    }

    /// Anisotropy clamp for a texture with import Aniso Level `level` (1 = off).
    pub fn anisotropy(&self, level: u8) -> u16 {
        let level = u16::from(level.clamp(1, 16));
        match self.anisotropic_textures {
            Anisotropic::Disabled => 1,
            Anisotropic::PerTexture => level,
            Anisotropic::ForcedOn => level.max(8),
        }
    }

    /// Ambient occlusion sample count (0 = off).
    pub fn ao_samples(&self) -> u32 {
        [0, 4, 8, 16][self.ambient_occlusion.index() as usize]
    }

    /// Depth slices of the volumetric fog grid (0 = off).
    pub fn fog_slices(&self) -> u32 {
        [0, 32, 64, 128][self.volumetric_fog.index() as usize]
    }

    /// Grass shadow casters: range in metres from the camera and the share of the drawn blades
    /// that cast (None = off).
    pub fn grass_shadow_casters(&self) -> Option<(f32, f32)> {
        match self.grass_shadows {
            Tier::Off => None,
            Tier::Low => Some((12.0, 0.35)),
            Tier::Medium => Some((20.0, 0.5)),
            Tier::High => Some((32.0, 0.6)),
        }
    }

    /// Side of the near-camera grass shadow map in pixels (1 = off).
    pub fn grass_shadow_map_size(&self) -> u32 {
        [1, 1024, 2048, 4096][self.grass_shadow_map.index() as usize]
    }

    /// Share of the grass blades drawn (see [`QualityLevel::grass_density`]).
    pub fn grass_density(&self) -> f32 {
        self.grass_density.clamp(0.1, 1.0)
    }

    /// Cube face size of baked/realtime reflection probes (0 = probes off).
    pub fn reflection_probe_size(&self) -> u32 {
        [0, 64, 128, 256][self.reflection_probes.index() as usize]
    }
}

/// A level's shadow values as the cascade fit and the soft-shadow pass use them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowSettings {
    /// Soft-shadow filtering; Off draws Soft lights with hard edges.
    pub soft: Tier,
    /// Directional light cascades (1–4).
    pub cascades: u32,
    /// Metres from the camera that the sun's shadows reach (they fade out over the last 10%).
    pub distance: f32,
    /// 0 = uniform splits, 1 = logarithmic; values between mix the two (practical split).
    pub split_lambda: f32,
    /// Fraction of each cascade blended into the next one.
    pub cascade_blend: f32,
    /// Side of each sun cascade's map in pixels.
    pub map_size: u32,
    /// Accumulate soft shadows over frames (render/shadow_denoise).
    pub temporal: bool,
    /// Edge-aware screen-space blur of soft shadows.
    pub denoise: bool,
}

impl Default for ShadowSettings {
    /// The High level's shadows.
    fn default() -> Self {
        QualityLevel::high().shadow_settings()
    }
}

impl ShadowSettings {
    /// (blocker search samples, filter samples) per soft-shadowed light.
    pub fn samples(&self) -> (u32, u32) {
        match self.soft {
            Tier::Off | Tier::Low => (8, 12),
            Tier::Medium => (12, 20),
            Tier::High => (16, 32),
        }
    }

    /// (widest, narrowest) soft filter radius in shadow-map texels. Higher tiers allow wider
    /// penumbrae (their noise is averaged away by more taps and the temporal mask).
    pub fn filter_texels(&self) -> (f32, f32) {
        match self.soft {
            Tier::Off | Tier::Low => (48.0, 1.5),
            Tier::Medium => (64.0, 1.5),
            Tier::High => (96.0, 1.5),
        }
    }

    /// The temporal / denoised shadow mask runs (soft shadows on and either option set).
    pub fn mask(&self) -> bool {
        self.soft != Tier::Off && (self.temporal || self.denoise)
    }
}

fn finite_clamp(v: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}

/// Every level and the one new players start on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QualitySettings {
    pub levels: Vec<QualityLevel>,
    /// Index into `levels`.
    pub default_level: usize,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            levels: vec![
                QualityLevel::low(),
                QualityLevel::medium(),
                QualityLevel::high(),
                QualityLevel::ultra(),
            ],
            default_level: 2,
        }
    }
}

impl QualitySettings {
    pub fn sanitize(&mut self) {
        if self.levels.is_empty() {
            *self = Self::default();
        }
        for l in &mut self.levels {
            l.sanitize();
        }
        self.default_level = self.default_level.min(self.levels.len() - 1);
    }

    pub fn names(&self) -> Vec<String> {
        self.levels.iter().map(|l| l.name.clone()).collect()
    }

    /// The level at `index`, or the default one.
    pub fn level(&self, index: usize) -> &QualityLevel {
        self.levels
            .get(index)
            .or_else(|| self.levels.get(self.default_level))
            .expect("sanitized settings have levels")
    }
}

// ---- the active level (process-wide) ------------------------------------------------------

struct Active {
    settings: QualitySettings,
    index: usize,
}

static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);
/// Bumped whenever the active level changes, so caches (textures) know to reload.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Install a project's levels and make `index` (clamped) the active one.
pub fn install(settings: &QualitySettings, index: usize) {
    let mut s = settings.clone();
    s.sanitize();
    let index = index.min(s.levels.len() - 1);
    let mut a = ACTIVE.write().unwrap_or_else(|e| e.into_inner());
    let changed = a
        .as_ref()
        .is_none_or(|old| old.settings.level(old.index) != s.level(index));
    *a = Some(Active { settings: s, index });
    if changed {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// `QualitySettings.SetQualityLevel(i)`: false when `index` is out of range.
pub fn set_level(index: usize) -> bool {
    let settings = match ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(a) if index < a.settings.levels.len() => a.settings.clone(),
        Some(_) => return false,
        None if index < QualitySettings::default().levels.len() => QualitySettings::default(),
        None => return false,
    };
    install(&settings, index);
    true
}

/// Index of the active level.
pub fn level() -> usize {
    match ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(a) => a.index,
        None => QualitySettings::default().default_level,
    }
}

/// Names of the installed levels.
pub fn names() -> Vec<String> {
    match ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(a) => a.settings.names(),
        None => QualitySettings::default().names(),
    }
}

/// The active level's settings (High when nothing was installed).
pub fn active() -> QualityLevel {
    match ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(a) => a.settings.level(a.index).clone(),
        None => QualityLevel::default(),
    }
}

/// Changes whenever [`active`] does.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_four_levels_and_high_is_default() {
        let q = QualitySettings::default();
        assert_eq!(q.names(), ["Low", "Medium", "High", "Ultra"]);
        assert_eq!(q.level(q.default_level).name, "High");
        assert_eq!(q.level(99).name, "High", "out of range = default");
    }

    #[test]
    fn accessors_turn_choices_into_numbers() {
        let low = QualityLevel::low();
        assert_eq!(low.shadow_map_size(), 512);
        assert_eq!(low.msaa_samples(), 1);
        assert_eq!(low.ao_samples(), 0);
        assert_eq!(low.texture_size_limit(2048), 512, "1024 cap, then half");
        assert_eq!(low.anisotropy(16), 1);
        let high = QualityLevel::high();
        assert_eq!(high.anisotropy(1), 8, "forced on");
        assert_eq!(QualityLevel::medium().anisotropy(4), 4);
        assert_eq!(QualityLevel::ultra().shadow_map_size(), 4096);
        assert_eq!(high.texture_size_limit(256), 256);
    }

    #[test]
    fn sanitize_fixes_bad_values_and_old_files_load() {
        let mut q: QualitySettings = serde_json::from_str(
            r#"{ "levels": [{ "name": "", "shadow_cascades": 3, "render_scale": 9 }],
                 "default_level": 5 }"#,
        )
        .unwrap();
        q.sanitize();
        assert_eq!(q.levels[0].name, "Level");
        assert_eq!(q.levels[0].shadow_cascades, 2);
        assert_eq!(q.levels[0].render_scale, 2.0);
        assert_eq!(q.levels[0].upscale_filter, UpscaleFilter::Bilinear);
        assert!(!q.levels[0].fxaa);
        assert_eq!(q.levels[0].taa_blend, TAA_BLEND);
        assert_eq!(q.default_level, 0);
        let mut taa: QualityLevel =
            serde_json::from_str(r#"{ "taa_blend": 9, "taa_sharpness": -1 }"#).unwrap();
        taa.sanitize();
        assert_eq!((taa.taa_blend, taa.taa_sharpness), (0.5, 0.0));
        let mut empty: QualitySettings = serde_json::from_str(r#"{ "levels": [] }"#).unwrap();
        empty.sanitize();
        assert_eq!(empty, QualitySettings::default());
    }

    #[test]
    fn upscaling_modes_and_old_files() {
        // Levels saved before Upscaling existed stay native.
        let old: QualityLevel = serde_json::from_str(r#"{ "name": "Old" }"#).unwrap();
        assert_eq!(old.upscaling, Upscaling::Off);
        assert_eq!(old.upscale_ratio(), 1.0);
        for l in QualitySettings::default().levels {
            assert_eq!(l.upscaling, Upscaling::Off, "{}", l.name);
        }
        let at = |u: Upscaling| QualityLevel {
            upscaling: u,
            ..QualityLevel::high()
        };
        assert_eq!(at(Upscaling::Quality).upscale_ratio(), 0.67);
        assert_eq!(at(Upscaling::Balanced).upscale_ratio(), 0.58);
        assert_eq!(at(Upscaling::Performance).upscale_ratio(), 0.5);
        let mut custom: QualityLevel =
            serde_json::from_str(r#"{ "upscaling": "Custom", "upscaling_scale": 0.1 }"#).unwrap();
        custom.sanitize();
        assert_eq!(custom.upscale_ratio(), UPSCALING_MIN);
        // The upscaler is TAA at a lower resolution: it runs with TAA unticked.
        let low = QualityLevel {
            upscaling: Upscaling::Performance,
            ..QualityLevel::low()
        };
        assert!(!low.temporal_aa && low.temporal());
        assert!(!QualityLevel::low().temporal());
    }
}
