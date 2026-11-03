//! Lua API for UI components: `Color`, and the `Text`, `Image` and `Button` components returned
//! by `GetComponent`.

use hecs::Entity;
use mlua::{FromLua, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, with_world};
use crate::scene::{SceneComponent, UiButton, UiImage, UiText};

/// RGBA colour, components 0..1 (sRGB, like the Inspector colour fields).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LColor(pub [f32; 4]);

impl FromLua for LColor {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => Ok(*ud.borrow::<LColor>()?),
            Value::Table(t) => Ok(LColor([
                t.get("r")?,
                t.get("g")?,
                t.get("b")?,
                t.get::<Option<f32>>("a")?.unwrap_or(1.0),
            ])),
            other => Err(mlua::Error::runtime(format!(
                "expected Color, got {}",
                other.type_name()
            ))),
        }
    }
}

impl UserData for LColor {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        for (i, name) in ["r", "g", "b", "a"].into_iter().enumerate() {
            f.add_field_method_get(name, move |_, c| Ok(c.0[i]));
            f.add_field_method_set(name, move |_, c, v: f32| {
                c.0[i] = v;
                Ok(())
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Eq, |_, a, b: LColor| Ok(a.0 == b.0));
        m.add_meta_method(MetaMethod::Mul, |_, a, b: Value| match b {
            Value::Integer(n) => Ok(LColor(a.0.map(|v| v * n as f32))),
            Value::Number(n) => Ok(LColor(a.0.map(|v| v * n as f32))),
            Value::UserData(ud) => {
                let b = ud.borrow::<LColor>()?.0;
                Ok(LColor(std::array::from_fn(|i| a.0[i] * b[i])))
            }
            _ => Err(mlua::Error::runtime("Color * expects a number or Color")),
        });
        m.add_meta_method(MetaMethod::ToString, |_, c, ()| {
            let [r, g, b, a] = c.0;
            Ok(format!("RGBA({r:.3}, {g:.3}, {b:.3}, {a:.3})"))
        });
    }
}

/// Register the `Color` table.
pub fn register(lua: &Lua) -> Result<()> {
    let color = lua.create_table()?;
    color.set(
        "new",
        lua.create_function(|_, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            Ok(LColor([r, g, b, a.unwrap_or(1.0)]))
        })?,
    )?;
    for (name, c) in [
        ("white", [1.0, 1.0, 1.0, 1.0]),
        ("black", [0.0, 0.0, 0.0, 1.0]),
        ("red", [1.0, 0.0, 0.0, 1.0]),
        ("green", [0.0, 1.0, 0.0, 1.0]),
        ("blue", [0.0, 0.0, 1.0, 1.0]),
        ("yellow", [1.0, 0.92, 0.016, 1.0]),
        ("cyan", [0.0, 1.0, 1.0, 1.0]),
        ("magenta", [1.0, 0.0, 1.0, 1.0]),
        ("gray", [0.5, 0.5, 0.5, 1.0]),
        ("clear", [0.0, 0.0, 0.0, 0.0]),
    ] {
        color.set(name, LColor(c))?;
    }
    color.set(
        "Lerp",
        lua.create_function(|_, (a, b, t): (LColor, LColor, f32)| {
            let t = t.clamp(0.0, 1.0);
            Ok(LColor(std::array::from_fn(|i| {
                a.0[i] + (b.0[i] - a.0[i]) * t
            })))
        })?,
    )?;
    lua.globals().set("Color", color)
}

/// Read a component of `e` (error if it was removed or the object destroyed).
fn get<T: SceneComponent, R>(lua: &Lua, e: Entity, f: impl FnOnce(&T) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&T>(e)
            .map(|c| f(&c))
            .map_err(|_| mlua::Error::runtime(format!("{} was destroyed", T::NAME)))
    })?
}

fn set<T: SceneComponent>(lua: &Lua, e: Entity, f: impl FnOnce(&mut T)) -> Result<()> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut T>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| mlua::Error::runtime(format!("{} was destroyed", T::NAME)))
    })?
}

