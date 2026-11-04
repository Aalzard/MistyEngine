//! Audio output backends: kira on the real sound device, and a silent recorder for tests.
//!
//! Voices play on tracks: a tree of sub-mixes with volume, an effect chain and a peak meter.
//! Mixer groups and per-source filters are tracks; a voice without a track goes straight out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};

use super::VoiceParams;
use super::effects::{EffectSpec, amplitude_to_db};
use super::kira_fx::{self, FxHandle, PeakMeter};

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
    /// A track under `parent` (`None` = main output) with an effect chain fixed from now on.
    fn add_track(
        &mut self,
        parent: Option<TrackId>,
        effects: &[EffectSpec],
    ) -> Result<TrackId, String>;
    /// New volume (linear) and effect values; `effects` matches the chain it was built with.
    fn set_track(&mut self, track: TrackId, volume: f32, effects: &[EffectSpec]);
    /// Remove a track (its sounds stop). Remove children first.
    fn remove_track(&mut self, track: TrackId);
    /// Loudest sample (linear) on the track since the last call.
    fn track_peak(&mut self, track: TrackId) -> f32;
}

// ---- kira ------------------------------------------------------------------------------------

struct KiraTrack {
    handle: TrackHandle,
    effects: Vec<FxHandle>,
    meter: Arc<AtomicU32>,
}

/// Plays through the default output device with kira.
pub struct KiraBackend {
    manager: AudioManager<DefaultBackend>,
    clips: HashMap<PathBuf, StaticSoundData>,
    voices: HashMap<VoiceId, (StaticSoundHandle, bool)>,
    tracks: HashMap<TrackId, KiraTrack>,
    next: u64,
}

impl KiraBackend {
    pub fn new() -> Result<Self, String> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            manager,
            clips: HashMap::new(),
            voices: HashMap::new(),
            tracks: HashMap::new(),
            next: 1,
        })
    }

    fn clip(&mut self, path: &Path) -> Result<StaticSoundData, String> {
        if let Some(c) = self.clips.get(path) {
            return Ok(c.clone());
        }
        let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let data =
            StaticSoundData::from_cursor(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        self.clips.insert(path.to_owned(), data.clone());
        Ok(data)
    }

    fn fresh(&mut self) -> u64 {
        self.next += 1;
        self.next - 1
    }
}

/// Linear amplitude to kira's decibels (-80 dB and below is silence).
fn decibels(amplitude: f32) -> Decibels {
    if amplitude <= 1e-4 {
        Decibels::SILENCE
    } else {
        Decibels(amplitude_to_db(amplitude))
    }
}

/// kira can't play backwards; negative pitch plays forwards at that speed.
fn rate(pitch: f32) -> f64 {
    pitch.abs().max(0.01) as f64
}

impl AudioBackend for KiraBackend {
    fn play(
        &mut self,
        path: &Path,
        p: VoiceParams,
        track: Option<TrackId>,
    ) -> Result<VoiceId, String> {
        let mut data = self
            .clip(path)?
            .volume(decibels(p.volume))
            .playback_rate(rate(p.pitch))
            .panning(p.pan);
        if p.looping {
            data = data.loop_region(..);
        }
        let handle = match track.and_then(|t| self.tracks.get_mut(&t)) {
            Some(t) => t.handle.play(data),
            None => self.manager.play(data),
        }
        .map_err(|e| e.to_string())?;
        let id = self.fresh();
        self.voices.insert(id, (handle, p.looping));
        Ok(id)
    }

    fn set(&mut self, voice: VoiceId, p: VoiceParams) {
        let Some((h, looping)) = self.voices.get_mut(&voice) else {
            return;
        };
        let tween = Tween::default();
        h.set_volume(decibels(p.volume), tween);
        h.set_playback_rate(rate(p.pitch), tween);
        h.set_panning(p.pan, tween);
        if *looping != p.looping {
            *looping = p.looping;
            if p.looping {
                h.set_loop_region(..);
            } else {
                h.set_loop_region(None);
            }
        }
    }

    fn pause(&mut self, voice: VoiceId) {
        if let Some((h, _)) = self.voices.get_mut(&voice) {
            h.pause(Tween::default());
        }
    }

    fn resume(&mut self, voice: VoiceId) {
        if let Some((h, _)) = self.voices.get_mut(&voice) {
            h.resume(Tween::default());
        }
    }

    fn stop(&mut self, voice: VoiceId) {
        if let Some((mut h, _)) = self.voices.remove(&voice) {
            h.stop(Tween::default());
        }
    }

    fn is_alive(&self, voice: VoiceId) -> bool {
        self.voices
            .get(&voice)
            .is_some_and(|(h, _)| h.state() != PlaybackState::Stopped)
    }

    fn position(&self, voice: VoiceId) -> f64 {
        self.voices.get(&voice).map_or(0.0, |(h, _)| h.position())
    }

    fn seek(&mut self, voice: VoiceId, seconds: f64) {
        if let Some((h, _)) = self.voices.get_mut(&voice) {
            h.seek_to(seconds.max(0.0));
        }
    }

    fn add_track(
        &mut self,
        parent: Option<TrackId>,
        effects: &[EffectSpec],
    ) -> Result<TrackId, String> {
        let mut builder = TrackBuilder::new();
        let fx = effects
            .iter()
            .map(|spec| kira_fx::add(&mut builder, spec))
            .collect();
        let meter = Arc::new(AtomicU32::new(0));
        builder.add_effect(PeakMeter(meter.clone()));
        let handle = match parent.and_then(|p| self.tracks.get_mut(&p)) {
            Some(p) => p.handle.add_sub_track(builder),
            None => self.manager.add_sub_track(builder),
        }
        .map_err(|e| e.to_string())?;
        let id = self.fresh();
        self.tracks.insert(
            id,
            KiraTrack {
                handle,
                effects: fx,
                meter,
            },
        );
        Ok(id)
    }

    fn set_track(&mut self, track: TrackId, volume: f32, effects: &[EffectSpec]) {
        if let Some(t) = self.tracks.get_mut(&track) {
            t.handle.set_volume(decibels(volume), Tween::default());
            for (h, spec) in t.effects.iter_mut().zip(effects) {
                kira_fx::set(h, spec);
            }
        }
    }

    fn remove_track(&mut self, track: TrackId) {
        // Dropping the handle removes the track.
        self.tracks.remove(&track);
    }

    fn track_peak(&mut self, track: TrackId) -> f32 {
        self.tracks
            .get(&track)
            .map_or(0.0, |t| kira_fx::take_peak(&t.meter))
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
            // Like kira: the chain itself is fixed, only values change.
            for (fx, new) in t.effects.iter_mut().zip(effects) {
                if fx.kind == new.kind {
                    *fx = new.clone();
                }
            }
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
