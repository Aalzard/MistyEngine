//! The Lua scripting API: Vector3, Transform, GameObject, Rigidbody, AudioSource, Animation
//! (ParticleSystem is in `particles_api`, UI components in `ui_api`, queries, LayerMask and the
//! CharacterController in `physics_api`) and global tables
//! (Time, Input, Cursor, Debug, Mathf, ForceMode, GameObject.Find, Instantiate, Destroy,
//! SceneManager, Application).
//!
//! Userdata only hold an entity id; they reach the scene through the [`World`] stored in the
//! Lua state's app data while a game frame runs.

use glam::Vec3;
use hecs::Entity;
use mlua::{
    FromLua, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
    Variadic,
};

use super::particles_api::LParticleSystem;
use super::{LogKind, Request, World};
use crate::camera::{euler_to_quat, quat_to_euler};
use crate::scene::prefab::Prefab;
use crate::scene::{Animation, AudioSource, ParticleSystem, Rigidbody};

/// Run `f` with the live game world, or fail if no frame is running.
pub fn with_world<R>(lua: &Lua, f: impl FnOnce(&mut World) -> R) -> Result<R> {
    let mut world = lua
        .app_data_mut::<World>()
        .ok_or_else(|| mlua::Error::runtime("the game is not running"))?;
    Ok(f(&mut world))
}

pub(super) fn missing() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

// ---- Vector3 ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LVec3(pub Vec3);

impl FromLua for LVec3 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => Ok(*ud.borrow::<LVec3>()?),
            Value::Table(t) => Ok(LVec3(Vec3::new(t.get("x")?, t.get("y")?, t.get("z")?))),
            other => Err(mlua::Error::runtime(format!(
                "expected Vector3, got {}",
                other.type_name()
            ))),
        }
    }
}

fn number(v: &Value) -> Option<f32> {
    match v {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(n) => Some(*n as f32),
        _ => None,
    }
}

/// Accept either one Vector3 or three numbers.
pub fn vec_args(lua: &Lua, args: &[Value]) -> Result<Vec3> {
    match args {
        [a, b, c, ..] if number(a).is_some() => Ok(Vec3::new(
            number(a).unwrap_or(0.0),
            number(b).unwrap_or(0.0),
            number(c).unwrap_or(0.0),
        )),
        [v, ..] => Ok(LVec3::from_lua(v.clone(), lua)?.0),
        [] => Err(mlua::Error::runtime("expected a Vector3 or x, y, z")),
    }
}

impl UserData for LVec3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("x", |_, v| Ok(v.0.x));
        f.add_field_method_get("y", |_, v| Ok(v.0.y));
        f.add_field_method_get("z", |_, v| Ok(v.0.z));
        f.add_field_method_set("x", |_, v, x: f32| {
            v.0.x = x;
            Ok(())
        });
        f.add_field_method_set("y", |_, v, y: f32| {
            v.0.y = y;
            Ok(())
        });
        f.add_field_method_set("z", |_, v, z: f32| {
            v.0.z = z;
            Ok(())
        });
        f.add_field_method_get("magnitude", |_, v| Ok(v.0.length()));
        f.add_field_method_get("sqrMagnitude", |_, v| Ok(v.0.length_squared()));
        f.add_field_method_get("normalized", |_, v| Ok(LVec3(v.0.normalize_or_zero())));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (LVec3, LVec3)| {
            Ok(LVec3(a.0 + b.0))
        });
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (LVec3, LVec3)| {
            Ok(LVec3(a.0 - b.0))
        });
        m.add_meta_function(MetaMethod::Unm, |_, a: LVec3| Ok(LVec3(-a.0)));
        m.add_meta_function(MetaMethod::Mul, |lua, (a, b): (Value, Value)| {
            match (number(&a), number(&b)) {
                (Some(k), None) => Ok(LVec3(LVec3::from_lua(b, lua)?.0 * k)),
                (None, Some(k)) => Ok(LVec3(LVec3::from_lua(a, lua)?.0 * k)),
                _ => Ok(LVec3(
                    LVec3::from_lua(a, lua)?.0 * LVec3::from_lua(b, lua)?.0,
                )),
            }
        });
        m.add_meta_function(MetaMethod::Div, |_, (a, k): (LVec3, f32)| {
            Ok(LVec3(a.0 / k))
        });
        m.add_meta_function(MetaMethod::Eq, |_, (a, b): (LVec3, LVec3)| Ok(a.0 == b.0));
        m.add_meta_method(MetaMethod::ToString, |_, v, ()| {
            Ok(format!("({:.2}, {:.2}, {:.2})", v.0.x, v.0.y, v.0.z))
        });
    }
}

