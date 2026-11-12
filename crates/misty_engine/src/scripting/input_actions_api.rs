//! Lua API of the Input System: `InputActions.Load(path)` / `Instantiate(path)`, action maps
//! and actions (`ReadValue`, `IsPressed`, `WasPressedThisFrame`, `WasReleasedThisFrame`,
//! `WasPerformedThisFrame`, `started` / `performed` / `canceled` listeners, `Enable` /
//! `Disable`), binding overrides and `PerformInteractiveRebinding`, `GetComponent
//! ("PlayerInput")`, `PlayerInput.all`, `Gamepad.current` (rumble), `Keyboard.current`,
//! `Mouse.current` and `Touchscreen.current`; and the dispatch of action events after each
//! Input System update.

use glam::{Vec2, Vec3};
use hecs::Entity;
use mlua::{
    Function, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};

use super::api::{LGameObject, LVec3, with_world};
use super::{Instance, ScriptHost, World};
use crate::input::actions::path::{ControlPath, DeviceFilter, PAD_BUTTONS};
use crate::input::actions::{
    ActionEvent, ActionType, Phase, PlayerInput, PlayerInputBehavior, SystemEvent,
    player_input::message_name,
};

const LISTENERS: &str = "misty.input.listeners";
const REBINDS: &str = "misty.input.rebinds";

fn err(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(msg.into())
}

/// A registry table by name (created on first use).
fn registry_table(lua: &Lua, name: &str) -> Result<Table> {
    match lua.named_registry_value::<Option<Table>>(name)? {
        Some(t) => Ok(t),
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(name, &t)?;
            Ok(t)
        }
    }
}

/// Drop the PlayerInput listeners of destroyed objects (see
/// [`super::ui_widgets_api::forget_listeners`]).
pub(super) fn forget(lua: &Lua, doomed: &[Entity]) {
    super::ui_widgets_api::forget_listeners(lua, LISTENERS, "p", doomed);
}

fn listeners(lua: &Lua, key: &str) -> Vec<Function> {
    let Ok(t) = registry_table(lua, LISTENERS) else {
        return Vec::new();
    };
    match t.get::<Option<Table>>(key) {
        Ok(Some(list)) => list.sequence_values::<Function>().flatten().collect(),
        _ => Vec::new(),
    }
}

/// Number or Vector2 for Lua.
pub fn value_to_lua(lua: &Lua, v: crate::input::actions::Value) -> Result<Value> {
    use crate::input::actions::Value as V;
    Ok(match v {
        V::Float(f) => Value::Number(f64::from(f)),
        V::Vector2(v) => Value::UserData(lua.create_userdata(LVec3(v.extend(0.0)))?),
    })
}

fn vec2(lua: &Lua, v: Vec2) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LVec3(v.extend(0.0)))?))
}

// ---- listener events ----------------------------------------------------------------------

/// An event with listeners (`action.performed`, `playerInput.onActionTriggered`).
#[derive(Clone)]
pub struct LInputEvent {
    key: String,
}

impl UserData for LInputEvent {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddListener", |lua, ev, f: Function| {
            let t = registry_table(lua, LISTENERS)?;
            let list = match t.get::<Option<Table>>(ev.key.as_str())? {
                Some(l) => l,
                None => {
                    let l = lua.create_table()?;
                    t.set(ev.key.as_str(), &l)?;
                    l
                }
            };
            list.push(f)
        });
        m.add_method("RemoveListener", |lua, ev, f: Function| {
            let t = registry_table(lua, LISTENERS)?;
            let Some(list) = t.get::<Option<Table>>(ev.key.as_str())? else {
                return Ok(());
            };
            let kept = lua.create_table()?;
            for g in list.sequence_values::<Function>().flatten() {
                if g != f {
                    kept.push(g)?;
                }
            }
            t.set(ev.key.as_str(), kept)
        });
        m.add_method("RemoveAllListeners", |lua, ev, ()| {
            registry_table(lua, LISTENERS)?.set(ev.key.as_str(), Value::Nil)
        });
    }
}

fn action_key(set: usize, map: usize, action: usize, phase: &str) -> String {
    format!("a{set}:{map}:{action}:{phase}")
}

fn player_key(e: Entity, name: &str) -> String {
    format!("p{}:{name}", e.to_bits().get())
}

// ---- asset, map, action -------------------------------------------------------------------

fn with_set<R>(
    lua: &Lua,
    set: usize,
    f: impl FnOnce(&mut crate::input::actions::ActionSet) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        w.input_system
            .set_mut(set)
            .map(f)
            .ok_or_else(|| err("these input actions are gone"))
    })?
}

/// `InputActions.Load(path)`, `playerInput.actions`.
#[derive(Clone, Copy)]
pub struct LActions(pub usize);

/// An action map.
#[derive(Clone, Copy)]
pub struct LActionMap {
    pub set: usize,
    pub map: usize,
}

/// An action.
#[derive(Clone, Copy)]
pub struct LAction {
    pub set: usize,
    pub map: usize,
    pub action: usize,
}

fn find_action(lua: &Lua, set: usize, name: &str) -> Result<Option<LAction>> {
    with_set(lua, set, |s| {
        s.asset
            .find_action(name)
            .map(|(map, action)| LAction { set, map, action })
    })
}

