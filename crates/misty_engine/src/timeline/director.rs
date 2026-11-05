//! Play-mode playback of Playable Directors: time, wrap modes, pausing, signals, audio voices
//! and the `stopped` event.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hecs::Entity;

use super::asset::{TimelineAsset, TrackKind};
use super::evaluate::{self, Baseline};
use super::tracks;
use crate::animation::AnimationClip;
use crate::audio::Audio;
use crate::scene::{AudioSource, DirectorUpdateMode, DirectorWrapMode, PlayableDirector, Scene};

/// `PlayableDirector.state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl PlayState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
        }
    }
}

/// Something a director did this frame that scripts hear about.
#[derive(Clone, Debug, PartialEq)]
pub enum DirectorEvent {
    /// A marker was passed: call `method` on the scripts of `target`.
    Signal {
        target: Entity,
        method: String,
        parameter: Option<String>,
    },
    /// The director stopped (reached the end with Wrap Mode None, or `Stop()`).
    Stopped(Entity),
}

/// Live state of one director.
#[derive(Default)]
struct Instance {
    asset: Option<Arc<TimelineAsset>>,
    time: f64,
    state: PlayState,
    /// Just started: the first update evaluates the start time without advancing.
    fresh: bool,
    base: Baseline,
    /// Audio track id → (clip index, Audio Source) playing.
    voices: HashMap<String, (usize, Entity)>,
}

pub struct DirectorPlayer {
    root: PathBuf,
    assets: HashMap<String, Option<Arc<TimelineAsset>>>,
    clips: HashMap<String, Arc<AnimationClip>>,
    failed_clips: std::collections::HashSet<String>,
    instances: HashMap<Entity, Instance>,
    /// Events made between frames (`Stop()` from a script), handed out with the next update.
    pending: Vec<DirectorEvent>,
    logs: Vec<String>,
}

fn director(scene: &Scene, e: Entity) -> Option<PlayableDirector> {
    scene
        .world
        .get::<&PlayableDirector>(e)
        .ok()
        .map(|d| (*d).clone())
}

impl DirectorPlayer {
    /// Timelines and clips are read from `root` (the project folder) on first use.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            assets: HashMap::new(),
            clips: HashMap::new(),
            failed_clips: Default::default(),
            instances: HashMap::new(),
            pending: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// A loaded timeline (cached; a broken file warns once) with its animation clips.
    pub fn asset(&mut self, path: &str) -> Option<Arc<TimelineAsset>> {
        if !self.assets.contains_key(path) {
            let loaded = match TimelineAsset::load(&self.root.join(path)) {
                Ok(t) => Some(Arc::new(t)),
                Err(e) => {
                    self.logs.push(format!("Timeline failed to load: {e}"));
                    None
                }
            };
            if let Some(t) = &loaded {
                self.load_clips(t);
            }
            self.assets.insert(path.to_owned(), loaded);
        }
        self.assets.get(path).cloned().flatten()
    }

    fn load_clips(&mut self, t: &TimelineAsset) {
        let paths: Vec<String> = t
            .tracks
            .iter()
            .filter(|tr| tr.kind == TrackKind::Animation)
            .flat_map(|tr| tr.clips.iter().filter_map(|c| c.asset.clone()))
            .collect();
        for p in paths {
            if self.clips.contains_key(&p) || self.failed_clips.contains(&p) {
                continue;
            }
            match AnimationClip::load(&self.root.join(&p)) {
                Ok(c) => {
                    self.clips.insert(p, Arc::new(c));
                }
                Err(e) => {
                    self.logs
                        .push(format!("Timeline animation clip failed to load: {e}"));
                    self.failed_clips.insert(p);
                }
            }
        }
    }

    /// The director's asset, loaded.
    fn asset_of(&mut self, scene: &Scene, e: Entity) -> Option<Arc<TimelineAsset>> {
        let path = director(scene, e)?.playable?;
        self.asset(&path)
    }

