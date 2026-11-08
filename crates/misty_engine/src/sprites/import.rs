//! Texture import settings, stored next to the image as `<image>.meta` (JSON).
//!
//! No `.meta` file = the Default texture type with trilinear, repeating, mipmapped sampling, which
//! is how every texture behaved before import settings existed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Texture Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureType {
    #[default]
    Default,
    /// Tangent-space normal map: always linear (not sRGB).
    NormalMap,
    /// Sprite (2D and UI): the image can be used by Sprite Renderers and UI Images.
    Sprite,
}

impl TextureType {
    pub const ALL: [Self; 3] = [Self::Default, Self::NormalMap, Self::Sprite];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::NormalMap => "Normal Map",
            Self::Sprite => "Sprite (2D and UI)",
        }
    }
}

/// Max Size choices (largest side in pixels).
pub const MAX_SIZES: [u32; 9] = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192];

/// "2048 (2K)" style label of a Max Size.
pub fn max_size_label(size: u32) -> String {
    match size {
        1024 => "1024 (1K)".into(),
        2048 => "2048 (2K)".into(),
        4096 => "4096 (4K)".into(),
        8192 => "8192 (8K)".into(),
        s => s.to_string(),
    }
}

/// Sprite Mode: one sprite for the whole image, or many cut out of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpriteMode {
    #[default]
    Single,
    Multiple,
}

impl SpriteMode {
    pub const ALL: [Self; 2] = [Self::Single, Self::Multiple];

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Multiple => "Multiple",
        }
    }
}

/// Filter Mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FilterMode {
    /// Nearest texel: crisp pixel art. Files from before the rename say "Point".
    #[serde(alias = "Point")]
    Nearest,
    /// Smooth inside a mip level, but jumps between levels.
    Bilinear,
    /// Smooth inside and between mip levels.
    #[default]
    Trilinear,
}

impl FilterMode {
    pub const ALL: [Self; 3] = [Self::Nearest, Self::Bilinear, Self::Trilinear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nearest => "Nearest Neighbor (no filter)",
            Self::Bilinear => "Bilinear",
            Self::Trilinear => "Trilinear",
        }
    }
}

/// Wrap Mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WrapMode {
    #[default]
    Repeat,
    Clamp,
    Mirror,
}

impl WrapMode {
    pub const ALL: [Self; 3] = [Self::Repeat, Self::Clamp, Self::Mirror];

    pub fn label(self) -> &'static str {
        match self {
            Self::Repeat => "Repeat",
            Self::Clamp => "Clamp",
            Self::Mirror => "Mirror",
        }
    }
}

/// Compression: GPU block compression (BCn) of the texture, made on import and cached in
/// `Library/TextureCache` (see [`super::compress`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextureCompression {
    /// Uncompressed RGBA8 (4 bytes per pixel).
    None,
    /// BC1 (opaque, 0.5 byte per pixel) or BC3 (with alpha, 1 byte); normal maps BC5.
    #[default]
    NormalQuality,
    /// BC7 (1 byte per pixel, close to the source); normal maps BC5.
    HighQuality,
}

impl TextureCompression {
    pub const ALL: [Self; 3] = [Self::None, Self::NormalQuality, Self::HighQuality];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::NormalQuality => "Normal Quality",
            Self::HighQuality => "High Quality",
        }
    }
}

/// Platforms a texture can override its Max Size and Compression for.
pub const PLATFORMS: [&str; 3] = ["Windows", "Linux", "macOS"];

/// The platform the program runs on, as named in [`PLATFORMS`].
pub fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        _ => "Windows",
    }
}

/// A per-platform override of Max Size and Compression (Unity's platform tabs, kept simple).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformOverride {
    /// One of [`PLATFORMS`].
    pub platform: String,
    pub max_size: u32,
    pub compression: TextureCompression,
}

impl Default for PlatformOverride {
    fn default() -> Self {
        Self {
            platform: current_platform().to_owned(),
            max_size: 2048,
            compression: TextureCompression::NormalQuality,
        }
    }
}

/// One sprite of a Multiple-mode texture: a pixel rect (origin bottom-left) and its pivot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteRect {
    pub name: String,
    /// x, y, width, height in pixels; y counts up from the bottom edge.
    pub rect: [f32; 4],
    /// Normalised pivot inside the rect (0.5, 0.5 = centre).
    pub pivot: [f32; 2],
}

/// Import settings of one image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextureImporter {
    pub texture_type: TextureType,
    pub sprite_mode: SpriteMode,
    /// How many image pixels make one world unit.
    pub pixels_per_unit: f32,
    /// Single mode: normalised pivot of the sprite.
    pub pivot: [f32; 2],
    pub filter_mode: FilterMode,
    pub wrap_mode: WrapMode,
    pub generate_mip_maps: bool,
    /// Multiple mode: the sliced sprites.
    pub sprites: Vec<SpriteRect>,
    /// Largest side on the GPU; bigger images are scaled down (Lanczos) when loaded.
    pub max_size: u32,
    /// Anisotropic filtering level, 0–16 (0 and 1 = off), used with the Quality page's
    /// Anisotropic Textures setting.
    pub aniso_level: u8,
    /// Colour data stored in sRGB (off for masks; Normal Map textures are always linear).
    pub srgb: bool,
    /// Compression; `None` = the type's default (Normal Quality for 3D textures and normal
    /// maps, uncompressed sprites). Left out of the `.meta` while unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<TextureCompression>,
    /// Per-platform Max Size and Compression.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<PlatformOverride>,
}

