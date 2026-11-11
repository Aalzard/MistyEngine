//! Ragdolls: the Ragdoll Wizard (colliders, rigidbodies and Character Joints on a skeleton's
//! bones) and switching a ragdoll between animated and physics while playing.
//!
//! The wizard takes thirteen bones (pelvis, hips, knees, feet, arms, elbows, middle spine,
//! head; hands optional), auto-filled from the Animator's humanoid Avatar or the bone names.
//! It gives eleven of them a Rigidbody (masses from typical human body-segment proportions of
//! the total mass), a collider (capsules along the limbs, boxes for the pelvis and chest, a
//! sphere for the head) and a Character Joint to the body above with humanlike limits, and puts
//! a [`Ragdoll`] on the root listing the bones.
//!
//! While a ragdoll is off its bones are kinematic and follow the animation (they still push
//! other bodies). Enabled, they turn dynamic and drive the bones; disabled again they turn
//! kinematic and blend from the physics pose back to the animation over the blend time.

use std::collections::HashSet;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::Physics;
use super::convert::parts;
use crate::animation::humanoid::{Avatar, HumanBone};
use crate::scene::{
    Animator, BoxCollider, CapsuleCollider, CharacterJoint, Interpolation, Ragdoll, Rigidbody,
    Scene, SoftJointLimit, SphereCollider,
};

/// The bones the wizard asks for (Unity's wizard's list plus optional hands).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RagdollBones {
    pub slots: [Option<Entity>; SLOTS],
}

pub const SLOTS: usize = 15;

/// Wizard field labels, in slot order.
pub const LABELS: [&str; SLOTS] = [
    "Pelvis",
    "Left Hips",
    "Left Knee",
    "Left Foot",
    "Right Hips",
    "Right Knee",
    "Right Foot",
    "Left Arm",
    "Left Elbow",
    "Right Arm",
    "Right Elbow",
    "Middle Spine",
    "Head",
    "Left Hand",
    "Right Hand",
];

const PELVIS: usize = 0;
const L_HIPS: usize = 1;
const L_KNEE: usize = 2;
const L_FOOT: usize = 3;
const R_HIPS: usize = 4;
const R_KNEE: usize = 5;
const R_FOOT: usize = 6;
const L_ARM: usize = 7;
const L_ELBOW: usize = 8;
const R_ARM: usize = 9;
const R_ELBOW: usize = 10;
const SPINE: usize = 11;
const HEAD: usize = 12;
const L_HAND: usize = 13;
const R_HAND: usize = 14;

/// Slots that must be filled (hands are optional).
pub const REQUIRED: usize = 13;

/// Human bone of each slot (for auto-fill).
const HUMAN: [HumanBone; SLOTS] = [
    HumanBone::Hips,
    HumanBone::LeftUpperLeg,
    HumanBone::LeftLowerLeg,
    HumanBone::LeftFoot,
    HumanBone::RightUpperLeg,
    HumanBone::RightLowerLeg,
    HumanBone::RightFoot,
    HumanBone::LeftUpperArm,
    HumanBone::LeftLowerArm,
    HumanBone::RightUpperArm,
    HumanBone::RightLowerArm,
    HumanBone::Chest,
    HumanBone::Head,
    HumanBone::LeftHand,
    HumanBone::RightHand,
];

/// Share of the total mass per body (pelvis, thighs, calves, upper arms, forearms, chest,
/// head), from typical human segment proportions; normalised over the eleven bodies.
const MASS: [(usize, f32); 11] = [
    (PELVIS, 0.17),
    (L_HIPS, 0.11),
    (R_HIPS, 0.11),
    (L_KNEE, 0.065),
    (R_KNEE, 0.065),
    (L_ARM, 0.035),
    (R_ARM, 0.035),
    (L_ELBOW, 0.03),
    (R_ELBOW, 0.03),
    (SPINE, 0.20),
    (HEAD, 0.08),
];

