//! Lua `RenderSettings`: the scene's Lighting window values (fog, fogColor, fogDensity,
//! fogStartDistance, fogEndDistance, ambientIntensity, reflectionIntensity, and the moon:
//! moon, moonMode, moonPhase, moonAzimuth, moonAltitude, moonDate, moonTime, moonUtcOffset,
//! moonLatitude, moonLongitude, moonSize, moonBrightness, moonEarthshine, moonLightIntensity,
//! moonLightColor, moonTexture, read-only moonIllumination / moonDirection, and the night sky:
//! nightSky, nightSkyDarkness, stars*, skyTime, milkyWay*, lightPollution*, aurora*, meteor*).
//! Changes apply to the running scene immediately.

use mlua::{Lua, Result, UserData, UserDataFields};

use super::api::{LVec3, with_world};
use super::ui_api::LColor;
use crate::moon::night_sky::NightSkySettings;
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

fn night_get<R>(lua: &Lua, f: impl FnOnce(&NightSkySettings) -> R) -> Result<R> {
    get(lua, |s| f(&s.night_sky))
}

fn night_set(lua: &Lua, f: impl FnOnce(&mut NightSkySettings)) -> Result<()> {
    set(lua, |s| f(&mut s.night_sky))
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
        // Night sky on/off (dark blue sky, stars, Milky Way, aurora, meteors)
        f.add_field_method_get("nightSky", |lua, _| night_get(lua, |n| n.enabled));
        f.add_field_method_set("nightSky", |lua, _, v: bool| {
            night_set(lua, |n| n.enabled = v)
        });
        // Night Sky Darkness: darkens the night sky colour, 0..1, stars stay
        f.add_field_method_get("nightSkyDarkness", |lua, _| night_get(lua, |n| n.darkness));
        f.add_field_method_set("nightSkyDarkness", |lua, _, v: f32| {
            night_set(lua, |n| n.darkness = v.clamp(0.0, 1.0))
        });
        // Star field brightness at night (0 = none)
        f.add_field_method_get("stars", |lua, _| night_get(lua, |n| n.star_brightness));
        f.add_field_method_set("stars", |lua, _, v: f32| {
            night_set(lua, |n| n.star_brightness = v.max(0.0))
        });
        // How many stars, 1 = default, up to 3
        f.add_field_method_get("starDensity", |lua, _| night_get(lua, |n| n.star_density));
        f.add_field_method_set("starDensity", |lua, _, v: f32| {
            night_set(lua, |n| n.star_density = v.clamp(0.0, 3.0))
        });
        // 0 = stars about equally bright, 1 = a few very bright among many faint
        f.add_field_method_get("starMagnitudeSpread", |lua, _| {
            night_get(lua, |n| n.star_magnitude_spread)
        });
        f.add_field_method_set("starMagnitudeSpread", |lua, _, v: f32| {
            night_set(lua, |n| n.star_magnitude_spread = v.clamp(0.0, 1.0))
        });
        // Glow and bloom around the bright stars
        f.add_field_method_get("starGlow", |lua, _| night_get(lua, |n| n.star_glow));
        f.add_field_method_set("starGlow", |lua, _, v: f32| {
            night_set(lua, |n| n.star_glow = v.clamp(0.0, 4.0))
        });
        // Star colour temperature spread, bluish to orange
        f.add_field_method_get("starColorVariation", |lua, _| {
            night_get(lua, |n| n.star_color_variation)
        });
        f.add_field_method_set("starColorVariation", |lua, _, v: f32| {
            night_set(lua, |n| n.star_color_variation = v.clamp(0.0, 1.0))
        });
        // Twinkle strength (stronger near the horizon)
        f.add_field_method_get("starTwinkle", |lua, _| night_get(lua, |n| n.star_twinkle));
        f.add_field_method_set("starTwinkle", |lua, _, v: f32| {
            night_set(lua, |n| n.star_twinkle = v.clamp(0.0, 2.0))
        });
        // Twinkle speed
        f.add_field_method_get("starTwinkleSpeed", |lua, _| {
            night_get(lua, |n| n.star_twinkle_speed)
        });
        f.add_field_method_set("starTwinkleSpeed", |lua, _, v: f32| {
            night_set(lua, |n| n.star_twinkle_speed = v.max(0.0))
        });
        // How much a bright sky washes out stars and the Milky Way
        f.add_field_method_get("starFade", |lua, _| night_get(lua, |n| n.star_fade));
        f.add_field_method_set("starFade", |lua, _, v: f32| {
            night_set(lua, |n| n.star_fade = v.clamp(0.0, 4.0))
        });
        // Local sidereal time in hours: turns the stars and the Milky Way (not in DateTime mode)
        f.add_field_method_get("skyTime", |lua, _| night_get(lua, |n| n.sky_time));
        f.add_field_method_set("skyTime", |lua, _, v: f32| {
            night_set(lua, |n| n.sky_time = v.rem_euclid(24.0))
        });
        // Milky Way visibility (0 = none)
        f.add_field_method_get("milkyWay", |lua, _| night_get(lua, |n| n.milky_way));
        f.add_field_method_set("milkyWay", |lua, _, v: f32| {
            night_set(lua, |n| n.milky_way = v.max(0.0))
        });
        // Milky Way core brightness
        f.add_field_method_get("milkyWayCore", |lua, _| {
            night_get(lua, |n| n.milky_way_core)
        });
        f.add_field_method_set("milkyWayCore", |lua, _, v: f32| {
            night_set(lua, |n| n.milky_way_core = v.max(0.0))
        });
        // Milky Way dust lane strength
        f.add_field_method_get("milkyWayDust", |lua, _| {
            night_get(lua, |n| n.milky_way_dust)
        });
        f.add_field_method_set("milkyWayDust", |lua, _, v: f32| {
            night_set(lua, |n| n.milky_way_dust = v.clamp(0.0, 2.0))
        });
        // Milky Way width multiplier
        f.add_field_method_get("milkyWayWidth", |lua, _| {
            night_get(lua, |n| n.milky_way_width)
        });
        f.add_field_method_set("milkyWayWidth", |lua, _, v: f32| {
            night_set(lua, |n| n.milky_way_width = v.clamp(0.1, 4.0))
        });
        // Horizon glow of towns; washes out stars
        f.add_field_method_get("lightPollution", |lua, _| {
            night_get(lua, |n| n.light_pollution)
        });
        f.add_field_method_set("lightPollution", |lua, _, v: f32| {
            night_set(lua, |n| n.light_pollution = v.max(0.0))
        });
        // Aurora visibility (0 = none)
        f.add_field_method_get("aurora", |lua, _| night_get(lua, |n| n.aurora));
        f.add_field_method_set("aurora", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora = v.max(0.0))
        });
        // Aurora lower edge in km
        f.add_field_method_get("auroraHeight", |lua, _| night_get(lua, |n| n.aurora_height));
        f.add_field_method_set("auroraHeight", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_height = v.clamp(10.0, 1000.0))
        });
        // Aurora thickness in km
        f.add_field_method_get("auroraThickness", |lua, _| {
            night_get(lua, |n| n.aurora_thickness)
        });
        f.add_field_method_set("auroraThickness", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_thickness = v.clamp(10.0, 1000.0))
        });
        // Aurora fold amplitude
        f.add_field_method_get("auroraWaviness", |lua, _| {
            night_get(lua, |n| n.aurora_waviness)
        });
        f.add_field_method_set("auroraWaviness", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_waviness = v.clamp(0.0, 3.0))
        });
        // Aurora vertical ray detail
        f.add_field_method_get("auroraRays", |lua, _| night_get(lua, |n| n.aurora_rays));
        f.add_field_method_set("auroraRays", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_rays = v.clamp(0.0, 3.0))
        });
        // Aurora direction in degrees from north towards east
        f.add_field_method_get("auroraAzimuth", |lua, _| {
            night_get(lua, |n| n.aurora_azimuth)
        });
        f.add_field_method_set("auroraAzimuth", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_azimuth = v.rem_euclid(360.0))
        });
        // Aurora distance over the ground in km (far = low on the sky)
        f.add_field_method_get("auroraDistance", |lua, _| {
            night_get(lua, |n| n.aurora_distance)
        });
        f.add_field_method_set("auroraDistance", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_distance = v.clamp(0.0, 3000.0))
        });
        // Aurora animation speed
        f.add_field_method_get("auroraSpeed", |lua, _| night_get(lua, |n| n.aurora_speed));
        f.add_field_method_set("auroraSpeed", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_speed = v.max(0.0))
        });
        // Aurora pulsing and flicker
        f.add_field_method_get("auroraPulse", |lua, _| night_get(lua, |n| n.aurora_pulse));
        f.add_field_method_set("auroraPulse", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_pulse = v.clamp(0.0, 2.0))
        });
        // How much the aurora tints the scene's sky light
        f.add_field_method_get("auroraAmbient", |lua, _| {
            night_get(lua, |n| n.aurora_ambient)
        });
        f.add_field_method_set("auroraAmbient", |lua, _, v: f32| {
            night_set(lua, |n| n.aurora_ambient = v.max(0.0))
        });
        // Shooting stars per minute (0 = none)
        f.add_field_method_get("meteorRate", |lua, _| night_get(lua, |n| n.meteor_rate));
        f.add_field_method_set("meteorRate", |lua, _, v: f32| {
            night_set(lua, |n| n.meteor_rate = v.clamp(0.0, 600.0))
        });
        // Shooting star brightness
        f.add_field_method_get("meteorBrightness", |lua, _| {
            night_get(lua, |n| n.meteor_brightness)
        });
        f.add_field_method_set("meteorBrightness", |lua, _, v: f32| {
            night_set(lua, |n| n.meteor_brightness = v.max(0.0))
        });
        // Milky Way tint
        f.add_field_method_get("milkyWayTint", |lua, _| {
            night_get(lua, |n| {
                let c = n.milky_way_tint;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("milkyWayTint", |lua, _, c: LColor| {
            night_set(lua, |n| n.milky_way_tint = [c.0[0], c.0[1], c.0[2]])
        });
        // Light pollution colour
        f.add_field_method_get("lightPollutionColor", |lua, _| {
            night_get(lua, |n| {
                let c = n.light_pollution_color;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("lightPollutionColor", |lua, _, c: LColor| {
            night_set(lua, |n| n.light_pollution_color = [c.0[0], c.0[1], c.0[2]])
        });
        // Aurora colour low in the curtains
        f.add_field_method_get("auroraColor", |lua, _| {
            night_get(lua, |n| {
                let c = n.aurora_color;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("auroraColor", |lua, _, c: LColor| {
            night_set(lua, |n| n.aurora_color = [c.0[0], c.0[1], c.0[2]])
        });
        // Aurora colour at the curtain tops
        f.add_field_method_get("auroraTopColor", |lua, _| {
            night_get(lua, |n| {
                let c = n.aurora_top_color;
                LColor([c[0], c[1], c[2], 1.0])
            })
        });
        f.add_field_method_set("auroraTopColor", |lua, _, c: LColor| {
            night_set(lua, |n| n.aurora_top_color = [c.0[0], c.0[1], c.0[2]])
        });
        // Number of aurora curtains, 1..6
        f.add_field_method_get("auroraCurtains", |lua, _| {
            night_get(lua, |n| n.aurora_curtains)
        });
        f.add_field_method_set("auroraCurtains", |lua, _, v: u32| {
            night_set(lua, |n| n.aurora_curtains = v.clamp(1, 6))
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
    RenderSettings.aurora = 1.5
    RenderSettings.auroraCurtains = 9
    RenderSettings.milkyWay = 2
    RenderSettings.auroraTopColor = Color.new(1, 0, 0.5)
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
        let n = &scene.render_settings.night_sky;
        assert_eq!((n.star_brightness, n.aurora, n.milky_way), (0.5, 1.5, 2.0));
        assert_eq!(n.aurora_curtains, 6);
        assert_eq!(n.aurora_top_color, [1.0, 0.0, 0.5]);
    }
}
