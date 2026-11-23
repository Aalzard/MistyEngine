//! The Player Input Manager while playing: players join by pressing a button on a device no
//! player uses yet (the Player Prefab is spawned and paired with it), split-screen camera
//! rects follow the player count, and the manager's scripts hear `OnPlayerJoined(player)` /
//! `OnPlayerLeft(player)`. Lua: `GetComponent("PlayerInputManager")` and
//! `PlayerInputManager.instance`.

use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, Source, instantiate, with_world};
use super::{ScriptHost, World};
use crate::input::actions::player_manager::split_rect;
use crate::input::actions::system::{DeviceId, InputSystem};
use crate::input::actions::{PlayerInput, PlayerInputManager, PlayerJoinBehavior};
use crate::scene::{Camera, Scene};

/// Players the manager has reported, between frames.
#[derive(Clone, Debug, Default)]
pub struct PlayerManagerState {
    /// Player Input objects as of the last frame (joined order).
    known: Vec<Entity>,
    /// `playerCount` while `OnPlayerJoined` runs: the players up to and including the one
    /// being reported (several joining in one frame hear 1, 2, … like they joined one by one).
    reporting: Option<usize>,
}

/// The scene's running Player Input Manager (the first enabled, active one).
pub fn manager(scene: &Scene) -> Option<(Entity, PlayerInputManager)> {
    if !scene.has_any::<PlayerInputManager>() {
        return None;
    }
    scene.walk().into_iter().find_map(|(e, _)| {
        let m = scene.world.get::<&PlayerInputManager>(e).ok()?;
        (m.enabled && scene.active_in_hierarchy(e)).then(|| (e, (*m).clone()))
    })
}

/// Active Player Input objects in Hierarchy order.
pub(crate) fn player_objects(scene: &Scene) -> Vec<Entity> {
    if !scene.has_any::<PlayerInput>() {
        return Vec::new();
    }
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.active_in_hierarchy(*e) && scene.world.get::<&PlayerInput>(*e).is_ok())
        .collect()
}

/// The camera of a player: the first Camera on the player object or under it.
pub(crate) fn player_camera(scene: &Scene, player: Entity) -> Option<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| *e == player || scene.is_ancestor(player, *e))
        .find(|e| scene.world.get::<&Camera>(*e).is_ok())
}

/// `playerCount` (Lua and C#): the players, or while `OnPlayerJoined` runs, the players up to
/// and including the one being reported.
pub(super) fn player_count(w: &World) -> usize {
    w.player_manager
        .reporting
        .unwrap_or_else(|| player_objects(&w.scene).len())
}

/// Spawn the manager's Player Prefab, paired with `device` when given.
pub(crate) fn join(
    w: &mut World,
    m: &PlayerInputManager,
    device: Option<DeviceId>,
) -> Result<Entity> {
    let Some(path) = m.player_prefab.clone().filter(|p| !p.trim().is_empty()) else {
        return Err(mlua::Error::runtime(
            "Player Input Manager: no Player Prefab assigned",
        ));
    };
    let e = instantiate(w, Source::Prefab(path), None)?;
    if let Some(d) = device {
        w.input_system.preferred.insert(e, d);
    }
    Ok(e)
}

/// One frame of joining and split-screen layout. Returns (joined, left) players.
fn frame(w: &mut World) -> Option<(Entity, Vec<Entity>, Vec<Entity>)> {
    let Some((me, m)) = manager(&w.scene) else {
        w.player_manager.known.clear();
        let World {
            input_system,
            input,
            ..
        } = w;
        input_system.set_joining(false, input);
        return None;
    };
    {
        let joining = m.join_behavior == PlayerJoinBehavior::JoinPlayersWhenButtonIsPressed
            && m.joining_enabled;
        let World {
            input_system,
            input,
            ..
        } = w;
        input_system.set_joining(joining, input);
    }
    let mut now = player_objects(&w.scene);
    let full = |n: usize| m.max_player_count > 0 && n >= m.max_player_count as usize;
    if m.join_behavior == PlayerJoinBehavior::JoinPlayersWhenButtonIsPressed
        && m.joining_enabled
        && m.player_prefab.is_some()
    {
        let claimed = w.input_system.claimed_devices();
        // The mouse and touches go with the keyboard and on-screen controls: they don't join.
        let joining: Vec<DeviceId> = InputSystem::pressed_devices(&w.input)
            .into_iter()
            .filter(|d| matches!(d, DeviceId::Keyboard | DeviceId::Gamepad(_)))
            .filter(|d| !claimed.contains(d))
            .collect();
        for d in joining {
            if full(now.len()) {
                break;
            }
            match join(w, &m, Some(d)) {
                Ok(e) => now.push(e),
                Err(err) => {
                    w.logs.push((super::LogKind::Error, err.to_string()));
                    break;
                }
            }
        }
    }
    let before = std::mem::take(&mut w.player_manager.known);
    let left: Vec<Entity> = before
        .iter()
        .copied()
        .filter(|e| !now.contains(e))
        .collect();
    // Joined order: the ones still here, then the new ones.
    let mut known: Vec<Entity> = before.into_iter().filter(|e| now.contains(e)).collect();
    let joined: Vec<Entity> = now.iter().copied().filter(|e| !known.contains(e)).collect();
    known.extend(joined.iter().copied());
    if m.split_screen {
        let screens = known.len().max(m.fixed_number_of_screens as usize);
        for (i, p) in known.iter().enumerate() {
            if let Some(cam) = player_camera(&w.scene, *p)
                && let Ok(mut c) = w.scene.world.get::<&mut Camera>(cam)
            {
                c.viewport = split_rect(i, screens);
            }
        }
    }
    w.player_manager.known = known;
    Some((me, joined, left))
}

