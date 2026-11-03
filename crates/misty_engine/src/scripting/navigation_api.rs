//! Lua navigation API: `GetComponent("NavMeshAgent")` (`SetDestination`, `destination`,
//! `remainingDistance`, `hasPath`, `isStopped`, `velocity`, `Warp`, …) and the `NavMesh` table
//! (`SamplePosition`, `CalculatePath`).

use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::navigation::NavMeshAgent;

#[derive(Clone, Copy)]
pub struct LNavMeshAgent(pub Entity);

fn missing() -> mlua::Error {
    mlua::Error::runtime("object was destroyed or has no NavMeshAgent")
}

/// Run `f` on the agent's live state (placing it on the mesh first).
fn state<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&mut crate::navigation::agent::AgentState) -> R,
) -> Result<Option<R>> {
    with_world(lua, |w| {
        let World {
            scene, navigation, ..
        } = w;
        navigation.agent(scene, e).map(f)
    })
}

/// Read or change the component's settings.
fn settings<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut NavMeshAgent) -> R) -> Result<R> {
    with_world(lua, |w| {
        let mut a = w
            .scene
            .world
            .get::<&mut NavMeshAgent>(e)
            .map_err(|_| missing())?;
        Ok(f(&mut a))
    })?
}

macro_rules! setting {
    ($f:ident, $lua:literal, $field:ident, $ty:ty) => {
        $f.add_field_method_get($lua, |lua, a| settings(lua, a.0, |c| c.$field));
        $f.add_field_method_set($lua, |lua, a, v: $ty| settings(lua, a.0, |c| c.$field = v));
    };
}

impl UserData for LNavMeshAgent {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
        f.add_field_method_get("transform", |_, a| Ok(LTransform(a.0)));
        setting!(f, "speed", speed, f32);
        setting!(f, "angularSpeed", angular_speed, f32);
        setting!(f, "acceleration", acceleration, f32);
        setting!(f, "stoppingDistance", stopping_distance, f32);
        setting!(f, "autoBraking", auto_braking, bool);
        setting!(f, "radius", radius, f32);
        setting!(f, "height", height, f32);
        setting!(f, "baseOffset", base_offset, f32);
        setting!(f, "avoidancePriority", priority, i32);
        setting!(f, "enabled", enabled, bool);
        f.add_field_method_get("destination", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.destination))?.unwrap_or(LVec3(glam::Vec3::ZERO)))
        });
        f.add_field_method_set("destination", |lua, a, v: LVec3| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.set_destination(scene, a.0, v.0);
            })
        });
        f.add_field_method_get("remainingDistance", |lua, a| {
            Ok(state(lua, a.0, |s| s.remaining_distance())?.unwrap_or(0.0))
        });
        f.add_field_method_get("hasPath", |lua, a| {
            Ok(state(lua, a.0, |s| s.has_path())?.unwrap_or(false))
        });
        // Paths are computed at once, never in the background.
        f.add_field_method_get("pathPending", |_, _| Ok(false));
        f.add_field_method_get("pathStatus", |lua, a| {
            Ok(state(lua, a.0, |s| s.status.name())?.unwrap_or("PathInvalid"))
        });
        f.add_field_method_get("isOnNavMesh", |lua, a| {
            Ok(state(lua, a.0, |s| s.on_mesh)?.unwrap_or(false))
        });
        f.add_field_method_get("isStopped", |lua, a| {
            Ok(state(lua, a.0, |s| s.is_stopped)?.unwrap_or(false))
        });
        f.add_field_method_set("isStopped", |lua, a, v: bool| {
            state(lua, a.0, |s| s.is_stopped = v).map(|_| ())
        });
        f.add_field_method_get("velocity", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.velocity))?.unwrap_or(LVec3(glam::Vec3::ZERO)))
        });
        f.add_field_method_set("velocity", |lua, a, v: LVec3| {
            state(lua, a.0, |s| {
                s.velocity = glam::Vec3::new(v.0.x, 0.0, v.0.z)
            })
            .map(|_| ())
        });
        f.add_field_method_get("nextPosition", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.position))?.unwrap_or(LVec3(glam::Vec3::ZERO)))
        });
        f.add_field_method_get("steeringTarget", |lua, a| {
            let t = state(lua, a.0, |s| {
                s.corners.first().copied().unwrap_or(s.position)
            })?;
            Ok(LVec3(t.unwrap_or_default()))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetDestination", |lua, a, v: LVec3| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.set_destination(scene, a.0, v.0)
            })
        });
        m.add_method("ResetPath", |lua, a, ()| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.reset_path(scene, a.0);
            })
        });
        m.add_method("Warp", |lua, a, v: LVec3| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.warp(scene, a.0, v.0)
            })
        });
    }
}

/// `GetComponent("NavMeshAgent")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "NavMeshAgent"
        || !with_world(lua, |w| w.scene.world.get::<&NavMeshAgent>(e).is_ok())?
    {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LNavMeshAgent(e))?,
    )))
}

