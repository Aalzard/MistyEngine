//! Lua physics API: `Physics.Raycast/RaycastAll/SphereCast/OverlapSphere`, `LayerMask`, and
//! the CharacterController component (`Move`, `SimpleMove`, `isGrounded`, `velocity`).
//!
//! Hits are tables: `point`, `normal`, `distance`, `gameObject`, `collider` (the collider's
//! GameObject), `transform`, `rigidbody` (or nil).

use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::World;
use super::api::{LGameObject, LRigidbody, LTransform, LVec3, with_world};
use crate::physics::{ALL_LAYERS, DEFAULT_RAYCAST_LAYERS, RaycastHit, layers};
use crate::scene::{CharacterController, Rigidbody};

/// Lua integers are 64-bit; masks keep their low 32 bits (so `~mask` works).
fn mask(v: Option<i64>, default: u32) -> u32 {
    v.map_or(default, |m| m as u32)
}

fn distance(v: Option<f32>) -> f32 {
    v.unwrap_or(f32::INFINITY)
}

/// A RaycastHit table.
fn hit_table(lua: &Lua, w: &World, hit: &RaycastHit) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("point", LVec3(hit.point))?;
    t.set("normal", LVec3(hit.normal))?;
    t.set("distance", hit.distance)?;
    t.set("gameObject", LGameObject(hit.entity))?;
    t.set("collider", LGameObject(hit.entity))?;
    t.set("transform", LTransform(hit.entity))?;
    let body = hit
        .body
        .filter(|b| w.scene.world.get::<&Rigidbody>(*b).is_ok());
    if let Some(b) = body {
        t.set("rigidbody", LRigidbody(b))?;
    }
    Ok(t)
}

fn hits_list(lua: &Lua, hits: &[RaycastHit]) -> Result<Table> {
    let list = lua.create_table()?;
    with_world(lua, |w| {
        for (i, h) in hits.iter().enumerate() {
            list.set(i + 1, hit_table(lua, w, h)?)?;
        }
        Ok::<_, mlua::Error>(())
    })??;
    Ok(list)
}

fn single(lua: &Lua, hit: Option<RaycastHit>) -> Result<Value> {
    match hit {
        Some(h) => with_world(lua, |w| hit_table(lua, w, &h).map(Value::Table))?,
        None => Ok(Value::Nil),
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let physics = lua.create_table()?;
    physics.set("DefaultRaycastLayers", DEFAULT_RAYCAST_LAYERS as i64)?;
    physics.set("AllLayers", ALL_LAYERS as i64)?;
    physics.set("IgnoreRaycastLayer", 1i64 << layers::IGNORE_RAYCAST_LAYER)?;
    physics.set(
        "Raycast",
        lua.create_function(
            |lua, (origin, dir, max, m): (LVec3, LVec3, Option<f32>, Option<i64>)| {
                let hit = with_world(lua, |w| {
                    w.physics.raycast(
                        origin.0,
                        dir.0,
                        distance(max),
                        mask(m, DEFAULT_RAYCAST_LAYERS),
                    )
                })?;
                single(lua, hit)
            },
        )?,
    )?;
    physics.set(
        "RaycastAll",
        lua.create_function(
            |lua, (origin, dir, max, m): (LVec3, LVec3, Option<f32>, Option<i64>)| {
                let hits = with_world(lua, |w| {
                    w.physics.raycast_all(
                        origin.0,
                        dir.0,
                        distance(max),
                        mask(m, DEFAULT_RAYCAST_LAYERS),
                    )
                })?;
                hits_list(lua, &hits)
            },
        )?,
    )?;
    physics.set(
        "SphereCast",
        lua.create_function(
            |lua, (origin, radius, dir, max, m): (LVec3, f32, LVec3, Option<f32>, Option<i64>)| {
                let hit = with_world(lua, |w| {
                    w.physics.sphere_cast(
                        origin.0,
                        radius,
                        dir.0,
                        distance(max),
                        mask(m, DEFAULT_RAYCAST_LAYERS),
                    )
                })?;
                single(lua, hit)
            },
        )?,
    )?;
    physics.set(
        "OverlapSphere",
        lua.create_function(|lua, (center, radius, m): (LVec3, f32, Option<i64>)| {
            let found = with_world(lua, |w| {
                w.physics
                    .overlap_sphere(center.0, radius, mask(m, ALL_LAYERS))
            })?;
            let list = lua.create_table()?;
            for (i, e) in found.into_iter().enumerate() {
                list.set(i + 1, LGameObject(e))?;
            }
            Ok(list)
        })?,
    )?;
    g.set("Physics", physics)?;

    let layer_mask = lua.create_table()?;
    layer_mask.set(
        "GetMask",
        lua.create_function(|_, names: Variadic<String>| Ok(layers::get_mask(&names) as i64))?,
    )?;
    layer_mask.set(
        "NameToLayer",
        lua.create_function(|_, name: String| Ok(layers::name_to_layer(&name)))?,
    )?;
    layer_mask.set(
        "LayerToName",
        lua.create_function(|_, layer: i32| Ok(layers::layer_to_name(layer)))?,
    )?;
    g.set("LayerMask", layer_mask)?;
    Ok(())
}

// ---- CharacterController ----------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LCharacterController(pub Entity);

fn settings<R>(lua: &Lua, e: Entity, f: impl FnOnce(&CharacterController) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&CharacterController>(e)
            .map(|c| f(&c))
            .map_err(|_| mlua::Error::runtime("object was destroyed"))
    })?
}

