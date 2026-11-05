//! `Assets`: load addressable assets by address or label while playing.
//!
//! ```lua
//! -- Callback style
//! local h = Assets.LoadAsync("Enemies/Ghost")
//! h:OnCompleted(function(h) Instantiate(h.result) end)
//!
//! -- Coroutine style: Assets.Run starts a coroutine the engine resumes every frame.
//! Assets.Run(function()
//!     local h = Assets.InstantiateAsync("Enemies/Ghost", self.transform):Await()
//!     Debug.Log("spawned", h.result.name)
//!     Assets.Release(h)                -- destroys the instance, unloads unused bundles
//! end)
//! ```
//!
//! A handle is a table: `id`, `key`, `isDone`, `status` ("Loading" / "Succeeded" / "Failed"),
//! `error`, `result` (the first asset path, or the GameObject for `InstantiateAsync`),
//! `results` (every path, for labels) and the methods `OnCompleted(fn)`, `Await()` (inside a
//! coroutine) and `Release()`. `Assets.GetStats()` returns the loaded / released counters.
//!
//! The handle bookkeeping is Lua (so `Await` can yield); the loads themselves are the
//! [`AssetSystem`] in the Lua state's app data, advanced once a frame by
//! [`ScriptHost::update`](super::ScriptHost::update).

use mlua::{Lua, Result, Table, Value};

use super::api::{LGameObject, LTransform, with_world};
use super::{LogKind, ScriptHost};
use crate::asset_groups::{AssetSystem, HandleKind, Status};

const PRELUDE: &str = r#"
local Assets = Assets
local prim = Assets.__prim
Assets.__prim = nil
local callbacks, running = {}, {}
local methods = {}
local Handle = {}
Handle.__index = function(h, k)
    local m = methods[k]
    if m then return m end
    return prim.field(rawget(h, "id"), k)
end
Handle.__tostring = function(h) return "AssetHandle(" .. tostring(rawget(h, "id")) .. ")" end
local function wrap(id) return setmetatable({ id = id }, Handle) end

