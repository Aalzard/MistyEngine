//! Lua `RenderSettings`: the scene's Lighting window values (fog, fogColor, fogDensity,
//! fogStartDistance, fogEndDistance, ambientIntensity, reflectionIntensity, and the moon:
//! moon, moonMode, moonPhase, moonAzimuth, moonAltitude, moonDate, moonTime, moonUtcOffset,
//! moonLatitude, moonLongitude, moonSize, moonBrightness, moonEarthshine, moonLightIntensity,
//! moonLightColor, stars, moonTexture, and read-only moonIllumination / moonDirection). Changes
//! apply to the running scene immediately.

use mlua::{Lua, Result, UserData, UserDataFields};

use super::api::{LVec3, with_world};
use super::ui_api::LColor;
use crate::moon::{MoonMode, MoonSettings};
use crate::scene::render_settings::RenderSettings;

struct LRenderSettings;

fn get<R>(lua: &Lua, f: impl FnOnce(&RenderSettings) -> R) -> Result<R> {
    with_world(lua, |w| f(&w.scene.render_settings))
}

fn set(lua: &Lua, f: impl FnOnce(&mut RenderSettings)) -> Result<()> {
    with_world(lua, |w| f(&mut w.scene.render_settings))
}

fn moon_get<R>(lua: &Lua, f: impl FnOnce(&MoonSettings) -> R) -> Result<R> {
    get(lua, |s| f(&s.moon))
}

fn moon_set(lua: &Lua, f: impl FnOnce(&mut MoonSettings)) -> Result<()> {
    set(lua, |s| f(&mut s.moon))
}

/// The scene's moon as the sky draws it (None: off or no sun to follow).
fn moon_state(lua: &Lua) -> Result<Option<crate::moon::MoonState>> {
    with_world(lua, |w| {
        let lights = crate::render_scene::lights(&w.scene);
        let env = crate::render::Environment::new(w.scene.render_settings.clone(), &lights);
        crate::render::moon::moon_state(&env)
    })
}

/// "YYYY-MM-DD" → (year, month, day).
fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut it = s.trim().split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok().filter(|m| (1..=12).contains(m))?;
    let d = it.next()?.parse().ok().filter(|d| (1..=31).contains(d))?;
    it.next().is_none().then_some((y, m, d))
}

