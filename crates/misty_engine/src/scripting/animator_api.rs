//! Lua `Animator`: parameters (Set/Get Float with optional damping, Integer, Bool,
//! SetTrigger/ResetTrigger), layers (`layerCount`, `GetLayerName`, `GetLayerIndex`,
//! `SetLayerWeight`, `GetLayerWeight`), `Play`, `CrossFade`, `CrossFadeInFixedTime`,
//! `GetCurrentAnimatorStateInfo(layer)`, `GetNextAnimatorStateInfo(layer)`,
//! `IsInTransition(layer)`, root motion (`applyRootMotion`, `deltaPosition`, `deltaRotation`,
//! `velocity`, `ApplyBuiltinRootMotion`), target matching (`MatchTarget`,
//! `InterruptMatchTarget`, `isMatchingTarget`), `speed` and `enabled`.
//!
//! There is no Quaternion type in Lua: rotations are Euler angles in degrees (Vector3).

use glam::{Quat, Vec3};
use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::World;
use super::api::{LGameObject, LVec3, with_world};
use crate::animation::animator::{self, AnimatorStatus, StateInfo};
use crate::animation::controller::ParamKind;
use crate::camera::{euler_to_quat, quat_to_euler};
use crate::scene::{Animator, Scene};

#[derive(Clone, Copy)]
pub struct LAnimator(pub Entity);

fn missing() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

/// Layer index for state queries: 0 … layerCount-1 (no controller: only 0 is accepted).
fn layer_index(lua: &Lua, e: Entity, layer: Option<i64>) -> Result<usize> {
    let n = with_world(lua, |w| {
        let World {
            animator, scene, ..
        } = w;
        animator.layer_count(scene, e)
    })?
    .max(1);
    match layer.unwrap_or(0) {
        l if l >= 0 && (l as usize) < n => Ok(l as usize),
        l => Err(mlua::Error::runtime(format!(
            "Animator layer index {l} is out of bounds (the controller has {n} layer{})",
            if n == 1 { "" } else { "s" }
        ))),
    }
}

/// Layer for Play / CrossFade: -1 (or nothing) = the first layer with that state.
fn play_layer(lua: &Lua, e: Entity, layer: Option<i64>) -> Result<i64> {
    match layer {
        None | Some(-1) => Ok(-1),
        Some(l) => layer_index(lua, e, Some(l)).map(|l| l as i64),
    }
}

fn set(lua: &Lua, e: Entity, name: &str, kinds: &[ParamKind], v: f32) -> Result<()> {
    with_world(lua, |w| {
        let World {
            animator, scene, ..
        } = w;
        animator.set_param(scene, e, name, kinds, v)
    })
}

fn get(lua: &Lua, e: Entity, name: &str, kinds: &[ParamKind]) -> Result<f32> {
    with_world(lua, |w| {
        let World {
            animator, scene, ..
        } = w;
        animator.param(scene, e, name, kinds)
    })
}

fn status(lua: &Lua, e: Entity) -> Result<Option<AnimatorStatus>> {
    with_world(lua, |w| {
        let World {
            animator, scene, ..
        } = w;
        animator.status(scene, e)
    })
}

/// The last string among `args` (works for both `t:F("x")` and `t.F("x")`).
fn last_string(args: &Variadic<Value>) -> Option<String> {
    args.iter().rev().find_map(|v| match v {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    })
}