// ---- Transform -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LTransform(pub Entity);

/// After a script moves an object with a Rigidbody, move its body too.
pub(crate) fn sync_body(w: &mut World, e: Entity) {
    if w.physics.has_body(e) {
        let (_, rot, pos) = w.scene.world_matrix(e).to_scale_rotation_translation();
        w.physics.teleport(e, pos, rot);
    }
    if w.physics2d.has_body(e) {
        let (_, rot, pos) = w.scene.world_matrix(e).to_scale_rotation_translation();
        w.physics2d.teleport(e, pos, rot);
    }
}

fn alive(w: &World, e: Entity) -> Result<()> {
    if w.scene.contains(e) {
        Ok(())
    } else {
        Err(missing())
    }
}

impl UserData for LTransform {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("position", |lua, t| {
            with_world(lua, |w| {
                alive(w, t.0).map(|_| LVec3(w.scene.world_position(t.0)))
            })?
        });
        f.add_field_method_set("position", |lua, t, v: LVec3| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                w.scene.set_world_position(t.0, v.0);
                sync_body(w, t.0);
                Ok(())
            })?
        });
        f.add_field_method_get("localPosition", |lua, t| {
            with_world(lua, |w| {
                alive(w, t.0).map(|_| LVec3(w.scene.transform(t.0).position))
            })?
        });
        f.add_field_method_set("localPosition", |lua, t, v: LVec3| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                let mut tr = w.scene.transform(t.0);
                tr.position = v.0;
                w.scene.set_transform(t.0, tr);
                sync_body(w, t.0);
                Ok(())
            })?
        });
        f.add_field_method_get("eulerAngles", |lua, t| {
            with_world(lua, |w| {
                alive(w, t.0).map(|_| LVec3(quat_to_euler(w.scene.world_rotation(t.0))))
            })?
        });
        f.add_field_method_set("eulerAngles", |lua, t, v: LVec3| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                w.scene.set_world_rotation(t.0, euler_to_quat(v.0));
                sync_body(w, t.0);
                Ok(())
            })?
        });
        f.add_field_method_get("localEulerAngles", |lua, t| {
            with_world(lua, |w| {
                alive(w, t.0).map(|_| LVec3(w.scene.transform(t.0).euler()))
            })?
        });
        f.add_field_method_set("localEulerAngles", |lua, t, v: LVec3| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                let mut tr = w.scene.transform(t.0);
                tr.set_euler(v.0);
                w.scene.set_transform(t.0, tr);
                sync_body(w, t.0);
                Ok(())
            })?
        });
        f.add_field_method_get("localScale", |lua, t| {
            with_world(lua, |w| {
                alive(w, t.0).map(|_| LVec3(w.scene.transform(t.0).scale))
            })?
        });
        f.add_field_method_set("localScale", |lua, t, v: LVec3| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                let mut tr = w.scene.transform(t.0);
                tr.scale = v.0;
                w.scene.set_transform(t.0, tr);
                Ok(())
            })?
        });
        for (name, axis) in [("forward", Vec3::Z), ("right", Vec3::X), ("up", Vec3::Y)] {
            f.add_field_method_get(name, move |lua, t| {
                with_world(lua, |w| {
                    alive(w, t.0).map(|_| LVec3(w.scene.world_rotation(t.0) * axis))
                })?
            });
        }
        f.add_field_method_get("parent", |lua, t| {
            with_world(lua, |w| {
                w.scene
                    .contains(t.0)
                    .then(|| w.scene.parent(t.0).map(LTransform))
                    .flatten()
            })
        });
        f.add_field_method_get("gameObject", |_, t| Ok(LGameObject(t.0)));
        f.add_field_method_get("name", |lua, t| {
            with_world(lua, |w| alive(w, t.0).map(|_| w.scene.name(t.0)))?
        });
        f.add_field_method_get("childCount", |lua, t| {
            with_world(lua, |w| alive(w, t.0).map(|_| w.scene.children(t.0).len()))?
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // GetChild(i): the i-th child (0-based, Hierarchy order), or an error past the end.
        m.add_method("GetChild", |lua, t, i: usize| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                w.scene
                    .children(t.0)
                    .get(i)
                    .map(|c| LTransform(*c))
                    .ok_or_else(|| {
                        mlua::Error::runtime(format!("Transform.GetChild: no child {i}"))
                    })
            })?
        });
        // Find("Head/Camera"): a child by name, or a descendant by a slash-separated path;
        // nil when missing.
        m.add_method("Find", |lua, t, path: String| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                let mut cur = t.0;
                for part in path.split('/').filter(|p| !p.is_empty()) {
                    let next = w
                        .scene
                        .children(cur)
                        .into_iter()
                        .find(|c| w.scene.name(*c) == part);
                    match next {
                        Some(c) => cur = c,
                        None => return Ok(None),
                    }
                }
                Ok((cur != t.0).then_some(LTransform(cur)))
            })?
        });
        // Translate in the object's own axes (Space.Self).
        m.add_method("Translate", |lua, t, args: Variadic<Value>| {
            let v = vec_args(lua, &args)?;
            with_world(lua, |w| {
                alive(w, t.0)?;
                let rot = w.scene.world_rotation(t.0);
                let p = w.scene.world_position(t.0) + rot * v;
                w.scene.set_world_position(t.0, p);
                sync_body(w, t.0);
                Ok(())
            })?
        });
        // Rotate by Euler degrees around the object's own axes.
        m.add_method("Rotate", |lua, t, args: Variadic<Value>| {
            let v = vec_args(lua, &args)?;
            with_world(lua, |w| {
                alive(w, t.0)?;
                let mut tr = w.scene.transform(t.0);
                tr.set_rotation(tr.rotation * euler_to_quat(v));
                w.scene.set_transform(t.0, tr);
                sync_body(w, t.0);
                Ok(())
            })?
        });
        m.add_method("LookAt", |lua, t, target: Value| {
            let point = match &target {
                Value::UserData(ud) if ud.is::<LTransform>() => {
                    let other = ud.borrow::<LTransform>()?.0;
                    with_world(lua, |w| w.scene.world_position(other))?
                }
                _ => LVec3::from_lua(target, lua)?.0,
            };
            with_world(lua, |w| {
                alive(w, t.0)?;
                let from = w.scene.world_position(t.0);
                let dir = (point - from).normalize_or_zero();
                if dir != Vec3::ZERO {
                    let up = if dir.y.abs() > 0.999 {
                        Vec3::Z
                    } else {
                        Vec3::Y
                    };
                    let q = glam::camera::lh::view::look_to_quat(dir, up).inverse();
                    w.scene.set_world_rotation(t.0, q);
                    sync_body(w, t.0);
                }
                Ok(())
            })?
        });
    }
}

