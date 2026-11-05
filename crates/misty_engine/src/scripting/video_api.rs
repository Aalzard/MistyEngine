//! Lua `VideoPlayer` (`GetComponent("VideoPlayer")`): `Play()`, `Pause()`, `Stop()`,
//! `Prepare()`, `time` and `frame` (read / write, writing seeks), `length`, `frameCount`,
//! `frameRate`, `isPlaying`, `isPaused`, `isPrepared`, `isLooping` (read / write),
//! `playbackSpeed`, `playOnAwake`, `clip`, `url`, `texture` (the runtime texture name, usable as
//! a Raw Image texture), `width`, `height`, `gameObject`, and the events `started`,
//! `loopPointReached`, `prepareCompleted` and `errorReceived`
//! (`vp.loopPointReached:AddListener(function(vp) … end)`).

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, missing, with_world};
use super::ui_widgets_api::LUiEvent;
use crate::video::{VideoEvent, VideoPlayback, VideoPlayer, VideoSourceKind};

#[derive(Clone, Copy)]
pub struct LVideoPlayer(pub Entity);

fn alive(w: &World, e: Entity) -> Result<()> {
    w.scene
        .world
        .get::<&VideoPlayer>(e)
        .map(|_| ())
        .map_err(|_| missing())
}

/// Run `f` on the playback with the scene and audio (checking the component exists).
fn player<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&mut VideoPlayback, &mut crate::scene::Scene, &mut crate::audio::Audio) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        alive(w, e)?;
        let World {
            video,
            scene,
            audio,
            ..
        } = w;
        Ok(f(video, scene, audio))
    })?
}

/// Read or change the component's settings.
fn settings<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut VideoPlayer) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut VideoPlayer>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| missing())
    })?
}

/// Events from calls made by scripts (`Play()` can fail or start at once) wait for the frame's
/// dispatch.
fn queue(lua: &Lua, events: Vec<VideoEvent>) -> Result<()> {
    with_world(lua, |w| w.video_events.extend(events))
}

impl UserData for LVideoPlayer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("time", |lua, v| player(lua, v.0, |p, _, _| p.time(v.0)));
        f.add_field_method_set("time", |lua, v, t: f64| {
            player(lua, v.0, |p, _, a| p.set_time(a, v.0, t))
        });
        f.add_field_method_get("frame", |lua, v| player(lua, v.0, |p, _, _| p.frame(v.0)));
        f.add_field_method_set("frame", |lua, v, n: i64| {
            player(lua, v.0, |p, _, a| p.set_frame(a, v.0, n))
        });
        f.add_field_method_get("length", |lua, v| player(lua, v.0, |p, _, _| p.length(v.0)));
        f.add_field_method_get("frameCount", |lua, v| {
            player(lua, v.0, |p, _, _| p.frame_count(v.0))
        });
        f.add_field_method_get("frameRate", |lua, v| {
            player(lua, v.0, |p, _, _| p.frame_rate(v.0))
        });
        f.add_field_method_get("isPlaying", |lua, v| {
            player(lua, v.0, |p, _, _| p.is_playing(v.0))
        });
        f.add_field_method_get("isPaused", |lua, v| {
            player(lua, v.0, |p, _, _| p.is_paused(v.0))
        });
        f.add_field_method_get("isPrepared", |lua, v| {
            player(lua, v.0, |p, _, _| p.is_prepared(v.0))
        });
        f.add_field_method_get("isLooping", |lua, v| {
            player(lua, v.0, |p, s, _| p.is_looping(s, v.0))
        });
        f.add_field_method_set("isLooping", |lua, v, on: bool| {
            player(lua, v.0, |p, s, _| p.set_looping(s, v.0, on))
        });
        f.add_field_method_get("playbackSpeed", |lua, v| {
            settings(lua, v.0, |c| c.playback_speed)
        });
        f.add_field_method_set("playbackSpeed", |lua, v, s: f32| {
            settings(lua, v.0, |c| c.playback_speed = s.clamp(0.01, 10.0))
        });
        f.add_field_method_get("playOnAwake", |lua, v| {
            settings(lua, v.0, |c| c.play_on_awake)
        });
        f.add_field_method_set("playOnAwake", |lua, v, on: bool| {
            settings(lua, v.0, |c| c.play_on_awake = on)
        });
        f.add_field_method_get("clip", |lua, v| settings(lua, v.0, |c| c.clip.clone()));
        f.add_field_method_set("clip", |lua, v, clip: Option<String>| {
            settings(lua, v.0, |c| {
                c.clip = clip;
                c.source = VideoSourceKind::VideoClip;
            })
        });
        f.add_field_method_get("url", |lua, v| settings(lua, v.0, |c| c.url.clone()));
        f.add_field_method_set("url", |lua, v, url: String| {
            settings(lua, v.0, |c| {
                c.url = url;
                c.source = VideoSourceKind::Url;
            })
        });
        f.add_field_method_get("texture", |lua, v| {
            player(lua, v.0, |p, s, _| {
                let own = s
                    .world
                    .get::<&VideoPlayer>(v.0)
                    .ok()
                    .and_then(|c| c.output_texture());
                own.or_else(|| p.texture(v.0))
            })
        });
        f.add_field_method_get("width", |lua, v| {
            player(lua, v.0, |p, _, _| p.size(v.0).map_or(0, |s| s.0))
        });
        f.add_field_method_get("height", |lua, v| {
            player(lua, v.0, |p, _, _| p.size(v.0).map_or(0, |s| s.1))
        });
        f.add_field_method_get("gameObject", |_, v| Ok(LGameObject(v.0)));
        for event in [
            "started",
            "loopPointReached",
            "prepareCompleted",
            "errorReceived",
        ] {
            f.add_field_method_get(event, move |_, v| Ok(LUiEvent { entity: v.0, event }));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Play", |lua, v, ()| {
            let ev = player(lua, v.0, |p, s, a| p.play(s, a, v.0))?;
            queue(lua, ev)
        });
        m.add_method("Pause", |lua, v, ()| {
            player(lua, v.0, |p, _, a| p.pause(a, v.0))
        });
        m.add_method("Stop", |lua, v, ()| {
            player(lua, v.0, |p, s, a| p.stop(s, a, v.0))
        });
        m.add_method("Prepare", |lua, v, ()| {
            let r = player(lua, v.0, |p, s, _| p.prepare(s, v.0))?;
            match r {
                Ok(()) => Ok(()),
                Err(e) => queue(lua, vec![VideoEvent::Error(v.0, e)]),
            }
        });
    }
}

/// `GetComponent("VideoPlayer")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "VideoPlayer" && name != "Video Player" {
        return Ok(None);
    }
    let has = with_world(lua, |w| alive(w, e).is_ok())?;
    if !has {
        return Ok(None);
    }
    lua.create_userdata(LVideoPlayer(e))
        .map(|u| Some(Value::UserData(u)))
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
