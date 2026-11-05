//! Video Players while playing: opening clips, the playback clock, picking decoded frames,
//! publishing them as runtime textures, the audio track and the events scripts listen to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hecs::Entity;

use super::component::{VideoAudioOutput, VideoMaterialProperty, VideoPlayer, VideoRenderMode};
use super::decoder::{Decoded, Decoder};
use super::source::VideoSource;
use super::textures;
use crate::audio::{Audio, VoiceId};
use crate::scene::{MeshRenderer, Scene};

/// Something scripts can listen to (`vp.started:AddListener(f)`, …).
#[derive(Clone, Debug, PartialEq)]
pub enum VideoEvent {
    /// The first frame is ready (`prepareCompleted`).
    PrepareCompleted(Entity),
    /// Playback began (`started`).
    Started(Entity),
    /// The end of the clip was reached, looping or not (`loopPointReached`).
    LoopPointReached(Entity),
    /// The clip couldn't be opened or decoded (`errorReceived`).
    Error(Entity, String),
}

/// Where a player's audio is going.
enum AudioVoice {
    Source(Entity),
    Direct(VoiceId),
}

struct Active {
    source: Arc<VideoSource>,
    decoder: Decoder,
    texture: String,
    prepared: bool,
    playing: bool,
    /// Play() was called before the first frame arrived.
    play_when_ready: bool,
    /// Stopped at the end of a non-looping clip.
    finished: bool,
    /// Seconds into the clip.
    clock: f64,
    loops: u64,
    /// Frame index on screen.
    shown: Option<u64>,
    /// Decoded ahead of the clock.
    pending: Option<Decoded>,
    /// Late frames skipped.
    dropped: u64,
    looping: bool,
    /// Registered audio clip path (None = no audio track or muted).
    audio_clip: Option<String>,
    voice: Option<AudioVoice>,
    /// The Material Override this player put on a renderer: (renderer, element).
    override_on: Option<(Entity, usize)>,
}

/// Every Video Player of the running game.
pub struct VideoPlayback {
    root: PathBuf,
    players: HashMap<Entity, Active>,
    logs: Vec<String>,
}

/// Resolve a clip or URL path to a file on disk (project-relative unless absolute).
fn resolve(root: &Path, file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_owned()
    } else {
        root.join(p)
    }
}

impl VideoPlayback {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            players: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// Prepare and play every enabled Video Player with Play On Awake (under `root` only, for
    /// objects created while playing).
    pub fn play_on_awake(
        &mut self,
        scene: &mut Scene,
        audio: &mut Audio,
        root: Option<Entity>,
    ) -> Vec<VideoEvent> {
        let awake: Vec<Entity> = scene
            .world
            .query::<(Entity, &VideoPlayer)>()
            .iter()
            .filter(|(e, v)| {
                v.enabled && v.play_on_awake && root.is_none_or(|r| scene.is_ancestor(r, *e))
            })
            .map(|(e, _)| e)
            .collect();
        let mut events = Vec::new();
        for e in awake {
            if scene.active_in_hierarchy(e) && !self.players.contains_key(&e) {
                events.extend(self.play(scene, audio, e));
            }
        }
        events
    }

    fn settings(scene: &Scene, e: Entity) -> Option<VideoPlayer> {
        scene
            .world
            .get::<&VideoPlayer>(e)
            .ok()
            .map(|v| (*v).clone())
    }

