//! Project Settings: per-project physics and time configuration.
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
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            fixed_timestep: 0.02,
            max_steps_per_frame: 8,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub physics: PhysicsSettings,
    pub time: TimeSettings,
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
        match std::fs::read_to_string(Self::path(root)) {
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
        let t = &mut self.time;
        t.time_scale = clamp_or(t.time_scale, 0.0, 100.0, 1.0);
        t.maximum_allowed_timestep = clamp_or(t.maximum_allowed_timestep, 0.0001, 10.0, 1.0 / 3.0);
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
        s.time.time_scale = 0.5;
        s.time.maximum_allowed_timestep = 0.1;
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
        assert_eq!(
            ProjectSettings::from_json("{}").unwrap(),
            ProjectSettings::default()
        );
        assert!(ProjectSettings::from_json("not json").is_err());
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
