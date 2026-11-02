//! Game loop time and the per-frame update that runs while playing.

use glam::Vec3;

use std::path::PathBuf;

use crate::physics::Physics;
use crate::scene::Scene;
use crate::scripting::{Input, LogKind, ScriptHost};

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

/// Everything that lives only while playing: scripts and the physics world they drive.
pub struct Runtime {
    host: ScriptHost,
    /// Warnings gathered before the first frame.
    pending: Vec<String>,
}

impl Runtime {
    /// Build physics and script instances from the scene as Play starts (runs `Awake`).
    pub fn start(scene: &mut Scene, project_root: PathBuf) -> Self {
        let mut physics = Physics::build(scene, DEFAULT_GRAVITY);
        let warnings = std::mem::take(&mut physics.warnings);
        let host = ScriptHost::start(scene, physics, project_root);
        Self {
            host,
            pending: warnings,
        }
    }

    /// One game frame.
    pub fn update(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        self.host.update(scene, time, input);
    }

    /// Messages for the Console (script output, errors, build problems).
    pub fn take_logs(&mut self) -> Vec<(LogKind, String)> {
        let mut out: Vec<(LogKind, String)> = std::mem::take(&mut self.pending)
            .into_iter()
            .map(|w| (LogKind::Warning, w))
            .collect();
        out.extend(self.host.take_logs());
        out
    }

    pub fn stats(&self) -> RuntimeStats {
        let (bodies, colliders) = self.host.physics_counts();
        RuntimeStats {
            bodies,
            colliders,
            scripts: self.host.instance_count(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeStats {
    pub bodies: usize,
    pub colliders: usize,
    pub scripts: usize,
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
