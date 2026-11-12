//! Humanoid avatars and retargeting.
//!
//! An Avatar (`.avatar`, JSON) maps a rig's bones to a standard human skeleton
//! ([`HumanBone`]) and remembers the rig's rest pose. A clip made for one rig is turned into a
//! humanoid clip ([`humanoid_clip`]): its curves become, per human bone, the bone's rotation
//! change from rest in the Animator's space (quaternion curves on path "@LeftUpperArm") plus the
//! hips' move in hips heights. An Animator with an Avatar plays humanoid clips on its own rig
//! ([`retarget`]): each bone gets the same change from its own rest pose, so rigs with other
//! bone names, axes and proportions move alike. Bones between mapped ones keep their rest pose.

use std::collections::HashMap;
use std::path::Path;

use glam::{Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::clip::{AnimationClip, Property};
use super::curve::TangentMode;
use crate::camera::{euler_to_quat, quat_to_euler};
use crate::scene::Scene;

/// File extension of avatars.
pub const AVATAR_EXTENSION: &str = "avatar";

pub fn is_avatar(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(AVATAR_EXTENSION))
}

/// Path prefix of humanoid curves ("@Hips").
pub const HUMAN_PREFIX: char = '@';

/// The standard human skeleton, parents before children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HumanBone {
    Hips,
    Spine,
    Chest,
    Neck,
    Head,
    LeftShoulder,
    LeftUpperArm,
    LeftLowerArm,
    LeftHand,
    RightShoulder,
    RightUpperArm,
    RightLowerArm,
    RightHand,
    LeftUpperLeg,
    LeftLowerLeg,
    LeftFoot,
    LeftToes,
    RightUpperLeg,
    RightLowerLeg,
    RightFoot,
    RightToes,
}

impl HumanBone {
    pub const ALL: [Self; 21] = [
        Self::Hips,
        Self::Spine,
        Self::Chest,
        Self::Neck,
        Self::Head,
        Self::LeftShoulder,
        Self::LeftUpperArm,
        Self::LeftLowerArm,
        Self::LeftHand,
        Self::RightShoulder,
        Self::RightUpperArm,
        Self::RightLowerArm,
        Self::RightHand,
        Self::LeftUpperLeg,
        Self::LeftLowerLeg,
        Self::LeftFoot,
        Self::LeftToes,
        Self::RightUpperLeg,
        Self::RightLowerLeg,
        Self::RightFoot,
        Self::RightToes,
    ];

    pub fn name(self) -> String {
        format!("{self:?}")
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }

    /// The bone above it in the standard skeleton.
    pub fn parent(self) -> Option<Self> {
        use HumanBone::*;
        Some(match self {
            Hips => return None,
            Spine | LeftUpperLeg | RightUpperLeg => Hips,
            Chest => Spine,
            Neck | LeftShoulder | RightShoulder => Chest,
            Head => Neck,
            LeftUpperArm => LeftShoulder,
            LeftLowerArm => LeftUpperArm,
            LeftHand => LeftLowerArm,
            RightUpperArm => RightShoulder,
            RightLowerArm => RightUpperArm,
            RightHand => RightLowerArm,
            LeftLowerLeg => LeftUpperLeg,
            LeftFoot => LeftLowerLeg,
            LeftToes => LeftFoot,
            RightLowerLeg => RightUpperLeg,
            RightFoot => RightLowerLeg,
            RightToes => RightFoot,
        })
    }

    /// Bones an avatar needs (the others are optional).
    pub fn required(self) -> bool {
        use HumanBone::*;
        !matches!(
            self,
            Chest | Neck | LeftShoulder | RightShoulder | LeftToes | RightToes
        )
    }

    /// Curve path of this bone in humanoid clips.
    pub fn curve_path(self) -> String {
        format!("{HUMAN_PREFIX}{}", self.name())
    }
}

/// One mapped bone and its rest pose (rotations in the Animator's space).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvatarBone {
    pub bone: HumanBone,
    /// Path below the Animator's object.
    pub path: String,
    /// Rest rotation of the bone, relative to the Animator's object.
    pub rest: [f32; 4],
    /// Rest rotation of the bone's actual parent, relative to the Animator's object.
    pub parent_rest: [f32; 4],
    /// Rest local position.
    pub rest_position: [f32; 3],
}

impl AvatarBone {
    fn rest(&self) -> Quat {
        Quat::from_array(self.rest).normalize()
    }

    fn parent_rest(&self) -> Quat {
        Quat::from_array(self.parent_rest).normalize()
    }

