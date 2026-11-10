//! Lua `Weather`: change the weather from scripts (a layer over every Weather volume until
//! `Weather.ClearState`), read surface wetness and snow cover, and force lightning.
//!
//! - `Weather.SetState(name [, seconds])`: blend to a preset ("Clear", "Drizzle", "LightRain",
//!   "Rain", "HeavyRain", "Downpour", "Thunderstorm", "Sleet", "Flurries", "LightSnow", "Snow",
//!   "HeavySnow", "Blizzard"); returns false for an unknown name.
//! - `Weather.SetRain(intensity [, seconds])`, `Weather.SetSnow(intensity [, seconds])`,
//!   `Weather.SetIntensity(intensity [, seconds])`, `Weather.SetWind(degrees, speed [, seconds])`,
//!   `Weather.SetLightning(perMinute [, seconds])`, `Weather.ClearState([seconds])`.
//! - `Weather.GetState()`, `GetRain()`, `GetSnow()`, `GetWind()`, `GetVisibility()`,
//!   `GetWetness()`, `GetPuddles()`, `GetSnowCover()`, `SetWetness(v)`, `SetSnowCover(v)`,
//!   `IsScripted()`, `Strike([position [, intensity]])`, `Weather.states`.
//! - Events (to every script and graph, like custom events): `OnLightning(distance, position,
//!   intensity)`, `OnThunder(distance, volume)`, `OnWeatherChanged(state, seconds)`.

use glam::Vec3;
use mlua::{Lua, Result};

use super::api::{LVec3, with_world};
use crate::visual_script::Value as GValue;
use crate::visual_script::runtime::CustomEvent;
use crate::weather::{WeatherEvent, WeatherParams, WeatherPreset};

/// Rain presets by intensity, for `SetRain` / `SetIntensity`.
const RAIN_STEPS: [(f32, WeatherPreset); 6] = [
    (0.0, WeatherPreset::Clear),
    (0.15, WeatherPreset::Drizzle),
    (0.3, WeatherPreset::LightRain),
    (0.55, WeatherPreset::Rain),
    (0.8, WeatherPreset::HeavyRain),
    (1.0, WeatherPreset::Downpour),
];
const SNOW_STEPS: [(f32, WeatherPreset); 6] = [
    (0.0, WeatherPreset::Clear),
    (0.15, WeatherPreset::Flurries),
    (0.35, WeatherPreset::LightSnow),
    (0.6, WeatherPreset::Snow),
    (0.85, WeatherPreset::HeavySnow),
    (1.0, WeatherPreset::Blizzard),
];

/// The presets of `steps` blended at `intensity` (0..1).
pub fn stepped(steps: &[(f32, WeatherPreset)], intensity: f32) -> WeatherParams {
    let x = intensity.clamp(0.0, 1.0);
    for w in steps.windows(2) {
        let ((a, pa), (b, pb)) = (w[0], w[1]);
        if x <= b {
            let t = if b > a { (x - a) / (b - a) } else { 1.0 };
            return pa.params().lerp(&pb.params(), t);
        }
    }
    steps[steps.len() - 1].1.params()
}

/// Rain fields of `src` into `dst`.
fn copy_rain(dst: &mut WeatherParams, src: &WeatherParams) {
    dst.rain_intensity = src.rain_intensity;
    dst.rain_density = src.rain_density;
    dst.drop_size = src.drop_size;
    dst.rain_speed = src.rain_speed;
    dst.rain_opacity = src.rain_opacity;
    dst.splashes = src.splashes;
    dst.mist = src.mist;
}

fn copy_snow(dst: &mut WeatherParams, src: &WeatherParams) {
    dst.snow_intensity = src.snow_intensity;
    dst.snow_density = src.snow_density;
    dst.flake_size = src.flake_size;
    dst.snow_speed = src.snow_speed;
    dst.snow_turbulence = src.snow_turbulence;
}

/// Visibility and cloud cover follow the heavier of the rain and snow.
fn copy_sky(dst: &mut WeatherParams, a: &WeatherParams, b: &WeatherParams) {
    let heavy = if a.visibility < b.visibility { a } else { b };
    dst.visibility = heavy.visibility;
    dst.cloud_coverage = a.cloud_coverage.max(b.cloud_coverage);
    dst.cloud_density = a.cloud_density.max(b.cloud_density);
    dst.sun_dimming = a.sun_dimming.max(b.sun_dimming);
    dst.ambient_dimming = a.ambient_dimming.max(b.ambient_dimming);
}

