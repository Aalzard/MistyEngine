//! The Standard shader's optional features as stored in a `.mat` file (Standard Lit v2):
//! Detail Maps, Parallax, Clear Coat, Translucency and Anisotropy sections, each with an
//! Enabled toggle, and a dielectric Specular tint. A section left at its defaults is left out
//! of the file, so materials written before these existed re-save unchanged; a disabled
//! section keeps its values (switch it back on and they return) but compiles nothing.
//!
//! [`Features::extras`] turns them into the renderer's [`LitExtras`] (`None` when nothing is
//! on: the material then draws with the plain Standard variants).

use serde::{Deserialize, Serialize};

use crate::render::material_v2::{LitExtras, LitUniform, feature};

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// sRGB (0..1) to linear.
fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Detail albedo (× 2: grey leaves the base unchanged) and detail normal on their own tiling,
/// masked by the mask's R (white where unassigned).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailMaps {
    pub enabled: bool,
    pub albedo_map: Option<String>,
    pub normal_map: Option<String>,
    pub normal_scale: f32,
    pub mask: Option<String>,
    /// Tiling and offset of the detail maps on the mesh uv (the main tiling doesn't apply).
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
}

impl Default for DetailMaps {
    fn default() -> Self {
        Self {
            enabled: false,
            albedo_map: None,
            normal_map: None,
            normal_scale: 1.0,
            mask: None,
            tiling: [4.0, 4.0],
            offset: [0.0, 0.0],
        }
    }
}

/// Parallax occlusion mapping from a height map (R, white = high).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Parallax {
    pub enabled: bool,
    pub height_map: Option<String>,
    /// Depth of the relief in uv units (0.02–0.08 for bricks and stones).
    pub scale: f32,
    /// Most steps the ray takes (at grazing angles; a quarter of them, at least 4, head on).
    pub steps: u32,
}

impl Default for Parallax {
    fn default() -> Self {
        Self {
            enabled: false,
            height_map: None,
            scale: 0.04,
            steps: 24,
        }
    }
}

/// A clear lacquer over the base (car paint, varnished wood, carbon fibre).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClearCoat {
    pub enabled: bool,
    pub strength: f32,
    pub smoothness: f32,
    /// The coat's own normal map (none: the coat follows the geometry, not the base's normal map).
    pub normal_map: Option<String>,
    pub normal_scale: f32,
}

impl Default for ClearCoat {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.0,
            smoothness: 0.95,
            normal_map: None,
            normal_scale: 1.0,
        }
    }
}

/// Light through thin or soft surfaces (leaves, skin, wax): wrap lighting tinted by the colour
/// and transmission from behind, less where the thickness map is white.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Translucency {
    pub enabled: bool,
    /// sRGB tint of the light that comes through and wraps round.
    pub color: [f32; 3],
    /// Light coming through from behind (0..1).
    pub strength: f32,
    /// How far light wraps round the terminator (0..1).
    pub wrap: f32,
    /// R = thickness (white = thick, no light through); multiplies `thickness`.
    pub thickness_map: Option<String>,
    pub thickness: f32,
}

impl Default for Translucency {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [1.0, 0.45, 0.3],
            strength: 0.6,
            wrap: 0.5,
            thickness_map: None,
            thickness: 0.0,
        }
    }
}

/// How the anisotropy direction is laid out over the surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnisotropyDirection {
    /// Along the texture's U (the tangent).
    #[default]
    Horizontal,
    /// Along the texture's V.
    Vertical,
    /// Away from the uv centre (a lathe-brushed disc's streak).
    Radial,
}

impl AnisotropyDirection {
    pub const ALL: [Self; 3] = [Self::Horizontal, Self::Vertical, Self::Radial];

    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal (U)",
            Self::Vertical => "Vertical (V)",
            Self::Radial => "Radial",
        }
    }
}

/// Highlights stretched along a direction (brushed metal, hair, satin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Anisotropy {
    pub enabled: bool,
    /// -1..1: positive stretches highlights along the direction, negative across it.
    pub strength: f32,
    pub direction: AnisotropyDirection,
    /// Degrees the direction is turned by (in the uv plane).
    pub rotation: f32,
}

impl Default for Anisotropy {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 0.8,
            direction: AnisotropyDirection::Horizontal,
            rotation: 0.0,
        }
    }
}