impl UserData for LRenderSettings {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("fog", |lua, _| get(lua, |s| s.fog.enabled));
        f.add_field_method_set("fog", |lua, _, v: bool| set(lua, |s| s.fog.enabled = v));
        f.add_field_method_get("fogColor", |lua, _| {
            get(lua, |s| {
                let c = s.fog.color;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("fogColor", |lua, _, c: LColor| {
            set(lua, |s| s.fog.color = [c.0[0], c.0[1], c.0[2]])
        });
        f.add_field_method_get("fogDensity", |lua, _| get(lua, |s| s.fog.density));
        f.add_field_method_set("fogDensity", |lua, _, v: f32| {
            set(lua, |s| s.fog.density = v.max(0.0))
        });
        f.add_field_method_get("fogStartDistance", |lua, _| get(lua, |s| s.fog.start));
        f.add_field_method_set("fogStartDistance", |lua, _, v: f32| {
            set(lua, |s| s.fog.start = v)
        });
        f.add_field_method_get("fogEndDistance", |lua, _| get(lua, |s| s.fog.end));
        f.add_field_method_set("fogEndDistance", |lua, _, v: f32| {
            set(lua, |s| s.fog.end = v)
        });
        f.add_field_method_get("ambientIntensity", |lua, _| {
            get(lua, |s| s.environment_lighting.intensity)
        });
        f.add_field_method_set("ambientIntensity", |lua, _, v: f32| {
            set(lua, |s| s.environment_lighting.intensity = v.max(0.0))
        });
        f.add_field_method_get("reflectionIntensity", |lua, _| {
            get(lua, |s| s.reflection_intensity)
        });
        f.add_field_method_set("reflectionIntensity", |lua, _, v: f32| {
            set(lua, |s| s.reflection_intensity = v.max(0.0))
        });
        // Moon on/off (disc, stars and moonlight at night)
        f.add_field_method_get("moon", |lua, _| moon_get(lua, |m| m.enabled));
        f.add_field_method_set("moon", |lua, _, v: bool| moon_set(lua, |m| m.enabled = v));
        // How the moon is placed: "FollowSun", "Manual" or "DateTime"
        f.add_field_method_get("moonMode", |lua, _| {
            moon_get(lua, |m| m.mode.name().to_owned())
        });
        f.add_field_method_set("moonMode", |lua, _, v: String| {
            let mode = MoonMode::from_name(&v).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "moonMode must be FollowSun, Manual or DateTime, not {v:?}"
                ))
            })?;
            moon_set(lua, |m| m.mode = mode)
        });
        // Moon phase 0..1: 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter (FollowSun, Manual)
        f.add_field_method_get("moonPhase", |lua, _| moon_get(lua, |m| m.phase));
        f.add_field_method_set("moonPhase", |lua, _, v: f32| {
            moon_set(lua, |m| m.phase = v.rem_euclid(1.0))
        });
        // Manual moon azimuth in degrees from north (+Z) towards east (+X)
        f.add_field_method_get("moonAzimuth", |lua, _| moon_get(lua, |m| m.azimuth));
        f.add_field_method_set("moonAzimuth", |lua, _, v: f32| {
            moon_set(lua, |m| m.azimuth = v.rem_euclid(360.0))
        });
        // Manual moon altitude in degrees above the horizon
        f.add_field_method_get("moonAltitude", |lua, _| moon_get(lua, |m| m.altitude));
        f.add_field_method_set("moonAltitude", |lua, _, v: f32| {
            moon_set(lua, |m| m.altitude = v.clamp(-90.0, 90.0))
        });
        // DateTime local date as "YYYY-MM-DD"
        f.add_field_method_get("moonDate", |lua, _| {
            moon_get(lua, |m| {
                format!("{:04}-{:02}-{:02}", m.year, m.month, m.day)
            })
        });
        f.add_field_method_set("moonDate", |lua, _, v: String| {
            let (y, mo, d) = parse_date(&v).ok_or_else(|| {
                mlua::Error::RuntimeError(format!("moonDate must be \"YYYY-MM-DD\", not {v:?}"))
            })?;
            moon_set(lua, |m| (m.year, m.month, m.day) = (y, mo, d))
        });
        // DateTime local time in hours (22.5 = 22:30)
        f.add_field_method_get("moonTime", |lua, _| moon_get(lua, |m| m.time));
        f.add_field_method_set("moonTime", |lua, _, v: f32| {
            moon_set(lua, |m| m.time = v.clamp(0.0, 24.0))
        });
        // DateTime hours east of UTC
        f.add_field_method_get("moonUtcOffset", |lua, _| moon_get(lua, |m| m.utc_offset));
        f.add_field_method_set("moonUtcOffset", |lua, _, v: f32| {
            moon_set(lua, |m| m.utc_offset = v.clamp(-14.0, 14.0))
        });
        // Observer latitude in degrees north (DateTime, and the sky's pole for FollowSun)
        f.add_field_method_get("moonLatitude", |lua, _| moon_get(lua, |m| m.latitude));
        f.add_field_method_set("moonLatitude", |lua, _, v: f32| {
            moon_set(lua, |m| m.latitude = v.clamp(-90.0, 90.0))
        });
        // Observer longitude in degrees east (DateTime)
        f.add_field_method_get("moonLongitude", |lua, _| moon_get(lua, |m| m.longitude));
        f.add_field_method_set("moonLongitude", |lua, _, v: f32| {
            moon_set(lua, |m| m.longitude = v.clamp(-180.0, 180.0))
        });
        // Moon size, multiple of its real size
        f.add_field_method_get("moonSize", |lua, _| moon_get(lua, |m| m.size));
        f.add_field_method_set("moonSize", |lua, _, v: f32| {
            moon_set(lua, |m| m.size = v.clamp(0.0, 40.0))
        });
        // Moon disc brightness multiplier
        f.add_field_method_get("moonBrightness", |lua, _| moon_get(lua, |m| m.brightness));
        f.add_field_method_set("moonBrightness", |lua, _, v: f32| {
            moon_set(lua, |m| m.brightness = v.max(0.0))
        });
        // Earthshine on the moon's dark side
        f.add_field_method_get("moonEarthshine", |lua, _| moon_get(lua, |m| m.earthshine));
        f.add_field_method_set("moonEarthshine", |lua, _, v: f32| {
            moon_set(lua, |m| m.earthshine = v.max(0.0))
        });
        // Moonlight intensity for a full moon high in the sky
        f.add_field_method_get("moonLightIntensity", |lua, _| {
            moon_get(lua, |m| m.light_intensity)
        });
        f.add_field_method_set("moonLightIntensity", |lua, _, v: f32| {
            moon_set(lua, |m| m.light_intensity = v.max(0.0))
        });
        // Moonlight colour
        f.add_field_method_get("moonLightColor", |lua, _| {
            moon_get(lua, |m| {
                let c = m.light_color;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("moonLightColor", |lua, _, c: LColor| {
            moon_set(lua, |m| m.light_color = [c.0[0], c.0[1], c.0[2]])
        });
        // Star field brightness at night (0 = none)
        f.add_field_method_get("stars", |lua, _| moon_get(lua, |m| m.stars));
        f.add_field_method_set("stars", |lua, _, v: f32| {
            moon_set(lua, |m| m.stars = v.max(0.0))
        });
        // Project path of the moon texture, nil = built-in
        f.add_field_method_get("moonTexture", |lua, _| moon_get(lua, |m| m.texture.clone()));
        f.add_field_method_set("moonTexture", |lua, _, v: Option<String>| {
            moon_set(lua, |m| m.texture = v.filter(|s| !s.is_empty()))
        });
        // Lit fraction of the moon's disc, 0 new .. 1 full (read-only)
        f.add_field_method_get("moonIllumination", |lua, _| {
            Ok(moon_state(lua)?.map_or(0.0, |m| m.illumination))
        });
        // Unit Vector3 towards the moon, nil when it is off (read-only)
        f.add_field_method_get("moonDirection", |lua, _| {
            Ok(moon_state(lua)?.map(|m| LVec3(m.direction)))
        });
    }
}

/// Register the `RenderSettings` global.
pub fn register(lua: &Lua) -> Result<()> {
    lua.globals().set("RenderSettings", LRenderSettings)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::Vec3;

    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Scene, ScriptRef, Scripts};
    use crate::scripting::{Input, ScriptHost};

    fn project(source: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_rs_script_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(root.join("Assets/Scripts/Fog.lua"), source).unwrap();
        root
    }

    #[test]
    fn scripts_read_and_change_fog_and_ambient() {
        let src = r#"
local F = {}
function F:Start()
    Debug.Log(tostring(RenderSettings.fog), RenderSettings.ambientIntensity)
    RenderSettings.fog = true
    RenderSettings.fogColor = Color.new(1, 0, 0)
    RenderSettings.ambientIntensity = 0.5
    RenderSettings.fogDensity = 0.25
    local c = RenderSettings.fogColor
    Debug.Log(tostring(RenderSettings.fog), c.r, c.g, RenderSettings.fogDensity)
end
return F
"#;
        let root = project(src);
        let mut scene = Scene::new("Fog");
        let e = scene.spawn("Runner", None);
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Fog.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root);
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs: Vec<String> = host.take_logs().into_iter().map(|(_, t)| t).collect();
        assert_eq!(logs, ["false 1", "true 1 0 0.25"], "{logs:?}");
        let rs = &scene.render_settings;
        assert!(rs.fog.enabled);
        assert_eq!(rs.fog.color, [1.0, 0.0, 0.0]);
        assert_eq!(rs.environment_lighting.intensity, 0.5);
    }

    #[test]
    fn scripts_drive_the_moon() {
        let src = r#"
local F = {}
function F:Start()
    Debug.Log(tostring(RenderSettings.moon), RenderSettings.moonMode, RenderSettings.moonPhase)
    RenderSettings.moonPhase = 0.25
    RenderSettings.moonMode = "DateTime"
    RenderSettings.moonDate = "2024-04-23"
    RenderSettings.moonTime = 23.8
    RenderSettings.moonSize = 4
    RenderSettings.stars = 0.5
    Debug.Log(RenderSettings.moonDate, string.format("%.2f", RenderSettings.moonIllumination))
    local ok = pcall(function() RenderSettings.moonMode = "Sideways" end)
    Debug.Log(tostring(ok), tostring(RenderSettings.moonDirection ~= nil))
end
return F
"#;
        let root = project(src);
        let mut scene = Scene::new("Moon");
        let e = scene.spawn("Runner", None);
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Fog.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root);
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs: Vec<String> = host.take_logs().into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            logs,
            ["true FollowSun 0.5", "2024-04-23 1.00", "false true"],
            "{logs:?}"
        );
        let m = &scene.render_settings.moon;
        assert_eq!((m.year, m.month, m.day), (2024, 4, 23));
        assert_eq!(m.phase, 0.25);
        assert_eq!(m.size, 4.0);
    }
}
