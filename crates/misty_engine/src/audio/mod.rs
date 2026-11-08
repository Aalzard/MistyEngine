//! Play-mode audio: Audio Sources heard through the Audio Listener, mixed through Audio Mixers.
//!
//! [`Audio`] owns the voices started by Audio Sources (play on awake, scripts) and, every frame,
//! recomputes their volume, pitch, pan and binaural direction from the listener and source
//! positions, and pushes mixer group volumes and effect values to the backend's tracks. Sound
//! is mixed by our own graph ([`engine`]: sends, ducking, live chain edits, streaming); the
//! sound device (through kira) is opened lazily on the first sound, so scenes without audio and
//! tests never touch it; if it can't be opened a warning is logged and everything keeps running
//! silently. Tests render the same graph offline ([`Audio::offline`]).

pub mod backend;
pub mod clip;
pub mod dsp;
pub mod effects;
pub mod engine;
pub mod graph_backend;
pub mod mixer;
pub mod mixer_state;
mod routing;
pub mod spatial;
pub mod stream;
mod video_voices;
pub mod wav;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::scene::{AudioListener, AudioSource, Scene};
pub use backend::{AudioBackend, NullBackend, NullLog, TrackId, VoiceId};
pub use effects::{EffectKind, EffectSpec, amplitude_to_db, db_to_amplitude};
pub use graph_backend::GraphBackend;
pub use mixer::{AudioMixer, is_mixer};
pub use mixer_state::MixerState;
use routing::{Ctx, Route};
pub use routing::{output_ref, split_output};
pub use spatial::VoiceParams;

/// File extensions treated as audio clips.
pub const CLIP_EXTENSIONS: [&str; 3] = ["wav", "ogg", "mp3"];

/// Voices beyond this many are silenced, least important (highest Priority value) first.
pub const MAX_REAL_VOICES: usize = 32;

/// Audio-thread mixing time and the length of audio it produced since the main thread last
/// reported them to the Profiler (ns).
static MIX_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static MIXED_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A mixed block: `cpu` ns of work for `audio` ns of sound (audio thread, while recording).
pub(crate) fn note_mix(cpu: u64, audio: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    MIX_NS.fetch_add(cpu, Relaxed);
    MIXED_NS.fetch_add(audio, Relaxed);
}

/// Mixing time (ms) and DSP load (% of one core) since the last call.
pub fn take_mix_load() -> (f32, f32) {
    use std::sync::atomic::Ordering::Relaxed;
    let cpu = MIX_NS.swap(0, Relaxed);
    let audio = MIXED_NS.swap(0, Relaxed);
    let load = match audio {
        0 => 0.0,
        a => cpu as f32 / a as f32 * 100.0,
    };
    (cpu as f32 / 1.0e6, load)
}

