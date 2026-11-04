//! `SceneManager` and `LoadSceneMode` for scripts.
//!
//! `LoadScene(nameOrIndex [, LoadSceneMode.Additive])` and `UnloadSceneAsync(nameOrIndex)` are
//! requests the host carries out after the frame (it knows the Build Settings scene list);
//! `GetActiveScene()` / `SetActiveScene(scene or name)` act on the loaded scenes right away.

use mlua::{Lua, Result, Table, Value};

use super::Request;
use super::api::with_world;
use crate::build::SceneRef;
use crate::scene::SceneId;

/// Scene name or build index from a script argument.
fn scene_ref(v: &Value, what: &str) -> Result<SceneRef> {
    match v {
        Value::Integer(i) => Ok(SceneRef::Index(*i)),
        Value::Number(n) if n.fract() == 0.0 => Ok(SceneRef::Index(*n as i64)),
        Value::String(s) => Ok(SceneRef::Name(s.to_string_lossy())),
        _ => Err(mlua::Error::runtime(format!(
            "{what} expects a scene name or build index"
        ))),
    }
}

/// `LoadSceneMode.Additive` (1) or `true` load additively; nil / `Single` (0) replace.
fn additive(mode: &Value) -> bool {
    match mode {
        Value::Boolean(b) => *b,
        Value::Integer(i) => *i == 1,
        Value::Number(n) => *n == 1.0,
        _ => false,
    }
}

/// A Scene table: `{ name, path, isLoaded }`.
fn scene_table(lua: &Lua, id: SceneId) -> Result<Table> {
    let (name, path, loaded) = with_world(lua, |w| {
        let s = &w.scene;
        (
            s.scene_name(id),
            s.scene_path(id).unwrap_or_default(),
            s.entry(id).is_some_and(|e| e.loaded),
        )
    })?;
    let t = lua.create_table()?;
    t.set("name", name)?;
    t.set("path", path)?;
    t.set("isLoaded", loaded)?;
    Ok(t)
}

pub fn register(lua: &Lua, g: &Table) -> Result<()> {
    let modes = lua.create_table()?;
    modes.set("Single", 0)?;
    modes.set("Additive", 1)?;
    g.set("LoadSceneMode", modes)?;

    let scenes = lua.create_table()?;
    scenes.set(
        "LoadScene",
        lua.create_function(|lua, (v, mode): (Value, Value)| {
            let scene = scene_ref(&v, "LoadScene")?;
            let request = if additive(&mode) {
                Request::LoadSceneAdditive(scene)
            } else {
                Request::LoadScene(scene)
            };
            with_world(lua, |w| w.requests.push(request))
        })?,
    )?;
    let unload = lua.create_function(|lua, v: Value| {
        let scene = scene_ref(&v, "UnloadSceneAsync")?;
        with_world(lua, |w| w.requests.push(Request::UnloadScene(scene)))
    })?;
    scenes.set("UnloadSceneAsync", unload.clone())?;
    scenes.set("UnloadScene", unload)?;
    scenes.set(
        "GetActiveScene",
        lua.create_function(|lua, ()| {
            let id = with_world(lua, |w| w.scene.active_scene())?;
            scene_table(lua, id)
        })?,
    )?;
    scenes.set(
        "GetSceneByName",
        lua.create_function(|lua, name: String| {
            match with_world(lua, |w| w.scene.find_loaded(&name))? {
                Some(id) => Ok(Value::Table(scene_table(lua, id)?)),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    scenes.set(
        "SetActiveScene",
        lua.create_function(|lua, v: Value| {
            let name = match &v {
                Value::String(s) => s.to_string_lossy(),
                Value::Table(t) => t.get::<String>("name")?,
                _ => {
                    return Err(mlua::Error::runtime(
                        "SetActiveScene expects a Scene or scene name",
                    ));
                }
            };
            with_world(lua, |w| match w.scene.find_loaded(&name) {
                Some(id) => w.scene.set_active_scene(id),
                None => {
                    w.logs.push((
                        super::LogKind::Warning,
                        format!("SetActiveScene: '{name}' is not loaded"),
                    ));
                    false
                }
            })
        })?,
    )?;
    scenes.set(
        "GetSceneCount",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.loaded_scenes().len()))?,
    )?;
    g.set("SceneManager", scenes)?;
    Ok(())
}
