//! Grid and Snap settings (Edit ▸ Grid and Snap), saved per project in `Library/SnapSettings.json`,
//! and the snapping maths used by the transform handles.
//!
//! Increment snapping (hold Ctrl) rounds the drag *delta*; grid snapping (Scene toolbar toggle)
//! rounds the handle's *world position* to the grid, like the familiar editor.

use std::path::Path;

use glam::Vec3;
use serde::{Deserialize, Serialize};

const FILE: &str = "Library/SnapSettings.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSettings {
    /// World grid cell size per axis (grid snapping, Align Selected to Grid).
    pub grid_size: [f32; 3],
    /// Ctrl-drag move increment per axis.
    pub move_step: [f32; 3],
    /// Ctrl-drag rotation increment in degrees.
    pub rotate_step: f32,
    /// Ctrl-drag scale factor increment.
    pub scale_step: f32,
    /// Scene toolbar "Grid Snapping": moves snap to the world grid.
    pub grid_snap: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            grid_size: [1.0; 3],
            move_step: [0.25; 3],
            rotate_step: 15.0,
            scale_step: 0.1,
            grid_snap: false,
        }
    }
}

impl SnapSettings {
    /// Settings saved in `root`'s Library, or defaults.
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = root.join(FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

/// `v` rounded to the nearest multiple of `step` (unchanged when `step` is not positive).
pub fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// Snap a move delta along each handle axis by that axis' increment.
pub fn snap_delta(delta: Vec3, axes: [Vec3; 3], step: [f32; 3]) -> Vec3 {
    (0..3)
        .map(|i| axes[i] * snap(delta.dot(axes[i]), step[i]))
        .sum()
}

/// World position with the world axes in `mask` rounded to the grid.
pub fn snap_to_grid(p: Vec3, grid: [f32; 3], mask: [bool; 3]) -> Vec3 {
    let mut out = p;
    for i in 0..3 {
        if mask[i] {
            out[i] = snap(p[i], grid[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_rounds_to_increments() {
        assert_eq!(snap(0.3, 0.25), 0.25);
        assert_eq!(snap(0.38, 0.25), 0.5);
        assert_eq!(snap(-0.13, 0.25), -0.25);
        assert_eq!(snap(22.0, 15.0), 15.0);
        assert_eq!(snap(23.0, 15.0), 30.0);
        assert!((snap(1.26, 0.1) - 1.3).abs() < 1e-6);
        assert_eq!(snap(0.37, 0.0), 0.37, "zero step leaves the value alone");
    }

    #[test]
    fn delta_snaps_per_axis_in_handle_space() {
        let world = [Vec3::X, Vec3::Y, Vec3::Z];
        let d = snap_delta(Vec3::new(0.3, 0.9, -0.6), world, [0.25, 1.0, 0.5]);
        assert_eq!(d, Vec3::new(0.25, 1.0, -0.5));
        // Rotated handle: the delta is snapped along the handle's own axes.
        let q = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let axes = [q * Vec3::X, q * Vec3::Y, q * Vec3::Z];
        let d = snap_delta(axes[0] * 0.3, axes, [0.25; 3]);
        assert!((d - axes[0] * 0.25).length() < 1e-5);
    }

    #[test]
    fn grid_snap_only_touches_masked_axes() {
        let p = Vec3::new(1.4, 2.6, -0.4);
        assert_eq!(
            snap_to_grid(p, [1.0; 3], [true, false, true]),
            Vec3::new(1.0, 2.6, -0.0)
        );
        assert_eq!(
            snap_to_grid(p, [0.5, 2.0, 1.0], [true; 3]),
            Vec3::new(1.5, 2.0, 0.0)
        );
    }

    #[test]
    fn settings_round_trip_through_library() {
        let root = std::env::temp_dir().join(format!("misty_snap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(SnapSettings::load(&root), SnapSettings::default());
        let s = SnapSettings {
            move_step: [0.5, 1.0, 2.0],
            rotate_step: 45.0,
            grid_snap: true,
            ..Default::default()
        };
        s.save(&root).unwrap();
        assert!(root.join("Library/SnapSettings.json").exists());
        assert_eq!(SnapSettings::load(&root), s);
        std::fs::remove_dir_all(root).ok();
    }
}
