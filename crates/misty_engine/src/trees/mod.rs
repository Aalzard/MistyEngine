//! Procedural trees: the `.tree` asset (a small JSON recipe: size, crown shape, branching,
//! leaves, bark and leaf textures, wind response) and the meshes built from it.
//!
//! Like the reference engine's tree assets, a tree is authored once and placed many times on a
//! terrain (`terrain::trees`): the renderer draws every placed copy with GPU instancing, a full
//! mesh close up, a lighter mesh further away and a baked billboard (impostor) far away.
//!
//! [`generate::build`] turns a recipe into a [`generate::TreeModel`] (deterministic for a given
//! recipe and textures); [`store`] caches built models per asset file.

mod cache;
pub mod generate;
pub mod impostor;
pub mod store;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File extension of tree assets.
pub const EXTENSION: &str = "tree";

/// Outline of the crown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrownShape {
    /// A full dome, widest halfway up (lime, field maple).
    #[default]
    Round,
    /// Taller than wide, widest low down (young trees, birch).
    Oval,
    /// Broad and flat-topped, widest high up (old oak in the open).
    Spreading,
    /// Slim column (poplar-like).
    Narrow,
}

impl CrownShape {
    pub const ALL: [CrownShape; 4] = [
        CrownShape::Round,
        CrownShape::Oval,
        CrownShape::Spreading,
        CrownShape::Narrow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CrownShape::Round => "Round",
            CrownShape::Oval => "Oval",
            CrownShape::Spreading => "Spreading",
            CrownShape::Narrow => "Narrow",
        }
    }

    /// Crown radius at `u` (0 = crown bottom .. 1 = top) as a share of the widest radius.
    pub fn profile(self, u: f32) -> f32 {
        let u = u.clamp(0.0, 1.0);
        // sin(π u^a)^b: `a` moves the widest point (0.5^(1/a)), `b` fills the outline out.
        let (a, b) = match self {
            CrownShape::Round => (1.0, 0.5),
            CrownShape::Oval => (0.8, 0.6),
            CrownShape::Spreading => (1.45, 0.38),
            CrownShape::Narrow => (0.9, 0.75),
        };
        (std::f32::consts::PI * u.powf(a)).sin().max(0.0).powf(b)
    }
}

/// How a tree moves in the wind (multipliers of the terrain's wind).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeWind {
    /// The whole tree bending from its foot (0 = rigid).
    pub trunk_bend: f32,
    /// Limbs and their twigs swaying.
    pub branch_sway: f32,
    /// Leaf clusters fluttering.
    pub leaf_flutter: f32,
}

impl Default for TreeWind {
    fn default() -> Self {
        Self {
            trunk_bend: 1.0,
            branch_sway: 1.0,
            leaf_flutter: 1.0,
        }
    }
}

