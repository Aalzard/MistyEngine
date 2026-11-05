//! Lua `GetComponent("VirtualCamera")` (priority, follow, lookAt, isLive) and
//! `GetComponent("CameraBrain")` (activeVirtualCamera, isBlending, blendWeight).

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, Value};

use super::api::{LGameObject, LTransform, missing, with_world};
use crate::vcam::{CameraBrain, VirtualCamera};

#[derive(Clone, Copy)]
pub struct LVirtualCamera(pub Entity);

#[derive(Clone, Copy)]
pub struct LCameraBrain(pub Entity);

/// A GameObject, a Transform or nil as an object reference.
fn object_arg(v: Value) -> Result<Option<Entity>> {
    match v {
        Value::Nil => Ok(None),
        Value::UserData(ud) => {
            if let Ok(g) = ud.borrow::<LGameObject>() {
                Ok(Some(g.0))
            } else if let Ok(t) = ud.borrow::<LTransform>() {
                Ok(Some(t.0))
            } else {
                Err(mlua::Error::runtime(
                    "expected a GameObject, Transform or nil",
                ))
            }
        }
        _ => Err(mlua::Error::runtime(
            "expected a GameObject, Transform or nil",
        )),
    }
}

/// Read or change the Virtual Camera of `e`.
fn with_vcam<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut VirtualCamera) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut VirtualCamera>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| missing())
    })?
}

impl UserData for LVirtualCamera {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
        f.add_field_method_get("enabled", |lua, c| with_vcam(lua, c.0, |v| v.enabled));
        f.add_field_method_set("enabled", |lua, c, on: bool| {
            with_vcam(lua, c.0, |v| v.enabled = on)
        });
        f.add_field_method_get("priority", |lua, c| with_vcam(lua, c.0, |v| v.priority));
        f.add_field_method_set("priority", |lua, c, p: i32| {
            with_vcam(lua, c.0, |v| v.priority = p)
        });
        f.add_field_method_get("follow", |lua, c| {
            with_vcam(lua, c.0, |v| v.follow.map(LGameObject))
        });
        f.add_field_method_set("follow", |lua, c, t: Value| {
            let t = object_arg(t)?;
            with_vcam(lua, c.0, |v| v.follow = t)
        });
        f.add_field_method_get("lookAt", |lua, c| {
            with_vcam(lua, c.0, |v| v.look_at.map(LGameObject))
        });
        f.add_field_method_set("lookAt", |lua, c, t: Value| {
            let t = object_arg(t)?;
            with_vcam(lua, c.0, |v| v.look_at = t)
        });
        f.add_field_method_get("fieldOfView", |lua, c| with_vcam(lua, c.0, |v| v.lens.fov));
        f.add_field_method_set("fieldOfView", |lua, c, fov: f32| {
            with_vcam(lua, c.0, |v| v.lens.fov = fov.clamp(1.0, 179.0))
        });
        f.add_field_method_get("dutch", |lua, c| with_vcam(lua, c.0, |v| v.lens.dutch));
        f.add_field_method_set("dutch", |lua, c, d: f32| {
            with_vcam(lua, c.0, |v| v.lens.dutch = d)
        });
        // Shown by a brain (live, or blending to or from it).
        f.add_field_method_get("isLive", |lua, c| {
            with_world(lua, |w| w.cameras.is_live(c.0))
        });
    }
}

impl UserData for LCameraBrain {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
        // GameObject of the live virtual camera (the one being blended to), or nil.
        f.add_field_method_get("activeVirtualCamera", |lua, c| {
            with_world(lua, |w| {
                w.cameras
                    .live_camera(c.0)
                    .or_else(|| w.cameras.pick_live(&w.scene))
                    .map(LGameObject)
            })
        });
        f.add_field_method_get("isBlending", |lua, c| {
            with_world(lua, |w| w.cameras.is_blending(c.0))
        });
        // Weight of the incoming camera, 0..1 (1 when not blending).
        f.add_field_method_get("blendWeight", |lua, c| {
            with_world(lua, |w| w.cameras.blend_weight(c.0).unwrap_or(1.0))
        });
    }
}