    /// Rest local rotation.
    fn rest_local(&self) -> Quat {
        self.parent_rest().inverse() * self.rest()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Avatar {
    pub bones: Vec<AvatarBone>,
    /// Height of the hips above the Animator's object at rest (scales hip movement).
    pub hips_height: f32,
}

/// Lower-case name without non-alphanumerics, and the side it names (-1 left, 1 right).
fn normalise(name: &str) -> (String, i32) {
    let lower = name.to_lowercase();
    let lower = lower.rsplit([':', '|']).next().unwrap_or(&lower).to_owned();
    let words: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let mut side = 0;
    let mut kept = Vec::new();
    for w in words {
        match w.as_str() {
            "l" | "left" => side = -1,
            "r" | "right" => side = 1,
            _ => {
                let w = if let Some(rest) = w.strip_prefix("left") {
                    side = -1;
                    rest.to_owned()
                } else if let Some(rest) = w.strip_prefix("right") {
                    side = 1;
                    rest.to_owned()
                } else {
                    w
                };
                kept.push(w);
            }
        }
    }
    let mut joined: String = kept.concat();
    // "bip01" style prefixes.
    for p in ["bip01", "bip001", "mixamorig", "def", "armature"] {
        if let Some(rest) = joined.strip_prefix(p)
            && !rest.is_empty()
        {
            joined = rest.to_owned();
        }
    }
    (joined, side)
}

/// The human bone a normalised name stands for (sides resolved by the caller).
fn classify(base: &str) -> Option<(&'static str, bool)> {
    // (kind, sided)
    let has = |words: &[&str]| words.contains(&base);
    let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit());
    let starts = |w: &str| trimmed == w;
    if has(&["hips", "hip", "pelvis"]) {
        return Some(("hips", false));
    }
    if starts("spine") {
        return Some(("spine", false));
    }
    if has(&["chest", "upperchest", "torso"]) {
        return Some(("chest", false));
    }
    if starts("neck") {
        return Some(("neck", false));
    }
    if has(&["head"]) {
        return Some(("head", false));
    }
    if has(&["shoulder", "clavicle", "collar"]) {
        return Some(("shoulder", true));
    }
    if has(&["upperarm", "arm", "uparm"]) {
        return Some(("upperarm", true));
    }
    if has(&["lowerarm", "forearm", "elbow"]) {
        return Some(("lowerarm", true));
    }
    if has(&["hand", "wrist"]) {
        return Some(("hand", true));
    }
    if has(&["upperleg", "upleg", "thigh"]) {
        return Some(("upperleg", true));
    }
    if has(&["lowerleg", "leg", "calf", "shin", "knee"]) {
        return Some(("lowerleg", true));
    }
    if has(&["foot", "ankle"]) {
        return Some(("foot", true));
    }
    if has(&["toes", "toe", "toebase"]) {
        return Some(("toes", true));
    }
    None
}

fn bone_for(kind: &str, side: i32) -> Option<HumanBone> {
    use HumanBone::*;
    let pick = |l, r| match side {
        -1 => Some(l),
        1 => Some(r),
        _ => None,
    };
    match kind {
        "hips" => Some(Hips),
        "spine" => Some(Spine),
        "chest" => Some(Chest),
        "neck" => Some(Neck),
        "head" => Some(Head),
        "shoulder" => pick(LeftShoulder, RightShoulder),
        "upperarm" => pick(LeftUpperArm, RightUpperArm),
        "lowerarm" => pick(LeftLowerArm, RightLowerArm),
        "hand" => pick(LeftHand, RightHand),
        "upperleg" => pick(LeftUpperLeg, RightUpperLeg),
        "lowerleg" => pick(LeftLowerLeg, RightLowerLeg),
        "foot" => pick(LeftFoot, RightFoot),
        "toes" => pick(LeftToes, RightToes),
        _ => None,
    }
}

/// Rotation of `e` relative to `root` (product of the local rotations between them).
fn rotation_below(scene: &Scene, root: Entity, e: Entity) -> Quat {
    let mut q = Quat::IDENTITY;
    let mut cur = Some(e);
    while let Some(c) = cur {
        if c == root {
            break;
        }
        q = scene.transform(c).rotation * q;
        cur = scene.parent(c);
    }
    q
}

impl Avatar {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(&crate::asset_db::resolve_json(text)).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn bone(&self, b: HumanBone) -> Option<&AvatarBone> {
        self.bones.iter().find(|x| x.bone == b)
    }

    /// Required bones the avatar lacks.
    pub fn missing(&self) -> Vec<HumanBone> {
        HumanBone::ALL
            .into_iter()
            .filter(|b| b.required() && self.bone(*b).is_none())
            .collect()
    }

    /// The nearest mapped bone above `b` in the standard skeleton.
    fn mapped_parent(&self, b: HumanBone) -> Option<&AvatarBone> {
        let mut cur = b.parent();
        while let Some(p) = cur {
            if let Some(x) = self.bone(p) {
                return Some(x);
            }
            cur = p.parent();
        }
        None
    }

