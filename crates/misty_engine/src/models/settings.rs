//! Model import settings (the model file's `.meta`), Unity-like: scene scale and axes, normals
//! and tangents, material location, animation clip splitting, Read/Write, lightmap UVs, LODs.
//!
//! New fields are left out of the `.meta` while they hold their defaults, so files written
//! before they existed stay unchanged.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Which way is up in the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisConversion {
    /// From the file: FBX scene settings; glTF and OBJ are Y-up.
    #[default]
    Automatic,
    /// The file is Y-up whatever its settings say.
    YUp,
    /// The file is Z-up (Blender, 3ds Max): the model is turned -90° about X.
    ZUp,
}

impl AxisConversion {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::YUp, Self::ZUp];

    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic (from file)",
            Self::YUp => "Y Up",
            Self::ZUp => "Z Up",
        }
    }
}

/// Where vertex normals (or tangents) come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorSource {
    /// From the file (calculated when the file has none).
    #[default]
    Import,
    /// Calculated on import (normals: with the Smoothing Angle).
    Calculate,
    /// None: normals point up, tangents along +X (unlit or custom-shaded meshes).
    None,
}

impl VectorSource {
    pub const ALL: [Self; 3] = [Self::Import, Self::Calculate, Self::None];

    pub fn label(self) -> &'static str {
        match self {
            Self::Import => "Import",
            Self::Calculate => "Calculate",
            Self::None => "None",
        }
    }
}

/// One animation clip cut out of a take of the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipSettings {
    pub name: String,
    /// Source take (the file's animation name); empty = the first.
    pub take: String,
    /// First and last frame (at [`super::gltf_anim::SAMPLE_RATE`] frames per second).
    pub first_frame: f32,
    pub last_frame: f32,
    /// Loop Time.
    pub looping: bool,
}

impl Default for ClipSettings {
    fn default() -> Self {
        Self {
            name: "Clip".into(),
            take: String::new(),
            first_frame: 0.0,
            last_frame: 30.0,
            looping: true,
        }
    }
}

/// One generated level of detail: its share of the triangles and its LOD Group threshold.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LodSetting {
    /// Share of LOD 0's triangles kept (0.02..1).
    pub quality: f32,
    /// Lowest screen-relative height this level draws at (0..1).
    pub screen_height: f32,
}

impl Default for LodSetting {
    fn default() -> Self {
        Self {
            quality: 0.5,
            screen_height: 0.3,
        }
    }
}

/// The default levels: LOD 0 (the file), half and a fifth of the triangles, culled below 3%.
pub fn default_lods() -> Vec<LodSetting> {
    vec![
        LodSetting {
            quality: 1.0,
            screen_height: 0.5,
        },
        LodSetting {
            quality: 0.5,
            screen_height: 0.2,
        },
        LodSetting {
            quality: 0.2,
            screen_height: 0.03,
        },
    ]
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_true(v: &bool) -> bool {
    *v
}

fn default_true() -> bool {
    true
}

fn default_one() -> f32 {
    1.0
}

fn is_calculate(v: &VectorSource) -> bool {
    *v == VectorSource::Calculate
}

fn default_calculate() -> VectorSource {
    VectorSource::Calculate
}

fn default_angle() -> f32 {
    60.0
}

fn is_default_angle(v: &f32) -> bool {
    *v == 60.0
}

fn is_default_lods(v: &Vec<LodSetting>) -> bool {
    *v == default_lods()
}

/// Import settings of a model file (its `.meta`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelImporter {
    /// Scale Factor: multiplies the whole model (meshes, node positions, animation).
    #[serde(skip_serializing_if = "is_one", default = "default_one")]
    pub scale_factor: f32,
    /// Convert Units: FBX files in centimetres (or any unit) come in as metres.
    #[serde(skip_serializing_if = "is_true", default = "default_true")]
    pub convert_units: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub axis_conversion: AxisConversion,
    #[serde(skip_serializing_if = "is_default")]
    pub normals: VectorSource,
    /// Normals: Calculate joins faces meeting at less than this angle (degrees).
    #[serde(skip_serializing_if = "is_default_angle", default = "default_angle")]
    pub smoothing_angle: f32,
    /// Tangents (Import uses the file's when it has them).
    #[serde(skip_serializing_if = "is_calculate", default = "default_calculate")]
    pub tangents: VectorSource,
    /// Folder (project-relative) the materials are extracted to; empty = next to the model.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub material_folder: String,
    /// Animation clips cut from the file's takes; empty = one clip per take.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<ClipSettings>,
    /// Read/Write Enabled (Unity parity; Misty keeps meshes readable on the CPU either way:
    /// physics, picking and baking read them).
    #[serde(skip_serializing_if = "is_true", default = "default_true")]
    pub read_write: bool,
    /// Generate lightmap UVs (the second UV set) for every non-skinned mesh on import.
    pub generate_lightmap_uvs: bool,
    /// Build the meshes for Misty Virtual Geometry (clusters and a cluster hierarchy): dense
    /// static meshes then draw with per-cluster level of detail. Opaque, non-skinned meshes
    /// only; left out of the `.meta` when off.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub virtual_geometry: bool,
    /// Generate LODs: simplified copies of the static meshes and a LOD Group on the model.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub generate_lods: bool,
    /// The LOD levels, LOD 0 first (its quality is always 1).
    #[serde(skip_serializing_if = "is_default_lods", default = "default_lods")]
    pub lods: Vec<LodSetting>,
}

