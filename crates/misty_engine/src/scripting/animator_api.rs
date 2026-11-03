//! Lua `Animator`: parameters (Set/Get Float, Integer, Bool, SetTrigger/ResetTrigger), `Play`,
//! `CrossFade`, `CrossFadeInFixedTime`, `GetCurrentAnimatorStateInfo(0)`, `IsInTransition(0)`,
//! `speed` and `enabled`. Only the base layer (0) exists.

use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::World;
use super::api::{LGameObject, with_world};
use crate::animation::animator::{self, AnimatorStatus, StateInfo};
use crate::animation::controller::ParamKind;
use crate::scene::Animator;

#[derive(Clone, Copy)]
pub struct LAnimator(pub Entity);

fn missing() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

/// Only layer 0 (Base Layer) exists; `-1` means "the default layer" in Play / CrossFade.
fn check_layer(layer: Option<i64>) -> Result<()> {
    match layer.unwrap_or(0) {
        -1 | 0 => Ok(()),
        n => Err(mlua::Error::runtime(format!(
            "Animator layer index {n} is out of bounds (only the Base Layer, 0, exists)"
        ))),
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

/// An AnimatorStateInfo-like table: `normalizedTime`, `length`, `speed`, `loop`, `IsName(name)`.
fn state_table(lua: &Lua, info: Option<StateInfo>) -> Result<Table> {
    let t = lua.create_table()?;
    let info = info.unwrap_or(StateInfo {
        name: String::new(),
        normalized_time: 0.0,
        length: 0.0,
        speed: 1.0,
        looping: false,
    });
    t.set("normalizedTime", info.normalized_time)?;
    t.set("length", info.length)?;
    t.set("speed", info.speed)?;
    t.set("loop", info.looping)?;
    let name = info.name;
    // Works with `info:IsName("Idle")` and `info.IsName("Idle")`; "Base Layer.Idle" also matches.
    t.set(
        "IsName",
        lua.create_function(move |_, args: Variadic<Value>| {
            let asked = args.iter().rev().find_map(|v| match v {
                Value::String(s) => Some(s.to_string_lossy()),
                _ => None,
            });
            Ok(asked.is_some_and(|a| {
                !name.is_empty() && (a == name || a == format!("Base Layer.{name}"))
            }))
        })?,
    )?;
    Ok(t)
}

impl UserData for LAnimator {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("speed", |lua, a| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&Animator>(a.0)
                    .map(|c| c.speed)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_set("speed", |lua, a, v: f32| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&mut Animator>(a.0)
                    .map(|mut c| c.speed = v)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_get("enabled", |lua, a| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&Animator>(a.0)
                    .map(|c| c.enabled)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_set("enabled", |lua, a, v: bool| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&mut Animator>(a.0)
                    .map(|mut c| c.enabled = v)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetFloat", |lua, a, (n, v): (String, f32)| {
            set(lua, a.0, &n, animator::FLOAT, v)
        });
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
                check_layer(layer)?;
                with_world(lua, |w| {
                    let World {
                        animator, scene, ..
                    } = w;
                    animator.play(scene, a.0, &state, norm)
                })
            },
        );
        // CrossFade(state, normalizedDuration [, layer [, normalizedOffset]]).
        m.add_method(
            "CrossFade",
            |lua, a, (state, d, layer, off): (String, f32, Option<i64>, Option<f32>)| {
                check_layer(layer)?;
                with_world(lua, |w| {
                    let World {
                        animator, scene, ..
                    } = w;
                    animator.cross_fade(scene, a.0, &state, d, false, off.unwrap_or(0.0))
                })
            },
        );
        // CrossFadeInFixedTime(state, seconds [, layer [, normalizedOffset]]).
        m.add_method(
            "CrossFadeInFixedTime",
            |lua, a, (state, d, layer, off): (String, f32, Option<i64>, Option<f32>)| {
                check_layer(layer)?;
                with_world(lua, |w| {
                    let World {
                        animator, scene, ..
                    } = w;
                    animator.cross_fade(scene, a.0, &state, d, true, off.unwrap_or(0.0))
                })
            },
        );
        m.add_method(
            "GetCurrentAnimatorStateInfo",
            |lua, a, layer: Option<i64>| {
                check_layer(layer)?;
                let st = status(lua, a.0)?;
                state_table(lua, st.and_then(|s| s.current))
            },
        );
        m.add_method("GetNextAnimatorStateInfo", |lua, a, layer: Option<i64>| {
            check_layer(layer)?;
            let st = status(lua, a.0)?;
            state_table(lua, st.and_then(|s| s.next).map(|(i, _)| i))
        });
        m.add_method("IsInTransition", |lua, a, layer: Option<i64>| {
            check_layer(layer)?;
            Ok(status(lua, a.0)?.is_some_and(|s| s.next.is_some()))
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
