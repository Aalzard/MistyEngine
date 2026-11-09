//! Graph values ↔ Lua values, and the Lua calls graphs make: API members without a native
//! implementation, functions of Lua scripts (Call Lua Function), fields of Lua values, and
//! custom events delivered to Lua scripts.

use std::rc::Rc;

use hecs::Entity;
use mlua::{Lua, MultiValue, ObjectLike, Value as LuaValue};

use super::api::Mode;
use super::compile::LuaOp;
use super::value::{LuaRef, Value};
use crate::scripting::api::{LGameObject, LTransform, LVec3};

/// A graph value for Lua.
pub fn to_lua(lua: &Lua, v: &Value) -> mlua::Result<LuaValue> {
    Ok(match v {
        Value::Nil => LuaValue::Nil,
        Value::Bool(b) => LuaValue::Boolean(*b),
        Value::Int(i) => LuaValue::Integer(*i),
        Value::Float(f) => LuaValue::Number(f64::from(*f)),
        Value::Str(s) => LuaValue::String(lua.create_string(s.as_bytes())?),
        Value::Vec3(v) => LuaValue::UserData(lua.create_userdata(LVec3(*v))?),
        // Lua has no quaternion type: rotations travel as Euler angles.
        Value::Quat(q) => LuaValue::UserData(
            lua.create_userdata(LVec3(crate::camera::quat_to_euler(*q)))?,
        ),
        Value::Object(e) => LuaValue::UserData(lua.create_userdata(LGameObject(*e))?),
        Value::List(l) => {
            let t = lua.create_table()?;
            for (i, x) in l.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, x)?)?;
            }
            LuaValue::Table(t)
        }
        Value::Lua(r) => lua.registry_value(&r.0)?,
    })
}

/// A Lua value for a graph: numbers, strings, booleans, Vector3, GameObjects (and Transforms)
/// convert; array tables become lists; anything else is kept as an opaque Lua value.
pub fn from_lua(lua: &Lua, v: LuaValue) -> Value {
    match v {
        LuaValue::Nil => Value::Nil,
        LuaValue::Boolean(b) => Value::Bool(b),
        LuaValue::Integer(i) => Value::Int(i),
        LuaValue::Number(n) => Value::Float(n as f32),
        LuaValue::String(s) => Value::str(&s.to_string_lossy()),
        LuaValue::UserData(ud) => {
            if let Ok(v) = ud.borrow::<LVec3>() {
                return Value::Vec3(v.0);
            }
            if let Ok(g) = ud.borrow::<LGameObject>() {
                return Value::Object(g.0);
            }
            if let Ok(t) = ud.borrow::<LTransform>() {
                return Value::Object(t.0);
            }
            keep(lua, LuaValue::UserData(ud))
        }
        LuaValue::Table(t) => {
            let n = t.raw_len();
            if n > 0 || t.is_empty() {
                let items = (1..=n)
                    .map(|i| from_lua(lua, t.raw_get(i).unwrap_or(LuaValue::Nil)))
                    .collect();
                Value::List(Rc::new(items))
            } else {
                keep(lua, LuaValue::Table(t))
            }
        }
        other => keep(lua, other),
    }
}

fn keep(lua: &Lua, v: LuaValue) -> Value {
    match lua.create_registry_value(v) {
        Ok(k) => Value::Lua(Rc::new(LuaRef(k))),
        Err(_) => Value::Nil,
    }
}

fn args(lua: &Lua, regs: &[Value], slots: &[u32]) -> mlua::Result<MultiValue> {
    let mut vals: Vec<LuaValue> = slots
        .iter()
        .map(|s| to_lua(lua, &regs[*s as usize]))
        .collect::<mlua::Result<_>>()?;
    // Optional parameters: leave trailing nils out.
    while matches!(vals.last(), Some(LuaValue::Nil)) {
        vals.pop();
    }
    Ok(MultiValue::from_vec(vals))
}

fn first(m: MultiValue) -> LuaValue {
    m.into_iter().next().unwrap_or(LuaValue::Nil)
}

/// The Lua object an object-type member is called on: the GameObject, its Transform, or its
/// component of that type.
fn component(lua: &Lua, ty: &str, e: Entity) -> Result<LuaValue, String> {
    let go = lua.create_userdata(LGameObject(e)).map_err(|e| e.to_string())?;
    let v = match ty {
        "GameObject" => LuaValue::UserData(go),
        "Transform" => {
            LuaValue::UserData(lua.create_userdata(LTransform(e)).map_err(|e| e.to_string())?)
        }
        _ => go
            .call_method::<LuaValue>("GetComponent", ty)
            .map_err(|e| e.to_string())?,
    };
    if v.is_nil() {
        return Err(format!("the object has no {ty}"));
    }
    Ok(v)
}

fn get(obj: &LuaValue, key: &str) -> mlua::Result<LuaValue> {
    match obj {
        LuaValue::Table(t) => t.get(key),
        LuaValue::UserData(u) => u.get(key),
        other => Err(mlua::Error::runtime(format!(
            "cannot read '{key}' of a {}",
            other.type_name()
        ))),
    }
}

