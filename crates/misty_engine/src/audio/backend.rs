//! Audio output backends: our mixer graph (on the sound device or rendered offline, see
//! [`super::graph_backend`]), and a silent recorder for tests.
//!
//! Voices play on tracks: a tree of sub-mixes with volume, an effect chain and a peak meter.
//! Mixer groups and per-source filters are tracks; a voice without a track goes straight out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::VoiceParams;
use super::dsp::Frame;
use super::effects::EffectSpec;

/// Handle to one playing sound.
pub type VoiceId = u64;
/// Handle to one track.
pub type TrackId = u64;

/// What [`super::Audio`] needs from a sound output.
pub trait AudioBackend {
    /// Start playing the clip file at `path` (absolute) on `track` (`None` = main output).
    fn play(
        &mut self,
        path: &Path,
        params: VoiceParams,
        track: Option<TrackId>,
    ) -> Result<VoiceId, String>;
    fn set(&mut self, voice: VoiceId, params: VoiceParams);
    fn pause(&mut self, voice: VoiceId);
    fn resume(&mut self, voice: VoiceId);
    fn stop(&mut self, voice: VoiceId);
    /// False once the voice has finished or was stopped (paused voices still count).
    fn is_alive(&self, voice: VoiceId) -> bool;
    /// Playback position in seconds.
    fn position(&self, voice: VoiceId) -> f64;
    fn seek(&mut self, voice: VoiceId, seconds: f64);
    /// A track under `parent` (`None` = main output) with an effect chain.
    fn add_track(
        &mut self,
        parent: Option<TrackId>,
        effects: &[EffectSpec],
    ) -> Result<TrackId, String>;
    /// New volume (linear) and effect values; `effects` matches the chain it was built with.
    fn set_track(&mut self, track: TrackId, volume: f32, effects: &[EffectSpec]);
    /// Give a track a different chain (effects added, removed or moved) while it plays: effects
    /// that stay keep their state, the change crossfades.
    fn set_chain(&mut self, track: TrackId, effects: &[EffectSpec]);
    /// Route a track into another parent (crossfaded).
    fn set_parent(&mut self, track: TrackId, parent: Option<TrackId>);
    /// Move a playing voice to another track (crossfaded).
    fn move_voice(&mut self, voice: VoiceId, track: Option<TrackId>);
    /// Remove a track (its sounds stop). Remove children first.
    fn remove_track(&mut self, track: TrackId);
    /// Loudest sample (linear) on the track since the last call.
    fn track_peak(&mut self, track: TrackId) -> f32;
    /// Strongest gain reduction (dB, ≤ 0) of a Compressor / Duck Volume on the track since the
    /// last call.
    fn track_reduction(&mut self, track: TrackId) -> f32 {
        let _ = track;
        0.0
    }
    /// Decode an in-memory sound file and play it whenever `path` is played (sound that has no
    /// file, such as a video's audio track).
    fn preload(&mut self, path: &Path, bytes: Vec<u8>) -> Result<(), String> {
        let _ = (path, bytes);
        Ok(())
    }
    /// Drop loaded clips (their import settings changed).
    fn forget_clips(&mut self) {}
    /// Offline backends: render the next `frames` of the mix.
    fn render(&mut self, frames: usize) -> Option<Vec<Frame>> {
        let _ = frames;
        None
    }
}

// ---- null ------------------------------------------------------------------------------------

/// A voice the null backend pretends to play.
#[derive(Clone, Debug, PartialEq)]
pub struct NullVoice {
    /// Clip path as given to `play`.
    pub path: PathBuf,
    pub params: VoiceParams,
    pub paused: bool,
    pub track: Option<TrackId>,
    /// Seconds, as last set by `seek`.
    pub position: f64,
}

/// A track the null backend pretends to mix.
#[derive(Clone, Debug, PartialEq)]
pub struct NullTrack {
    pub parent: Option<TrackId>,
    pub volume: f32,
    pub effects: Vec<EffectSpec>,
}

/// Everything the null backend has been asked to do (shared so tests can inspect it).
#[derive(Debug, Default)]
pub struct NullLog {
    /// Clip of every `play` call, in order.
    pub played: Vec<PathBuf>,
    /// Voices still "playing" (they never finish on their own; see [`NullLog::finish`]).
    pub voices: HashMap<VoiceId, NullVoice>,
    pub tracks: HashMap<TrackId, NullTrack>,
    next: u64,
}

impl NullLog {
    /// Pretend a voice reached the end of its clip.
    pub fn finish(&mut self, voice: VoiceId) {
        self.voices.remove(&voice);
    }

    /// Product of the track volumes from `track` out to the main output.
    pub fn chain_volume(&self, track: Option<TrackId>) -> f32 {
        let mut v = 1.0;
        let mut cur = track;
        while let Some(t) = cur.and_then(|t| self.tracks.get(&t)) {
            v *= t.volume;
            cur = t.parent;
        }
        v
    }

