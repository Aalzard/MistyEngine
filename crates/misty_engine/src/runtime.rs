//! Game loop time and the per-frame update that runs while playing (scripts, physics, audio,
//! particles).

use glam::Vec3;

use std::path::{Path, PathBuf};

use crate::audio::Audio;
use crate::build::{BuildSettings, SceneRef};
use crate::physics::Physics;
use crate::profiler::Category;
use crate::scene::{Scene, SceneId};
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
    /// Project folder and settings, for reloading the scene of a loaded save.
    root: PathBuf,
    settings: ProjectSettings,
    /// `SaveSystem.Load` replaced the scene since the last [`Runtime::take_scene_reloaded`].
    reloaded: bool,
    /// World Partition cells streaming in and out around the sources.
    streaming: crate::world_partition::Streamer,
}

/// Streamed objects get physics bodies and scripts as they arrive, and lose them as they go.
struct HostHooks<'a>(&'a mut ScriptHost);

impl crate::world_partition::StreamHooks for HostHooks<'_> {
    fn added(&mut self, scene: &mut Scene, roots: &[hecs::Entity]) {
        if !roots.is_empty() {
            self.0.add_objects(scene, roots);
        }
    }

    fn remove(&mut self, scene: &mut Scene, roots: &[hecs::Entity]) {
        self.0.remove_objects(scene, roots);
    }
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
        crate::vfx::clear_all(scene);
        let mut physics = Physics::build_with(scene, &settings.physics);
        let mut warnings = std::mem::take(&mut physics.warnings);
        let mut physics2d =
            crate::physics2d::Physics2D::build(scene, &settings.physics2d, &settings.physics);
        warnings.append(&mut physics2d.warnings);
        let host = ScriptHost::start_with_physics(
            scene,
            physics,
            physics2d,
            project_root.clone(),
            audio,
            settings,
        );
        Self {
            host,
            pending: warnings,
            root: project_root,
            settings: settings.clone(),
            reloaded: false,
            streaming: crate::world_partition::Streamer::new(),
        }
    }

    /// Load addressables with `assets` (the editor's Play Mode Script).
    pub fn set_assets(&mut self, assets: crate::asset_groups::AssetSystem) {
        self.host.set_assets(assets);
    }

    /// Addressable load counters (handles, bundles).
    pub fn asset_stats(&self) -> crate::asset_groups::AssetStats {
        self.host.with_assets(|s| s.stats()).unwrap_or_default()
    }

    /// Restore a save `SaveSystem.Load` asked for: reload its scene file and start it again
    /// (Awake), then apply the saved states. Without a scene file, the states are applied to
    /// the current scene.
    fn load_save(&mut self, scene: &mut Scene, data: crate::save::SaveData) {
        let rel = data.info.scene_path.clone();
        let loaded = rel
            .as_ref()
            .map(|rel| Scene::load(&self.root.join(rel)).map(|r| (rel.clone(), r)));
        match loaded {
            Some(Ok((rel, (mut fresh, report)))) => {
                fresh.set_scene_path(0, &rel);
                let audio = self.host.take_audio();
                let folder = self.host.save_folder();
                let mut pending = std::mem::take(&mut self.pending);
                pending.extend(report.warnings.into_iter().map(|w| format!("{rel}: {w}")));
                let old_logs = self.host.take_logs();
                // Old scripts and bodies go before the reloaded scene's Awake.
                *scene = Scene::default();
                let mut next =
                    Runtime::start_with_audio(&mut fresh, self.root.clone(), &self.settings, audio);
                // A multiplayer game keeps its connection across the reload.
                let session = self.host.take_network();
                next.host.adopt_network(&mut fresh, session);
                next.host.set_save_folder(folder);
                next.host.apply_loaded(&mut fresh, &data, false);
                next.host.prepend_logs(old_logs);
                next.pending.splice(0..0, pending);
                next.reloaded = true;
                *scene = fresh;
                *self = next;
            }
            other => {
                if let (Some(rel), Some(Err(e))) = (&rel, other) {
                    self.pending
                        .push(format!("SaveSystem.Load: {rel}: {e}; restoring in place"));
                }
                self.host.apply_loaded(scene, &data, true);
            }
        }
    }

    /// Save the game to `slot` between frames (like `SaveSystem.Save`).
    pub fn save_game(&mut self, scene: &mut Scene, slot: &str) -> Result<(), String> {
        self.host.save_game(scene, slot)
    }

    /// Load `slot` like `SaveSystem.Load`: the scene is restored after the next frame.
    pub fn load_game(&mut self, slot: &str) -> Result<(), String> {
        self.host.load_game(slot)
    }

    /// Whether a loaded save replaced the scene (its objects are new) since the last call.
    pub fn take_scene_reloaded(&mut self) -> bool {
        std::mem::take(&mut self.reloaded)
    }

    /// Where `SaveSystem` keeps this game's saves.
    pub fn save_folder(&self) -> PathBuf {
        self.host.save_folder()
    }

    /// Keep this game's saves in `folder` (tests, tools).
    pub fn set_save_folder(&self, folder: PathBuf) {
        self.host.set_save_folder(folder);
    }

    /// The game's String Tables and current locale.
    pub fn with_localization<R>(&self, f: impl FnOnce(&crate::localization::Localizer) -> R) -> R {
        self.host.with_localization(f)
    }

    /// One game frame.
    pub fn update(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        crate::profile_scope!("PlayerLoop", Category::Others);
        // Shaders, waves and buoyancy follow the game clock.
        crate::clock::set_play_time(time.time);
        // Spline Extrude meshes and Spline Instantiate copies follow edits made while playing.
        crate::splines::update(scene);
        self.update_streaming(scene);
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
        crate::profile_scope!("VisualEffect.Update", Category::Others);
        crate::vfx::update(scene, time.delta);
        if let Some(data) = self.host.take_pending_load() {
            self.load_save(scene, data);
        }
    }

    /// World Partition: cells in and out within the frame budget, scripts' requests applied and
    /// the streaming state handed to them.
    fn update_streaming(&mut self, scene: &mut Scene) {
        let wp = crate::world_partition::find(scene).map(|(_, wp)| wp);
        if wp.is_none() && self.streaming.stats().frames == 0 {
            return;
        }
        let mut hooks = HostHooks(&mut self.host);
        self.streaming.update(scene, &[], &mut hooks);
        let logs = self.streaming.take_logs();
        self.pending.extend(logs);
        // What scripts see this frame (after the streaming work); their requests count next.
        let state = crate::scripting::world_partition_api::ScriptStreaming {
            stats: self.streaming.stats().clone(),
            phases: wp
                .as_ref()
                .map(|wp| {
                    wp.cells
                        .iter()
                        .map(|c| (c.coord(), self.streaming.phase(c.coord())))
                        .collect()
                })
                .unwrap_or_default(),
            requests: Vec::new(),
        };
        for (c, load) in self.host.exchange_streaming(state) {
            if load {
                self.streaming.load(c);
            } else {
                self.streaming.unload(c);
            }
        }
    }

    /// The World Partition streamer (stats, cell states, loading by hand).
    pub fn streaming(&self) -> &crate::world_partition::Streamer {
        &self.streaming
    }

    pub fn streaming_mut(&mut self) -> &mut crate::world_partition::Streamer {
        &mut self.streaming
    }

    /// Add a scene file's objects as another loaded scene and start them (bodies, `Awake`).
    pub fn load_scene_additive(
        &mut self,
        scene: &mut Scene,
        rel: &str,
        text: &str,
    ) -> Result<SceneId, String> {
        let (id, roots, report) = scene.load_additive(rel, text)?;
        let warnings = report.warnings.into_iter().map(|w| format!("{rel}: {w}"));
        self.pending.extend(warnings);
        self.host.add_objects(scene, &roots);
        Ok(id)
    }

    /// Remove a loaded scene and its objects (scripts and bodies go with them).
    pub fn unload_scene(&mut self, scene: &mut Scene, id: SceneId) -> Result<(), String> {
        if !scene.loaded_scenes().contains(&id) {
            return Err(format!("'{}' is not loaded", scene.scene_name(id)));
        }
        if scene.loaded_scenes().len() <= 1 {
            return Err("the last loaded scene can't be unloaded".into());
        }
        let roots = scene.roots_in(id);
        self.host.remove_objects(scene, &roots);
        scene.remove_scene(id)
    }

    /// Carry out a script's `LoadScene(…, Additive)` or `UnloadSceneAsync(…)` (scenes come
    /// from the Build Settings list; files are read from `root`, or the game's pak).
    pub fn apply_scene_request(
        &mut self,
        scene: &mut Scene,
        build: &BuildSettings,
        root: &Path,
        request: &Request,
    ) -> Result<(), String> {
        match request {
            Request::LoadSceneAdditive(which) => {
                let (rel, _) = build.resolve(which)?;
                let text = crate::vfs::read_to_string(root.join(&rel))
                    .map_err(|e| format!("{rel}: {e}"))?;
                self.load_scene_additive(scene, &rel, &text).map(|_| ())
            }
            Request::UnloadScene(which) => {
                let id = match which {
                    SceneRef::Name(name) => scene.find_loaded(name),
                    SceneRef::Index(_) => build
                        .resolve(which)
                        .ok()
                        .and_then(|(rel, _)| scene.find_loaded(&rel)),
                };
                let id = id.ok_or_else(|| format!("UnloadSceneAsync: {which} is not loaded"))?;
                self.unload_scene(scene, id)
            }
            _ => Ok(()),
        }
    }

    /// Run `f` with the game's world (and `scene`) installed in another Lua state between
    /// frames (PlayMode tests use the game API this way; see [`ScriptHost::lend_world`]).
    pub fn lend_world<R>(
        &mut self,
        scene: &mut Scene,
        other: &mlua::Lua,
        f: impl FnOnce() -> R,
    ) -> R {
        self.host.lend_world(scene, other, f)
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

    /// Change an Animator layer's weight from the editor while playing.
    pub fn set_animator_layer_weight(
        &mut self,
        scene: &Scene,
        e: hecs::Entity,
        layer: usize,
        weight: f32,
    ) {
        self.host
            .with_animator(|a| a.set_layer_weight(scene, e, layer, weight));
    }

    /// Playback time and state of the Playable Director on `e` (the Timeline window's playhead).
    pub fn director_status(&self, e: hecs::Entity) -> (f64, crate::timeline::PlayState) {
        self.host.with_timeline(|t| (t.time(e), t.state(e)))
    }

    /// Corners left on `e`'s Nav Mesh Agent path, from the agent (editor path gizmo).
    pub fn nav_path(&self, e: hecs::Entity) -> Option<Vec<Vec3>> {
        self.host.with_navigation(|n| n.path_of(e))
    }

    /// The game's live navigation (nav meshes with carved tiles, agents) for editor drawing.
    pub fn with_navigation<R>(&self, f: impl FnOnce(&crate::navigation::NavRuntime) -> R) -> R {
        self.host.with_navigation(f)
    }

    /// Camera Brains and Virtual Cameras of this game (live camera, blend progress).
    pub fn with_cameras<R>(&self, f: impl FnOnce(&crate::vcam::CameraRig) -> R) -> R {
        self.host.with_cameras(f)
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

    /// Start networking between frames (Multiplayer Play Mode, the player's `--net-client`,
    /// the Network Manager Inspector's buttons).
    pub fn network_start(
        &mut self,
        scene: &mut Scene,
        start: &crate::network::AutoStart,
    ) -> Result<(), String> {
        self.host.network_start(scene, start)
    }

    /// Stop networking between frames (spawned copies go, scene objects stay).
    pub fn network_shutdown(&mut self, scene: &mut Scene) {
        self.host.network_shutdown(scene);
    }

    /// The network session between frames (status, settings before starting, tests).
    pub fn with_network<R>(&self, f: impl FnOnce(&mut crate::network::NetSession) -> R) -> R {
        self.host.with_network(f)
    }

    /// Traffic over the last second while networking runs.
    pub fn network_stats(&self) -> Option<crate::network::NetStats> {
        self.with_network(|s| {
            s.is_running().then_some(crate::network::NetStats {
                mode: s.mode,
                objects: s.objects.len(),
                ..s.stats
            })
        })
    }

    /// Take the network session out (before the next scene's runtime starts; this runtime
    /// is left offline).
    pub fn take_network(&mut self) -> crate::network::NetSession {
        self.host.take_network()
    }

    /// Continue a session taken from the previous scene's runtime, so a multiplayer game stays
    /// connected across scene loads (a server sends the new scene to its clients).
    pub fn adopt_network(&mut self, scene: &mut Scene, session: crate::network::NetSession) {
        self.host.adopt_network(scene, session);
    }

    /// What scripts asked of the cursor (`Cursor.lockState`, `Cursor.visible`); the window
    /// applies it.
    pub fn cursor(&self) -> crate::scripting::CursorState {
        self.host.cursor()
    }

    /// Give the cursor back (Escape in the editor, lost focus); scripts see `lockState` None.
    pub fn release_cursor(&self) {
        self.host.release_cursor();
    }

    /// Gamepad motor speeds scripts asked for since the last call: (slot, low, high).
    pub fn take_rumble(&self) -> Option<Vec<(usize, f32, f32)>> {
        self.host.take_rumble()
    }

    /// A playing Player Input as Inspector rows.
    pub fn player_input_info(&self, e: hecs::Entity) -> Option<Vec<(String, String)>> {
        self.host.player_input_info(e)
    }

    /// Stop every gamepad motor.
    pub fn reset_haptics(&self) {
        self.host.reset_haptics();
    }

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
    /// The playing physics world (debug view, cloth stats).
    pub fn with_physics<R>(&self, f: impl FnOnce(&mut crate::physics::Physics) -> R) -> R {
        self.host.with_physics(f)
    }

    /// The playing physics world with the scene (switching a ragdoll from the editor).
    pub fn with_physics_scene<R>(
        &mut self,
        scene: &mut Scene,
        f: impl FnOnce(&mut crate::physics::Physics, &mut Scene) -> R,
    ) -> R {
        self.host.with_physics_scene(scene, f)
    }

    pub fn physics_settings(&self) -> crate::settings::PhysicsSettings {
        self.host.physics_settings()
    }

    /// Bytes the scripts' Lua state holds (Memory Profiler).
    pub fn lua_memory(&self) -> usize {
        self.host.lua_memory()
    }

    /// Audio clips the mixer keeps in memory (Memory Profiler).
    pub fn audio_clip_memory(&self) -> Vec<(String, u64, &'static str)> {
        self.host.audio_clip_memory()
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