/// Seconds the current callback's move covers.
fn frame_dt(w: &World) -> f32 {
    if w.time.delta > 0.0 {
        w.time.delta
    } else {
        w.physics.fixed_dt
    }
}

impl UserData for LCharacterController {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("isGrounded", |lua, c| {
            with_world(lua, |w| w.physics.character_grounded(c.0))
        });
        f.add_field_method_get("velocity", |lua, c| {
            with_world(lua, |w| LVec3(w.physics.character_velocity(c.0)))
        });
        f.add_field_method_get("height", |lua, c| settings(lua, c.0, |s| s.height));
        f.add_field_method_get("radius", |lua, c| settings(lua, c.0, |s| s.radius));
        f.add_field_method_get("center", |lua, c| settings(lua, c.0, |s| LVec3(s.center)));
        f.add_field_method_get("slopeLimit", |lua, c| settings(lua, c.0, |s| s.slope_limit));
        f.add_field_method_get("stepOffset", |lua, c| settings(lua, c.0, |s| s.step_offset));
        f.add_field_method_get("skinWidth", |lua, c| settings(lua, c.0, |s| s.skin_width));
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Move(motion): world-space displacement for this frame (no gravity added).
        m.add_method("Move", |lua, c, motion: LVec3| {
            with_world(lua, |w| {
                let dt = frame_dt(w);
                let World { physics, scene, .. } = w;
                physics.move_character(scene, c.0, motion.0, dt);
            })
        });
        // SimpleMove(speed): units per second, Y ignored, gravity applied. Returns isGrounded.
        m.add_method("SimpleMove", |lua, c, speed: LVec3| {
            with_world(lua, |w| {
                let dt = frame_dt(w);
                let World { physics, scene, .. } = w;
                physics.simple_move(scene, c.0, speed.0, dt)
            })
        });
    }
}

/// `GetComponent` for the physics components handled here.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "CharacterController" {
        return Ok(None);
    }
    let has = with_world(lua, |w| {
        w.scene.world.get::<&CharacterController>(e).is_ok()
    })?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LCharacterController(e))?,
    )))
}

/// A ControllerColliderHit table for OnControllerColliderHit.
pub fn controller_hit_table(lua: &Lua, hit: &crate::physics::ControllerHit) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("controller", LCharacterController(hit.controller))?;
    t.set("gameObject", LGameObject(hit.other))?;
    t.set("collider", LGameObject(hit.other))?;
    t.set("transform", LTransform(hit.other))?;
    t.set("point", LVec3(hit.point))?;
    t.set("normal", LVec3(hit.normal))?;
    t.set("moveDirection", LVec3(hit.move_direction))?;
    t.set("moveLength", hit.move_length)?;
    let body = with_world(lua, |w| {
        w.scene.world.get::<&Rigidbody>(hit.other).is_ok() && w.physics.has_body(hit.other)
    })?;
    if body {
        t.set("rigidbody", LRigidbody(hit.other))?;
    }
    Ok(t)
}

#[cfg(test)]
#[path = "physics_tests.rs"]
mod tests;