impl RagdollBones {
    pub fn get(&self, slot: usize) -> Option<Entity> {
        self.slots.get(slot).copied().flatten()
    }

    pub fn set(&mut self, slot: usize, e: Option<Entity>) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = e;
        }
    }

    /// Labels of required slots that are empty or point at a missing object.
    pub fn missing(&self, scene: &Scene) -> Vec<&'static str> {
        (0..REQUIRED)
            .filter(|i| self.get(*i).is_none_or(|e| !scene.contains(e)))
            .map(|i| LABELS[i])
            .collect()
    }
}

/// Ragdoll Wizard settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RagdollOptions {
    /// Mass of the whole ragdoll (kg).
    pub total_mass: f32,
    /// Break force of the joints (0 = unbreakable).
    pub strength: f32,
    /// The character faces -Z instead of +Z.
    pub flip_forward: bool,
}

impl Default for RagdollOptions {
    fn default() -> Self {
        Self {
            total_mass: 20.0,
            strength: 0.0,
            flip_forward: false,
        }
    }
}

/// What the wizard made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RagdollReport {
    /// The eleven bones that got a body, in slot order.
    pub bodies: Vec<Entity>,
    pub total_mass: f32,
}

/// The object at `path` ("Hips/Spine") below `root`.
fn find_path(scene: &Scene, root: Entity, path: &str) -> Option<Entity> {
    let mut cur = root;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        cur = scene
            .children(cur)
            .into_iter()
            .find(|c| scene.name(*c) == part)?;
    }
    Some(cur)
}

/// Fill the bones from the Animator's humanoid Avatar (or, without one, by mapping the bone
/// names below `root` like Avatar ▸ Create does).
pub fn auto_fill(scene: &Scene, root: Entity) -> RagdollBones {
    let avatar = scene
        .world
        .get::<&Animator>(root)
        .ok()
        .and_then(|a| a.avatar.clone())
        .and_then(|p| Avatar::load(&crate::assets::absolute(&p)).ok())
        .filter(|a| !a.bones.is_empty())
        .unwrap_or_else(|| Avatar::auto_map(scene, root));
    let mut out = RagdollBones::default();
    for (slot, bone) in HUMAN.iter().enumerate() {
        let mut found = avatar
            .bone(*bone)
            .and_then(|b| find_path(scene, root, &b.path));
        // No chest: the spine is the middle spine.
        if found.is_none() && *bone == HumanBone::Chest {
            found = avatar
                .bone(HumanBone::Spine)
                .and_then(|b| find_path(scene, root, &b.path));
        }
        out.set(slot, found);
    }
    out
}

/// Character directions: (right, up, forward) in world space.
fn frame(scene: &Scene, root: Entity, b: &RagdollBones, flip: bool) -> (Vec3, Vec3, Vec3) {
    let rot = scene.world_rotation(root);
    let up = (rot * Vec3::Y).normalize_or(Vec3::Y);
    let mut forward = rot * Vec3::Z;
    if flip {
        forward = -forward;
    }
    let hips = |s| b.get(s).map(|e| scene.world_position(e));
    let right = match (hips(L_HIPS), hips(R_HIPS)) {
        (Some(l), Some(r)) if (r - l).length() > 1e-4 => {
            let r = r - l;
            (r - up * r.dot(up)).normalize_or(rot * Vec3::X)
        }
        _ => up.cross(forward).normalize_or(rot * Vec3::X),
    };
    // Forward perpendicular to right and up, on the side `forward` points to.
    let mut f = right.cross(up).normalize_or(forward);
    if f.dot(forward) < 0.0 {
        f = -f;
    }
    (right, up, f)
}

/// Remove the physics components a previous wizard run left on `e`.
fn clear_bone(scene: &mut Scene, e: Entity) {
    for name in [
        "Character Joint",
        "Box Collider",
        "Sphere Collider",
        "Capsule Collider",
        "Rigidbody",
    ] {
        scene.remove_by_name(e, name);
    }
}