#[derive(Clone, Copy)]
pub struct LText(pub Entity);
#[derive(Clone, Copy)]
pub struct LImage(pub Entity);
#[derive(Clone, Copy)]
pub struct LButton(pub Entity);

impl UserData for LText {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("text", |lua, t| get(lua, t.0, |c: &UiText| c.text.clone()));
        f.add_field_method_set("text", |lua, t, v: Value| {
            // Numbers are shown as text, like string concatenation would.
            let s = match v {
                Value::String(s) => s.to_string_lossy(),
                Value::Nil => String::new(),
                other => other.to_string()?,
            };
            set(lua, t.0, |c: &mut UiText| c.text = s)
        });
        f.add_field_method_get("fontSize", |lua, t| get(lua, t.0, |c: &UiText| c.font_size));
        f.add_field_method_set("fontSize", |lua, t, v: f32| {
            set(lua, t.0, |c: &mut UiText| c.font_size = v.max(1.0))
        });
        f.add_field_method_get("color", |lua, t| {
            get(lua, t.0, |c: &UiText| LColor(c.color))
        });
        f.add_field_method_set("color", |lua, t, v: LColor| {
            set(lua, t.0, |c: &mut UiText| c.color = v.0)
        });
        f.add_field_method_get("enabled", |lua, t| get(lua, t.0, |c: &UiText| c.enabled));
        f.add_field_method_set("enabled", |lua, t, v: bool| {
            set(lua, t.0, |c: &mut UiText| c.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, t| Ok(LGameObject(t.0)));
    }
}

impl UserData for LImage {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("color", |lua, i| {
            get(lua, i.0, |c: &UiImage| LColor(c.color))
        });
        f.add_field_method_set("color", |lua, i, v: LColor| {
            set(lua, i.0, |c: &mut UiImage| c.color = v.0)
        });
        f.add_field_method_get("enabled", |lua, i| get(lua, i.0, |c: &UiImage| c.enabled));
        f.add_field_method_set("enabled", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiImage| c.enabled = v)
        });
        f.add_field_method_get("raycastTarget", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.raycast_target)
        });
        f.add_field_method_set("raycastTarget", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiImage| c.raycast_target = v)
        });
        f.add_field_method_get("texture", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.texture.clone())
        });
        f.add_field_method_set("texture", |lua, i, v: Option<String>| {
            set(lua, i.0, |c: &mut UiImage| {
                c.texture = v.filter(|p| !p.is_empty())
            })
        });
        f.add_field_method_get("gameObject", |_, i| Ok(LGameObject(i.0)));
    }
}

impl UserData for LButton {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("interactable", |lua, b| {
            get(lua, b.0, |c: &UiButton| c.interactable)
        });
        f.add_field_method_set("interactable", |lua, b, v: bool| {
            set(lua, b.0, |c: &mut UiButton| c.interactable = v)
        });
        f.add_field_method_get("gameObject", |_, b| Ok(LGameObject(b.0)));
    }
}

/// `GetComponent` for UI component names; `Ok(None)` = not a UI component (or not present).
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has =
        |lua: &Lua, f: fn(&hecs::World, Entity) -> bool| with_world(lua, |w| f(&w.scene.world, e));
    let present = match name {
        "Text" => has(lua, |w, e| w.get::<&UiText>(e).is_ok())?,
        "Image" => has(lua, |w, e| w.get::<&UiImage>(e).is_ok())?,
        "Button" => has(lua, |w, e| w.get::<&UiButton>(e).is_ok())?,
        _ => false,
    };
    if !present {
        return Ok(None);
    }
    let ud = match name {
        "Text" => lua.create_userdata(LText(e))?,
        "Image" => lua.create_userdata(LImage(e))?,
        _ => lua.create_userdata(LButton(e))?,
    };
    Ok(Some(Value::UserData(ud)))
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
