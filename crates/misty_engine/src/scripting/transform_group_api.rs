//! Lua `TransformGroup`: move many objects from one script in one call.
//!
//! A script per object pays for every engine call it makes (each `transform.position` is a
//! trip into the engine and a new Vector3), which is fine for hundreds of objects and too much
//! for thousands. A group hands all positions to the script as one flat list of numbers
//! (`x1, y1, z1, x2, ...`) and takes them back the same way: two engine calls per frame for
//! the whole crowd, and plain number math in between.
//!
//! ```lua
//! local Swarm = { speed = 2 }
//! function Swarm:Start()
//!     self.group = TransformGroup.new()
//!     for _, o in ipairs(GameObject.FindGameObjectsWithTag("Bee")) do self.group:Add(o) end
//! end
//! function Swarm:Update()
//!     local p = self.group:GetPositions(self.p)   -- the list is reused every frame
//!     self.p = p
//!     local step = self.speed * Time.deltaTime
//!     for i = 1, #p, 3 do p[i] = p[i] + step end
//!     self.group:SetPositions(p)
//! end
//! return Swarm
//! ```
//!
//! Object `i` (from 1) owns the numbers `3i-2 .. 3i` of a position or direction list and
//! `4i-3 .. 4i` of a rotation list (a quaternion `x, y, z, w`). Destroyed objects keep their
//! place (their numbers are left alone and writes to them are skipped) until `Prune()`.

use glam::{Quat, Vec3};
use hecs::Entity;
use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value, ffi};

use super::World;
use super::api::{LGameObject, LTransform, raw_stack_ok, sync_body, with_world};
use crate::scene::{Node, Transform};

/// The objects of a group, in the order they were added (`RemoveAt` and `Prune` move the last
/// one into a freed place).
pub struct LTransformGroup(Vec<Entity>);

fn entity_of(v: &Value) -> Result<Entity> {
    if let Value::UserData(ud) = v {
        if let Ok(g) = ud.borrow::<LGameObject>() {
            return Ok(g.0);
        }
        if let Ok(t) = ud.borrow::<LTransform>() {
            return Ok(t.0);
        }
    }
    Err(mlua::Error::runtime("expected a GameObject or a Transform"))
}

/// The first `n` numbers of the list `buf` (NaN where there is no number).
fn read_numbers(lua: &Lua, buf: &Table, n: usize) -> Result<Vec<f32>> {
    lua.exec_raw_lua(|raw| {
        let s = raw.state();
        let mut out = Vec::with_capacity(n);
        // SAFETY: two free stack slots are asked for and used; the stack is left as it was
        // found. Raw reads of a table run no Lua code, allocate nothing and raise no error.
        unsafe {
            if ffi::lua_checkstack(s, 2) == 0 {
                return Err(mlua::Error::runtime("stack overflow"));
            }
            mlua::IntoLua::push_into_stack(buf, raw)?;
            for i in 1..=n {
                ffi::lua_rawgeti(s, -1, i as ffi::lua_Integer);
                let mut is_number = 0;
                let v = ffi::lua_tonumberx(s, -1, &mut is_number);
                ffi::lua_pop(s, 1);
                out.push(if is_number != 0 { v as f32 } else { f32::NAN });
            }
            ffi::lua_pop(s, 1);
        }
        Ok(out)
    })
}

/// Write `values` into the list `buf` from index 1 (`None`: leave that number alone).
fn write_numbers(lua: &Lua, buf: &Table, values: &[Option<f32>]) -> Result<()> {
    if !raw_stack_ok(lua) {
        // A state with a memory limit: through the protected calls.
        for (i, v) in values.iter().enumerate() {
            if let Some(v) = v {
                buf.raw_set(i + 1, *v)?;
            }
        }
        return Ok(());
    }
    lua.exec_raw_lua(|raw| {
        let s = raw.state();
        // SAFETY: two free stack slots are asked for and used; the stack is left as it was
        // found. Raw writes run no Lua code; only an allocation failure can raise an error,
        // and this state has no memory limit (`raw_stack_ok`).
        unsafe {
            if ffi::lua_checkstack(s, 2) == 0 {
                return Err(mlua::Error::runtime("stack overflow"));
            }
            mlua::IntoLua::push_into_stack(buf, raw)?;
            for (i, v) in values.iter().enumerate() {
                if let Some(v) = v {
                    ffi::lua_pushnumber(s, f64::from(*v));
                    ffi::lua_rawseti(s, -2, (i + 1) as ffi::lua_Integer);
                }
            }
            ffi::lua_pop(s, 1);
        }
        Ok(())
    })
}

