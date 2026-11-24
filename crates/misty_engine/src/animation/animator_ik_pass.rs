//! The Animator side of the IK pass: which Animators run one, the Avatar they use, and the
//! per-layer evaluation (the goals and solver live in `rigging::animator_ik`).
//!
//! Per layer, like the familiar engine: while the Animator blends its layers it keeps each
//! layer's pose ([`IkLayerPose`]) when the controller has IK Pass layers. The IK pass then
//! blends them again bottom to top; after blending IK layer `i` it puts that pose on the rig,
//! calls `OnAnimatorIK(i)` (goals cleared before each call) and solves the goals on it,
//! scaled by layer `i`'s weight and limited to its Avatar Mask. The IK'd bones go back into
//! the running pose, so the layers above blend over them (an override layer at weight 1 that
//! animates the arm replaces the arm IK of the layers below). After the last layer the final
//! pose is written. Only Transform values of objects below the Animator are rewritten (the
//! Animator's own object keeps its root motion; lights, blend shapes and active flags are left
//! as the Animator wrote them). Controllers without IK Pass layers keep nothing and pay
//! nothing.

use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use super::{AnimatorPlayer, Key, Pose};
use crate::animation::binding;
use crate::animation::clip::Property;
use crate::animation::humanoid::Avatar;
use crate::camera::euler_to_quat;
use crate::scene::{Animator, Scene, Transform};

/// One layer's pose as it was blended this frame.
pub(super) struct IkLayerPose {
    layer: usize,
    pose: Pose,
    weight: f32,
    /// Additive layers: the reference pose (first frame) the layer's change is taken from.
    reference: Option<Pose>,
}

impl IkLayerPose {
    pub(super) fn new(layer: usize, pose: &Pose, weight: f32, reference: Option<&Pose>) -> Self {
        Self {
            layer,
            pose: pose.clone(),
            weight,
            reference: reference.cloned(),
        }
    }
}

/// The layers of one Animator this frame and how far the IK pass has blended them.
#[derive(Default)]
pub(super) struct IkFrame {
    stack: Vec<IkLayerPose>,
    next: usize,
    running: Option<Pose>,
    /// Pose of the bones IK can move before the pass.
    inputs: Vec<(Entity, Transform)>,
}

impl IkFrame {
    pub(super) fn new(stack: Vec<IkLayerPose>) -> Self {
        Self {
            stack,
            ..Default::default()
        }
    }
}

/// Values the pass rewrites: Transform values of objects below the Animator.
fn rewritable(root: Entity, k: &Key) -> bool {
    k.0 != root
        && matches!(
            k.1,
            Property::Position(_) | Property::Rotation(_) | Property::Scale(_)
        )
}

/// Blend `lp` over `running`, exactly as the Animator did.
fn blend_into(running: &mut Option<Pose>, lp: &IkLayerPose, rest: &mut dyn FnMut(Key) -> f32) {
    let Some(below) = running.as_mut() else {
        *running = Some(lp.pose.clone());
        return;
    };
    for (k, v) in &lp.pose {
        let base = match below.get(k) {
            Some(b) => *b,
            None => rest(*k),
        };
        let v = match &lp.reference {
            Some(r) => base + lp.weight * (v - r.get(k).copied().unwrap_or(*v)),
            None => base + (v - base) * lp.weight,
        };
        below.insert(*k, v);
    }
}

/// Euler angles of `t`'s rotation closest to `near` (same rotation, ±360° and the other
/// decomposition tried), so layers above blend from where the animation was.
fn euler_near(t: &Transform, near: Vec3) -> Vec3 {
    let e = t.euler();
    let wrap = |a: f32, n: f32| a + ((n - a) / 360.0).round() * 360.0;
    let q = euler_to_quat(e);
    [
        e,
        Vec3::new(e.x + 180.0, 180.0 - e.y, e.z + 180.0),
        Vec3::new(180.0 - e.x, e.y + 180.0, e.z + 180.0),
    ]
    .into_iter()
    .filter(|c| euler_to_quat(*c).dot(q).abs() > 0.9999)
    .map(|c| Vec3::new(wrap(c.x, near.x), wrap(c.y, near.y), wrap(c.z, near.z)))
    .min_by(|a, b| {
        a.distance_squared(near)
            .total_cmp(&b.distance_squared(near))
    })
    .unwrap_or(e)
}

impl AnimatorPlayer {
    /// Playing Animators whose controller has layers with IK Pass on, with those layers.
    pub fn ik_passes(&mut self, scene: &Scene) -> Vec<(Entity, Vec<usize>)> {
        let mut out: Vec<(Entity, Vec<usize>)> = self
            .instances
            .iter()
            .filter(|(_, inst)| {
                inst.ctrl.base_ik_pass || inst.ctrl.layers.iter().any(|l| l.ik_pass)
            })
            .filter(|(e, _)| {
                scene.contains(**e)
                    && scene.active_in_hierarchy(**e)
                    && scene.world.get::<&Animator>(**e).is_ok_and(|a| a.enabled)
            })
            .map(|(e, inst)| {
                let layers = (0..inst.ctrl.layer_count())
                    .filter(|l| inst.ctrl.ik_pass(*l))
                    .collect();
                (*e, layers)
            })
            .collect();
        out.sort_by_key(|(e, _)| e.to_bits());
        if !out.is_empty() {
            self.ik.retain(scene);
        }
        out
    }