/// Capsule from `e` towards the world point `to`, `radius` × its length.
fn capsule(scene: &Scene, e: Entity, to: Vec3, radius: f32) -> CapsuleCollider {
    let inv = scene.world_matrix(e).inverse();
    let local = inv.transform_point3(to);
    let axis = local.abs().max_position();
    let len = local.length().max(1e-3);
    CapsuleCollider {
        center: local * 0.5,
        radius: len * radius,
        height: len,
        direction: axis as u8,
        ..Default::default()
    }
}

/// Box with world centre `center` and world edge vectors `edges`, in `e`'s local space.
fn local_box(scene: &Scene, e: Entity, center: Vec3, edges: [Vec3; 3]) -> BoxCollider {
    let inv = scene.world_matrix(e).inverse();
    let size = edges
        .iter()
        .map(|v| inv.transform_vector3(*v).abs())
        .fold(Vec3::ZERO, |a, b| a + b);
    BoxCollider {
        center: inv.transform_point3(center),
        size: size.max(Vec3::splat(1e-3)),
        ..Default::default()
    }
}

/// Twist range (low, high) in degrees for a limb pointing along `dir`, where positive values of
/// `range` bend it towards `towards` (rotation about `axis`).
fn signed_range(axis: Vec3, dir: Vec3, towards: Vec3, range: (f32, f32)) -> (f32, f32) {
    if axis.cross(dir).dot(towards) >= 0.0 {
        range
    } else {
        (-range.1, -range.0)
    }
}

