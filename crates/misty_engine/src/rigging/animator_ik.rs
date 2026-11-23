//! The Animator's IK pass (`OnAnimatorIK`): goals scripts set for the hands, feet and head,
//! solved on the animated pose of a humanoid rig.
//!
//! Each frame, for every Animator whose controller has a layer with **IK Pass** on, the
//! scripting host (after the Animator wrote its pose and root motion was applied):
//! 1. calls [`AnimatorIkState::begin`]: goals are cleared (weights 0, like the familiar engine,
//!    they must be set again every frame) and bones IK moved last frame that nothing else moved
//!    since go back to their pre-IK pose, so weights blend against the animation instead of
//!    accumulating;
//! 2. calls `OnAnimatorIK(layerIndex)` on the Animator object's scripts, once per IK layer;
//!    they call `SetIKPosition` / `SetIKRotation` / weights, hints and `SetLookAt…`;
//! 3. calls [`AnimatorIkState::solve`]: Two Bone IK per limb (upper → lower → hand / foot,
//!    bent towards the elbow / knee hint), then the hand / foot rotation, then look-at on the
//!    spine / chest (body weight) and head (head weight).
//!
//! Bones come from the Animator's Avatar; without one they are found by name
//! ([`Avatar::auto_map`], the pose at the first IK frame then counts as the rest pose).
//!
//! Rotations are humanoid effector rotations, as in the familiar engine: identity means "the
//! bone as in the Avatar's rest pose with the character unrotated"; a bone's world rotation is
//! `goal rotation * rest rotation (in the Animator's space)`. Look-at turns the head's rest
//! forward (the Animator's +Z at rest) towards the point.
//!
//! Limits: goals of all IK layers are solved once, on the final pose (not per layer); no eye
//! bones (eyes weight is stored but unused); no foot pivot, body position or stretch.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::solvers::{self, pos, rot};
use crate::animation::binding;
use crate::animation::humanoid::{Avatar, HumanBone};
use crate::scene::{Scene, Transform};

/// `AvatarIKGoal`, in the familiar engine's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IkGoal {
    LeftFoot = 0,
    RightFoot = 1,
    LeftHand = 2,
    RightHand = 3,
}

impl IkGoal {
    pub const ALL: [Self; 4] = [
        Self::LeftFoot,
        Self::RightFoot,
        Self::LeftHand,
        Self::RightHand,
    ];

    pub fn from_index(i: i64) -> Option<Self> {
        Self::ALL.get(usize::try_from(i).ok()?).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LeftFoot => "LeftFoot",
            Self::RightFoot => "RightFoot",
            Self::LeftHand => "LeftHand",
            Self::RightHand => "RightHand",
        }
    }

    /// Upper, lower and end bone of the limb.
    pub fn chain(self) -> [HumanBone; 3] {
        use HumanBone::*;
        match self {
            Self::LeftFoot => [LeftUpperLeg, LeftLowerLeg, LeftFoot],
            Self::RightFoot => [RightUpperLeg, RightLowerLeg, RightFoot],
            Self::LeftHand => [LeftUpperArm, LeftLowerArm, LeftHand],
            Self::RightHand => [RightUpperArm, RightLowerArm, RightHand],
        }
    }
}

/// `AvatarIKHint`, in the familiar engine's order (hint `i` bends goal `i`'s limb).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IkHint {
    LeftKnee = 0,
    RightKnee = 1,
    LeftElbow = 2,
    RightElbow = 3,
}

impl IkHint {
    pub const ALL: [Self; 4] = [
        Self::LeftKnee,
        Self::RightKnee,
        Self::LeftElbow,
        Self::RightElbow,
    ];

    pub fn from_index(i: i64) -> Option<Self> {
        Self::ALL.get(usize::try_from(i).ok()?).copied()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LeftKnee => "LeftKnee",
            Self::RightKnee => "RightKnee",
            Self::LeftElbow => "LeftElbow",
            Self::RightElbow => "RightElbow",
        }
    }

    /// The middle bone it bends.
    pub fn bone(self) -> HumanBone {
        IkGoal::ALL[self as usize].chain()[1]
    }
}