// ---- GameObject ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LGameObject(pub Entity);

impl UserData for LGameObject {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("name", |lua, g| {
            with_world(lua, |w| alive(w, g.0).map(|_| w.scene.name(g.0)))?
        });
        f.add_field_method_set("name", |lua, g, name: String| {
            with_world(lua, |w| {
                alive(w, g.0)?;
                w.scene.game_object_mut(g.0).name = name;
                Ok(())
            })?
        });
        f.add_field_method_get("tag", |lua, g| {
            with_world(lua, |w| {
                alive(w, g.0).map(|_| w.scene.game_object(g.0).tag.clone())
            })?
        });
        f.add_field_method_get("activeSelf", |lua, g| {
            with_world(lua, |w| {
                alive(w, g.0).map(|_| w.scene.game_object(g.0).active)
            })?
        });
        f.add_field_method_get("transform", |_, g| Ok(LTransform(g.0)));
        // So `hit.collider.gameObject` works on query hits.
        f.add_field_method_get("gameObject", |_, g| Ok(LGameObject(g.0)));
        f.add_field_method_get("layer", |lua, g| {
            with_world(lua, |w| {
                alive(w, g.0).map(|_| w.scene.game_object(g.0).layer as i64)
            })?
        });
        f.add_field_method_set("layer", |lua, g, layer: u8| {
            with_world(lua, |w| {
                alive(w, g.0)?;
                let layer = layer.min(31);
                w.scene.game_object_mut(g.0).layer = layer;
                w.physics.set_layer(g.0, layer);
                w.physics2d.set_layer(g.0, layer);
                Ok(())
            })?
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetActive", |lua, g, on: bool| {
            with_world(lua, |w| {
                alive(w, g.0)?;
                w.scene.game_object_mut(g.0).active = on;
                Ok(())
            })?
        });
        // SendCustomEvent(name, ...): runs the Custom Event node `name` of the object's Script
        // Graphs (and the `name` function of its Lua scripts) with the arguments, after this
        // callback.
        m.add_method(
            "SendCustomEvent",
            |lua, g, (name, args): (String, Variadic<Value>)| {
                let args = args
                    .into_iter()
                    .map(|a| crate::visual_script::lua_bridge::from_lua(lua, a))
                    .collect();
                with_world(lua, |w| {
                    w.graph_events
                        .push(crate::visual_script::runtime::CustomEvent {
                            target: Some(g.0),
                            name: std::rc::Rc::from(name.as_str()),
                            args,
                        })
                })
            },
        );
        // GetVariable(name): an Object variable of the object (set by its Script Graphs or
        // SetVariable), or nil.
        m.add_method("GetVariable", |lua, g, name: String| {
            match with_world(lua, |w| w.graph_vars.get(g.0, &name))? {
                Some(v) => crate::visual_script::lua_bridge::to_lua(lua, &v),
                None => Ok(Value::Nil),
            }
        });
        // SetVariable(name, value): set an Object variable, shared with the object's Script
        // Graphs.
        m.add_method("SetVariable", |lua, g, (name, value): (String, Value)| {
            let v = crate::visual_script::lua_bridge::from_lua(lua, value);
            with_world(lua, |w| w.graph_vars.set(g.0, &name, v))
        });
        m.add_method("CompareTag", |lua, g, tag: String| {
            with_world(lua, |w| {
                alive(w, g.0).map(|_| w.scene.game_object(g.0).tag == tag)
            })?
        });
        m.add_method("GetComponent", |lua, g, name: String| {
            let e = g.0;
            let kind = with_world(lua, |w| {
                alive(w, e)?;
                Ok::<_, mlua::Error>(match name.as_str() {
                    "Transform" => Some(0),
                    "Rigidbody" if w.scene.world.get::<&Rigidbody>(e).is_ok() => Some(1),
                    "AudioSource" if w.scene.world.get::<&AudioSource>(e).is_ok() => Some(2),
                    "Animation" if w.scene.world.get::<&Animation>(e).is_ok() => Some(3),
                    "ParticleSystem" if w.scene.world.get::<&ParticleSystem>(e).is_ok() => Some(4),
                    "Animator" if w.scene.world.get::<&crate::scene::Animator>(e).is_ok() => {
                        Some(5)
                    }
                    _ => None,
                })
            })??;
            match kind {
                Some(0) => lua.create_userdata(LTransform(e)).map(Value::UserData),
                Some(1) => lua.create_userdata(LRigidbody(e)).map(Value::UserData),
                Some(2) => lua.create_userdata(LAudioSource(e)).map(Value::UserData),
                Some(3) => lua.create_userdata(LAnimation(e)).map(Value::UserData),
                Some(5) => super::animator_api::component(lua, e),
                Some(_) => lua.create_userdata(LParticleSystem(e)).map(Value::UserData),
                None => other_component(lua, e, &name),
            }
        });
        m.add_meta_method(MetaMethod::Eq, |_, a, b: LGameObjectArg| Ok(a.0 == b.0));
        m.add_meta_method(MetaMethod::ToString, |lua, g, ()| {
            with_world(lua, |w| {
                if w.scene.contains(g.0) {
                    w.scene.name(g.0)
                } else {
                    "null".into()
                }
            })
        });
    }
}