impl UserData for LActions {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, a| {
            with_set(lua, a.0, |s| s.asset.name.clone())
        });
        f.add_field_method_get("enabled", |lua, a| with_set(lua, a.0, |s| s.any_enabled()));
        f.add_field_method_get("actionMaps", |lua, a| {
            let n = with_set(lua, a.0, |s| s.asset.maps.len())?;
            lua.create_sequence_from((0..n).map(|map| LActionMap { set: a.0, map }))
        });
        f.add_field_method_get("controlSchemes", |lua, a| {
            let names = with_set(lua, a.0, |s| {
                s.asset
                    .control_schemes
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
            })?;
            lua.create_sequence_from(names)
        });
        f.add_field_method_get("bindingMask", |lua, a| {
            with_set(lua, a.0, |s| s.binding_mask.clone())
        });
        f.add_field_method_set("bindingMask", |lua, a, mask: Option<String>| {
            with_set(lua, a.0, |s| {
                s.binding_mask = mask.filter(|m| !m.is_empty())
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("FindAction", |lua, a, name: String| {
            find_action(lua, a.0, &name)
        });
        m.add_method("FindActionMap", |lua, a, name: String| {
            with_set(lua, a.0, |s| {
                s.asset
                    .find_map(&name)
                    .map(|map| LActionMap { set: a.0, map })
            })
        });
        m.add_method("Enable", |lua, a, ()| {
            with_set(lua, a.0, |s| s.set_all_enabled(true))
        });
        m.add_method("Disable", |lua, a, ()| {
            with_set(lua, a.0, |s| s.set_all_enabled(false))
        });
        m.add_method("SaveBindingOverridesAsJson", |lua, a, ()| {
            with_set(lua, a.0, |s| s.overrides_json())
        });
        m.add_method("LoadBindingOverridesFromJson", |lua, a, json: String| {
            with_set(lua, a.0, |s| s.load_overrides_json(&json))?
                .map(|_| ())
                .map_err(|e| err(format!("LoadBindingOverridesFromJson: {e}")))
        });
        m.add_method("RemoveAllBindingOverrides", |lua, a, ()| {
            with_set(lua, a.0, |s| s.clear_overrides(None))
        });
        // `actions["Move"]`, `actions["Player/Jump"]`.
        m.add_meta_method(MetaMethod::Index, |lua, a, key: String| {
            find_action(lua, a.0, &key)
        });
        m.add_meta_method(MetaMethod::ToString, |lua, a, ()| {
            with_set(lua, a.0, |s| format!("InputActions({})", s.asset.name))
        });
    }
}

impl UserData for LActionMap {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, m| {
            with_set(lua, m.set, |s| s.asset.maps[m.map].name.clone())
        });
        f.add_field_method_get("enabled", |lua, m| {
            with_set(lua, m.set, |s| s.map_enabled(m.map))
        });
        f.add_field_method_get("actions", |lua, m| {
            let n = with_set(lua, m.set, |s| s.asset.maps[m.map].actions.len())?;
            lua.create_sequence_from((0..n).map(|action| LAction {
                set: m.set,
                map: m.map,
                action,
            }))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Enable", |lua, m, ()| {
            with_set(lua, m.set, |s| s.set_map_enabled(m.map, true))
        });
        methods.add_method("Disable", |lua, m, ()| {
            with_set(lua, m.set, |s| s.set_map_enabled(m.map, false))
        });
        let find = |lua: &Lua, m: &LActionMap, name: String| {
            with_set(lua, m.set, |s| {
                s.asset.maps[m.map]
                    .actions
                    .iter()
                    .position(|a| a.name.eq_ignore_ascii_case(&name))
                    .map(|action| LAction {
                        set: m.set,
                        map: m.map,
                        action,
                    })
            })
        };
        methods.add_method("FindAction", move |lua, m, name: String| find(lua, m, name));
        methods.add_meta_method(MetaMethod::Index, move |lua, m, name: String| {
            find(lua, m, name)
        });
    }
}

fn action_state<R>(
    lua: &Lua,
    a: &LAction,
    f: impl FnOnce(&crate::input::actions::state::ActionState) -> R,
) -> Result<R> {
    with_set(lua, a.set, |s| s.state(a.map, a.action).map(f))?.ok_or_else(|| err("no such action"))
}

/// A flat binding index from Lua (0-based), checked.
fn flat_count(lua: &Lua, a: &LAction) -> Result<usize> {
    with_set(lua, a.set, |s| {
        s.asset.maps[a.map].actions[a.action].flat_bindings().len()
    })
}

