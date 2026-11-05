//! Lua API of tilemaps and 2D lights: `Tilemap` (SetTile, GetTile, HasTile, ClearAllTiles,
//! WorldToCell, CellToWorld, GetCellCenterWorld, color) and `Light2D` (intensity, color,
//! radii, enabled). Tiles are named by their asset path ("Assets/Tiles/Ground.tile").
//!
//! Cell positions are a Vector3 (z ignored) or two numbers; cells come back as a Vector3 with
//! whole x and y.

use glam::{IVec2, Vec3};
use hecs::Entity;
use mlua::{FromLua, Lua, Result, UserData, UserDataFields, UserDataMethods, Value, Variadic};

use super::api::{LGameObject, LVec3, with_world};
use super::ui_api::{LColor, get, set};
use crate::light2d::Light2D;
use crate::tilemap::{self, TileTransform, Tilemap};

/// A cell from leading arguments (`x, y` or a Vector3/table); returns it and the arguments left.
fn cell_args<'a>(lua: &Lua, args: &'a [Value]) -> Result<(IVec2, &'a [Value])> {
    let num = |v: &Value| match v {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(n) => Some(*n as f32),
        _ => None,
    };
    match args {
        [a, b, rest @ ..] if num(a).is_some() && num(b).is_some() => Ok((
            IVec2::new(
                num(a).unwrap_or(0.0).floor() as i32,
                num(b).unwrap_or(0.0).floor() as i32,
            ),
            rest,
        )),
        [v, rest @ ..] => {
            let p = LVec3::from_lua(v.clone(), lua)?.0;
            Ok((p.truncate().floor().as_ivec2(), rest))
        }
        [] => Err(mlua::Error::runtime("expected a cell (x, y or a Vector3)")),
    }
}

fn cell_value(c: IVec2) -> LVec3 {
    LVec3(Vec3::new(c.x as f32, c.y as f32, 0.0))
}

#[derive(Clone, Copy)]
pub struct LTilemap(pub Entity);

impl UserData for LTilemap {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("color", |lua, t| {
            get(lua, t.0, |m: &Tilemap| LColor(m.color))
        });
        f.add_field_method_set("color", |lua, t, c: LColor| {
            set(lua, t.0, |m: &mut Tilemap| m.color = c.0)
        });
        f.add_field_method_get("gameObject", |_, t| Ok(LGameObject(t.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("SetTile", |lua, t, args: Variadic<Value>| {
            let (c, rest) = cell_args(lua, &args)?;
            let tile = match rest.first() {
                Some(Value::String(s)) => Some(s.to_str()?.to_owned()),
                Some(Value::Nil) | None => None,
                Some(other) => {
                    return Err(mlua::Error::runtime(format!(
                        "SetTile: expected a tile path or nil, got {}",
                        other.type_name()
                    )));
                }
            };
            set(lua, t.0, |m: &mut Tilemap| {
                m.set(c, tile.as_deref(), TileTransform::default());
            })
        });
        m.add_method("GetTile", |lua, t, args: Variadic<Value>| {
            let (c, _) = cell_args(lua, &args)?;
            get(lua, t.0, |m: &Tilemap| m.tile_at(c).map(str::to_owned))
        });
        m.add_method("HasTile", |lua, t, args: Variadic<Value>| {
            let (c, _) = cell_args(lua, &args)?;
            get(lua, t.0, |m: &Tilemap| m.get(c).is_some())
        });
        m.add_method("ClearAllTiles", |lua, t, ()| {
            set(lua, t.0, |m: &mut Tilemap| m.clear())
        });
        m.add_method("GetUsedTilesCount", |lua, t, ()| {
            get(lua, t.0, |m: &Tilemap| m.len())
        });
        m.add_method("WorldToCell", |lua, t, p: LVec3| {
            with_world(lua, |w| {
                cell_value(tilemap::world_to_cell(&w.scene, t.0, p.0))
            })
        });
        // The cell's lower-left corner, like the reference API (the centre is
        // GetCellCenterWorld).
        m.add_method("CellToWorld", |lua, t, args: Variadic<Value>| {
            let (c, _) = cell_args(lua, &args)?;
            with_world(lua, |w| {
                let g = tilemap::grid_of(&w.scene, t.0);
                let corner = g.cell_center(c) - g.cell_size * 0.5;
                LVec3(
                    w.scene
                        .world_matrix(t.0)
                        .transform_point3(corner.extend(0.0)),
                )
            })
        });
        m.add_method("GetCellCenterWorld", |lua, t, args: Variadic<Value>| {
            let (c, _) = cell_args(lua, &args)?;
            with_world(lua, |w| LVec3(tilemap::cell_to_world(&w.scene, t.0, c)))
        });
    }
}

#[derive(Clone, Copy)]
pub struct LLight2D(pub Entity);

impl UserData for LLight2D {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("intensity", |lua, l| {
            get(lua, l.0, |c: &Light2D| c.intensity)
        });
        f.add_field_method_set("intensity", |lua, l, v: f32| {
            set(lua, l.0, |c: &mut Light2D| c.intensity = v.max(0.0))
        });
        f.add_field_method_get("color", |lua, l| {
            get(lua, l.0, |c: &Light2D| {
                LColor([c.color[0], c.color[1], c.color[2], 1.0])
            })
        });
        f.add_field_method_set("color", |lua, l, v: LColor| {
            set(lua, l.0, |c: &mut Light2D| {
                c.color = [v.0[0], v.0[1], v.0[2]]
            })
        });
        f.add_field_method_get("pointLightOuterRadius", |lua, l| {
            get(lua, l.0, |c: &Light2D| c.outer_radius)
        });
        f.add_field_method_set("pointLightOuterRadius", |lua, l, v: f32| {
            set(lua, l.0, |c: &mut Light2D| c.outer_radius = v.max(0.0))
        });
        f.add_field_method_get("pointLightInnerRadius", |lua, l| {
            get(lua, l.0, |c: &Light2D| c.inner_radius)
        });
        f.add_field_method_set("pointLightInnerRadius", |lua, l, v: f32| {
            set(lua, l.0, |c: &mut Light2D| c.inner_radius = v.max(0.0))
        });
        f.add_field_method_get("enabled", |lua, l| get(lua, l.0, |c: &Light2D| c.enabled));
        f.add_field_method_set("enabled", |lua, l, v: bool| {
            set(lua, l.0, |c: &mut Light2D| c.enabled = v)
        });
        f.add_field_method_get("gameObject", |_, l| Ok(LGameObject(l.0)));
    }
}

/// `GetComponent` for "Tilemap" and "Light2D".
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |f: fn(&hecs::World, Entity) -> bool| with_world(lua, |w| f(&w.scene.world, e));
    let ud = |v: Result<mlua::AnyUserData>| v.map(|u| Some(Value::UserData(u)));
    match name {
        "Tilemap" if has(|w, e| w.get::<&Tilemap>(e).is_ok())? => {
            ud(lua.create_userdata(LTilemap(e)))
        }
        "Light2D" if has(|w, e| w.get::<&Light2D>(e).is_ok())? => {
            ud(lua.create_userdata(LLight2D(e)))
        }
        _ => Ok(None),
    }
}
