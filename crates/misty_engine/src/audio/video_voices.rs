//! Voices for sound that isn't a clip file: a Video Player's audio track, decoded into a WAV
//! in memory and handed to the backend under a made-up clip path. It plays either through an
//! Audio Source (its volume, 3D settings and mixer) or straight to the speakers.

use hecs::Entity;

use super::{Audio, SourceVoice, VoiceId, VoiceParams, playable};
use crate::scene::Scene;

impl Audio {
    /// Make `clip` (project-relative, need not exist on disk) play the WAV file `wav`.
    pub fn register_clip(&mut self, clip: &str, wav: Vec<u8>) -> bool {
        let path = self.root.join(clip);
        let Some(b) = self.backend() else {
            return false;
        };
        match b.preload(&path, wav) {
            Ok(()) => true,
            Err(e) => {
                if self.bad_clips.insert(clip.to_owned()) {
                    self.logs.push(format!("Audio clip {clip}: {e}"));
                }
                false
            }
        }
    }

    /// `AudioSource.Play()` with another clip: (re)start source `e` playing `clip`.
    pub fn play_source_clip(&mut self, scene: &Scene, e: Entity, clip: &str) -> bool {
        self.stop(e);
        let Some(source) = playable(scene, e) else {
            return false;
        };
        self.backend();
        let route = self.route(scene, e, &source);
        let mut params = self.ctx(scene).source_params(scene, e, &source, &route);
        params.looping = false;
        match self.start_voice(clip, params, route.target(&self.mixers)) {
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

    /// A 2D voice for `clip` straight to the main output; the caller controls it by id.
    pub fn play_direct(&mut self, clip: &str, volume: f32, pitch: f32) -> Option<VoiceId> {
        let params = VoiceParams {
            volume: if self.muted {
                0.0
            } else {
                volume.clamp(0.0, 1.0)
            },
            pitch,
            ..Default::default()
        };
        let id = self.start_voice(clip, params, None)?;
        self.clips.push(id);
        Some(id)
    }

    pub fn voice_pause(&mut self, id: VoiceId) {
        if let Some(b) = self.backend.as_mut() {
            b.pause(id);
        }
    }

    pub fn voice_resume(&mut self, id: VoiceId) {
        if let Some(b) = self.backend.as_mut() {
            b.resume(id);
        }
    }

    pub fn voice_stop(&mut self, id: VoiceId) {
        if let Some(b) = self.backend.as_mut() {
            b.stop(id);
        }
        self.clips.retain(|c| *c != id);
    }

    pub fn voice_seek(&mut self, id: VoiceId, seconds: f64) {
        if let Some(b) = self.backend.as_mut() {
            b.seek(id, seconds);
        }
    }

    /// Playback position of a voice in seconds (0 when it's gone).
    pub fn voice_position(&self, id: VoiceId) -> f64 {
        self.backend.as_ref().map_or(0.0, |b| b.position(id))
    }

    pub fn voice_alive(&self, id: VoiceId) -> bool {
        self.backend.as_ref().is_some_and(|b| b.is_alive(id))
    }

    /// The Play() voice of source `e`, if it's playing.
    pub fn source_voice(&self, e: Entity) -> Option<VoiceId> {
        self.sources.get(&e).map(|v| v.id)
    }

    /// Set a direct voice's volume and speed.
    pub fn voice_set(&mut self, id: VoiceId, volume: f32, pitch: f32) {
        let muted = self.muted;
        if let Some(b) = self.backend.as_mut() {
            b.set(
                id,
                VoiceParams {
                    volume: if muted { 0.0 } else { volume.clamp(0.0, 1.0) },
                    pitch,
                    ..Default::default()
                },
            );
        }
    }
}