/// An AnimatorStateInfo-like table: `name`, `fullPath`, `tag`, `normalizedTime`, `length`,
/// `speed`, `loop`, `IsName(name)` and `IsTag(tag)`.
fn state_table(lua: &Lua, layer_name: &str, info: Option<StateInfo>) -> Result<Table> {
    let t = lua.create_table()?;
    let info = info.unwrap_or(StateInfo {
        name: String::new(),
        full_path: String::new(),
        tag: String::new(),
        normalized_time: 0.0,
        length: 0.0,
        speed: 1.0,
        looping: false,
    });
    t.set("name", info.name.as_str())?;
    t.set("fullPath", info.full_path.as_str())?;
    t.set("tag", info.tag.as_str())?;
    t.set("normalizedTime", info.normalized_time)?;
    t.set("length", info.length)?;
    t.set("speed", info.speed)?;
    t.set("loop", info.looping)?;
    // "Idle", "Base Layer.Idle" and the full path ("Base Layer.Combat.Idle") all match.
    let (name, full, layer) = (info.name, info.full_path, layer_name.to_owned());
    t.set(
        "IsName",
        lua.create_function(move |_, args: Variadic<Value>| {
            Ok(last_string(&args).is_some_and(|a| {
                !name.is_empty() && (a == name || a == full || a == format!("{layer}.{name}"))
            }))
        })?,
    )?;
    let tag = info.tag;
    t.set(
        "IsTag",
        lua.create_function(move |_, args: Variadic<Value>| {
            Ok(last_string(&args).is_some_and(|a| !tag.is_empty() && a == tag))
        })?,
    )?;
    Ok(t)
}

fn layer_info(
    lua: &Lua,
    e: Entity,
    layer: Option<i64>,
    next: bool,
) -> Result<(String, Option<StateInfo>)> {
    let l = layer_index(lua, e, layer)?;
    let st = status(lua, e)?;
    Ok(st
        .and_then(|s| s.layers.into_iter().nth(l))
        .map(|ls| {
            let info = if next {
                ls.next.map(|(i, _)| i)
            } else {
                ls.current
            };
            (ls.name, info)
        })
        .unwrap_or_default())
}

fn component_field<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut Animator) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut Animator>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| missing())
    })?
}

/// Move `e` by an Animator's root motion: through its Character Controller when it has one
/// (collisions, steps), otherwise directly; then turn it.
pub fn apply_root_motion(w: &mut World, e: Entity, delta: Vec3, turn: Quat, dt: f32) {
    if !w.scene.contains(e) {
        return;
    }
    if w.physics.is_character(e) {
        let World { physics, scene, .. } = w;
        physics.move_character(scene, e, delta, dt.max(1e-4));
    } else if delta != Vec3::ZERO {
        let p = w.scene.world_position(e);
        w.scene.set_world_position(e, p + delta);
    }
    if turn != Quat::IDENTITY {
        let r = w.scene.world_rotation(e);
        w.scene.set_world_rotation(e, r * turn);
    }
}

/// `MatchTarget` weights: a MatchTargetWeightMask-like table (`positionXYZWeight`,
/// `rotationWeight`), or nil for full weights.
fn weight_mask(v: Option<Table>) -> Result<(Vec3, f32)> {
    let Some(t) = v else {
        return Ok((Vec3::ONE, 1.0));
    };
    let pos = t
        .get::<Option<LVec3>>("positionXYZWeight")?
        .map_or(Vec3::ONE, |v| v.0);
    let rot = t.get::<Option<f32>>("rotationWeight")?.unwrap_or(1.0);
    Ok((pos, rot))
}

fn scene_of(w: &mut World) -> (&mut animator::AnimatorPlayer, &mut Scene) {
    let World {
        animator, scene, ..
    } = w;
    (animator, scene)
}