    /// `Prepare()`: open the clip and start decoding (no-op when already prepared). Errors are
    /// returned as an event.
    pub fn prepare(&mut self, scene: &mut Scene, e: Entity) -> Result<(), String> {
        if self.players.contains_key(&e) {
            return Ok(());
        }
        let vp = Self::settings(scene, e).ok_or("no Video Player")?;
        let file = vp.file().ok_or("no clip or URL set")?;
        if file.starts_with("http://") || file.starts_with("https://") {
            return Err(format!(
                "{file}: only local files can be played (no network streaming)"
            ));
        }
        let source = Arc::new(VideoSource::open_file(&resolve(&self.root, &file))?);
        // Render Texture players publish under their target name, the others under their own.
        let texture = match vp.render_mode {
            VideoRenderMode::RenderTexture => textures::target_name(&vp.target_texture),
            _ => None,
        }
        .unwrap_or_else(textures::new_name);
        let decoder = Decoder::start(source.clone(), vp.looping);
        let audio_clip = (source.info().audio.is_some()
            && vp.audio_output != VideoAudioOutput::None)
            .then(|| format!("{file}#audio"));
        let mut active = Active {
            source,
            decoder,
            texture: texture.clone(),
            prepared: false,
            playing: false,
            play_when_ready: false,
            finished: false,
            clock: 0.0,
            loops: 0,
            shown: None,
            pending: None,
            dropped: 0,
            looping: vp.looping,
            audio_clip,
            voice: None,
            override_on: None,
        };
        if let Ok(mut c) = scene.world.get::<&mut VideoPlayer>(e) {
            c.runtime_texture = Some(texture.clone());
        }
        if vp.render_mode == VideoRenderMode::MaterialOverride {
            let target = vp.target_renderer.unwrap_or(e);
            let slot = vp.target_material_index;
            if let Ok(mut r) = scene.world.get::<&mut MeshRenderer>(target) {
                r.edit_overrides(slot, |o| match vp.target_material_property {
                    VideoMaterialProperty::MainTexture => o.main_texture = Some(texture.clone()),
                    VideoMaterialProperty::EmissionMap => {
                        o.emission_texture = Some(texture.clone())
                    }
                });
                active.override_on = Some((target, slot));
            } else {
                self.logs.push(format!(
                    "Video Player on {}: Material Override needs a Mesh Renderer",
                    scene.name(e)
                ));
            }
        }
        self.players.insert(e, active);
        Ok(())
    }

    /// `Play()`: prepare if needed, then play (from the start after the end, else resume).
    pub fn play(&mut self, scene: &mut Scene, audio: &mut Audio, e: Entity) -> Vec<VideoEvent> {
        let mut events = Vec::new();
        if let Err(err) = self.prepare(scene, e) {
            self.logs
                .push(format!("Video Player on {}: {err}", scene.name(e)));
            events.push(VideoEvent::Error(e, err));
            return events;
        }
        let Some(a) = self.players.get_mut(&e) else {
            return events;
        };
        if a.playing {
            return events;
        }
        if !a.prepared {
            a.play_when_ready = true;
            return events;
        }
        if a.finished {
            a.finished = false;
            a.clock = 0.0;
            a.loops = 0;
            a.pending = None;
            a.decoder.seek(0);
        }
        a.playing = true;
        events.push(VideoEvent::Started(e));
        self.start_audio(scene, audio, e);
        events
    }

    pub fn pause(&mut self, audio: &mut Audio, e: Entity) {
        let Some(a) = self.players.get_mut(&e) else {
            return;
        };
        a.playing = false;
        a.play_when_ready = false;
        match a.voice {
            Some(AudioVoice::Source(s)) => audio.pause(s),
            Some(AudioVoice::Direct(id)) => audio.voice_pause(id),
            None => {}
        }
    }

    /// `Stop()`: stop, release the decoder and texture, rewind.
    pub fn stop(&mut self, scene: &mut Scene, audio: &mut Audio, e: Entity) {
        let Some(a) = self.players.remove(&e) else {
            return;
        };
        Self::release(scene, audio, e, a);
    }