    /// Start every enabled director with Play On Awake (below `under`, or in the whole scene).
    pub fn play_on_awake(&mut self, scene: &Scene, under: Option<Entity>) {
        let starters: Vec<Entity> = scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| under.is_none_or(|r| scene.is_ancestor(r, *e)))
            .filter(|e| {
                director(scene, *e)
                    .is_some_and(|d| d.enabled && d.play_on_awake && d.playable.is_some())
            })
            .collect();
        for e in starters {
            self.play(scene, e);
        }
    }

    /// `Play()`: start from the initial time, or carry on after `Pause()`.
    pub fn play(&mut self, scene: &Scene, e: Entity) -> bool {
        let Some(d) = director(scene, e) else {
            return false;
        };
        let Some(asset) = self.asset_of(scene, e) else {
            return false;
        };
        let inst = self.instances.entry(e).or_default();
        match inst.state {
            PlayState::Playing => {}
            PlayState::Paused => inst.state = PlayState::Playing,
            PlayState::Stopped => {
                inst.asset = Some(asset);
                inst.state = PlayState::Playing;
                inst.fresh = true;
                // `time` set while stopped is kept; otherwise start at Initial Time.
                if inst.time <= 0.0 {
                    inst.time = d.initial_time.max(0.0) as f64;
                }
            }
        }
        true
    }

    /// `Pause()`: stays on the current frame; sounds pause.
    pub fn pause(&mut self, audio: &mut Audio, e: Entity) {
        if let Some(inst) = self.instances.get_mut(&e)
            && inst.state == PlayState::Playing
        {
            inst.state = PlayState::Paused;
            for (_, src) in inst.voices.values() {
                audio.pause(*src);
            }
        }
    }

    /// `Resume()`: carry on after `Pause()`.
    pub fn resume(&mut self, audio: &mut Audio, e: Entity) {
        if let Some(inst) = self.instances.get_mut(&e)
            && inst.state == PlayState::Paused
        {
            inst.state = PlayState::Playing;
            for (_, src) in inst.voices.values() {
                audio.unpause(*src);
            }
        }
    }

    /// `Stop()`: sounds stop, Activation tracks apply their Post-playback State, time goes back
    /// to 0 and `stopped` fires (with the next update).
    pub fn stop(&mut self, scene: &mut Scene, audio: &mut Audio, e: Entity) {
        if let Some(ev) = self.finish(scene, audio, e) {
            self.pending.push(ev);
        }
    }

    fn finish(&mut self, scene: &mut Scene, audio: &mut Audio, e: Entity) -> Option<DirectorEvent> {
        let inst = self.instances.get_mut(&e)?;
        if inst.state == PlayState::Stopped {
            return None;
        }
        inst.state = PlayState::Stopped;
        inst.time = 0.0;
        for (_, (_, src)) in inst.voices.drain() {
            audio.stop(src);
        }
        if let (Some(asset), Some(d)) = (inst.asset.clone(), director(scene, e)) {
            for tr in &asset.tracks {
                if tr.kind == TrackKind::Activation
                    && !asset.is_muted(tr)
                    && let Some(obj) = d.binding(&tr.id)
                {
                    tracks::activation::finish(scene, tr, obj, &inst.base);
                }
            }
        }
        inst.base = Baseline::default();
        Some(DirectorEvent::Stopped(e))
    }

    pub fn state(&self, e: Entity) -> PlayState {
        self.instances
            .get(&e)
            .map_or(PlayState::Stopped, |i| i.state)
    }

    /// `time`: seconds into the timeline.
    pub fn time(&self, e: Entity) -> f64 {
        self.instances.get(&e).map_or(0.0, |i| i.time)
    }

    /// Set `time` (playback carries on from there; sounds restart at the new place).
    pub fn set_time(&mut self, e: Entity, t: f64) {
        let inst = self.instances.entry(e).or_default();
        inst.time = t.max(0.0);
    }

    /// `duration`: the timeline's length in seconds.
    pub fn duration(&mut self, scene: &Scene, e: Entity) -> f64 {
        self.asset_of(scene, e).map_or(0.0, |a| a.duration() as f64)
    }

    /// `Evaluate()`: pose the scene at the current time now (also while stopped or paused).
    pub fn evaluate(&mut self, scene: &mut Scene, e: Entity) {
        let Some(asset) = self.asset_of(scene, e) else {
            return;
        };
        let Some(d) = director(scene, e) else { return };
        let inst = self.instances.entry(e).or_default();
        let t = inst.time as f32;
        let bind = |id: &str| d.binding(id);
        evaluate::apply(scene, &asset, &bind, &self.clips, t, &mut inst.base);
    }

    /// Advance every playing director by its clock, pose the scene, keep audio in step and
    /// collect the signals and stops of this frame.
    pub fn update(
        &mut self,
        scene: &mut Scene,
        audio: &mut Audio,
        dt: f32,
        unscaled_dt: f32,
    ) -> Vec<DirectorEvent> {
        let mut events = std::mem::take(&mut self.pending);
        let gone: Vec<Entity> = self
            .instances
            .keys()
            .copied()
            .filter(|e| !scene.contains(*e))
            .collect();
        for e in gone {
            if let Some(inst) = self.instances.remove(&e) {
                for (_, src) in inst.voices.into_values() {
                    audio.stop(src);
                }
            }
        }
        let playing: Vec<Entity> = self
            .instances
            .iter()
            .filter(|(_, i)| i.state == PlayState::Playing)
            .map(|(e, _)| *e)
            .collect();
        for e in playing {
            let Some(d) = director(scene, e) else {
                continue;
            };
            if !d.enabled || !scene.active_in_hierarchy(e) {
                continue;
            }
            let inst = self.instances.get_mut(&e).expect("listed");
            let Some(asset) = inst.asset.clone() else {
                continue;
            };
            let step = match d.update_method {
                DirectorUpdateMode::GameTime => dt,
                DirectorUpdateMode::UnscaledGameTime => unscaled_dt,
                DirectorUpdateMode::Manual => 0.0,
            } as f64;
            let fresh = std::mem::take(&mut inst.fresh);
            let prev = inst.time;
            if !fresh {
                inst.time += step;
            }
            let duration = asset.duration() as f64;
            // Spans of time passed this frame, for markers (two when looping wraps).
            let mut spans: Vec<(f32, f32)> = Vec::new();
            let start = if fresh { prev - 1e-6 } else { prev };
            let mut ended = false;
            match d.wrap_mode {
                DirectorWrapMode::Loop if duration > 0.0 && inst.time >= duration => {
                    spans.push((start as f32, duration as f32));
                    inst.time = inst.time.rem_euclid(duration);
                    spans.push((-1e-6, inst.time as f32));
                }
                DirectorWrapMode::Hold => {
                    inst.time = inst.time.min(duration);
                    spans.push((start as f32, inst.time as f32));
                }
                DirectorWrapMode::None if inst.time >= duration => {
                    spans.push((start as f32, duration as f32));
                    ended = true;
                }
                _ => spans.push((start as f32, inst.time as f32)),
            }
            let t = if ended { duration } else { inst.time } as f32;

            // Markers on the timeline's row go to the director's own object.
            let mut fire = |markers: &[super::asset::Marker], target: Entity| {
                for (i, &(a, b)) in spans.iter().enumerate() {
                    for m in markers {
                        if tracks::signal::fires(m, a, b, fresh && i == 0) && !m.method.is_empty() {
                            events.push(DirectorEvent::Signal {
                                target,
                                method: m.method.clone(),
                                parameter: m.parameter.clone(),
                            });
                        }
                    }
                }
            };
            fire(&asset.markers, e);
            for tr in &asset.tracks {
                if tr.kind == TrackKind::Signal && !asset.is_muted(tr) {
                    fire(&tr.markers, d.binding(&tr.id).unwrap_or(e));
                }
            }

            let bind = |id: &str| d.binding(id);
            evaluate::apply(scene, &asset, &bind, &self.clips, t, &mut inst.base);
            sync_audio(
                scene,
                audio,
                &asset,
                &d,
                inst,
                evaluate::sample_time(t, duration as f32),
            );
            if ended && let Some(ev) = self.finish(scene, audio, e) {
                events.push(ev);
            }
        }
        events
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

/// Start, switch and stop the Audio track voices for time `t`.
fn sync_audio(
    scene: &mut Scene,
    audio: &mut Audio,
    asset: &TimelineAsset,
    d: &PlayableDirector,
    inst: &mut Instance,
    t: f32,
) {
    for tr in asset.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
        let src = d
            .binding(&tr.id)
            .filter(|e| scene.world.get::<&AudioSource>(*e).is_ok());
        let want = src
            .filter(|_| !asset.is_muted(tr))
            .and_then(|_| tracks::audio::current(tr, t));
        let have = inst.voices.get(&tr.id).copied();
        if have.map(|h| h.0) == want.map(|w| w.0) && have.map(|h| h.1) == src {
            continue;
        }
        if let Some((_, old)) = inst.voices.remove(&tr.id) {
            audio.stop(old);
        }
        let (Some((i, offset)), Some(src)) = (want, src) else {
            continue;
        };
        let Some(clip) = tr.clips[i].asset.clone() else {
            continue;
        };
        // The source plays the timeline's clip through its own settings and mixer output.
        let own = scene
            .world
            .get::<&mut AudioSource>(src)
            .map(|mut s| s.clip.replace(clip))
            .ok()
            .flatten();
        let started = audio.play(scene, src);
        if let Ok(mut s) = scene.world.get::<&mut AudioSource>(src) {
            s.clip = own;
        }
        if started {
            if offset > 0.01 {
                audio.set_time(src, offset);
            }
            inst.voices.insert(tr.id.clone(), (i, src));
        }
    }
}
