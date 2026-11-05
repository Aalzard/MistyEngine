//! Lua 2D API: `Rigidbody2D`, 2D colliders, `SpriteRenderer`, `Physics2D` queries and gravity,
//! `Vector2`, `ForceMode2D`, and the Collision2D tables of `OnCollision*2D` callbacks.
//!
//! There is no separate Vector2 type: 2D values are Vector3 userdata with z = 0, and every 2D
//! function accepts a Vector3 (z ignored), a `{x=, y=}` table or two numbers.

use glam::Vec2;
use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::World;
use super::api::{LGameObject, LTransform, LVec3, vec_args, with_world};
use super::ui_api::{LColor, get, set};
use crate::physics::{ALL_LAYERS, DEFAULT_RAYCAST_LAYERS};
use crate::physics2d::{Contact2D, RaycastHit2D};
use crate::scene::{
    BodyType2D, BoxCollider2D, CapsuleCollider2D, CircleCollider2D, Collider2DCommon,
    EdgeCollider2D, PolygonCollider2D, Rigidbody2D, SpriteRenderer,
};

fn v2(v: Vec2) -> LVec3 {
    LVec3(v.extend(0.0))
}

/// A 2D vector from a Vector3, a table or two numbers.
fn vec2_args(lua: &Lua, args: &[Value]) -> Result<Vec2> {
    match args {
        [a, b, ..] if matches!(a, Value::Integer(_) | Value::Number(_)) => {
            let n = |v: &Value| match v {
                Value::Integer(i) => *i as f32,
                Value::Number(n) => *n as f32,
                _ => 0.0,
            };
            Ok(Vec2::new(n(a), n(b)))
        }
        [Value::Table(t), ..] => Ok(Vec2::new(t.get("x")?, t.get("y")?)),
        _ => Ok(vec_args(lua, args)?.truncate()),
    }
}

fn mask(v: Option<i64>, default: u32) -> u32 {
    v.map_or(default, |m| m as u32)
}

// ---- Rigidbody2D ------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LRigidbody2D(pub Entity);

/// Velocity change of a force for one fixed step (mass from the component, like 3D).
fn delta_velocity(w: &World, e: Entity, f: Vec2, mode: &str) -> Vec2 {
    let mass = w
        .scene
        .world
        .get::<&Rigidbody2D>(e)
        .map(|r| r.mass.max(1e-4))
        .unwrap_or(1.0);
    match mode {
        "Impulse" => f / mass,
        _ => f * w.physics2d.fixed_dt / mass,
    }
}

/// Force vector and ForceMode2D name ("Force" when left out).
fn force_args(lua: &Lua, args: &Variadic<Value>) -> Result<(Vec2, String)> {
    let mode = match args.last() {
        Some(Value::String(s)) => s.to_str()?.to_owned(),
        _ => "Force".to_owned(),
    };
    Ok((vec2_args(lua, args)?, mode))
}

