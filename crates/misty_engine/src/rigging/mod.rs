//! Animation rigging: constraints evaluated on top of the animated pose every frame, after
//! animation and before rendering (see [`evaluate`]).
//!
//! A **Rig Builder** lists rigs; a **Rig** is an object whose descendants carry constraints
//! (Two Bone IK, Multi-Aim, Damped Transform, Chain IK, Override Transform). Bones, targets and
//! hints are ordinary Transforms, so constraints work on any hierarchy, skinned or not.
//!
//! Evaluation is non-destructive: [`RigState`] remembers each driven bone's pose before and
//! after the rig. When nothing else changed a bone since the last frame (no animation curve or
//! script wrote it), its pre-rig pose is restored before solving, so weights blend against the
//! animated pose instead of accumulating frame after frame.

pub mod components;
mod solvers;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use glam::{Quat, Vec3};
use hecs::Entity;

pub use components::*;

use crate::camera::euler_to_quat;
use crate::scene::{Scene, Transform};
use solvers::{pos, rot};

/// One driven bone's pose before and after the rig, last frame.
#[derive(Clone, Copy)]
struct Cached {
    input: Transform,
    output: Transform,
}

/// Offsets captured when a constraint first runs ("bind" time).
#[derive(Clone, Copy, Default)]
struct Bind {
    /// Tip − target (world), target⁻¹ · tip rotation.
    position: Vec3,
    rotation: Quat,
}

/// Per-run evaluation memory (one per Play session or Animation window preview).
#[derive(Default)]
pub struct RigState {
    poses: HashMap<Entity, Cached>,
    binds: HashMap<Entity, Bind>,
    /// Damped Transform: last world pose of the constrained object.
    damped: HashMap<Entity, (Vec3, Quat)>,
}

impl RigState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put every driven bone back to its pre-rig pose (when nothing else moved it) and forget
    /// everything (end of an edit-mode preview).
    pub fn restore(&mut self, scene: &mut Scene) {
        for (e, c) in self.poses.drain() {
            if scene.contains(e) && same_pose(&scene.transform(e), &c.output) {
                scene.set_transform(e, c.input);
            }
        }
        self.binds.clear();
        self.damped.clear();
    }
}

fn same_pose(a: &Transform, b: &Transform) -> bool {
    a.position == b.position && a.rotation == b.rotation && a.scale == b.scale
}

/// A constraint found under a rig: its object, kind and the rig's weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    TwoBoneIk,
    MultiAim,
    Damped,
    ChainIk,
    Override,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::TwoBoneIk,
        Kind::MultiAim,
        Kind::Damped,
        Kind::ChainIk,
        Kind::Override,
    ];

    pub fn of(scene: &Scene, e: Entity) -> Vec<Kind> {
        let w = &scene.world;
        Kind::ALL
            .into_iter()
            .filter(|k| match k {
                Kind::TwoBoneIk => w.get::<&TwoBoneIk>(e).is_ok(),
                Kind::MultiAim => w.get::<&MultiAim>(e).is_ok(),
                Kind::Damped => w.get::<&DampedTransform>(e).is_ok(),
                Kind::ChainIk => w.get::<&ChainIk>(e).is_ok(),
                Kind::Override => w.get::<&OverrideTransform>(e).is_ok(),
            })
            .collect()
    }
}

/// The bones a constraint writes, root first (empty when it is not set up).
pub fn driven(scene: &Scene, e: Entity, kind: Kind) -> Vec<Entity> {
    let w = &scene.world;
    let ok = |o: Option<Entity>| o.filter(|e| scene.contains(*e));
    match kind {
        Kind::TwoBoneIk => w
            .get::<&TwoBoneIk>(e)
            .ok()
            .and_then(|c| Some(vec![ok(c.root)?, ok(c.mid)?, ok(c.tip)?]))
            .unwrap_or_default(),
        Kind::MultiAim => w
            .get::<&MultiAim>(e)
            .ok()
            .and_then(|c| ok(c.constrained_object))
            .into_iter()
            .collect(),
        Kind::Damped => w
            .get::<&DampedTransform>(e)
            .ok()
            .and_then(|c| ok(c.constrained_object))
            .into_iter()
            .collect(),
        Kind::ChainIk => w
            .get::<&ChainIk>(e)
            .ok()
            .and_then(|c| solvers::chain(scene, ok(c.root)?, ok(c.tip)?))
            .unwrap_or_default(),
        Kind::Override => w
            .get::<&OverrideTransform>(e)
            .ok()
            .and_then(|c| ok(c.constrained_object))
            .into_iter()
            .collect(),
    }
}