/// `HumanBodyBones` index (the familiar engine's numbering, 0 … 20) → our human bone.
pub fn human_body_bone(i: i64) -> Option<HumanBone> {
    use HumanBone::*;
    Some(match i {
        0 => Hips,
        1 => LeftUpperLeg,
        2 => RightUpperLeg,
        3 => LeftLowerLeg,
        4 => RightLowerLeg,
        5 => LeftFoot,
        6 => RightFoot,
        7 => Spine,
        8 => Chest,
        9 => Neck,
        10 => Head,
        11 => LeftShoulder,
        12 => RightShoulder,
        13 => LeftUpperArm,
        14 => RightUpperArm,
        15 => LeftLowerArm,
        16 => RightLowerArm,
        17 => LeftHand,
        18 => RightHand,
        19 => LeftToes,
        20 => RightToes,
        _ => return None,
    })
}

/// One hand / foot goal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub position: Vec3,
    pub rotation: Quat,
    pub position_weight: f32,
    pub rotation_weight: f32,
    /// Set this frame (otherwise Get… report the animated pose).
    pub position_set: bool,
    pub rotation_set: bool,
}

impl Default for Goal {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            position_weight: 0.0,
            rotation_weight: 0.0,
            position_set: false,
            rotation_set: false,
        }
    }
}

/// One elbow / knee hint.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hint {
    pub position: Vec3,
    pub weight: f32,
    pub set: bool,
}

/// `SetLookAtPosition` / `SetLookAtWeight`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookAt {
    pub position: Vec3,
    pub weight: f32,
    pub body_weight: f32,
    pub head_weight: f32,
    pub eyes_weight: f32,
    /// 0: the head turns freely; 0.5: at most 90° from where it looks; 1: not at all.
    pub clamp_weight: f32,
}

impl Default for LookAt {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            weight: 0.0,
            body_weight: 0.0,
            head_weight: 1.0,
            eyes_weight: 0.0,
            clamp_weight: 0.5,
        }
    }
}

/// What the scripts asked for this frame, per Animator.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IkGoals {
    pub goals: [Goal; 4],
    pub hints: [Hint; 4],
    pub look_at: LookAt,
}

impl IkGoals {
    pub fn goal_mut(&mut self, g: IkGoal) -> &mut Goal {
        &mut self.goals[g as usize]
    }

    pub fn hint_mut(&mut self, h: IkHint) -> &mut Hint {
        &mut self.hints[h as usize]
    }
}

#[derive(Clone, Copy)]
struct Cached {
    input: Transform,
    output: Transform,
}

/// Goals and evaluation memory of every Animator's IK pass (one per Play session).
#[derive(Default)]
pub struct AnimatorIkState {
    goals: HashMap<Entity, IkGoals>,
    /// Bone poses before and after last frame's IK, per Animator.
    poses: HashMap<Entity, Vec<(Entity, Cached)>>,
    /// Avatars found by bone names for Animators without one.
    auto: HashMap<Entity, Arc<Avatar>>,
}

/// The object of `bone` under the Animator `root`.
pub fn bone_entity(
    scene: &Scene,
    root: Entity,
    avatar: &Avatar,
    bone: HumanBone,
) -> Option<Entity> {
    let b = avatar.bone(bone)?;
    let e = if b.path.is_empty() {
        root
    } else {
        binding::resolve(scene, root, &b.path)?
    };
    scene.contains(e).then_some(e)
}

/// Rest rotation of `bone` in the Animator's space.
fn rest(avatar: &Avatar, bone: HumanBone) -> Quat {
    avatar
        .bone(bone)
        .map_or(Quat::IDENTITY, |b| Quat::from_array(b.rest).normalize())
}