impl UserData for LRigidbody2D {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("velocity", |lua, r| {
            with_world(lua, |w| v2(w.physics2d.velocity(r.0).unwrap_or_default()))
        });
        f.add_field_method_set("velocity", |lua, r, v: Value| {
            let v = vec2_args(lua, &[v])?;
            with_world(lua, |w| w.physics2d.set_velocity(r.0, v))
        });
        f.add_field_method_get("angularVelocity", |lua, r| {
            with_world(lua, |w| w.physics2d.angular_velocity(r.0).unwrap_or(0.0))
        });
        f.add_field_method_set("angularVelocity", |lua, r, deg: f32| {
            with_world(lua, |w| w.physics2d.set_angular_velocity(r.0, deg))
        });
        f.add_field_method_get("position", |lua, r| {
            with_world(lua, |w| match w.physics2d.body_pose(r.0) {
                Some((p, _)) => v2(p),
                None => v2(w.scene.world_position(r.0).truncate()),
            })
        });
        f.add_field_method_get("rotation", |lua, r| {
            with_world(lua, |w| w.physics2d.body_pose(r.0).map_or(0.0, |p| p.1))
        });
        f.add_field_method_get("mass", |lua, r| get(lua, r.0, |b: &Rigidbody2D| b.mass));
        f.add_field_method_get("gravityScale", |lua, r| {
            with_world(lua, |w| w.physics2d.gravity_scale(r.0).unwrap_or(1.0))
        });
        f.add_field_method_set("gravityScale", |lua, r, s: f32| {
            set(lua, r.0, |b: &mut Rigidbody2D| b.gravity_scale = s)?;
            with_world(lua, |w| w.physics2d.set_gravity_scale(r.0, s))
        });
        f.add_field_method_get("bodyType", |lua, r| {
            get(lua, r.0, |b: &Rigidbody2D| b.body_type.label())
        });
        f.add_field_method_set("bodyType", |lua, r, name: String| {
            let kind = BodyType2D::ALL
                .into_iter()
                .find(|k| k.label() == name)
                .ok_or_else(|| mlua::Error::runtime(format!("unknown body type '{name}'")))?;
            set(lua, r.0, |b: &mut Rigidbody2D| b.body_type = kind)?;
            with_world(lua, |w| w.physics2d.set_body_type(r.0, kind))
        });
        f.add_field_method_get("isKinematic", |lua, r| {
            get(lua, r.0, |b: &Rigidbody2D| {
                b.body_type == BodyType2D::Kinematic
            })
        });
        f.add_field_method_get("freezeRotation", |lua, r| {
            get(lua, r.0, |b: &Rigidbody2D| b.freeze_rotation)
        });
        f.add_field_method_get("gameObject", |_, r| Ok(LGameObject(r.0)));
        f.add_field_method_get("transform", |_, r| Ok(LTransform(r.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("AddForce", |lua, r, args: Variadic<Value>| {
            let (force, mode) = force_args(lua, &args)?;
            with_world(lua, |w| {
                let dv = delta_velocity(w, r.0, force, &mode);
                let v = w.physics2d.velocity(r.0).unwrap_or_default();
                w.physics2d.set_velocity(r.0, v + dv);
            })
        });
        // AddTorque(degrees-ish torque [, mode]): counter-clockwise positive.
        m.add_method(
            "AddTorque",
            |lua, r, (torque, mode): (f32, Option<String>)| {
                with_world(lua, |w| {
                    let dt = w.physics2d.fixed_dt;
                    let impulse = match mode.as_deref() {
                        Some("Impulse") => torque,
                        _ => torque * dt,
                    };
                    w.physics2d.apply_torque_impulse(r.0, impulse);
                })
            },
        );
        m.add_method("MovePosition", |lua, r, args: Variadic<Value>| {
            let p = vec2_args(lua, &args)?;
            with_world(lua, |w| w.physics2d.move_body(r.0, Some(p), None))
        });
        m.add_method("MoveRotation", |lua, r, degrees: f32| {
            with_world(lua, |w| w.physics2d.move_body(r.0, None, Some(degrees)))
        });
    }
}

// ---- Colliders --------------------------------------------------------------------------------

/// Any 2D collider; `kind` = the component name without spaces ("BoxCollider2D").
#[derive(Clone, Copy)]
pub struct LCollider2D(pub Entity, pub &'static str);

fn with_common<R>(
    lua: &Lua,
    c: LCollider2D,
    f: impl FnOnce(&mut Collider2DCommon) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        let wd = &w.scene.world;
        let e = c.0;
        let out = match c.1 {
            "BoxCollider2D" => wd
                .get::<&mut BoxCollider2D>(e)
                .map(|mut x| f(&mut x.common)),
            "CircleCollider2D" => wd
                .get::<&mut CircleCollider2D>(e)
                .map(|mut x| f(&mut x.common)),
            "CapsuleCollider2D" => wd
                .get::<&mut CapsuleCollider2D>(e)
                .map(|mut x| f(&mut x.common)),
            "PolygonCollider2D" => wd
                .get::<&mut PolygonCollider2D>(e)
                .map(|mut x| f(&mut x.common)),
            _ => wd
                .get::<&mut EdgeCollider2D>(e)
                .map(|mut x| f(&mut x.common)),
        };
        out.map_err(|_| mlua::Error::runtime(format!("{} was destroyed", c.1)))
    })?
}

impl UserData for LCollider2D {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("isTrigger", |lua, c| with_common(lua, *c, |x| x.is_trigger));
        f.add_field_method_get("enabled", |lua, c| with_common(lua, *c, |x| x.enabled));
        f.add_field_method_get("offset", |lua, c| with_common(lua, *c, |x| v2(x.offset)));
        f.add_field_method_get("size", |lua, c| match c.1 {
            "BoxCollider2D" => get(lua, c.0, |b: &BoxCollider2D| Some(v2(b.size))),
            "CapsuleCollider2D" => get(lua, c.0, |b: &CapsuleCollider2D| Some(v2(b.size))),
            _ => Ok(None),
        });
        f.add_field_method_get("radius", |lua, c| match c.1 {
            "CircleCollider2D" => get(lua, c.0, |b: &CircleCollider2D| Some(b.radius)),
            _ => Ok(None),
        });
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
        f.add_field_method_get("attachedRigidbody", |lua, c| {
            with_world(lua, |w| {
                let mut cur = Some(c.0);
                while let Some(e) = cur {
                    if w.physics2d.has_body(e) {
                        return Some(LRigidbody2D(e));
                    }
                    cur = w.scene.parent(e);
                }
                None
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("CompareTag", |lua, c, tag: String| {
            with_world(lua, |w| w.scene.game_object(c.0).tag == tag)
        });
    }
}

// ---- SpriteRenderer ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LSpriteRenderer(pub Entity);

impl UserData for LSpriteRenderer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("sprite", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.sprite.clone())
        });
        f.add_field_method_set("sprite", |lua, s, v: Option<String>| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.sprite = v)
        });
        f.add_field_method_get("color", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| LColor(r.color))
        });
        f.add_field_method_set("color", |lua, s, c: LColor| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.color = c.0)
        });
        f.add_field_method_get("flipX", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.flip_x)
        });
        f.add_field_method_set("flipX", |lua, s, v: bool| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.flip_x = v)
        });
        f.add_field_method_get("flipY", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.flip_y)
        });
        f.add_field_method_set("flipY", |lua, s, v: bool| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.flip_y = v)
        });
        f.add_field_method_get("sortingLayerName", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.sorting_layer.clone())
        });
        f.add_field_method_set("sortingLayerName", |lua, s, v: String| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.sorting_layer = v)
        });
        f.add_field_method_get("sortingOrder", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.order_in_layer)
        });
        f.add_field_method_set("sortingOrder", |lua, s, v: i32| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.order_in_layer = v)
        });
        f.add_field_method_get("enabled", |lua, s| {
            get(lua, s.0, |r: &SpriteRenderer| r.enabled)
        });
        f.add_field_method_set("enabled", |lua, s, v: bool| {
            set(lua, s.0, |r: &mut SpriteRenderer| r.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, s| Ok(LGameObject(s.0)));
    }
}