    fn release(scene: &mut Scene, audio: &mut Audio, e: Entity, a: Active) {
        match a.voice {
            Some(AudioVoice::Source(s)) => audio.stop(s),
            Some(AudioVoice::Direct(id)) => audio.voice_stop(id),
            None => {}
        }
        textures::remove(&a.texture);
        if let Ok(mut c) = scene.world.get::<&mut VideoPlayer>(e) {
            c.runtime_texture = None;
        }
        if let Some((r, slot)) = a.override_on
            && let Ok(mut mr) = scene.world.get::<&mut MeshRenderer>(r)
        {
            let ours = Some(a.texture.as_str());
            mr.edit_overrides(slot, |o| {
                if o.main_texture.as_deref() == ours {
                    o.main_texture = None;
                }
                if o.emission_texture.as_deref() == ours {
                    o.emission_texture = None;
                }
            });
        }
    }

    /// Stop every player (Play mode ends).
    pub fn stop_all(&mut self, scene: &mut Scene, audio: &mut Audio) {
        for (e, a) in std::mem::take(&mut self.players) {
            Self::release(scene, audio, e, a);
        }
    }

    pub fn is_playing(&self, e: Entity) -> bool {
        self.players
            .get(&e)
            .is_some_and(|a| a.playing || a.play_when_ready)
    }

    pub fn is_prepared(&self, e: Entity) -> bool {
        self.players.get(&e).is_some_and(|a| a.prepared)
    }

    pub fn is_paused(&self, e: Entity) -> bool {
        self.players
            .get(&e)
            .is_some_and(|a| a.prepared && !a.playing && !a.finished)
    }

    pub fn is_looping(&self, scene: &Scene, e: Entity) -> bool {
        match self.players.get(&e) {
            Some(a) => a.looping,
            None => Self::settings(scene, e).is_some_and(|v| v.looping),
        }
    }

    pub fn set_looping(&mut self, scene: &mut Scene, e: Entity, on: bool) {
        if let Ok(mut c) = scene.world.get::<&mut VideoPlayer>(e) {
            c.looping = on;
        }
        if let Some(a) = self.players.get_mut(&e) {
            a.looping = on;
            a.decoder.set_looping(on);
        }
    }

    /// `time`: seconds into the clip.
    pub fn time(&self, e: Entity) -> f64 {
        self.players.get(&e).map_or(0.0, |a| a.clock)
    }

    /// `frame`: index of the frame on screen (-1 before the first).
    pub fn frame(&self, e: Entity) -> i64 {
        self.players
            .get(&e)
            .and_then(|a| a.shown)
            .map_or(-1, |f| f as i64)
    }

    /// `length` in seconds (0 until prepared or opened).
    pub fn length(&self, e: Entity) -> f64 {
        self.players.get(&e).map_or(0.0, |a| a.source.info().length)
    }

    pub fn frame_count(&self, e: Entity) -> u64 {
        self.players
            .get(&e)
            .map_or(0, |a| a.source.info().frame_count)
    }

    pub fn frame_rate(&self, e: Entity) -> f64 {
        self.players.get(&e).map_or(0.0, |a| a.source.info().fps)
    }

    /// Frames skipped because they were late.
    pub fn dropped_frames(&self, e: Entity) -> u64 {
        self.players.get(&e).map_or(0, |a| a.dropped)
    }

    /// The runtime texture name frames are published under.
    pub fn texture(&self, e: Entity) -> Option<String> {
        self.players.get(&e).map(|a| a.texture.clone())
    }

    /// Pixel size of the clip.
    pub fn size(&self, e: Entity) -> Option<(u32, u32)> {
        self.players
            .get(&e)
            .map(|a| (a.source.info().width, a.source.info().height))
    }

    /// Seek to `t` seconds (`time = t`).
    pub fn set_time(&mut self, audio: &mut Audio, e: Entity, t: f64) {
        let Some(a) = self.players.get_mut(&e) else {
            return;
        };
        let len = a.source.info().length;
        let t = t.clamp(0.0, len.max(0.0));
        let frame = a.source.frame_at(t);
        a.clock = t;
        a.loops = 0;
        a.finished = false;
        a.pending = None;
        a.decoder.seek(frame);
        match a.voice {
            Some(AudioVoice::Source(s)) => audio.set_time(s, t as f32),
            Some(AudioVoice::Direct(id)) => audio.voice_seek(id, t),
            None => {}
        }
    }