impl Default for ModelImporter {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            convert_units: true,
            axis_conversion: AxisConversion::Automatic,
            normals: VectorSource::Import,
            smoothing_angle: 60.0,
            tangents: VectorSource::Calculate,
            material_folder: String::new(),
            clips: Vec::new(),
            read_write: true,
            generate_lightmap_uvs: true,
            virtual_geometry: false,
            generate_lods: false,
            lods: default_lods(),
        }
    }
}

impl ModelImporter {
    /// Settings of `model` (defaults without a readable `.meta`).
    pub fn load(model: &Path) -> Self {
        crate::vfs::read_to_string(crate::asset_db::meta::meta_path(model))
            .ok()
            .and_then(|t| serde_json::from_str::<Self>(&t).ok())
            .map(|mut s| {
                s.sanitize();
                s
            })
            .unwrap_or_default()
    }

    /// Write the settings into the `.meta` file, keeping its GUID.
    pub fn save(&self, model: &Path) -> Result<(), String> {
        crate::asset_db::meta::write_settings(model, self)
    }

    /// Keep values usable (positive scale, angle in 0..180, LOD levels in order).
    pub fn sanitize(&mut self) {
        if !self.scale_factor.is_finite() || self.scale_factor <= 0.0 {
            self.scale_factor = 1.0;
        }
        if !self.smoothing_angle.is_finite() {
            self.smoothing_angle = 60.0;
        }
        self.smoothing_angle = self.smoothing_angle.clamp(0.0, 180.0);
        for c in &mut self.clips {
            if !c.first_frame.is_finite() {
                c.first_frame = 0.0;
            }
            if !c.last_frame.is_finite() {
                c.last_frame = c.first_frame;
            }
            c.first_frame = c.first_frame.max(0.0);
            c.last_frame = c.last_frame.max(c.first_frame);
        }
        if self.lods.is_empty() {
            self.lods = default_lods();
        }
        self.lods
            .truncate(crate::scene::lod_group::LodGroup::MAX_LEVELS);
        let mut prev_quality = 1.0f32;
        let mut prev_height = 1.0f32;
        for (i, l) in self.lods.iter_mut().enumerate() {
            let q = if l.quality.is_finite() {
                l.quality
            } else {
                0.5
            };
            l.quality = if i == 0 {
                1.0
            } else {
                q.clamp(0.02, prev_quality)
            };
            let h = if l.screen_height.is_finite() {
                l.screen_height
            } else {
                0.1
            };
            l.screen_height = h.clamp(0.0, prev_height);
            prev_quality = l.quality;
            prev_height = l.screen_height;
        }
    }
}
