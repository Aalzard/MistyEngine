//! Lua `VisualEffect`: `Play()`, `Stop()`, `Reinit()`, `SendEvent("OnPlay" | "OnStop")`,
//! `SetFloat/SetVector3/SetColor(name, value)`, `GetFloat(name)`, `HasProperty(name)`, and
//! `aliveCount` (read back from the GPU a frame or two late), `isPlaying`, `seed`.

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, LVec3, with_world};
use super::ui_api::LColor;
use crate::vfx::{self, EffectInstance, PropertyValue, VisualEffect};

#[derive(Clone, Copy)]
pub struct LVisualEffect(pub Entity);

/// `GetComponent("VisualEffect")` (or "Visual Effect").
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "VisualEffect" && name != "Visual Effect" {
        return Ok(None);
    }
    let has = with_world(lua, |w| w.scene.world.get::<&VisualEffect>(e).is_ok())?;
    if !has {
        return Ok(None);
    }
    lua.create_userdata(LVisualEffect(e))
        .map(|u| Some(Value::UserData(u)))
}

fn with<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&mut EffectInstance, &mut VisualEffect) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        vfx::with_instance(&mut w.scene, e, f)
            .ok_or_else(|| mlua::Error::runtime("object was destroyed"))
    })?
}

/// Set a property override, checking the graph has a property of that type.
fn set(lua: &Lua, e: Entity, name: &str, value: PropertyValue) -> Result<()> {
    with(lua, e, |i, c| {
        if let Some(effect) = &i.effect {
            match effect.properties.iter().find(|p| p.name == name) {
                Some(p) if p.value.same_type(&value) => {}
                Some(p) => {
                    return Err(mlua::Error::runtime(format!(
                        "property {name} is a {}, not a {}",
                        p.value.type_label(),
                        value.type_label()
                    )));
                }
                None => {
                    return Err(mlua::Error::runtime(format!(
                        "the effect has no property {name}"
                    )));
                }
            }
        }
        c.set_override(name, value);
        Ok(())
    })?
}

impl UserData for LVisualEffect {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("aliveCount", |lua, v| {
            with(lua, v.0, |i, _| vfx::stats(i.id).map_or(0, |s| s.alive))
        });
        f.add_field_method_get("isPlaying", |lua, v| with(lua, v.0, |i, _| i.playing));
        f.add_field_method_get("seed", |lua, v| with(lua, v.0, |i, _| i.seed));
        f.add_field_method_get("gameObject", |_, v| Ok(LGameObject(v.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Play", |lua, v, ()| with(lua, v.0, |i, c| i.play(c)));
        m.add_method("Stop", |lua, v, ()| with(lua, v.0, |i, _| i.stop()));
        m.add_method("Reinit", |lua, v, ()| with(lua, v.0, |i, c| i.reinit(c)));
        m.add_method("SendEvent", |lua, v, name: String| {
            with(lua, v.0, |i, c| match name.as_str() {
                "OnPlay" => {
                    i.play(c);
                    Ok(())
                }
                "OnStop" => {
                    i.stop();
                    Ok(())
                }
                other => Err(mlua::Error::runtime(format!(
                    "unknown event {other} (OnPlay, OnStop)"
                ))),
            })?
        });
        m.add_method("SetFloat", |lua, v, (name, x): (String, f32)| {
            set(lua, v.0, &name, PropertyValue::Float(x))
        });
        m.add_method("SetVector3", |lua, v, (name, x): (String, LVec3)| {
            set(lua, v.0, &name, PropertyValue::Vector3(x.0.to_array()))
        });
        m.add_method("SetColor", |lua, v, (name, x): (String, LColor)| {
            set(lua, v.0, &name, PropertyValue::Color(x.0))
        });
        m.add_method("GetFloat", |lua, v, name: String| {
            with(lua, v.0, |i, c| match c.override_value(&name) {
                Some(PropertyValue::Float(f)) => *f,
                _ => i
                    .effect
                    .as_ref()
                    .and_then(|e| e.properties.iter().find(|p| p.name == name))
                    .and_then(|p| match p.value {
                        PropertyValue::Float(f) => Some(f),
                        _ => None,
                    })
                    .unwrap_or(0.0),
            })
        });
        m.add_method("HasProperty", |lua, v, name: String| {
            with(lua, v.0, |i, _| {
                i.effect
                    .as_ref()
                    .is_some_and(|e| e.properties.iter().any(|p| p.name == name))
            })
        });
        m.add_method("ResetOverride", |lua, v, name: String| {
            with(lua, v.0, |_, c| c.clear_override(&name))
        });
    }
}
