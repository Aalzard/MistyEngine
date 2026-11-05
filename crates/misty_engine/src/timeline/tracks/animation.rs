//! Animation tracks: `.anim` clips played on the bound object (curve paths are relative to
//! it). Overlapping clips cross-fade; eases fade from the values the object had before the
//! timeline touched it, and gaps return to those values.

use std::collections::BTreeMap;

use hecs::Entity;

use crate::animation::{AnimationClip, Property, binding};
use crate::scene::Scene;
use crate::timeline::asset::Track;
use crate::timeline::evaluate::{Baseline, ClipSource, samples};

/// Seconds into `clip` for an unwrapped local time: looping clips wrap, others hold their ends.
pub fn wrap(clip: &AnimationClip, local: f32) -> f32 {
    if clip.length <= 0.0 {
        0.0
    } else if clip.looping {
        local.rem_euclid(clip.length)
    } else {
        local.clamp(0.0, clip.length)
    }
}

/// Weighted sum of one property over the playing clips.
#[derive(Clone, Copy, Default)]
struct Mix {
    sum: f32,
    weight: f32,
    /// Discrete properties take the strongest clip's value.
    best: (f32, f32),
}

/// Every property the track's clips animate, with this time's blend of them.
fn mix(track: &Track, clips: &dyn ClipSource, t: f32) -> BTreeMap<(String, Property), Mix> {
    let mut out: BTreeMap<(String, Property), Mix> = BTreeMap::new();
    let all = track
        .clips
        .iter()
        .filter_map(|c| clips.clip(c.asset.as_deref()?));
    for anim in all.chain(track.recorded.as_ref()) {
        for c in anim.curves.iter().filter(|c| !c.keys.is_empty()) {
            out.entry((c.path.clone(), c.property)).or_default();
        }
    }
    let mut add = |anim: &AnimationClip, local: f32, w: f32| {
        let lt = wrap(anim, local);
        for c in anim.curves.iter().filter(|c| !c.keys.is_empty()) {
            let v = c.evaluate(lt);
            let m = out.entry((c.path.clone(), c.property)).or_default();
            m.sum += w * v;
            m.weight += w;
            if w > m.best.0 {
                m.best = (w, v);
            }
        }
    };
    if track.clips.is_empty() {
        if let Some(rec) = &track.recorded {
            add(rec, t, 1.0);
        }
        return out;
    }
    for s in samples(&track.clips, t) {
        let c = &track.clips[s.clip];
        if let Some(anim) = c.asset.as_deref().and_then(|p| clips.clip(p)) {
            add(anim, s.local, s.weight);
        }
    }
    out
}

pub fn apply(
    scene: &mut Scene,
    track: &Track,
    root: Entity,
    clips: &dyn ClipSource,
    t: f32,
    base: &mut Baseline,
) {
    for ((path, p), m) in mix(track, clips, t) {
        let Some(e) = binding::resolve(scene, root, &path) else {
            continue;
        };
        if !binding::has(scene, e, p) {
            continue;
        }
        let b = *base
            .values
            .entry((e, p))
            .or_insert_with(|| binding::read(scene, e, p).unwrap_or(0.0));
        let v = if p.is_discrete() {
            if m.best.0 >= 0.5 { m.best.1 } else { b }
        } else {
            b * (1.0 - m.weight.min(1.0)) + m.sum
        };
        binding::write(scene, e, p, v);
    }
}