/// `GetComponent` for the components of the other API modules, then for a script by its name
/// (`GetComponent("Rotator")` is that object's `Rotator.lua` instance table); nil otherwise.
fn other_component(lua: &Lua, e: Entity, name: &str) -> Result<Value> {
    type Lookup = fn(&Lua, Entity, &str) -> Result<Option<Value>>;
    const LOOKUPS: [Lookup; 12] = [
        super::physics_api::component,
        super::ui_api::component,
        super::material_api::component,
        super::terrain_api::component,
        super::navigation_api::component,
        super::physics2d_api::component,
        super::rigging_api::component,
        super::vcam_api::component,
        super::timeline_api::component,
        super::camera_api::component,
        super::network_api::component,
        super::input_actions_api::component,
    ];
    for lookup in LOOKUPS {
        if let Some(v) = lookup(lua, e, name)? {
            return Ok(v);
        }
    }
    Ok(super::script_instance(lua, e, name).map_or(Value::Nil, Value::Table))
}

/// Helper to compare GameObjects with `==`.
pub struct LGameObjectArg(Entity);

impl FromLua for LGameObjectArg {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => Ok(LGameObjectArg(ud.borrow::<LGameObject>()?.0)),
            _ => Err(mlua::Error::runtime("expected GameObject")),
        }
    }
}

// ---- Rigidbody --------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LRigidbody(pub Entity);

/// Force modes as a velocity change for one fixed step (mass from the Rigidbody component,
/// so forces work before the physics engine has computed mass properties).
pub(crate) fn delta_velocity(w: &World, e: Entity, v: Vec3, mode: &str) -> Vec3 {
    let dt = w.physics.fixed_dt;
    let mass = w
        .scene
        .world
        .get::<&Rigidbody>(e)
        .map(|r| r.mass.max(1e-4))
        .unwrap_or(1.0);
    match mode {
        "Impulse" => v / mass,
        "VelocityChange" => v,
        "Acceleration" => v * dt,
        _ => v * dt / mass, // Force
    }
}

