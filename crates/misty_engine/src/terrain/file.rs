//! `.terrain` files: JSON with the settings and layers in the clear and the two big arrays as
//! base64 (heights as 16-bit little-endian integers, the splat map as RGBA bytes).

use std::path::Path;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::cliffs::CliffSettings;
use super::data::{TerrainData, TerrainLayer};
use super::detail_layers::DetailLayer;
use super::details::DetailSettings;
use super::flowers::FlowerSettings;
use super::harmony::HarmonySettings;
use super::trees::TreeSettings;

/// File extension of terrain data assets.
pub const EXTENSION: &str = "terrain";

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct TerrainFile {
    version: u32,
    heightmap_resolution: usize,
    size: [f32; 3],
    alphamap_resolution: usize,
    layers: Vec<TerrainLayer>,
    /// base64 of `u16` LE heights (0..65535 = 0..1).
    heights: String,
    /// base64 of RGBA weights.
    alphamap: String,
    // Added after version 1; older files load with the defaults.
    rule_seed: u32,
    cliffs: CliffSettings,
    harmony: HarmonySettings,
    details: DetailSettings,
    /// base64 of one detail density byte per splat texel (empty = none).
    detail_map: String,
    /// Left out while there are no flowers (older files stay as they were).
    #[serde(skip_serializing_if = "FlowerSettings::is_default")]
    flowers: FlowerSettings,
    /// Left out while there are no trees (older files stay as they were).
    #[serde(skip_serializing_if = "TreeSettings::is_default")]
    trees: TreeSettings,
    /// base64 of the hole bits, one per heightmap cell (8 to a byte, low bit first); left
    /// out while there are no holes.
    #[serde(skip_serializing_if = "String::is_empty")]
    holes: String,
    /// base64 of the RGBA weights of layers 5.. (one map after another); left out with four
    /// layers or fewer.
    #[serde(skip_serializing_if = "String::is_empty")]
    alphamap_extra: String,
    /// Grass species and detail meshes (left out while there are none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    detail_layers: Vec<DetailLayer>,
}

impl Default for TerrainFile {
    fn default() -> Self {
        Self {
            version: 1,
            heightmap_resolution: 0,
            size: [1.0; 3],
            alphamap_resolution: 0,
            layers: Vec::new(),
            heights: String::new(),
            alphamap: String::new(),
            rule_seed: 1,
            cliffs: CliffSettings::default(),
            harmony: HarmonySettings::default(),
            details: DetailSettings::default(),
            detail_map: String::new(),
            flowers: FlowerSettings::default(),
            trees: TreeSettings::default(),
            holes: String::new(),
            alphamap_extra: String::new(),
            detail_layers: Vec::new(),
        }
    }
}

impl TerrainData {
    /// Serialise to the `.terrain` text format.
    pub fn to_file_string(&self) -> String {
        let heights: Vec<u8> = self
            .heights
            .iter()
            .flat_map(|h| quantize(*h).to_le_bytes())
            .collect();
        let alphamap: Vec<u8> = self.alphamap.iter().flatten().copied().collect();
        let file = TerrainFile {
            version: 1,
            heightmap_resolution: self.heightmap_resolution,
            size: self.size.to_array(),
            alphamap_resolution: self.alphamap_resolution,
            layers: self.layers.clone(),
            heights: encode(&heights),
            alphamap: encode(&alphamap),
            rule_seed: self.rule_seed,
            cliffs: self.cliffs.clone(),
            harmony: self.harmony.clone(),
            details: self.details.clone(),
            detail_map: encode(&self.detail_map),
            flowers: self.flowers.clone(),
            trees: self.trees.clone(),
            holes: if self.has_holes() {
                encode(&pack_bits(&self.holes))
            } else {
                String::new()
            },
            alphamap_extra: encode(&self.alphamap_extra.concat()),
            detail_layers: self.detail_layers.clone(),
        };
        serde_json::to_string_pretty(&file).expect("terrain serialises")
    }

    pub fn from_file_str(text: &str) -> Result<Self, String> {
        let f: TerrainFile = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        let heights = decode(&f.heights)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b) as f32 / 65535.0)
            .collect();
        let alphamap = decode(&f.alphamap)?.as_chunks::<4>().0.to_vec();
        let mut d = TerrainData {
            heightmap_resolution: f.heightmap_resolution,
            size: Vec3::from_array(f.size),
            heights,
            alphamap_resolution: f.alphamap_resolution,
            alphamap,
            alphamap_extra: decode(&f.alphamap_extra)?.as_chunks::<4>().0.to_vec(),
            layers: f.layers,
            rule_seed: f.rule_seed,
            cliffs: f.cliffs,
            harmony: f.harmony,
            details: f.details,
            detail_map: decode(&f.detail_map)?,
            flowers: f.flowers,
            trees: f.trees,
            holes: Vec::new(),
            detail_layers: f.detail_layers,
        };
        if !f.holes.is_empty() {
            let cells = f.heightmap_resolution.saturating_sub(1);
            d.holes = unpack_bits(&decode(&f.holes)?, cells * cells);
        }
        d.sanitize();
        Ok(d)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_file_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_file_string()))
            .map_err(|e| e.to_string())
    }
}

/// One bit per value (non-zero = set), 8 to a byte, low bit first.
pub(super) fn pack_bits(v: &[u8]) -> Vec<u8> {
    v.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |acc, (i, b)| acc | (u8::from(*b != 0) << i))
        })
        .collect()
}

/// `n` values (0 / 1) from [`pack_bits`] output.
pub(super) fn unpack_bits(bytes: &[u8], n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| bytes.get(i / 8).map_or(0, |b| (b >> (i % 8)) & 1))
        .collect()
}

/// Normalised height to the stored 16-bit value.
pub(super) fn quantize(h: f32) -> u16 {
    (h.clamp(0.0, 1.0) * 65535.0).round() as u16
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub(super) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(super) fn decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("invalid base64 character '{}'", c as char)),
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}
