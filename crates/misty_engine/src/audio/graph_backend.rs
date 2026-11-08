//! The real backend: our mixer graph ([`super::engine`]) on the sound device through kira, or
//! rendered offline into buffers (tests, benchmarks). The main-thread half kept here loads clips
//! (as their import settings say), builds effect state, mirrors what the graph has (so unchanged
//! values aren't sent again) and reads levels and positions back.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use kira::sound::static_sound::StaticSoundData;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend};

use super::backend::{AudioBackend, TrackId, VoiceId};
use super::clip::{AudioImporter, LoadType};
use super::dsp::Frame;
use super::effects::EffectSpec;
use super::engine::{Command, Engine, EngineSound, Slot, Source, Status};
use super::spatial::VoiceParams;
use super::stream::{StreamInput, StreamReader};

/// A clip as its load type keeps it.
#[derive(Clone)]
enum Clip {
    Decoded { frames: Arc<[Frame]>, rate: u32 },
    Compressed { bytes: Arc<[u8]>, mono: bool },
    Streamed { path: PathBuf, mono: bool },
}

struct VoiceInfo {
    params: VoiceParams,
}

struct TrackInfo {
    volume: f32,
    effects: Vec<EffectSpec>,
}

/// Plays through our mixer graph.
pub struct GraphBackend {
    tx: Option<rtrb::Producer<Command>>,
    /// Commands that didn't fit in the queue (sent before anything new).
    overflow: VecDeque<Command>,
    /// Offline: the graph itself.
    engine: Option<Engine>,
    _manager: Option<AudioManager<DefaultBackend>>,
    status: Arc<Mutex<Status>>,
    clips: HashMap<PathBuf, Clip>,
    voices: HashMap<VoiceId, VoiceInfo>,
    tracks: HashMap<TrackId, TrackInfo>,
    next: u64,
}

