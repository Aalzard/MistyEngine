//! Lua `Screen`, `FullScreenMode`, `QualitySettings` and `PlayerPrefs`.
//!
//! These talk to process-wide state ([`crate::screen`], [`crate::quality`], [`crate::prefs`]) that
//! the host fills in, so they work the same in the player and the editor's Play mode.

use mlua::{Lua, Result, UserData, UserDataFields, Value};

use crate::player_settings::ScreenMode;
use crate::prefs::{self, PrefValue};
use crate::screen;

/// `FullScreenMode` numbers, as in the familiar API.
const MODES: [(&str, ScreenMode); 4] = [
    ("ExclusiveFullScreen", ScreenMode::ExclusiveFullscreen),
    ("FullScreenWindow", ScreenMode::FullscreenWindow),
    ("MaximizedWindow", ScreenMode::Maximized),
    ("Windowed", ScreenMode::Windowed),
];

fn mode_number(m: ScreenMode) -> i64 {
    MODES.iter().position(|(_, x)| *x == m).unwrap_or(3) as i64
}

/// `true`/`false` (fullscreen window or windowed) or a `FullScreenMode` number.
fn mode_arg(v: &Value, current: ScreenMode) -> ScreenMode {
    match v {
        Value::Boolean(true) => ScreenMode::FullscreenWindow,
        Value::Boolean(false) => ScreenMode::Windowed,
        Value::Integer(i) => usize::try_from(*i)
            .ok()
            .and_then(|i| MODES.get(i))
            .map_or(current, |(_, m)| *m),
        Value::Number(n) => MODES.get(*n as usize).map_or(current, |(_, m)| *m),
        _ => current,
    }
}

struct LScreen;

impl UserData for LScreen {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("width", |_, _| Ok(screen::state().width));
        f.add_field_method_get("height", |_, _| Ok(screen::state().height));
        f.add_field_method_get("fullScreen", |_, _| {
            Ok(screen::state().mode.is_fullscreen())
        });
        f.add_field_method_set("fullScreen", |_, _, on: bool| {
            let s = screen::state();
            let mode = if on {
                ScreenMode::FullscreenWindow
            } else {
                ScreenMode::Windowed
            };
            screen::request(s.width, s.height, mode);
            Ok(())
        });
        f.add_field_method_get("fullScreenMode", |_, _| {
            Ok(mode_number(screen::state().mode))
        });
        f.add_field_method_get("resolutions", |lua, _| {
            let list = lua.create_table()?;
            for (i, r) in screen::state().resolutions.iter().enumerate() {
                let t = lua.create_table()?;
                t.set("width", r.width)?;
                t.set("height", r.height)?;
                t.set("refreshRate", r.refresh)?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        });
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(m: &mut M) {
        // Called with a dot (`Screen.SetResolution(w, h, mode)`), so there is no `self` value.
        m.add_function(
            "SetResolution",
            |_, (w, h, mode): (u32, u32, Option<Value>)| {
                let current = screen::state().mode;
                let mode = mode.map_or(current, |v| mode_arg(&v, current));
                screen::request(w, h, mode);
                Ok(())
            },
        );
    }
}

struct LQuality;

impl UserData for LQuality {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("names", |_, _| Ok(crate::quality::names()));
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(m: &mut M) {
        m.add_function("GetQualityLevel", |_, ()| Ok(crate::quality::level()));
        m.add_function("SetQualityLevel", |_, (i, _): (usize, Option<bool>)| {
            if crate::quality::set_level(i) {
                prefs::set(prefs::keys::QUALITY, PrefValue::Int(i as i64));
            }
            Ok(())
        });
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    g.set("Screen", LScreen)?;
    let modes = lua.create_table()?;
    for (i, (name, _)) in MODES.iter().enumerate() {
        modes.set(*name, i)?;
    }
    g.set("FullScreenMode", modes)?;
    g.set("QualitySettings", LQuality)?;

    let p = lua.create_table()?;
    p.set(
        "GetInt",
        lua.create_function(|_, (k, d): (String, Option<i64>)| {
            Ok(prefs::get_int(&k, d.unwrap_or(0)))
        })?,
    )?;
    p.set(
        "SetInt",
        lua.create_function(|_, (k, v): (String, i64)| {
            prefs::set(&k, PrefValue::Int(v));
            Ok(())
        })?,
    )?;
    p.set(
        "GetFloat",
        lua.create_function(|_, (k, d): (String, Option<f64>)| {
            Ok(prefs::get_float(&k, d.unwrap_or(0.0)))
        })?,
    )?;
    p.set(
        "SetFloat",
        lua.create_function(|_, (k, v): (String, f64)| {
            prefs::set(&k, PrefValue::Float(v));
            Ok(())
        })?,
    )?;
    p.set(
        "GetString",
        lua.create_function(|_, (k, d): (String, Option<String>)| {
            Ok(prefs::get_string(&k, d.as_deref().unwrap_or("")))
        })?,
    )?;
    p.set(
        "SetString",
        lua.create_function(|_, (k, v): (String, String)| {
            prefs::set(&k, PrefValue::String(v));
            Ok(())
        })?,
    )?;
    p.set(
        "HasKey",
        lua.create_function(|_, k: String| Ok(prefs::has_key(&k)))?,
    )?;
    p.set(
        "DeleteKey",
        lua.create_function(|_, k: String| {
            prefs::delete_key(&k);
            Ok(())
        })?,
    )?;
    p.set(
        "DeleteAll",
        lua.create_function(|_, ()| {
            prefs::delete_all();
            Ok(())
        })?,
    )?;
    p.set(
        "Save",
        lua.create_function(|_, ()| prefs::save().map_err(mlua::Error::external))?,
    )?;
    g.set("PlayerPrefs", p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;

    #[test]
    fn screen_quality_and_prefs_from_lua() {
        let _lock = prefs::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lua = Lua::new();
        register(&lua).unwrap();
        screen::set_size(800, 600, ScreenMode::Windowed);
        screen::set_resolutions(vec![screen::Resolution {
            width: 1920,
            height: 1080,
            refresh: 60,
        }]);
        screen::take_requests();
        let out: String = lua
            .load(
                r#"
                Screen.SetResolution(1024, 768, FullScreenMode.ExclusiveFullScreen)
                Screen.SetResolution(640, 480, false)
                local r = Screen.resolutions[1]
                PlayerPrefs.SetInt("app_api_test.score", 12)
                PlayerPrefs.SetString("app_api_test.name", "Bo")
                return Screen.width .. "x" .. Screen.height .. " " .. tostring(Screen.fullScreen)
                    .. " " .. r.width .. " " .. #QualitySettings.names
                    .. " " .. PlayerPrefs.GetInt("app_api_test.score")
                    .. " " .. PlayerPrefs.GetString("app_api_test.name")
                    .. " " .. PlayerPrefs.GetFloat("app_api_test.missing", 1.5)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "800x600 false 1920 4 12 Bo 1.5");
        assert_eq!(
            screen::take_requests(),
            [
                (1024, 768, ScreenMode::ExclusiveFullscreen),
                (640, 480, ScreenMode::Windowed)
            ]
        );
        let level: usize = lua
            .load("QualitySettings.SetQualityLevel(0) return QualitySettings.GetQualityLevel()")
            .eval()
            .unwrap();
        assert_eq!(level, 0);
        assert_eq!(crate::quality::active().name, "Low");
        crate::quality::set_level(2);
        prefs::delete_key("app_api_test.score");
    }
}