/// Run the Ragdoll Wizard on `root` with these bones. Replaces what an earlier run made.
pub fn create(
    scene: &mut Scene,
    root: Entity,
    bones: &RagdollBones,
    options: &RagdollOptions,
) -> Result<RagdollReport, String> {
    let missing = bones.missing(scene);
    if !missing.is_empty() {
        return Err(format!("Missing bones: {}", missing.join(", ")));
    }
    let b = |s: usize| bones.get(s).expect("checked above");
    let positions: Vec<Vec3> = (0..SLOTS)
        .map(|s| {
            bones
                .get(s)
                .filter(|e| scene.contains(*e))
                .map_or(Vec3::ZERO, |e| scene.world_position(e))
        })
        .collect();
    let pos = |s: usize| positions[s];
    let present: Vec<bool> = (0..SLOTS)
        .map(|s| bones.get(s).is_some_and(|e| scene.contains(e)))
        .collect();
    let (right, up, forward) = frame(scene, root, bones, options.flip_forward);

    for (slot, _) in MASS {
        clear_bone(scene, b(slot));
    }

    // Forearm ends: the hands, else as long as the upper arm.
    let l_hand = if present[L_HAND] {
        pos(L_HAND)
    } else {
        pos(L_ELBOW) * 2.0 - pos(L_ARM)
    };
    let r_hand = if present[R_HAND] {
        pos(R_HAND)
    } else {
        pos(R_ELBOW) * 2.0 - pos(R_ARM)
    };

    // Limbs: capsules from each bone to the next.
    let limbs = [
        (L_HIPS, pos(L_KNEE), 0.2),
        (R_HIPS, pos(R_KNEE), 0.2),
        (L_KNEE, pos(L_FOOT), 0.15),
        (R_KNEE, pos(R_FOOT), 0.15),
        (L_ARM, pos(L_ELBOW), 0.25),
        (R_ARM, pos(R_ELBOW), 0.25),
        (L_ELBOW, l_hand, 0.2),
        (R_ELBOW, r_hand, 0.2),
    ];
    for (slot, to, radius) in limbs {
        let c = capsule(scene, b(slot), to, radius);
        scene.add(b(slot), c);
    }

    // Pelvis box: from below the hips up to the middle spine, as wide as the hips.
    let hip_width = (pos(R_HIPS) - pos(L_HIPS)).length().max(0.05);
    let spine_up = (pos(SPINE) - pos(PELVIS)).dot(up).abs().max(0.05);
    let hips_mid = (pos(L_HIPS) + pos(R_HIPS)) * 0.5;
    let bottom = hips_mid.dot(up).min(pos(PELVIS).dot(up)) - hip_width * 0.25;
    let top = pos(PELVIS).dot(up) + spine_up * 0.9;
    let pelvis_center = pos(PELVIS) + up * ((bottom + top) * 0.5 - pos(PELVIS).dot(up));
    let pelvis_box = local_box(
        scene,
        b(PELVIS),
        pelvis_center,
        [
            right * hip_width * 1.6,
            up * (top - bottom).max(0.05),
            forward * hip_width * 0.9,
        ],
    );
    scene.add(b(PELVIS), pelvis_box);

    // Chest box: from the middle spine to the shoulders, as wide as the arms.
    let shoulders = (pos(L_ARM) + pos(R_ARM)) * 0.5;
    let shoulder_width = (pos(R_ARM) - pos(L_ARM)).length().max(0.05);
    let chest_bottom = pos(SPINE).dot(up) - spine_up * 0.1;
    let chest_top = shoulders.dot(up).max(chest_bottom + 0.05);
    let chest_center = pos(SPINE) + up * ((chest_bottom + chest_top) * 0.5 - pos(SPINE).dot(up));
    let chest_box = local_box(
        scene,
        b(SPINE),
        chest_center,
        [
            right * shoulder_width * 0.95,
            up * (chest_top - chest_bottom),
            forward * shoulder_width * 0.5,
        ],
    );
    scene.add(b(SPINE), chest_box);

    // Head sphere above the head bone.
    let neck = (pos(HEAD) - shoulders).length().max(0.05);
    let head_radius = (neck * 0.75).clamp(0.04, 0.5);
    let head_center = pos(HEAD) + up * head_radius * 0.8;
    let head_m = scene.world_matrix(b(HEAD));
    let head_scale = head_m.to_scale_rotation_translation().0.abs().max_element();
    scene.add(
        b(HEAD),
        SphereCollider {
            center: head_m.inverse().transform_point3(head_center),
            radius: head_radius / head_scale.max(1e-4),
            ..Default::default()
        },
    );

    // Bodies.
    let share: f32 = MASS.iter().map(|(_, m)| m).sum();
    let total = options.total_mass.max(1e-3);
    let mut report = RagdollReport {
        total_mass: total,
        ..Default::default()
    };
    for (slot, m) in MASS {
        scene.add(
            b(slot),
            Rigidbody {
                mass: total * m / share,
                interpolation: Interpolation::Interpolate,
                ..Default::default()
            },
        );
        report.bodies.push(b(slot));
    }

    // Joints: (bone, parent body, limb direction, bends towards, twist, swing 1, swing 2).
    let back = -forward;
    let joints = [
        (
            L_HIPS,
            PELVIS,
            pos(L_KNEE) - pos(L_HIPS),
            forward,
            (-20.0, 80.0),
            30.0,
            15.0,
        ),
        (
            R_HIPS,
            PELVIS,
            pos(R_KNEE) - pos(R_HIPS),
            forward,
            (-20.0, 80.0),
            30.0,
            15.0,
        ),
        (
            L_KNEE,
            L_HIPS,
            pos(L_FOOT) - pos(L_KNEE),
            back,
            (-5.0, 120.0),
            5.0,
            5.0,
        ),
        (
            R_KNEE,
            R_HIPS,
            pos(R_FOOT) - pos(R_KNEE),
            back,
            (-5.0, 120.0),
            5.0,
            5.0,
        ),
        (
            L_ARM,
            SPINE,
            pos(L_ELBOW) - pos(L_ARM),
            forward,
            (-60.0, 110.0),
            80.0,
            30.0,
        ),
        (
            R_ARM,
            SPINE,
            pos(R_ELBOW) - pos(R_ARM),
            forward,
            (-60.0, 110.0),
            80.0,
            30.0,
        ),
        (
            L_ELBOW,
            L_ARM,
            l_hand - pos(L_ELBOW),
            forward,
            (-5.0, 130.0),
            5.0,
            5.0,
        ),
        (
            R_ELBOW,
            R_ARM,
            r_hand - pos(R_ELBOW),
            forward,
            (-5.0, 130.0),
            5.0,
            5.0,
        ),
        (SPINE, PELVIS, up, forward, (-25.0, 45.0), 15.0, 15.0),
        (HEAD, SPINE, up, forward, (-40.0, 40.0), 30.0, 25.0),
    ];
    let break_force = if options.strength > 0.0 {
        options.strength
    } else {
        f32::INFINITY
    };
    for (slot, parent, dir, towards, twist, swing1, swing2) in joints {
        let e = b(slot);
        let inv = scene.world_rotation(e).inverse();
        let (low, high) = signed_range(right, dir.normalize_or(-up), towards, twist);
        scene.add(
            e,
            CharacterJoint {
                connected_body: Some(b(parent)),
                axis: (inv * right).normalize_or(Vec3::X),
                swing_axis: (inv * forward).normalize_or(Vec3::Z),
                low_twist_limit: SoftJointLimit::new(low),
                high_twist_limit: SoftJointLimit::new(high),
                swing1_limit: SoftJointLimit::new(swing1),
                swing2_limit: SoftJointLimit::new(swing2),
                break_force,
                break_torque: break_force,
                ..Default::default()
            },
        );
    }

    let keep = scene.world.get::<&Ragdoll>(root).ok().map(|r| (*r).clone());
    scene.add(
        root,
        Ragdoll {
            bones: report.bodies.iter().map(|e| Some(*e)).collect(),
            ..keep.unwrap_or_default()
        },
    );
    Ok(report)
}