    /// Map the hierarchy below `root` by bone names (Hips / pelvis, Spine, Chest, Neck, Head,
    /// shoulder / clavicle, upper arm, forearm, hand, thigh / up leg, leg / calf, foot, toes; left
    /// and right from "Left", "L_", "_R", ".l"…) and record its current pose as the rest pose.
    pub fn auto_map(scene: &Scene, root: Entity) -> Avatar {
        let mut out = Avatar::default();
        let mut order = Vec::new();
        fn walk(scene: &Scene, e: Entity, prefix: &str, out: &mut Vec<(Entity, String)>) {
            for c in scene.children(e) {
                let name = scene.name(c);
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                out.push((c, path.clone()));
                walk(scene, c, &path, out);
            }
        }
        walk(scene, root, "", &mut order);
        let mut spines = 0;
        for (e, path) in &order {
            let name = path.rsplit('/').next().unwrap_or(path);
            let (base, side) = normalise(name);
            let Some((kind, _)) = classify(&base) else {
                continue;
            };
            // A second spine bone is the chest.
            let kind = if kind == "spine" {
                spines += 1;
                if spines > 1 { "chest" } else { "spine" }
            } else {
                kind
            };
            let Some(bone) = bone_for(kind, side) else {
                continue;
            };
            if out.bone(bone).is_some() {
                continue;
            }
            // A bone must sit below its mapped standard parent.
            if let Some(p) = out.mapped_parent(bone)
                && !super::mask::path_within(path, &p.path)
            {
                continue;
            }
            let rest = rotation_below(scene, root, *e);
            let parent_rest = scene
                .parent(*e)
                .map_or(Quat::IDENTITY, |p| rotation_below(scene, root, p));
            out.bones.push(AvatarBone {
                bone,
                path: path.clone(),
                rest: rest.to_array(),
                parent_rest: parent_rest.to_array(),
                rest_position: scene.transform(*e).position.to_array(),
            });
            if bone == HumanBone::Hips {
                let root_inv = scene.world_matrix(root).inverse();
                out.hips_height = root_inv
                    .transform_point3(scene.world_position(*e))
                    .y
                    .abs()
                    .max(1e-3);
            }
        }
        out.bones.sort_by_key(|b| b.bone);
        out
    }

    /// Animated rotations (relative to the Animator) of the mapped bones, from their animated
    /// local rotations (`local`; missing = rest). Bones between mapped ones stay at rest.
    fn globals(&self, local: &HashMap<HumanBone, Quat>) -> HashMap<HumanBone, Quat> {
        let mut out: HashMap<HumanBone, Quat> = HashMap::new();
        for b in &self.bones {
            let parent = self.parent_space(b, &out);
            let l = local
                .get(&b.bone)
                .copied()
                .unwrap_or_else(|| b.rest_local());
            out.insert(b.bone, parent * l);
        }
        out
    }