impl UserData for LAction {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, a| {
            with_set(lua, a.set, |s| {
                s.asset.maps[a.map].actions[a.action].name.clone()
            })
        });
        f.add_field_method_get("id", |lua, a| {
            with_set(lua, a.set, |s| {
                s.asset.maps[a.map].actions[a.action].id.clone()
            })
        });
        f.add_field_method_get("type", |lua, a| {
            with_set(lua, a.set, |s| {
                match s.asset.maps[a.map].actions[a.action].kind {
                    ActionType::Value => "Value",
                    ActionType::Button => "Button",
                    ActionType::PassThrough => "PassThrough",
                }
            })
        });
        f.add_field_method_get("phase", |lua, a| action_state(lua, a, |s| s.phase.name()));
        f.add_field_method_get("enabled", |lua, a| action_state(lua, a, |s| s.enabled));
        f.add_field_method_get("triggered", |lua, a| {
            action_state(lua, a, |s| s.performed_this_frame)
        });
        f.add_field_method_get("activeControl", |lua, a| {
            action_state(lua, a, |s| s.active_control.clone())
        });
        f.add_field_method_get("actionMap", |_, a| {
            Ok(LActionMap {
                set: a.set,
                map: a.map,
            })
        });
        f.add_field_method_get("started", |_, a| Ok(event(a, "started")));
        f.add_field_method_get("performed", |_, a| Ok(event(a, "performed")));
        f.add_field_method_get("canceled", |_, a| Ok(event(a, "canceled")));
        f.add_field_method_get("bindings", |lua, a| {
            let rows = with_set(lua, a.set, |s| {
                let def = &s.asset.maps[a.map].actions[a.action];
                def.flat_bindings()
                    .iter()
                    .enumerate()
                    .map(|(i, fb)| {
                        let b = &def.bindings[fb.binding];
                        let (name, path, groups, composite) = match fb.part {
                            None => (
                                b.name.clone(),
                                b.path.clone(),
                                b.groups.clone(),
                                b.is_composite(),
                            ),
                            Some(p) => (
                                b.parts[p].name.clone(),
                                b.parts[p].path.clone(),
                                b.parts[p].groups.clone(),
                                false,
                            ),
                        };
                        let effective = s.effective_path(a.map, a.action, i).unwrap_or_default();
                        (name, path, effective, groups, composite, fb.part.is_some())
                    })
                    .collect::<Vec<_>>()
            })?;
            let list = lua.create_table()?;
            for (name, path, effective, groups, composite, part) in rows {
                let t = lua.create_table()?;
                t.set("name", name)?;
                t.set("path", path.clone())?;
                t.set("effectivePath", effective.clone())?;
                if path != effective {
                    t.set("overridePath", effective)?;
                }
                t.set("groups", groups)?;
                t.set("isComposite", composite)?;
                t.set("isPartOfComposite", part)?;
                list.push(t)?;
            }
            Ok(list)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("ReadValue", |lua, a, ()| {
            let v = action_state(lua, a, |s| s.value)?;
            value_to_lua(lua, v)
        });
        m.add_method("IsPressed", |lua, a, ()| {
            action_state(lua, a, |s| s.pressed)
        });
        m.add_method("WasPressedThisFrame", |lua, a, ()| {
            action_state(lua, a, |s| s.pressed_this_frame)
        });
        m.add_method("WasReleasedThisFrame", |lua, a, ()| {
            action_state(lua, a, |s| s.released_this_frame)
        });
        m.add_method("WasPerformedThisFrame", |lua, a, ()| {
            action_state(lua, a, |s| s.performed_this_frame)
        });
        m.add_method("IsInProgress", |lua, a, ()| {
            action_state(lua, a, |s| {
                matches!(s.phase, Phase::Started | Phase::Performed)
            })
        });
        m.add_method("Enable", |lua, a, ()| {
            with_set(lua, a.set, |s| s.set_action_enabled(a.map, a.action, true))
        });
        m.add_method("Disable", |lua, a, ()| {
            with_set(lua, a.set, |s| s.set_action_enabled(a.map, a.action, false))
        });
        m.add_method("GetBindingDisplayString", |lua, a, index: Option<usize>| {
            with_set(lua, a.set, |s| s.display_string(a.map, a.action, index))
        });
        // `ApplyBindingOverride(path)` (first binding) or `ApplyBindingOverride(index, path)`.
        m.add_method(
            "ApplyBindingOverride",
            |lua, a, (x, y): (Value, Option<String>)| {
                let (index, path) = match (x, y) {
                    (Value::Integer(i), Some(p)) => (i.max(0) as usize, p),
                    (Value::Number(n), Some(p)) => (n.max(0.0) as usize, p),
                    (Value::String(p), None) => (0, p.to_str()?.to_owned()),
                    _ => return Err(err("ApplyBindingOverride(index, path) or (path)")),
                };
                if index >= flat_count(lua, a)? {
                    return Err(err(format!("binding index {index} is out of range")));
                }
                with_set(lua, a.set, |s| {
                    s.set_override(a.map, a.action, index, &path);
                })
            },
        );
        m.add_method("RemoveBindingOverride", |lua, a, index: usize| {
            with_set(lua, a.set, |s| {
                s.set_override(a.map, a.action, index, "");
            })
        });
        m.add_method("RemoveAllBindingOverrides", |lua, a, ()| {
            with_set(lua, a.set, |s| s.clear_overrides(Some((a.map, a.action))))
        });
        m.add_method(
            "PerformInteractiveRebinding",
            |lua, a, index: Option<usize>| {
                let id = with_world(lua, |w| {
                    w.input_system.new_rebind(a.set, a.map, a.action, index)
                })?
                .map_err(|e| err(format!("PerformInteractiveRebinding: {e}")))?;
                Ok(LRebind(id))
            },
        );
        m.add_meta_method(MetaMethod::Eq, |_, a, b: mlua::UserDataRef<LAction>| {
            Ok(a.set == b.set && a.map == b.map && a.action == b.action)
        });
        m.add_meta_method(MetaMethod::ToString, |lua, a, ()| {
            with_set(lua, a.set, |s| {
                format!(
                    "{}/{}",
                    s.asset.maps[a.map].name, s.asset.maps[a.map].actions[a.action].name
                )
            })
        });
    }
}

