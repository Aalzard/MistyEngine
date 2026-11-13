//! Lua audio API: the AudioSource component (with `spatialize` and custom rolloff curves),
//! `AudioSource.PlayClipAtPoint`, and Audio Mixers (`AudioMixer.Find(name)` → `SetFloat` /
//! `GetFloat` / `ClearFloat` on exposed parameters — volumes, pitches, any effect parameter or
//! wet level —, `FindSnapshot(name)` → `TransitionTo(time)`, `TransitionToSnapshots(snapshots,
//! weights, time)`, `FindMatchingGroups(path)`, `outputAudioMixerGroup` for nested mixers).
//! Mixers are found by file name among the project's `.mixer` assets, so no Resources folder is
//! needed.

use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, LVec3, missing, with_world};
use crate::audio::{output_ref, split_output};
use crate::scene::{AudioRolloff, AudioSource};

// ---- AudioSource ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LAudioSource(pub Entity);

/// Read a field of the object's Audio Source.
fn source_get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&AudioSource) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&AudioSource>(e)
            .map(|s| f(&s))
            .map_err(|_| missing())
    })?
}

/// Change the object's Audio Source (a playing voice picks it up at the end of the frame).
fn source_set(lua: &Lua, e: Entity, f: impl FnOnce(&mut AudioSource)) -> Result<()> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut AudioSource>(e)
            .map(|mut s| f(&mut s))
            .map_err(|_| missing())
    })?
}

/// Number fields: (Lua name, getter, setter with clamping).
type NumField = (
    &'static str,
    fn(&AudioSource) -> f32,
    fn(&mut AudioSource, f32),
);
const NUMBERS: [NumField; 9] = [
    ("volume", |s| s.volume, |s, v| s.volume = v.clamp(0.0, 1.0)),
    ("pitch", |s| s.pitch, |s, v| s.pitch = v.clamp(-3.0, 3.0)),
    (
        "panStereo",
        |s| s.stereo_pan,
        |s, v| s.stereo_pan = v.clamp(-1.0, 1.0),
    ),
    (
        "spatialBlend",
        |s| s.spatial_blend,
        |s, v| s.spatial_blend = v.clamp(0.0, 1.0),
    ),
    (
        "dopplerLevel",
        |s| s.doppler_level,
        |s, v| s.doppler_level = v.clamp(0.0, 5.0),
    ),
    (
        "spread",
        |s| s.spread,
        |s, v| s.spread = v.clamp(0.0, 360.0),
    ),
    (
        "reverbZoneMix",
        |s| s.reverb_zone_mix,
        |s, v| s.reverb_zone_mix = v.clamp(0.0, 1.1),
    ),
    (
        "minDistance",
        |s| s.min_distance,
        |s, v| s.min_distance = v.max(0.0),
    ),
    (
        "maxDistance",
        |s| s.max_distance,
        |s, v| s.max_distance = v.max(0.01),
    ),
];

type BoolField = (
    &'static str,
    fn(&AudioSource) -> bool,
    fn(&mut AudioSource, bool),
);
const FLAGS: [BoolField; 8] = [
    ("loop", |s| s.looping, |s, v| s.looping = v),
    ("spatialize", |s| s.spatialize, |s, v| s.spatialize = v),
    ("mute", |s| s.mute, |s, v| s.mute = v),
    ("enabled", |s| s.enabled, |s, v| s.enabled = v),
    (
        "playOnAwake",
        |s| s.play_on_awake,
        |s, v| s.play_on_awake = v,
    ),
    (
        "bypassEffects",
        |s| s.bypass_effects,
        |s, v| s.bypass_effects = v,
    ),
    (
        "bypassListenerEffects",
        |s| s.bypass_listener_effects,
        |s, v| s.bypass_listener_effects = v,
    ),
    (
        "bypassReverbZones",
        |s| s.bypass_reverb_zones,
        |s, v| s.bypass_reverb_zones = v,
    ),
];

