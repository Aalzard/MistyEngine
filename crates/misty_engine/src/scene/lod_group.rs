//! LOD Group: picks which of several renderer sets draws, by how tall the group appears on
//! screen. Level `i` is used while the group's screen-relative height is at least
//! `lods[i].screen_height` (and below the previous level's); under the last one it is culled.

use hecs::Entity;
use serde::{Deserialize, Serialize};

/// How neighbouring levels switch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LodFadeMode {
    /// Switch instantly at the threshold.
    #[default]
    None,
    /// Dithered cross-fade over `fade_width` of each threshold.
    CrossFade,
}

/// One level: its threshold and the objects whose Mesh Renderers (with their children's) draw.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LodLevel {
    /// Lowest screen-relative height (0..1) this level is used at.
    pub screen_height: f32,
    #[serde(with = "renderer_list")]
    pub renderers: Vec<Entity>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LodGroup {
    pub enabled: bool,
    pub fade_mode: LodFadeMode,
    /// Cross-fade band as a fraction of each threshold.
    pub fade_width: f32,
    /// Finest first; thresholds decrease.
    pub lods: Vec<LodLevel>,
}

impl Default for LodGroup {
    fn default() -> Self {
        let level = |screen_height| LodLevel {
            screen_height,
            renderers: Vec::new(),
        };
        Self {
            enabled: true,
            fade_mode: LodFadeMode::None,
            fade_width: 0.1,
            lods: vec![level(0.6), level(0.3), level(0.1)],
        }
    }
}

impl LodGroup {
    pub const MAX_LEVELS: usize = 8;

    /// Screen-height range `min..max` of level `i` (`max` = infinity for LOD 0).
    pub fn range(&self, i: usize) -> (f32, f32) {
        let max = match i {
            0 => f32::INFINITY,
            _ => self.lods[i - 1].screen_height,
        };
        (self.lods[i].screen_height, max)
    }

    /// Index of the level drawn at screen height `h` (`None` = culled).
    pub fn level_at(&self, h: f32) -> Option<usize> {
        self.lods.iter().position(|l| h >= l.screen_height)
    }

    /// The cross-fade band used by the renderer (0 when fading is off).
    pub fn fade(&self) -> f32 {
        match self.fade_mode {
            LodFadeMode::None => 0.0,
            LodFadeMode::CrossFade => self.fade_width.clamp(0.0, 1.0),
        }
    }

    /// Keep thresholds within 0..1 and never above the previous level's.
    pub fn clamp_thresholds(&mut self) {
        let mut hi = 1.0f32;
        for l in &mut self.lods {
            l.screen_height = l.screen_height.clamp(0.0, hi);
            hi = l.screen_height;
        }
    }
}

/// Entity lists that drop references a scene file could not map (null) instead of failing.
mod renderer_list {
    use hecs::Entity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[Entity], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|e| e.to_bits().get())
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Entity>, D::Error> {
        let bits = Vec::<Option<u64>>::deserialize(d)?;
        Ok(bits
            .into_iter()
            .flatten()
            .filter_map(Entity::from_bits)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_ranges_and_lossy_references() {
        let g = LodGroup::default();
        assert_eq!(g.level_at(0.8), Some(0));
        assert_eq!(g.level_at(0.45), Some(1));
        assert_eq!(g.level_at(0.12), Some(2));
        assert_eq!(g.level_at(0.05), None);
        assert_eq!(g.range(0), (0.6, f32::INFINITY));
        assert_eq!(g.range(2), (0.1, 0.3));
        let l: LodLevel =
            serde_json::from_str(r#"{"screen_height":0.5,"renderers":[null]}"#).unwrap();
        assert!(l.renderers.is_empty());

        let mut g = LodGroup::default();
        g.lods[1].screen_height = 0.9;
        g.clamp_thresholds();
        assert!(g.lods[0].screen_height >= g.lods[1].screen_height);
        assert!(
            g.lods[1].screen_height <= 1.0 && g.lods[1].screen_height >= g.lods[2].screen_height
        );
    }
}
