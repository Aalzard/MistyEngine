//! Lua `RenderSettings`: the scene's Lighting window values (fog, fogColor, fogDensity,
//! fogStartDistance, fogEndDistance, ambientIntensity, reflectionIntensity). Changes apply
//! to the running scene immediately.

use mlua::{Lua, Result, UserData, UserDataFields};

use super::api::with_world;
use super::ui_api::LColor;
use crate::scene::render_settings::RenderSettings;

struct LRenderSettings;

fn get<R>(lua: &Lua, f: impl FnOnce(&RenderSettings) -> R) -> Result<R> {
    with_world(lua, |w| f(&w.scene.render_settings))
}

fn set(lua: &Lua, f: impl FnOnce(&mut RenderSettings)) -> Result<()> {
    with_world(lua, |w| f(&mut w.scene.render_settings))
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
}
