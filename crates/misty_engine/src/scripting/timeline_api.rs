//! Lua `PlayableDirector` (`GetComponent("PlayableDirector")`): `Play()`, `Pause()`,
//! `Resume()`, `Stop()`, `Evaluate()`, `time` (read / write), `duration`, `state`
//! ("Playing", "Paused" or "Stopped"), `playOnAwake`, `gameObject` and the `stopped` event
//! (`director.stopped:AddListener(function(d) … end)`).

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, missing, with_world};
use super::ui_widgets_api::LUiEvent;
use crate::scene::PlayableDirector;

#[derive(Clone, Copy)]
pub struct LDirector(pub Entity);

fn alive(w: &World, e: Entity) -> Result<()> {
    w.scene
        .world
        .get::<&PlayableDirector>(e)
        .map(|_| ())
        .map_err(|_| missing())
}

/// Run `f` on the director player with the scene and audio (checking the component exists).
fn player<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(
        &mut crate::timeline::DirectorPlayer,
        &mut crate::scene::Scene,
        &mut crate::audio::Audio,
    ) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        alive(w, e)?;
        let World {
            timeline,
            scene,
            audio,
            ..
        } = w;
        Ok(f(timeline, scene, audio))
    })?
}

impl UserData for LDirector {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("time", |lua, d| player(lua, d.0, |p, _, _| p.time(d.0)));
        f.add_field_method_set("time", |lua, d, t: f64| {
            player(lua, d.0, |p, _, _| p.set_time(d.0, t))
        });
        f.add_field_method_get("duration", |lua, d| {
            player(lua, d.0, |p, s, _| p.duration(s, d.0))
        });
        f.add_field_method_get("state", |lua, d| {
            player(lua, d.0, |p, _, _| p.state(d.0).label())
        });
        f.add_field_method_get("playOnAwake", |lua, d| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&PlayableDirector>(d.0)
                    .map(|c| c.play_on_awake)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_set("playOnAwake", |lua, d, v: bool| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&mut PlayableDirector>(d.0)
                    .map(|mut c| c.play_on_awake = v)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_get("gameObject", |_, d| Ok(LGameObject(d.0)));
        f.add_field_method_get("stopped", |_, d| {
            Ok(LUiEvent {
                entity: d.0,
                event: "stopped",
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Play", |lua, d, ()| {
            player(lua, d.0, |p, s, _| p.play(s, d.0))
        });
        m.add_method("Pause", |lua, d, ()| {
            player(lua, d.0, |p, _, a| p.pause(a, d.0))
        });
        m.add_method("Resume", |lua, d, ()| {
            player(lua, d.0, |p, _, a| p.resume(a, d.0))
        });
        m.add_method("Stop", |lua, d, ()| {
            player(lua, d.0, |p, s, a| p.stop(s, a, d.0))
        });
        m.add_method("Evaluate", |lua, d, ()| {
            player(lua, d.0, |p, s, _| p.evaluate(s, d.0))
        });
    }
}

/// `GetComponent("PlayableDirector")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "PlayableDirector" {
        return Ok(None);
    }
    let has = with_world(lua, |w| alive(w, e).is_ok())?;
    if !has {
        return Ok(None);
    }
    lua.create_userdata(LDirector(e))
        .map(|u| Some(Value::UserData(u)))
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