impl ScriptHost {
    /// After Input.Update: Player Input Manager joining, split screen and its messages.
    pub(super) fn player_manager_frame(&mut self) {
        let Some((me, joined, left)) = self.with_world(frame) else {
            return;
        };
        if joined.is_empty() && left.is_empty() {
            return;
        }
        // Scripts on the new players get Awake before the manager hears about them.
        self.flush_spawned();
        let total = self.with_world(|w| w.player_manager.known.len());
        let first = total.saturating_sub(joined.len());
        for (list, method) in [(joined, "OnPlayerJoined"), (left, "OnPlayerLeft")] {
            for (i, p) in list.into_iter().enumerate() {
                let reporting = (method == "OnPlayerJoined").then_some(first + i + 1);
                self.with_world(|w| w.player_manager.reporting = reporting);
                let alive = self.with_world(|w| w.scene.contains(p));
                let arg = if alive {
                    self.lua
                        .create_userdata(LGameObject(p))
                        .ok()
                        .map(Value::UserData)
                } else {
                    None
                };
                let cs_arg = super::csharp::input::PendingArg::Player(p);
                self.call_if_defined_with(me, method, arg, cs_arg);
                self.cs_manager_event(me, p, method == "OnPlayerJoined");
                self.with_world(|w| w.player_manager.reporting = None);
            }
        }
    }
}

/// `GetComponent("PlayerInputManager")`.
#[derive(Clone, Copy)]
pub struct LPlayerInputManager(pub Entity);

fn get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&PlayerInputManager) -> R) -> Result<R> {
    super::ui_api::get(lua, e, f)
}

fn set(lua: &Lua, e: Entity, f: impl FnOnce(&mut PlayerInputManager)) -> Result<()> {
    super::ui_api::set(lua, e, f)
}

impl UserData for LPlayerInputManager {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("playerCount", |lua, _| with_world(lua, |w| player_count(w)));
        f.add_field_method_get("maxPlayerCount", |lua, m| {
            get(lua, m.0, |c| c.max_player_count)
        });
        f.add_field_method_get("joiningEnabled", |lua, m| {
            get(lua, m.0, |c| c.joining_enabled)
        });
        f.add_field_method_get("splitScreen", |lua, m| get(lua, m.0, |c| c.split_screen));
        f.add_field_method_set("splitScreen", |lua, m, v: bool| {
            set(lua, m.0, |c| c.split_screen = v)
        });
        f.add_field_method_get("enabled", |lua, m| get(lua, m.0, |c| c.enabled));
        f.add_field_method_set("enabled", |lua, m, v: bool| {
            set(lua, m.0, |c| c.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, m| Ok(LGameObject(m.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("EnableJoining", |lua, m, ()| {
            set(lua, m.0, |c| c.joining_enabled = true)
        });
        m.add_method("DisableJoining", |lua, m, ()| {
            set(lua, m.0, |c| c.joining_enabled = false)
        });
        // Spawn a player now (paired with free devices): its GameObject, or nil when full.
        m.add_method("JoinPlayer", |lua, m, ()| {
            let c = get(lua, m.0, Clone::clone)?;
            with_world(lua, |w| {
                let n = player_objects(&w.scene).len();
                if c.max_player_count > 0 && n >= c.max_player_count as usize {
                    return Ok(None);
                }
                join(w, &c, None).map(|e| Some(LGameObject(e)))
            })?
        });
    }
}

/// `GetComponent("PlayerInputManager")`; `Ok(None)` for other names.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if !matches!(name, "PlayerInputManager" | "Player Input Manager") {
        return Ok(None);
    }
    let has = with_world(lua, |w| w.scene.world.get::<&PlayerInputManager>(e).is_ok())?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LPlayerInputManager(e))?,
    )))
}

/// `PlayerInputManager.instance`: the running manager, or nil.
pub fn register(lua: &Lua) -> Result<()> {
    let t = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| {
            if key != "instance" {
                return Ok(Value::Nil);
            }
            let e = with_world(lua, |w| manager(&w.scene).map(|(e, _)| e))?;
            match e {
                Some(e) => Ok(Value::UserData(
                    lua.create_userdata(LPlayerInputManager(e))?,
                )),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    t.set_metatable(Some(meta))?;
    lua.globals().set("PlayerInputManager", t)
}

#[cfg(test)]
#[path = "player_manager_tests.rs"]
mod tests;