/// `GetComponent` for the 2D components handled here.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |f: fn(&hecs::World, Entity) -> bool| with_world(lua, |w| f(&w.scene.world, e));
    let ud = |v: Result<mlua::AnyUserData>| v.map(|u| Some(Value::UserData(u)));
    match name {
        "Rigidbody2D" if has(|w, e| w.get::<&Rigidbody2D>(e).is_ok())? => {
            ud(lua.create_userdata(LRigidbody2D(e)))
        }
        "SpriteRenderer" if has(|w, e| w.get::<&SpriteRenderer>(e).is_ok())? => {
            ud(lua.create_userdata(LSpriteRenderer(e)))
        }
        "BoxCollider2D" if has(|w, e| w.get::<&BoxCollider2D>(e).is_ok())? => {
            ud(lua.create_userdata(LCollider2D(e, "BoxCollider2D")))
        }
        "CircleCollider2D" if has(|w, e| w.get::<&CircleCollider2D>(e).is_ok())? => {
            ud(lua.create_userdata(LCollider2D(e, "CircleCollider2D")))
        }
        "CapsuleCollider2D" if has(|w, e| w.get::<&CapsuleCollider2D>(e).is_ok())? => {
            ud(lua.create_userdata(LCollider2D(e, "CapsuleCollider2D")))
        }
        "PolygonCollider2D" if has(|w, e| w.get::<&PolygonCollider2D>(e).is_ok())? => {
            ud(lua.create_userdata(LCollider2D(e, "PolygonCollider2D")))
        }
        "EdgeCollider2D" if has(|w, e| w.get::<&EdgeCollider2D>(e).is_ok())? => {
            ud(lua.create_userdata(LCollider2D(e, "EdgeCollider2D")))
        }
        // Tilemap and Light2D (tilemap_api.rs).
        _ => super::tilemap_api::component(lua, e, name),
    }
}

// ---- Queries and globals ----------------------------------------------------------------------

