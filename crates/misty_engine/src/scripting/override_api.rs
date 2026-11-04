//! Lua per-object material overrides on a Mesh Renderer:
//! `renderer:SetOverride(name, value [, element])`, `renderer:ClearOverride(name [, element])`,
//! `renderer:GetOverride(name [, element])` (nil when not overridden). Names: "tiling",
//! "offset" (Vector2), "rotation" (degrees), "color" (Color), "smoothness", "metallic",
//! "emission" (multipliers). `element` is the 1-based Materials element (default 1).

use glam::Vec3;
use mlua::{FromLua, IntoLua, Lua, Result, UserDataMethods, Value};

use super::api::{LVec3, with_world};
use super::material_api::LMeshRenderer;
use super::ui_api::LColor;
use crate::render::overrides::{MaterialOverrides, OverrideValue};
use crate::scene::MeshRenderer;

fn unknown(name: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "unknown override '{name}' (expected one of {})",
        MaterialOverrides::NAMES.join(", ")
    ))
}

/// A Lua value as the kind of value override `name` takes.
fn value(lua: &Lua, name: &str, v: Value) -> Result<OverrideValue> {
    Ok(match name {
        "tiling" | "offset" => {
            let p = LVec3::from_lua(v, lua)
                .map_err(|_| mlua::Error::runtime(format!("{name} expects a Vector2")))?;
            OverrideValue::Vec2([p.0.x, p.0.y])
        }
        "color" => OverrideValue::Color(LColor::from_lua(v, lua)?.0),
        "rotation" | "smoothness" | "metallic" | "emission" => {
            OverrideValue::Float(f32::from_lua(v, lua)?)
        }
        _ => return Err(unknown(name)),
    })
}

fn to_lua(lua: &Lua, v: OverrideValue) -> Result<Value> {
    match v {
        OverrideValue::Vec2([x, y]) => LVec3(Vec3::new(x, y, 0.0)).into_lua(lua),
        OverrideValue::Color(c) => LColor(c).into_lua(lua),
        OverrideValue::Float(f) => f.into_lua(lua),
    }
}

fn edit<R>(lua: &Lua, r: &LMeshRenderer, f: impl FnOnce(&mut MeshRenderer) -> R) -> Result<R> {
    with_world(lua, |w| {
        let mut c = w
            .scene
            .world
            .get::<&mut MeshRenderer>(r.0)
            .map_err(|_| mlua::Error::runtime("the MeshRenderer was destroyed"))?;
        Ok(f(&mut c))
    })?
}

fn slot(element: Option<usize>) -> usize {
    element.unwrap_or(1).saturating_sub(1)
}

pub(super) fn add_methods<M: UserDataMethods<LMeshRenderer>>(m: &mut M) {
    m.add_method(
        "SetOverride",
        |lua, r, (name, v, element): (String, Value, Option<usize>)| {
            let v = value(lua, &name, v)?;
            edit(lua, r, |c| {
                c.edit_overrides(slot(element), |o| o.set(&name, v))
            })?;
            Ok(())
        },
    );
    m.add_method(
        "ClearOverride",
        |lua, r, (name, element): (String, Option<usize>)| {
            if !MaterialOverrides::NAMES.contains(&name.as_str()) {
                return Err(unknown(&name));
            }
            edit(lua, r, |c| {
                c.edit_overrides(slot(element), |o| o.clear(&name))
            })?;
            Ok(())
        },
    );
    m.add_method(
        "GetOverride",
        |lua, r, (name, element): (String, Option<usize>)| {
            if !MaterialOverrides::NAMES.contains(&name.as_str()) {
                return Err(unknown(&name));
            }
            let v = edit(lua, r, |c| {
                c.overrides_at(slot(element)).and_then(|o| o.get(&name))
            })?;
            v.map_or(Ok(Value::Nil), |v| to_lua(lua, v))
        },
    );
}
