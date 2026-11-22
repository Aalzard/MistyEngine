//! Lua `EventSystem.current`: the UI selection that keyboard and gamepad navigation move
//! (`currentSelectedGameObject`, `SetSelectedGameObject`), and the scene's Event System
//! settings (`firstSelectedGameObject`, `sendNavigationEvents`). Scripts on the selected object
//! hear `OnSelect()` / `OnDeselect()` when the selection moves.
//!
//! A selection set by a script takes effect at the start of the next UI pass (before the next
//! frame's Update), but `currentSelectedGameObject` returns it at once.

use hecs::Entity;
use mlua::{Lua, ObjectLike, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, with_world};
use crate::scene::EventSystem;

/// `EventSystem.current`.
#[derive(Clone, Copy)]
pub struct LEventSystem;

/// The selection scripts see: the one asked for this frame, else the UI's.
pub fn current_selected(w: &World) -> Option<Entity> {
    let e = match w.ui_select_request {
        Some(req) => req,
        None => w.ui_selected,
    };
    e.filter(|e| w.scene.contains(*e))
}

/// Ask for `e` to be selected (`None` clears the selection).
pub fn request_select(w: &mut World, e: Option<Entity>) {
    w.ui_select_request = Some(e.filter(|e| w.scene.contains(*e)));
}

/// The object a selection argument names: a GameObject, a component (its `gameObject`), or nil.
pub fn object_arg(v: &Value) -> Result<Option<Entity>> {
    match v {
        Value::Nil => Ok(None),
        Value::UserData(ud) if ud.is::<LGameObject>() => Ok(Some(ud.borrow::<LGameObject>()?.0)),
        Value::UserData(ud) => match ud.get::<Value>("gameObject") {
            Ok(Value::UserData(go)) if go.is::<LGameObject>() => {
                Ok(Some(go.borrow::<LGameObject>()?.0))
            }
            _ => Err(mlua::Error::runtime(
                "SetSelectedGameObject: expected a GameObject, a component or nil",
            )),
        },
        Value::Table(t) => match t.get::<Value>("gameObject")? {
            Value::UserData(go) if go.is::<LGameObject>() => {
                Ok(Some(go.borrow::<LGameObject>()?.0))
            }
            _ => Err(mlua::Error::runtime(
                "SetSelectedGameObject: expected a GameObject, a component or nil",
            )),
        },
        other => Err(mlua::Error::runtime(format!(
            "SetSelectedGameObject: expected a GameObject or nil, got {}",
            other.type_name()
        ))),
    }
}

/// The object holding the scene's running Event System (the first enabled, active one).
fn event_system_object(w: &World) -> Option<Entity> {
    if !w.scene.has_any::<EventSystem>() {
        return None;
    }
    w.scene.walk().into_iter().map(|(e, _)| e).find(|e| {
        w.scene.active_in_hierarchy(*e)
            && w.scene
                .world
                .get::<&EventSystem>(*e)
                .is_ok_and(|es| es.enabled)
    })
}

fn with_event_system<R>(w: &mut World, f: impl FnOnce(&mut EventSystem) -> R) -> Option<R> {
    let e = event_system_object(w)?;
    let mut es = w.scene.world.get::<&mut EventSystem>(e).ok()?;
    Some(f(&mut es))
}

impl UserData for LEventSystem {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("currentSelectedGameObject", |lua, _| {
            with_world(lua, |w| current_selected(w).map(LGameObject))
        });
        f.add_field_method_get("firstSelectedGameObject", |lua, _| {
            with_world(lua, |w| {
                crate::ui::events::settings(&w.scene)
                    .first_selected
                    .filter(|e| w.scene.contains(*e))
                    .map(LGameObject)
            })
        });
        f.add_field_method_set("firstSelectedGameObject", |lua, _, v: Value| {
            let e = object_arg(&v)?;
            with_world(lua, |w| {
                with_event_system(w, |es| es.first_selected = e);
            })
        });
        f.add_field_method_get("sendNavigationEvents", |lua, _| {
            with_world(lua, |w| {
                crate::ui::events::settings(&w.scene).send_navigation_events
            })
        });
        f.add_field_method_set("sendNavigationEvents", |lua, _, on: bool| {
            with_world(lua, |w| {
                with_event_system(w, |es| es.send_navigation_events = on);
            })
        });
        f.add_field_method_get("gameObject", |lua, _| {
            with_world(lua, |w| event_system_object(w).map(LGameObject))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetSelectedGameObject", |lua, _, v: Value| {
            let e = object_arg(&v)?;
            with_world(lua, |w| request_select(w, e))
        });
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    let t = lua.create_table()?;
    t.set("current", LEventSystem)?;
    lua.globals().set("EventSystem", t)
}

#[cfg(test)]
#[path = "event_system_tests.rs"]
mod tests;
