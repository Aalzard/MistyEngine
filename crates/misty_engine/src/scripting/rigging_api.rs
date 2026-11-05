//! Lua access to Animation Rigging components: `GetComponent("TwoBoneIKConstraint")` (or the
//! Inspector name, "Two Bone IK Constraint") returns an object whose fields mirror the
//! Inspector in camelCase: `weight`, `target`, `hint`, `targetPositionWeight`,
//! `constrainedObject`, `sourceObject`, `dampPosition`, `maxIterations`, … Object fields take
//! and return GameObjects (a Transform works too; `nil` clears). Multi-Aim sources are reached
//! with `sourceCount`, `GetSource(i)`, `SetSource(i, obj)`, `GetSourceWeight(i)`,
//! `SetSourceWeight(i, w)` and `AddSource(obj, w)` (1-based).

use hecs::Entity;
use mlua::{Lua, MetaMethod, Result, UserData, UserDataMethods, Value};

use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::rigging::RIG_REFS;
use crate::scene::registry;

/// Script names → component names.
const NAMES: [(&str, &str); 7] = [
    ("RigBuilder", "Rig Builder"),
    ("Rig", "Rig"),
    ("TwoBoneIKConstraint", "Two Bone IK Constraint"),
    ("MultiAimConstraint", "Multi-Aim Constraint"),
    ("DampedTransform", "Damped Transform"),
    ("ChainIKConstraint", "Chain IK Constraint"),
    ("OverrideTransform", "Override Transform"),
];

#[derive(Clone, Copy)]
pub struct LRigComponent {
    e: Entity,
    name: &'static str,
}

pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let Some(&(_, comp)) = NAMES.iter().find(|(s, c)| *s == name || *c == name) else {
        return Ok(None);
    };
    let has = with_world(lua, |w| {
        registry::find(comp).is_some_and(|i| (i.has)(&w.scene.world, e))
    })?;
    if !has {
        return Ok(None);
    }
    let ud = lua.create_userdata(LRigComponent { e, name: comp })?;
    Ok(Some(Value::UserData(ud)))
}

/// `targetPositionWeight` → `target_position_weight`; `sourceObject` → `source`.
fn field_name(key: &str) -> String {
    if key == "sourceObject" {
        return "source".into();
    }
    let mut out = String::new();
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_object_field(comp: &str, field: &str) -> bool {
    RIG_REFS
        .iter()
        .any(|(c, list, f)| *c == comp && list.is_empty() && *f == field)
}

/// The component as JSON, or a "destroyed" error.
fn read(lua: &Lua, c: &LRigComponent) -> Result<serde_json::Value> {
    with_world(lua, |w| {
        registry::find(c.name).and_then(|i| (i.to_json)(&w.scene.world, c.e))
    })?
    .ok_or_else(|| mlua::Error::runtime(format!("{} was destroyed", c.name)))
}

fn write(lua: &Lua, c: &LRigComponent, v: serde_json::Value) -> Result<()> {
    with_world(lua, |w| {
        let info = registry::find(c.name).expect("rig component is registered");
        (info.from_json)(&mut w.scene.world, c.e, v)
    })?
    .map_err(mlua::Error::runtime)
}

/// An f32 field as the f64 Lua sees (0.8, not 0.800000011920929).
fn number(v: &serde_json::Value) -> f64 {
    let f = v.as_f64().unwrap_or(0.0) as f32;
    f.to_string().parse().unwrap_or(f as f64)
}

fn object_value(lua: &Lua, bits: &serde_json::Value) -> Result<Value> {
    match bits.as_u64().and_then(Entity::from_bits) {
        Some(e) => lua.create_userdata(LGameObject(e)).map(Value::UserData),
        None => Ok(Value::Nil),
    }
}

fn object_bits(v: &Value) -> Result<serde_json::Value> {
    match v {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::UserData(ud) if ud.is::<LGameObject>() => {
            Ok(ud.borrow::<LGameObject>()?.0.to_bits().get().into())
        }
        Value::UserData(ud) if ud.is::<LTransform>() => {
            Ok(ud.borrow::<LTransform>()?.0.to_bits().get().into())
        }
        _ => Err(mlua::Error::runtime(
            "expected a GameObject, Transform or nil",
        )),
    }
}

fn get(lua: &Lua, c: &LRigComponent, key: &str) -> Result<Value> {
    if key == "gameObject" {
        return lua.create_userdata(LGameObject(c.e)).map(Value::UserData);
    }
    let json = read(lua, c)?;
    if key == "sourceCount" || key == "layerCount" {
        let list = if key == "sourceCount" {
            "sources"
        } else {
            "layers"
        };
        let n = json
            .get(list)
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len);
        return Ok(Value::Integer(n as i64));
    }
    let field = field_name(key);
    let Some(v) = json.get(&field) else {
        return Err(mlua::Error::runtime(format!(
            "{} has no field '{key}'",
            c.name
        )));
    };
    if is_object_field(c.name, &field) {
        return object_value(lua, v);
    }
    Ok(match v {
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) if n.is_u64() || n.is_i64() => {
            Value::Integer(n.as_i64().unwrap_or(0))
        }
        serde_json::Value::Number(_) => Value::Number(number(v)),
        serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
        serde_json::Value::Array(a) if a.len() == 3 && a.iter().all(|x| x.is_number()) => {
            let f = |i: usize| number(&a[i]) as f32;
            lua.create_userdata(LVec3(glam::Vec3::new(f(0), f(1), f(2))))
                .map(Value::UserData)?
        }
        _ => {
            return Err(mlua::Error::runtime(format!(
                "{}.{key} cannot be read from scripts",
                c.name
            )));
        }
    })
}