/// A tree recipe (`.tree` file). Every field has a sensible default; the presets
/// ([`TreeAsset::oak`] and friends) are good starting points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeAsset {
    /// Changes the branching and leaves while keeping the overall shape.
    pub seed: u32,
    /// Metres from the foot to the top of the crown.
    pub height: f32,
    pub crown_shape: CrownShape,
    /// Crown diameter as a share of the height.
    pub crown_width: f32,
    /// Where the crown starts, as a share of the height (the bare trunk below).
    pub crown_base: f32,
    /// Trunk radius at the foot (metres, above the root flare).
    pub trunk_radius: f32,
    /// 0 = the trunk divides into limbs inside the crown (oak, lime), 1 = it runs up to the
    /// top with the limbs on its sides (birch, young trees).
    pub leader: f32,
    /// Lean and wobble of the trunk (0 = straight).
    pub trunk_bend: f32,
    /// Main limbs.
    pub branches: u32,
    /// Angle between the limbs and the trunk where they leave it (degrees).
    pub branch_angle: f32,
    /// The limbs' curve: up towards the light (> 0) or drooping (< 0).
    pub branch_curve: f32,
    /// Side branches per limb (the longest limb; shorter ones get fewer).
    pub twigs: u32,
    /// How thickly the leaf clusters cover the crown (1 = full).
    pub leaf_density: f32,
    /// Side of one leaf cluster card (metres).
    pub leaf_size: f32,
    /// Leaf tint (sRGB, multiplies the leaf texture).
    pub leaf_color: [f32; 3],
    /// Per-cluster and per-tree colour differences (0 = all alike).
    pub leaf_variation: f32,
    /// Sunlight shining through the leaves (0 = opaque).
    pub translucency: f32,
    /// 0..1: deep shade inside and under the crown, as in a high sun: inner and lower leaf
    /// clusters get less sunlight and sky light, the sunlit outer dome stays bright
    /// (0 = the soft, evenly lit crown).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub crown_shading: f32,
    /// 0..1: leaf clusters gather into rounded lumps with gaps between them (a cauliflower
    /// crown of many sunlit lumps and dark hollows; 0 = clusters spread evenly over the crown).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clumping: f32,
    /// Bark tint (sRGB, multiplies the bark texture).
    pub bark_color: [f32; 3],
    /// Project-relative textures: bark albedo and normal map (square, tileable; u around the
    /// trunk), leaf atlas (2 × 2 clusters, alpha = cut-out) and its normal map.
    pub bark_texture: String,
    pub bark_normal: String,
    pub leaf_texture: String,
    pub leaf_normal: String,
    /// Optional grayscale maps (empty = plain values): bark roughness, ambient occlusion and
    /// height (parallax; 0 = deepest furrow), leaf roughness and translucency (how much
    /// sunlight the leaf lets through).
    #[serde(default)]
    pub bark_roughness: String,
    #[serde(default)]
    pub bark_occlusion: String,
    #[serde(default)]
    pub bark_height: String,
    #[serde(default)]
    pub leaf_roughness: String,
    #[serde(default)]
    pub leaf_translucency: String,
    pub wind: TreeWind,
}

impl Default for TreeAsset {
    fn default() -> Self {
        Self::oak()
    }
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Folder of the sample tree textures (SampleProject).
pub const SAMPLE_TEXTURES: &str = "Assets/Trees/Textures";

fn sample_texture(name: &str) -> String {
    format!("{SAMPLE_TEXTURES}/{name}.png")
}

impl TreeAsset {
    /// A broad meadow oak: short sturdy trunk dividing into a few big limbs, a wide rounded
    /// crown of dark green lobed leaves.
    pub fn oak() -> Self {
        Self {
            seed: 1,
            height: 14.0,
            crown_shape: CrownShape::Spreading,
            crown_width: 0.95,
            crown_base: 0.25,
            trunk_radius: 0.38,
            leader: 0.15,
            trunk_bend: 0.35,
            branches: 9,
            branch_angle: 60.0,
            branch_curve: 0.35,
            twigs: 12,
            leaf_density: 1.6,
            leaf_size: 1.4,
            leaf_color: [0.9, 0.9, 0.85],
            leaf_variation: 0.5,
            translucency: 0.6,
            crown_shading: 0.0,
            clumping: 0.0,
            bark_color: [1.0, 1.0, 1.0],
            bark_texture: sample_texture("OakBark"),
            bark_normal: sample_texture("OakBarkNormal"),
            leaf_texture: sample_texture("OakLeaves"),
            leaf_normal: sample_texture("OakLeavesNormal"),
            bark_roughness: sample_texture("OakBarkRoughness"),
            bark_occlusion: sample_texture("OakBarkAO"),
            bark_height: sample_texture("OakBarkHeight"),
            leaf_roughness: sample_texture("OakLeavesRoughness"),
            leaf_translucency: sample_texture("OakLeavesTranslucency"),
            wind: TreeWind {
                trunk_bend: 0.7,
                branch_sway: 0.9,
                leaf_flutter: 1.0,
            },
        }
    }

    /// A lime (linden): straight trunk, tall dome of fresh green heart-shaped leaves.
    pub fn linden() -> Self {
        Self {
            seed: 3,
            height: 17.0,
            crown_shape: CrownShape::Oval,
            crown_width: 0.62,
            crown_base: 0.22,
            trunk_radius: 0.34,
            leader: 0.6,
            trunk_bend: 0.15,
            branches: 14,
            branch_angle: 50.0,
            branch_curve: 0.45,
            twigs: 9,
            leaf_density: 1.6,
            leaf_size: 1.25,
            leaf_color: [0.88, 0.9, 0.85],
            bark_texture: sample_texture("LindenBark"),
            bark_normal: sample_texture("LindenBarkNormal"),
            leaf_texture: sample_texture("LindenLeaves"),
            leaf_normal: sample_texture("LindenLeavesNormal"),
            bark_roughness: sample_texture("LindenBarkRoughness"),
            bark_occlusion: sample_texture("LindenBarkAO"),
            bark_height: sample_texture("LindenBarkHeight"),
            leaf_roughness: sample_texture("LindenLeavesRoughness"),
            leaf_translucency: sample_texture("LindenLeavesTranslucency"),
            ..Self::oak()
        }
    }

