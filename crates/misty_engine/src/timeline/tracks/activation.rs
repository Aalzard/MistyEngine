//! Activation tracks: the bound object is active while a clip covers the playhead and
//! inactive in the gaps; when the director stops, Post-playback State decides what stays.

use hecs::Entity;

use crate::scene::Scene;
use crate::timeline::asset::{PostPlayback, Track};
use crate::timeline::evaluate::Baseline;

/// Whether a clip covers `t` (eases don't apply: the object is either on or off).
pub fn is_active(track: &Track, t: f32) -> bool {
    track.clips.iter().any(|c| t >= c.start && t < c.end())
}

pub fn apply(scene: &mut Scene, track: &Track, e: Entity, t: f32, base: &mut Baseline) {
    let was = scene.game_object(e).active;
    base.active.entry(e).or_insert(was);
    scene.game_object_mut(e).active = is_active(track, t);
}

/// Post-playback State, applied when the director stops.
pub fn finish(scene: &mut Scene, track: &Track, e: Entity, base: &Baseline) {
    if !scene.contains(e) {
        return;
    }
    let on = match track.post_playback {
        PostPlayback::Active => true,
        PostPlayback::Inactive => false,
        PostPlayback::Revert => match base.active.get(&e) {
            Some(v) => *v,
            None => return,
        },
        PostPlayback::LeaveAsIs => return,
    };
    scene.game_object_mut(e).active = on;
}