fn force_args(lua: &Lua, args: &Variadic<Value>) -> Result<(Vec3, String)> {
    let mode = match args.last() {
        Some(Value::String(s)) => s.to_str()?.to_owned(),
        _ => "Force".to_owned(),
    };
    Ok((vec_args(lua, args)?, mode))
}

fn rigidbody_get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&Rigidbody) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&Rigidbody>(e)
            .map(|b| f(&b))
            .map_err(|_| missing())
    })?
}

impl UserData for LRigidbody {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("velocity", |lua, r| {
            with_world(lua, |w| LVec3(w.physics.velocity(r.0).unwrap_or_default()))
        });
        f.add_field_method_set("velocity", |lua, r, v: LVec3| {
            with_world(lua, |w| w.physics.set_velocity(r.0, v.0))
        });
        f.add_field_method_get("angularVelocity", |lua, r| {
            with_world(lua, |w| {
                LVec3(w.physics.angular_velocity(r.0).unwrap_or_default())
            })
        });
        f.add_field_method_set("angularVelocity", |lua, r, v: LVec3| {
            with_world(lua, |w| w.physics.set_angular_velocity(r.0, v.0))
        });
        f.add_field_method_get("mass", |lua, r| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&Rigidbody>(r.0)
                    .map(|b| b.mass)
                    .unwrap_or(0.0)
            })
        });
        f.add_field_method_set("mass", |lua, r, mass: f32| {
            with_world(lua, |w| {
                let mass = mass.max(1e-4);
                if let Ok(mut b) = w.scene.world.get::<&mut Rigidbody>(r.0) {
                    b.mass = mass;
                }
                w.physics.set_mass(r.0, mass);
            })
        });
        f.add_field_method_get("gameObject", |_, r| Ok(LGameObject(r.0)));
        // Pose in the physics world (the Transform may be interpolated).
        f.add_field_method_get("position", |lua, r| {
            with_world(lua, |w| match w.physics.body_pose(r.0) {
                Some((p, _)) => LVec3(p),
                None => LVec3(w.scene.world_position(r.0)),
            })
        });
        f.add_field_method_get("isKinematic", |lua, r| {
            rigidbody_get(lua, r.0, |b| b.is_kinematic)
        });
        f.add_field_method_set("isKinematic", |lua, r, on: bool| {
            with_world(lua, |w| {
                if let Ok(mut b) = w.scene.world.get::<&mut Rigidbody>(r.0) {
                    b.is_kinematic = on;
                }
                w.physics.set_kinematic(r.0, on);
            })
        });
        f.add_field_method_get("useGravity", |lua, r| {
            rigidbody_get(lua, r.0, |b| b.use_gravity)
        });
        f.add_field_method_set("useGravity", |lua, r, on: bool| {
            with_world(lua, |w| {
                if let Ok(mut b) = w.scene.world.get::<&mut Rigidbody>(r.0) {
                    b.use_gravity = on;
                }
                w.physics.set_use_gravity(r.0, on);
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // MovePosition(v): kinematic bodies sweep there in the next step, pushing others.
        m.add_method("MovePosition", |lua, r, args: Variadic<Value>| {
            let p = vec_args(lua, &args)?;
            with_world(lua, |w| {
                let rot = w.scene.world_rotation(r.0);
                let World { physics, scene, .. } = w;
                physics.move_body(scene, r.0, p, rot);
            })
        });
        // MoveRotation(euler): Euler angles in degrees (there is no Quaternion type).
        m.add_method("MoveRotation", |lua, r, args: Variadic<Value>| {
            let euler = vec_args(lua, &args)?;
            with_world(lua, |w| {
                let pos = w.scene.world_position(r.0);
                let World { physics, scene, .. } = w;
                physics.move_body(scene, r.0, pos, euler_to_quat(euler));
            })
        });
        m.add_method("AddForce", |lua, r, args: Variadic<Value>| {
            let (v, mode) = force_args(lua, &args)?;
            with_world(lua, |w| {
                let dv = delta_velocity(w, r.0, v, &mode);
                let vel = w.physics.velocity(r.0).unwrap_or_default();
                w.physics.set_velocity(r.0, vel + dv);
            })
        });
        m.add_method("AddTorque", |lua, r, args: Variadic<Value>| {
            let (v, mode) = force_args(lua, &args)?;
            with_world(lua, |w| {
                let dw = delta_velocity(w, r.0, v, &mode);
                let ang = w.physics.angular_velocity(r.0).unwrap_or_default();
                w.physics.set_angular_velocity(r.0, ang + dw);
            })
        });
    }
}

pub use super::audio_api::LAudioSource;

// ---- Animation --------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LAnimation(pub Entity);

impl UserData for LAnimation {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("isPlaying", |lua, a| {
            with_world(lua, |w| w.animation.is_playing(a.0))
        });
        f.add_field_method_get("time", |lua, a| with_world(lua, |w| w.animation.time(a.0)));
        f.add_field_method_set("time", |lua, a, t: f32| {
            with_world(lua, |w| w.animation.set_time(a.0, t))
        });
        f.add_field_method_get("speed", |lua, a| {
            with_world(lua, |w| w.animation.speed(a.0))
        });
        f.add_field_method_set("speed", |lua, a, v: f32| {
            with_world(lua, |w| w.animation.set_speed(a.0, v))
        });
        f.add_field_method_get("clip", |lua, a| {
            with_world(lua, |w| w.animation.current_clip(a.0))
        });
        f.add_field_method_get("enabled", |lua, a| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&Animation>(a.0)
                    .map(|c| c.enabled)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_set("enabled", |lua, a, v: bool| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&mut Animation>(a.0)
                    .map(|mut c| c.enabled = v)
                    .map_err(|_| missing())
            })?
        });
        f.add_field_method_get("gameObject", |_, a| Ok(LGameObject(a.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Play([name]): the default clip, or the clip with that file name. Returns false if missing.
        m.add_method("Play", |lua, a, name: Option<String>| {
            with_world(lua, |w| {
                let World {
                    animation, scene, ..
                } = w;
                animation.play(scene, a.0, name.as_deref())
            })
        });
        m.add_method("Stop", |lua, a, ()| {
            with_world(lua, |w| w.animation.stop(a.0))
        });
        m.add_method("IsPlaying", |lua, a, name: String| {
            with_world(lua, |w| {
                w.animation.is_playing(a.0) && w.animation.current_clip(a.0) == Some(name)
            })
        });
    }
}

// ---- globals ----------------------------------------------------------------------------------

fn log(lua: &Lua, kind: LogKind, args: Variadic<Value>) -> Result<()> {
    let text = args
        .iter()
        .map(|v| match v {
            Value::String(s) => s.to_string_lossy(),
            other => other.to_string().unwrap_or_else(|_| format!("{other:?}")),
        })
        .collect::<Vec<_>>()
        .join(" ");
    // Logging must never fail, even outside a running frame.
    let _ = with_world(lua, |w| w.logs.push((kind, text)));
    Ok(())
}

/// Register the global API tables in `lua`.
pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();

    // Vector3
    let v3 = lua.create_table()?;
    v3.set(
        "new",
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LVec3(Vec3::new(
                x.unwrap_or(0.0),
                y.unwrap_or(0.0),
                z.unwrap_or(0.0),
            )))
        })?,
    )?;
    for (name, v) in [
        ("zero", Vec3::ZERO),
        ("one", Vec3::ONE),
        ("up", Vec3::Y),
        ("down", Vec3::NEG_Y),
        ("right", Vec3::X),
        ("left", Vec3::NEG_X),
        ("forward", Vec3::Z),
        ("back", Vec3::NEG_Z),
    ] {
        v3.set(name, LVec3(v))?;
    }
    v3.set(
        "Distance",
        lua.create_function(|_, (a, b): (LVec3, LVec3)| Ok(a.0.distance(b.0)))?,
    )?;
    v3.set(
        "Dot",
        lua.create_function(|_, (a, b): (LVec3, LVec3)| Ok(a.0.dot(b.0)))?,
    )?;
    v3.set(
        "Cross",
        lua.create_function(|_, (a, b): (LVec3, LVec3)| Ok(LVec3(a.0.cross(b.0))))?,
    )?;
    v3.set(
        "Lerp",
        lua.create_function(|_, (a, b, t): (LVec3, LVec3, f32)| {
            Ok(LVec3(a.0.lerp(b.0, t.clamp(0.0, 1.0))))
        })?,
    )?;
    g.set("Vector3", v3)?;
    super::ui_api::register(lua)?;
    super::attributes::register(lua)?;
    super::network_api::register(lua)?;
    super::render_settings_api::register(lua)?;
    super::light_probes_api::register(lua)?;
    super::vcam_api::register(lua)?;
    super::app_api::register(lua)?;
    super::accessibility_api::register(lua)?;
    super::terrain_api::register(lua)?;
    super::physics_api::register(lua)?;
    super::physics2d_api::register(lua)?;
    super::audio_api::register(lua)?;
    super::water_api::register(lua)?;

    // Debug / print
    let debug = lua.create_table()?;
    debug.set(
        "Log",
        lua.create_function(|lua, a: Variadic<Value>| log(lua, LogKind::Info, a))?,
    )?;
    debug.set(
        "LogWarning",
        lua.create_function(|lua, a: Variadic<Value>| log(lua, LogKind::Warning, a))?,
    )?;
    debug.set(
        "LogError",
        lua.create_function(|lua, a: Variadic<Value>| log(lua, LogKind::Error, a))?,
    )?;
    g.set("Debug", debug)?;
    g.set(
        "print",
        lua.create_function(|lua, a: Variadic<Value>| log(lua, LogKind::Info, a))?,
    )?;

    // Mathf
    let mathf = lua.create_table()?;
    mathf.set("PI", std::f64::consts::PI)?;
    mathf.set("Infinity", f64::INFINITY)?;
    mathf.set("Deg2Rad", std::f64::consts::PI / 180.0)?;
    mathf.set("Rad2Deg", 180.0 / std::f64::consts::PI)?;
    mathf.set(
        "Clamp",
        lua.create_function(|_, (v, lo, hi): (f64, f64, f64)| Ok(v.clamp(lo.min(hi), hi.max(lo))))?,
    )?;
    mathf.set(
        "Clamp01",
        lua.create_function(|_, v: f64| Ok(v.clamp(0.0, 1.0)))?,
    )?;
    mathf.set(
        "Lerp",
        lua.create_function(|_, (a, b, t): (f64, f64, f64)| Ok(a + (b - a) * t.clamp(0.0, 1.0)))?,
    )?;
    mathf.set("Abs", lua.create_function(|_, v: f64| Ok(v.abs()))?)?;
    mathf.set("Sin", lua.create_function(|_, v: f64| Ok(v.sin()))?)?;
    mathf.set("Cos", lua.create_function(|_, v: f64| Ok(v.cos()))?)?;
    mathf.set("Sqrt", lua.create_function(|_, v: f64| Ok(v.sqrt()))?)?;
    mathf.set(
        "Min",
        lua.create_function(|_, (a, b): (f64, f64)| Ok(a.min(b)))?,
    )?;
    mathf.set(
        "Max",
        lua.create_function(|_, (a, b): (f64, f64)| Ok(a.max(b)))?,
    )?;
    g.set("Mathf", mathf)?;

    // ForceMode
    let fm = lua.create_table()?;
    for m in ["Force", "Impulse", "VelocityChange", "Acceleration"] {
        fm.set(m, m)?;
    }
    g.set("ForceMode", fm)?;

    // Time (values refreshed every frame by `refresh_time`)
    g.set("Time", lua.create_table()?)?;

    // Input
    let input = lua.create_table()?;
    input.set(
        "GetKey",
        lua.create_function(|lua, k: String| with_world(lua, |w| w.input.key(&k)))?,
    )?;
    input.set(
        "GetKeyDown",
        lua.create_function(|lua, k: String| with_world(lua, |w| w.input.key_down(&k)))?,
    )?;
    input.set(
        "GetKeyUp",
        lua.create_function(|lua, k: String| with_world(lua, |w| w.input.key_up(&k)))?,
    )?;
    input.set(
        "GetMouseButton",
        lua.create_function(|lua, b: usize| with_world(lua, |w| w.input.mouse(b)))?,
    )?;
    input.set(
        "GetMouseButtonDown",
        lua.create_function(|lua, b: usize| with_world(lua, |w| w.input.mouse_down(b)))?,
    )?;
    input.set(
        "GetMouseButtonUp",
        lua.create_function(|lua, b: usize| with_world(lua, |w| w.input.mouse_up(b)))?,
    )?;
    g.set("Input", input)?;
    super::camera_api::register(lua)?;
    super::cursor_api::register(lua)?;
    super::navigation_api::register(lua)?;
    super::save_api::register(lua)?;
    super::localization_api::register(lua)?;

    // GameObject.Find / Destroy
    let go = lua.create_table()?;
    go.set(
        "Find",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| w.scene.find_by_name(&name).map(LGameObject))
        })?,
    )?;
    g.set("GameObject", go)?;
    // Axes, buttons and tags (needs Input, its mousePosition lookup and GameObject).
    super::input_api::register(lua)?;
    super::input_actions_api::register(lua)?;
    g.set(
        "Instantiate",
        lua.create_function(|lua, args: Variadic<Value>| {
            let position = match args.get(1) {
                Some(v) if !v.is_nil() => Some(LVec3::from_lua(v.clone(), lua)?.0),
                _ => None,
            };
            let source = match args.first() {
                Some(Value::String(s)) => Source::Prefab(s.to_string_lossy()),
                Some(Value::UserData(ud)) if ud.is::<LGameObject>() => {
                    Source::Object(ud.borrow::<LGameObject>()?.0)
                }
                Some(Value::UserData(ud)) if ud.is::<LTransform>() => {
                    Source::Object(ud.borrow::<LTransform>()?.0)
                }
                _ => {
                    return Err(mlua::Error::runtime(
                        "Instantiate expects a prefab path or a GameObject",
                    ));
                }
            };
            with_world(lua, |w| instantiate(w, source, position))?.map(LGameObject)
        })?,
    )?;
    g.set(
        "Destroy",
        lua.create_function(|lua, target: Value| {
            let e = match &target {
                Value::UserData(ud) if ud.is::<LGameObject>() => ud.borrow::<LGameObject>()?.0,
                Value::UserData(ud) if ud.is::<LTransform>() => ud.borrow::<LTransform>()?.0,
                _ => return Err(mlua::Error::runtime("Destroy expects a GameObject")),
            };
            with_world(lua, |w| w.destroy.push(e))
        })?,
    )?;

    // SceneManager / Application: loads are requests handled after the frame.
    super::scene_api::register(lua, &g)?;
    let app = lua.create_table()?;
    app.set(
        "Quit",
        lua.create_function(|lua, ()| with_world(lua, |w| w.requests.push(Request::Quit)))?,
    )?;
    // Scripting defines of the Build Profile (`Application.HasDefine("DEMO")`).
    app.set(
        "HasDefine",
        lua.create_function(|_, name: String| Ok(crate::build_profile::has_define(&name)))?,
    )?;
    app.set(
        "isDedicatedServer",
        crate::build_profile::has_define("MISTY_SERVER"),
    )?;
    app.set("isEditor", crate::build_profile::has_define("MISTY_EDITOR"))?;
    g.set("Application", app)?;
    crate::achievements::register_lua(lua)?;
    super::assets_api::register(lua)?;
    Ok(())
}

