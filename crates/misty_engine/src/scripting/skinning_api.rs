//! Lua `SkinnedMeshRenderer` (from `GetComponent("SkinnedMeshRenderer")`): blend shape weights
//! (`GetBlendShapeWeight` / `SetBlendShapeWeight`, 0–100 by shape index), shape lookup by name
//! and the usual renderer switches.

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, with_world};
use crate::scene::SkinnedMeshRenderer;

#[derive(Clone, Copy)]
pub struct LSkinnedMeshRenderer(pub Entity);

fn gone() -> mlua::Error {
    mlua::Error::runtime("object was destroyed")
}

fn get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&SkinnedMeshRenderer) -> R) -> Result<R> {
    with_world(lua, |w| {
        let r = w
            .scene
            .world
            .get::<&SkinnedMeshRenderer>(e)
            .map_err(|_| gone())?;
        Ok(f(&r))
    })?
}

fn set(lua: &Lua, e: Entity, f: impl FnOnce(&mut SkinnedMeshRenderer)) -> Result<()> {
    with_world(lua, |w| {
        let mut r = w
            .scene
            .world
            .get::<&mut SkinnedMeshRenderer>(e)
            .map_err(|_| gone())?;
        f(&mut r);
        Ok(())
    })?
}

/// Blend shape names of the renderer's mesh (read relative to the project root when the
/// model cache has no project folder set).
fn shape_names(lua: &Lua, e: Entity) -> Result<Vec<String>> {
    with_world(lua, |w| {
        let r = w
            .scene
            .world
            .get::<&SkinnedMeshRenderer>(e)
            .map_err(|_| gone())?;
        let Some(mesh) = r.mesh.clone() else {
            return Ok(Vec::new());
        };
        if let Some(n) = crate::skinning::blend_shape_names(&mesh) {
            return Ok(n);
        }
        let abs = mesh.split_asset().map(|(p, i)| {
            let p = w.root.join(p).to_string_lossy().replace('\\', "/");
            crate::scene::MeshRef::asset(&p, i)
        });
        Ok(abs
            .as_ref()
            .and_then(crate::skinning::blend_shape_names)
            .unwrap_or_else(|| vec![String::new(); r.blend_shape_weights.len()]))
    })?
}

/// Lua indices are the engine's (0-based), like the reference API.
fn index(i: i64) -> Result<usize> {
    usize::try_from(i)
        .map_err(|_| mlua::Error::runtime(format!("blend shape index {i} out of range")))
}

impl UserData for LSkinnedMeshRenderer {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("enabled", |lua, r| get(lua, r.0, |c| c.enabled));
        f.add_field_method_set("enabled", |lua, r, v: bool| {
            set(lua, r.0, |c| c.enabled = v)
        });
        f.add_field_method_get("updateWhenOffscreen", |lua, r| {
            get(lua, r.0, |c| c.update_when_offscreen)
        });
        f.add_field_method_set("updateWhenOffscreen", |lua, r, v: bool| {
            set(lua, r.0, |c| c.update_when_offscreen = v)
        });
        f.add_field_method_get("blendShapeCount", |lua, r| Ok(shape_names(lua, r.0)?.len()));
        f.add_field_method_get("rootBone", |lua, r| {
            get(lua, r.0, |c| c.root_bone.map(LGameObject))
        });
        f.add_field_method_get("gameObject", |_, r| Ok(LGameObject(r.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("GetBlendShapeWeight", |lua, r, i: i64| {
            let i = index(i)?;
            get(lua, r.0, |c| c.blend_shape_weight(i))
        });
        m.add_method("SetBlendShapeWeight", |lua, r, (i, v): (i64, f32)| {
            let i = index(i)?;
            let count = shape_names(lua, r.0)?.len();
            if i >= count {
                return Err(mlua::Error::runtime(format!(
                    "blend shape index {i} out of range (the mesh has {count})"
                )));
            }
            set(lua, r.0, |c| c.set_blend_shape_weight(i, v))
        });
        m.add_method("GetBlendShapeIndex", |lua, r, name: String| {
            let names = shape_names(lua, r.0)?;
            Ok(names
                .iter()
                .position(|s| *s == name)
                .map_or(-1, |i| i as i64))
        });
        m.add_method("GetBlendShapeName", |lua, r, i: i64| {
            let i = index(i)?;
            Ok(shape_names(lua, r.0)?.get(i).cloned())
        });
    }
}

/// `GetComponent("SkinnedMeshRenderer")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "SkinnedMeshRenderer"
        || !with_world(lua, |w| {
            w.scene.world.get::<&SkinnedMeshRenderer>(e).is_ok()
        })?
    {
        return Ok(None);
    }
    Ok(Some(Value::UserData(
        lua.create_userdata(LSkinnedMeshRenderer(e))?,
    )))
}
