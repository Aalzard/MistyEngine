//! Lua SplineContainer and SplineAnimate.
//!
//! SplineContainer: `EvaluatePosition(t [, spline])`, `EvaluateTangent`, `EvaluateUpVector`
//! (world space; spline index from 1, default the first), `EvaluateCurvature`,
//! `CalculateLength([spline])`, `GetNearestPoint(position [, spline])` (point and t),
//! `splineCount`, `KnotCount([spline])`, `GetKnotPosition(index [, spline])`, `closed`.
//! SplineAnimate: `Play()`, `Pause()`, `Restart()` (and lower-case `play`/`pause`/`restart`),
//! `normalizedTime` and `elapsedTime` (settable), `isPlaying`, `duration`, `speed`, `length`.

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::splines::{SplineAnimate, SplineContainer, SplinePath, animate};

#[derive(Clone, Copy)]
pub struct LSplineContainer(pub Entity);

#[derive(Clone, Copy)]
pub struct LSplineAnimate(pub Entity);

fn gone() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

/// Spline index from an optional 1-based Lua argument.
fn index(i: Option<i64>) -> usize {
    i.unwrap_or(1).max(1) as usize - 1
}

/// World matrix and path of one spline of the container on `e`.
fn path(lua: &Lua, e: Entity, i: Option<i64>) -> Result<(SplinePath, glam::Mat4)> {
    with_world(lua, |w| {
        let c = w
            .scene
            .world
            .get::<&SplineContainer>(e)
            .map_err(|_| gone())?;
        let i = index(i);
        if i >= c.splines.len() {
            return Err(mlua::Error::runtime(format!(
                "spline {} out of range (the container has {})",
                i + 1,
                c.splines.len()
            )));
        }
        Ok((c.path(i), w.scene.world_matrix(e)))
    })?
}

impl UserData for LSplineContainer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("splineCount", |lua, c| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&SplineContainer>(c.0)
                    .map(|s| s.splines.len())
                    .map_err(|_| gone())
            })?
        });
        f.add_field_method_get("closed", |lua, c| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&SplineContainer>(c.0)
                    .map(|s| s.splines.first().is_some_and(|s| s.closed))
                    .map_err(|_| gone())
            })?
        });
        f.add_field_method_get("length", |lua, c| world_length(lua, c.0, None));
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("EvaluatePosition", |lua, c, (t, i): (f32, Option<i64>)| {
            let (p, mat) = path(lua, c.0, i)?;
            Ok(LVec3(mat.transform_point3(p.position(t))))
        });
        m.add_method("EvaluateTangent", |lua, c, (t, i): (f32, Option<i64>)| {
            let (p, mat) = path(lua, c.0, i)?;
            Ok(LVec3(mat.transform_vector3(p.derivative(t))))
        });
        m.add_method("EvaluateUpVector", |lua, c, (t, i): (f32, Option<i64>)| {
            let (p, mat) = path(lua, c.0, i)?;
            Ok(LVec3(mat.transform_vector3(p.up(t)).normalize_or_zero()))
        });
        m.add_method("EvaluateCurvature", |lua, c, (t, i): (f32, Option<i64>)| {
            let (p, _) = path(lua, c.0, i)?;
            Ok(p.curvature(t))
        });
        m.add_method("CalculateLength", |lua, c, i: Option<i64>| {
            world_length(lua, c.0, i)
        });
        m.add_method("GetLength", |lua, c, i: Option<i64>| {
            world_length(lua, c.0, i)
        });
        m.add_method(
            "GetNearestPoint",
            |lua, c, (at, i): (LVec3, Option<i64>)| {
                let (p, mat) = path(lua, c.0, i)?;
                let local = mat.inverse().transform_point3(at.0);
                let n = p.nearest(local);
                Ok((LVec3(mat.transform_point3(n.point)), n.t))
            },
        );
        m.add_method("KnotCount", |lua, c, i: Option<i64>| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&SplineContainer>(c.0)
                    .map(|s| s.splines.get(index(i)).map_or(0, |s| s.knots.len()))
                    .map_err(|_| gone())
            })?
        });
        m.add_method("GetKnotPosition", |lua, c, (k, i): (i64, Option<i64>)| {
            with_world(lua, |w| {
                let s = w
                    .scene
                    .world
                    .get::<&SplineContainer>(c.0)
                    .map_err(|_| gone())?;
                let knot = s
                    .splines
                    .get(index(i))
                    .and_then(|s| s.knots.get((k.max(1) - 1) as usize))
                    .ok_or_else(|| mlua::Error::runtime("knot index out of range"))?;
                Ok(LVec3(
                    w.scene.world_matrix(c.0).transform_point3(knot.position),
                ))
            })?
        });
    }
}

