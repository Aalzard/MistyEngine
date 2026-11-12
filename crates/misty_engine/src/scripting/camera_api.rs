//! Lua `Camera.main` (first enabled camera tagged MainCamera) and `GetComponent("Camera")`
//! with `fieldOfView`, `nearClipPlane`, `farClipPlane`, `enabled`, `ScreenPointToRay` and
//! `WorldToScreenPoint`, and `Input.mousePosition` (pixels from the bottom-left, like the
//! familiar API).

use glam::{Vec2, Vec3};
use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::camera::{Camera, Projection};
use crate::scene::Camera as CameraComponent;

#[derive(Clone, Copy)]
pub struct LCamera(pub Entity);

/// Game image size (falls back to 16:9 before the first frame).
fn screen(w: &World) -> Vec2 {
    let s = w.input.screen_size;
    if s.x > 0.0 && s.y > 0.0 {
        s
    } else {
        Vec2::new(1920.0, 1080.0)
    }
}

/// View camera of object `e`.
fn camera_of(w: &World, e: Entity) -> Option<Camera> {
    let c = w.scene.world.get::<&CameraComponent>(e).ok()?;
    let (_, rotation, position) = w.scene.world_matrix(e).to_scale_rotation_translation();
    let projection = if c.orthographic {
        Projection::Orthographic { size: c.ortho_size }
    } else {
        Projection::Perspective { fov_y: c.fov }
    };
    Some(Camera {
        position,
        rotation,
        projection,
        near: c.near,
        far: c.far,
    })
}

fn main_camera(w: &World) -> Option<Entity> {
    w.scene.walk().into_iter().map(|(e, _)| e).find(|&e| {
        w.scene.active_in_hierarchy(e)
            && w.scene.game_object(e).tag == "MainCamera"
            && w.scene
                .world
                .get::<&CameraComponent>(e)
                .is_ok_and(|c| c.enabled)
    })
}

fn get<R>(lua: &Lua, e: Entity, f: impl FnOnce(&CameraComponent) -> R) -> Result<R> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&CameraComponent>(e)
            .map(|c| f(&c))
            .map_err(|_| super::api::missing())
    })?
}

fn set(lua: &Lua, e: Entity, f: impl FnOnce(&mut CameraComponent)) -> Result<()> {
    with_world(lua, |w| {
        w.scene
            .world
            .get::<&mut CameraComponent>(e)
            .map(|mut c| f(&mut c))
            .map_err(|_| super::api::missing())
    })?
}

