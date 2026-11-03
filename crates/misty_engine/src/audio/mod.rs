//! Play-mode audio: Audio Sources heard through the Audio Listener.
//!
//! [`Audio`] owns the voices started by Audio Sources (play on awake, scripts) and, every frame,
//! recomputes their volume and pan from the listener and source positions. The sound device
//! (kira) is opened lazily on the first sound, so scenes without audio and tests never touch it;
//! if it can't be opened a warning is logged and everything keeps running silently.

pub mod backend;
pub mod spatial;
pub mod wav;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::Mat4;
use hecs::Entity;

use crate::scene::{AudioListener, AudioSource, Scene};
pub use backend::{AudioBackend, KiraBackend, NullBackend, NullLog, VoiceId};
pub use spatial::VoiceParams;

/// File extensions treated as audio clips.
pub const CLIP_EXTENSIONS: [&str; 3] = ["wav", "ogg", "mp3"];

pub fn is_clip(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CLIP_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

type Factory = fn() -> Result<Box<dyn AudioBackend>, String>;

/// The voice an Audio Source is playing with `Play()`.
#[derive(Clone, Copy)]
struct SourceVoice {
    id: VoiceId,
    paused: bool,
}

/// A `PlayOneShot` voice: follows its source's position, scaled by its own volume.
struct OneShot {
    source: Entity,
    id: VoiceId,
    scale: f32,
}

pub struct Audio {
    backend: Option<Box<dyn AudioBackend>>,
    /// Opens the device on first use; `None` once tried.
    factory: Option<Factory>,
    root: PathBuf,
    sources: HashMap<Entity, SourceVoice>,
    one_shots: Vec<OneShot>,
    /// Untracked 2D voices (`play_clip`, e.g. the editor's clip preview).
    clips: Vec<VoiceId>,
    muted: bool,
    paused: bool,
    logs: Vec<String>,
    /// Clips that already failed to load (warn once each).
    bad_clips: HashSet<String>,
}

impl Audio {
    /// Plays through the default sound device (opened on the first sound).
    pub fn with_device(root: PathBuf) -> Self {
        let mut a = Self::new(root, None);
        a.factory = Some(|| KiraBackend::new().map(|b| Box::new(b) as Box<dyn AudioBackend>));
        a
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
            clips: Vec::new(),
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

    /// Start a voice for `clip` (project-relative), logging load errors once per clip.
    fn start_voice(&mut self, clip: &str, params: VoiceParams) -> Option<VoiceId> {
        let path = self.root.join(clip);
        let paused = self.paused;
        let backend = self.backend()?;
        match backend.play(&path, params) {
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
        let params = spatial::mix(
            &source,
            scene.world_position(e),
            listener(scene),
            self.muted,
        );
        match self.start_voice(&clip, params) {
            Some(id) => {
                self.sources.insert(e, SourceVoice { id, paused: false });
                true
            }
            None => false,
        }
    }

    pub fn stop(&mut self, e: Entity) {
        if let Some(v) = self.sources.remove(&e)
            && let Some(b) = self.backend.as_mut()
        {
            b.stop(v.id);
        }
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

    /// `AudioSource.PlayOneShot(clip, volumeScale)`: an extra voice through the source's mix.
    pub fn play_one_shot(&mut self, scene: &Scene, e: Entity, clip: &str, scale: f32) -> bool {
        let Some(source) = playable(scene, e) else {
            return false;
        };
        let scale = scale.clamp(0.0, 1.0);
        let mut params = spatial::mix(
            &source,
            scene.world_position(e),
            listener(scene),
            self.muted,
        );
        params.volume *= scale;
        params.looping = false;
        match self.start_voice(clip, params) {
            Some(id) => {
                self.one_shots.push(OneShot {
                    source: e,
                    id,
                    scale,
                });
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
        let id = self.start_voice(clip, params);
        self.clips.extend(id);
        id.is_some()
    }

    /// Whether any voice is still sounding.
    pub fn any_playing(&self) -> bool {
        let Some(b) = self.backend.as_ref() else {
            return false;
        };
        self.sources.values().any(|v| b.is_alive(v.id))
            || self.one_shots.iter().any(|o| b.is_alive(o.id))
            || self.clips.iter().any(|id| b.is_alive(*id))
    }

    /// Number of live voices (Stats).
    pub fn voice_count(&self) -> usize {
        let Some(b) = self.backend.as_ref() else {
            return 0;
        };
        let ids = self.sources.values().map(|v| v.id);
        ids.chain(self.one_shots.iter().map(|o| o.id))
            .chain(self.clips.iter().copied())
            .filter(|id| b.is_alive(*id))
            .count()
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<VoiceId> = self
            .sources
            .drain()
            .map(|(_, v)| v.id)
            .chain(self.one_shots.drain(..).map(|o| o.id))
            .chain(self.clips.drain(..))
            .collect();
        if let Some(b) = self.backend.as_mut() {
            for id in ids {
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
            .collect();
        if let Some(b) = self.backend.as_mut() {
            for id in resumable {
                if paused { b.pause(id) } else { b.resume(id) }
            }
        }
    }

    /// Per frame: stop voices of destroyed/disabled sources, drop finished ones, and update
    /// volume, pitch and pan from the current positions.
    pub fn update(&mut self, scene: &Scene) {
        let Some(b) = self.backend.as_mut() else {
            return;
        };
        let ear = listener(scene);
        let muted = self.muted;
        let mut ended = Vec::new();
        self.sources.retain(|e, v| {
            let alive = b.is_alive(v.id);
            match playable(scene, *e).filter(|_| alive) {
                Some(src) => {
                    b.set(
                        v.id,
                        spatial::mix(&src, scene.world_position(*e), ear, muted),
                    );
                    true
                }
                None => {
                    ended.push(v.id);
                    false
                }
            }
        });
        self.one_shots.retain(|o| {
            let alive = b.is_alive(o.id);
            match playable(scene, o.source).filter(|_| alive) {
                Some(src) => {
                    let mut p = spatial::mix(&src, scene.world_position(o.source), ear, muted);
                    p.volume *= o.scale;
                    p.looping = false;
                    b.set(o.id, p);
                    true
                }
                None => {
                    ended.push(o.id);
                    false
                }
            }
        });
        self.clips.retain(|id| b.is_alive(*id));
        for id in ended {
            b.stop(id);
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
mod tests;
