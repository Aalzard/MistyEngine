//! Lua `Input` Manager functions (`GetAxis`, `GetAxisRaw`, `GetButton/Down/Up`,
//! `GetJoystickNames`, `anyKey`, `anyKeyDown`, `mouseScrollDelta`, `mouseDelta`) and tag lookups
//! (`GameObject.FindWithTag`, `FindGameObjectsWithTag`).

use glam::Vec3;
use mlua::{Function, Lua, Result, Table, Value};

use super::api::{LGameObject, LVec3, with_world};
use super::{Input, World};

fn not_setup(name: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "Input Axis {name} is not setup. To change the input settings use: Edit ▸ Project \
         Settings ▸ Input Manager"
    ))
}

/// A virtual button test (held / pressed / released) for `GetButton*`.
fn button(lua: &Lua, test: fn(&Input, &str) -> bool) -> Result<Function> {
    lua.create_function(move |lua, name: String| {
        with_world(lua, |w| w.axes.button(&name, &w.input, test))?.ok_or_else(|| not_setup(&name))
    })
}

/// Register after `Input` and `GameObject` exist (and after `camera_api`, whose
/// `mousePosition` lookup keeps working).
pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let input: Table = g.get("Input")?;
    input.set(
        "GetAxis",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| w.axes.axis(&name))?.ok_or_else(|| not_setup(&name))
        })?,
    )?;
    input.set(
        "GetAxisRaw",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| w.axes.axis_raw(&name))?.ok_or_else(|| not_setup(&name))
        })?,
    )?;
    input.set("GetButton", button(lua, Input::button_held)?)?;
    input.set("GetButtonDown", button(lua, Input::button_pressed)?)?;
    input.set("GetButtonUp", button(lua, Input::button_released)?)?;
    input.set(
        "GetJoystickNames",
        lua.create_function(|lua, ()| {
            let names: Vec<String> = with_world(lua, |w| {
                w.input.joysticks.iter().map(|j| j.name.clone()).collect()
            })?;
            lua.create_sequence_from(names)
        })?,
    )?;

    // Properties, falling back to the earlier lookup (mousePosition).
    let meta = input.metatable().unwrap_or(lua.create_table()?);
    let previous: Option<Function> = meta.get("__index").ok();
    meta.set(
        "__index",
        lua.create_function(move |lua, (t, key): (Table, String)| {
            let value = with_world(lua, |w| property(w, &key))?;
            match (value, &previous) {
                (Some(Prop::Bool(b)), _) => Ok(Value::Boolean(b)),
                (Some(Prop::Vec(v)), _) => Ok(Value::UserData(lua.create_userdata(LVec3(v))?)),
                (None, Some(f)) => f.call((t, key)),
                (None, None) => Ok(Value::Nil),
            }
        })?,
    )?;
    input.set_metatable(Some(meta))?;

    let go: Table = g.get("GameObject")?;
    go.set(
        "FindWithTag",
        lua.create_function(|lua, tag: String| {
            Ok(tagged(lua, &tag)?.into_iter().next().map(LGameObject))
        })?,
    )?;
    go.set(
        "FindGameObjectsWithTag",
        lua.create_function(|lua, tag: String| {
            let found = tagged(lua, &tag)?;
            lua.create_sequence_from(found.into_iter().map(LGameObject))
        })?,
    )?;
    Ok(())
}

enum Prop {
    Bool(bool),
    Vec(Vec3),
}

fn property(w: &World, key: &str) -> Option<Prop> {
    Some(match key {
        "anyKey" => Prop::Bool(w.input.any_key()),
        "anyKeyDown" => Prop::Bool(w.input.any_key_down()),
        "mouseScrollDelta" => {
            let n = w.input.scroll_notches();
            Prop::Vec(Vec3::new(n.x, n.y, 0.0))
        }
        // Pointer movement this frame in pixels, y up (what the Mouse X / Mouse Y axes read
        // before their sensitivity); raw motion while the cursor is locked.
        "mouseDelta" => {
            let d = w.input.mouse_delta;
            Prop::Vec(Vec3::new(d.x, -d.y, 0.0))
        }
        _ => return None,
    })
}

/// Active objects with `tag`, in Hierarchy order; an undefined tag is an error.
fn tagged(lua: &Lua, tag: &str) -> Result<Vec<hecs::Entity>> {
    with_world(lua, |w| {
        if !w.tags_layers.has_tag(tag) {
            return Err(mlua::Error::runtime(format!("Tag: {tag} is not defined.")));
        }
        Ok(w.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| w.scene.active_in_hierarchy(*e) && w.scene.game_object(*e).tag == tag)
            .collect())
    })?
}