/// The list to fill: the script's own, or a new one with room for `n` numbers.
fn list(lua: &Lua, buf: Option<Table>, n: usize) -> Result<Table> {
    match buf {
        Some(t) => Ok(t),
        None => lua.create_table_with_capacity(n, 0),
    }
}

impl LTransformGroup {
    /// `N` numbers for every object from `get` into the script's list (a new one without);
    /// `get` is `None` for a destroyed object.
    fn fill<const N: usize>(
        &self,
        lua: &Lua,
        buf: Option<Table>,
        get: impl Fn(&mut World, Entity) -> Option<[f32; N]>,
    ) -> Result<Table> {
        let values = with_world(lua, |w| {
            let mut values = Vec::with_capacity(self.0.len() * N);
            for &e in &self.0 {
                match get(w, e) {
                    Some(v) => values.extend(v.map(Some)),
                    None => values.extend([None; N]),
                }
            }
            values
        })?;
        let buf = list(lua, buf, values.len())?;
        write_numbers(lua, &buf, &values)?;
        Ok(buf)
    }

    /// `set` for every object with its `N` numbers of `buf`; objects whose numbers are
    /// missing or not finite stay as they are. `set` says whether it moved the object (not
    /// when it is destroyed).
    fn apply<const N: usize>(
        &self,
        lua: &Lua,
        buf: &Table,
        set: impl Fn(&mut World, Entity, [f32; N]) -> bool,
    ) -> Result<()> {
        let values = read_numbers(lua, buf, self.0.len() * N)?;
        with_world(lua, |w| {
            // Bodies follow the objects a script moves; no world has any: nothing to ask.
            let bodies = w.physics.body_count() + w.physics2d.body_count() > 0;
            for (&e, v) in self.0.iter().zip(values.chunks_exact(N)) {
                if v.iter().all(|x| x.is_finite())
                    && set(w, e, std::array::from_fn(|i| v[i]))
                    && bodies
                {
                    sync_body(w, e);
                }
            }
        })
    }
}

/// What a root object's Transform gives with `root` (its local values are its world values),
/// what `other` gives for an object under a parent, `None` for a destroyed object. One look
/// at the object instead of one per question (alive? parent? transform?).
fn with_transform<R>(
    w: &mut World,
    e: Entity,
    root: impl FnOnce(&mut Transform) -> R,
    other: impl FnOnce(&mut World) -> R,
) -> Option<R> {
    match w.scene.world.query_one_mut::<(&mut Transform, &Node)>(e) {
        Ok((t, node)) if node.parent.is_none() => Some(root(t)),
        Ok(_) => Some(other(w)),
        Err(_) => None,
    }
}

fn set_rotation(w: &mut World, e: Entity, q: Quat) -> bool {
    with_transform(
        w,
        e,
        |t| t.set_rotation(q),
        |w| w.scene.set_world_rotation(e, q),
    )
    .is_some()
}