/// Every bone the pass can move.
const MOVED: [HumanBone; 15] = {
    use HumanBone::*;
    [
        LeftUpperLeg,
        LeftLowerLeg,
        LeftFoot,
        RightUpperLeg,
        RightLowerLeg,
        RightFoot,
        LeftUpperArm,
        LeftLowerArm,
        LeftHand,
        RightUpperArm,
        RightLowerArm,
        RightHand,
        Spine,
        Chest,
        Head,
    ]
};

fn same_pose(a: &Transform, b: &Transform) -> bool {
    a.position == b.position && a.rotation == b.rotation && a.scale == b.scale
}

/// Rotation turning `from` onto `to`, at most `max` radians.
fn clamped_arc(from: Vec3, to: Vec3, max: f32) -> Quat {
    let (Some(a), Some(b)) = (from.try_normalize(), to.try_normalize()) else {
        return Quat::IDENTITY;
    };
    let q = Quat::from_rotation_arc(a, b);
    let (axis, angle) = q.to_axis_angle();
    if angle <= max || !angle.is_finite() {
        q
    } else {
        Quat::from_axis_angle(axis, max.max(0.0))
    }
}

/// Turn `e` by the world rotation `delta` about its pivot.
fn turn(scene: &mut Scene, e: Entity, delta: Quat) {
    let r = rot(scene, e);
    scene.set_world_rotation(e, (delta * r).normalize());
}

impl AnimatorIkState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The Avatar the IK of `root` uses: `avatar` (the Animator's), or one found by bone names.
    pub fn avatar_for(
        &mut self,
        scene: &Scene,
        root: Entity,
        avatar: Option<Arc<Avatar>>,
    ) -> Arc<Avatar> {
        if let Some(a) = avatar {
            return a;
        }
        self.auto
            .entry(root)
            .or_insert_with(|| Arc::new(Avatar::auto_map(scene, root)))
            .clone()
    }

    /// This frame's goals of `root` (default ones when none were set).
    pub fn goals(&self, root: Entity) -> IkGoals {
        self.goals.get(&root).cloned().unwrap_or_default()
    }

    pub fn goals_mut(&mut self, root: Entity) -> &mut IkGoals {
        self.goals.entry(root).or_default()
    }

    /// Forget objects that are gone.
    pub fn retain(&mut self, scene: &Scene) {
        self.goals.retain(|e, _| scene.contains(*e));
        self.poses.retain(|e, _| scene.contains(*e));
        self.auto.retain(|e, _| scene.contains(*e));
    }

    /// Start `root`'s IK pass: clear its goals and put back the pre-IK pose of bones nothing
    /// else moved since last frame's solve.
    pub fn begin(&mut self, scene: &mut Scene, root: Entity) {
        self.goals.insert(root, IkGoals::default());
        if let Some(list) = self.poses.remove(&root) {
            for (b, c) in list {
                if scene.contains(b) && same_pose(&scene.transform(b), &c.output) {
                    scene.set_transform(b, c.input);
                }
            }
        }
    }

    /// Solve `root`'s goals on the current pose.
    pub fn solve(&mut self, scene: &mut Scene, root: Entity, avatar: &Avatar) {
        let goals = self.goals(root);
        let bones: Vec<Entity> = MOVED
            .iter()
            .filter_map(|b| bone_entity(scene, root, avatar, *b))
            .collect();
        let inputs: Vec<Transform> = bones.iter().map(|b| scene.transform(*b)).collect();
        solve_goals(scene, root, avatar, &goals);
        let list = bones
            .iter()
            .zip(inputs)
            .map(|(b, input)| {
                let output = scene.transform(*b);
                (*b, Cached { input, output })
            })
            .collect();
        self.poses.insert(root, list);
    }
}