fn event(a: &LAction, phase: &str) -> LInputEvent {
    LInputEvent {
        key: action_key(a.set, a.map, a.action, phase),
    }
}

// ---- rebinding ----------------------------------------------------------------------------

/// A running `PerformInteractiveRebinding` operation.
#[derive(Clone, Copy)]
pub struct LRebind(pub u64);

fn rebind_entry(lua: &Lua, id: u64) -> Result<Table> {
    let t = registry_table(lua, REBINDS)?;
    match t.get::<Option<Table>>(id)? {
        Some(e) => Ok(e),
        None => {
            let e = lua.create_table()?;
            t.set(id, &e)?;
            Ok(e)
        }
    }
}

fn edit_rebind(
    lua: &Lua,
    id: u64,
    f: impl FnOnce(&mut crate::input::actions::system::Rebind),
) -> Result<()> {
    with_world(lua, |w| {
        if let Some(r) = w.input_system.rebind_mut(id) {
            f(r);
        }
    })
}

impl UserData for LRebind {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("started", |lua, r| {
            let running = with_world(lua, |w| {
                w.input_system.rebind(r.0).is_some_and(|x| x.started)
            })?;
            let done: Option<String> = rebind_entry(lua, r.0)?.get("done")?;
            Ok(running || done.is_some())
        });
        f.add_field_method_get("completed", |lua, r| {
            let done: Option<String> = rebind_entry(lua, r.0)?.get("done")?;
            Ok(done.as_deref() == Some("complete"))
        });
        f.add_field_method_get("canceled", |lua, r| {
            let done: Option<String> = rebind_entry(lua, r.0)?.get("done")?;
            Ok(done.as_deref() == Some("cancel"))
        });
        f.add_field_method_get("selectedControl", |lua, r| {
            rebind_entry(lua, r.0)?.get::<Option<String>>("path")
        });
        f.add_field_method_get("bindingIndex", |lua, r| {
            with_world(lua, |w| w.input_system.rebind(r.0).map(|x| x.flat))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_function(
            "WithControlsExcluding",
            |lua, (r, path): (mlua::UserDataRef<LRebind>, String)| {
                edit_rebind(lua, r.0, |x| x.excluded.push(path))?;
                Ok(*r)
            },
        );
        m.add_function(
            "WithCancelingThrough",
            |lua, (r, path): (mlua::UserDataRef<LRebind>, String)| {
                edit_rebind(lua, r.0, |x| x.cancel = vec![path])?;
                Ok(*r)
            },
        );
        m.add_function(
            "WithTargetBinding",
            |lua, (r, index): (mlua::UserDataRef<LRebind>, usize)| {
                with_world(lua, |w| w.input_system.retarget_rebind(r.0, index))?.map_err(err)?;
                Ok(*r)
            },
        );
        m.add_function(
            "OnMatchWaitForAnother",
            |lua, (r, secs): (mlua::UserDataRef<LRebind>, f64)| {
                edit_rebind(lua, r.0, |x| x.wait = secs.max(0.0))?;
                Ok(*r)
            },
        );
        m.add_function(
            "OnComplete",
            |lua, (r, f): (mlua::UserDataRef<LRebind>, Function)| {
                rebind_entry(lua, r.0)?.set("complete", f)?;
                Ok(*r)
            },
        );
        m.add_function(
            "OnCancel",
            |lua, (r, f): (mlua::UserDataRef<LRebind>, Function)| {
                rebind_entry(lua, r.0)?.set("cancel", f)?;
                Ok(*r)
            },
        );
        m.add_function("Start", |lua, r: mlua::UserDataRef<LRebind>| {
            with_world(lua, |w| w.input_system.start_rebind(r.0))?;
            Ok(*r)
        });
        m.add_method("Cancel", |lua, r, ()| {
            let events = with_world(lua, |w| w.input_system.cancel_rebind(r.0))?;
            // Run the cancel callback right away.
            for e in events {
                if let SystemEvent::RebindCanceled { id } = e {
                    rebind_done(lua, id, None)?;
                }
            }
            Ok(())
        });
        m.add_method("Dispose", |lua, r, ()| {
            with_world(lua, |w| {
                w.input_system.cancel_rebind(r.0);
            })?;
            registry_table(lua, REBINDS)?.set(r.0, Value::Nil)
        });
    }
}

/// Mark a rebind finished and run its callback (with the operation).
fn rebind_done(lua: &Lua, id: u64, path: Option<String>) -> Result<()> {
    let e = rebind_entry(lua, id)?;
    let which = if path.is_some() { "complete" } else { "cancel" };
    e.set("done", which)?;
    if let Some(p) = path {
        e.set("path", p)?;
    }
    if let Some(f) = e.get::<Option<Function>>(which)? {
        f.call::<()>(LRebind(id))?;
    }
    Ok(())
}

// ---- Player Input -------------------------------------------------------------------------

/// `GetComponent("PlayerInput")`.
#[derive(Clone, Copy)]
pub struct LPlayerInput(pub Entity);

fn player<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&mut World, &crate::input::actions::system::PlayerState) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        let w: &mut World = w;
        if w.input_system.player(e).is_none() {
            let World {
                input_system,
                scene,
                input,
                ..
            } = w;
            input_system.sync_players(scene, input);
        }
        let p = w.input_system.player(e).cloned().ok_or_else(|| {
            err("this Player Input isn't playing (no Actions asset, or inactive)")
        })?;
        Ok(f(w, &p))
    })?
}

