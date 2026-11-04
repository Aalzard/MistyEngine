//! Biomes: small `.biome` JSON assets (in `Assets/WorldBuilder/`) with a texture per terrain
//! feature, scatter lists and a colour tone, plus the six built-in presets.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File extension of biome assets.
pub const EXTENSION: &str = "biome";
/// Where presets and generated assets go.
pub const FOLDER: &str = "Assets/WorldBuilder";

/// Terrain features a texel is classified into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Flat,
    Slope,
    Cliff,
    /// Beaches, river banks and lake beds.
    Shore,
    /// High ground: snow or bare summits.
    Peak,
}

impl Feature {
    pub const ALL: [Self; 5] = [
        Self::Flat,
        Self::Slope,
        Self::Cliff,
        Self::Shore,
        Self::Peak,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Slope => "Slope",
            Self::Cliff => "Cliff",
            Self::Shore => "Shore / Underwater",
            Self::Peak => "Peak / Snow",
        }
    }
}

/// The texture one feature uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureLayer {
    /// Project-relative albedo texture.
    pub texture: Option<String>,
    pub normal_map: Option<String>,
    /// World size of one texture repeat in metres.
    pub tiling: f32,
}

impl Default for FeatureLayer {
    fn default() -> Self {
        Self {
            texture: None,
            normal_map: None,
            tiling: 10.0,
        }
    }
}

/// What a scatter item draws when it has no prefab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScatterShape {
    /// Trunk + round crown.
    #[default]
    BroadleafTree,
    /// Trunk + tall narrow crown.
    PineTree,
    /// Squashed sphere.
    Rock,
    Boulder,
    Bush,
    /// Small upright tuft.
    Grass,
    Cactus,
}

impl ScatterShape {
    pub const ALL: [Self; 7] = [
        Self::BroadleafTree,
        Self::PineTree,
        Self::Rock,
        Self::Boulder,
        Self::Bush,
        Self::Grass,
        Self::Cactus,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::BroadleafTree => "Broadleaf Tree",
            Self::PineTree => "Pine Tree",
            Self::Rock => "Rock",
            Self::Boulder => "Boulder",
            Self::Bush => "Bush",
            Self::Grass => "Grass Tuft",
            Self::Cactus => "Cactus",
        }
    }

    /// Rocks gather in Rock Areas; plants avoid them.
    pub fn is_rock(self) -> bool {
        matches!(self, Self::Rock | Self::Boulder)
    }
}

/// One kind of object scattered over a biome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScatterItem {
    pub name: String,
    /// Prefab asset to place (else `shape` from primitives).
    pub prefab: Option<String>,
    pub shape: ScatterShape,
    /// sRGB colour of the primitive shape (main part).
    pub color: [f32; 3],
    /// Instances per 1000 m² where the rules allow it fully.
    pub density: f32,
    /// No two instances closer than this (metres).
    pub min_spacing: f32,
    /// Uniform scale range.
    pub scale: [f32; 2],
    /// Random yaw.
    pub random_rotation: bool,
    /// Steepest ground allowed (degrees).
    pub max_slope: f32,
    /// Allowed height band as fractions of the terrain's maximum height.
    pub height_range: [f32; 2],
    /// Tilt with the ground (rocks) instead of standing upright (trees).
    pub align_to_normal: bool,
}

impl Default for ScatterItem {
    fn default() -> Self {
        Self {
            name: "Tree".into(),
            prefab: None,
            shape: ScatterShape::BroadleafTree,
            color: [0.25, 0.5, 0.2],
            density: 2.0,
            min_spacing: 4.0,
            scale: [0.8, 1.3],
            random_rotation: true,
            max_slope: 30.0,
            height_range: [0.0, 1.0],
            align_to_normal: false,
        }
    }
}

