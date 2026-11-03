//! Texture import settings, stored next to the image as `<image>.meta` (JSON).
//!
//! No `.meta` file = the Default texture type with bilinear, repeating, mipmapped sampling, which
//! is how every texture behaved before import settings existed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Texture Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureType {
    #[default]
    Default,
    /// Sprite (2D and UI): the image can be used by Sprite Renderers and UI Images.
    Sprite,
}

impl TextureType {
    pub const ALL: [Self; 2] = [Self::Default, Self::Sprite];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Sprite => "Sprite (2D and UI)",
        }
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
    /// Nearest texel: crisp pixel art.
    Point,
    #[default]
    Bilinear,
}

impl FilterMode {
    pub const ALL: [Self; 2] = [Self::Point, Self::Bilinear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Point => "Point (no filter)",
            Self::Bilinear => "Bilinear",
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
}

impl Default for TextureImporter {
    fn default() -> Self {
        Self {
            texture_type: TextureType::Default,
            sprite_mode: SpriteMode::Single,
            pixels_per_unit: 100.0,
            pivot: [0.5, 0.5],
            filter_mode: FilterMode::Bilinear,
            wrap_mode: WrapMode::Repeat,
            generate_mip_maps: true,
            sprites: Vec::new(),
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

    pub fn is_sprite(&self) -> bool {
        self.texture_type == TextureType::Sprite
    }

    /// Where the settings of `image` live.
    pub fn meta_path(image: &Path) -> PathBuf {
        let mut name = image.as_os_str().to_owned();
        name.push(".meta");
        PathBuf::from(name)
    }

    /// Settings of `image` (defaults when there is no `.meta` file or it can't be read).
    pub fn load(image: &Path) -> Self {
        Self::try_load(image).unwrap_or_default()
    }

    /// Settings of `image`; an error when there is no `.meta` file or it is invalid.
    pub fn try_load(image: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(Self::meta_path(image)).map_err(|e| e.to_string())?;
        let mut s: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        s.sanitize();
        Ok(s)
    }

    pub fn save(&self, image: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::meta_path(image), text + "\n").map_err(|e| e.to_string())
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