/// A RaycastHit2D table (nil-able by the caller).
fn hit_table(lua: &Lua, hit: &RaycastHit2D) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("point", v2(hit.point))?;
    t.set("normal", v2(hit.normal))?;
    t.set("distance", hit.distance)?;
    t.set("fraction", hit.fraction)?;
    t.set("gameObject", LGameObject(hit.entity))?;
    t.set("collider", LGameObject(hit.entity))?;
    t.set("transform", LTransform(hit.entity))?;
    if let Some(b) = hit.body {
        t.set("rigidbody", LRigidbody2D(b))?;
    }
    Ok(t)
}

fn objects(lua: &Lua, list: Vec<Entity>) -> Result<Table> {
    let t = lua.create_table()?;
    for (i, e) in list.into_iter().enumerate() {
        t.set(i + 1, LGameObject(e))?;
    }
    Ok(t)
}

/// Split `(…vector args…, number, number)`: the vector, then trailing numbers.
fn ray_args(lua: &Lua, args: &[Value]) -> Result<(Vec2, Vec2, Option<f32>, Option<i64>)> {
    // Accept Vector3/table pairs only (the usual call shape).
    let origin = vec2_args(lua, &args[..1.min(args.len())])?;
    let dir = vec2_args(lua, args.get(1..2).unwrap_or(&[]))?;
    let num = |i: usize| match args.get(i) {
        Some(Value::Integer(n)) => Some(*n as f64),
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    };
    Ok((
        origin,
        dir,
        num(2).map(|d| d as f32),
        num(3).map(|m| m as i64),
    ))
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();

    let p2 = lua.create_table()?;
    p2.set("DefaultRaycastLayers", DEFAULT_RAYCAST_LAYERS as i64)?;
    p2.set("AllLayers", ALL_LAYERS as i64)?;
    p2.set(
        "Raycast",
        lua.create_function(|lua, args: Variadic<Value>| {
            let (o, d, dist, m) = ray_args(lua, &args)?;
            let hit = with_world(lua, |w| {
                w.physics2d.raycast(
                    o,
                    d,
                    dist.unwrap_or(f32::INFINITY),
                    mask(m, DEFAULT_RAYCAST_LAYERS),
                )
            })?;
            hit.map(|h| hit_table(lua, &h)).transpose()
        })?,
    )?;
    p2.set(
        "RaycastAll",
        lua.create_function(|lua, args: Variadic<Value>| {
            let (o, d, dist, m) = ray_args(lua, &args)?;
            let hits = with_world(lua, |w| {
                w.physics2d.raycast_all(
                    o,
                    d,
                    dist.unwrap_or(f32::INFINITY),
                    mask(m, DEFAULT_RAYCAST_LAYERS),
                )
            })?;
            let t = lua.create_table()?;
            for (i, h) in hits.iter().enumerate() {
                t.set(i + 1, hit_table(lua, h)?)?;
            }
            Ok(t)
        })?,
    )?;
    p2.set(
        "OverlapCircle",
        lua.create_function(|lua, (c, r, m): (Value, f32, Option<i64>)| {
            let c = vec2_args(lua, &[c])?;
            let found = with_world(lua, |w| {
                w.physics2d.overlap_circle_all(c, r, mask(m, ALL_LAYERS))
            })?;
            Ok(found.first().map(|e| LGameObject(*e)))
        })?,
    )?;
    p2.set(
        "OverlapCircleAll",
        lua.create_function(|lua, (c, r, m): (Value, f32, Option<i64>)| {
            let c = vec2_args(lua, &[c])?;
            let found = with_world(lua, |w| {
                w.physics2d.overlap_circle_all(c, r, mask(m, ALL_LAYERS))
            })?;
            objects(lua, found)
        })?,
    )?;
    p2.set(
        "OverlapBox",
        lua.create_function(
            |lua, (c, s, angle, m): (Value, Value, Option<f32>, Option<i64>)| {
                let (c, s) = (vec2_args(lua, &[c])?, vec2_args(lua, &[s])?);
                let found = with_world(lua, |w| {
                    w.physics2d
                        .overlap_box_all(c, s, angle.unwrap_or(0.0), mask(m, ALL_LAYERS))
                })?;
                Ok(found.first().map(|e| LGameObject(*e)))
            },
        )?,
    )?;
    p2.set(
        "OverlapPoint",
        lua.create_function(|lua, (p, m): (Value, Option<i64>)| {
            let p = vec2_args(lua, &[p])?;
            let found = with_world(lua, |w| {
                w.physics2d.overlap_point_all(p, mask(m, ALL_LAYERS))
            })?;
            Ok(found.first().map(|e| LGameObject(*e)))
        })?,
    )?;
    // `Physics2D.gravity` reads and writes the live world.
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| match key.as_str() {
            "gravity" => with_world(lua, |w| Some(v2(w.physics2d.gravity()))),
            _ => Ok(None),
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|lua, (t, key, v): (Table, String, Value)| {
            if key == "gravity" {
                let g = vec2_args(lua, &[v])?;
                with_world(lua, |w| w.physics2d.set_gravity(g))
            } else {
                t.raw_set(key, v)
            }
        })?,
    )?;
    p2.set_metatable(Some(meta))?;
    g.set("Physics2D", p2)?;

    let fm = lua.create_table()?;
    fm.set("Force", "Force")?;
    fm.set("Impulse", "Impulse")?;
    g.set("ForceMode2D", fm)?;

    let vec2 = lua.create_table()?;
    vec2.set(
        "new",
        lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| {
            Ok(v2(Vec2::new(x.unwrap_or(0.0), y.unwrap_or(0.0))))
        })?,
    )?;
    for (name, v) in [
        ("zero", Vec2::ZERO),
        ("one", Vec2::ONE),
        ("up", Vec2::Y),
        ("down", Vec2::NEG_Y),
        ("left", Vec2::NEG_X),
        ("right", Vec2::X),
    ] {
        vec2.set(name, v2(v))?;
    }
    vec2.set(
        "Distance",
        lua.create_function(|lua, (a, b): (Value, Value)| {
            Ok(vec2_args(lua, &[a])?.distance(vec2_args(lua, &[b])?))
        })?,
    )?;
    vec2.set(
        "Dot",
        lua.create_function(|lua, (a, b): (Value, Value)| {
            Ok(vec2_args(lua, &[a])?.dot(vec2_args(lua, &[b])?))
        })?,
    )?;
    vec2.set(
        "Lerp",
        lua.create_function(|lua, (a, b, t): (Value, Value, f32)| {
            let (a, b) = (vec2_args(lua, &[a])?, vec2_args(lua, &[b])?);
            Ok(v2(a.lerp(b, t.clamp(0.0, 1.0))))
        })?,
    )?;
    // Calling the table works like `new`: Vector2(1, 2).
    let call = lua.create_table()?;
    call.set(
        "__call",
        lua.create_function(|_, (_, x, y): (Table, Option<f32>, Option<f32>)| {
            Ok(v2(Vec2::new(x.unwrap_or(0.0), y.unwrap_or(0.0))))
        })?,
    )?;
    vec2.set_metatable(Some(call))?;
    g.set("Vector2", vec2)?;
    Ok(())
}