/// World-space length (scaled by the object's transform).
fn world_length(lua: &Lua, e: Entity, i: Option<i64>) -> Result<f32> {
    let (p, mat) = path(lua, e, i)?;
    let pts = p.polyline(32);
    Ok(pts
        .windows(2)
        .map(|w| {
            mat.transform_point3(w[0])
                .distance(mat.transform_point3(w[1]))
        })
        .sum())
}

fn read<R>(lua: &Lua, e: Entity, f: impl FnOnce(&SplineAnimate, f32) -> R) -> Result<R> {
    with_world(lua, |w| {
        let len = animate::length(&w.scene, e);
        w.scene
            .world
            .get::<&SplineAnimate>(e)
            .map(|a| f(&a, len))
            .map_err(|_| gone())
    })?
}

/// Change the component, then put the object where it now belongs.
fn write(lua: &Lua, e: Entity, f: impl FnOnce(&mut SplineAnimate, f32)) -> Result<()> {
    with_world(lua, |w| {
        let len = animate::length(&w.scene, e);
        w.scene
            .world
            .get::<&mut SplineAnimate>(e)
            .map(|mut a| f(&mut a, len))
            .map_err(|_| gone())?;
        animate::apply(&mut w.scene, e);
        Ok(())
    })?
}

fn play(a: &mut SplineAnimate, len: f32) {
    a.started = true;
    // Playing a finished Once animation starts it over.
    let finished = animate::normalized_time(a, animate::duration(a, len)) >= 1.0;
    if a.loop_mode == crate::splines::components::LoopMode::Once && !a.playing && finished {
        a.elapsed = 0.0;
    }
    a.playing = true;
}