pub fn is_clip(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CLIP_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

type Factory = fn() -> Result<Box<dyn AudioBackend>, String>;

/// The voice an Audio Source is playing with `Play()`.
struct SourceVoice {
    id: VoiceId,
    paused: bool,
    route: Route,
}

/// A `PlayOneShot` voice: follows its source's position, scaled by its own volume.
struct OneShot {
    source: Entity,
    id: VoiceId,
    scale: f32,
    route: Route,
}

/// `AudioSource.PlayClipAtPoint`: a fully 3D voice at a fixed point.
struct PointVoice {
    id: VoiceId,
    at: Vec3,
    volume: f32,
}

pub struct Audio {
    backend: Option<Box<dyn AudioBackend>>,
    /// Opens the device on first use; `None` once tried.
    factory: Option<Factory>,
    root: PathBuf,
    sources: HashMap<Entity, SourceVoice>,
    one_shots: Vec<OneShot>,
    points: Vec<PointVoice>,
    /// Untracked 2D voices (`play_clip`, e.g. the editor's clip preview).
    clips: Vec<VoiceId>,
    /// Mixers loaded so far (by first use).
    mixers: Vec<MixerState>,
    /// Positions last frame, for Doppler.
    last_pos: HashMap<Entity, Vec3>,
    last_ear: Option<Vec3>,
    /// Scaled seconds of the current frame (`tick`).
    dt: f32,
    muted: bool,
    paused: bool,
    logs: Vec<String>,
    /// Clips / outputs that already failed (warn once each).
    bad_clips: HashSet<String>,
}

impl Audio {
    /// Plays through the default sound device (opened on the first sound).
    pub fn with_device(root: PathBuf) -> Self {
        let mut a = Self::new(root, None);
        a.factory =
            Some(|| GraphBackend::with_device().map(|b| Box::new(b) as Box<dyn AudioBackend>));
        a
    }

    /// Audio mixed by the real graph but rendered on demand with [`Audio::render`] (tests,
    /// benchmarks): no sound device.
    pub fn offline(root: PathBuf, sample_rate: f32) -> Self {
        Self::new(root, Some(Box::new(GraphBackend::offline(sample_rate))))
    }

    /// Load clips again on their next play (their import settings changed).
    pub fn forget_clips(&mut self) {
        if let Some(b) = self.backend.as_mut() {
            b.forget_clips();
        }
    }

    /// Clips the mixer keeps in memory (Memory Profiler).
    pub fn clip_memory(&self) -> Vec<(String, u64, &'static str)> {
        self.backend
            .as_ref()
            .map_or(Vec::new(), |b| b.clip_memory())
    }

    /// Offline audio: the next `frames` of the mix (`None` for device or null backends).
    pub fn render(&mut self, frames: usize) -> Option<Vec<dsp::Frame>> {
        self.backend.as_mut()?.render(frames)
    }

    /// Silent audio whose calls are recorded in the returned log.
    pub fn null(root: PathBuf) -> (Self, std::sync::Arc<std::sync::Mutex<NullLog>>) {
        let backend = NullBackend::default();
        let log = backend.log.clone();
        (Self::new(root, Some(Box::new(backend))), log)
    }

    pub fn new(root: PathBuf, backend: Option<Box<dyn AudioBackend>>) -> Self {
        Self {
            backend,
            factory: None,
            root,
            sources: HashMap::new(),
            one_shots: Vec::new(),
            points: Vec::new(),
            clips: Vec::new(),
            mixers: Vec::new(),
            last_pos: HashMap::new(),
            last_ear: None,
            dt: 0.0,
            muted: false,
            paused: false,
            logs: Vec::new(),
            bad_clips: HashSet::new(),
        }
    }

    fn backend(&mut self) -> Option<&mut Box<dyn AudioBackend>> {
        if self.backend.is_none()
            && let Some(open) = self.factory.take()
        {
            match open() {
                Ok(b) => self.backend = Some(b),
                Err(e) => self
                    .logs
                    .push(format!("Audio: no output device ({e}); playing silently")),
            }
        }
        self.backend.as_mut()
    }

    /// Start a voice for `clip` (project-relative) on `track`, logging load errors once per clip.
    fn start_voice(
        &mut self,
        clip: &str,
        params: VoiceParams,
        track: Option<TrackId>,
    ) -> Option<VoiceId> {
        let path = self.root.join(clip);
        let paused = self.paused;
        let backend = self.backend()?;
        match backend.play(&path, params, track) {
            Ok(id) => {
                if paused {
                    backend.pause(id);
                }
                Some(id)
            }
            Err(e) => {
                if self.bad_clips.insert(clip.to_owned()) {
                    self.logs.push(format!("Audio clip {clip}: {e}"));
                }
                None
            }
        }
    }

    /// Mixing context for this frame (no motion: Doppler starts at 1).
    fn ctx(&self, scene: &Scene) -> Ctx<'_> {
        Ctx::new(scene, &self.mixers, self.muted)
    }

    // ---- Audio Sources ------------------------------------------------------------------------

    /// Play every enabled, active source with Play On Awake that isn't playing yet
    /// (only under `root`, for objects created while playing).
    pub fn play_on_awake(&mut self, scene: &Scene, root: Option<Entity>) {
        let awake: Vec<Entity> = scene
            .world
            .query::<(Entity, &AudioSource)>()
            .iter()
            .filter(|(e, s)| s.play_on_awake && root.is_none_or(|r| scene.is_ancestor(r, *e)))
            .map(|(e, _)| e)
            .collect();
        for e in awake {
            if !self.sources.contains_key(&e) {
                self.play(scene, e);
            }
        }
    }

    /// `AudioSource.Play()`: (re)start the source's clip from the beginning.
    pub fn play(&mut self, scene: &Scene, e: Entity) -> bool {
        self.stop(e);
        let Some(source) = playable(scene, e) else {
            return false;
        };
        let Some(clip) = source.clip.clone().filter(|c| !c.is_empty()) else {
            return false;
        };
        self.backend();
        let route = self.route(scene, e, &source);
        let params = self.ctx(scene).source_params(scene, e, &source, &route);
        match self.start_voice(&clip, params, route.target(&self.mixers)) {
            Some(id) => {
                self.sources.insert(
                    e,
                    SourceVoice {
                        id,
                        paused: false,
                        route,
                    },
                );
                true
            }
            None => {
                self.drop_route(route);
                false
            }
        }
    }

    pub fn stop(&mut self, e: Entity) {
        if let Some(v) = self.sources.remove(&e) {
            self.end(v.id, v.route);
        }
    }

    /// Stop a voice and remove its own effect track.
    fn end(&mut self, id: VoiceId, route: Route) {
        if let Some(b) = self.backend.as_mut() {
            b.stop(id);
        }
        self.drop_route(route);
    }

    pub fn pause(&mut self, e: Entity) {
        if let Some(v) = self.sources.get_mut(&e)
            && !v.paused
        {
            v.paused = true;
            if let Some(b) = self.backend.as_mut() {
                b.pause(v.id);
            }
        }
    }

    pub fn unpause(&mut self, e: Entity) {
        if let Some(v) = self.sources.get_mut(&e)
            && v.paused
        {
            v.paused = false;
            if !self.paused
                && let Some(b) = self.backend.as_mut()
            {
                b.resume(v.id);
            }
        }
    }

    /// `isPlaying`: started, not paused and not finished.
    pub fn is_playing(&self, e: Entity) -> bool {
        self.sources
            .get(&e)
            .is_some_and(|v| !v.paused && self.backend.as_ref().is_some_and(|b| b.is_alive(v.id)))
    }

    /// `AudioSource.time`: playback position of the source's `Play()` voice in seconds.
    pub fn time(&self, e: Entity) -> f32 {
        match (self.sources.get(&e), self.backend.as_ref()) {
            (Some(v), Some(b)) => b.position(v.id) as f32,
            _ => 0.0,
        }
    }

    pub fn set_time(&mut self, e: Entity, seconds: f32) {
        if let (Some(v), Some(b)) = (self.sources.get(&e), self.backend.as_mut()) {
            b.seek(v.id, seconds.max(0.0) as f64);
        }
    }

    /// `AudioSource.PlayOneShot(clip, volumeScale)`: an extra voice through the source's mix.
    pub fn play_one_shot(&mut self, scene: &Scene, e: Entity, clip: &str, scale: f32) -> bool {
        let Some(source) = playable(scene, e) else {
            return false;
        };
        let scale = scale.clamp(0.0, 1.0);
        self.backend();
        let route = self.route(scene, e, &source);
        let mut params = self.ctx(scene).source_params(scene, e, &source, &route);
        params.volume *= scale;
        params.looping = false;
        match self.start_voice(clip, params, route.target(&self.mixers)) {
            Some(id) => {
                self.one_shots.push(OneShot {
                    source: e,
                    id,
                    scale,
                    route,
                });
                true
            }
            None => {
                self.drop_route(route);
                false
            }
        }
    }

    /// `AudioSource.PlayClipAtPoint(clip, position [, volume])`: a fully 3D one-shot at a
    /// fixed world position with the default rolloff, straight to the listener.
    pub fn play_clip_at_point(&mut self, scene: &Scene, clip: &str, at: Vec3, volume: f32) -> bool {
        let volume = volume.clamp(0.0, 1.0);
        let params = spatial::mix(&point_source(volume), at, listener(scene), self.muted);
        match self.start_voice(clip, params, None) {
            Some(id) => {
                self.points.push(PointVoice { id, at, volume });
                true
            }
            None => false,
        }
    }

    /// Play a clip in 2D at `volume`, outside any scene (editor preview).
    pub fn play_clip(&mut self, clip: &str, volume: f32) -> bool {
        let params = VoiceParams {
            volume: if self.muted { 0.0 } else { volume },
            ..Default::default()
        };
        let id = self.start_voice(clip, params, None);
        self.clips.extend(id);
        id.is_some()
    }

    fn all_ids(&self) -> impl Iterator<Item = VoiceId> + '_ {
        let ids = self.sources.values().map(|v| v.id);
        ids.chain(self.one_shots.iter().map(|o| o.id))
            .chain(self.points.iter().map(|p| p.id))
            .chain(self.clips.iter().copied())
    }

    /// Whether any voice is still sounding.
    pub fn any_playing(&self) -> bool {
        self.voice_count() > 0
    }

    /// Number of live voices (Stats).
    pub fn voice_count(&self) -> usize {
        let Some(b) = self.backend.as_ref() else {
            return 0;
        };
        self.all_ids().filter(|id| b.is_alive(*id)).count()
    }

    pub fn stop_all(&mut self) {
        let sources: Vec<(VoiceId, Route)> =
            self.sources.drain().map(|(_, v)| (v.id, v.route)).collect();
        let shots: Vec<(VoiceId, Route)> =
            self.one_shots.drain(..).map(|o| (o.id, o.route)).collect();
        for (id, route) in sources.into_iter().chain(shots) {
            self.end(id, route);
        }
        let rest: Vec<VoiceId> = self
            .points
            .drain(..)
            .map(|p| p.id)
            .chain(self.clips.drain(..))
            .collect();
        if let Some(b) = self.backend.as_mut() {
            for id in rest {
                b.stop(id);
            }
        }
    }

    // ---- global state -------------------------------------------------------------------------

    /// Game view "Mute Audio" (applied on the next update).
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// Editor pause: hold every voice, resume the ones scripts didn't pause themselves.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        let resumable: Vec<VoiceId> = self
            .sources
            .values()
            .filter(|v| paused || !v.paused)
            .map(|v| v.id)
            .chain(self.one_shots.iter().map(|o| o.id))
            .chain(self.points.iter().map(|p| p.id))
            .collect();
        if let Some(b) = self.backend.as_mut() {
            for id in resumable {
                if paused { b.pause(id) } else { b.resume(id) }
            }
        }
    }

    /// Advance time-based mixing (snapshot transitions, Doppler velocities) by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        self.dt = dt.max(0.0);
        for m in &mut self.mixers {
            m.tick(self.dt);
        }
    }

    /// Per frame: stop voices of destroyed/disabled sources, drop finished ones, update volume,
    /// pitch and pan from the current positions, and push mixer values to their tracks.
    pub fn update(&mut self, scene: &Scene) {
        if self.backend.is_none() {
            return;
        }
        let mut ended: Vec<(VoiceId, Route)> = Vec::new();
        let ctx = Ctx::moving(
            scene,
            &self.mixers,
            self.muted,
            self.dt,
            self.last_ear,
            &self.last_pos,
        );
        let Self {
            backend,
            sources,
            one_shots,
            points,
            clips,
            ..
        } = self;
        let b = backend.as_mut().expect("checked");
        for m in ctx.mixers {
            m.push(b.as_mut());
        }
        // (voice, params, priority) for everything scene-driven.
        let mut planned: Vec<(VoiceId, VoiceParams, u16)> = Vec::new();
        for (e, v) in std::mem::take(sources) {
            match playable(scene, e).filter(|_| b.is_alive(v.id)) {
                Some(src) => {
                    let p = ctx.source_params(scene, e, &src, &v.route);
                    ctx.push_own(b.as_mut(), scene, e, &src, &v.route);
                    planned.push((v.id, p, src.priority));
                    sources.insert(e, v);
                }
                None => ended.push((v.id, v.route)),
            }
        }
        for o in std::mem::take(one_shots) {
            match playable(scene, o.source).filter(|_| b.is_alive(o.id)) {
                Some(src) => {
                    let mut p = ctx.source_params(scene, o.source, &src, &o.route);
                    p.volume *= o.scale;
                    p.looping = false;
                    ctx.push_own(b.as_mut(), scene, o.source, &src, &o.route);
                    planned.push((o.id, p, src.priority));
                    one_shots.push(o);
                }
                None => ended.push((o.id, o.route)),
            }
        }
        points.retain(|p| b.is_alive(p.id));
        for p in points.iter() {
            let params = spatial::mix(&point_source(p.volume), p.at, ctx.ear, ctx.muted);
            planned.push((p.id, params, 128));
        }
        clips.retain(|id| b.is_alive(*id));
        limit_voices(&mut planned);
        if crate::profiler::enabled() {
            report_profiler(&planned, &self.mixers);
        }
        for (id, p, _) in planned {
            b.set(id, p);
        }
        let (ear, positions) = ctx.positions(scene, sources.keys().copied());
        self.last_ear = ear;
        self.last_pos = positions;
        for (id, route) in ended {
            self.end(id, route);
        }
    }

    /// Warnings for the Console.
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// The Profiler's Audio module counters of this frame (main thread side; the graph reports its
/// own voices and the audio thread's time).
fn report_profiler(planned: &[(VoiceId, VoiceParams, u16)], mixers: &[MixerState]) {
    use crate::profiler::{Counter, add_counter, set_counter};
    let heard = planned.iter().filter(|p| p.1.volume > 0.0).count();
    let groups: usize = mixers.iter().map(|m| m.asset.groups.len()).sum();
    let effects: usize = mixers
        .iter()
        .flat_map(|m| &m.asset.groups)
        .map(|g| g.effects.len())
        .sum();
    let (mix_ms, load) = take_mix_load();
    set_counter(Counter::AudioPlayingSources, heard as f32);
    set_counter(
        Counter::AudioVirtualVoices,
        planned.len().saturating_sub(MAX_REAL_VOICES) as f32,
    );
    set_counter(Counter::AudioMixerGroups, groups as f32);
    set_counter(Counter::AudioEffects, effects as f32);
    add_counter(Counter::AudioMixMs, mix_ms);
    set_counter(Counter::AudioDspLoad, load);
}

