//! Asset types stored as files in a project's `Assets` folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::material_features::Features;
use crate::render::custom_shader::{CustomMaterial, resolve_values};
use crate::render::{MaterialParams, RenderingMode, ShaderKind};
use crate::render_scene::MaterialSource;
use crate::shader_asset::{self, Blend, Cull, PropertyValue, ShaderModel};
use std::sync::Arc;

static PROJECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set the folder that project-relative asset paths ("Assets/…") resolve against.
/// The renderer, model importer and physics read textures and models through it.
pub fn set_project_root(root: &Path) {
    *PROJECT_ROOT.write().unwrap_or_else(|e| e.into_inner()) = Some(root.to_path_buf());
    ROOT_SETS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// How many times the project root was set: tests that read assets by project path compare
/// it before and after, to tell a root moved by a test running alongside from a real failure.
static ROOT_SETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn root_generation() -> u64 {
    ROOT_SETS.load(std::sync::atomic::Ordering::SeqCst)
}

/// Absolute path of a project-relative asset path (unchanged when already absolute or no root is set).
pub fn absolute(rel: &str) -> PathBuf {
    match &*PROJECT_ROOT.read().unwrap_or_else(|e| e.into_inner()) {
        Some(root) => root.join(rel),
        None => PathBuf::from(rel),
    }
}

/// "Assets/…" path with forward slashes for a file inside the project (else the full path).
pub fn relative(path: &Path) -> String {
    let root = PROJECT_ROOT.read().unwrap_or_else(|e| e.into_inner());
    root.as_deref()
        .and_then(|r| path.strip_prefix(r).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Display name of a material path ("Red"), or "Default-Material" for none.
pub fn material_name(rel: Option<&str>) -> String {
    match rel {
        Some(r) => Path::new(r)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        None => "Default-Material".to_owned(),
    }
}

/// File extensions loaded as textures.
pub const TEXTURE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

pub fn is_texture(path: &str) -> bool {
    has_extension(path, &TEXTURE_EXTENSIONS)
}

pub(crate) fn has_extension(path: &str, list: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Where the Standard shader reads smoothness from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmoothnessSource {
    /// The Metallic map's alpha (the slider when there is no map).
    #[default]
    MetallicAlpha,
    /// The Albedo texture's alpha (which is then not used as transparency).
    AlbedoAlpha,
}

impl SmoothnessSource {
    pub const ALL: [Self; 2] = [Self::MetallicAlpha, Self::AlbedoAlpha];

    pub fn label(self) -> &'static str {
        match self {
            Self::MetallicAlpha => "Metallic Alpha",
            Self::AlbedoAlpha => "Albedo Alpha",
        }
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_half(v: &f32) -> bool {
    *v == 0.5
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// A material asset (`.mat`, JSON). Mirrors the Standard shader's properties; fields added after
/// the first version are left out of the file while at their defaults, so older files re-save
/// unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    /// "Standard", "Unlit/Color", "Unlit/Texture" or "Unlit/Transparent" (see [`ShaderKind`]).
    pub shader: String,
    /// Albedo, sRGB + alpha.
    pub color: [f32; 4],
    pub metallic: f32,
    pub smoothness: f32,
    pub emission_enabled: bool,
    /// sRGB emission colour.
    pub emission: [f32; 3],
    pub emission_intensity: f32,
    /// Albedo texture ("Assets/…/Name.png"), multiplied by `color`.
    pub albedo_map: Option<String>,
    /// Tangent-space normal map (OpenGL convention: green = up).
    pub normal_map: Option<String>,
    pub normal_scale: f32,
    /// UV scale and offset applied to both maps.
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
    /// Standard shader Rendering Mode.
    #[serde(skip_serializing_if = "is_default")]
    pub rendering_mode: RenderingMode,
    /// Cutout: alpha below this is cut away.
    #[serde(skip_serializing_if = "is_half")]
    pub alpha_cutoff: f32,
    #[serde(skip_serializing_if = "is_default")]
    pub smoothness_source: SmoothnessSource,
    /// R = metallic, A = smoothness; replaces the Metallic slider (Smoothness then scales A).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic_map: Option<String>,
    /// G channel = ambient occlusion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion_map: Option<String>,
    #[serde(skip_serializing_if = "is_one")]
    pub occlusion_strength: f32,
    /// Multiplied by the emission colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission_map: Option<String>,
    /// Draw both faces (Cull Off).
    #[serde(skip_serializing_if = "is_default")]
    pub double_sided: bool,
    /// UV rotation in degrees about the texture centre (glTF `KHR_texture_transform`).
    #[serde(skip_serializing_if = "is_default")]
    pub uv_rotation: f32,
    /// Optional Standard features (Detail Maps, Parallax, Clear Coat, Translucency,
    /// Anisotropy, Specular), each left out of the file at its defaults.
    #[serde(flatten)]
    pub features: Features,
    /// Property values of a `.shader` asset by name (left out of the file when empty).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, PropertyValue>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            shader: "Standard".into(),
            color: [1.0; 4],
            metallic: 0.0,
            smoothness: 0.5,
            emission_enabled: false,
            emission: [0.0; 3],
            emission_intensity: 1.0,
            albedo_map: None,
            normal_map: None,
            normal_scale: 1.0,
            tiling: [1.0, 1.0],
            offset: [0.0, 0.0],
            rendering_mode: RenderingMode::Opaque,
            alpha_cutoff: 0.5,
            smoothness_source: SmoothnessSource::MetallicAlpha,
            metallic_map: None,
            occlusion_map: None,
            occlusion_strength: 1.0,
            emission_map: None,
            double_sided: false,
            uv_rotation: 0.0,
            features: Features::default(),
            properties: BTreeMap::new(),
        }
    }
}

impl Material {
    pub fn shader_kind(&self) -> ShaderKind {
        ShaderKind::from_name(&self.shader)
    }

    /// Renderer values. Unlit shaders keep only what they use (Unlit/Color: colour,
    /// Unlit/Texture: texture, Unlit/Transparent: texture with alpha blending).
    pub fn params(&self) -> MaterialParams {
        if shader_asset::is_shader_path(&self.shader) {
            return self.custom_params();
        }
        let standard = self.standard_params();
        let unlit = MaterialParams {
            unlit: true,
            color: [1.0; 4],
            albedo_map: self.albedo_map.clone(),
            tiling: self.tiling,
            offset: self.offset,
            double_sided: self.double_sided,
            ..Default::default()
        };
        match self.shader_kind() {
            ShaderKind::Standard => standard,
            ShaderKind::UnlitColor => MaterialParams {
                color: self.color,
                albedo_map: None,
                ..unlit
            },
            ShaderKind::UnlitTexture => unlit,
            ShaderKind::UnlitTransparent => MaterialParams {
                mode: RenderingMode::Fade,
                ..unlit
            },
        }
    }

    /// Uses a `.shader` asset (its path is in `shader`).
    pub fn is_custom(&self) -> bool {
        shader_asset::is_shader_path(&self.shader)
    }

    /// A `.shader` material: render state from the shader's header, values from `properties`
    /// (defaults for the rest). A missing or broken shader draws magenta.
    fn custom_params(&self) -> MaterialParams {
        let (settings, values, variant) = match shader_asset::load(&self.shader) {
            Ok(a) => {
                let variant = a.variant_key(&self.properties);
                // The variant decides the render state too (a keyword may switch features).
                let settings = shader_asset::load_variant(&self.shader, &variant)
                    .map_or(a.settings, |v| v.settings);
                (settings, resolve_values(&a, &self.properties), variant)
            }
            Err(_) => (Default::default(), self.properties.clone(), String::new()),
        };
        MaterialParams {
            unlit: settings.model == ShaderModel::Unlit,
            mode: match settings.blend {
                Blend::Off => RenderingMode::Opaque,
                Blend::Alpha | Blend::Additive => RenderingMode::Fade,
                Blend::Premultiply => RenderingMode::Transparent,
            },
            double_sided: settings.cull == Cull::Off,
            custom: Some(CustomMaterial {
                shader: self.shader.clone(),
                values: Arc::new(values),
                queue: settings.queue,
                variant,
            }),
            ..Default::default()
        }
    }

    fn standard_params(&self) -> MaterialParams {
        MaterialParams {
            color: self.color,
            metallic: self.metallic,
            smoothness: self.smoothness,
            emission: self.emission,
            emission_intensity: if self.emission_enabled {
                self.emission_intensity
            } else {
                0.0
            },
            albedo_map: self.albedo_map.clone(),
            normal_map: self.normal_map.clone(),
            normal_scale: self.normal_scale,
            tiling: self.tiling,
            offset: self.offset,
            uv_rotation: self.uv_rotation,
            unlit: false,
            mode: self.rendering_mode,
            alpha_cutoff: self.alpha_cutoff,
            double_sided: self.double_sided,
            smoothness_from_albedo: self.smoothness_source == SmoothnessSource::AlbedoAlpha,
            metallic_map: self.metallic_map.clone(),
            occlusion_map: self.occlusion_map.clone(),
            occlusion_strength: self.occlusion_strength,
            emission_map: self.emission_map.clone().filter(|_| self.emission_enabled),
            custom: None,
            lit_2d: false,
            lightmap: None,
            light_probes: false,
            contribute_gi: true,
            light_glow: [0.0; 2],
            extras: self.features.extras().map(Arc::new),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&crate::asset_db::resolve_json(&text)).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let text = crate::asset_db::embed_json(&text);
        crate::asset_db::write_atomic(path, text).map_err(|e| e.to_string())
    }

    /// [`Self::save`] through the background writer (see [`crate::asset_db::saver`]): for
    /// edits that save on every change, like a slider being dragged in the Inspector.
    pub fn save_later(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let text = crate::asset_db::embed_json(&text);
        crate::asset_db::saver::save_later(path, text.into_bytes()).map_err(|e| e.to_string())
    }
}

/// Read-only asset lookups for a built game: every material under `Assets/`, loaded once.
#[derive(Default)]
pub struct AssetCache {
    /// Project-relative path ("Assets/Materials/Red.mat") → material.
    pub materials: BTreeMap<String, Material>,
}

impl AssetCache {
    /// Load the materials of the project at `root`. Unreadable files are skipped.
    pub fn open(root: &Path) -> Self {
        let mut cache = Self::default();
        for path in crate::vfs::files_under(&root.join("Assets")) {
            if path.extension().is_some_and(|e| e == "mat")
                && let Ok(m) = Material::load(&path)
            {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let rel = rel.to_string_lossy().replace('\\', "/");
                cache.materials.insert(rel, m);
            }
        }
        cache
    }
}

impl MaterialSource for AssetCache {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.materials.get(r))
            .map(Material::params)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn material_round_trips_and_fills_missing_fields() {
        let dir = std::env::temp_dir().join("misty_mat_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Red.mat");
        let m = Material {
            color: [1.0, 0.0, 0.0, 1.0],
            smoothness: 0.8,
            ..Default::default()
        };
        m.save(&path).unwrap();
        assert_eq!(Material::load(&path).unwrap(), m);

        std::fs::write(&path, r#"{ "metallic": 1.0 }"#).unwrap();
        let partial = Material::load(&path).unwrap();
        assert_eq!(partial.metallic, 1.0);
        assert_eq!(partial.smoothness, 0.5, "missing fields use defaults");
    }

    /// A `.mat` written before textures existed loads with no maps and identity UVs.
    #[test]
    fn old_material_files_load_without_texture_fields() {
        let old = r#"{
  "shader": "Standard",
  "color": [0.8, 0.1, 0.1, 1.0],
  "metallic": 0.0,
  "smoothness": 0.6,
  "emission_enabled": false,
  "emission": [0.0, 0.0, 0.0],
  "emission_intensity": 1.0
}"#;
        let m: Material = serde_json::from_str(old).unwrap();
        assert_eq!(m.color, [0.8, 0.1, 0.1, 1.0]);
        assert_eq!(m.albedo_map, None);
        assert_eq!(m.normal_map, None);
        assert_eq!(m.normal_scale, 1.0);
        assert_eq!(m.tiling, [1.0, 1.0]);
        assert_eq!(m.offset, [0.0, 0.0]);
        let p = m.params();
        assert_eq!(p.tiling, [1.0, 1.0]);
        assert!(p.albedo_map.is_none());
    }

    #[test]
    fn texture_paths_round_trip() {
        let m = Material {
            albedo_map: Some("Assets/Textures/Checker.png".into()),
            normal_map: Some("Assets/Textures/Bumps.png".into()),
            normal_scale: 0.5,
            tiling: [4.0, 2.0],
            offset: [0.25, 0.0],
            ..Default::default()
        };
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Material>(&text).unwrap(), m);
        assert!(is_texture("Assets/a.PNG") && is_texture("b.jpeg") && !is_texture("c.mat"));
    }

    #[test]
    fn emission_only_when_enabled() {
        let mut m = Material {
            emission: [1.0, 0.5, 0.0],
            emission_intensity: 2.0,
            ..Default::default()
        };
        assert_eq!(m.params().emission_intensity, 0.0);
        m.emission_enabled = true;
        assert_eq!(m.params().emission, [1.0, 0.5, 0.0]);
        assert_eq!(m.params().emission_intensity, 2.0);
    }

    /// Committed materials written before rendering modes and the extra maps existed re-save
    /// byte-identical (new fields at their defaults are left out).
    #[test]
    fn old_sample_materials_re_save_unchanged() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject/Assets/Materials");
        for name in [
            "Checker.mat",
            "Bumpy Tiles.mat",
            "Laser.mat",
            "Wood.mat",
            "Blue Steel.mat",
        ] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            let m: Material = serde_json::from_str(&text).unwrap();
            assert_eq!(m.rendering_mode, RenderingMode::Opaque, "{name}");
            assert_eq!(m.alpha_cutoff, 0.5);
            assert_eq!(m.occlusion_strength, 1.0);
            assert_eq!(
                serde_json::to_string_pretty(&m).unwrap(),
                text.trim_end(),
                "{name}"
            );
            // Same renderer values as before: opaque, lit, no extra maps.
            let p = m.params();
            assert!(!p.unlit && !p.double_sided && !p.smoothness_from_albedo);
            assert!(p.metallic_map.is_none() && p.occlusion_map.is_none());
            assert!(p.shadow_key().is_some_and(|k| !k.cutout));
        }
    }

    #[test]
    fn standard_options_round_trip() {
        let m = Material {
            rendering_mode: RenderingMode::Cutout,
            alpha_cutoff: 0.3,
            smoothness_source: SmoothnessSource::AlbedoAlpha,
            metallic_map: Some("Assets/Textures/Metal.png".into()),
            occlusion_map: Some("Assets/Textures/AO.png".into()),
            occlusion_strength: 0.7,
            emission_enabled: true,
            emission_map: Some("Assets/Textures/Glow.png".into()),
            double_sided: true,
            ..Default::default()
        };
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert!(text.contains("\"rendering_mode\": \"Cutout\""), "{text}");
        assert_eq!(serde_json::from_str::<Material>(&text).unwrap(), m);
        let p = m.params();
        assert_eq!(p.mode, RenderingMode::Cutout);
        assert!(p.smoothness_from_albedo && p.double_sided);
        assert_eq!(p.emission_map.as_deref(), Some("Assets/Textures/Glow.png"));
        let shadow = p.shadow_key().unwrap();
        assert!(
            shadow.cutout && shadow.double_sided,
            "cutout casts clipped shadows"
        );

        let defaults = serde_json::to_string_pretty(&Material::default()).unwrap();
        for key in [
            "rendering_mode",
            "alpha_cutoff",
            "metallic_map",
            "double_sided",
        ] {
            assert!(!defaults.contains(key), "{key} left out at its default");
        }
    }

    #[test]
    fn blended_and_unlit_materials_cast_no_shadows() {
        for mode in [RenderingMode::Fade, RenderingMode::Transparent] {
            let m = Material {
                rendering_mode: mode,
                ..Default::default()
            };
            assert!(m.params().shadow_key().is_none(), "{mode:?}");
            assert!(m.params().mode.is_blended());
        }
        let unlit = |shader: &str| Material {
            shader: shader.into(),
            color: [0.2, 0.4, 0.6, 0.5],
            albedo_map: Some("Assets/Textures/Sign.png".into()),
            emission_enabled: true,
            ..Default::default()
        };
        let color = unlit("Unlit/Color").params();
        assert!(color.unlit && color.shadow_key().is_none());
        assert_eq!(color.color, [0.2, 0.4, 0.6, 0.5]);
        assert_eq!(color.albedo_map, None, "Unlit/Color has no texture");
        assert_eq!(color.mode, RenderingMode::Opaque);
        let texture = unlit("Unlit/Texture").params();
        assert_eq!(texture.color, [1.0; 4], "Unlit/Texture has no colour");
        assert!(texture.albedo_map.is_some());
        assert_eq!(texture.emission_intensity, 0.0);
        assert_eq!(
            unlit("Unlit/Transparent").params().mode,
            RenderingMode::Fade
        );
    }

    #[test]
    fn cache_loads_project_materials() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let cache = AssetCache::open(&root);
        let red = cache.params(Some("Assets/Materials/Red.mat"));
        assert_ne!(red, MaterialParams::default());
        assert_eq!(cache.params(None), MaterialParams::default());
        assert_eq!(
            cache.params(Some("Assets/Missing.mat")),
            MaterialParams::default()
        );
    }
}