impl UserData for LAudioSource {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("isPlaying", |lua, a| {
            with_world(lua, |w| w.audio.is_playing(a.0))
        });
        f.add_field_method_get("time", |lua, a| with_world(lua, |w| w.audio.time(a.0)));
        f.add_field_method_set("time", |lua, a, t: f32| {
            with_world(lua, |w| w.audio.set_time(a.0, t))
        });
        for (name, get, set) in NUMBERS {
            f.add_field_method_get(name, move |lua, a| source_get(lua, a.0, get));
            f.add_field_method_set(name, move |lua, a, v: f32| {
                // NaN passes `clamp`: one NaN voice turns the whole mix to noise or silence.
                if v.is_nan() {
                    return Err(mlua::Error::runtime(format!(
                        "AudioSource.{name} must be a number, not NaN"
                    )));
                }
                source_set(lua, a.0, |s| set(s, v))
            });
        }
        for (name, get, set) in FLAGS {
            f.add_field_method_get(name, move |lua, a| source_get(lua, a.0, get));
            f.add_field_method_set(name, move |lua, a, v: bool| {
                source_set(lua, a.0, |s| set(s, v))
            });
        }
        f.add_field_method_get("priority", |lua, a| source_get(lua, a.0, |s| s.priority));
        f.add_field_method_set("priority", |lua, a, v: i64| {
            source_set(lua, a.0, |s| s.priority = v.clamp(0, 256) as u16)
        });
        f.add_field_method_get("rolloffMode", |lua, a| {
            source_get(lua, a.0, |s| format!("{:?}", s.rolloff))
        });
        f.add_field_method_set("rolloffMode", |lua, a, v: String| {
            let mode = match v.as_str() {
                "Linear" => AudioRolloff::Linear,
                "Logarithmic" => AudioRolloff::Logarithmic,
                "Custom" => AudioRolloff::Custom,
                other => {
                    return Err(mlua::Error::runtime(format!("unknown rolloffMode {other}")));
                }
            };
            source_set(lua, a.0, |s| s.rolloff = mode)
        });
        f.add_field_method_get("clip", |lua, a| source_get(lua, a.0, |s| s.clip.clone()));
        f.add_field_method_set("clip", |lua, a, v: Option<String>| {
            source_set(lua, a.0, |s| s.clip = v.filter(|c| !c.is_empty()))
        });
        f.add_field_method_get("outputAudioMixerGroup", |lua, a| {
            source_get(lua, a.0, |s| s.output.clone().map(LMixerGroup))
        });
        f.add_field_method_set("outputAudioMixerGroup", |lua, a, v: Value| {
            let output = match v {
                Value::Nil => None,
                Value::UserData(ud) => Some(ud.borrow::<LMixerGroup>()?.0.clone()),
                Value::String(name) => {
                    let name = name.to_str()?.to_owned();
                    let found = with_world(lua, |w| {
                        let prefer = w
                            .scene
                            .world
                            .get::<&AudioSource>(a.0)
                            .ok()
                            .and_then(|s| s.output.clone());
                        let prefer = prefer.as_deref().map(|o| split_output(o).0);
                        w.audio.find_group(&name, prefer)
                    })?;
                    Some(found.ok_or_else(|| {
                        mlua::Error::runtime(format!("no Audio Mixer group called {name}"))
                    })?)
                }
                _ => return Err(mlua::Error::runtime("expected a group name or group")),
            };
            source_set(lua, a.0, |s| s.output = output)
        });
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Play", |lua, a, ()| {
            with_world(lua, |w| {
                let World { audio, scene, .. } = w;
                audio.play(scene, a.0);
            })
        });
        m.add_method("Stop", |lua, a, ()| with_world(lua, |w| w.audio.stop(a.0)));
        m.add_method("Pause", |lua, a, ()| {
            with_world(lua, |w| w.audio.pause(a.0))
        });
        m.add_method("UnPause", |lua, a, ()| {
            with_world(lua, |w| w.audio.unpause(a.0))
        });
        // `SetCustomCurve("CustomRolloff", {{0, 1}, {0.5, 0.2}, {1, 0}})`: (distance as a share
        // of Max Distance, volume) keys; also switches the source to Custom Rolloff.
        m.add_method("SetCustomCurve", |lua, a, (kind, keys): (String, Table)| {
            if kind != "CustomRolloff" {
                return Err(mlua::Error::runtime(format!("unknown curve {kind}")));
            }
            let mut curve = crate::audio::spatial::RolloffCurve { keys: Vec::new() };
            for k in keys.sequence_values::<Table>() {
                let k = k?;
                curve.keys.push([k.get(1)?, k.get(2)?]);
            }
            curve.sanitize();
            source_set(lua, a.0, |s| {
                s.custom_rolloff = curve;
                s.rolloff = AudioRolloff::Custom;
            })
        });
        m.add_method("GetCustomCurve", |lua, a, kind: String| {
            if kind != "CustomRolloff" {
                return Err(mlua::Error::runtime(format!("unknown curve {kind}")));
            }
            let keys = source_get(lua, a.0, |s| s.custom_rolloff.keys.clone())?;
            let t = lua.create_table()?;
            for (i, k) in keys.into_iter().enumerate() {
                t.set(i + 1, lua.create_sequence_from(k)?)?;
            }
            Ok(t)
        });
        m.add_method(
            "PlayOneShot",
            |lua, a, (clip, volume): (String, Option<f32>)| {
                with_world(lua, |w| {
                    let World { audio, scene, .. } = w;
                    audio.play_one_shot(scene, a.0, &clip, volume.unwrap_or(1.0));
                })
            },
        );
    }
}

// ---- mixers -----------------------------------------------------------------------------------

/// A loaded mixer (index into the game's mixers, which are never unloaded).
#[derive(Clone, Copy)]
pub struct LMixer(usize);

/// A snapshot of a mixer.
#[derive(Clone, Copy)]
pub struct LSnapshot(usize, usize);

/// A mixer group, as the Output reference an Audio Source stores.
#[derive(Clone)]
pub struct LMixerGroup(pub String);

fn mixer<R>(lua: &Lua, i: usize, f: impl FnOnce(&mut crate::audio::MixerState) -> R) -> Result<R> {
    with_world(lua, |w| w.audio.mixer_mut(i).map(f))?
        .ok_or_else(|| mlua::Error::runtime("mixer is not loaded"))
}