impl UserData for LAnimator {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("speed", |lua, a| component_field(lua, a.0, |c| c.speed));
        f.add_field_method_set("speed", |lua, a, v: f32| {
            component_field(lua, a.0, |c| c.speed = v)
        });
        f.add_field_method_get("enabled", |lua, a| component_field(lua, a.0, |c| c.enabled));
        f.add_field_method_set("enabled", |lua, a, v: bool| {
            component_field(lua, a.0, |c| c.enabled = v)
        });
        f.add_field_method_get("applyRootMotion", |lua, a| {
            component_field(lua, a.0, |c| c.apply_root_motion)
        });
        f.add_field_method_set("applyRootMotion", |lua, a, v: bool| {
            component_field(lua, a.0, |c| c.apply_root_motion = v)
        });
        // World-space movement of the last update's root motion.
        f.add_field_method_get("deltaPosition", |lua, a| {
            with_world(lua, |w| LVec3(w.animator.root_delta(a.0).0))
        });
        // Turn of the last update (Euler degrees; apply with `transform:Rotate`).
        f.add_field_method_get("deltaRotation", |lua, a| {
            with_world(lua, |w| LVec3(quat_to_euler(w.animator.root_delta(a.0).1)))
        });
        f.add_field_method_get("velocity", |lua, a| {
            with_world(lua, |w| {
                let dt = w.time.delta;
                let d = w.animator.root_delta(a.0).0;
                LVec3(if dt > 0.0 { d / dt } else { Vec3::ZERO })
            })
        });
        f.add_field_method_get("layerCount", |lua, a| {
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.layer_count(scene, a.0)
            })
        });
        f.add_field_method_get("isMatchingTarget", |lua, a| {
            with_world(lua, |w| w.animator.is_matching_target(a.0))
        });
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // SetFloat(name, value [, dampTime, deltaTime]).
        m.add_method(
            "SetFloat",
            |lua, a, (n, v, damp, dt): (String, f32, Option<f32>, Option<f32>)| match damp {
                None => set(lua, a.0, &n, animator::FLOAT, v),
                Some(damp) => with_world(lua, |w| {
                    let dt = dt.unwrap_or(w.time.delta);
                    let (animator, scene) = scene_of(w);
                    animator.set_float_damped(scene, a.0, &n, v, damp, dt)
                }),
            },
        );
        m.add_method("GetFloat", |lua, a, n: String| {
            get(lua, a.0, &n, animator::FLOAT)
        });
        m.add_method("SetInteger", |lua, a, (n, v): (String, i64)| {
            set(lua, a.0, &n, animator::INT, v as f32)
        });
        m.add_method("GetInteger", |lua, a, n: String| {
            get(lua, a.0, &n, animator::INT).map(|v| v.round() as i64)
        });
        m.add_method("SetBool", |lua, a, (n, v): (String, bool)| {
            set(lua, a.0, &n, animator::BOOL, if v { 1.0 } else { 0.0 })
        });
        m.add_method("GetBool", |lua, a, n: String| {
            get(lua, a.0, &n, animator::BOOL).map(|v| v >= 0.5)
        });
        m.add_method("SetTrigger", |lua, a, n: String| {
            set(lua, a.0, &n, animator::TRIGGER, 1.0)
        });
        m.add_method("ResetTrigger", |lua, a, n: String| {
            set(lua, a.0, &n, animator::TRIGGER, 0.0)
        });
        // Play(state [, layer [, normalizedTime]]).
        m.add_method(
            "Play",
            |lua, a, (state, layer, norm): (String, Option<i64>, Option<f32>)| {
                let layer = play_layer(lua, a.0, layer)?;
                with_world(lua, |w| {
                    let (animator, scene) = scene_of(w);
                    animator.play(scene, a.0, &state, layer, norm)
                })
            },
        );
        // CrossFade(state, normalizedDuration [, layer [, normalizedOffset]]).
        m.add_method(
            "CrossFade",
            |lua, a, (state, d, layer, off): (String, f32, Option<i64>, Option<f32>)| {
                let layer = play_layer(lua, a.0, layer)?;
                with_world(lua, |w| {
                    let (animator, scene) = scene_of(w);
                    animator.cross_fade(scene, a.0, &state, d, false, layer, off.unwrap_or(0.0))
                })
            },
        );
        // CrossFadeInFixedTime(state, seconds [, layer [, normalizedOffset]]).
        m.add_method(
            "CrossFadeInFixedTime",
            |lua, a, (state, d, layer, off): (String, f32, Option<i64>, Option<f32>)| {
                let layer = play_layer(lua, a.0, layer)?;
                with_world(lua, |w| {
                    let (animator, scene) = scene_of(w);
                    animator.cross_fade(scene, a.0, &state, d, true, layer, off.unwrap_or(0.0))
                })
            },
        );
        m.add_method(
            "GetCurrentAnimatorStateInfo",
            |lua, a, layer: Option<i64>| {
                let (name, info) = layer_info(lua, a.0, layer, false)?;
                state_table(lua, &name, info)
            },
        );
        m.add_method("GetNextAnimatorStateInfo", |lua, a, layer: Option<i64>| {
            let (name, info) = layer_info(lua, a.0, layer, true)?;
            state_table(lua, &name, info)
        });
        m.add_method("IsInTransition", |lua, a, layer: Option<i64>| {
            let l = layer_index(lua, a.0, layer)?;
            Ok(status(lua, a.0)?
                .and_then(|s| s.layers.into_iter().nth(l))
                .is_some_and(|ls| ls.next.is_some()))
        });
        m.add_method("GetLayerName", |lua, a, layer: i64| {
            let l = layer_index(lua, a.0, Some(layer))?;
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.layer_name(scene, a.0, l)
            })
        });
        m.add_method("GetLayerIndex", |lua, a, name: String| {
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.layer_index(scene, a.0, &name)
            })
        });
        m.add_method("GetLayerWeight", |lua, a, layer: i64| {
            let l = layer_index(lua, a.0, Some(layer))?;
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.layer_weight(scene, a.0, l)
            })
        });
        m.add_method("SetLayerWeight", |lua, a, (layer, weight): (i64, f32)| {
            let l = layer_index(lua, a.0, Some(layer))?;
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.set_layer_weight(scene, a.0, l, weight);
            })
        });
        // ApplyBuiltinRootMotion(): from OnAnimatorMove, apply the motion the usual way.
        m.add_method("ApplyBuiltinRootMotion", |lua, a, ()| {
            with_world(lua, |w| {
                let (d, turn) = w.animator.root_delta(a.0);
                let dt = w.time.delta;
                apply_root_motion(w, a.0, d, turn, dt);
            })
        });
        // MatchTarget(position, rotation, target, weightMask, startTime [, targetTime]).
        m.add_method(
            "MatchTarget",
            |lua,
             a,
             (pos, rot, target, mask, start, end): (
                LVec3,
                LVec3,
                Value,
                Option<Table>,
                f32,
                Option<f32>,
            )| {
                let root = match &target {
                    Value::Nil | Value::Integer(0) => true,
                    Value::String(s) => s.to_string_lossy() == "Root",
                    _ => false,
                };
                if !root {
                    return Err(mlua::Error::runtime(
                        "MatchTarget: only the Root target (\"Root\" or 0) is supported",
                    ));
                }
                let (pw, rw) = weight_mask(mask)?;
                with_world(lua, |w| {
                    let (animator, scene) = scene_of(w);
                    animator.match_target(
                        scene,
                        a.0,
                        pos.0,
                        euler_to_quat(rot.0),
                        pw,
                        rw,
                        start,
                        end.unwrap_or(1.0),
                    )
                })
            },
        );
        // InterruptMatchTarget([completeMatch = true]).
        m.add_method("InterruptMatchTarget", |lua, a, complete: Option<bool>| {
            with_world(lua, |w| {
                let (animator, scene) = scene_of(w);
                animator.interrupt_match_target(scene, a.0, complete.unwrap_or(true))
            })
        });
    }
}

/// `GetComponent("Animator")`.
pub fn component(lua: &Lua, e: Entity) -> Result<Value> {
    lua.create_userdata(LAnimator(e)).map(Value::UserData)
}

#[cfg(test)]
#[path = "animator_tests.rs"]
mod tests;
