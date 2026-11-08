//! Timeline edits with undo: tracks, clips and markers, plus the move / trim / snap math the
//! mouse drags use.

use misty_engine::hecs::Entity;
use misty_engine::scene::PlayableDirector;
use misty_engine::timeline::{Marker, TimelineAsset, TimelineClip, TrackKind};

use super::{TimelineSel, current};
use crate::state::EditorState;
use crate::timelines;

/// Snap distance in pixels for clip edges and the playhead.
pub const SNAP_PX: f32 = 8.0;
/// Default length of a new Activation clip.
const ACTIVATION_LEN: f32 = 2.0;

/// "Create": a new `.timeline` in Assets/Timelines named after the object and a Playable
/// Director on it that plays it.
pub fn create_for(s: &mut EditorState, e: Entity) -> Option<String> {
    let base = s.scene.name(e);
    let taken = |rel: &str| s.project.root.join(rel).exists();
    let rel = (0..)
        .map(|n| match n {
            0 => format!("Assets/Timelines/{base}Timeline.timeline"),
            n => format!("Assets/Timelines/{base}Timeline {n}.timeline"),
        })
        .find(|r| !taken(r))?;
    if let Err(err) = s
        .anim_clips
        .timelines
        .create(&rel, TimelineAsset::default())
    {
        s.console.warn(format!("Could not create timeline: {err}"));
        return None;
    }
    s.record("Create Timeline");
    if s.scene.world.get::<&PlayableDirector>(e).is_err() {
        s.scene.add(e, PlayableDirector::default());
    }
    if let Ok(mut d) = s.scene.world.get::<&mut PlayableDirector>(e) {
        d.playable = Some(rel.clone());
    }
    s.refresh_assets();
    s.timeline.director = Some(e);
    s.timeline.selected = None;
    s.console.info(format!("Created {rel}"));
    Some(rel)
}

/// Add a track (inside `group` if given) and select it. Returns its id.
pub fn add_track(
    s: &mut EditorState,
    path: &str,
    kind: TrackKind,
    group: Option<&str>,
) -> Option<String> {
    let mut id = None;
    let label = format!("Add {}", kind.label());
    timelines::edit(s, path, &label, false, |t| {
        id = Some(t.add_track(kind, group));
    });
    if let Some(id) = &id {
        s.timeline.selected = Some(TimelineSel::Track(id.clone()));
    }
    id
}

/// Remove a track (a group takes its tracks along); its binding goes too.
pub fn remove_track(s: &mut EditorState, path: &str, id: &str) {
    let gone: Vec<String> = s
        .anim_clips
        .timelines
        .get(path)
        .map(|t| {
            t.tracks
                .iter()
                .filter(|x| x.id == id || x.group.as_deref() == Some(id))
                .map(|x| x.id.clone())
                .collect()
        })
        .unwrap_or_default();
    timelines::edit(s, path, "Delete Track", false, |t| t.remove_track(id));
    if let Some((Some(d), _)) = current(s)
        && let Ok(mut c) = s.scene.world.get::<&mut PlayableDirector>(d)
    {
        c.bindings.retain(|b| !gone.contains(&b.track));
    }
    s.timeline.selected = None;
}

/// Bind track `id` of the window's director to `object` (with undo).
pub fn set_binding(s: &mut EditorState, director: Entity, id: &str, object: Option<Entity>) {
    s.record("Set Track Binding");
    if let Ok(mut d) = s.scene.world.get::<&mut PlayableDirector>(director) {
        d.set_binding(id, object);
    }
}

/// A clip of the track's kind at `time`: Activation clips last 2 s, Animation and Audio clips
/// the length of their asset.
pub fn new_clip(
    s: &mut EditorState,
    kind: TrackKind,
    asset: Option<&str>,
    time: f32,
) -> TimelineClip {
    let length = match (kind, asset) {
        (TrackKind::Animation, Some(a)) => s.anim_clips.get(a).map(|c| c.length).unwrap_or(1.0),
        (TrackKind::Audio, Some(a)) => {
            misty_engine::audio::wav::duration_of(&s.project.root.join(a)).unwrap_or(1.0)
        }
        _ => ACTIVATION_LEN,
    };
    TimelineClip {
        name: match asset {
            Some(a) => misty_engine::scene::clip_name(a),
            None => "Active".into(),
        },
        start: time.max(0.0),
        duration: length.max(0.05),
        asset: asset.map(str::to_owned),
        ..Default::default()
    }
}