/// A biome asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Biome {
    pub name: String,
    /// sRGB tint for the biome's scatter (and its swatch in the editor).
    pub tone: [f32; 3],
    pub flat: FeatureLayer,
    pub slope: FeatureLayer,
    pub cliff: FeatureLayer,
    pub shore: FeatureLayer,
    pub peak: FeatureLayer,
    /// Ground steeper than this is Slope (degrees).
    pub slope_angle: f32,
    /// Ground steeper than this is Cliff (degrees).
    pub cliff_angle: f32,
    /// Peak / snow line as a fraction of the terrain's maximum height.
    pub peak_height: f32,
    /// Shore band above the water line (metres).
    pub shore_width: f32,
    pub scatter: Vec<ScatterItem>,
}

impl Default for Biome {
    fn default() -> Self {
        Self {
            name: "New Biome".into(),
            tone: [1.0; 3],
            flat: FeatureLayer::default(),
            slope: FeatureLayer::default(),
            cliff: FeatureLayer::default(),
            shore: FeatureLayer::default(),
            peak: FeatureLayer::default(),
            slope_angle: 22.0,
            cliff_angle: 40.0,
            peak_height: 0.75,
            shore_width: 2.5,
            scatter: Vec::new(),
        }
    }
}

impl Biome {
    pub fn layer(&self, f: Feature) -> &FeatureLayer {
        match f {
            Feature::Flat => &self.flat,
            Feature::Slope => &self.slope,
            Feature::Cliff => &self.cliff,
            Feature::Shore => &self.shore,
            Feature::Peak => &self.peak,
        }
    }

