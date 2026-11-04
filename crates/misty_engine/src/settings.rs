//! Project Settings: per-project physics, time, input axes, tags & layers, player and quality.
//!
//! Stored in `<Project>/ProjectSettings/ProjectSettings.json`, which belongs in version control
//! (unlike `Library/`). Missing fields load as their defaults.

use std::path::{Path, PathBuf};

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Project-relative location of the settings file.
pub const SETTINGS_FILE: &str = "ProjectSettings/ProjectSettings.json";

/// Project Settings ▸ Physics.
///
/// Friction and bounciness are owned by each collider (no project-wide default material).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicsSettings {
    pub gravity: Vec3,
    /// Seconds per physics step (Time ▸ Fixed Timestep in the reference editor).
    pub fixed_timestep: f32,
    /// Upper bound of fixed steps per frame, so a slow frame can't spiral.
    pub max_steps_per_frame: u32,
    /// Raycasts and overlap queries also return trigger colliders.
    pub queries_hit_triggers: bool,
    /// Layer Collision Matrix: layer pairs that don't collide (unchecked boxes), smaller first.
    pub ignored_layer_pairs: Vec<[u8; 2]>,
}

impl PhysicsSettings {
    /// Do objects on layers `a` and `b` collide?
    pub fn layers_collide(&self, a: u8, b: u8) -> bool {
        !self.ignored_layer_pairs.contains(&[a.min(b), a.max(b)])
    }

    /// Tick or untick one box of the Layer Collision Matrix.
    pub fn set_layers_collide(&mut self, a: u8, b: u8, collide: bool) {
        let pair = [a.min(b), a.max(b)];
        self.ignored_layer_pairs.retain(|p| *p != pair);
        if !collide {
            self.ignored_layer_pairs.push(pair);
            self.ignored_layer_pairs.sort();
        }
    }

    /// Bit mask of the layers that `layer` collides with.
    pub fn collision_mask(&self, layer: u8) -> u32 {
        (0..32u8)
            .filter(|other| self.layers_collide(layer, *other))
            .fold(0, |m, other| m | 1 << other)
    }
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            fixed_timestep: 0.02,
            max_steps_per_frame: 8,
            queries_hit_triggers: true,
            ignored_layer_pairs: Vec::new(),
        }
    }
}

/// Project Settings ▸ Time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeSettings {
    pub time_scale: f32,
    /// Longest frame delta the game sees; long hitches are capped to this.
    pub maximum_allowed_timestep: f32,
}

impl Default for TimeSettings {
    fn default() -> Self {
        Self {
            time_scale: 1.0,
            maximum_allowed_timestep: 1.0 / 3.0,
        }
    }
}

/// Project Settings ▸ Physics 2D (the layer matrix is shared with 3D physics).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Physics2DSettings {
    pub gravity: glam::Vec2,
    /// Raycasts and overlap queries also return trigger colliders.
    pub queries_hit_triggers: bool,
    /// Raycasts starting inside a collider report it (at distance 0).
    pub queries_start_in_colliders: bool,
}

impl Default for Physics2DSettings {
    fn default() -> Self {
        Self {
            gravity: glam::Vec2::new(0.0, -9.81),
            queries_hit_triggers: true,
            queries_start_in_colliders: true,
        }
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_default_sorting(v: &Vec<String>) -> bool {
    *v == crate::sprites::sorting::default_layers()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub physics: PhysicsSettings,
    pub time: TimeSettings,
    /// Input Manager axes.
    pub input: crate::input::InputSettings,
    pub tags_and_layers: crate::tags_layers::TagsAndLayers,
    /// Left out of the file at its defaults (older settings files re-save unchanged).
    #[serde(skip_serializing_if = "is_default")]
    pub physics2d: Physics2DSettings,
    /// Sorting Layers in draw order ("Default" always present).
    #[serde(skip_serializing_if = "is_default_sorting")]
    pub sorting_layers: Vec<String>,
    /// Player page; `None` in projects made before it (see [`ProjectSettings::player_or`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player: Option<crate::player_settings::PlayerSettings>,
    /// Quality levels.
    #[serde(skip_serializing_if = "is_default")]
    pub quality: crate::quality::QualitySettings,
    /// Preset Manager: component or asset kind → preset applied when one is added or created.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub default_presets: std::collections::BTreeMap<String, String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            physics: PhysicsSettings::default(),
            time: TimeSettings::default(),
            input: Default::default(),
            tags_and_layers: Default::default(),
            physics2d: Physics2DSettings::default(),
            sorting_layers: crate::sprites::sorting::default_layers(),
            player: None,
            quality: Default::default(),
            default_presets: Default::default(),
        }
    }
}