/// Add a clip to track `id` (after its last clip when `time` is `None`) and select it.
pub fn add_clip(s: &mut EditorState, path: &str, id: &str, asset: Option<&str>, time: Option<f32>) {
    let Ok(t) = s.anim_clips.timelines.get(path) else {
        return;
    };
    let Some(track) = t.track(id) else { return };
    let kind = track.kind;
    if !kind.has_clips() || t.is_locked(track) {
        return;
    }
    let after = track.clips.iter().map(|c| c.end()).fold(0.0, f32::max);
    let mut clip = new_clip(s, kind, asset, time.unwrap_or(after));
    let rate = s
        .anim_clips
        .timelines
        .get(path)
        .map_or(60.0, |t| t.frame_rate);
    clip.start = (clip.start * rate).round() / rate;
    let mut index = None;
    timelines::edit(s, path, "Add Clip", false, |t| {
        if let Some(tr) = t.track_mut(id) {
            tr.clips.push(clip);
            index = Some(tr.clips.len() - 1);
        }
    });
    if let Some(i) = index {
        s.timeline.selected = Some(TimelineSel::Clip(id.to_owned(), i));
    }
}

/// Seconds a new Camera shot or Control clip lasts.
const REFERENCE_CLIP_LEN: f32 = 3.0;

/// Add a Camera shot / Control clip for `object` to track `id` (at `time`, else after the
/// last clip): the clip gets a new reference name and the window's director points it at
/// `object`. Selects the clip.
pub fn add_reference_clip(
    s: &mut EditorState,
    path: &str,
    id: &str,
    object: Entity,
    time: Option<f32>,
) {
    let Some((Some(d), _)) = current(s) else {
        return;
    };
    let Ok(t) = s.anim_clips.timelines.get(path) else {
        return;
    };
    let Some(track) = t.track(id) else { return };
    if !track.kind.clips_reference_objects() || t.is_locked(track) {
        return;
    }
    let after = track.clips.iter().map(|c| c.end()).fold(0.0, f32::max);
    let start = t.snap(time.unwrap_or(after).max(0.0));
    let reference = t.next_reference();
    let clip = TimelineClip {
        name: s.scene.name(object),
        start,
        duration: REFERENCE_CLIP_LEN,
        reference: Some(reference.clone()),
        ..Default::default()
    };
    set_reference(s, d, &reference, Some(object));
    let mut index = None;
    timelines::edit(s, path, "Add Clip", false, |t| {
        if let Some(tr) = t.track_mut(id) {
            tr.clips.push(clip);
            index = Some(tr.clips.len() - 1);
        }
    });
    if let Some(i) = index {
        s.timeline.selected = Some(TimelineSel::Clip(id.to_owned(), i));
    }
}

/// Point the director's clip reference `name` at `object` (with undo).
pub fn set_reference(s: &mut EditorState, director: Entity, name: &str, object: Option<Entity>) {
    s.record("Set Clip Object");
    if let Ok(mut d) = s.scene.world.get::<&mut PlayableDirector>(director) {
        d.set_reference(name, object);
    }
}

/// Objects a Control clip can control: ones with a Playable Director, particles, an Audio
/// Source or an Impulse Source first, then every other object.
pub fn controllable(s: &EditorState) -> Vec<(Entity, String)> {
    use misty_engine::scene::{AudioSource, ParticleSystem};
    let w = &s.scene.world;
    let special = |e: Entity| {
        w.get::<&PlayableDirector>(e).is_ok()
            || w.get::<&ParticleSystem>(e).is_ok()
            || w.get::<&AudioSource>(e).is_ok()
            || w.get::<&misty_engine::vcam::ImpulseSource>(e).is_ok()
    };
    let all: Vec<Entity> = s.scene.walk().into_iter().map(|(e, _)| e).collect();
    let (mut first, rest): (Vec<Entity>, Vec<Entity>) = all.into_iter().partition(|e| special(*e));
    first.extend(rest);
    first.into_iter().map(|e| (e, s.scene.name(e))).collect()
}

/// Ripple: shift every clip that started at or after `from` (in `base`, the clips before the
/// edit) by `delta`, except clip `skip`.
pub fn ripple(
    clips: &mut [TimelineClip],
    base: &[TimelineClip],
    skip: usize,
    from: f32,
    delta: f32,
) {
    for (i, (c, b)) in clips.iter_mut().zip(base).enumerate() {
        if i != skip && b.start >= from - 1e-4 {
            c.start = (b.start + delta).max(0.0);
        }
    }
}