/// The Collision2D table for `me`'s side of a contact.
pub fn collision_table(lua: &Lua, me: Entity, c: &Contact2D) -> Result<Table> {
    let (other, sign) = if c.a == me { (c.b, 1.0) } else { (c.a, -1.0) };
    // Normals point away from the other object, towards `me`.
    let normal = -c.normal * sign;
    let t = lua.create_table()?;
    t.set("gameObject", LGameObject(other))?;
    t.set("collider", LGameObject(other))?;
    t.set("otherCollider", LGameObject(me))?;
    t.set("transform", LTransform(other))?;
    t.set("relativeVelocity", v2(c.relative_velocity * sign))?;
    let has_body = with_world(lua, |w| w.physics2d.has_body(other))?;
    if has_body {
        t.set("rigidbody", LRigidbody2D(other))?;
    }
    let contact = lua.create_table()?;
    contact.set("point", v2(c.point))?;
    contact.set("normal", v2(normal))?;
    let list = lua.create_table()?;
    list.set(1, contact)?;
    t.set("contacts", list)?;
    t.set("contactCount", 1)?;
    let get_contact = lua.create_function(|_, (t, i): (Table, i64)| {
        t.get::<Table>("contacts")?.get::<Value>(i + 1)
    })?;
    t.set("GetContact", get_contact)?;
    Ok(t)
}

#[cfg(test)]
#[path = "physics2d_tests.rs"]
mod tests;