/// Rigs a builder evaluates, in order, with their weights.
pub fn builder_rigs(scene: &Scene, builder: Entity) -> Vec<(Entity, f32)> {
    let Ok(b) = scene.world.get::<&RigBuilder>(builder) else {
        return Vec::new();
    };
    let weight = |r: Entity| scene.world.get::<&Rig>(r).ok().map(|r| r.weight);
    if b.layers.is_empty() {
        return scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| scene.is_ancestor(builder, *e))
            .filter_map(|e| Some((e, weight(e)?)))
            .collect();
    }
    b.layers
        .iter()
        .filter(|l| l.active)
        .filter_map(|l| {
            let r = l.rig.filter(|r| scene.contains(*r))?;
            Some((r, weight(r)?))
        })
        .collect()
}

/// Constraints under a rig in hierarchy order (the rig object itself included).
pub fn rig_constraints(scene: &Scene, rig: Entity) -> Vec<(Entity, Kind)> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| *e == rig || scene.is_ancestor(rig, *e))
        .filter(|e| scene.active_in_hierarchy(*e))
        .flat_map(|e| Kind::of(scene, e).into_iter().map(move |k| (e, k)))
        .collect()
}

/// Constraint weight (0..1).
pub fn weight(scene: &Scene, e: Entity, kind: Kind) -> f32 {
    let w = &scene.world;
    match kind {
        Kind::TwoBoneIk => w.get::<&TwoBoneIk>(e).map(|c| c.weight),
        Kind::MultiAim => w.get::<&MultiAim>(e).map(|c| c.weight),
        Kind::Damped => w.get::<&DampedTransform>(e).map(|c| c.weight),
        Kind::ChainIk => w.get::<&ChainIk>(e).map(|c| c.weight),
        Kind::Override => w.get::<&OverrideTransform>(e).map(|c| c.weight),
    }
    .unwrap_or(0.0)
    .clamp(0.0, 1.0)
}

/// Every enabled, active Rig Builder (inside `root` when given).
fn builders(scene: &Scene, root: Option<Entity>) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| root.is_none_or(|r| *e == r || scene.is_ancestor(r, *e)))
        .filter(|e| scene.active_in_hierarchy(*e))
        .filter(|e| scene.world.get::<&RigBuilder>(*e).is_ok_and(|b| b.enabled))
        .collect()
}

/// Evaluate every rig in the scene (or below `root`) on the current pose. `dt` drives
/// Damped Transform.
pub fn evaluate(scene: &mut Scene, state: &mut RigState, dt: f32, root: Option<Entity>) {
    let mut work: Vec<(Entity, Kind, f32)> = Vec::new();
    for b in builders(scene, root) {
        for (rig, rig_weight) in builder_rigs(scene, b) {
            let rig_weight = rig_weight.clamp(0.0, 1.0);
            for (e, k) in rig_constraints(scene, rig) {
                work.push((e, k, rig_weight * weight(scene, e, k)));
            }
        }
    }
    if work.is_empty() {
        return;
    }
    // Restore pre-rig poses of bones nothing else touched since last frame.
    let mut bones: Vec<Entity> = work
        .iter()
        .flat_map(|(e, k, _)| driven(scene, *e, *k))
        .collect();
    bones.sort();
    bones.dedup();
    for &b in &bones {
        let now = scene.transform(b);
        let input = match state.poses.get(&b) {
            Some(c) if same_pose(&now, &c.output) => c.input,
            _ => now,
        };
        scene.set_transform(b, input);
        state.poses.insert(
            b,
            Cached {
                input,
                output: input,
            },
        );
    }
    for (e, kind, w) in work {
        let bones = driven(scene, e, kind);
        if bones.is_empty() {
            continue;
        }
        if w <= 0.0 {
            // Off: forget the lag so it does not jump when turned back on.
            if kind == Kind::Damped {
                state.damped.remove(&e);
            }
            continue;
        }
        let before: Vec<Transform> = bones.iter().map(|b| scene.transform(*b)).collect();
        solve(scene, state, e, kind, &bones, dt);
        if w < 1.0 {
            for (b, old) in bones.iter().zip(&before) {
                let new = scene.transform(*b);
                let mut t = *old;
                t.position = old.position.lerp(new.position, w);
                t.set_rotation(old.rotation.slerp(new.rotation, w));
                t.scale = old.scale.lerp(new.scale, w);
                scene.set_transform(*b, t);
            }
        }
        if kind == Kind::Damped
            && let Some(&b) = bones.first()
        {
            state.damped.insert(e, (pos(scene, b), rot(scene, b)));
        }
    }
    for b in bones {
        if let Some(c) = state.poses.get_mut(&b) {
            c.output = scene.transform(b);
        }
    }
}