impl GraphBackend {
    /// On the default output device.
    pub fn with_device() -> Result<Self, String> {
        let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|e| e.to_string())?;
        let (tx, rx) = rtrb::RingBuffer::new(8192);
        let status = Arc::new(Mutex::new(Status::default()));
        let engine = Engine::new(48000.0, Some(rx), status.clone());
        manager
            .play(EngineSound(engine))
            .map_err(|_| "could not start the mixer".to_string())?;
        Ok(Self::make(Some(tx), None, Some(manager), status))
    }

    /// Rendered on demand with [`AudioBackend::render`] at `sample_rate`; streamed clips are
    /// waited for, so renders are the same every time.
    pub fn offline(sample_rate: f32) -> Self {
        let status = Arc::new(Mutex::new(Status::default()));
        let mut engine = Engine::new(sample_rate, None, status.clone());
        engine.wait_for_streams = true;
        Self::make(None, Some(engine), None, status)
    }

    fn make(
        tx: Option<rtrb::Producer<Command>>,
        engine: Option<Engine>,
        manager: Option<AudioManager<DefaultBackend>>,
        status: Arc<Mutex<Status>>,
    ) -> Self {
        Self {
            tx,
            overflow: VecDeque::new(),
            engine,
            _manager: manager,
            status,
            clips: HashMap::new(),
            voices: HashMap::new(),
            tracks: HashMap::new(),
            next: 1,
        }
    }

    fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sample_rate(&self) -> f32 {
        if let Some(e) = &self.engine {
            return e.sample_rate;
        }
        let sr = self.status().sample_rate;
        if sr > 0.0 { sr } else { 48000.0 }
    }

    fn fresh(&mut self) -> u64 {
        self.next += 1;
        self.next - 1
    }

    fn send(&mut self, cmd: Command) {
        if let Some(e) = self.engine.as_mut() {
            e.apply(cmd);
            return;
        }
        let Some(tx) = self.tx.as_mut() else { return };
        while let Some(c) = self.overflow.pop_front() {
            if let Err(rtrb::PushError::Full(c)) = tx.push(c) {
                self.overflow.push_front(c);
                break;
            }
        }
        if self.overflow.is_empty() {
            if let Err(rtrb::PushError::Full(c)) = tx.push(cmd) {
                self.overflow.push_back(c);
            }
        } else {
            self.overflow.push_back(cmd);
        }
    }

    /// Forget voices the graph finished.
    fn sync(&mut self) {
        let ended: Vec<VoiceId> = std::mem::take(&mut self.status().ended);
        for id in ended {
            self.voices.remove(&id);
            self.status().positions.remove(&id);
        }
    }

    /// The clip at `path`, loaded as its import settings say.
    fn clip(&mut self, path: &Path) -> Result<Clip, String> {
        if let Some(c) = self.clips.get(path) {
            return Ok(c.clone());
        }
        let import = AudioImporter::load(path);
        let mono = import.force_to_mono;
        let clip = match import.load_type {
            LoadType::Streaming if path.is_file() => Clip::Streamed {
                path: path.to_owned(),
                mono,
            },
            // Streaming from inside a packed build reads the packed bytes.
            LoadType::Streaming | LoadType::CompressedInMemory => Clip::Compressed {
                bytes: read(path)?.into(),
                mono,
            },
            LoadType::DecompressOnLoad => decode(read(path)?, mono)?,
        };
        self.clips.insert(path.to_owned(), clip.clone());
        Ok(clip)
    }

    fn slots(&self, effects: &[EffectSpec]) -> Vec<Slot> {
        let sr = self.sample_rate();
        effects
            .iter()
            .map(|s| Slot::new(s.clone(), sr, false))
            .collect()
    }

    /// Frames rendered so far.
    pub fn frames(&self) -> u64 {
        self.status().frames
    }

    /// Samples streamed voices had to wait for their workers.
    pub fn underruns(&self) -> u64 {
        self.status().underruns
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Decode a whole clip into memory.
fn decode(bytes: Vec<u8>, mono: bool) -> Result<Clip, String> {
    let data =
        StaticSoundData::from_cursor(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let frames: Arc<[Frame]> = if mono {
        data.frames.iter().map(|f| f.as_mono()).collect()
    } else {
        data.frames.clone()
    };
    Ok(Clip::Decoded {
        frames,
        rate: data.sample_rate,
    })
}

/// Structure of a chain: what can't change without [`AudioBackend::set_chain`].
fn shape(effects: &[EffectSpec]) -> Vec<(super::EffectKind, u32)> {
    effects.iter().map(|e| (e.kind, e.id)).collect()
}

impl AudioBackend for GraphBackend {
    fn preload(&mut self, path: &Path, bytes: Vec<u8>) -> Result<(), String> {
        let clip = decode(bytes, false)?;
        self.clips.insert(path.to_owned(), clip);
        Ok(())
    }

    fn forget_clips(&mut self) {
        self.clips.clear();
    }

    fn play(
        &mut self,
        path: &Path,
        params: VoiceParams,
        track: Option<TrackId>,
    ) -> Result<VoiceId, String> {
        self.sync();
        let source = match self.clip(path)? {
            Clip::Decoded { frames, rate } => Source::Static { frames, rate },
            Clip::Compressed { bytes, mono } => Source::Stream(StreamReader::open(
                StreamInput::Memory(bytes),
                params.looping,
                mono,
                0,
            )?),
            Clip::Streamed { path, mono } => Source::Stream(StreamReader::open(
                StreamInput::File(path),
                params.looping,
                mono,
                0,
            )?),
        };
        let id = self.fresh();
        self.voices.insert(id, VoiceInfo { params });
        self.status().positions.insert(id, 0.0);
        self.send(Command::Play {
            id,
            source,
            params,
            track,
        });
        Ok(id)
    }

    fn set(&mut self, voice: VoiceId, params: VoiceParams) {
        self.sync();
        let Some(v) = self.voices.get_mut(&voice) else {
            return;
        };
        if v.params != params {
            v.params = params;
            self.send(Command::SetVoice { id: voice, params });
        }
    }

    fn pause(&mut self, voice: VoiceId) {
        if self.voices.contains_key(&voice) {
            self.send(Command::Pause(voice));
        }
    }

    fn resume(&mut self, voice: VoiceId) {
        if self.voices.contains_key(&voice) {
            self.send(Command::Resume(voice));
        }
    }

    fn stop(&mut self, voice: VoiceId) {
        self.sync();
        if self.voices.remove(&voice).is_some() {
            self.status().positions.remove(&voice);
            self.send(Command::Stop(voice));
        }
    }

    fn is_alive(&self, voice: VoiceId) -> bool {
        self.voices.contains_key(&voice) && !self.status().ended.contains(&voice)
    }

    fn position(&self, voice: VoiceId) -> f64 {
        self.status().positions.get(&voice).copied().unwrap_or(0.0)
    }

    fn seek(&mut self, voice: VoiceId, seconds: f64) {
        if self.voices.contains_key(&voice) {
            self.status().positions.insert(voice, seconds.max(0.0));
            self.send(Command::Seek { id: voice, seconds });
        }
    }

    fn move_voice(&mut self, voice: VoiceId, track: Option<TrackId>) {
        if self.voices.contains_key(&voice) {
            self.send(Command::MoveVoice { id: voice, track });
        }
    }

    fn add_track(
        &mut self,
        parent: Option<TrackId>,
        effects: &[EffectSpec],
    ) -> Result<TrackId, String> {
        let id = self.fresh();
        let chain = self.slots(effects);
        self.tracks.insert(
            id,
            TrackInfo {
                volume: 1.0,
                effects: effects.to_vec(),
            },
        );
        self.send(Command::AddTrack { id, parent, chain });
        Ok(id)
    }

    fn set_track(&mut self, track: TrackId, volume: f32, effects: &[EffectSpec]) {
        let Some(t) = self.tracks.get_mut(&track) else {
            return;
        };
        if t.volume == volume && t.effects == effects {
            return;
        }
        t.volume = volume;
        // Values only: the chain itself changes with `set_chain`.
        for (old, new) in t.effects.iter_mut().zip(effects) {
            if old.kind == new.kind {
                *old = new.clone();
            }
        }
        let effects = t.effects.clone();
        self.send(Command::SetTrack {
            id: track,
            volume,
            effects,
        });
    }

    fn set_chain(&mut self, track: TrackId, effects: &[EffectSpec]) {
        let Some(t) = self.tracks.get(&track) else {
            return;
        };
        if shape(&t.effects) == shape(effects) {
            let volume = t.volume;
            self.set_track(track, volume, effects);
            return;
        }
        let chain = self.slots(effects);
        if let Some(t) = self.tracks.get_mut(&track) {
            t.effects = effects.to_vec();
        }
        self.send(Command::SetChain { id: track, chain });
    }

    fn set_parent(&mut self, track: TrackId, parent: Option<TrackId>) {
        if self.tracks.contains_key(&track) {
            self.send(Command::SetParent { id: track, parent });
        }
    }

    fn remove_track(&mut self, track: TrackId) {
        if self.tracks.remove(&track).is_some() {
            self.send(Command::RemoveTrack(track));
        }
    }

    fn track_peak(&mut self, track: TrackId) -> f32 {
        self.status().peaks.remove(&track).unwrap_or(0.0)
    }

    fn track_reduction(&mut self, track: TrackId) -> f32 {
        self.status().reductions.remove(&track).unwrap_or(0.0)
    }

    fn render(&mut self, frames: usize) -> Option<Vec<Frame>> {
        let e = self.engine.as_mut()?;
        let mut out = vec![Frame::ZERO; frames];
        e.render(&mut out);
        self.sync();
        Some(out)
    }
}
