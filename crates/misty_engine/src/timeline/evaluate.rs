//! Evaluating a timeline at a time: clip weights (ease in/out, blends where clips overlap),
//! clip-local times, and posing the bound scene objects. Shared by Play mode
//! ([`super::DirectorPlayer`]) and the Timeline window's edit-mode preview.

use std::collections::HashMap;
use std::sync::Arc;

use hecs::Entity;

use super::asset::{Extrapolation, TimelineAsset, TimelineClip, TrackKind};
use super::tracks;
use crate::animation::{AnimationClip, Property};
use crate::scene::{Camera, PlayableDirector, Scene, Transform};
use crate::vcam::CameraOverride;

/// Ease curve shared by eases and blends: `s(x) + s(1 - x) = 1`, so the two clips of a blend
/// always add up to full weight.
pub fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Seconds clip `i` overlaps an earlier clip at its start (0 = no blend in).
pub fn blend_in(clips: &[TimelineClip], i: usize) -> f32 {
    let c = &clips[i];
    clips
        .iter()
        .enumerate()
        .filter(|(j, o)| (*j != i && o.start < c.start) || (*j < i && o.start == c.start))
        .map(|(_, o)| (o.end() - c.start).clamp(0.0, c.duration))
        .fold(0.0, f32::max)
}

/// Seconds a later clip overlaps the end of clip `i` (0 = no blend out).
pub fn blend_out(clips: &[TimelineClip], i: usize) -> f32 {
    let c = &clips[i];
    clips
        .iter()
        .enumerate()
        .filter(|(j, o)| (*j != i && o.start > c.start) || (*j > i && o.start == c.start))
        .filter(|(_, o)| o.start < c.end())
        .map(|(_, o)| (c.end() - o.start).clamp(0.0, c.duration))
        .fold(0.0, f32::max)
}

/// Fade-in and fade-out lengths: the blend with a neighbour, or the clip's own ease.
pub fn fades(clips: &[TimelineClip], i: usize) -> (f32, f32) {
    let c = &clips[i];
    let bi = blend_in(clips, i);
    let bo = blend_out(clips, i);
    (
        if bi > 0.0 { bi } else { c.ease_in },
        if bo > 0.0 { bo } else { c.ease_out },
    )
}

/// One clip's part of a track at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Index into the track's clips.
    pub clip: usize,
    pub weight: f32,
    /// Seconds into the source asset (not wrapped by its length).
    pub local: f32,
}

/// Seconds into clip `c`'s source asset at timeline time `t`.
pub fn local_time(c: &TimelineClip, t: f32) -> f32 {
    c.clip_in + (t - c.start) * c.speed
}

/// Weight of clip `i` at `t` inside its own range (ignores extrapolation).
pub fn weight(clips: &[TimelineClip], i: usize, t: f32) -> f32 {
    let c = &clips[i];
    if t < c.start || t >= c.end() {
        return 0.0;
    }
    let (fin, fout) = fades(clips, i);
    let mut w = 1.0;
    if fin > 0.0 {
        w *= smooth((t - c.start) / fin);
    }
    if fout > 0.0 {
        w *= smooth((c.end() - t) / fout);
    }
    w
}

/// The clips playing at `t` with their weights (summing to at most 1). A clip with Hold
/// extrapolation keeps its last frame until the next clip starts.
pub fn samples(clips: &[TimelineClip], t: f32) -> Vec<Sample> {
    let mut out: Vec<Sample> = (0..clips.len())
        .filter_map(|i| {
            let w = weight(clips, i, t);
            (w > 0.0).then(|| Sample {
                clip: i,
                weight: w,
                local: local_time(&clips[i], t),
            })
        })
        .collect();
    if out.is_empty() {
        // In a gap: the latest clip that ended before `t` and holds.
        let held = clips
            .iter()
            .enumerate()
            .filter(|(_, c)| c.end() <= t && c.post_extrapolation == Extrapolation::Hold)
            .max_by(|a, b| a.1.end().total_cmp(&b.1.end()));
        if let Some((i, c)) = held {
            let next_starts_before = clips.iter().any(|o| o.start >= c.end() && o.start <= t);
            if !next_starts_before {
                out.push(Sample {
                    clip: i,
                    weight: 1.0,
                    local: local_time(c, c.end()),
                });
            }
        }
    }
    let total: f32 = out.iter().map(|s| s.weight).sum();
    if total > 1.0 {
        for s in &mut out {
            s.weight /= total;
        }
    }
    out
}

/// The time to evaluate for playback time `t`: at (or past) the end, just inside the last
/// frame, so clips that end with the timeline still show.
pub fn sample_time(t: f32, duration: f32) -> f32 {
    if duration > 0.0 && t >= duration {
        (duration - 1e-4).max(0.0)
    } else {
        t.max(0.0)
    }
}