/// World position of `e` when set and alive.
fn object_pos(scene: &Scene, e: Option<Entity>) -> Option<Vec3> {
    e.filter(|e| scene.contains(*e)).map(|e| pos(scene, e))
}

fn solve(
    scene: &mut Scene,
    state: &mut RigState,
    e: Entity,
    kind: Kind,
    bones: &[Entity],
    dt: f32,
) {
    match kind {
        Kind::TwoBoneIk => {
            let Some(c) = scene.world.get::<&TwoBoneIk>(e).ok().map(|c| (*c).clone()) else {
                return;
            };
            let Some(target) = c.target.filter(|t| scene.contains(*t)) else {
                return;
            };
            let tip = bones[2];
            let bind = *state.binds.entry(e).or_insert_with(|| Bind {
                position: pos(scene, tip) - pos(scene, target),
                rotation: rot(scene, target).inverse() * rot(scene, tip),
            });
            let mut t_pos = pos(scene, target);
            let mut t_rot = rot(scene, target);
            if c.maintain_target_position_offset {
                t_pos += bind.position;
            }
            if c.maintain_target_rotation_offset {
                t_rot *= bind.rotation;
            }
            let goal = pos(scene, tip).lerp(t_pos, c.target_position_weight.clamp(0.0, 1.0));
            let hint = object_pos(scene, c.hint).map(|h| (h, c.hint_weight.clamp(0.0, 1.0)));
            solvers::two_bone(scene, [bones[0], bones[1], tip], goal, hint);
            let rw = c.target_rotation_weight.clamp(0.0, 1.0);
            if rw > 0.0 {
                let r = rot(scene, tip).slerp(t_rot, rw);
                scene.set_world_rotation(tip, r);
            }
        }
        Kind::ChainIk => {
            let Some(c) = scene.world.get::<&ChainIk>(e).ok().map(|c| (*c).clone()) else {
                return;
            };
            let Some(target) = c.target.filter(|t| scene.contains(*t)) else {
                return;
            };
            let tip = *bones.last().expect("chain has bones");
            let bind = *state.binds.entry(e).or_insert_with(|| Bind {
                position: pos(scene, tip) - pos(scene, target),
                rotation: rot(scene, target).inverse() * rot(scene, tip),
            });
            let mut t_pos = pos(scene, target);
            let mut t_rot = rot(scene, target);
            if c.maintain_target_position_offset {
                t_pos += bind.position;
            }
            if c.maintain_target_rotation_offset {
                t_rot *= bind.rotation;
            }
            solvers::chain_ik(
                scene,
                bones,
                t_pos,
                c.solver,
                c.max_iterations,
                c.tolerance,
                c.chain_rotation_weight.clamp(0.0, 1.0),
            );
            let rw = c.tip_rotation_weight.clamp(0.0, 1.0);
            if rw > 0.0 {
                let r = rot(scene, tip).slerp(t_rot, rw);
                scene.set_world_rotation(tip, r);
            }
        }
        Kind::MultiAim => {
            let Some(c) = scene.world.get::<&MultiAim>(e).ok().map(|c| (*c).clone()) else {
                return;
            };
            let obj = bones[0];
            let (mut sum, mut total) = (Vec3::ZERO, 0.0);
            for src in &c.sources {
                if let Some(p) = object_pos(scene, src.object) {
                    let w = src.weight.clamp(0.0, 1.0);
                    sum += p * w;
                    total += w;
                }
            }
            if total <= 0.0 {
                return;
            }
            let here = pos(scene, obj);
            let current = rot(scene, obj);
            let up = match c.world_up_type {
                WorldUpType::None => None,
                WorldUpType::SceneUp => Some(Vec3::Y),
                WorldUpType::Vector => Some(c.world_up_vector),
                WorldUpType::ObjectUp => object_pos(scene, c.world_up_object).map(|p| p - here),
                WorldUpType::ObjectRotationUp => c
                    .world_up_object
                    .filter(|o| scene.contains(*o))
                    .map(|o| rot(scene, o) * c.world_up_vector),
            };
            let aim = solvers::aim_rotation(
                current,
                c.aim_axis.vector(),
                c.up_axis.vector(),
                sum / total - here,
                up,
            );
            let bind = *state.binds.entry(e).or_insert_with(|| Bind {
                position: Vec3::ZERO,
                rotation: aim.inverse() * current,
            });
            let mut out = aim;
            if c.maintain_offset {
                out *= bind.rotation;
            }
            out *= euler_to_quat(c.offset);
            // Limits and axes act on the turn away from the input pose, in local space.
            let parent = scene
                .parent(obj)
                .map(|p| rot(scene, p))
                .unwrap_or(Quat::IDENTITY);
            let input_local = scene.transform(obj).rotation;
            let out_local = parent.inverse() * out;
            let delta = solvers::limit(
                input_local.inverse() * out_local,
                c.constrained_axes,
                c.min_limit,
                c.max_limit,
            );
            let mut t = scene.transform(obj);
            t.set_rotation(input_local * delta);
            scene.set_transform(obj, t);
        }
        Kind::Damped => {
            let Some(c) = scene
                .world
                .get::<&DampedTransform>(e)
                .ok()
                .map(|c| (*c).clone())
            else {
                return;
            };
            let Some(src) = c.source.filter(|s| scene.contains(*s)) else {
                return;
            };
            let obj = bones[0];
            let (sp, sr) = (pos(scene, src), rot(scene, src));
            let bind = *state.binds.entry(e).or_insert_with(|| Bind {
                position: sr.inverse() * (pos(scene, obj) - sp),
                rotation: sr.inverse() * rot(scene, obj),
            });
            let goal_p = sp + sr * bind.position;
            let goal_r = sr * bind.rotation;
            let (prev_p, prev_r) = state.damped.get(&e).copied().unwrap_or((goal_p, goal_r));
            // Fraction of the gap closed this frame: frame-rate independent, 0 = stuck.
            let follow = |damp: f32| 1.0 - damp.clamp(0.0, 1.0).powf(dt.max(0.0) * 60.0);
            let p = prev_p.lerp(goal_p, follow(c.damp_position));
            let mut r = prev_r.slerp(goal_r, follow(c.damp_rotation));
            if c.maintain_aim {
                // Keep the source → object direction the rotation implies.
                let want = p - sp;
                let had = r * (bind.rotation.inverse() * bind.position);
                if want.length_squared() > 1e-8 && had.length_squared() > 1e-8 {
                    r = Quat::from_rotation_arc(had.normalize(), want.normalize()) * r;
                }
            }
            scene.set_world_position(obj, p);
            scene.set_world_rotation(obj, r.normalize());
        }
        Kind::Override => {
            let Some(c) = scene
                .world
                .get::<&OverrideTransform>(e)
                .ok()
                .map(|c| (*c).clone())
            else {
                return;
            };
            let obj = bones[0];
            let input = scene.transform(obj);
            let (p, r) = match c.source.filter(|s| scene.contains(*s)) {
                Some(s) => (pos(scene, s), rot(scene, s)),
                None => {
                    let r = euler_to_quat(c.rotation);
                    match c.space {
                        OverrideSpace::World => (c.position, r),
                        OverrideSpace::Local | OverrideSpace::Pivot => {
                            let (lp, lr) = if c.space == OverrideSpace::Local {
                                (c.position, r)
                            } else {
                                (
                                    input.position + input.rotation * c.position,
                                    input.rotation * r,
                                )
                            };
                            let parent = scene.parent(obj).map(|p| scene.world_matrix(p));
                            match parent {
                                Some(m) => {
                                    let (_, pr, _) = m.to_scale_rotation_translation();
                                    (m.transform_point3(lp), pr * lr)
                                }
                                None => (lp, lr),
                            }
                        }
                    }
                }
            };
            let pw = c.position_weight.clamp(0.0, 1.0);
            let rw = c.rotation_weight.clamp(0.0, 1.0);
            let here = pos(scene, obj);
            scene.set_world_position(obj, here.lerp(p, pw));
            let r0 = rot(scene, obj);
            scene.set_world_rotation(obj, r0.slerp(r, rw));
        }
    }
}
