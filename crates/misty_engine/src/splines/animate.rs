//! Spline Animate: moves its object along a spline while playing — by duration or speed, once,
//! looping or ping-ponging, with easing, optionally turned to face along the spline.

use glam::Vec3;
use hecs::Entity;

use super::components::{
    AnimateAlignment, AnimateMethod, LoopMode, SplineAnimate, SplineContainer, container_object,
};
use super::{SplinePath, look_rotation};
use crate::scene::Scene;

/// Seconds one pass takes.
pub fn duration(anim: &SplineAnimate, length: f32) -> f32 {
    match anim.method {
        AnimateMethod::Time => anim.duration.max(1e-3),
        AnimateMethod::Speed => (length / anim.speed.max(1e-3)).max(1e-3),
    }
}

/// Raw progress: passes completed plus the share of the current one (Start Offset included).
pub fn normalized_time(anim: &SplineAnimate, duration: f32) -> f32 {
    anim.elapsed / duration.max(1e-6) + anim.start_offset
}

/// Set the progress (moves `elapsed`).
pub fn set_normalized_time(anim: &mut SplineAnimate, value: f32, duration: f32) {
    anim.elapsed = (value - anim.start_offset) * duration;
}

/// Spline t for a raw progress value, after the loop mode and easing.
pub fn spline_t(anim: &SplineAnimate, progress: f32) -> f32 {
    let x = match anim.loop_mode {
        LoopMode::Once => progress.clamp(0.0, 1.0),
        LoopMode::Loop => {
            let f = progress.rem_euclid(1.0);
            // Exactly at the end of a pass shows the end, not the start.
            if f == 0.0 && progress > 0.0 { 1.0 } else { f }
        }
        LoopMode::PingPong => {
            let f = progress.rem_euclid(2.0);
            if f > 1.0 { 2.0 - f } else { f }
        }
    };
    anim.easing.apply(x)
}

/// The container path and world matrix an animate component follows.
fn target(
    scene: &Scene,
    e: Entity,
    anim: &SplineAnimate,
) -> Option<(SplinePath, glam::Mat4, Entity)> {
    let c = container_object(scene, e, anim.container)?;
    let path = scene
        .world
        .get::<&SplineContainer>(c)
        .ok()?
        .path(anim.spline);
    (!path.is_empty()).then(|| (path, scene.world_matrix(c), c))
}

/// Length of the spline the component on `e` follows.
pub fn length(scene: &Scene, e: Entity) -> f32 {
    let Ok(anim) = scene.world.get::<&SplineAnimate>(e).map(|a| (*a).clone()) else {
        return 0.0;
    };
    target(scene, e, &anim).map_or(0.0, |(p, _, _)| p.length)
}

/// Put the object where its progress says.
pub fn apply(scene: &mut Scene, e: Entity) {
    let Ok(anim) = scene.world.get::<&SplineAnimate>(e).map(|a| (*a).clone()) else {
        return;
    };
    let Some((path, m, c)) = target(scene, e, &anim) else {
        return;
    };
    let d = duration(&anim, path.length);
    let t = spline_t(&anim, normalized_time(&anim, d));
    scene.set_world_position(e, m.transform_point3(path.position(t)));
    let forward = m.transform_vector3(path.tangent(t));
    let rotation = match anim.alignment {
        AnimateAlignment::None => return,
        AnimateAlignment::SplineElement => look_rotation(forward, m.transform_vector3(path.up(t))),
        AnimateAlignment::World => look_rotation(forward, Vec3::Y),
        AnimateAlignment::SplineObject => scene.world_rotation(c),
    };
    scene.set_world_rotation(e, rotation);
}

/// Edit mode: objects whose Spline Animate hasn't started sit at their start point.
pub fn place_unstarted(scene: &mut Scene) {
    let list: Vec<Entity> = scene
        .world
        .query::<(Entity, &SplineAnimate)>()
        .iter()
        .filter(|(_, a)| a.enabled && !a.started)
        .map(|(e, _)| e)
        .collect();
    for e in list {
        apply(scene, e);
    }
}

/// One frame: start Play On Awake components, advance playing ones by `dt`, place them all.
pub fn update(scene: &mut Scene, dt: f32) {
    let list: Vec<Entity> = scene
        .world
        .query::<(Entity, &SplineAnimate)>()
        .iter()
        .filter(|(_, a)| a.enabled)
        .map(|(e, _)| e)
        .collect();
    for e in list {
        let len = length(scene, e);
        if let Ok(mut a) = scene.world.get::<&mut SplineAnimate>(e) {
            if !a.started {
                a.started = true;
                a.playing = a.play_on_awake;
            }
            if a.playing {
                a.elapsed += dt;
                let d = duration(&a, len);
                if a.loop_mode == LoopMode::Once && normalized_time(&a, d) >= 1.0 {
                    let end = (1.0 - a.start_offset) * d;
                    a.elapsed = end;
                    a.playing = false;
                }
            }
        }
        apply(scene, e);
    }
}
