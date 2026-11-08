//! Record mode: arm an Animation track (the ⏺ button on its header) and every change made to
//! its bound object or the objects under it — in the Scene view, the Inspector or anywhere
//! else — becomes a key at the playhead in the track's recorded keys. Runs inside the preview:
//! after the timeline poses the scene each frame we remember the recordable values, and the
//! next frame anything that differs was changed by hand and is keyed. Like the rest of the
//! preview, leaving it puts the scene back; the change lives on as keys.

use std::collections::HashMap;

use misty_engine::animation::{AnimationClip, Property, binding};
use misty_engine::hecs::Entity;
use misty_engine::timeline::TrackKind;

use crate::state::EditorState;
use crate::timelines;

/// Arm `track` for recording (or disarm with `None`). Recording turns the preview on.
pub fn arm(s: &mut EditorState, track: Option<String>) {
    let w = &mut s.timeline;
    w.recording = track;
    w.rec_snapshot.clear();
    if w.recording.is_some() {
        w.preview = true;
        w.playing = false;
    }
}

/// Whether `track` can record: an Animation track without clips (its keys go to the track's
/// own recorded clip), bound to an object, not locked.
pub fn can_record(s: &mut EditorState, path: &str, track: &str) -> bool {
    let Some((Some(d), _)) = super::current(s) else {
        return false;
    };
    let bound = super::director(s, d)
        .and_then(|c| c.binding(track))
        .is_some_and(|e| s.scene.contains(e));
    let Ok(t) = s.anim_clips.timelines.get(path) else {
        return false;
    };
    t.track(track).is_some_and(|tr| {
        tr.kind == TrackKind::Animation && tr.clips.is_empty() && !t.is_locked(tr)
    }) && bound
}

/// Every recordable value of `root` and the objects under it: (object, property) → (curve
/// path, value).
fn values(s: &EditorState, root: Entity) -> HashMap<(Entity, Property), (String, f32)> {
    let mut out = HashMap::new();
    for (e, _) in s.scene.walk() {
        if !s.scene.is_ancestor(root, e) {
            continue;
        }
        let Some(path) = binding::path_of(&s.scene, root, e) else {
            continue;
        };
        for p in Property::ALL {
            if binding::has(&s.scene, e, p)
                && let Some(v) = binding::read(&s.scene, e, p)
            {
                out.insert((e, p), (path.clone(), v));
            }
        }
    }
    out
}

/// The armed track's bound object, while recording is possible.
fn armed(s: &mut EditorState, path: &str) -> Option<(String, Entity)> {
    let id = s.timeline.recording.clone()?;
    if !can_record(s, path, &id) {
        s.timeline.recording = None;
        return None;
    }
    let (Some(d), _) = super::current(s)? else {
        return None;
    };
    let root = super::director(s, d)?.binding(&id)?;
    Some((id, root))
}

/// Before the timeline poses the scene: key what changed by hand since last frame.
pub fn capture(s: &mut EditorState, path: &str) {
    let Some((id, root)) = armed(s, path) else {
        return;
    };
    if s.timeline.rec_snapshot.is_empty() {
        return;
    }
    let now = values(s, root);
    let changed: Vec<((Entity, Property), String, f32, f32)> = now
        .iter()
        .filter_map(|(k, (p, v))| {
            let (_, old) = s.timeline.rec_snapshot.get(k)?;
            ((v - old).abs() > 1e-5).then(|| (*k, p.clone(), *old, *v))
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let time = s.timeline.time;
    // The preview puts back what the object had before (not the hand-made change).
    for (k, _, old, _) in &changed {
        s.timeline.base.values.entry(*k).or_insert(*old);
    }
    timelines::edit(s, path, "Record Key", true, |t| {
        let Some(tr) = t.track_mut(&id) else { return };
        let clip = tr.recorded.get_or_insert_with(|| AnimationClip {
            looping: false,
            ..Default::default()
        });
        for ((_, prop), curve, _, v) in &changed {
            clip.set_key(curve, *prop, time, *v);
        }
    });
}

/// After the timeline posed the scene: remember the values to spot changes next frame.
pub fn snapshot(s: &mut EditorState, path: &str) {
    let Some((_, root)) = armed(s, path) else {
        s.timeline.rec_snapshot.clear();
        return;
    };
    s.timeline.rec_snapshot = values(s, root);
}
