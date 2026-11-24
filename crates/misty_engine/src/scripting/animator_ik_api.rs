//! Lua side of the Animator IK pass, called from `OnAnimatorIK(layerIndex)`:
//! `SetIKPosition(goal, pos)`, `SetIKRotation(goal, euler)`, `SetIKPositionWeight`,
//! `SetIKRotationWeight`, `GetIKPosition`, `GetIKRotation`, `GetIKPositionWeight`,
//! `GetIKRotationWeight`, `SetIKHintPosition(hint, pos)`, `SetIKHintPositionWeight`,
//! `GetIKHintPosition`, `GetIKHintPositionWeight`, `SetLookAtPosition(pos)`,
//! `SetLookAtWeight(weight [, body, head, eyes, clamp])` and `GetBoneTransform(bone)`.
//!
//! Goals are `AvatarIKGoal.LeftFoot / RightFoot / LeftHand / RightHand` (0 … 3, or the name as
//! text), hints `AvatarIKHint.LeftKnee / RightKnee / LeftElbow / RightElbow`, bones
//! `HumanBodyBones.Hips …` (the familiar engine's numbers 0 … 20). Rotations are Euler degrees.
//! See `rigging::animator_ik` for how goals are solved.

use hecs::Entity;
use mlua::{Lua, Result, Table, UserDataMethods, Value};

use super::World;
use super::animator_api::LAnimator;
use super::api::{LTransform, LVec3, with_world};
use crate::animation::humanoid::HumanBone;
use crate::camera::{euler_to_quat, quat_to_euler};
use crate::rigging::animator_ik::{self as ik, IkGoal, IkHint};

fn bad(what: &str, v: &Value) -> mlua::Error {
    mlua::Error::runtime(format!("{what}: unknown value {v:?}"))
}

fn goal_of(v: &Value) -> Result<IkGoal> {
    match v {
        Value::Integer(i) => IkGoal::from_index(*i),
        Value::Number(n) => IkGoal::from_index(*n as i64),
        Value::String(s) => IkGoal::from_name(&s.to_string_lossy()),
        _ => None,
    }
    .ok_or_else(|| bad("AvatarIKGoal", v))
}

fn hint_of(v: &Value) -> Result<IkHint> {
    match v {
        Value::Integer(i) => IkHint::from_index(*i),
        Value::Number(n) => IkHint::from_index(*n as i64),
        Value::String(s) => IkHint::from_name(&s.to_string_lossy()),
        _ => None,
    }
    .ok_or_else(|| bad("AvatarIKHint", v))
}

fn bone_of(v: &Value) -> Result<HumanBone> {
    match v {
        Value::Integer(i) => ik::human_body_bone(*i),
        Value::Number(n) => ik::human_body_bone(*n as i64),
        Value::String(s) => HumanBone::from_name(&s.to_string_lossy()),
        _ => None,
    }
    .ok_or_else(|| bad("HumanBodyBones", v))
}

/// `f` on the goals of `e` (the object must be alive).
fn goals<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut ik::IkGoals) -> R) -> Result<R> {
    with_world(lua, |w| {
        if !w.scene.contains(e) {
            return Err(mlua::Error::runtime("object was destroyed"));
        }
        Ok(f(w.animator.ik.goals_mut(e)))
    })?
}

/// `f` on the scene, `e` and the Avatar its IK uses.
fn posed<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&World, &crate::animation::humanoid::Avatar) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        if !w.scene.contains(e) {
            return Err(mlua::Error::runtime("object was destroyed"));
        }
        let avatar = w.animator.ik_avatar(&w.scene, e);
        Ok(f(w, &avatar))
    })?
}