/// `Instantiate(path [, position])` for other APIs (`Assets.InstantiateAsync`).
pub(super) fn instantiate_prefab_path(
    w: &mut World,
    path: &str,
    position: Option<Vec3>,
) -> Result<Entity> {
    instantiate(w, Source::Prefab(path.to_owned()), position)
}

enum Source {
    /// Project-relative `.prefab` path.
    Prefab(String),
    Object(Entity),
}

/// Copy a prefab or object to a new root object named "Name(Clone)", with physics bodies.
/// Script instances are created by the host after the current callback.
fn instantiate(w: &mut World, source: Source, position: Option<Vec3>) -> Result<Entity> {
    let e = match source {
        Source::Prefab(path) => {
            let prefab = Prefab::load(&w.root.join(&path)).map_err(mlua::Error::runtime)?;
            // Runtime copies are plain objects, not linked instances.
            let e = w.scene.instantiate_prefab(&prefab, None, None);
            // Save games recreate it from its prefab.
            w.save.spawned.push((e, path));
            e
        }
        Source::Object(src) => {
            alive(w, src)?;
            let world = w.scene.world_matrix(src);
            let e = w.scene.copy_subtree(src, None);
            // Same world placement as the original, now at the root.
            let (scale, rot, pos) = world.to_scale_rotation_translation();
            let mut t = w.scene.transform(e);
            t.position = pos;
            t.scale = scale;
            t.set_rotation(rot);
            w.scene.set_transform(e, t);
            w.scene.unlink_prefab(e);
            crate::save::fresh_ids_in(&mut w.scene, e);
            e
        }
    };
    let name = format!("{}(Clone)", w.scene.name(e));
    w.scene.game_object_mut(e).name = name;
    if let Some(p) = position {
        w.scene.set_world_position(e, p);
    }
    w.physics.add_subtree(&w.scene, e);
    w.physics2d.add_subtree(&w.scene, e);
    w.spawned.push(e);
    Ok(e)
}

/// Copy the current frame's time into the `Time` table.
pub fn refresh_time(lua: &Lua, w: &World) -> Result<()> {
    let t: Table = lua.globals().get("Time")?;
    t.set("deltaTime", w.time.delta)?;
    t.set("unscaledDeltaTime", w.time.unscaled_delta)?;
    t.set("time", w.time.time)?;
    t.set("frameCount", w.time.frame)?;
    t.set("timeScale", w.time.time_scale)?;
    t.set("fixedDeltaTime", w.physics.fixed_dt)?;
    Ok(())
}