impl UserData for LTransformGroup {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("count", |_, g| Ok(g.0.len()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Len, |_, g, ()| Ok(g.0.len()));
        // `Add(object)`: its place in the group (from 1).
        m.add_method_mut("Add", |lua, g, v: Value| {
            let e = entity_of(&v)?;
            with_world(lua, |w| super::api::alive(w, e))??;
            g.0.push(e);
            Ok(g.0.len())
        });
        // `Get(i)`: the object at place `i` (nil outside the group or when destroyed).
        m.add_method("Get", |lua, g, i: usize| {
            let Some(&e) = i.checked_sub(1).and_then(|i| g.0.get(i)) else {
                return Ok(None);
            };
            Ok(with_world(lua, |w| w.scene.contains(e))?.then_some(LGameObject(e)))
        });
        // `RemoveAt(i)`: the last object takes place `i` (do the same with your own lists).
        m.add_method_mut("RemoveAt", |_, g, i: usize| {
            match i.checked_sub(1).filter(|i| *i < g.0.len()) {
                Some(i) => {
                    g.0.swap_remove(i);
                    Ok(())
                }
                None => Err(mlua::Error::runtime(format!(
                    "RemoveAt({i}): the group has {} objects",
                    g.0.len()
                ))),
            }
        });
        // `Prune()`: drop destroyed objects. Returns the freed places in the order they were
        // filled, each by the object that was last at that moment:
        // `for _, i in ipairs(group:Prune()) do mine[i] = mine[#mine]; mine[#mine] = nil end`.
        m.add_method_mut("Prune", |lua, g, ()| {
            with_world(lua, |w| {
                let mut freed = Vec::new();
                let mut i = g.0.len();
                // From the end: the object moved into a freed place is known to be alive.
                while i > 0 {
                    i -= 1;
                    if !w.scene.contains(g.0[i]) {
                        g.0.swap_remove(i);
                        freed.push(i + 1);
                    }
                }
                freed
            })
        });
        m.add_method_mut("Clear", |_, g, ()| {
            g.0.clear();
            Ok(())
        });
        // World positions as `x, y, z` per object.
        m.add_method("GetPositions", |lua, g, buf: Option<Table>| {
            g.fill(lua, buf, |w, e| {
                with_transform(
                    w,
                    e,
                    |t| t.position.to_array(),
                    |w| w.scene.world_position(e).to_array(),
                )
            })
        });
        m.add_method("SetPositions", |lua, g, buf: Table| {
            g.apply(lua, &buf, |w, e, p: [f32; 3]| {
                let p = Vec3::from_array(p);
                with_transform(
                    w,
                    e,
                    |t| t.position = p,
                    |w| w.scene.set_world_position(e, p),
                )
                .is_some()
            })
        });
        // World rotations as a quaternion `x, y, z, w` per object.
        m.add_method("GetRotations", |lua, g, buf: Option<Table>| {
            g.fill(lua, buf, |w, e| {
                with_transform(
                    w,
                    e,
                    |t| t.rotation.to_array(),
                    |w| w.scene.world_rotation(e).to_array(),
                )
            })
        });
        m.add_method("SetRotations", |lua, g, buf: Table| {
            g.apply(lua, &buf, |w, e, q: [f32; 4]| {
                let q = Quat::from_array(q);
                q.length_squared() > 1e-12 && set_rotation(w, e, q.normalize())
            })
        });
        // `LookAlong(directions)`: turn every object to face its direction `x, y, z` (like
        // `transform:LookAt(position + direction)`); a zero direction leaves it as it is.
        m.add_method("LookAlong", |lua, g, buf: Table| {
            g.apply(lua, &buf, |w, e, d: [f32; 3]| {
                let dir = Vec3::from_array(d).normalize_or_zero();
                if dir == Vec3::ZERO {
                    return false;
                }
                let up = if dir.y.abs() > 0.999 {
                    Vec3::Z
                } else {
                    Vec3::Y
                };
                set_rotation(
                    w,
                    e,
                    glam::camera::lh::view::look_to_quat(dir, up).inverse(),
                )
            })
        });
    }
}

/// Register the global `TransformGroup` table.
pub fn register(lua: &Lua) -> Result<()> {
    let t = lua.create_table()?;
    // `TransformGroup.new([objects])`: a group, with the objects of a list already in it.
    t.set(
        "new",
        lua.create_function(|lua, objects: Option<Table>| {
            let mut group = Vec::new();
            if let Some(objects) = objects {
                for v in objects.sequence_values::<Value>() {
                    group.push(entity_of(&v?)?);
                }
                with_world(lua, |w| group.retain(|e| w.scene.contains(*e)))?;
            }
            Ok(LTransformGroup(group))
        })?,
    )?;
    lua.globals().set("TransformGroup", t)
}