    pub fn layer_mut(&mut self, f: Feature) -> &mut FeatureLayer {
        match f {
            Feature::Flat => &mut self.flat,
            Feature::Slope => &mut self.slope,
            Feature::Cliff => &mut self.cliff,
            Feature::Shore => &mut self.shore,
            Feature::Peak => &mut self.peak,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

/// Built-in biome presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Forest,
    Desert,
    Grassland,
    SnowyMountains,
    BeachCoast,
    RockyBadlands,
}

/// Procedural textures the presets use (see [`super::textures`]), by file stem.
pub const TEXTURES: [&str; 7] = [
    "Grass",
    "Dirt",
    "Rock",
    "Sand",
    "Snow",
    "DesertSand",
    "RedRock",
];

/// Project path of a generated preset texture ("Assets/WorldBuilder/Textures/Rock.png").
pub fn texture_path(stem: &str) -> String {
    format!("{FOLDER}/Textures/{stem}.png")
}

fn tex(stem: &str, tiling: f32) -> FeatureLayer {
    FeatureLayer {
        texture: Some(texture_path(stem)),
        normal_map: None,
        tiling,
    }
}

fn item(
    name: &str,
    shape: ScatterShape,
    color: [f32; 3],
    density: f32,
    spacing: f32,
    max_slope: f32,
) -> ScatterItem {
    let rock = shape.is_rock();
    ScatterItem {
        name: name.into(),
        shape,
        color,
        density,
        min_spacing: spacing,
        max_slope,
        scale: if rock { [0.6, 1.8] } else { [0.8, 1.3] },
        align_to_normal: rock,
        ..Default::default()
    }
}

impl Preset {
    pub const ALL: [Self; 6] = [
        Self::Forest,
        Self::Desert,
        Self::Grassland,
        Self::SnowyMountains,
        Self::BeachCoast,
        Self::RockyBadlands,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Forest => "Forest",
            Self::Desert => "Desert",
            Self::Grassland => "Grassland",
            Self::SnowyMountains => "Snowy Mountains",
            Self::BeachCoast => "Beach",
            Self::RockyBadlands => "Rocky Badlands",
        }
    }

    /// Project path the preset is written to.
    pub fn path(self) -> String {
        format!("{FOLDER}/{}.{EXTENSION}", self.name())
    }

    pub fn biome(self) -> Biome {
        use ScatterShape as S;
        let rock = [0.42, 0.4, 0.38];
        let base = Biome {
            name: self.name().into(),
            cliff: tex("Rock", 12.0),
            shore: tex("Sand", 6.0),
            ..Default::default()
        };
        match self {
            Self::Forest => Biome {
                tone: [0.32, 0.55, 0.25],
                flat: tex("Grass", 6.0),
                slope: tex("Dirt", 8.0),
                peak: tex("Rock", 12.0),
                scatter: vec![
                    item("Oak", S::BroadleafTree, [0.22, 0.45, 0.16], 12.0, 5.0, 28.0),
                    item("Fir", S::PineTree, [0.12, 0.32, 0.16], 10.0, 4.0, 34.0),
                    item("Bush", S::Bush, [0.26, 0.46, 0.18], 8.0, 2.5, 32.0),
                    item("Rock", S::Rock, rock, 0.6, 4.0, 60.0),
                ],
                ..base
            },
            Self::Desert => Biome {
                tone: [0.85, 0.68, 0.42],
                flat: tex("DesertSand", 8.0),
                slope: tex("DesertSand", 8.0),
                cliff: tex("RedRock", 12.0),
                peak: tex("RedRock", 12.0),
                shore: tex("Sand", 6.0),
                scatter: vec![
                    item("Cactus", S::Cactus, [0.3, 0.5, 0.25], 0.25, 8.0, 20.0),
                    item("Rock", S::Rock, [0.6, 0.42, 0.3], 0.3, 5.0, 60.0),
                ],
                ..base
            },
            Self::Grassland => Biome {
                tone: [0.5, 0.68, 0.3],
                flat: tex("Grass", 6.0),
                slope: tex("Grass", 6.0),
                peak: tex("Dirt", 8.0),
                scatter: vec![
                    item("Grass", S::Grass, [0.4, 0.62, 0.22], 12.0, 1.2, 30.0),
                    item("Oak", S::BroadleafTree, [0.25, 0.5, 0.18], 0.15, 12.0, 20.0),
                    item("Bush", S::Bush, [0.3, 0.5, 0.2], 0.6, 4.0, 25.0),
                ],
                ..base
            },
            Self::SnowyMountains => Biome {
                tone: [0.85, 0.9, 0.95],
                flat: tex("Snow", 10.0),
                slope: tex("Rock", 12.0),
                peak: tex("Snow", 10.0),
                slope_angle: 28.0,
                cliff_angle: 38.0,
                peak_height: 0.55,
                scatter: vec![
                    item("Pine", S::PineTree, [0.1, 0.25, 0.15], 2.5, 5.0, 28.0),
                    item("Boulder", S::Boulder, [0.5, 0.5, 0.52], 0.8, 6.0, 50.0),
                ],
                ..base
            },
            Self::BeachCoast => Biome {
                tone: [0.95, 0.85, 0.6],
                flat: tex("Sand", 6.0),
                slope: tex("Sand", 6.0),
                peak: tex("Grass", 6.0),
                shore_width: 6.0,
                peak_height: 0.3,
                scatter: vec![
                    item("Rock", S::Rock, [0.55, 0.52, 0.48], 0.2, 6.0, 50.0),
                    item("Beach Grass", S::Grass, [0.6, 0.65, 0.35], 1.0, 2.0, 20.0),
                ],
                ..base
            },
            Self::RockyBadlands => Biome {
                tone: [0.7, 0.45, 0.32],
                flat: tex("Dirt", 8.0),
                slope: tex("RedRock", 12.0),
                cliff: tex("RedRock", 12.0),
                peak: tex("Rock", 12.0),
                slope_angle: 15.0,
                cliff_angle: 32.0,
                scatter: vec![
                    item("Boulder", S::Boulder, [0.55, 0.36, 0.26], 0.4, 6.0, 60.0),
                    item("Shrub", S::Bush, [0.45, 0.45, 0.25], 0.4, 4.0, 25.0),
                ],
                ..base
            },
        }
    }
}
