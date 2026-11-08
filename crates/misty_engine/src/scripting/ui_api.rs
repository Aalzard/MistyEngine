//! Lua API for UI components: `Color`, and the `Text`, `Image` (with its type and fill),
//! `Button`, `Canvas`, `CanvasGroup` and `AspectRatioFitter` components returned by
//! `GetComponent`.

use hecs::Entity;
use mlua::{FromLua, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, with_world};
use crate::scene::{
    AspectMode, AspectRatioFitter, Canvas, CanvasGroup, CanvasRenderMode, FillMethod, ImageType,
    SceneComponent, UiButton, UiImage, UiText,
};

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
pub(super) fn get<T: SceneComponent, R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&T) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&T>(e)
            .map(|c| f(&c))
            .map_err(|_| mlua::Error::runtime(format!("{} was destroyed", T::NAME)))
    })?
}

pub(super) fn set<T: SceneComponent>(lua: &Lua, e: Entity, f: impl FnOnce(&mut T)) -> Result<()> {
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
#[derive(Clone, Copy)]
pub struct LCanvasGroup(pub Entity);
#[derive(Clone, Copy)]
pub struct LAspectRatioFitter(pub Entity);
#[derive(Clone, Copy)]
pub struct LCanvas(pub Entity);

impl UserData for LCanvasGroup {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("alpha", |lua, g| get(lua, g.0, |c: &CanvasGroup| c.alpha));
        f.add_field_method_set("alpha", |lua, g, v: f32| {
            set(lua, g.0, |c: &mut CanvasGroup| c.alpha = v.clamp(0.0, 1.0))
        });
        f.add_field_method_get("interactable", |lua, g| {
            get(lua, g.0, |c: &CanvasGroup| c.interactable)
        });
        f.add_field_method_set("interactable", |lua, g, v: bool| {
            set(lua, g.0, |c: &mut CanvasGroup| c.interactable = v)
        });
        f.add_field_method_get("blocksRaycasts", |lua, g| {
            get(lua, g.0, |c: &CanvasGroup| c.blocks_raycasts)
        });
        f.add_field_method_set("blocksRaycasts", |lua, g, v: bool| {
            set(lua, g.0, |c: &mut CanvasGroup| c.blocks_raycasts = v)
        });
        f.add_field_method_get("ignoreParentGroups", |lua, g| {
            get(lua, g.0, |c: &CanvasGroup| c.ignore_parent_groups)
        });
        f.add_field_method_set("ignoreParentGroups", |lua, g, v: bool| {
            set(lua, g.0, |c: &mut CanvasGroup| c.ignore_parent_groups = v)
        });
        f.add_field_method_get("enabled", |lua, g| {
            get(lua, g.0, |c: &CanvasGroup| c.enabled)
        });
        f.add_field_method_set("enabled", |lua, g, v: bool| {
            set(lua, g.0, |c: &mut CanvasGroup| c.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, g| Ok(LGameObject(g.0)));
    }
}

impl UserData for LAspectRatioFitter {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("aspectRatio", |lua, a| {
            get(lua, a.0, |c: &AspectRatioFitter| c.aspect_ratio)
        });
        f.add_field_method_set("aspectRatio", |lua, a, v: f32| {
            set(lua, a.0, |c: &mut AspectRatioFitter| {
                c.aspect_ratio = v.clamp(0.001, 1000.0)
            })
        });
        f.add_field_method_get("aspectMode", |lua, a| {
            get(lua, a.0, |c: &AspectRatioFitter| {
                c.aspect_mode.label().replace(' ', "")
            })
        });
        f.add_field_method_set("aspectMode", |lua, a, v: String| {
            let m = AspectMode::ALL
                .into_iter()
                .find(|m| m.label().replace(' ', "").eq_ignore_ascii_case(&v))
                .ok_or_else(|| mlua::Error::runtime(format!("unknown aspect mode '{v}'")))?;
            set(lua, a.0, |c: &mut AspectRatioFitter| c.aspect_mode = m)
        });
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
    }
}

impl UserData for LCanvas {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("sortingOrder", |lua, c| {
            get(lua, c.0, |c: &Canvas| c.sort_order)
        });
        f.add_field_method_set("sortingOrder", |lua, c, v: i32| {
            set(lua, c.0, |c: &mut Canvas| c.sort_order = v)
        });
        f.add_field_method_get("planeDistance", |lua, c| {
            get(lua, c.0, |c: &Canvas| c.plane_distance)
        });
        f.add_field_method_set("planeDistance", |lua, c, v: f32| {
            set(lua, c.0, |c: &mut Canvas| c.plane_distance = v.max(0.01))
        });
        f.add_field_method_get("renderMode", |lua, c| {
            get(lua, c.0, |c: &Canvas| match c.render_mode {
                CanvasRenderMode::ScreenSpaceOverlay => "ScreenSpaceOverlay",
                CanvasRenderMode::ScreenSpaceCamera => "ScreenSpaceCamera",
                CanvasRenderMode::WorldSpace => "WorldSpace",
            })
        });
        f.add_field_method_set("renderMode", |lua, c, v: String| {
            let m = match v.as_str() {
                "ScreenSpaceOverlay" => CanvasRenderMode::ScreenSpaceOverlay,
                "ScreenSpaceCamera" => CanvasRenderMode::ScreenSpaceCamera,
                "WorldSpace" => CanvasRenderMode::WorldSpace,
                _ => return Err(mlua::Error::runtime(format!("unknown render mode '{v}'"))),
            };
            set(lua, c.0, |c: &mut Canvas| c.render_mode = m)
        });
        f.add_field_method_get("enabled", |lua, c| get(lua, c.0, |c: &Canvas| c.enabled));
        f.add_field_method_set("enabled", |lua, c, v: bool| {
            set(lua, c.0, |c: &mut Canvas| c.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
    }
}

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
        f.add_field_method_get("richText", |lua, t| get(lua, t.0, |c: &UiText| c.rich_text));
        f.add_field_method_set("richText", |lua, t, v: bool| {
            set(lua, t.0, |c: &mut UiText| c.rich_text = v)
        });
        f.add_field_method_get("page", |lua, t| get(lua, t.0, |c: &UiText| c.page));
        f.add_field_method_set("page", |lua, t, v: u32| {
            set(lua, t.0, |c: &mut UiText| c.page = v.max(1))
        });
        // `text.onLinkClicked:AddListener(function(id) … end)`.
        f.add_field_method_get("onLinkClicked", |_, t| {
            Ok(super::ui_widgets_api::LUiEvent {
                entity: t.0,
                event: "onLinkClicked",
            })
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
        f.add_field_method_get("type", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.image_type.label())
        });
        f.add_field_method_set("type", |lua, i, v: String| {
            let t = ImageType::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(&v))
                .ok_or_else(|| mlua::Error::runtime(format!("unknown Image type '{v}'")))?;
            set(lua, i.0, |c: &mut UiImage| c.image_type = t)
        });
        f.add_field_method_get("fillAmount", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.fill.amount)
        });
        f.add_field_method_set("fillAmount", |lua, i, v: f32| {
            set(lua, i.0, |c: &mut UiImage| {
                c.fill.amount = v.clamp(0.0, 1.0)
            })
        });
        f.add_field_method_get("fillMethod", |lua, i| {
            get(lua, i.0, |c: &UiImage| {
                c.fill.method.label().replace(' ', "")
            })
        });
        f.add_field_method_set("fillMethod", |lua, i, v: String| {
            let m = FillMethod::ALL
                .into_iter()
                .find(|m| {
                    m.label()
                        .replace(' ', "")
                        .eq_ignore_ascii_case(&v.replace(' ', ""))
                })
                .ok_or_else(|| mlua::Error::runtime(format!("unknown fill method '{v}'")))?;
            set(lua, i.0, |c: &mut UiImage| {
                if c.fill.method != m {
                    c.fill.method = m;
                    c.fill.origin = 0;
                }
            })
        });
        f.add_field_method_get("fillOrigin", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.fill.origin)
        });
        f.add_field_method_set("fillOrigin", |lua, i, v: u32| {
            set(lua, i.0, |c: &mut UiImage| {
                c.fill.origin = v.min(c.fill.method.origins().len() as u32 - 1)
            })
        });
        f.add_field_method_get("fillClockwise", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.fill.clockwise)
        });
        f.add_field_method_set("fillClockwise", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiImage| c.fill.clockwise = v)
        });
        f.add_field_method_get("fillCenter", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.fill_center)
        });
        f.add_field_method_set("fillCenter", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiImage| c.fill_center = v)
        });
        f.add_field_method_get("preserveAspect", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.preserve_aspect)
        });
        f.add_field_method_set("preserveAspect", |lua, i, v: bool| {
            set(lua, i.0, |c: &mut UiImage| c.preserve_aspect = v)
        });
        f.add_field_method_get("sprite", |lua, i| {
            get(lua, i.0, |c: &UiImage| c.sprite.clone())
        });
        f.add_field_method_set("sprite", |lua, i, v: Option<String>| {
            set(lua, i.0, |c: &mut UiImage| {
                c.sprite = v.filter(|p| !p.is_empty())
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
        f.add_field_method_get("onClick", |_, b| Ok(super::ui_widgets_api::on_click(b.0)));
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
        "CanvasGroup" => has(lua, |w, e| w.get::<&CanvasGroup>(e).is_ok())?,
        "AspectRatioFitter" => has(lua, |w, e| w.get::<&AspectRatioFitter>(e).is_ok())?,
        "Canvas" => has(lua, |w, e| w.get::<&Canvas>(e).is_ok())?,
        _ => false,
    };
    if !present {
        return super::ui_widgets_api::component(lua, e, name);
    }
    let ud = match name {
        "Text" => lua.create_userdata(LText(e))?,
        "Image" => lua.create_userdata(LImage(e))?,
        "CanvasGroup" => lua.create_userdata(LCanvasGroup(e))?,
        "AspectRatioFitter" => lua.create_userdata(LAspectRatioFitter(e))?,
        "Canvas" => lua.create_userdata(LCanvas(e))?,
        _ => lua.create_userdata(LButton(e))?,
    };
    Ok(Some(Value::UserData(ud)))
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
