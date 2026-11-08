//! Camera tracks: bound to a Camera Brain, each clip shows a virtual camera (the clip's
//! reference). Where two clips overlap the brain blends from the earlier camera to the later
//! one over the overlap; a clip's ease in / out blends with the brain's own live camera. In
//! Play mode the result is a [`CameraOverride`] for the camera rig; in edit mode
//! [`preview`] poses the brain camera directly (and the preview's baseline puts it back).

use hecs::Entity;

use crate::scene::{Camera, Scene, Transform};
use crate::timeline::asset::Track;
use crate::timeline::evaluate::{Baseline, Resolver, samples};
use crate::vcam::solve::{CamState, preview_pose};
use crate::vcam::{CameraOverride, VirtualCamera, rig};

/// What the track shows on `brain` at `t`: `None` between clips.
pub fn shot(
    track: &Track,
    resolver: &dyn Resolver,
    brain: Entity,
    t: f32,
) -> Option<CameraOverride> {
    let mut playing = samples(&track.clips, t);
    if playing.is_empty() {
        return None;
    }
    let camera = |i: usize| {
        track.clips[i]
            .reference
            .as_deref()
            .and_then(|r| resolver.reference(r))
    };
    // The two latest-starting clips: the later one blends in over the earlier one.
    playing.sort_by(|a, b| {
        let (ca, cb) = (&track.clips[a.clip], &track.clips[b.clip]);
        ca.start.total_cmp(&cb.start).then(a.clip.cmp(&b.clip))
    });
    let to = playing[playing.len() - 1];
    if playing.len() == 1 {
        return Some(CameraOverride {
            brain,
            from: None,
            to: camera(to.clip),
            weight: to.weight,
        });
    }
    let from = playing[playing.len() - 2];
    let total = from.weight + to.weight;
    Some(CameraOverride {
        brain,
        from: camera(from.clip),
        to: camera(to.clip),
        weight: if total > 1e-6 { to.weight / total } else { 1.0 },
    })
}

/// The brain camera's pose before the preview touched it.
fn baseline_pose(t: &Transform, cam: Option<&Camera>) -> CamState {
    let c = cam.cloned().unwrap_or_default();
    CamState {
        position: t.position,
        rotation: t.rotation,
        fov: c.fov,
        near: c.near,
        far: c.far,
    }
}

/// Write a pose onto a brain camera (world space).
fn put(scene: &mut Scene, brain: Entity, pose: &CamState) {
    scene.set_world_position(brain, pose.position);
    scene.set_world_rotation(brain, pose.rotation);
    if let Ok(mut cam) = scene.world.get::<&mut Camera>(brain) {
        cam.fov = pose.fov;
        cam.near = pose.near;
        cam.far = pose.far;
    }
}

/// Put a brain camera back as the baseline remembered it.
pub fn put_back(scene: &mut Scene, brain: Entity, t: &Transform, cam: Option<&Camera>) {
    if !scene.contains(brain) {
        return;
    }
    scene.set_transform(brain, *t);
    if let (Some(c), Ok(mut now)) = (cam, scene.world.get::<&mut Camera>(brain)) {
        *now = c.clone();
    }
}

/// Edit mode: the camera with the highest priority stands in for the brain's live camera.
fn live_stand_in(scene: &Scene) -> Option<Entity> {
    rig::active_cameras(scene).into_iter().max_by_key(|e| {
        scene
            .world
            .get::<&VirtualCamera>(*e)
            .map_or(i32::MIN, |v| v.priority)
    })
}

/// Edit-mode preview: pose each overridden brain camera as its shot shows it (cameras solved
/// without damping), remembering the camera's own pose in `base` first. Brains no shot covers
/// any more go back to that pose.
pub fn preview(scene: &mut Scene, shots: &[CameraOverride], base: &mut Baseline, aspect: f32) {
    for s in shots {
        if !scene.contains(s.brain) {
            continue;
        }
        let saved = base.cameras.entry(s.brain).or_insert_with(|| {
            (
                scene.transform(s.brain),
                scene
                    .world
                    .get::<&Camera>(s.brain)
                    .ok()
                    .map(|c| (*c).clone()),
            )
        });
        let own = baseline_pose(&saved.0, saved.1.as_ref());
        let live = live_stand_in(scene)
            .and_then(|e| preview_pose(scene, e, aspect))
            .unwrap_or(own);
        let pose = |c: Option<Entity>| {
            c.filter(|c| scene.contains(*c))
                .and_then(|c| preview_pose(scene, c, aspect))
                .unwrap_or(live)
        };
        let shown = pose(s.from).lerp(&pose(s.to), s.weight.clamp(0.0, 1.0));
        put(scene, s.brain, &shown);
    }
    let idle: Vec<(Entity, Transform, Option<Camera>)> = base
        .cameras
        .iter()
        .filter(|(b, _)| !shots.iter().any(|s| s.brain == **b))
        .map(|(b, (t, c))| (*b, *t, c.clone()))
        .collect();
    for (b, t, c) in idle {
        put_back(scene, b, &t, c.as_ref());
    }
}
