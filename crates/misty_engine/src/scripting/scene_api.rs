//! `SceneManager` and `LoadSceneMode` for scripts.
//!
//! `LoadScene(nameOrIndex [, LoadSceneMode.Additive])` and `UnloadSceneAsync(nameOrIndex)` are
//! requests the host carries out after the frame (it knows the Build Settings scene list);
//! `GetActiveScene()` / `SetActiveScene(scene or name)` act on the loaded scenes right away.
//! `LoadSceneAsync` / `UnloadSceneAsync` return an operation (scene_ops.rs): `isDone`,
//! `progress`, `allowSceneActivation`, `OnCompleted(fn)` (or `op.completed = fn`).

use mlua::{Lua, Result, Table, Value};

use super::api::with_world;
use super::{LogKind, Request, ScriptHost};
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

const PRELUDE: &str = r#"
local SM = SceneManager
local prim = SM.__prim
SM.__prim = nil
local callbacks = {}
local Op = {}
local function onCompleted(op, fn)
    if type(fn) ~= "function" then error("OnCompleted expects a function", 2) end
    if op.isDone then fn(op) return op end
    local id = rawget(op, "id")
    local list = callbacks[id]
    if not list then list = {} callbacks[id] = list end
    list[#list + 1] = fn
    return op
end
Op.__index = function(op, k)
    local id = rawget(op, "id")
    if k == "isDone" then return prim.state(id) == 2
    elseif k == "progress" then return prim.state(id) == 2 and 1 or 0.9
    elseif k == "allowSceneActivation" then return prim.state(id) ~= 0
    elseif k == "OnCompleted" then return onCompleted
    end
    return nil
end
Op.__newindex = function(op, k, v)
    if k == "allowSceneActivation" then
        prim.allow(rawget(op, "id"), v and true or false)
    elseif k == "completed" then
        onCompleted(op, v)
    else
        rawset(op, k, v)
    end
end
Op.__tostring = function(op) return "AsyncOperation(" .. tostring(rawget(op, "id")) .. ")" end
local function wrap(id) return setmetatable({ id = id }, Op) end
function SM.LoadSceneAsync(scene, mode) return wrap(prim.load(scene, mode)) end
function SM.UnloadSceneAsync(scene) return wrap(prim.unload(scene)) end
-- Called by the engine at the start of a frame with the operations that finished.
function SM.__tick(done)
    local errors = {}
    for _, id in ipairs(done) do
        local list = callbacks[id]
        callbacks[id] = nil
        if list then
            local op = wrap(id)
            for _, fn in ipairs(list) do
                local ok, err = pcall(fn, op)
                if not ok then errors[#errors + 1] = "AsyncOperation.completed: " .. tostring(err) end
            end
        end
    end
    return errors
end
"#;

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
    scenes.set(
        "UnloadScene",
        lua.create_function(|lua, v: Value| {
            let scene = scene_ref(&v, "UnloadScene")?;
            with_world(lua, |w| w.requests.push(Request::UnloadScene(scene)))
        })?,
    )?;
    // The operations' primitives; the prelude wraps them (and removes `__prim`).
    let prim = lua.create_table()?;
    prim.set(
        "load",
        lua.create_function(|lua, (v, mode): (Value, Value)| {
            let scene = scene_ref(&v, "LoadSceneAsync")?;
            let request = if additive(&mode) {
                Request::LoadSceneAdditive(scene)
            } else {
                Request::LoadScene(scene)
            };
            with_world(lua, |w| w.scene_op_start(request, true))
        })?,
    )?;
    prim.set(
        "unload",
        lua.create_function(|lua, v: Value| {
            let scene = scene_ref(&v, "UnloadSceneAsync")?;
            with_world(lua, |w| w.scene_op_start(Request::UnloadScene(scene), true))
        })?,
    )?;
    prim.set(
        "state",
        lua.create_function(|lua, id: i64| with_world(lua, |w| w.scene_op_state(id)))?,
    )?;
    prim.set(
        "allow",
        lua.create_function(|lua, (id, allow): (i64, bool)| {
            with_world(lua, |w| w.scene_op_allow(id, allow))
        })?,
    )?;
    scenes.set("__prim", prim)?;
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
    lua.load(PRELUDE).set_name("=SceneManager").exec()
}

impl ScriptHost {
    /// Start of a frame: finish the scene operations carried out between frames and run
    /// their Lua `completed` callbacks (C# ones run in `SetTime`).
    pub(super) fn scene_ops_tick(&mut self) {
        let done = self.with_world(|w| w.scene_ops_tick());
        if done.is_empty() {
            return;
        }
        let tick = self
            .lua
            .globals()
            .get::<Table>("SceneManager")
            .and_then(|t| t.get::<mlua::Function>("__tick"));
        let Ok(tick) = tick else {
            return;
        };
        match tick.call::<Vec<String>>(done) {
            Ok(errors) => {
                for e in errors {
                    self.log(LogKind::Error, e);
                }
            }
            Err(e) => self.log(LogKind::Error, format!("SceneManager: {e}")),
        }
    }
}