/// `GetComponent` for the camera rig components; `None` = not one of them.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |vcam: bool| {
        with_world(lua, |w| {
            if vcam {
                w.scene.world.get::<&VirtualCamera>(e).is_ok()
            } else {
                w.scene.world.get::<&CameraBrain>(e).is_ok()
            }
        })
    };
    Ok(match name {
        "VirtualCamera" | "Virtual Camera" => has(true)?
            .then(|| lua.create_userdata(LVirtualCamera(e)).map(Value::UserData))
            .transpose()?
            .or(Some(Value::Nil)),
        "CameraBrain" | "Camera Brain" => has(false)?
            .then(|| lua.create_userdata(LCameraBrain(e)).map(Value::UserData))
            .transpose()?
            .or(Some(Value::Nil)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Camera, Scene, ScriptRef, Scripts};
    use crate::scripting::ScriptHost;
    use crate::vcam::{BlendDefinition, BlendStyle, BodyKind, CameraBrain, VirtualCamera};

    #[test]
    fn scripts_switch_cameras_and_read_the_brain() {
        let dir = std::env::temp_dir().join("misty_vcam_api");
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        std::fs::write(
            dir.join("Assets/Director.lua"),
            r#"local D = {}
function D:Start()
    self._brain = GameObject.Find("Main Camera"):GetComponent("CameraBrain")
    self._wide = GameObject.Find("Wide"):GetComponent("VirtualCamera")
    self._hero = GameObject.Find("Hero")
    assert(self._wide.priority == 5)
    assert(self._wide.follow == nil)
    assert(GameObject.Find("Hero"):GetComponent("VirtualCamera") == nil)
end
function D:Update()
    local b = self._brain
    if Time.frameCount == 2 then
        assert(b.activeVirtualCamera.name == "Follow", tostring(b.activeVirtualCamera))
        assert(not b.isBlending and b.blendWeight == 1)
        self._wide.priority = 50
        self._wide.lookAt = self._hero
        self._wide.follow = self._hero.transform
        self._wide.follow = nil
    elseif Time.frameCount == 4 then
        assert(b.activeVirtualCamera.name == "Wide")
        assert(b.isBlending, "blending")
        assert(b.blendWeight > 0 and b.blendWeight < 1)
        assert(self._wide.isLive)
        assert(self._wide.lookAt == self._hero)
        print("ok")
    end
end
return D
"#,
        )
        .unwrap();
        let mut s = Scene::new("v");
        let cam = s.spawn("Main Camera", None);
        s.add(cam, Camera::default());
        s.add(
            cam,
            CameraBrain {
                default_blend: BlendDefinition {
                    style: BlendStyle::Linear,
                    duration: 1.0,
                },
                ..Default::default()
            },
        );
        let hero = s.spawn("Hero", None);
        for (name, priority) in [("Follow", 10), ("Wide", 5)] {
            let v = s.spawn(name, None);
            s.set_world_position(v, Vec3::new(priority as f32, 0.0, 0.0));
            s.add(
                v,
                VirtualCamera {
                    priority,
                    body: BodyKind::DoNothing,
                    ..Default::default()
                },
            );
        }
        s.add(
            hero,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Director.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&s, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut s, physics, dir);
        let mut time = Time::default();
        for _ in 0..5 {
            time.advance(1.0 / 60.0);
            host.update(&mut s, &time, Default::default());
        }
        let logs = host.take_logs();
        assert_eq!(logs.len(), 1, "{logs:?}");
        assert_eq!(logs[0].1, "ok");
        let wide = s.find_by_name("Wide").unwrap();
        let x = s.world_position(cam).x;
        assert!(
            x > 10.0 - 1.0 && x < 10.0,
            "blending from Follow (x 10) to Wide (x 5): {x}"
        );
        assert_eq!(
            s.world.get::<&VirtualCamera>(wide).unwrap().look_at,
            Some(hero)
        );
    }
}
