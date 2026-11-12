//! LOD Group: picks which of several renderer sets draws, by how tall the group appears on
//! screen. Level `i` is used while the group's screen-relative height is at least
//! `lods[i].screen_height` (and below the previous level's); under the last one it is culled.
//! With Cross Fade, both neighbouring levels draw through complementary dithers in a band of
//! `fade_width` × the threshold above it (render/lod.rs), so a switch never pops.

use hecs::Entity;
use serde::{Deserialize, Serialize};

/// How neighbouring levels switch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LodFadeMode {
    /// Switch instantly at the threshold. What a file saved without a fade mode loads as
    /// (groups made before cross-fades were the default keep their look).
    #[default]
    None,
    /// Dithered cross-fade over `fade_width` of each threshold (the default of new groups).
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
    /// Missing in a file = `None` (the field's own default), not the new groups' Cross Fade.
    #[serde(default)]
    pub fade_mode: LodFadeMode,
    /// Cross-fade band as a fraction of each threshold (0.1 ≈ a tenth of the switch distance).
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
            fade_mode: LodFadeMode::CrossFade,
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

    #[test]
    fn new_groups_cross_fade_and_old_files_keep_switching_instantly() {
        let g = LodGroup::default();
        assert_eq!(g.fade_mode, LodFadeMode::CrossFade);
        assert!((g.fade() - 0.1).abs() < 1e-6, "{}", g.fade());
        // A group saved before the fade settings existed (no fade_mode, no fade_width).
        let old: LodGroup = serde_json::from_str(
            r#"{"enabled":true,"lods":[{"renderers":[],"screen_height":0.5},{"renderers":[],"screen_height":0.1}]}"#,
        )
        .unwrap();
        assert_eq!(old.fade_mode, LodFadeMode::None);
        assert_eq!(old.fade(), 0.0);
        assert_eq!(old.lods.len(), 2);
        // Saved later with the mode written out: loads as written.
        let none: LodGroup = serde_json::from_str(r#"{"fade_mode":"None","fade_width":0.3}"#)
            .unwrap();
        assert_eq!((none.fade_mode, none.fade()), (LodFadeMode::None, 0.0));
    }

    #[test]
    fn fade_settings_round_trip_through_a_saved_scene() {
        let mut scene = crate::scene::Scene::new("Fade");
        let root = scene.spawn("Rock", None);
        let lod0 = scene.spawn("LOD0", Some(root));
        let lod1 = scene.spawn("LOD1", Some(root));
        for (fade_mode, fade_width) in [(LodFadeMode::CrossFade, 0.25), (LodFadeMode::None, 0.05)]
        {
            let g = LodGroup {
                fade_mode,
                fade_width,
                lods: vec![
                    LodLevel {
                        screen_height: 0.4,
                        renderers: vec![lod0],
                    },
                    LodLevel {
                        screen_height: 0.05,
                        renderers: vec![lod1],
                    },
                ],
                ..Default::default()
            };
            scene.add(root, g);
            let text = scene.to_json();
            let (back, report) = crate::scene::Scene::from_json("Fade", &text).unwrap();
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            let rock = back.find_by_name("Rock").unwrap();
            let loaded = back.world.get::<&LodGroup>(rock).unwrap();
            assert_eq!(loaded.fade_mode, fade_mode);
            assert_eq!(loaded.fade_width, fade_width);
            assert_eq!(loaded.lods.len(), 2);
            let names: Vec<String> = loaded
                .lods
                .iter()
                .map(|l| back.name(l.renderers[0]))
                .collect();
            assert_eq!(names, ["LOD0", "LOD1"]);
        }
    }
}