pub fn set_rain(p: &mut WeatherParams, intensity: f32) {
    let r = stepped(&RAIN_STEPS, intensity);
    let s = stepped(&SNOW_STEPS, p.snow_intensity);
    copy_rain(p, &r);
    copy_sky(p, &r, &s);
}

pub fn set_snow(p: &mut WeatherParams, intensity: f32) {
    let s = stepped(&SNOW_STEPS, intensity);
    let r = stepped(&RAIN_STEPS, p.rain_intensity);
    copy_snow(p, &s);
    copy_sky(p, &r, &s);
}

fn seconds(s: Option<f32>) -> f32 {
    s.filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0)
}

/// A weather event as the broadcast custom event scripts and graphs hear.
pub fn custom_event(e: &WeatherEvent) -> CustomEvent {
    let (name, args) = match e {
        WeatherEvent::Lightning {
            position,
            distance,
            intensity,
        } => (
            "OnLightning",
            vec![
                GValue::Float(*distance),
                GValue::Vec3(*position),
                GValue::Float(*intensity),
            ],
        ),
        WeatherEvent::Thunder { distance, volume } => (
            "OnThunder",
            vec![GValue::Float(*distance), GValue::Float(*volume)],
        ),
        WeatherEvent::Changed { state, transition } => (
            "OnWeatherChanged",
            vec![GValue::str(state), GValue::Float(*transition)],
        ),
    };
    CustomEvent {
        target: None,
        name: name.into(),
        args,
    }
}