/// Animation clips (and timelines, for sub-timelines) by path, for evaluation.
pub trait ClipSource {
    fn clip(&self, path: &str) -> Option<&AnimationClip>;

    /// A timeline asset already loaded (Control clips play sub-timelines from it).
    fn timeline(&self, _path: &str) -> Option<&TimelineAsset> {
        None
    }
}

impl ClipSource for HashMap<String, Arc<AnimationClip>> {
    fn clip(&self, path: &str) -> Option<&AnimationClip> {
        self.get(path).map(|c| c.as_ref())
    }
}

/// Where a timeline's tracks and clips find their scene objects: a Playable Director's
/// bindings (tracks) and references (Camera / Control clips).
pub trait Resolver {
    /// The object track `id` is bound to.
    fn binding(&self, id: &str) -> Option<Entity>;
    /// The object a clip's reference `name` points at.
    fn reference(&self, _name: &str) -> Option<Entity> {
        None
    }
}

impl Resolver for PlayableDirector {
    fn binding(&self, id: &str) -> Option<Entity> {
        PlayableDirector::binding(self, id)
    }

    fn reference(&self, name: &str) -> Option<Entity> {
        PlayableDirector::reference(self, name)
    }
}

/// Track bindings from a function (no clip references).
pub struct Bindings<'a>(pub &'a dyn Fn(&str) -> Option<Entity>);

impl Resolver for Bindings<'_> {
    fn binding(&self, id: &str) -> Option<Entity> {
        (self.0)(id)
    }
}

/// Values the scene had before the timeline first touched them. Animation blends ease from
/// them, and they are written back when an edit-mode preview ends.
#[derive(Clone, Debug, Default)]
pub struct Baseline {
    pub values: HashMap<(Entity, Property), f32>,
    pub active: HashMap<Entity, bool>,
    /// Edit-mode Camera track previews: each brain camera's transform and lens before.
    pub cameras: HashMap<Entity, (Transform, Option<Camera>)>,
}

impl Baseline {
    /// Put every touched value back.
    pub fn restore(&self, scene: &mut Scene) {
        for (&(e, p), &v) in &self.values {
            crate::animation::binding::write(scene, e, p, v);
        }
        for (&e, &on) in &self.active {
            if scene.contains(e) {
                scene.game_object_mut(e).active = on;
            }
        }
        for (&e, (t, cam)) in &self.cameras {
            tracks::camera::put_back(scene, e, t, cam.as_ref());
        }
    }
}

/// Sub-timelines nest at most this deep (a timeline that plays itself stops there).
pub const MAX_DEPTH: usize = 8;

/// Pose the scene as the timeline is at `t`: Activation tracks switch their object on or off,
/// Animation tracks blend their clips, Control tracks activate their objects and pose their
/// sub-timelines. Camera tracks don't move anything here: they return the brain overrides
/// for this time (Play mode hands them to the camera rig; the editor's preview poses the
/// brain itself with [`tracks::camera::preview`]). Audio and Signal tracks are left to the
/// director (they only act while playing). Muted tracks do nothing.
pub fn apply(
    scene: &mut Scene,
    asset: &TimelineAsset,
    resolver: &dyn Resolver,
    clips: &dyn ClipSource,
    t: f32,
    base: &mut Baseline,
) -> Vec<CameraOverride> {
    let mut shots = Vec::new();
    apply_nested(scene, asset, resolver, clips, t, base, 0, &mut shots);
    shots
}

/// [`apply`] for a timeline nested `depth` levels deep, adding camera overrides to `shots`.
#[allow(clippy::too_many_arguments)]
pub fn apply_nested(
    scene: &mut Scene,
    asset: &TimelineAsset,
    resolver: &dyn Resolver,
    clips: &dyn ClipSource,
    t: f32,
    base: &mut Baseline,
    depth: usize,
    shots: &mut Vec<CameraOverride>,
) {
    let t = sample_time(t, asset.duration());
    for track in &asset.tracks {
        if asset.is_muted(track) {
            continue;
        }
        if track.kind == TrackKind::Control {
            tracks::control::apply(scene, track, resolver, clips, t, base, depth, shots);
            continue;
        }
        let Some(e) = resolver.binding(&track.id).filter(|e| scene.contains(*e)) else {
            continue;
        };
        match track.kind {
            TrackKind::Activation => tracks::activation::apply(scene, track, e, t, base),
            TrackKind::Animation => tracks::animation::apply(scene, track, e, clips, t, base),
            TrackKind::Camera => shots.extend(tracks::camera::shot(track, resolver, e, t)),
            TrackKind::Audio | TrackKind::Signal | TrackKind::Group | TrackKind::Control => {}
        }
    }
}