/// The IK methods of `Animator` (added to `LAnimator`'s methods).
pub fn add_methods<M: UserDataMethods<LAnimator>>(m: &mut M) {
    // World position the hand / foot reaches for (with SetIKPositionWeight > 0; call in OnAnimatorIK).
    m.add_method(
        "SetIKPosition",
        |lua, a, (goal, position): (Value, LVec3)| {
            let g = goal_of(&goal)?;
            goals(lua, a.0, |x| {
                let goal = x.goal_mut(g);
                goal.position = position.0;
                goal.position_set = true;
            })
        },
    );
    // Effector rotation (Euler degrees) the hand / foot turns to; identity = the Avatar's rest pose.
    m.add_method("SetIKRotation", |lua, a, (goal, euler): (Value, LVec3)| {
        let g = goal_of(&goal)?;
        goals(lua, a.0, |x| {
            let goal = x.goal_mut(g);
            goal.rotation = euler_to_quat(euler.0);
            goal.rotation_set = true;
        })
    });
    // 0 … 1: how far from the animated pose towards the goal position (reset to 0 every frame).
    m.add_method(
        "SetIKPositionWeight",
        |lua, a, (goal, weight): (Value, f32)| {
            let g = goal_of(&goal)?;
            goals(lua, a.0, |x| {
                x.goal_mut(g).position_weight = weight.clamp(0.0, 1.0)
            })
        },
    );
    // 0 … 1: how far from the animated rotation towards the goal rotation (reset every frame).
    m.add_method(
        "SetIKRotationWeight",
        |lua, a, (goal, weight): (Value, f32)| {
            let g = goal_of(&goal)?;
            goals(lua, a.0, |x| {
                x.goal_mut(g).rotation_weight = weight.clamp(0.0, 1.0)
            })
        },
    );
    m.add_method("GetIKPositionWeight", |lua, a, goal: Value| {
        let g = goal_of(&goal)?;
        goals(lua, a.0, |x| x.goal_mut(g).position_weight)
    });
    m.add_method("GetIKRotationWeight", |lua, a, goal: Value| {
        let g = goal_of(&goal)?;
        goals(lua, a.0, |x| x.goal_mut(g).rotation_weight)
    });
    // The goal set this frame, or where the animation put the hand / foot.
    m.add_method("GetIKPosition", |lua, a, goal: Value| {
        let g = goal_of(&goal)?;
        let set = goals(lua, a.0, |x| {
            let goal = *x.goal_mut(g);
            goal.position_set.then_some(goal.position)
        })?;
        match set {
            Some(p) => Ok(LVec3(p)),
            None => posed(lua, a.0, |w, av| {
                LVec3(ik::effector_position(&w.scene, a.0, av, g))
            }),
        }
    });
    m.add_method("GetIKRotation", |lua, a, goal: Value| {
        let g = goal_of(&goal)?;
        let set = goals(lua, a.0, |x| {
            let goal = *x.goal_mut(g);
            goal.rotation_set.then_some(goal.rotation)
        })?;
        let q = match set {
            Some(q) => q,
            None => posed(lua, a.0, |w, av| {
                ik::effector_rotation(&w.scene, a.0, av, g)
            })?,
        };
        Ok(LVec3(quat_to_euler(q)))
    });
    // Where the elbow / knee should point (with SetIKHintPositionWeight > 0).
    m.add_method(
        "SetIKHintPosition",
        |lua, a, (hint, position): (Value, LVec3)| {
            let h = hint_of(&hint)?;
            goals(lua, a.0, |x| {
                let hint = x.hint_mut(h);
                hint.position = position.0;
                hint.set = true;
            })
        },
    );
    // 0 … 1: how much the hint turns the limb's bend.
    m.add_method(
        "SetIKHintPositionWeight",
        |lua, a, (hint, weight): (Value, f32)| {
            let h = hint_of(&hint)?;
            goals(lua, a.0, |x| x.hint_mut(h).weight = weight.clamp(0.0, 1.0))
        },
    );
    m.add_method("GetIKHintPositionWeight", |lua, a, hint: Value| {
        let h = hint_of(&hint)?;
        goals(lua, a.0, |x| x.hint_mut(h).weight)
    });
    m.add_method("GetIKHintPosition", |lua, a, hint: Value| {
        let h = hint_of(&hint)?;
        let set = goals(lua, a.0, |x| {
            let hint = *x.hint_mut(h);
            hint.set.then_some(hint.position)
        })?;
        match set {
            Some(p) => Ok(LVec3(p)),
            None => posed(lua, a.0, |w, av| {
                LVec3(ik::hint_position(&w.scene, a.0, av, h))
            }),
        }
    });
    // World point the head (and with body weight the spine) turns towards.
    m.add_method("SetLookAtPosition", |lua, a, position: LVec3| {
        goals(lua, a.0, |x| x.look_at.position = position.0)
    });
    // SetLookAtWeight(weight [, bodyWeight = 0, headWeight = 1, eyesWeight = 0, clampWeight = 0.5]).
    m.add_method(
        "SetLookAtWeight",
        |lua,
         a,
         (weight, body, head, eyes, clamp): (
            f32,
            Option<f32>,
            Option<f32>,
            Option<f32>,
            Option<f32>,
        )| {
            goals(lua, a.0, |x| {
                let la = &mut x.look_at;
                la.weight = weight.clamp(0.0, 1.0);
                la.body_weight = body.unwrap_or(0.0).clamp(0.0, 1.0);
                la.head_weight = head.unwrap_or(1.0).clamp(0.0, 1.0);
                la.eyes_weight = eyes.unwrap_or(0.0).clamp(0.0, 1.0);
                la.clamp_weight = clamp.unwrap_or(0.5).clamp(0.0, 1.0);
            })
        },
    );
    // The Transform of a human bone (nil when the rig has none).
    m.add_method("GetBoneTransform", |lua, a, bone: Value| {
        let b = bone_of(&bone)?;
        posed(lua, a.0, |w, av| {
            ik::bone_entity(&w.scene, a.0, av, b).map(LTransform)
        })
    });
}

/// `AvatarIKGoal`, `AvatarIKHint` and `HumanBodyBones` constants.
pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let goals: Table = lua.create_table()?;
    for x in IkGoal::ALL {
        goals.set(x.name(), x as i64)?;
    }
    g.set("AvatarIKGoal", goals)?;
    let hints: Table = lua.create_table()?;
    for x in IkHint::ALL {
        hints.set(x.name(), x as i64)?;
    }
    g.set("AvatarIKHint", hints)?;
    let bones: Table = lua.create_table()?;
    for i in 0..=20 {
        if let Some(b) = ik::human_body_bone(i) {
            bones.set(b.name(), i)?;
        }
    }
    g.set("HumanBodyBones", bones)?;
    Ok(())
}