impl UserData for LPlayerInput {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, p| Ok(LGameObject(p.0)));
        f.add_field_method_get("actions", |lua, p| player(lua, p.0, |_, s| LActions(s.set)));
        f.add_field_method_get("playerIndex", |lua, p| player(lua, p.0, |_, s| s.index));
        f.add_field_method_get("currentControlScheme", |lua, p| {
            player(lua, p.0, |_, s| s.scheme.clone())
        });
        f.add_field_method_get("currentActionMap", |lua, p| {
            player(lua, p.0, |_, s| {
                s.map.map(|map| LActionMap { set: s.set, map })
            })
        });
        f.add_field_method_set("currentActionMap", |lua, p, name: String| {
            with_world(lua, |w| w.input_system.switch_map(p.0, &name))?.map_err(err)
        });
        f.add_field_method_get("inputIsActive", |lua, p| player(lua, p.0, |_, s| s.active));
        f.add_field_method_get("notificationBehavior", |lua, p| {
            player(lua, p.0, |_, s| s.behavior.label())
        });
        f.add_field_method_get("devices", |lua, p| {
            let names = player(lua, p.0, |w, s| {
                s.devices
                    .iter()
                    .map(|d| d.name(&w.input))
                    .collect::<Vec<_>>()
            })?;
            lua.create_sequence_from(names)
        });
        for name in [
            "onActionTriggered",
            "onControlsChanged",
            "onDeviceLost",
            "onDeviceRegained",
        ] {
            f.add_field_method_get(name, move |_, p| {
                Ok(LInputEvent {
                    key: player_key(p.0, name),
                })
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SwitchCurrentActionMap", |lua, p, name: String| {
            with_world(lua, |w| w.input_system.switch_map(p.0, &name))?.map_err(err)
        });
        m.add_method("SwitchCurrentControlScheme", |lua, p, name: String| {
            with_world(lua, |w| {
                let World {
                    input_system,
                    input,
                    ..
                } = w;
                input_system.switch_scheme(p.0, &name, input)
            })?
            .map_err(err)
        });
        m.add_method("ActivateInput", |lua, p, ()| {
            with_world(lua, |w| w.input_system.set_player_active(p.0, true))
        });
        m.add_method("DeactivateInput", |lua, p, ()| {
            with_world(lua, |w| w.input_system.set_player_active(p.0, false))
        });
        // `GetDevice("Gamepad")`: the player's first paired gamepad (nil when none).
        m.add_method("GetDevice", |lua, p, kind: String| {
            if !kind.eq_ignore_ascii_case("gamepad") {
                return Ok(None);
            }
            player(lua, p.0, |w, s| {
                s.devices.iter().find_map(|d| match d {
                    crate::input::actions::DeviceId::Gamepad(i)
                        if w.input
                            .joysticks
                            .get(*i)
                            .is_some_and(|j| !j.name.is_empty()) =>
                    {
                        Some(LGamepad(*i))
                    }
                    _ => None,
                })
            })
        });
    }
}

/// `GetComponent("PlayerInput")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if !matches!(name, "PlayerInput" | "Player Input") {
        return Ok(None);
    }
    let has = with_world(lua, |w| w.scene.world.get::<&PlayerInput>(e).is_ok())?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(lua.create_userdata(LPlayerInput(e))?)))
}

// ---- devices ------------------------------------------------------------------------------

/// A gamepad by joystick slot.
#[derive(Clone, Copy)]
pub struct LGamepad(pub usize);

fn pad<R>(lua: &Lua, i: usize, f: impl FnOnce(&crate::input::Joystick) -> R) -> Result<R> {
    with_world(lua, |w| w.input.joysticks.get(i).map(f))?.ok_or_else(|| err("the gamepad is gone"))
}

/// `{ isPressed, wasPressedThisFrame, wasReleasedThisFrame, value }` for a button.
fn button_table(lua: &Lua, held: bool, pressed: bool, released: bool, value: f32) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("isPressed", held)?;
    t.set("wasPressedThisFrame", pressed)?;
    t.set("wasReleasedThisFrame", released)?;
    t.set("value", value)?;
    Ok(t)
}