impl Default for TextureImporter {
    fn default() -> Self {
        Self {
            texture_type: TextureType::Default,
            sprite_mode: SpriteMode::Single,
            pixels_per_unit: 100.0,
            pivot: [0.5, 0.5],
            filter_mode: FilterMode::Trilinear,
            wrap_mode: WrapMode::Repeat,
            generate_mip_maps: true,
            sprites: Vec::new(),
            max_size: 2048,
            aniso_level: 1,
            srgb: true,
            compression: None,
            overrides: Vec::new(),
        }
    }
}

impl TextureImporter {
    /// Settings a freshly imported sprite gets: clamped, no mips (like the reference importer).
    pub fn sprite() -> Self {
        Self {
            texture_type: TextureType::Sprite,
            wrap_mode: WrapMode::Clamp,
            generate_mip_maps: false,
            ..Default::default()
        }
    }

    /// Settings a freshly chosen texture type starts from (mips on for 3D, off for sprites;
    /// normal maps are linear).
    pub fn set_type(&mut self, t: TextureType) {
        if t == self.texture_type {
            return;
        }
        self.texture_type = t;
        match t {
            TextureType::Sprite => {
                self.wrap_mode = WrapMode::Clamp;
                self.generate_mip_maps = false;
            }
            TextureType::NormalMap => {
                self.srgb = false;
                self.generate_mip_maps = true;
            }
            TextureType::Default => self.generate_mip_maps = true,
        }
    }

    pub fn is_sprite(&self) -> bool {
        self.texture_type == TextureType::Sprite
    }

    /// The override for `platform`, if any.
    pub fn override_for(&self, platform: &str) -> Option<&PlatformOverride> {
        self.overrides.iter().find(|o| o.platform == platform)
    }

    /// Compression used on `platform`: its override, the texture's setting, or the type's
    /// default (sprites stay uncompressed: block artefacts show on pixel art and UI).
    pub fn compression_for(&self, platform: &str) -> TextureCompression {
        if let Some(o) = self.override_for(platform) {
            return o.compression;
        }
        self.compression.unwrap_or(match self.texture_type {
            TextureType::Sprite => TextureCompression::None,
            _ => TextureCompression::NormalQuality,
        })
    }

    /// Max Size used on `platform` (its override or the texture's).
    pub fn max_size_for(&self, platform: &str) -> u32 {
        self.override_for(platform)
            .map_or(self.max_size, |o| o.max_size)
    }

    /// Whether colour is decoded from sRGB on the GPU.
    pub fn is_srgb(&self) -> bool {
        self.srgb && self.texture_type != TextureType::NormalMap
    }

    /// Where the settings of `image` live.
    pub fn meta_path(image: &Path) -> PathBuf {
        crate::asset_db::meta::meta_path(image)
    }

    /// Settings of `image` (defaults when there is no `.meta` file or it can't be read).
    pub fn load(image: &Path) -> Self {
        Self::try_load(image).unwrap_or_default()
    }

    /// Settings of `image`; an error when there is no `.meta` file or it is invalid.
    pub fn try_load(image: &Path) -> Result<Self, String> {
        let text = crate::vfs::read_to_string(Self::meta_path(image)).map_err(|e| e.to_string())?;
        let mut s: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        s.sanitize();
        Ok(s)
    }

    /// Write the settings into the `.meta` file, keeping the GUID already there.
    pub fn save(&self, image: &Path) -> Result<(), String> {
        crate::asset_db::meta::write_settings(image, self)
    }

    /// Keep values usable (positive PPU, pivots and rects finite).
    pub fn sanitize(&mut self) {
        if !self.pixels_per_unit.is_finite() || self.pixels_per_unit < 0.01 {
            self.pixels_per_unit = 100.0;
        }
        let fix = |p: &mut [f32; 2]| {
            for v in p {
                if !v.is_finite() {
                    *v = 0.5;
                }
            }
        };
        fix(&mut self.pivot);
        // Snap to the nearest Max Size choice.
        let size = self
            .max_size
            .clamp(MAX_SIZES[0], MAX_SIZES[MAX_SIZES.len() - 1]);
        self.max_size = MAX_SIZES.into_iter().find(|s| *s >= size).unwrap_or(8192);
        self.aniso_level = self.aniso_level.min(16);
        for s in &mut self.sprites {
            fix(&mut s.pivot);
            for v in &mut s.rect {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
            s.rect[2] = s.rect[2].max(1.0);
            s.rect[3] = s.rect[3].max(1.0);
        }
    }

    /// The sprite rect called `name` (Multiple mode).
    pub fn find(&self, name: &str) -> Option<&SpriteRect> {
        self.sprites.iter().find(|s| s.name == name)
    }
}

/// Pivot presets of the Inspector's Pivot dropdown.
pub const PIVOTS: [(&str, [f32; 2]); 9] = [
    ("Center", [0.5, 0.5]),
    ("Top Left", [0.0, 1.0]),
    ("Top", [0.5, 1.0]),
    ("Top Right", [1.0, 1.0]),
    ("Left", [0.0, 0.5]),
    ("Right", [1.0, 0.5]),
    ("Bottom Left", [0.0, 0.0]),
    ("Bottom", [0.5, 0.0]),
    ("Bottom Right", [1.0, 0.0]),
];

/// Name of a pivot preset, or "Custom".
pub fn pivot_name(p: [f32; 2]) -> &'static str {
    PIVOTS
        .iter()
        .find(|(_, v)| *v == p)
        .map_or("Custom", |(n, _)| n)
}