/// The world position of goal `g`'s bone (the animated pose before IK when called in
/// `OnAnimatorIK`).
pub fn effector_position(scene: &Scene, root: Entity, avatar: &Avatar, g: IkGoal) -> Vec3 {
    bone_entity(scene, root, avatar, g.chain()[2]).map_or(Vec3::ZERO, |e| pos(scene, e))
}

/// The effector rotation of goal `g`'s bone: its world rotation without the rest rotation.
pub fn effector_rotation(scene: &Scene, root: Entity, avatar: &Avatar, g: IkGoal) -> Quat {
    let bone = g.chain()[2];
    bone_entity(scene, root, avatar, bone).map_or(Quat::IDENTITY, |e| {
        (rot(scene, e) * rest(avatar, bone).inverse()).normalize()
    })
}

/// The world position of hint `h`'s middle bone.
pub fn hint_position(scene: &Scene, root: Entity, avatar: &Avatar, h: IkHint) -> Vec3 {
    bone_entity(scene, root, avatar, h.bone()).map_or(Vec3::ZERO, |e| pos(scene, e))
}

fn solve_goals(scene: &mut Scene, root: Entity, avatar: &Avatar, goals: &IkGoals) {
    for g in IkGoal::ALL {
        let goal = goals.goals[g as usize];
        let pw = goal.position_weight.clamp(0.0, 1.0);
        let rw = goal.rotation_weight.clamp(0.0, 1.0);
        if pw <= 0.0 && rw <= 0.0 {
            continue;
        }
        let chain = g.chain();
        let Some(tip) = bone_entity(scene, root, avatar, chain[2]) else {
            continue;
        };
        if pw > 0.0
            && goal.position.is_finite()
            && let (Some(upper), Some(lower)) = (
                bone_entity(scene, root, avatar, chain[0]),
                bone_entity(scene, root, avatar, chain[1]),
            )
        {
            let hint = goals.hints[g as usize];
            let hint = (hint.weight > 0.0 && hint.position.is_finite())
                .then_some((hint.position, hint.weight.clamp(0.0, 1.0)));
            let target = pos(scene, tip).lerp(goal.position, pw);
            solvers::two_bone(scene, [upper, lower, tip], target, hint);
        }
        if rw > 0.0 && goal.rotation.is_finite() {
            let want = (goal.rotation.normalize() * rest(avatar, chain[2])).normalize();
            let r = rot(scene, tip).slerp(want, rw);
            scene.set_world_rotation(tip, r);
        }
    }
    look_at(scene, root, avatar, &goals.look_at);
}

fn look_at(scene: &mut Scene, root: Entity, avatar: &Avatar, la: &LookAt) {
    let w = la.weight.clamp(0.0, 1.0);
    if w <= 0.0 || !la.position.is_finite() {
        return;
    }
    let Some(head) = bone_entity(scene, root, avatar, HumanBone::Head) else {
        return;
    };
    // The head's rest forward, in its own space.
    let axis = rest(avatar, HumanBone::Head).inverse() * Vec3::Z;
    let max = PI * (1.0 - la.clamp_weight.clamp(0.0, 1.0));
    let look = |scene: &Scene| rot(scene, head) * axis;
    let body = (w * la.body_weight).clamp(0.0, 1.0);
    if body > 0.0 {
        let spine: Vec<Entity> = [HumanBone::Spine, HumanBone::Chest]
            .iter()
            .filter_map(|b| bone_entity(scene, root, avatar, *b))
            .collect();
        let share = body / spine.len().max(1) as f32;
        for b in spine {
            let delta = clamped_arc(look(scene), la.position - pos(scene, head), max);
            turn(scene, b, Quat::IDENTITY.slerp(delta, share));
        }
    }
    let hw = (w * la.head_weight).clamp(0.0, 1.0);
    if hw > 0.0 {
        let delta = clamped_arc(look(scene), la.position - pos(scene, head), max);
        turn(scene, head, Quat::IDENTITY.slerp(delta, hw));
    }
}

#[cfg(test)]
#[path = "animator_ik_tests.rs"]
mod tests;