/// Register the `NavMesh` global.
pub fn register(lua: &Lua) -> Result<()> {
    let nav = lua.create_table()?;
    nav.set("AllAreas", -1i64)?;
    // SamplePosition(position, maxDistance [, areaMask]) → { position, distance, hit } or nil.
    nav.set(
        "SamplePosition",
        lua.create_function(|lua, (p, max, _mask): (LVec3, f32, Option<i64>)| {
            let hit = with_world(lua, |w| {
                w.navigation.mesh().and_then(|m| m.nearest(p.0, max))
            })?;
            let Some(h) = hit else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("position", LVec3(h.position))?;
            t.set("distance", h.distance)?;
            t.set("hit", true)?;
            Ok(Value::Table(t))
        })?,
    )?;
    // CalculatePath(from, to [, areaMask]) → list of corners with a `status` field.
    nav.set(
        "CalculatePath",
        lua.create_function(|lua, (a, b, _mask): (LVec3, LVec3, Option<i64>)| {
            let path = with_world(lua, |w| w.navigation.calculate_path(a.0, b.0))?;
            let t: Table = lua.create_sequence_from(path.corners.iter().map(|c| LVec3(*c)))?;
            t.set("status", path.status.name())?;
            Ok(t)
        })?,
    )?;
    lua.globals().set("NavMesh", nav)
}

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3};

    use crate::navigation::NavMeshAgent;
    use crate::navigation::tests::floor;
    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Camera, Scene, ScriptRef, Scripts, Transform};
    use crate::scripting::{Input, LogKind, ScriptHost};

    const SCRIPT: &str = r#"
local T = {}
function T:Start()
    self.agent = self.gameObject:GetComponent("NavMeshAgent")
    local ok = self.agent:SetDestination(Vector3.new(3, 0, 2))
    local hit = NavMesh.SamplePosition(Vector3.new(1, 3, 1), 5)
    local miss = NavMesh.SamplePosition(Vector3.new(50, 0, 0), 1)
    local path = NavMesh.CalculatePath(Vector3.new(-3, 0, -3), Vector3.new(3, 0, 3))
    Debug.Log(tostring(ok), tostring(self.agent.hasPath), tostring(self.agent.pathPending),
        tostring(self.agent.remainingDistance > 5), hit.position.y, tostring(miss == nil),
        #path, path.status, self.agent.speed)
    self.frames = 0
end
function T:Update()
    self.frames = self.frames + 1
    if self.frames == 240 then
        Debug.Log("arrived", tostring(self.agent.hasPath), self.agent.remainingDistance,
            math.floor(self.transform.position.x + 0.5), self.transform.position.y)
        self.agent:Warp(Vector3.new(-2, 0, -2))
        self.agent.isStopped = true
        self.agent:SetDestination(Vector3.new(2, 0, 2))
    elseif self.frames == 300 then
        local p = self.transform.position
        Debug.Log("stopped", tostring(self.agent.isStopped), p.x, p.z,
            tostring(self.agent.hasPath))
        self.agent:ResetPath()
        Debug.Log(tostring(self.agent.hasPath))
    end
end
return T
"#;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_{tag}_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        root
    }

    fn texts(host: &ScriptHost) -> Vec<String> {
        let logs = host.take_logs();
        assert!(logs.iter().all(|(k, _)| *k != LogKind::Error), "{logs:?}");
        logs.into_iter().map(|(_, t)| t).collect()
    }

    fn with_script(scene: &mut Scene, e: hecs::Entity) {
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/T.lua".into(),
                    ..Default::default()
                }],
            },
        );
    }

    #[test]
    fn scripts_drive_agents_and_query_the_nav_mesh() {
        let root = temp_root("nav_script");
        std::fs::write(root.join("Assets/Scripts/T.lua"), SCRIPT).unwrap();
        let mut scene = Scene::new("T");
        floor(&mut scene, 0.0, 0.0, 10.0, 10.0);
        let rel = "Assets/Scenes/T/NavMesh.navmesh";
        crate::navigation::bake::bake_scene(&scene)
            .save(&root.join(rel))
            .unwrap();
        scene.navigation.nav_mesh = Some(rel.into());
        let e = scene.spawn("Agent", None);
        scene.set_transform(e, Transform::from_position(Vec3::new(-3.0, 1.0, -3.0)));
        scene.add(
            e,
            NavMeshAgent {
                base_offset: 1.0,
                ..Default::default()
            },
        );
        with_script(&mut scene, e);
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root.clone());
        let mut time = Time::default();
        let mut all = Vec::new();
        for _ in 0..300 {
            time.advance(1.0 / 60.0);
            host.update(&mut scene, &time, Input::default());
            all.extend(texts(&host));
        }
        assert_eq!(
            all,
            [
                "true true false true 0 true 2 PathComplete 3.5",
                "arrived false 0 3 1",
                "stopped true -2 -2 true",
                "false",
            ],
            "{all:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn camera_rays_from_the_mouse_position() {
        let root = temp_root("nav_camera");
        let src = r#"
local T = {}
function T:Update()
    local m = Input.mousePosition
    local ray = Camera.main:ScreenPointToRay(m)
    local s = Camera.main:WorldToScreenPoint(Vector3.new(0, 0, 0))
    Debug.Log(m.x, m.y, math.floor(ray.direction.y * 100 + 0.5) / 100, tostring(ray.origin.y > 9),
        math.floor(s.x + 0.5), math.floor(s.y + 0.5))
end
return T
"#;
        std::fs::write(root.join("Assets/Scripts/T.lua"), src).unwrap();
        let mut scene = Scene::new("T");
        let cam = scene.spawn("Main Camera", None);
        scene.game_object_mut(cam).tag = "MainCamera".into();
        scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(90.0, 0.0, 0.0)),
        );
        scene.add(cam, Camera::default());
        with_script(&mut scene, cam);
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root.clone());
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        let input = Input {
            mouse_position: Some(Vec2::new(100.0, 30.0)),
            screen_size: Vec2::new(200.0, 100.0),
            ..Default::default()
        };
        host.update(&mut scene, &time, input);
        // Pixels from the bottom-left; the ray through (100, 70) tilts a little off straight down.
        let out = texts(&host);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].starts_with("100 70 -0.9"), "{out:?}");
        assert!(out[0].ends_with("true 100 50"), "{out:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