impl UserData for LSplineAnimate {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("normalizedTime", |lua, c| {
            read(lua, c.0, |a, len| {
                animate::normalized_time(a, animate::duration(a, len))
            })
        });
        f.add_field_method_set("normalizedTime", |lua, c, v: f32| {
            write(lua, c.0, |a, len| {
                let d = animate::duration(a, len);
                animate::set_normalized_time(a, v, d);
            })
        });
        f.add_field_method_get("elapsedTime", |lua, c| read(lua, c.0, |a, _| a.elapsed));
        f.add_field_method_set("elapsedTime", |lua, c, v: f32| {
            write(lua, c.0, |a, _| a.elapsed = v)
        });
        f.add_field_method_get("isPlaying", |lua, c| read(lua, c.0, |a, _| a.playing));
        f.add_field_method_get("duration", |lua, c| read(lua, c.0, animate::duration));
        f.add_field_method_set("duration", |lua, c, v: f32| {
            write(lua, c.0, |a, _| a.duration = v.max(1e-3))
        });
        f.add_field_method_get("speed", |lua, c| read(lua, c.0, |a, _| a.speed));
        f.add_field_method_set("speed", |lua, c, v: f32| {
            write(lua, c.0, |a, _| a.speed = v.max(1e-3))
        });
        f.add_field_method_get("length", |lua, c| read(lua, c.0, |_, len| len));
        f.add_field_method_get("enabled", |lua, c| read(lua, c.0, |a, _| a.enabled));
        f.add_field_method_set("enabled", |lua, c, v: bool| {
            write(lua, c.0, |a, _| a.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        for name in ["Play", "play"] {
            m.add_method(name, |lua, c, ()| write(lua, c.0, play));
        }
        for name in ["Pause", "pause"] {
            m.add_method(name, |lua, c, ()| {
                write(lua, c.0, |a, _| {
                    a.started = true;
                    a.playing = false;
                })
            });
        }
        for name in ["Restart", "restart"] {
            m.add_method(name, |lua, c, auto: Option<bool>| {
                write(lua, c.0, |a, _| {
                    a.started = true;
                    a.elapsed = 0.0;
                    a.playing = auto.unwrap_or(true);
                })
            });
        }
    }
}

/// `GetComponent("SplineContainer")` / `GetComponent("SplineAnimate")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = with_world(lua, |w| match name {
        "SplineContainer" => w.scene.world.get::<&SplineContainer>(e).is_ok(),
        "SplineAnimate" => w.scene.world.get::<&SplineAnimate>(e).is_ok(),
        _ => false,
    })?;
    if !has {
        return Ok(None);
    }
    let ud = match name {
        "SplineContainer" => lua.create_userdata(LSplineContainer(e))?,
        _ => lua.create_userdata(LSplineAnimate(e))?,
    };
    Ok(Some(Value::UserData(ud)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::Vec3;

    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Scene, ScriptRef, Scripts};
    use crate::scripting::{Input, LogKind, ScriptHost};
    use crate::splines::{Knot, Spline, SplineAnimate, SplineContainer};

    const RIDER: &str = r#"
local Rider = {}
function Rider:Start()
    self.road = GameObject.Find("Road"):GetComponent("SplineContainer")
    self.anim = self.gameObject:GetComponent("SplineAnimate")
    local p = self.road:EvaluatePosition(0.5)
    local tan = self.road:EvaluateTangent(0.5)
    local up = self.road:EvaluateUpVector(0.5)
    local near, t = self.road:GetNearestPoint(Vector3.new(3, 1, 7))
    Debug.Log(string.format("mid=%.2f,%.2f,%.2f tan=%.2f up=%.2f len=%.2f n=%d near=%.2f t=%.2f",
        p.x, p.y, p.z, tan.normalized.z, up.y, self.road:CalculateLength(), self.road:KnotCount(),
        near.z, t))
    self.anim:Pause()
    self.anim.normalizedTime = 0.25
end
function Rider:Update()
    Debug.Log(string.format("playing=%s time=%.2f z=%.2f", tostring(self.anim.isPlaying),
        self.anim.normalizedTime, self.transform.position.z))
    if self.anim.normalizedTime < 0.3 then self.anim:play() end
end
return Rider
"#;

    fn project() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_spline_script_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(root.join("Assets/Scripts/Rider.lua"), RIDER).unwrap();
        root
    }

    #[test]
    fn scripts_evaluate_splines_and_drive_spline_animate() {
        let mut scene = Scene::new("t");
        let road = scene.spawn("Road", None);
        scene.add(
            road,
            SplineContainer::with(Spline::new(
                vec![Knot::at(Vec3::ZERO), Knot::at(Vec3::Z * 10.0)],
                false,
            )),
        );
        let rider = scene.spawn("Rider", None);
        scene.add(
            rider,
            SplineAnimate {
                container: Some(road),
                duration: 10.0,
                ..Default::default()
            },
        );
        scene.add(
            rider,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Rider.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, project());
        let mut time = Time::default();
        for _ in 0..3 {
            time.advance(0.5);
            host.update(&mut scene, &time, Input::default());
        }
        let logs = host.take_logs();
        for (kind, text) in &logs {
            assert_ne!(*kind, LogKind::Error, "{text}");
        }
        let first = &logs[0].1;
        assert!(first.contains("mid=0.00,0.00,5.00"), "{first}");
        assert!(first.contains("tan=1.00 up=1.00 len=10.00 n=2"), "{first}");
        assert!(first.contains("near=7.00 t=0.70"), "{first}");
        // Paused at a quarter, then played from there.
        assert!(
            logs[1].1.contains("playing=false time=0.25 z=2.50"),
            "{}",
            logs[1].1
        );
        let last = &logs.last().unwrap().1;
        assert!(last.contains("playing=true"), "{last}");
        let z = scene.world_position(rider).z;
        // Two half-second steps after Play (the first frame has no delta).
        assert!((z - 3.5).abs() < 0.05, "{z}");
    }
}