    /// Animated rotation of `b`'s actual parent, from its nearest mapped ancestor's.
    fn parent_space(&self, b: &AvatarBone, globals: &HashMap<HumanBone, Quat>) -> Quat {
        match self.mapped_parent(b.bone) {
            Some(h) => {
                let g = globals.get(&h.bone).copied().unwrap_or_else(|| h.rest());
                g * h.rest().inverse() * b.parent_rest()
            }
            None => b.parent_rest(),
        }
    }
}

/// Turn a clip made for `avatar`'s rig into a humanoid clip, sampled `rate` times a second.
/// The object's own curves (path "", root motion) are kept; other non-human curves are dropped.
pub fn humanoid_clip(clip: &AnimationClip, avatar: &Avatar, rate: f32) -> AnimationClip {
    let mut out = AnimationClip {
        length: clip.length,
        looping: clip.looping,
        sample_rate: clip.sample_rate,
        events: clip.events.clone(),
        curves: clip
            .curves
            .iter()
            .filter(|c| c.path.is_empty())
            .cloned()
            .collect(),
    };
    let length = crate::animation::import_length(clip.length);
    let steps = ((length * rate.clamp(1.0, 1000.0)).ceil() as usize).max(1);
    let mut previous: HashMap<HumanBone, Quat> = HashMap::new();
    for i in 0..=steps {
        let t = length * i as f32 / steps as f32;
        let mut local = HashMap::new();
        for b in &avatar.bones {
            let has = (0..3).any(|a| clip.curve(&b.path, Property::Rotation(a)).is_some());
            if !has {
                continue;
            }
            let rest = quat_to_euler(b.rest_local());
            let e = Vec3::new(
                clip.curve(&b.path, Property::Rotation(0))
                    .map_or(rest.x, |c| c.evaluate(t)),
                clip.curve(&b.path, Property::Rotation(1))
                    .map_or(rest.y, |c| c.evaluate(t)),
                clip.curve(&b.path, Property::Rotation(2))
                    .map_or(rest.z, |c| c.evaluate(t)),
            );
            local.insert(b.bone, euler_to_quat(e));
        }
        let globals = avatar.globals(&local);
        for b in &avatar.bones {
            if !local.contains_key(&b.bone) {
                continue;
            }
            let mut d = (globals[&b.bone] * b.rest().inverse()).normalize();
            // Keep neighbouring samples on the same side so they interpolate the short way.
            if previous.get(&b.bone).is_some_and(|p| p.dot(d) < 0.0) {
                d = -d;
            }
            previous.insert(b.bone, d);
            for (k, v) in d.to_array().into_iter().enumerate() {
                out.set_key(&b.bone.curve_path(), Property::HumanRotation(k as u8), t, v);
            }
        }
        if let Some(h) = avatar.bone(HumanBone::Hips) {
            let has = (0..3).any(|a| clip.curve(&h.path, Property::Position(a)).is_some());
            if has {
                let p = Vec3::from(h.rest_position);
                let now = Vec3::new(
                    clip.curve(&h.path, Property::Position(0))
                        .map_or(p.x, |c| c.evaluate(t)),
                    clip.curve(&h.path, Property::Position(1))
                        .map_or(p.y, |c| c.evaluate(t)),
                    clip.curve(&h.path, Property::Position(2))
                        .map_or(p.z, |c| c.evaluate(t)),
                );
                let d = h.parent_rest() * (now - p) / avatar.hips_height.max(1e-3);
                for (k, v) in d.to_array().into_iter().enumerate() {
                    out.set_key(
                        &HumanBone::Hips.curve_path(),
                        Property::HumanPosition(k as u8),
                        t,
                        v,
                    );
                }
            }
        }
    }
    for c in out
        .curves
        .iter_mut()
        .filter(|c| c.path.starts_with(HUMAN_PREFIX))
    {
        for k in 0..c.keys.len() {
            c.keys.set_tangent(k, TangentMode::Linear);
        }
    }
    out
}

/// Whether a clip is a humanoid clip (has "@Bone" curves).
pub fn is_humanoid(clip: &AnimationClip) -> bool {
    clip.curves.iter().any(|c| c.path.starts_with(HUMAN_PREFIX))
}

/// Local values for `avatar`'s rig from a humanoid clip at time `t`: (bone path, property,
/// value) for the rotations (Euler) of the bones the clip animates and the hips position.
pub fn retarget(clip: &AnimationClip, avatar: &Avatar, t: f32) -> Vec<(String, Property, f32)> {
    let mut deltas: HashMap<HumanBone, Quat> = HashMap::new();
    for b in HumanBone::ALL {
        let path = b.curve_path();
        let comps: Vec<f32> = (0..4)
            .filter_map(|k| clip.curve(&path, Property::HumanRotation(k)))
            .map(|c| c.evaluate(t))
            .collect();
        if comps.len() == 4 {
            let q = Quat::from_xyzw(comps[0], comps[1], comps[2], comps[3]);
            if q.length_squared() > 1e-8 {
                deltas.insert(b, q.normalize());
            }
        }
    }
    let mut out = Vec::new();
    let mut globals: HashMap<HumanBone, Quat> = HashMap::new();
    for b in &avatar.bones {
        let parent = avatar.parent_space(b, &globals);
        let g = match deltas.get(&b.bone) {
            Some(d) => *d * b.rest(),
            None => parent * b.rest_local(),
        };
        globals.insert(b.bone, g);
        if deltas.contains_key(&b.bone) {
            let e = quat_to_euler(parent.inverse() * g);
            for (k, v) in e.to_array().into_iter().enumerate() {
                out.push((b.path.clone(), Property::Rotation(k as u8), v));
            }
        }
    }
    if let Some(h) = avatar.bone(HumanBone::Hips) {
        let path = HumanBone::Hips.curve_path();
        let comps: Vec<f32> = (0..3)
            .filter_map(|k| clip.curve(&path, Property::HumanPosition(k)))
            .map(|c| c.evaluate(t))
            .collect();
        if comps.len() == 3 {
            let d = Vec3::new(comps[0], comps[1], comps[2]) * avatar.hips_height;
            let local = h.parent_rest().inverse() * d + Vec3::from(h.rest_position);
            for (k, v) in local.to_array().into_iter().enumerate() {
                out.push((h.path.clone(), Property::Position(k as u8), v));
            }
        }
    }
    out
}