impl UserData for LGamepad {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, g| pad(lua, g.0, |j| j.name.clone()));
        f.add_field_method_get("index", |_, g| Ok(g.0));
        for (name, axes) in [("leftStick", (0usize, 1usize)), ("rightStick", (3, 4))] {
            f.add_field_method_get(name, move |lua, g| {
                let v = pad(lua, g.0, |j| Vec2::new(j.axes[axes.0], -j.axes[axes.1]))?;
                vec2(lua, v)
            });
        }
        f.add_field_method_get("dpad", |lua, g| {
            let v = pad(lua, g.0, |j| Vec2::new(j.axes[5], j.axes[6]))?;
            vec2(lua, v)
        });
        f.add_field_method_get("leftTrigger", |lua, g| pad(lua, g.0, |j| j.axes[8]));
        f.add_field_method_get("rightTrigger", |lua, g| pad(lua, g.0, |j| j.axes[9]));
        for (name, index, _) in PAD_BUTTONS {
            f.add_field_method_get(*name, move |lua, g| {
                let (h, p, r) = pad(lua, g.0, |j| {
                    (j.held[*index], j.pressed[*index], j.released[*index])
                })?;
                button_table(lua, h, p, r, if h { 1.0 } else { 0.0 })
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetMotorSpeeds", |lua, g, (low, high): (f32, f32)| {
            with_world(lua, |w| {
                w.input_system.set_motor_speeds(Some(g.0), low, high)
            })
        });
        m.add_method("ResetHaptics", |lua, g, ()| {
            with_world(lua, |w| {
                w.input_system.set_motor_speeds(Some(g.0), 0.0, 0.0)
            })
        });
        m.add_method("PauseHaptics", |lua, g, ()| {
            with_world(lua, |w| {
                w.input_system.set_motor_speeds(Some(g.0), 0.0, 0.0)
            })
        });
        m.add_meta_method(MetaMethod::ToString, |lua, g, ()| {
            pad(lua, g.0, |j| j.name.clone())
        });
    }
}

/// `Keyboard.current` / `Mouse.current`: controls by name (`spaceKey`, `wKey`, `leftButton`,
/// `delta`, `position`, `scroll`).
#[derive(Clone, Copy)]
pub struct LDevice(pub crate::input::actions::path::DeviceKind);

impl UserData for LDevice {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, d, key: String| {
            use crate::input::actions::path::DeviceKind;
            let control = key.strip_suffix("Key").unwrap_or(&key);
            let path = format!("{}/{control}", d.0.layout());
            let Ok(c) = ControlPath::parse(&path) else {
                return Ok(Value::Nil);
            };
            let (value, pressed, released) = with_world(lua, |w| {
                let v = c.read(&w.input, &DeviceFilter::all());
                let edge = |set: &std::collections::HashSet<String>| match &c.control {
                    crate::input::actions::path::Control::Keys(k) => {
                        k.iter().any(|k| set.contains(*k))
                    }
                    _ => false,
                };
                let (p, r) = match (&c.control, d.0) {
                    (crate::input::actions::path::Control::MouseButton(i), DeviceKind::Mouse) => {
                        (w.input.mouse_pressed[*i], w.input.mouse_released[*i])
                    }
                    _ => (edge(&w.input.pressed), edge(&w.input.released)),
                };
                (v, p, r)
            })?;
            if c.is_vector() {
                return value_to_lua(lua, value);
            }
            let f = value.as_float();
            Ok(Value::Table(button_table(
                lua,
                f >= 0.5,
                pressed,
                released,
                f,
            )?))
        });
    }
}

/// `Touchscreen.current`: `primaryTouch` and `touches`.
#[derive(Clone, Copy)]
pub struct LTouchscreen;

fn touch_table(lua: &Lua, t: &crate::input::Touch, screen_h: f32) -> Result<Table> {
    let tt = lua.create_table()?;
    tt.set("fingerId", t.finger_id)?;
    tt.set(
        "position",
        LVec3(Vec3::new(t.position.x, screen_h - t.position.y, 0.0)),
    )?;
    tt.set(
        "deltaPosition",
        LVec3(Vec3::new(t.delta.x, -t.delta.y, 0.0)),
    )?;
    tt.set("delta", LVec3(Vec3::new(t.delta.x, -t.delta.y, 0.0)))?;
    tt.set("phase", t.phase.name())?;
    tt.set("isInProgress", t.phase.is_down())?;
    Ok(tt)
}

/// Touches as Lua tables (positions from the bottom-left).
pub fn touches(lua: &Lua) -> Result<Vec<Table>> {
    let (list, h) = with_world(lua, |w| (w.input.touches.clone(), w.input.screen_size.y))?;
    list.iter().map(|t| touch_table(lua, t, h)).collect()
}

impl UserData for LTouchscreen {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("primaryTouch", |lua, _| {
            Ok(touches(lua)?.into_iter().next())
        });
        f.add_field_method_get("touches", |lua, _| lua.create_sequence_from(touches(lua)?));
    }
}

// ---- globals ------------------------------------------------------------------------------

