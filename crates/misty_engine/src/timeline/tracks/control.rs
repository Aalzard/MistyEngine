//! Control tracks: each clip controls the scene object its reference names. While the clip
//! plays the object is active (Activation) and its Playable Director's timeline is posed at
//! the clip's local time (a sub-timeline: `clip_in + (t − start) × speed`, looping when that
//! director loops, else holding its last frame). In Play mode the director also plays the
//! object's particles and audio and fires its Impulse Source as clips start and end (see
//! [`crate::timeline::DirectorPlayer`]).

use std::collections::BTreeMap;

use hecs::Entity;

use crate::scene::{DirectorWrapMode, PlayableDirector, Scene};
use crate::timeline::asset::{PostPlayback, TimelineClip, Track};
use crate::timeline::evaluate::{
    Baseline, ClipSource, MAX_DEPTH, Resolver, apply_nested, local_time,
};
use crate::vcam::CameraOverride;

/// Whether clip `c` covers `t` (start inclusive, end exclusive).
pub fn covers(c: &TimelineClip, t: f32) -> bool {
    t >= c.start && t < c.end()
}

/// Seconds into a sub-timeline of length `duration` at parent time `t`.
pub fn sub_time(c: &TimelineClip, t: f32, duration: f32, looping: bool) -> f32 {
    let local = local_time(c, t).max(0.0);
    if duration <= 0.0 {
        0.0
    } else if looping {
        local.rem_euclid(duration)
    } else {
        local.min(duration)
    }
}

/// The object clip `c` controls.
pub fn object(c: &TimelineClip, resolver: &dyn Resolver, scene: &Scene) -> Option<Entity> {
    c.reference
        .as_deref()
        .and_then(|r| resolver.reference(r))
        .filter(|e| scene.contains(*e))
}

/// The director of `e` and its timeline path, when it has one to play.
pub fn sub_director(scene: &Scene, e: Entity) -> Option<(PlayableDirector, String)> {
    let d = scene
        .world
        .get::<&PlayableDirector>(e)
        .ok()
        .map(|d| (*d).clone())?;
    let path = d.playable.clone()?;
    Some((d, path))
}

#[allow(clippy::too_many_arguments)]
pub fn apply(
    scene: &mut Scene,
    track: &Track,
    resolver: &dyn Resolver,
    clips: &dyn ClipSource,
    t: f32,
    base: &mut Baseline,
    depth: usize,
    shots: &mut Vec<CameraOverride>,
) {
    // Activation: on while any of its clips covers `t`.
    let mut active: BTreeMap<Entity, bool> = BTreeMap::new();
    for c in track.clips.iter().filter(|c| c.control.activation) {
        if let Some(e) = object(c, resolver, scene) {
            *active.entry(e).or_default() |= covers(c, t);
        }
    }
    for (e, on) in active {
        let was = scene.game_object(e).active;
        base.active.entry(e).or_insert(was);
        scene.game_object_mut(e).active = on;
    }
    if depth + 1 >= MAX_DEPTH {
        return;
    }
    for c in track
        .clips
        .iter()
        .filter(|c| c.control.sub_timeline && covers(c, t))
    {
        let Some(e) = object(c, resolver, scene) else {
            continue;
        };
        let Some((d, path)) = sub_director(scene, e) else {
            continue;
        };
        let Some(nested) = clips.timeline(&path) else {
            continue;
        };
        let local = sub_time(
            c,
            t,
            nested.duration(),
            d.wrap_mode == DirectorWrapMode::Loop,
        );
        apply_nested(scene, nested, &d, clips, local, base, depth + 1, shots);
    }
}

/// Post-playback State of each clip's object when the director stops.
pub fn finish(scene: &mut Scene, track: &Track, resolver: &dyn Resolver, base: &Baseline) {
    for c in track.clips.iter().filter(|c| c.control.activation) {
        let Some(e) = object(c, resolver, scene) else {
            continue;
        };
        let on = match c.control.post_playback {
            PostPlayback::Active => true,
            PostPlayback::Inactive => false,
            PostPlayback::Revert => match base.active.get(&e) {
                Some(v) => *v,
                None => continue,
            },
            PostPlayback::LeaveAsIs => continue,
        };
        scene.game_object_mut(e).active = on;
    }
}