    /// Seek to frame `n` (`frame = n`).
    pub fn set_frame(&mut self, audio: &mut Audio, e: Entity, n: i64) {
        let Some(t) = self
            .players
            .get(&e)
            .map(|a| a.source.frame_time(n.max(0) as u64))
        else {
            return;
        };
        self.set_time(audio, e, t);
    }

    fn start_audio(&mut self, scene: &Scene, audio: &mut Audio, e: Entity) {
        let Some(vp) = Self::settings(scene, e) else {
            return;
        };
        let Some(a) = self.players.get_mut(&e) else {
            return;
        };
        if vp.mute {
            return;
        }
        let Some(clip) = a.audio_clip.clone() else {
            return;
        };
        // Resume a paused voice.
        match a.voice {
            Some(AudioVoice::Source(s)) if audio.source_voice(s).is_some() => {
                audio.unpause(s);
                return;
            }
            Some(AudioVoice::Direct(id)) if audio.voice_alive(id) => {
                audio.voice_resume(id);
                return;
            }
            _ => {}
        }
        let Some(pcm) = a.source.audio() else {
            return;
        };
        if !audio.register_clip(&clip, pcm.to_wav()) {
            return;
        }
        let t = a.clock;
        let speed = vp.playback_speed.clamp(0.01, 10.0);
        let source_target = vp.audio_source.unwrap_or(e);
        let use_source = vp.audio_output == VideoAudioOutput::AudioSource
            && scene
                .world
                .get::<&crate::scene::AudioSource>(source_target)
                .is_ok();
        if vp.audio_output == VideoAudioOutput::AudioSource && !use_source {
            self.logs.push(format!(
                "Video Player on {}: no Audio Source to play through; playing directly",
                scene.name(e)
            ));
        }
        let Some(a) = self.players.get_mut(&e) else {
            return;
        };
        a.voice = if use_source {
            audio
                .play_source_clip(scene, source_target, &clip)
                .then_some(AudioVoice::Source(source_target))
        } else {
            audio
                .play_direct(&clip, vp.direct_volume, speed)
                .map(AudioVoice::Direct)
        };
        if t > 0.0 {
            match a.voice {
                Some(AudioVoice::Source(s)) => audio.set_time(s, t as f32),
                Some(AudioVoice::Direct(id)) => audio.voice_seek(id, t),
                None => {}
            }
        }
    }

    /// Audio sample position (seconds) of a player's audio voice, if it has one.
    pub fn audio_time(&self, audio: &Audio, e: Entity) -> Option<f64> {
        match self.players.get(&e)?.voice.as_ref()? {
            AudioVoice::Source(s) => Some(audio.time(*s) as f64),
            AudioVoice::Direct(id) => Some(audio.voice_position(*id)),
        }
    }