/// Statics whose fields are computed (`Gamepad.current`).
fn computed(lua: &Lua, get: impl Fn(&Lua, String) -> Result<Value> + 'static) -> Result<Table> {
    let t = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Table, String)| get(lua, key))?,
    )?;
    t.set_metatable(Some(meta))?;
    Ok(t)
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let actions = lua.create_table()?;
    actions.set(
        "Load",
        lua.create_function(|lua, path: String| {
            with_world(lua, |w| w.input_system.load_shared(&path))?
                .map(LActions)
                .map_err(|e| err(format!("InputActions.Load: {e}")))
        })?,
    )?;
    actions.set(
        "Instantiate",
        lua.create_function(|lua, path: String| {
            with_world(lua, |w| w.input_system.instantiate(&path))?
                .map(LActions)
                .map_err(|e| err(format!("InputActions.Instantiate: {e}")))
        })?,
    )?;
    g.set("InputActions", actions)?;

    let gamepad = computed(lua, |lua, key| match key.as_str() {
        "current" => {
            let i = with_world(lua, |w| {
                w.input_system.last_pad.filter(|i| {
                    w.input
                        .joysticks
                        .get(*i)
                        .is_some_and(|j| !j.name.is_empty())
                })
            })?;
            Ok(match i {
                Some(i) => Value::UserData(lua.create_userdata(LGamepad(i))?),
                None => Value::Nil,
            })
        }
        "all" => {
            let pads: Vec<usize> = with_world(lua, |w| {
                w.input
                    .joysticks
                    .iter()
                    .enumerate()
                    .filter(|(_, j)| !j.name.is_empty())
                    .map(|(i, _)| i)
                    .collect()
            })?;
            Ok(Value::Table(
                lua.create_sequence_from(pads.into_iter().map(LGamepad))?,
            ))
        }
        _ => Ok(Value::Nil),
    })?;
    g.set("Gamepad", gamepad)?;
    use crate::input::actions::path::DeviceKind;
    g.set(
        "Keyboard",
        computed(lua, |lua, key| {
            Ok(if key == "current" {
                Value::UserData(lua.create_userdata(LDevice(DeviceKind::Keyboard))?)
            } else {
                Value::Nil
            })
        })?,
    )?;
    g.set(
        "Mouse",
        computed(lua, |lua, key| {
            Ok(if key == "current" {
                Value::UserData(lua.create_userdata(LDevice(DeviceKind::Mouse))?)
            } else {
                Value::Nil
            })
        })?,
    )?;
    g.set(
        "Touchscreen",
        computed(lua, |lua, key| {
            let seen = with_world(lua, |w| {
                w.input_system.touch_seen || !w.input.touches.is_empty()
            })?;
            Ok(if key == "current" && seen {
                Value::UserData(lua.create_userdata(LTouchscreen)?)
            } else {
                Value::Nil
            })
        })?,
    )?;
    let players = computed(lua, |lua, key| match key.as_str() {
        "all" => {
            let list: Vec<Entity> = with_world(lua, |w| {
                let World {
                    input_system,
                    scene,
                    input,
                    ..
                } = w;
                input_system.sync_players(scene, input);
                input_system.players.iter().map(|p| p.entity).collect()
            })?;
            Ok(Value::Table(lua.create_sequence_from(
                list.into_iter().map(LPlayerInput),
            )?))
        }
        "GetPlayerByIndex" => Ok(Value::Function(lua.create_function(|lua, i: usize| {
            with_world(lua, |w| {
                w.input_system
                    .players
                    .iter()
                    .find(|p| p.index == i)
                    .map(|p| LPlayerInput(p.entity))
            })
        })?)),
        _ => Ok(Value::Nil),
    })?;
    g.set("PlayerInput", players)?;
    Ok(())
}

// ---- dispatch -----------------------------------------------------------------------------

/// The callback context listeners and Invoke Events methods get.
fn context(lua: &Lua, set: usize, e: &ActionEvent) -> Result<Table> {
    let t = lua.create_table()?;
    t.set(
        "action",
        LAction {
            set,
            map: e.map,
            action: e.action,
        },
    )?;
    t.set("phase", e.phase.name())?;
    t.set("started", e.phase == Phase::Started)?;
    t.set("performed", e.phase == Phase::Performed)?;
    t.set("canceled", e.phase == Phase::Canceled)?;
    t.set("time", e.time)?;
    t.set("startTime", e.start_time)?;
    t.set("duration", e.time - e.start_time)?;
    t.set("interaction", e.interaction)?;
    t.set("control", e.control.clone())?;
    let value = value_to_lua(lua, e.value)?;
    t.set("value", value.clone())?;
    // `ctx:ReadValue()` / `ctx.ReadValue()`.
    t.set(
        "ReadValue",
        lua.create_function(move |_, _: mlua::MultiValue| Ok(value.clone()))?,
    )?;
    let button = e.value.magnitude() >= crate::input::actions::interactions::DEFAULT_PRESS_POINT;
    t.set(
        "ReadValueAsButton",
        lua.create_function(move |_, _: mlua::MultiValue| Ok(button))?,
    )?;
    Ok(t)
}

impl ScriptHost {
    /// Call `method` with `arg` on every running script of `target` that defines it (no
    /// warning when none does, like a message nobody needs to receive).
    fn message(&mut self, target: Entity, method: &str, arg: Option<Value>) {
        for i in 0..self.instances.len() {
            let inst: &Instance = &self.instances[i];
            if inst.entity != target || !self.runnable(inst) {
                continue;
            }
            let defined = self
                .lua
                .registry_value::<Table>(&inst.table)
                .and_then(|t| t.get::<Option<Function>>(method))
                .is_ok_and(|f| f.is_some());
            if defined {
                self.call(i, method, arg.clone());
                self.flush_spawned();
            }
        }
    }

