//! Play-mode playback of Animation components: clip time per object, wrap modes and events.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hecs::Entity;

use super::binding;
use super::clip::AnimationClip;
use crate::scene::{Animation, Scene, WrapMode};

/// Playback state of one Animation component.
#[derive(Clone, Debug)]
struct State {
    /// Clip asset path; `None` until something is played.
    clip: Option<String>,
    /// Seconds since Play (keeps growing while looping, like `AnimationState.time`).
    time: f32,
    speed: f32,
    playing: bool,
    /// Just started: the first update samples time 0 without advancing.
    fresh: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            clip: None,
            time: 0.0,
            speed: 1.0,
            playing: false,
            fresh: false,
        }
    }
}

/// Where playback is inside the clip for an (unwrapped) time, and whether it has ended.
pub fn wrap_time(time: f32, length: f32, mode: WrapMode) -> (f32, bool) {
    if length <= 0.0 {
        return (0.0, mode == WrapMode::Once);
    }
    match mode {
        WrapMode::Loop => (time.rem_euclid(length), false),
        WrapMode::PingPong => {
            let m = time.rem_euclid(2.0 * length);
            (if m > length { 2.0 * length - m } else { m }, false)
        }
        WrapMode::ClampForever => (time.clamp(0.0, length), false),
        WrapMode::Once | WrapMode::Default => {
            let ended = time >= length || time < 0.0;
            (time.clamp(0.0, length), ended)
        }
    }
}

/// The wrap mode actually used: `Default` follows the clip's Loop Time flag.
pub fn effective_wrap(mode: WrapMode, clip: &AnimationClip) -> WrapMode {
    match mode {
        WrapMode::Default if clip.looping => WrapMode::Loop,
        WrapMode::Default => WrapMode::Once,
        m => m,
    }
}

/// True if playback moving from `prev` to `cur` passes `event` (once per frame at most).
pub(super) fn crossed(prev: f32, cur: f32, event: f32, length: f32, looping: bool) -> bool {
    if cur <= prev {
        return false;
    }
    if looping && length > 0.0 {
        ((cur - event) / length).floor() > ((prev - event) / length).floor()
    } else {
        prev < event && event <= cur
    }
}

pub struct AnimationPlayer {
    root: PathBuf,
    clips: HashMap<String, Option<Arc<AnimationClip>>>,
    states: HashMap<Entity, State>,
    logs: Vec<String>,
}

impl AnimationPlayer {
    /// Clips are read from `root` (the project folder) on first use.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            clips: HashMap::new(),
            states: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// A loaded clip (cached; a missing or broken file logs one warning).
    pub fn clip(&mut self, path: &str) -> Option<Arc<AnimationClip>> {
        if !self.clips.contains_key(path) {
            let loaded = match AnimationClip::load(&self.root.join(path)) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    self.logs
                        .push(format!("Animation clip failed to load: {e}"));
                    None
                }
            };
            self.clips.insert(path.to_owned(), loaded);
        }
        self.clips.get(path).cloned().flatten()
    }

    /// Start every enabled Animation with Play Automatically (below `under`, or in the whole scene).
    pub fn play_automatically(&mut self, scene: &Scene, under: Option<Entity>) {
        let starters: Vec<Entity> = scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| under.is_none_or(|r| scene.is_ancestor(r, *e)))
            .filter(|e| {
                scene
                    .world
                    .get::<&Animation>(*e)
                    .is_ok_and(|a| a.enabled && a.play_automatically && a.clip.is_some())
            })
            .collect();
        for e in starters {
            self.play(scene, e, None);
        }
    }

    /// `Play(name)`: start a clip from the beginning (`None` = the default clip).
    /// Returns false (with a warning) if the object has no such clip.
    pub fn play(&mut self, scene: &Scene, e: Entity, name: Option<&str>) -> bool {
        let path = scene
            .world
            .get::<&Animation>(e)
            .ok()
            .and_then(|a| match name {
                Some(n) => a.find_clip(n),
                None => a.clip.clone(),
            });
        let Some(path) = path.filter(|p| self.clip(p).is_some()) else {
            let what = name.unwrap_or("(default clip)");
            self.logs.push(format!(
                "The animation state {what} could not be played because it couldn't be found on '{}'.",
                if scene.contains(e) { scene.name(e) } else { "null".into() }
            ));
            return false;
        };
        let st = self.states.entry(e).or_default();
        *st = State {
            clip: Some(path),
            time: 0.0,
            speed: st.speed,
            playing: true,
            fresh: true,
        };
        true
    }

    /// Stop playing and rewind (the current pose stays).
    pub fn stop(&mut self, e: Entity) {
        if let Some(st) = self.states.get_mut(&e) {
            st.playing = false;
            st.time = 0.0;
        }
    }

    pub fn is_playing(&self, e: Entity) -> bool {
        self.states.get(&e).is_some_and(|s| s.playing)
    }

    /// Name of the clip playing (or last played).
    pub fn current_clip(&self, e: Entity) -> Option<String> {
        self.states
            .get(&e)
            .and_then(|s| s.clip.as_deref())
            .map(crate::scene::clip_name)
    }

    pub fn time(&self, e: Entity) -> f32 {
        self.states.get(&e).map_or(0.0, |s| s.time)
    }

    pub fn set_time(&mut self, e: Entity, t: f32) {
        self.states.entry(e).or_default().time = t;
    }

    pub fn speed(&self, e: Entity) -> f32 {
        self.states.get(&e).map_or(1.0, |s| s.speed)
    }

    pub fn set_speed(&mut self, e: Entity, v: f32) {
        self.states.entry(e).or_default().speed = v;
    }

    /// Advance every playing Animation by `dt` and pose its objects.
    /// Returns the animation events passed this frame as (object, function name).
    pub fn update(&mut self, scene: &mut Scene, dt: f32) -> Vec<(Entity, String)> {
        self.states.retain(|e, _| scene.contains(*e));
        let mut events = Vec::new();
        let playing: Vec<Entity> = self
            .states
            .iter()
            .filter(|(_, s)| s.playing)
            .map(|(e, _)| *e)
            .collect();
        for e in playing {
            let mode = match scene.world.get::<&Animation>(e) {
                Ok(a) if a.enabled && scene.active_in_hierarchy(e) => a.wrap_mode,
                _ => continue,
            };
            let Some(path) = self.states[&e].clip.clone() else {
                continue;
            };
            let Some(clip) = self.clip(&path) else {
                continue;
            };
            let st = self.states.get_mut(&e).expect("state exists");
            let prev = if st.fresh { -f32::EPSILON } else { st.time };
            if !st.fresh {
                st.time += dt * st.speed;
            }
            st.fresh = false;
            let mode = effective_wrap(mode, &clip);
            let (t, ended) = wrap_time(st.time, clip.length, mode);
            if ended {
                st.playing = false;
            }
            let looping = matches!(mode, WrapMode::Loop);
            for ev in &clip.events {
                if crossed(prev, st.time, ev.time, clip.length, looping) {
                    events.push((e, ev.function.clone()));
                }
            }
            binding::sample(scene, e, &clip, t);
        }
        events
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}