fn set(lua: &Lua, c: &LRigComponent, key: &str, value: Value) -> Result<()> {
    let mut json = read(lua, c)?;
    let field = field_name(key);
    let Some(old) = json.get(&field).cloned() else {
        return Err(mlua::Error::runtime(format!(
            "{} has no field '{key}'",
            c.name
        )));
    };
    let new = if is_object_field(c.name, &field) {
        object_bits(&value)?
    } else {
        match (&old, value) {
            (serde_json::Value::Bool(_), Value::Boolean(b)) => b.into(),
            (serde_json::Value::Number(_), Value::Integer(i)) => i.into(),
            (serde_json::Value::Number(_), Value::Number(n)) => n.into(),
            (serde_json::Value::String(_), Value::String(s)) => s.to_str()?.to_string().into(),
            (serde_json::Value::Array(_), v @ (Value::UserData(_) | Value::Table(_))) => {
                let v = mlua::FromLua::from_lua(v, lua).map(|LVec3(v)| v)?;
                serde_json::json!([v.x, v.y, v.z])
            }
            (_, v) => {
                return Err(mlua::Error::runtime(format!(
                    "{}.{key}: unexpected {}",
                    c.name,
                    v.type_name()
                )));
            }
        }
    };
    json[field.as_str()] = new;
    write(lua, c, json)
}

/// Multi-Aim source `i` (1-based) as a mutable JSON object.
fn with_source<R>(
    lua: &Lua,
    c: &LRigComponent,
    i: usize,
    f: impl FnOnce(&mut serde_json::Value) -> Result<R>,
) -> Result<R> {
    let mut json = read(lua, c)?;
    let Some(item) = json
        .get_mut("sources")
        .and_then(|s| s.as_array_mut())
        .and_then(|a| a.get_mut(i.wrapping_sub(1)))
    else {
        return Err(mlua::Error::runtime(format!("no source {i}")));
    };
    let r = f(item)?;
    write(lua, c, json)?;
    Ok(r)
}

impl UserData for LRigComponent {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetSource", |lua, c, i: usize| {
            let obj = with_source(lua, c, i, |s| Ok(s["object"].clone()))?;
            object_value(lua, &obj)
        });
        m.add_method("SetSource", |lua, c, (i, obj): (usize, Value)| {
            let bits = object_bits(&obj)?;
            with_source(lua, c, i, |s| {
                s["object"] = bits;
                Ok(())
            })
        });
        m.add_method("GetSourceWeight", |lua, c, i: usize| {
            with_source(lua, c, i, |s| Ok(number(&s["weight"])))
        });
        m.add_method("SetSourceWeight", |lua, c, (i, w): (usize, f64)| {
            with_source(lua, c, i, |s| {
                s["weight"] = w.into();
                Ok(())
            })
        });
        m.add_method("AddSource", |lua, c, (obj, w): (Value, Option<f64>)| {
            let bits = object_bits(&obj)?;
            let mut json = read(lua, c)?;
            let Some(list) = json.get_mut("sources").and_then(|s| s.as_array_mut()) else {
                return Err(mlua::Error::runtime(format!("{} has no sources", c.name)));
            };
            list.push(serde_json::json!({ "object": bits, "weight": w.unwrap_or(1.0) }));
            write(lua, c, json)
        });
        m.add_meta_method(MetaMethod::Index, |lua, c, key: String| get(lua, c, &key));
        m.add_meta_method(
            MetaMethod::NewIndex,
            |lua, c, (key, value): (String, Value)| set(lua, c, &key, value),
        );
        m.add_meta_method(MetaMethod::ToString, |_, c, ()| Ok(c.name.to_owned()));
    }
}