/// Every optional feature of a Standard material (flattened into [`crate::assets::Material`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    #[serde(skip_serializing_if = "is_default")]
    pub detail: DetailMaps,
    #[serde(skip_serializing_if = "is_default")]
    pub parallax: Parallax,
    #[serde(skip_serializing_if = "is_default")]
    pub clear_coat: ClearCoat,
    #[serde(skip_serializing_if = "is_default")]
    pub translucency: Translucency,
    #[serde(skip_serializing_if = "is_default")]
    pub anisotropy: Anisotropy,
    /// Dielectric reflectance: 1 = the usual 4% (KHR_materials_specular `specularFactor`).
    #[serde(skip_serializing_if = "is_one")]
    pub specular: f32,
    /// sRGB tint of the dielectric reflectance (KHR_materials_specular `specularColorFactor`).
    #[serde(skip_serializing_if = "is_white")]
    pub specular_color: [f32; 3],
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_white(v: &[f32; 3]) -> bool {
    *v == [1.0; 3]
}

impl Default for Features {
    fn default() -> Self {
        Self {
            detail: DetailMaps::default(),
            parallax: Parallax::default(),
            clear_coat: ClearCoat::default(),
            translucency: Translucency::default(),
            anisotropy: Anisotropy::default(),
            specular: 1.0,
            specular_color: [1.0; 3],
        }
    }
}

impl Features {
    pub fn new() -> Self {
        Self::default()
    }

    /// The feature bits that are on and do something.
    pub fn bits(&self) -> u8 {
        let mut f = 0;
        let d = &self.detail;
        if d.enabled && (d.albedo_map.is_some() || d.normal_map.is_some()) {
            f |= feature::DETAIL;
        }
        let p = &self.parallax;
        if p.enabled && p.height_map.is_some() && p.scale != 0.0 {
            f |= feature::PARALLAX;
        }
        if self.clear_coat.enabled && self.clear_coat.strength > 0.0 {
            f |= feature::CLEARCOAT;
        }
        let t = &self.translucency;
        if t.enabled && (t.strength > 0.0 || t.wrap > 0.0) {
            f |= feature::TRANSLUCENCY;
        }
        if self.anisotropy.enabled && self.anisotropy.strength != 0.0 {
            f |= feature::ANISOTROPY;
        }
        if self.specular != 1.0 || self.specular_color != [1.0; 3] {
            f |= feature::SPECULAR;
        }
        f
    }

    /// Renderer values (`None` when no feature is on).
    pub fn extras(&self) -> Option<LitExtras> {
        let bits = self.bits();
        if bits == 0 {
            return None;
        }
        let on = |bit: u8| bits & bit != 0;
        let mut e = LitExtras {
            features: bits,
            ..Default::default()
        };
        let mut u = LitUniform::default();
        if on(feature::DETAIL) {
            let d = &self.detail;
            e.detail_albedo = d.albedo_map.clone();
            e.detail_normal = d.normal_map.clone();
            e.detail_mask = d.mask.clone();
            u.detail_st = [d.tiling[0], d.tiling[1], d.offset[0], d.offset[1]];
            u.detail = [
                d.normal_scale,
                f32::from(d.albedo_map.is_some()),
                f32::from(d.normal_map.is_some()),
                0.0,
            ];
        }
        if on(feature::PARALLAX) {
            let p = &self.parallax;
            e.height_map = p.height_map.clone();
            let most = p.steps.clamp(1, 64) as f32;
            u.parallax = [p.scale, (most / 4.0).max(4.0).min(most), most, 0.0];
        }
        if on(feature::CLEARCOAT) {
            let c = &self.clear_coat;
            let perceptual = 1.0 - c.smoothness.clamp(0.0, 1.0);
            e.coat_normal_map = c.normal_map.clone();
            let scale = if c.normal_map.is_some() {
                c.normal_scale
            } else {
                0.0
            };
            u.coat = [
                c.strength.clamp(0.0, 1.0),
                perceptual * perceptual,
                scale,
                0.0,
            ];
        }
        if on(feature::TRANSLUCENCY) {
            let t = &self.translucency;
            e.thickness_map = t.thickness_map.clone();
            let c = t.color.map(linear);
            u.translucency = [c[0], c[1], c[2], t.strength.max(0.0)];
            u.subsurface = [t.wrap.clamp(0.0, 1.0), t.thickness.max(0.0), 0.0, 0.0];
        }
        if on(feature::ANISOTROPY) {
            let a = &self.anisotropy;
            let base = match a.direction {
                AnisotropyDirection::Horizontal | AnisotropyDirection::Radial => 0.0,
                AnisotropyDirection::Vertical => 90.0,
            };
            let (sin, cos) = (base + a.rotation).to_radians().sin_cos();
            let radial = f32::from(a.direction == AnisotropyDirection::Radial);
            u.aniso = [a.strength.clamp(-1.0, 1.0), cos, sin, radial];
        }
        if on(feature::SPECULAR) {
            let c = self.specular_color.map(linear);
            let k = self.specular.max(0.0);
            u.specular = [c[0] * k, c[1] * k, c[2] * k, 0.0];
        }
        e.values = u;
        Some(e)
    }