    fn message_tree(&mut self, root: Entity, broadcast: bool, method: &str, arg: Option<Value>) {
        let targets: Vec<Entity> = if broadcast {
            self.with_world(|w| {
                w.scene
                    .walk()
                    .into_iter()
                    .map(|(e, _)| e)
                    .filter(|e| w.scene.is_ancestor(root, *e))
                    .collect()
            })
        } else {
            vec![root]
        };
        for t in targets {
            self.message(t, method, arg.clone());
        }
    }

    fn run_listeners(&mut self, key: &str, arg: Value, what: &str) {
        for f in listeners(&self.lua, key) {
            if let Err(e) = f.call::<()>(arg.clone()) {
                self.log(super::LogKind::Error, format!("{what} listener: {e}"));
            }
            self.flush_spawned();
        }
    }

    /// Deliver the Input System's events: action listeners, Player Input messages / event
    /// calls, device and scheme notifications, rebinding callbacks.
    pub(super) fn input_events(&mut self, events: Vec<SystemEvent>) {
        for ev in events {
            match ev {
                SystemEvent::Action { set, event } => self.action_event(set, &event),
                SystemEvent::ControlsChanged(e) => {
                    self.player_note(e, "onControlsChanged", "OnControlsChanged")
                }
                SystemEvent::DeviceLost(e) => self.player_note(e, "onDeviceLost", "OnDeviceLost"),
                SystemEvent::DeviceRegained(e) => {
                    self.player_note(e, "onDeviceRegained", "OnDeviceRegained")
                }
                SystemEvent::RebindComplete { id, path } => {
                    if let Err(e) = rebind_done(&self.lua, id, Some(path)) {
                        self.log(super::LogKind::Error, format!("OnComplete: {e}"));
                    }
                    self.flush_spawned();
                }
                SystemEvent::RebindCanceled { id } => {
                    if let Err(e) = rebind_done(&self.lua, id, None) {
                        self.log(super::LogKind::Error, format!("OnCancel: {e}"));
                    }
                    self.flush_spawned();
                }
            }
        }
    }

    fn player_note(&mut self, e: Entity, listener: &str, message: &str) {
        let behavior = self.with_world(|w| w.input_system.player(e).map(|p| p.behavior));
        let Ok(arg) = self
            .lua
            .create_userdata(LPlayerInput(e))
            .map(Value::UserData)
        else {
            return;
        };
        self.run_listeners(&player_key(e, listener), arg.clone(), listener);
        match behavior {
            Some(PlayerInputBehavior::SendMessages) => {
                self.message_tree(e, false, message, Some(arg))
            }
            Some(PlayerInputBehavior::BroadcastMessages) => {
                self.message_tree(e, true, message, Some(arg))
            }
            _ => {}
        }
    }

    fn action_event(&mut self, set: usize, e: &ActionEvent) {
        let phase = match e.phase {
            Phase::Started => "started",
            Phase::Performed => "performed",
            _ => "canceled",
        };
        let Ok(ctx) = context(&self.lua, set, e) else {
            return;
        };
        self.run_listeners(
            &action_key(set, e.map, e.action, phase),
            Value::Table(ctx.clone()),
            phase,
        );
        // Player Inputs playing this set.
        let (players, info) = self.with_world(|w| {
            let players: Vec<(Entity, PlayerInputBehavior)> = w
                .input_system
                .players
                .iter()
                .filter(|p| p.set == set)
                .map(|p| (p.entity, p.behavior))
                .collect();
            let info = w.input_system.set(set).map(|s| {
                let map = &s.asset.maps[e.map];
                let a = &map.actions[e.action];
                (format!("{}/{}", map.name, a.name), a.name.clone(), a.kind)
            });
            (players, info)
        });
        let Some((full, name, kind)) = info else {
            return;
        };
        for (entity, behavior) in players {
            self.run_listeners(
                &player_key(entity, "onActionTriggered"),
                Value::Table(ctx.clone()),
                "onActionTriggered",
            );
            match behavior {
                PlayerInputBehavior::SendMessages | PlayerInputBehavior::BroadcastMessages => {
                    // Performed, plus canceled for value actions (back to the default value).
                    let send = e.phase == Phase::Performed
                        || (e.phase == Phase::Canceled && kind != ActionType::Button);
                    if send {
                        let arg = value_to_lua(&self.lua, e.value).ok();
                        let broadcast = behavior == PlayerInputBehavior::BroadcastMessages;
                        self.message_tree(entity, broadcast, &message_name(&name), arg);
                    }
                }
                PlayerInputBehavior::InvokeEvents => {
                    let calls: Vec<(Option<Entity>, String)> = self.with_world(|w| {
                        w.scene
                            .world
                            .get::<&PlayerInput>(entity)
                            .map(|c| {
                                c.events
                                    .iter()
                                    .filter(|call| {
                                        call.action.eq_ignore_ascii_case(&full)
                                            || call.action.eq_ignore_ascii_case(&name)
                                    })
                                    .map(|call| (call.target, call.method.clone()))
                                    .collect()
                            })
                            .unwrap_or_default()
                    });
                    for (target, method) in calls {
                        if let Some(t) = target
                            && !method.is_empty()
                        {
                            self.message(t, &method, Some(Value::Table(ctx.clone())));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "input_actions_tests.rs"]
mod tests;