/// Add a marker at `time` to a Signal track (`None` = the timeline's markers row).
pub fn add_marker(s: &mut EditorState, path: &str, track: Option<&str>, time: f32) {
    let mut index = None;
    timelines::edit(s, path, "Add Marker", false, |t| {
        let time = t.snap(time.max(0.0));
        let m = Marker {
            time,
            ..Default::default()
        };
        let list = match track {
            Some(id) => match t.track_mut(id) {
                Some(tr) => &mut tr.markers,
                None => return,
            },
            None => &mut t.markers,
        };
        list.push(m);
        index = Some(list.len() - 1);
    });
    if let Some(i) = index {
        s.timeline.selected = Some(TimelineSel::Marker(track.map(str::to_owned), i));
    }
}

/// Delete whatever is selected.
pub fn delete_selected(s: &mut EditorState, path: &str) {
    let Some(sel) = s.timeline.selected.clone() else {
        return;
    };
    match sel {
        TimelineSel::Track(id) => remove_track(s, path, &id),
        TimelineSel::Clip(id, i) => {
            let ripple = s.timeline.ripple;
            timelines::edit(s, path, "Delete Clip", false, |t| {
                if let Some(tr) = t.track_mut(&id)
                    && i < tr.clips.len()
                {
                    // Ripple closes the gap the clip leaves.
                    if ripple {
                        let base = tr.clips.clone();
                        let gone = &base[i];
                        self::ripple(&mut tr.clips, &base, i, gone.end(), -gone.duration);
                    }
                    tr.clips.remove(i);
                }
            });
        }
        TimelineSel::Marker(track, i) => {
            timelines::edit(s, path, "Delete Marker", false, |t| {
                let list = match &track {
                    Some(id) => t.track_mut(id).map(|tr| &mut tr.markers),
                    None => Some(&mut t.markers),
                };
                if let Some(list) = list
                    && i < list.len()
                {
                    list.remove(i);
                }
            });
        }
    }
    s.timeline.selected = None;
}

/// The nearest of `candidates` to `t` within `radius` seconds.
pub fn snap_to(t: f32, candidates: &[f32], radius: f32) -> Option<f32> {
    candidates
        .iter()
        .copied()
        .filter(|c| (c - t).abs() <= radius)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
}

/// Clip edges of the other clips on the track, 0 and the playhead: what moved edges snap to.
pub fn snap_points(asset: &TimelineAsset, track: &str, except: usize, playhead: f32) -> Vec<f32> {
    let mut v = vec![0.0, playhead];
    if let Some(tr) = asset.track(track) {
        for (i, c) in tr.clips.iter().enumerate() {
            if i != except {
                v.push(c.start);
                v.push(c.end());
            }
        }
    }
    v
}

/// Moved start for a clip of `duration` dragged to `start`: snapped to frames, then to a nearby
/// edge (either end of the clip may snap).
pub fn snapped_start(
    asset: &TimelineAsset,
    start: f32,
    duration: f32,
    points: &[f32],
    radius: f32,
    snap: bool,
) -> f32 {
    let mut t = start.max(0.0);
    if snap {
        t = asset.snap(t);
        let by_start = snap_to(t, points, radius).map(|p| p - t);
        let by_end = snap_to(t + duration, points, radius).map(|p| p - (t + duration));
        let best = [by_start, by_end]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.abs().total_cmp(&b.abs()));
        if let Some(d) = best {
            t += d;
        }
    }
    t.max(0.0)
}

/// Trim the start of `base` to `t`: the end stays, the source asset offset follows.
pub fn trim_start(base: &TimelineClip, t: f32, min_len: f32) -> TimelineClip {
    let mut c = base.clone();
    let mut t = t.min(base.end() - min_len).max(0.0);
    if base.asset.is_some() {
        // Not before the start of the source.
        let earliest = base.start - base.clip_in / base.speed.max(1e-3);
        t = t.max(earliest);
    }
    c.clip_in = (base.clip_in + (t - base.start) * base.speed).max(0.0);
    c.duration = base.end() - t;
    c.start = t;
    c.ease_in = c.ease_in.min(c.duration);
    c.ease_out = c.ease_out.min(c.duration);
    c
}

/// Trim the end of `c` to `t`.
pub fn trim_end(c: &mut TimelineClip, t: f32, min_len: f32) {
    c.duration = (t - c.start).max(min_len);
    c.ease_in = c.ease_in.min(c.duration);
    c.ease_out = c.ease_out.min(c.duration);
}