impl ProjectSettings {
    pub fn path(root: &Path) -> PathBuf {
        root.join(SETTINGS_FILE)
    }

    /// Parse settings JSON; missing fields use defaults, invalid values are clamped.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut s: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        s.sanitize();
        Ok(s)
    }

    /// Load a project's settings. No file = defaults; an unreadable file is an error.
    pub fn load(root: &Path) -> Result<Self, String> {
        match crate::vfs::read_to_string(Self::path(root)) {
            Ok(text) => Self::from_json(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = Self::path(root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    /// Keep values in ranges the runtime can use (no zero or negative timesteps).
    pub fn sanitize(&mut self) {
        let p = &mut self.physics;
        if !p.gravity.is_finite() {
            p.gravity = PhysicsSettings::default().gravity;
        }
        p.fixed_timestep = clamp_or(p.fixed_timestep, 0.0001, 10.0, 0.02);
        p.max_steps_per_frame = p.max_steps_per_frame.clamp(1, 100);
        // Ordered, unique pairs of real layers.
        let mut pairs: Vec<[u8; 2]> = p
            .ignored_layer_pairs
            .iter()
            .filter(|[a, b]| *a < 32 && *b < 32)
            .map(|[a, b]| [*a.min(b), *a.max(b)])
            .collect();
        pairs.sort();
        pairs.dedup();
        p.ignored_layer_pairs = pairs;
        let t = &mut self.time;
        t.time_scale = clamp_or(t.time_scale, 0.0, 100.0, 1.0);
        t.maximum_allowed_timestep = clamp_or(t.maximum_allowed_timestep, 0.0001, 10.0, 1.0 / 3.0);
        self.input.sanitize();
        self.tags_and_layers.sanitize();
        if !self.physics2d.gravity.is_finite() {
            self.physics2d.gravity = Physics2DSettings::default().gravity;
        }
        self.sorting_layers = crate::sprites::sorting::normalized(&self.sorting_layers);
        if let Some(p) = self.player.as_mut() {
            p.sanitize();
        }
        self.quality.sanitize();
    }

    /// Player settings; older projects get theirs from the old Build Settings fields.
    pub fn player_or(
        &self,
        build: &crate::build::BuildSettings,
    ) -> crate::player_settings::PlayerSettings {
        self.player
            .clone()
            .unwrap_or_else(|| crate::player_settings::PlayerSettings::from_legacy(build))
    }

    /// Install what the renderer reads process-wide (Sorting Layers). Quality levels are
    /// installed by the editor and player with the level they start on (`quality::install`).
    pub fn apply_globals(&self) {
        crate::sprites::sorting::set_layers(&self.sorting_layers);
    }
}

fn clamp_or(v: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_a_project_folder() {
        let root = std::env::temp_dir().join("misty_settings_round_trip");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            ProjectSettings::load(&root).unwrap(),
            ProjectSettings::default()
        );
        let mut s = ProjectSettings::default();
        s.physics.gravity = Vec3::new(1.0, -3.0, 0.5);
        s.physics.fixed_timestep = 0.01;
        s.physics.max_steps_per_frame = 4;
        s.physics.queries_hit_triggers = false;
        s.physics.set_layers_collide(5, 0, false);
        s.time.time_scale = 0.5;
        s.time.maximum_allowed_timestep = 0.1;
        s.input.axes[0].sensitivity = 5.0;
        s.input.axes.pop();
        s.tags_and_layers.tags.push("Enemy".into());
        s.tags_and_layers.layers[8] = "Hidden".into();
        s.save(&root).unwrap();
        assert!(root.join("ProjectSettings/ProjectSettings.json").is_file());
        assert_eq!(ProjectSettings::load(&root).unwrap(), s);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s = ProjectSettings::from_json(r#"{ "physics": { "gravity": [0, -20, 0] } }"#).unwrap();
        assert_eq!(s.physics.gravity, Vec3::new(0.0, -20.0, 0.0));
        assert_eq!(s.physics.fixed_timestep, 0.02);
        assert_eq!(s.physics.max_steps_per_frame, 8);
        assert_eq!(s.time, TimeSettings::default());
        // Files from before the Input Manager / Tags and Layers pages.
        assert_eq!(s.input, crate::input::InputSettings::default());
        assert_eq!(s.tags_and_layers.name_to_layer("Player"), 5);
        assert_eq!(
            ProjectSettings::from_json("{}").unwrap(),
            ProjectSettings::default()
        );
        assert!(ProjectSettings::from_json("not json").is_err());
    }

    #[test]
    fn layer_collision_matrix() {
        let mut p = PhysicsSettings::default();
        assert!(p.layers_collide(0, 5));
        assert_eq!(p.collision_mask(3), u32::MAX);
        p.set_layers_collide(5, 2, false);
        assert!(!p.layers_collide(2, 5) && !p.layers_collide(5, 2));
        assert_eq!(p.ignored_layer_pairs, vec![[2, 5]]);
        assert_eq!(p.collision_mask(5), !(1 << 2));
        p.set_layers_collide(2, 5, true);
        assert!(p.ignored_layer_pairs.is_empty());
        let s = ProjectSettings::from_json(
            r#"{ "physics": { "ignored_layer_pairs": [[4, 1], [1, 4], [40, 1]] } }"#,
        )
        .unwrap();
        assert_eq!(s.physics.ignored_layer_pairs, vec![[1, 4]]);
    }

    #[test]
    fn physics2d_and_sorting_layers_round_trip_and_stay_out_at_defaults() {
        let text = serde_json::to_string(&ProjectSettings::default()).unwrap();
        assert!(!text.contains("physics2d") && !text.contains("sorting_layers"));
        let mut s = ProjectSettings::default();
        s.physics2d.gravity = glam::Vec2::new(0.0, -20.0);
        s.sorting_layers = vec!["Background".into(), "Default".into(), "UI".into()];
        let back = ProjectSettings::from_json(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let fixed = ProjectSettings::from_json(r#"{ "sorting_layers": ["Front"] }"#).unwrap();
        assert_eq!(fixed.sorting_layers, vec!["Default", "Front"]);
    }

    #[test]
    fn player_and_quality_stay_out_at_defaults_and_round_trip() {
        let text = serde_json::to_string(&ProjectSettings::default()).unwrap();
        assert!(!text.contains("player") && !text.contains("quality"));
        let mut s = ProjectSettings::default();
        let build = crate::build::BuildSettings {
            product_name: "Old".into(),
            ..Default::default()
        };
        assert_eq!(s.player_or(&build).product_name, "Old");
        s.player = Some(crate::player_settings::PlayerSettings {
            product_name: "New".into(),
            ..Default::default()
        });
        s.quality.default_level = 1;
        s.quality.levels[3].render_scale = 0.5;
        let back = ProjectSettings::from_json(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.player_or(&build).product_name, "New");
    }

    #[test]
    fn invalid_values_are_clamped() {
        let s = ProjectSettings::from_json(
            r#"{ "physics": { "fixed_timestep": 0, "max_steps_per_frame": 0 },
                 "time": { "time_scale": -1 } }"#,
        )
        .unwrap();
        assert!(s.physics.fixed_timestep > 0.0);
        assert_eq!(s.physics.max_steps_per_frame, 1);
        assert_eq!(s.time.time_scale, 0.0);
    }
}