function methods.OnCompleted(h, fn)
    if h.isDone then fn(h) return h end
    local list = callbacks[h.id]
    if not list then list = {} callbacks[h.id] = list end
    list[#list + 1] = fn
    return h
end
function methods.Await(h)
    if h.isDone then return h end
    local _, main = coroutine.running()
    if main then
        error("Await must run inside a coroutine: wrap the code in Assets.Run(function() ... end)", 2)
    end
    while not h.isDone do coroutine.yield() end
    return h
end
function methods.Release(h) Assets.Release(h) end

function Assets.LoadAsync(key) return wrap(prim.load(tostring(key))) end
function Assets.InstantiateAsync(key, parent) return wrap(prim.instantiate(tostring(key), parent)) end
function Assets.Release(target)
    if type(target) == "table" and getmetatable(target) == Handle then
        callbacks[target.id] = nil
        return prim.release(target.id)
    end
    return prim.release_object(target)
end
function Assets.Run(fn, ...)
    local co = coroutine.create(fn)
    local ok, err = coroutine.resume(co, ...)
    if not ok then error(err, 2) end
    if coroutine.status(co) ~= "dead" then running[#running + 1] = co end
    return co
end
Assets.GetStats = prim.stats

-- Called by the engine after each update with the handles that finished.
function Assets.__tick(done)
    local errors = {}
    for _, id in ipairs(done) do
        local list = callbacks[id]
        callbacks[id] = nil
        if list then
            local h = wrap(id)
            for _, fn in ipairs(list) do
                local ok, err = pcall(fn, h)
                if not ok then errors[#errors + 1] = "OnCompleted: " .. tostring(err) end
            end
        end
    end
    local still = {}
    for _, co in ipairs(running) do
        local ok, err = coroutine.resume(co)
        if not ok then
            errors[#errors + 1] = "Assets.Run: " .. tostring(err)
        elseif coroutine.status(co) ~= "dead" then
            still[#still + 1] = co
        end
    end
    running = still
    return errors
end
"#;

fn with_assets<R>(lua: &Lua, f: impl FnOnce(&mut AssetSystem) -> R) -> Result<R> {
    let mut sys = lua
        .app_data_mut::<AssetSystem>()
        .ok_or_else(|| mlua::Error::runtime("asset loading is not available"))?;
    Ok(f(&mut sys))
}

fn entity_of(v: &Value) -> Option<hecs::Entity> {
    match v {
        Value::UserData(ud) if ud.is::<LGameObject>() => {
            ud.borrow::<LGameObject>().ok().map(|g| g.0)
        }
        Value::UserData(ud) if ud.is::<LTransform>() => ud.borrow::<LTransform>().ok().map(|t| t.0),
        _ => None,
    }
}

/// The global `Assets` table.
pub fn register(lua: &Lua) -> Result<()> {
    let assets = lua.create_table()?;
    let prim = lua.create_table()?;
    prim.set(
        "load",
        lua.create_function(|lua, key: String| {
            with_assets(lua, |s| s.load(&key, HandleKind::Load))
        })?,
    )?;
    prim.set(
        "instantiate",
        lua.create_function(|lua, (key, parent): (String, Value)| {
            let parent = entity_of(&parent);
            with_assets(lua, |s| s.load(&key, HandleKind::Instantiate { parent }))
        })?,
    )?;
    prim.set(
        "release",
        lua.create_function(|lua, id: u64| {
            let released = with_assets(lua, |s| s.release(id))?;
            let Some(h) = released else {
                return Ok(false);
            };
            if let Some(e) = h.instance {
                with_world(lua, |w| w.destroy.push(e))?;
            }
            Ok(true)
        })?,
    )?;
    prim.set(
        "release_object",
        lua.create_function(|lua, target: Value| {
            let Some(e) = entity_of(&target) else {
                return Err(mlua::Error::runtime(
                    "Assets.Release expects a handle or an object from InstantiateAsync",
                ));
            };
            let found = with_assets(lua, |s| {
                s.handle_for_instance(e).and_then(|id| s.release(id))
            })?;
            match found {
                Some(_) => {
                    with_world(lua, |w| w.destroy.push(e))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        })?,
    )?;
    prim.set(
        "field",
        lua.create_function(|lua, (id, key): (u64, String)| field(lua, id, &key))?,
    )?;
    prim.set(
        "stats",
        lua.create_function(|lua, ()| {
            let st = with_assets(lua, |s| s.stats())?;
            let t = lua.create_table()?;
            t.set("loaded", st.loaded)?;
            t.set("released", st.released)?;
            t.set("handles", st.live_handles)?;
            t.set("bundles", st.live_bundles)?;
            t.set("bundlesLoaded", st.bundles_loaded)?;
            t.set("bundlesUnloaded", st.bundles_unloaded)?;
            Ok(t)
        })?,
    )?;
    assets.set("__prim", prim)?;
    lua.globals().set("Assets", assets)?;
    lua.load(PRELUDE).set_name("=Assets").exec()
}

fn field(lua: &Lua, id: u64, key: &str) -> Result<Value> {
    let h = with_assets(lua, |s| s.handle(id).cloned())?;
    let Some(h) = h else {
        // Released handles read as done and empty.
        return Ok(match key {
            "isDone" => Value::Boolean(true),
            "status" => Value::String(lua.create_string("Released")?),
            _ => Value::Nil,
        });
    };
    Ok(match key {
        "key" => Value::String(lua.create_string(&h.key)?),
        "isDone" => Value::Boolean(h.status != Status::Loading),
        "status" => Value::String(lua.create_string(h.status.name())?),
        "error" => match &h.error {
            Some(e) => Value::String(lua.create_string(e)?),
            None => Value::Nil,
        },
        "result" => match (h.kind, h.instance, h.status) {
            (HandleKind::Instantiate { .. }, Some(e), _) => {
                Value::UserData(lua.create_userdata(LGameObject(e))?)
            }
            (HandleKind::Load, _, Status::Succeeded) => match h.paths.first() {
                Some(p) => Value::String(lua.create_string(p)?),
                None => Value::Nil,
            },
            _ => Value::Nil,
        },
        "results" => {
            let t: Table = lua.create_table()?;
            if h.status == Status::Succeeded {
                for (i, p) in h.paths.iter().enumerate() {
                    t.set(i + 1, p.as_str())?;
                }
            }
            Value::Table(t)
        }
        "progress" => Value::Number(if h.status == Status::Loading {
            0.0
        } else {
            1.0
        }),
        _ => Value::Nil,
    })
}

impl ScriptHost {
    /// Replace the asset loader (the editor picks one per its Play Mode Script).
    pub fn set_assets(&mut self, assets: AssetSystem) {
        self.lua.set_app_data(assets);
    }

    /// The asset loader of this game (between frames).
    pub fn with_assets<R>(&self, f: impl FnOnce(&mut AssetSystem) -> R) -> Option<R> {
        self.lua
            .app_data_mut::<AssetSystem>()
            .map(|mut s| f(&mut s))
    }

    /// Finish loads, instantiate what `InstantiateAsync` asked for, fire `OnCompleted`
    /// callbacks and resume `Assets.Run` coroutines.
    pub(super) fn assets_update(&mut self) {
        let done = match self.lua.app_data_mut::<AssetSystem>() {
            Some(mut s) => s.update(),
            None => return,
        };
        let mut spawned = false;
        for id in &done {
            let Some((kind, status, path)) = self
                .with_assets(|s| {
                    s.handle(*id)
                        .map(|h| (h.kind, h.status, h.paths.first().cloned()))
                })
                .flatten()
            else {
                continue;
            };
            let (HandleKind::Instantiate { parent }, Status::Succeeded, Some(path)) =
                (kind, status, path)
            else {
                continue;
            };
            let made = self.with_world(|w| {
                let e = super::api::instantiate_prefab_path(w, &path, None)?;
                if let Some(p) = parent.filter(|p| w.scene.contains(*p)) {
                    w.scene.set_parent(e, Some(p), None, false);
                }
                Ok::<_, mlua::Error>(e)
            });
            self.with_assets(|s| {
                if let Some(h) = s.handle_mut(*id) {
                    match made {
                        Ok(e) => h.instance = Some(e),
                        Err(err) => {
                            h.status = Status::Failed;
                            h.error = Some(err.to_string());
                        }
                    }
                }
            });
            spawned = true;
        }
        if spawned {
            self.flush_spawned();
        }
        let tick = self
            .lua
            .globals()
            .get::<Table>("Assets")
            .and_then(|a| a.get::<mlua::Function>("__tick"));
        let Ok(tick) = tick else {
            return;
        };
        match tick.call::<Vec<String>>(done) {
            Ok(errors) => {
                for e in errors {
                    self.log(LogKind::Error, e);
                }
            }
            Err(e) => self.log(LogKind::Error, format!("Assets: {e}")),
        }
        self.flush_spawned();
    }
}