impl UserData for LCamera {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, c| Ok(LGameObject(c.0)));
        f.add_field_method_get("transform", |_, c| Ok(LTransform(c.0)));
        // Vertical field of view in degrees (a sprint FOV kick lerps it).
        f.add_field_method_get("fieldOfView", |lua, c| get(lua, c.0, |cam| cam.fov));
        f.add_field_method_set("fieldOfView", |lua, c, fov: f32| {
            // NaN passes `clamp` and would poison the projection.
            if !fov.is_finite() {
                return Err(mlua::Error::runtime("fieldOfView must be a finite number"));
            }
            set(lua, c.0, |cam| cam.fov = fov.clamp(1.0, 179.0))
        });
        f.add_field_method_get("nearClipPlane", |lua, c| get(lua, c.0, |cam| cam.near));
        f.add_field_method_set("nearClipPlane", |lua, c, v: f32| {
            set(lua, c.0, |cam| cam.near = v.max(0.001))
        });
        f.add_field_method_get("farClipPlane", |lua, c| get(lua, c.0, |cam| cam.far));
        f.add_field_method_set("farClipPlane", |lua, c, v: f32| {
            set(lua, c.0, |cam| cam.far = v.max(0.002))
        });
        f.add_field_method_get("orthographic", |lua, c| {
            get(lua, c.0, |cam| cam.orthographic)
        });
        f.add_field_method_set("orthographic", |lua, c, on: bool| {
            set(lua, c.0, |cam| cam.orthographic = on)
        });
        f.add_field_method_get("orthographicSize", |lua, c| {
            get(lua, c.0, |cam| cam.ortho_size)
        });
        f.add_field_method_set("orthographicSize", |lua, c, v: f32| {
            set(lua, c.0, |cam| cam.ortho_size = v.max(0.001))
        });
        f.add_field_method_get("enabled", |lua, c| get(lua, c.0, |cam| cam.enabled));
        f.add_field_method_set("enabled", |lua, c, on: bool| {
            set(lua, c.0, |cam| cam.enabled = on)
        });
        // Layer bits the camera renders (`~(1 << layer)` hides one).
        f.add_field_method_get("cullingMask", |lua, c| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&CameraComponent>(c.0)
                    .map_or(0, |cam| cam.culling_mask as i64)
            })
        });
        f.add_field_method_set("cullingMask", |lua, c, mask: i64| {
            with_world(lua, |w| {
                if let Ok(mut cam) = w.scene.world.get::<&mut CameraComponent>(c.0) {
                    cam.culling_mask = mask as u32;
                }
            })
        });
        // Baked occlusion culling for this camera.
        f.add_field_method_get("useOcclusionCulling", |lua, c| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&CameraComponent>(c.0)
                    .is_ok_and(|cam| cam.occlusion_culling)
            })
        });
        f.add_field_method_set("useOcclusionCulling", |lua, c, on: bool| {
            with_world(lua, |w| {
                if let Ok(mut cam) = w.scene.world.get::<&mut CameraComponent>(c.0) {
                    cam.occlusion_culling = on;
                }
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Ray through a screen point (pixels, origin bottom-left): { origin, direction }.
        m.add_method("ScreenPointToRay", |lua, c, p: LVec3| {
            let ray = with_world(lua, |w| {
                let s = screen(w);
                let uv = Vec2::new(p.0.x / s.x, 1.0 - p.0.y / s.y);
                camera_of(w, c.0).map(|cam| cam.screen_ray(s.x / s.y, uv))
            })?;
            let Some((origin, direction)) = ray else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("origin", LVec3(origin))?;
            t.set("direction", LVec3(direction))?;
            Ok(Value::Table(t))
        });
        // Screen pixels (origin bottom-left) and depth in front of the camera.
        m.add_method("WorldToScreenPoint", |lua, c, p: LVec3| {
            with_world(lua, |w| {
                let s = screen(w);
                let cam = camera_of(w, c.0)?;
                let (uv, depth) = cam.world_to_screen(s.x / s.y, p.0)?;
                Some(LVec3(Vec3::new(uv.x * s.x, (1.0 - uv.y) * s.y, depth)))
            })
        });
    }
}

/// `GetComponent("Camera")`: the object's Camera, or nil.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "Camera" {
        return Ok(None);
    }
    let has = with_world(lua, |w| w.scene.world.get::<&CameraComponent>(e).is_ok())?;
    if !has {
        return Ok(None);
    }
    Ok(Some(Value::UserData(lua.create_userdata(LCamera(e))?)))
}

/// `Camera` global and `Input.mousePosition` (call after `Input` exists).
pub fn register(lua: &Lua) -> Result<()> {
    let statics = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| {
            if key != "main" {
                return Ok(None);
            }
            with_world(lua, |w| main_camera(w).map(LCamera))
        })?,
    )?;
    statics.set_metatable(Some(meta))?;
    lua.globals().set("Camera", statics)?;

    let input: Table = lua.globals().get("Input")?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| {
            if key != "mousePosition" {
                return Ok(None);
            }
            with_world(lua, |w| {
                let p = w.input.mouse_position.unwrap_or_default();
                Some(LVec3(Vec3::new(p.x, screen(w).y - p.y, 0.0)))
            })
        })?,
    )?;
    input.set_metatable(Some(meta))?;
    Ok(())
}