/// One playing ragdoll.
struct Run {
    root: Entity,
    /// Bones with bodies, parents before children.
    bones: Vec<Entity>,
    enabled: bool,
    blend_time: f32,
    blend: Option<Blend>,
}

/// Physics → animation blend after disabling.
struct Blend {
    elapsed: f32,
    duration: f32,
    /// Local poses when the ragdoll was disabled.
    from: Vec<(Vec3, Quat)>,
}

/// The playing ragdolls.
#[derive(Default)]
pub struct Ragdolls {
    runs: Vec<Run>,
    bones: HashSet<Entity>,
}

impl Ragdolls {
    /// Is `e` a bone of a ragdoll that is on (written by the ragdoll, not by `sync_out`)?
    pub(super) fn is_bone(&self, e: Entity) -> bool {
        self.bones.contains(&e) && self.runs.iter().any(|r| r.enabled && r.bones.contains(&e))
    }

    fn find(&self, e: Entity) -> Option<usize> {
        self.runs
            .iter()
            .position(|r| r.root == e)
            .or_else(|| self.runs.iter().position(|r| r.bones.contains(&e)))
    }
}

impl Physics {
    /// Ragdolls among `objects` (after their bodies exist): bones start kinematic unless the
    /// ragdoll starts enabled.
    pub(super) fn add_ragdolls(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let Ok(r) = scene.world.get::<&Ragdoll>(e).map(|r| (*r).clone()) else {
                continue;
            };
            let mut bones: Vec<Entity> = r
                .bones
                .iter()
                .flatten()
                .copied()
                .filter(|b| self.bodies.contains_key(b))
                .collect();
            if bones.len() < r.bones.len() {
                self.warnings.push(format!(
                    "'{}': some Ragdoll bones have no Rigidbody.",
                    scene.name(e)
                ));
            }
            scene.sort_hierarchy_order(&mut bones);
            for b in &bones {
                self.set_kinematic(*b, !r.enabled_on_start);
            }
            self.ragdolls.bones.extend(bones.iter().copied());
            self.ragdolls.runs.push(Run {
                root: e,
                bones,
                enabled: r.enabled_on_start,
                blend_time: r.blend_time.max(0.0),
                blend: None,
            });
        }
    }

    /// Switch the ragdoll of `e` (its root or one of its bones) to physics (`true`) or back to
    /// the animation, blending over `blend` seconds (default: the Ragdoll's Blend Time).
    /// False when `e` isn't part of a playing ragdoll.
    pub fn set_ragdoll(
        &mut self,
        scene: &Scene,
        e: Entity,
        enabled: bool,
        blend: Option<f32>,
    ) -> bool {
        let Some(i) = self.ragdolls.find(e) else {
            return false;
        };
        let bones = self.ragdolls.runs[i].bones.clone();
        if self.ragdolls.runs[i].enabled == enabled {
            return true;
        }
        if enabled {
            for b in &bones {
                self.set_kinematic(*b, false);
                if let Some(body) = self.body_mut(*b) {
                    body.wake_up(true);
                }
            }
            let run = &mut self.ragdolls.runs[i];
            run.enabled = true;
            run.blend = None;
        } else {
            let from = bones
                .iter()
                .map(|b| {
                    // A bone destroyed while the ragdoll played has no pose to blend from
                    // (the blend skips it).
                    scene
                        .world
                        .get::<&crate::scene::Transform>(*b)
                        .map_or((Vec3::ZERO, Quat::IDENTITY), |t| (t.position, t.rotation))
                })
                .collect();
            for b in &bones {
                self.set_kinematic(*b, true);
            }
            let run = &mut self.ragdolls.runs[i];
            run.enabled = false;
            let duration = blend.unwrap_or(run.blend_time).max(0.0);
            run.blend = (duration > 0.0).then_some(Blend {
                elapsed: 0.0,
                duration,
                from,
            });
        }
        true
    }

    /// Is the ragdoll of `e` under physics? `None` when `e` isn't part of a playing ragdoll.
    pub fn ragdoll_enabled(&self, e: Entity) -> Option<bool> {
        self.ragdolls.find(e).map(|i| self.ragdolls.runs[i].enabled)
    }

    /// Enabled ragdolls write their bodies' poses into the bones, parents first.
    pub(super) fn pose_ragdolls(&self, scene: &mut Scene) {
        for run in self.ragdolls.runs.iter().filter(|r| r.enabled) {
            for b in &run.bones {
                if !scene.contains(*b) {
                    continue;
                }
                if let Some(body) = self.body(*b) {
                    let (p, r) = parts(body.position());
                    scene.set_world_position(*b, p);
                    scene.set_world_rotation(*b, r);
                }
            }
        }
    }

    /// After the Animator posed the bones this frame: enabled ragdolls put their physics pose
    /// back, blending ones mix the physics pose into the animation; then cloth is stepped.
    pub fn after_animation(&mut self, scene: &mut Scene, dt: f32) {
        self.pose_ragdolls(scene);
        for run in &mut self.ragdolls.runs {
            let Some(blend) = run.blend.as_mut() else {
                continue;
            };
            blend.elapsed += dt;
            let t = (blend.elapsed / blend.duration.max(1e-4)).clamp(0.0, 1.0);
            for (b, (p0, r0)) in run.bones.iter().zip(&blend.from) {
                if !scene.contains(*b) {
                    continue;
                }
                let mut tr = scene.transform(*b);
                tr.position = p0.lerp(tr.position, t);
                tr.set_rotation(r0.slerp(tr.rotation, t));
                scene.set_transform(*b, tr);
            }
            if t >= 1.0 {
                run.blend = None;
            }
        }
        self.update_cloth(scene, dt);
    }
}

/// A standing humanoid skeleton of bones for tests ("Character" with "Hips"…"RightFoot").
#[cfg(test)]
pub(crate) fn test_skeleton(scene: &mut Scene) -> Entity {
    tests::skeleton(scene)
}

#[cfg(test)]
#[path = "ragdoll_tests.rs"]
mod tests;