    /// A silver birch: slim white trunk to the top, light airy crown of small leaves.
    pub fn birch() -> Self {
        Self {
            seed: 5,
            height: 15.0,
            crown_shape: CrownShape::Oval,
            crown_width: 0.42,
            crown_base: 0.3,
            trunk_radius: 0.2,
            leader: 1.0,
            trunk_bend: 0.3,
            branches: 16,
            branch_angle: 48.0,
            branch_curve: -0.25,
            twigs: 8,
            leaf_density: 1.35,
            leaf_size: 0.95,
            translucency: 0.8,
            bark_texture: sample_texture("BirchBark"),
            bark_normal: sample_texture("BirchBarkNormal"),
            leaf_texture: sample_texture("BirchLeaves"),
            leaf_normal: sample_texture("BirchLeavesNormal"),
            bark_roughness: sample_texture("BirchBarkRoughness"),
            bark_occlusion: sample_texture("BirchBarkAO"),
            bark_height: sample_texture("BirchBarkHeight"),
            leaf_roughness: sample_texture("BirchLeavesRoughness"),
            leaf_translucency: sample_texture("BirchLeavesTranslucency"),
            wind: TreeWind {
                trunk_bend: 1.2,
                branch_sway: 1.3,
                leaf_flutter: 1.4,
            },
            ..Self::oak()
        }
    }

    /// Every texture the recipe names (empty ones included).
    pub fn textures(&self) -> [&String; 9] {
        [
            &self.bark_texture,
            &self.bark_normal,
            &self.bark_roughness,
            &self.bark_occlusion,
            &self.bark_height,
            &self.leaf_texture,
            &self.leaf_normal,
            &self.leaf_roughness,
            &self.leaf_translucency,
        ]
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut t: TreeAsset = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        t.sanitize();
        Ok(t)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("tree asset serialises")
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| e.to_string())
    }

    /// Clamp every value into its usable range (hand-edited or older files).
    pub fn sanitize(&mut self) {
        let fix = |v: &mut f32, lo: f32, hi: f32, default: f32| {
            *v = if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            };
        };
        fix(&mut self.height, 1.0, 60.0, 14.0);
        fix(&mut self.crown_width, 0.1, 2.0, 0.8);
        fix(&mut self.crown_base, 0.0, 0.9, 0.3);
        fix(&mut self.trunk_radius, 0.02, 2.0, 0.3);
        fix(&mut self.leader, 0.0, 1.0, 0.3);
        fix(&mut self.trunk_bend, 0.0, 1.0, 0.2);
        self.branches = self.branches.clamp(1, 40);
        fix(&mut self.branch_angle, 5.0, 110.0, 55.0);
        fix(&mut self.branch_curve, -1.0, 1.0, 0.3);
        self.twigs = self.twigs.min(30);
        fix(&mut self.leaf_density, 0.0, 3.0, 1.0);
        fix(&mut self.leaf_size, 0.1, 4.0, 1.0);
        fix(&mut self.leaf_variation, 0.0, 1.0, 0.5);
        fix(&mut self.translucency, 0.0, 2.0, 0.6);
        fix(&mut self.crown_shading, 0.0, 1.0, 0.0);
        fix(&mut self.clumping, 0.0, 1.0, 0.0);
        for c in self.leaf_color.iter_mut().chain(&mut self.bark_color) {
            fix(c, 0.0, 2.0, 1.0);
        }
        fix(&mut self.wind.trunk_bend, 0.0, 4.0, 1.0);
        fix(&mut self.wind.branch_sway, 0.0, 4.0, 1.0);
        fix(&mut self.wind.leaf_flutter, 0.0, 4.0, 1.0);
    }
}

#[cfg(test)]
mod tests;
