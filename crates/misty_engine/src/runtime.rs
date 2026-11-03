//! Game loop time and the per-frame update that runs while playing (scripts, physics, audio,
//! particles).

use glam::Vec3;

use std::path::PathBuf;

use crate::audio::Audio;
use crate::physics::Physics;
use crate::profiler::Category;
use crate::scene::Scene;
use crate::scripting::{Input, LogKind, Request, ScriptHost};
use crate::settings::{ProjectSettings, TimeSettings};

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
    /// Longest real delta a frame may use (Maximum Allowed Timestep).
    pub maximum_delta: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            time: 0.0,
            delta: 0.0,
            unscaled_delta: 0.0,
            frame: 0,
            time_scale: 1.0,
            maximum_delta: 1.0 / 3.0,
        }
    }
}

impl Time {
    /// Fresh clock using Project Settings ▸ Time.
    pub fn new(settings: &TimeSettings) -> Self {
        Self {
            time_scale: settings.time_scale,
            maximum_delta: settings.maximum_allowed_timestep,
            ..Default::default()
        }
    }

    /// Advance by `real_dt` seconds of wall-clock time (capped at the maximum allowed timestep).
    pub fn advance(&mut self, real_dt: f32) {
        let dt = real_dt.clamp(0.0, self.maximum_delta);
        self.unscaled_delta = dt;
        self.delta = dt * self.time_scale;
        self.time += self.delta as f64;
        self.frame += 1;
    }
}

/// Everything that lives only while playing: scripts, the physics world they drive and audio.
/// Dropping it stops every sound.
pub struct Runtime {
    host: ScriptHost,
    /// Warnings gathered before the first frame.
    pending: Vec<String>,
}

impl Runtime {
    /// Build physics and script instances from the scene as Play starts (runs `Awake`),
    /// playing sound through the default audio device.
    pub fn start(scene: &mut Scene, project_root: PathBuf, settings: &ProjectSettings) -> Self {
        let audio = Audio::with_device(project_root.clone());
        Self::start_with_audio(scene, project_root, settings, audio)
    }

    /// [`Runtime::start`] with a chosen audio output (e.g. [`Audio::null`] in tests).
    pub fn start_with_audio(
        scene: &mut Scene,
        project_root: PathBuf,
        settings: &ProjectSettings,
        audio: Audio,
    ) -> Self {
        // Edit-mode particle previews don't carry into the game.
        crate::particles::clear_all(scene);
        let mut physics = Physics::build_with(scene, &settings.physics);
        let warnings = std::mem::take(&mut physics.warnings);
        let host = ScriptHost::start_configured(scene, physics, project_root, audio, settings);
        Self {
            host,
            pending: warnings,
        }
    }

    /// One game frame.
    pub fn update(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        crate::profile_scope!("PlayerLoop", Category::Others);
        self.host.update(scene, time, input);
        {
            crate::profile_scope!("Audio.Update", Category::Audio);
            self.host.with_audio(|a| {
                a.tick(time.delta);
                a.update(scene);
            });
        }
        crate::profile_scope!("ParticleSystem.Update", Category::Others);
        let (gravity, _) = self.host.physics_config();
        crate::particles::update(scene, time.delta, gravity);
    }

    /// Live Animator state of `e` (current state, cross-fade, parameter values).
    pub fn animator_status(
        &self,
        scene: &Scene,
        e: hecs::Entity,
    ) -> Option<crate::animation::animator::AnimatorStatus> {
        self.host.with_animator(|a| a.status(scene, e))
    }

    /// Change an Animator parameter from the editor while playing (any parameter type).
    pub fn set_animator_param(&mut self, scene: &Scene, e: hecs::Entity, name: &str, value: f32) {
        use crate::animation::controller::ParamKind;
        self.host
            .with_animator(|a| a.set_param(scene, e, name, &ParamKind::ALL, value));
    }

    /// Corners left on `e`'s Nav Mesh Agent path, from the agent (editor path gizmo).
    pub fn nav_path(&self, e: hecs::Entity) -> Option<Vec<Vec3>> {
        self.host.with_navigation(|n| n.path_of(e))
    }

    /// Game view "Mute Audio" and the editor's Pause button.
    pub fn set_audio_state(&mut self, muted: bool, paused: bool) {
        self.host.with_audio(|a| {
            a.set_muted(muted);
            a.set_paused(paused);
        });
    }

    /// The game's audio between frames (the Audio Mixer window's meters and live edits).
    pub fn with_audio<R>(&self, f: impl FnOnce(&mut Audio) -> R) -> R {
        self.host.with_audio(f)
    }

    /// Messages for the Console (script output, errors, build problems).
    pub fn take_logs(&mut self) -> Vec<(LogKind, String)> {
        let mut out: Vec<(LogKind, String)> = std::mem::take(&mut self.pending)
            .into_iter()
            .map(|w| (LogKind::Warning, w))
            .collect();
        out.extend(self.host.take_logs());
        let audio = self.host.with_audio(|a| a.take_logs());
        out.extend(audio.into_iter().map(|w| (LogKind::Warning, w)));
        out
    }

    /// `SceneManager.LoadScene` / `Application.Quit` calls made by scripts since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        self.host.take_requests()
    }

    /// UI button hover/press state, for drawing buttons with their Colour Tint.
    pub fn ui_interaction(&self) -> &crate::ui::UiInteraction {
        self.host.ui_interaction()
    }

    /// Gravity and fixed timestep the physics world was built with.
    pub fn physics_config(&self) -> (Vec3, f32) {
        self.host.physics_config()
    }

    /// Project Settings ▸ Physics the world was built with (layer matrix, queries).
    pub fn physics_settings(&self) -> crate::settings::PhysicsSettings {
        self.host.physics_settings()
    }

    pub fn stats(&self) -> RuntimeStats {
        let (bodies, colliders) = self.host.physics_counts();
        RuntimeStats {
            bodies,
            colliders,
            scripts: self.host.instance_count(),
            voices: self.host.with_audio(|a| a.voice_count()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeStats {
    pub bodies: usize,
    pub colliders: usize,
    pub scripts: usize,
    /// Sounds currently playing.
    pub voices: usize,
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

    #[test]
    fn time_uses_project_settings() {
        let mut t = Time::new(&TimeSettings {
            time_scale: 2.0,
            maximum_allowed_timestep: 0.1,
        });
        t.advance(0.5);
        assert!((t.unscaled_delta - 0.1).abs() < 1e-6);
        assert!((t.delta - 0.2).abs() < 1e-6);
    }
}
