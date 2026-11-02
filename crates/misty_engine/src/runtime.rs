//! Game loop time and the per-frame update that runs while playing.

use glam::Vec3;

use crate::physics::Physics;
use crate::scene::Scene;

/// Like the familiar `Time` class: elapsed time, frame delta, frame count, time scale.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    /// Seconds since Play started (scaled).
    pub time: f64,
    /// Scaled seconds since the previous frame.
    pub delta: f32,
    /// Real seconds since the previous frame.
    pub unscaled_delta: f32,
    pub frame: u64,
    pub time_scale: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            time: 0.0,
            delta: 0.0,
            unscaled_delta: 0.0,
            frame: 0,
            time_scale: 1.0,
        }
    }
}

impl Time {
    /// Advance by `real_dt` seconds of wall-clock time (capped like the maximum allowed timestep).
    pub fn advance(&mut self, real_dt: f32) {
        let dt = real_dt.clamp(0.0, 1.0 / 3.0);
        self.unscaled_delta = dt;
        self.delta = dt * self.time_scale;
        self.time += self.delta as f64;
        self.frame += 1;
    }
}

/// Gravity used by physics (Project Settings ▸ Physics default).
pub const DEFAULT_GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

/// Everything that lives only while playing: physics now, scripts in step 8.
pub struct Runtime {
    pub physics: Physics,
}

impl Runtime {
    /// Build runtime state from the scene as Play starts.
    pub fn start(scene: &Scene) -> Self {
        Self {
            physics: Physics::build(scene, DEFAULT_GRAVITY),
        }
    }

    /// One game frame.
    pub fn update(&mut self, scene: &mut Scene, time: &Time) {
        self.physics.update(scene, time.delta);
    }

    /// Messages for the Console (build problems etc.).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.physics.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_scales_and_caps_delta() {
        let mut t = Time::default();
        t.advance(0.016);
        assert_eq!(t.frame, 1);
        t.time_scale = 0.5;
        t.advance(0.02);
        assert!((t.delta - 0.01).abs() < 1e-6);
        t.advance(5.0);
        assert!(
            (t.unscaled_delta - 1.0 / 3.0).abs() < 1e-6,
            "long hitches are capped"
        );
    }
}