    /// Whether `track` is `ancestor` or feeds into it.
    pub fn routes_through(&self, track: Option<TrackId>, ancestor: TrackId) -> bool {
        let mut cur = track;
        while let Some(t) = cur {
            if t == ancestor {
                return true;
            }
            cur = self.tracks.get(&t).and_then(|t| t.parent);
        }
        false
    }

    /// What a meter on `track` would read: the loudest voice feeding it, after the volumes
    /// between the voice and that track (a pretend "signal level" of 1 per voice).
    pub fn level(&self, track: TrackId) -> f32 {
        self.voices
            .values()
            .filter(|v| !v.paused && self.routes_through(v.track, track))
            .map(|v| v.params.volume * self.volume_between(v.track, track))
            .fold(0.0, f32::max)
    }

    /// Product of the track volumes from `from` up to and including `to`.
    fn volume_between(&self, from: Option<TrackId>, to: TrackId) -> f32 {
        let mut v = 1.0;
        let mut cur = from;
        while let Some(id) = cur {
            let Some(t) = self.tracks.get(&id) else { break };
            v *= t.volume;
            if id == to {
                break;
            }
            cur = t.parent;
        }
        v
    }
}

/// Silent backend: records calls, never touches a sound device.
#[derive(Clone, Default)]
pub struct NullBackend {
    pub log: Arc<Mutex<NullLog>>,
}

impl NullBackend {
    fn log(&self) -> std::sync::MutexGuard<'_, NullLog> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl AudioBackend for NullBackend {
    fn play(
        &mut self,
        path: &Path,
        params: VoiceParams,
        track: Option<TrackId>,
    ) -> Result<VoiceId, String> {
        let mut log = self.log();
        log.next += 1;
        let id = log.next;
        log.played.push(path.to_owned());
        log.voices.insert(
            id,
            NullVoice {
                path: path.to_owned(),
                params,
                paused: false,
                track,
                position: 0.0,
            },
        );
        Ok(id)
    }

    fn set(&mut self, voice: VoiceId, params: VoiceParams) {
        if let Some(v) = self.log().voices.get_mut(&voice) {
            v.params = params;
        }
    }

    fn pause(&mut self, voice: VoiceId) {
        if let Some(v) = self.log().voices.get_mut(&voice) {
            v.paused = true;
        }
    }

    fn resume(&mut self, voice: VoiceId) {
        if let Some(v) = self.log().voices.get_mut(&voice) {
            v.paused = false;
        }
    }

    fn stop(&mut self, voice: VoiceId) {
        self.log().voices.remove(&voice);
    }

    fn is_alive(&self, voice: VoiceId) -> bool {
        self.log().voices.contains_key(&voice)
    }

    fn position(&self, voice: VoiceId) -> f64 {
        self.log().voices.get(&voice).map_or(0.0, |v| v.position)
    }

    fn seek(&mut self, voice: VoiceId, seconds: f64) {
        if let Some(v) = self.log().voices.get_mut(&voice) {
            v.position = seconds.max(0.0);
        }
    }

    fn add_track(
        &mut self,
        parent: Option<TrackId>,
        effects: &[EffectSpec],
    ) -> Result<TrackId, String> {
        let mut log = self.log();
        log.next += 1;
        let id = log.next;
        log.tracks.insert(
            id,
            NullTrack {
                parent,
                volume: 1.0,
                effects: effects.to_vec(),
            },
        );
        Ok(id)
    }

    fn set_track(&mut self, track: TrackId, volume: f32, effects: &[EffectSpec]) {
        if let Some(t) = self.log().tracks.get_mut(&track) {
            t.volume = volume;
            // The chain itself only changes with `set_chain`.
            for (fx, new) in t.effects.iter_mut().zip(effects) {
                if fx.kind == new.kind {
                    *fx = new.clone();
                }
            }
        }
    }

    fn set_chain(&mut self, track: TrackId, effects: &[EffectSpec]) {
        if let Some(t) = self.log().tracks.get_mut(&track) {
            t.effects = effects.to_vec();
        }
    }

    fn set_parent(&mut self, track: TrackId, parent: Option<TrackId>) {
        if let Some(t) = self.log().tracks.get_mut(&track) {
            t.parent = parent;
        }
    }

    fn move_voice(&mut self, voice: VoiceId, track: Option<TrackId>) {
        if let Some(v) = self.log().voices.get_mut(&voice) {
            v.track = track;
        }
    }

    fn remove_track(&mut self, track: TrackId) {
        let mut log = self.log();
        log.tracks.remove(&track);
        log.voices.retain(|_, v| v.track != Some(track));
    }

    fn track_peak(&mut self, track: TrackId) -> f32 {
        let log = self.log();
        if log.tracks.contains_key(&track) {
            log.level(track)
        } else {
            0.0
        }
    }
}