fn set(obj: &LuaValue, key: &str, v: LuaValue) -> mlua::Result<()> {
    match obj {
        LuaValue::Table(t) => t.set(key, v),
        LuaValue::UserData(u) => u.set(key, v),
        other => Err(mlua::Error::runtime(format!(
            "cannot set '{key}' of a {}",
            other.type_name()
        ))),
    }
}

fn target(regs: &[Value], slot: Option<u32>, this: Entity) -> Result<Entity, String> {
    match slot.map(|s| &regs[s as usize]) {
        Some(Value::Object(e)) => Ok(*e),
        None => Ok(this),
        Some(Value::Nil) => Err("no target GameObject (nil)".into()),
        Some(v) => Err(format!("expected a GameObject target, got {}", v.type_name())),
    }
}

/// Make one Lua call of a graph, writing its result register.
pub fn exec(lua: &Lua, op: &LuaOp, regs: &mut [Value], this: Entity) -> Result<(), String> {
    match op {
        LuaOp::Api {
            owner,
            member,
            mode,
            target: t,
            args: a,
            out,
            ..
        } => {
            let object = if let Some(ty) = owner.strip_prefix('#') {
                Some(component(lua, ty, target(regs, *t, this)?)?)
            } else if owner.is_empty() {
                None
            } else {
                let g: LuaValue = lua.globals().get(owner.as_str()).map_err(|e| e.to_string())?;
                if g.is_nil() {
                    return Err(format!("there is no Lua global '{owner}'"));
                }
                Some(g)
            };
            let result = match (mode, &object) {
                (Mode::Get, Some(o)) => get(o, member).map_err(|e| e.to_string())?,
                (Mode::Get, None) => lua
                    .globals()
                    .get::<LuaValue>(member.as_str())
                    .map_err(|e| e.to_string())?,
                (Mode::Set, o) => {
                    let v = a
                        .first()
                        .map(|s| to_lua(lua, &regs[*s as usize]))
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .unwrap_or(LuaValue::Nil);
                    match o {
                        Some(o) => set(o, member, v),
                        None => lua.globals().set(member.as_str(), v),
                    }
                    .map_err(|e| e.to_string())?;
                    LuaValue::Nil
                }
                (Mode::Call, Some(o)) if owner.starts_with('#') => {
                    let call_args = args(lua, regs, a).map_err(|e| e.to_string())?;
                    match o {
                        LuaValue::UserData(u) => u.call_method::<MultiValue>(member, call_args),
                        LuaValue::Table(t) => t.call_method::<MultiValue>(member, call_args),
                        _ => Err(mlua::Error::runtime("not an object")),
                    }
                    .map(first)
                    .map_err(|e| e.to_string())?
                }
                (Mode::Call, o) => {
                    let f = match o {
                        Some(o) => get(o, member),
                        None => lua.globals().get::<LuaValue>(member.as_str()),
                    }
                    .map_err(|e| e.to_string())?;
                    let LuaValue::Function(f) = f else {
                        return Err(format!("{owner}.{member} is not a function"));
                    };
                    let call_args = args(lua, regs, a).map_err(|e| e.to_string())?;
                    f.call::<MultiValue>(call_args)
                        .map(first)
                        .map_err(|e| e.to_string())?
                }
            };
            if let Some(o) = out {
                regs[*o as usize] = from_lua(lua, result);
            }
            Ok(())
        }
        LuaOp::CallScript {
            script,
            function,
            target: t,
            args: a,
            out,
            ..
        } => {
            let e = target(regs, Some(*t), this)?;
            let Some(instance) = crate::scripting::script_instance(lua, e, script) else {
                return Err(format!("the object has no Lua script '{script}'"));
            };
            let f: LuaValue = instance.get(function.as_str()).map_err(|e| e.to_string())?;
            let LuaValue::Function(f) = f else {
                return Err(format!("{script} has no function '{function}'"));
            };
            let mut call_args = vec![LuaValue::Table(instance)];
            call_args.extend(args(lua, regs, a).map_err(|e| e.to_string())?);
            let r = f
                .call::<MultiValue>(MultiValue::from_vec(call_args))
                .map_err(|e| e.to_string())?;
            regs[*out as usize] = from_lua(lua, first(r));
            Ok(())
        }
        LuaOp::GetField {
            object, field, out, ..
        } => {
            let key = regs[*field as usize].to_text(None);
            let v = match &regs[*object as usize] {
                Value::Object(e) => {
                    let go = to_lua(lua, &Value::Object(*e)).map_err(|e| e.to_string())?;
                    match get(&go, &key) {
                        Ok(v) if !v.is_nil() => v,
                        _ => component(lua, &key, *e).unwrap_or(LuaValue::Nil),
                    }
                }
                other => {
                    let o = to_lua(lua, other).map_err(|e| e.to_string())?;
                    get(&o, &key).map_err(|e| e.to_string())?
                }
            };
            regs[*out as usize] = from_lua(lua, v);
            Ok(())
        }
    }
}