/// Keep the most important voices audible: sort by Priority (lower first), then loudness,
/// and silence everything past [`MAX_REAL_VOICES`].
fn limit_voices(planned: &mut [(VoiceId, VoiceParams, u16)]) {
    if planned.len() <= MAX_REAL_VOICES {
        return;
    }
    planned.sort_by(|a, b| {
        a.2.cmp(&b.2)
            .then(b.1.volume.total_cmp(&a.1.volume))
            .then(a.0.cmp(&b.0))
    });
    for (_, p, _) in &mut planned[MAX_REAL_VOICES..] {
        p.volume = 0.0;
    }
}

/// The settings `PlayClipAtPoint` plays with.
fn point_source(volume: f32) -> AudioSource {
    AudioSource {
        volume,
        spatial_blend: 1.0,
        ..Default::default()
    }
}

/// The source's settings if it may sound now (exists, enabled, object active).
fn playable(scene: &Scene, e: Entity) -> Option<AudioSource> {
    if !scene.contains(e) || !scene.active_in_hierarchy(e) {
        return None;
    }
    let s = scene.world.get::<&AudioSource>(e).ok()?;
    s.enabled.then(|| (*s).clone())
}

/// World matrix of the first enabled, active Audio Listener.
pub fn listener(scene: &Scene) -> Option<Mat4> {
    let e = scene
        .world
        .query::<(Entity, &AudioListener)>()
        .iter()
        .filter(|(e, l)| l.enabled && scene.active_in_hierarchy(*e))
        .map(|(e, _)| e)
        .next()?;
    Some(scene.world_matrix(e))
}

#[cfg(test)]
mod cost_report;
#[cfg(test)]
mod graph_tests;
#[cfg(test)]
mod mixer_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod tests;