impl UserData for LMixer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, m| mixer(lua, m.0, |s| s.name()));
        // Nested mixers: the group of another mixer this one's Master plays into.
        f.add_field_method_get("outputAudioMixerGroup", |lua, m| {
            mixer(lua, m.0, |s| s.asset.output.clone().map(LMixerGroup))
        });
        f.add_field_method_set(
            "outputAudioMixerGroup",
            |lua, m, g: Option<mlua::UserDataRef<LMixerGroup>>| {
                let output = g.map(|g| g.0.clone());
                with_world(lua, |w| {
                    let Some(s) = w.audio.mixer(m.0) else {
                        return;
                    };
                    let (path, mut asset) = (s.path.clone(), s.asset.clone());
                    asset.output = output;
                    w.audio.replace_mixer(&path, asset);
                })
            },
        );
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetFloat", |lua, mx, (name, v): (String, f32)| {
            mixer(lua, mx.0, |s| s.set_float(&name, v))
        });
        m.add_method("GetFloat", |lua, mx, name: String| {
            mixer(lua, mx.0, |s| s.get_float(&name))
        });
        m.add_method("ClearFloat", |lua, mx, name: String| {
            mixer(lua, mx.0, |s| s.clear_float(&name))
        });
        m.add_method("FindSnapshot", |lua, mx, name: String| {
            let i = mixer(lua, mx.0, |s| s.asset.snapshot_index(&name))?;
            Ok(i.map(|i| LSnapshot(mx.0, i)))
        });
        // `TransitionToSnapshots({a, b}, {wa, wb}, time)`: move to a weighted blend.
        m.add_method(
            "TransitionToSnapshots",
            |lua, mx, (snaps, weights, time): (Table, Table, f32)| {
                let mut blend = Vec::new();
                for (i, s) in snaps
                    .sequence_values::<mlua::UserDataRef<LSnapshot>>()
                    .enumerate()
                {
                    let s = s?;
                    if s.0 != mx.0 {
                        return Err(mlua::Error::runtime("snapshot of another mixer"));
                    }
                    let w: f32 = weights.get(i + 1).unwrap_or(0.0);
                    blend.push((s.1, w));
                }
                mixer(lua, mx.0, |s| s.transition_to_weighted(&blend, time))
            },
        );
        // Groups whose path ("Master/SFX/Steps") ends with `path`, like the reference API.
        m.add_method("FindMatchingGroups", |lua, mx, path: String| {
            let found = mixer(lua, mx.0, |s| {
                let a = &s.asset;
                a.tree()
                    .into_iter()
                    .filter(|(g, _)| {
                        let full = a.path(*g);
                        full == path || full.ends_with(&format!("/{path}")) || path.is_empty()
                    })
                    .filter_map(|(g, _)| a.group(g).map(|g| output_ref(&s.path, &g.name)))
                    .collect::<Vec<_>>()
            })?;
            let t = lua.create_table()?;
            for (i, o) in found.into_iter().enumerate() {
                t.set(i + 1, LMixerGroup(o))?;
            }
            Ok(t)
        });
    }
}

impl UserData for LSnapshot {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, s| {
            // Nil once the snapshot is gone (deleted in the Audio Mixer window during Play).
            mixer(lua, s.0, |m| {
                m.asset.snapshots.get(s.1).map(|x| x.name.clone())
            })
        });
        f.add_field_method_get("audioMixer", |_, s| Ok(LMixer(s.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("TransitionTo", |lua, s, time: f32| {
            mixer(lua, s.0, |m| m.transition_to(s.1, time))
        });
    }
}

impl UserData for LMixerGroup {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |_, g| Ok(split_output(&g.0).1.to_owned()));
        f.add_field_method_get("audioMixer", |lua, g| {
            let path = split_output(&g.0).0.to_owned();
            with_world(lua, |w| w.audio.load_mixer(&path).map(LMixer))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(
            mlua::MetaMethod::Eq,
            |_, a, b: mlua::UserDataRef<LMixerGroup>| Ok(a.0 == b.0),
        );
        m.add_meta_method(mlua::MetaMethod::ToString, |_, g, ()| Ok(g.0.clone()));
    }
}

// ---- globals ----------------------------------------------------------------------------------

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let source: Table = lua.create_table()?;
    source.set(
        "PlayClipAtPoint",
        lua.create_function(
            |lua, (clip, at, volume, pitch): (String, LVec3, Option<f32>, Option<f32>)| {
                with_world(lua, |w| {
                    let World { audio, scene, .. } = w;
                    audio.play_clip_at_point_pitched(
                        scene,
                        &clip,
                        at.0,
                        volume.unwrap_or(1.0),
                        pitch.unwrap_or(1.0),
                    );
                })
            },
        )?,
    )?;
    g.set("AudioSource", source)?;
    let mixers: Table = lua.create_table()?;
    mixers.set(
        "Find",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| w.audio.find_mixer(&name).map(LMixer))
        })?,
    )?;
    g.set("AudioMixer", mixers)?;
    Ok(())
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