    /// The Avatar IK of `e` works with: its Animator's, or one found by bone names.
    pub fn ik_avatar(&mut self, scene: &Scene, e: Entity) -> Arc<Avatar> {
        let own = scene
            .world
            .get::<&Animator>(e)
            .ok()
            .and_then(|a| a.avatar.clone())
            .and_then(|p| self.assets.avatar(&p));
        self.ik.avatar_for(scene, e, own)
    }

    /// Start `e`'s IK pass (goals cleared, last frame's IK undone where nothing else moved).
    pub fn ik_begin(&mut self, scene: &mut Scene, e: Entity) {
        self.ik.begin(scene, e);
        let avatar = self.ik_avatar(scene, e);
        let inputs = self.ik.snapshot(scene, e, &avatar);
        if let Some(f) = self.ik_frames.get_mut(&e) {
            f.next = 0;
            f.running = None;
            f.inputs = inputs;
        } else {
            self.ik_frames.insert(
                e,
                IkFrame {
                    inputs,
                    ..Default::default()
                },
            );
        }
    }

    /// Blend `e`'s layers up to and including `upto` (those not blended yet) and put the
    /// result on the rig; with `all`, every layer above too.
    fn ik_blend(&mut self, scene: &mut Scene, e: Entity, upto: usize, all: bool) {
        let Some(f) = self.ik_frames.get_mut(&e) else {
            return;
        };
        let defaults = &self.defaults;
        let mut rest = |k: Key| -> f32 {
            defaults
                .get(&k)
                .copied()
                .or_else(|| binding::read(scene, k.0, k.1))
                .unwrap_or(0.0)
        };
        while let Some(lp) = f.stack.get(f.next) {
            if !all && lp.layer > upto {
                break;
            }
            blend_into(&mut f.running, lp, &mut rest);
            f.next += 1;
        }
        let Some(running) = f.running.as_ref() else {
            return;
        };
        // Values only the layers above animate sit at their rest value for now.
        let mut keys: Vec<(Key, f32)> = f
            .stack
            .iter()
            .flat_map(|lp| lp.pose.keys())
            .chain(running.keys())
            .filter(|k| rewritable(e, k))
            .map(|k| (*k, running.get(k).copied().unwrap_or_else(|| rest(*k))))
            .collect();
        keys.sort_by_key(|((t, p), _)| (t.to_bits(), *p));
        keys.dedup_by_key(|(k, _)| *k);
        for ((t, p), v) in keys {
            binding::write(scene, t, p, v);
        }
    }

    /// Before `OnAnimatorIK(layer)`: `e`'s rig shows its layers blended up to and including
    /// `layer`, and the goals are cleared.
    pub fn ik_layer_begin(&mut self, scene: &mut Scene, e: Entity, layer: usize) {
        self.ik_blend(scene, e, layer, false);
        self.ik.reset_goals(e);
    }

    /// After `OnAnimatorIK(layer)`: solve its goals by the layer's weight and Avatar Mask; the
    /// moved bones go back into the running pose for the layers above.
    pub fn ik_layer_solve(&mut self, scene: &mut Scene, e: Entity, layer: usize) {
        let avatar = self.ik_avatar(scene, e);
        let frame = self.ik_frames.get(&e);
        let weight = match frame {
            _ if layer == 0 => 1.0,
            Some(f) if !f.stack.is_empty() => f
                .stack
                .iter()
                .find(|lp| lp.layer == layer)
                .map_or(0.0, |lp| lp.weight),
            // No pose kept this frame: the layer's weight as set.
            _ => self.layer_weight(scene, e, layer),
        };
        let mask = self
            .instances
            .get(&e)
            .and_then(|i| i.ctrl.layer(layer))
            .and_then(|l| l.mask.clone())
            .and_then(|m| self.assets.mask(&m));
        let moved = self
            .ik
            .solve_layer(scene, e, &avatar, mask.as_deref(), weight);
        let Some(running) = self.ik_frames.get_mut(&e).and_then(|f| f.running.as_mut()) else {
            return;
        };
        for b in moved {
            let t = scene.transform(b);
            let cur = t.euler();
            let near = |i: u8| running.get(&(b, Property::Rotation(i))).copied();
            let near = Vec3::new(
                near(0).unwrap_or(cur.x),
                near(1).unwrap_or(cur.y),
                near(2).unwrap_or(cur.z),
            );
            let euler = euler_near(&t, near);
            for i in 0..3u8 {
                running.insert((b, Property::Rotation(i)), euler[i as usize]);
            }
        }
    }

    /// End `e`'s IK pass: blend the layers above the last IK layer and write the final pose.
    pub fn ik_end(&mut self, scene: &mut Scene, e: Entity) {
        self.ik_blend(scene, e, usize::MAX, true);
        let inputs = self
            .ik_frames
            .get_mut(&e)
            .map(|f| std::mem::take(&mut f.inputs))
            .unwrap_or_default();
        self.ik.remember(scene, e, inputs);
    }

    /// Solve the goals set in `e`'s `OnAnimatorIK`.
    pub fn ik_solve(&mut self, scene: &mut Scene, e: Entity) {
        let avatar = self.ik_avatar(scene, e);
        self.ik.solve(scene, e, &avatar);
    }
}