    /// Advance every playing clip by `dt` seconds, show the frame due, and report events.
    pub fn update(&mut self, scene: &mut Scene, audio: &mut Audio, dt: f32) -> Vec<VideoEvent> {
        let mut events = Vec::new();
        // Players whose component went away or was disabled stop.
        let gone: Vec<Entity> = self
            .players
            .keys()
            .copied()
            .filter(|e| {
                !scene.contains(*e)
                    || !scene.active_in_hierarchy(*e)
                    || Self::settings(scene, *e).is_none_or(|v| !v.enabled)
            })
            .collect();
        for e in gone {
            self.stop(scene, audio, e);
        }
        let entities: Vec<Entity> = self.players.keys().copied().collect();
        let mut restart_audio = Vec::new();
        let mut start = Vec::new();
        for e in entities {
            let Some(vp) = Self::settings(scene, e) else {
                continue;
            };
            let Some(a) = self.players.get_mut(&e) else {
                continue;
            };
            let info = a.source.info().clone();
            if !a.prepared {
                if let Some(d) = a.pending.take().or_else(|| a.decoder.try_next()) {
                    match &d.image {
                        Ok(img) => textures::publish(&a.texture, img),
                        Err(err) => {
                            events.push(VideoEvent::Error(e, err.clone()));
                            self.logs.push(format!("Video Player: {err}"));
                        }
                    }
                    a.shown = Some(d.index);
                    a.prepared = true;
                    events.push(VideoEvent::PrepareCompleted(e));
                    if a.play_when_ready {
                        a.play_when_ready = false;
                        start.push(e);
                    }
                } else if !vp.wait_for_first_frame && a.play_when_ready {
                    // Start the clock anyway; frames catch up.
                    a.play_when_ready = false;
                    a.prepared = true;
                    events.push(VideoEvent::PrepareCompleted(e));
                    start.push(e);
                }
                continue;
            }
            if !a.playing {
                continue;
            }
            let speed = vp.playback_speed.clamp(0.01, 10.0) as f64;
            a.clock += dt.max(0.0) as f64 * speed;
            if info.length > 0.0 && a.clock >= info.length {
                events.push(VideoEvent::LoopPointReached(e));
                if a.looping {
                    while a.clock >= info.length {
                        a.clock -= info.length;
                        a.loops += 1;
                    }
                    restart_audio.push(e);
                } else {
                    a.clock = info.length;
                    a.playing = false;
                    a.finished = true;
                    match a.voice.take() {
                        Some(AudioVoice::Source(s)) => audio.stop(s),
                        Some(AudioVoice::Direct(id)) => audio.voice_stop(id),
                        None => {}
                    }
                }
            }
            let n = info.frame_count.max(1);
            let index = if a.finished {
                n - 1
            } else {
                a.source.frame_at(a.clock)
            };
            let target = a.loops * n + index;
            Self::present(a, target, vp.skip_on_drop);
        }
        for e in start {
            if let Some(a) = self.players.get_mut(&e) {
                a.playing = true;
                events.push(VideoEvent::Started(e));
            }
            self.start_audio(scene, audio, e);
        }
        for e in restart_audio {
            if let Some(a) = self.players.get_mut(&e) {
                match a.voice.take() {
                    Some(AudioVoice::Source(s)) => audio.stop(s),
                    Some(AudioVoice::Direct(id)) => audio.voice_stop(id),
                    None => {}
                }
            }
            self.start_audio(scene, audio, e);
        }
        events
    }

    /// Show the newest decoded frame not after `target` (every frame in turn when frames may not
    /// be skipped).
    fn present(a: &mut Active, target: u64, skip: bool) {
        let mut chosen: Option<Decoded> = None;
        loop {
            let next = match a.pending.take() {
                Some(p) => Some(p),
                None => a.decoder.try_next(),
            };
            let Some(d) = next else { break };
            if d.seq > target {
                a.pending = Some(d);
                break;
            }
            if chosen.is_some() {
                a.dropped += 1;
            }
            chosen = Some(d);
            if !skip {
                break;
            }
        }
        if let Some(d) = chosen {
            if let Ok(img) = &d.image {
                textures::publish(&a.texture, img);
            }
            a.shown = Some(d.index);
        }
    }

    /// Wait (up to `timeout`) until every player's first frame is decoded — tests and the
    /// screenshot tool use it so the first rendered frame already shows video.
    pub fn wait_prepared(&mut self, timeout: std::time::Duration) {
        for a in self.players.values_mut() {
            if a.prepared || a.pending.is_some() {
                continue;
            }
            a.pending = a.decoder.next_timeout(timeout);
        }
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

impl Drop for VideoPlayback {
    fn drop(&mut self) {
        for a in self.players.values() {
            textures::remove(&a.texture);
        }
    }
}
