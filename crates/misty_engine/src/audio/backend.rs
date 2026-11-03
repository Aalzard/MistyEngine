//! Audio output backends: kira on the real sound device, and a silent recorder for tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};

use super::VoiceParams;

/// Handle to one playing sound.
pub type VoiceId = u64;

/// What [`super::Audio`] needs from a sound output.
pub trait AudioBackend {
    /// Start playing the clip file at `path` (absolute).
    fn play(&mut self, path: &Path, params: VoiceParams) -> Result<VoiceId, String>;
    fn set(&mut self, voice: VoiceId, params: VoiceParams);
    fn pause(&mut self, voice: VoiceId);
    fn resume(&mut self, voice: VoiceId);
    fn stop(&mut self, voice: VoiceId);
    /// False once the voice has finished or was stopped (paused voices still count).
    fn is_alive(&self, voice: VoiceId) -> bool;
}

// ---- kira ------------------------------------------------------------------------------------

/// Plays through the default output device with kira.
pub struct KiraBackend {
    manager: AudioManager<DefaultBackend>,
    clips: HashMap<PathBuf, StaticSoundData>,
    voices: HashMap<VoiceId, (StaticSoundHandle, bool)>,
    next: VoiceId,
}

impl KiraBackend {
    pub fn new() -> Result<Self, String> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            manager,
            clips: HashMap::new(),
            voices: HashMap::new(),
            next: 1,
        })
    }

    fn clip(&mut self, path: &Path) -> Result<StaticSoundData, String> {
        if let Some(c) = self.clips.get(path) {
            return Ok(c.clone());
        }
        let data = StaticSoundData::from_file(path).map_err(|e| e.to_string())?;
        self.clips.insert(path.to_owned(), data.clone());
        Ok(data)
    }
}

/// Linear amplitude to kira's decibels (-60 dB is silence).
fn decibels(amplitude: f32) -> Decibels {
    if amplitude <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * amplitude.log10())
    }
}

/// kira can't play backwards; negative pitch plays forwards at that speed.
fn rate(pitch: f32) -> f64 {
    pitch.abs().max(0.01) as f64
}

impl AudioBackend for KiraBackend {
    fn play(&mut self, path: &Path, p: VoiceParams) -> Result<VoiceId, String> {
        let mut data = self
            .clip(path)?
            .volume(decibels(p.volume))
            .playback_rate(rate(p.pitch))
            .panning(p.pan);
        if p.looping {
            data = data.loop_region(..);
        }
        let handle = self.manager.play(data).map_err(|e| e.to_string())?;
        let id = self.next;
        self.next += 1;
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
}

// ---- null ------------------------------------------------------------------------------------

/// A voice the null backend pretends to play.
#[derive(Clone, Debug, PartialEq)]
pub struct NullVoice {
    /// Clip path as given to `play`.
    pub path: PathBuf,
    pub params: VoiceParams,
    pub paused: bool,
}

/// Everything the null backend has been asked to do (shared so tests can inspect it).
#[derive(Debug, Default)]
pub struct NullLog {
    /// Clip of every `play` call, in order.
    pub played: Vec<PathBuf>,
    /// Voices still "playing" (they never finish on their own; see [`NullLog::finish`]).
    pub voices: HashMap<VoiceId, NullVoice>,
    next: VoiceId,
}

impl NullLog {
    /// Pretend a voice reached the end of its clip.
    pub fn finish(&mut self, voice: VoiceId) {
        self.voices.remove(&voice);
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
    fn play(&mut self, path: &Path, params: VoiceParams) -> Result<VoiceId, String> {
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
}