    /// Every texture the features reference (for dependency tracking and the Memory Profiler).
    pub fn textures(&self) -> Vec<&str> {
        [
            &self.detail.albedo_map,
            &self.detail.normal_map,
            &self.detail.mask,
            &self.parallax.height_map,
            &self.clear_coat.normal_map,
            &self.translucency.thickness_map,
        ]
        .into_iter()
        .filter_map(|p| p.as_deref())
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_on_means_no_extras() {
        let f = Features::new();
        assert_eq!(f.bits(), 0);
        assert!(f.extras().is_none());
        // Enabled but doing nothing: still nothing compiled.
        let mut f = Features::new();
        f.detail.enabled = true;
        f.parallax.enabled = true;
        f.anisotropy.enabled = true;
        f.anisotropy.strength = 0.0;
        assert_eq!(
            f.bits(),
            0,
            "detail without maps, parallax without a height map"
        );
        // Disabled sections keep their values but compile nothing.
        f.clear_coat.strength = 0.5;
        assert_eq!(f.bits(), 0);
    }

    #[test]
    fn each_feature_sets_its_bit_and_values() {
        let mut f = Features::new();
        f.detail = DetailMaps {
            enabled: true,
            albedo_map: Some("Assets/D.png".into()),
            tiling: [8.0, 8.0],
            ..Default::default()
        };
        f.parallax = Parallax {
            enabled: true,
            height_map: Some("Assets/H.png".into()),
            scale: 0.05,
            steps: 32,
        };
        f.clear_coat = ClearCoat {
            enabled: true,
            smoothness: 0.9,
            ..Default::default()
        };
        f.translucency.enabled = true;
        f.anisotropy = Anisotropy {
            enabled: true,
            strength: -0.5,
            direction: AnisotropyDirection::Vertical,
            rotation: 0.0,
        };
        f.specular = 0.5;
        assert_eq!(f.bits(), 63);
        let e = f.extras().unwrap();
        assert_eq!(e.detail_albedo.as_deref(), Some("Assets/D.png"));
        assert_eq!(e.values.detail_st, [8.0, 8.0, 0.0, 0.0]);
        assert_eq!(e.values.detail[1..3], [1.0, 0.0]);
        assert_eq!(e.values.parallax, [0.05, 8.0, 32.0, 0.0]);
        assert!(
            (e.values.coat[1] - 0.01).abs() < 1e-6,
            "roughness = (1 - s)²"
        );
        assert_eq!(e.values.coat[2], 0.0, "no coat normal map");
        assert!(e.values.aniso[1].abs() < 1e-6 && (e.values.aniso[2] - 1.0).abs() < 1e-6);
        assert_eq!(e.values.aniso[0], -0.5);
        assert!((e.values.specular[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn defaults_are_left_out_of_the_file() {
        let text = serde_json::to_string(&Features::new()).unwrap();
        assert_eq!(text, "{}");
        let mut f = Features::new();
        f.clear_coat.enabled = true;
        let text = serde_json::to_string(&f).unwrap();
        assert!(
            text.contains("clear_coat") && !text.contains("detail"),
            "{text}"
        );
        let back: Features = serde_json::from_str(&text).unwrap();
        assert_eq!(back.clear_coat, f.clear_coat);
    }
}