/// Register the global `Weather` table.
pub fn register(lua: &Lua) -> Result<()> {
    let t = lua.create_table()?;
    t.set(
        "SetState",
        lua.create_function(|lua, (name, secs): (String, Option<f32>)| {
            let Some(p) = WeatherPreset::parse(&name) else {
                return Ok(false);
            };
            with_world(lua, |w| {
                w.scene
                    .weather
                    .set_state(p.params(), seconds(secs), p.key());
            })?;
            Ok(true)
        })?,
    )?;
    t.set(
        "ClearState",
        lua.create_function(|lua, secs: Option<f32>| {
            with_world(lua, |w| w.scene.weather.release(seconds(secs)))
        })?,
    )?;
    t.set(
        "SetRain",
        lua.create_function(|lua, (v, secs): (f32, Option<f32>)| {
            with_world(lua, |w| {
                w.scene.weather.modify(seconds(secs), |p| set_rain(p, v))
            })
        })?,
    )?;
    t.set(
        "SetSnow",
        lua.create_function(|lua, (v, secs): (f32, Option<f32>)| {
            with_world(lua, |w| {
                w.scene.weather.modify(seconds(secs), |p| set_snow(p, v))
            })
        })?,
    )?;
    t.set(
        "SetIntensity",
        lua.create_function(|lua, (v, secs): (f32, Option<f32>)| {
            with_world(lua, |w| {
                w.scene.weather.modify(seconds(secs), |p| {
                    let (rain, snow) = (p.rain_intensity > 0.0, p.snow_intensity > 0.0);
                    if rain || !snow {
                        set_rain(p, v);
                    }
                    if snow {
                        set_snow(p, v);
                    }
                })
            })
        })?,
    )?;
    t.set(
        "SetWind",
        lua.create_function(|lua, (deg, speed, secs): (f32, f32, Option<f32>)| {
            with_world(lua, |w| {
                w.scene.weather.modify(seconds(secs), |p| {
                    p.wind_direction = deg;
                    p.wind_speed = speed.max(0.0);
                })
            })
        })?,
    )?;
    t.set(
        "SetLightning",
        lua.create_function(|lua, (rate, secs): (f32, Option<f32>)| {
            with_world(lua, |w| {
                w.scene
                    .weather
                    .modify(seconds(secs), |p| p.lightning_frequency = rate.max(0.0))
            })
        })?,
    )?;
    t.set(
        "Strike",
        lua.create_function(|lua, (pos, intensity): (Option<LVec3>, Option<f32>)| {
            with_world(lua, |w| {
                let wx = &mut w.scene.weather;
                let at = pos.map(|p| p.0).unwrap_or_else(|| {
                    let a = (wx.clock as f32 * 12.9898).sin() * 43758.547;
                    let a = (a - a.floor()) * std::f32::consts::TAU;
                    wx.camera + Vec3::new(a.sin(), 0.0, a.cos()) * 1800.0
                });
                wx.strike(at, intensity.unwrap_or(1.0).max(0.0));
            })
        })?,
    )?;
    t.set(
        "GetState",
        lua.create_function(|lua, ()| {
            with_world(lua, |w| {
                w.scene.weather.current.nearest_preset().key().to_owned()
            })
        })?,
    )?;
    t.set(
        "GetRain",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.current.rain_intensity))?,
    )?;
    t.set(
        "GetSnow",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.current.snow_intensity))?,
    )?;
    t.set(
        "GetWind",
        lua.create_function(|lua, ()| with_world(lua, |w| LVec3(w.scene.weather.wind)))?,
    )?;
    t.set(
        "GetVisibility",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.current.visibility))?,
    )?;
    t.set(
        "GetWetness",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.wetness()))?,
    )?;
    t.set(
        "GetPuddles",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.puddles()))?,
    )?;
    t.set(
        "GetSnowCover",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.snow_cover()))?,
    )?;
    t.set(
        "SetWetness",
        lua.create_function(|lua, v: f32| {
            with_world(lua, |w| w.scene.weather.set_surfaces(Some(v), None))
        })?,
    )?;
    t.set(
        "SetSnowCover",
        lua.create_function(|lua, v: f32| {
            with_world(lua, |w| w.scene.weather.set_surfaces(None, Some(v)))
        })?,
    )?;
    t.set(
        "IsScripted",
        lua.create_function(|lua, ()| with_world(lua, |w| w.scene.weather.scripted()))?,
    )?;
    let states = lua.create_table()?;
    for (i, p) in WeatherPreset::ALL.iter().enumerate() {
        states.set(i + 1, p.key())?;
    }
    t.set("states", states)?;
    lua.globals().set("Weather", t)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::physics::Physics;
    use crate::scene::{Scene, ScriptRef, Scripts};
    use crate::scripting::ScriptHost;

    #[test]
    fn stepped_intensity_hits_the_presets() {
        assert_eq!(stepped(&RAIN_STEPS, 0.55), WeatherPreset::Rain.params());
        assert_eq!(stepped(&RAIN_STEPS, 1.0), WeatherPreset::Downpour.params());
        assert_eq!(stepped(&SNOW_STEPS, 0.0), WeatherPreset::Clear.params());
        let half = stepped(&RAIN_STEPS, 0.675);
        assert!(half.rain_density > 22.0 && half.rain_density < 34.0);
        let mut p = WeatherPreset::Snow.params();
        set_rain(&mut p, 0.4);
        assert!(p.rain_density > 0.0 && p.snow_density > 0.0, "sleet");
    }

    #[test]
    fn lua_weather_api_sets_state_and_hears_events() {
        let dir = std::env::temp_dir().join("misty_weather_api");
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        std::fs::write(
            dir.join("Assets/Storm.lua"),
            r#"local S = {}
function S:Start()
    assert(Weather.SetState("HeavyRain", 0))
    assert(not Weather.SetState("Volcano"))
    assert(#Weather.states == 13)
    self.strikes = 0
    self.changes = ""
end
function S:Update()
    self.frame = (self.frame or 0) + 1
    if self.frame == 3 then
        assert(Weather.GetState() == "HeavyRain", Weather.GetState())
        assert(Weather.GetRain() > 0.7)
        assert(Weather.IsScripted())
        Weather.Strike(Vector3.new(0, 0, 343))
        Weather.SetWind(90, 12)
        Weather.SetWetness(0.6)
    elseif self.frame == 5 then
        local w = Weather.GetWind()
        assert(w.x > 5, tostring(w.x))
        assert(math.abs(Weather.GetWetness() - 0.6) < 0.05, Weather.GetWetness())
        Weather.SetSnow(1, 0)
        Weather.SetRain(0, 0)
    elseif self.frame == 7 then
        assert(Weather.GetSnow() > 0.9)
        assert(Weather.GetVisibility() < 200)
        Weather.ClearState(0)
    end
end
function S:OnLightning(distance, position, intensity)
    self.strikes = self.strikes + 1
    self.distance = distance
    self.transform.position = position
end
function S:OnWeatherChanged(state, seconds)
    self.changes = self.changes .. state .. ";"
end
return S
"#,
        )
        .unwrap();
        let mut s = Scene::new("w");
        let e = s.spawn("Storm", None);
        s.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Storm.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&s, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut s, physics, dir);
        let time = crate::runtime::Time {
            delta: 1.0 / 60.0,
            ..Default::default()
        };
        for _ in 0..10 {
            crate::weather::update_scene(&mut s, time.delta, Vec3::ZERO);
            host.update(&mut s, &time, Default::default());
            let logs = host.take_logs();
            assert!(logs.is_empty(), "{logs:?}");
        }
        // The forced strike 343 m away: the script moved itself there.
        let p = s.world_position(e);
        assert!((p.z - 343.0).abs() < 0.1, "{p}");
        assert!(!s.weather.scripted());
    }
}
