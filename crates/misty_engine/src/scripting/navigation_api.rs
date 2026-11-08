//! Lua navigation API.
//!
//! - `GetComponent("NavMeshAgent")`: `SetDestination`, `destination`, `remainingDistance`,
//!   `hasPath`, `pathPending`, `pathStatus`, `isOnNavMesh`, `isOnOffMeshLink`,
//!   `currentOffMeshLinkData`, `CompleteOffMeshLink`, `autoTraverseOffMeshLink`, `isStopped`,
//!   `velocity`, `desiredVelocity`, `Warp`, `ResetPath`, `Move`, `areaMask`, `agentTypeID`,
//!   `SetAreaCost` / `GetAreaCost`, `CalculatePath`, `Raycast`, `FindClosestEdge`, settings.
//! - `GetComponent("NavMeshSurface")`: `BuildNavMesh()`, `UpdateNavMesh()` (only changed tiles).
//! - `GetComponent("NavMeshObstacle")`: `carving`, `enabled`, `carveOnlyStationary`.
//! - The `NavMesh` table: `SamplePosition`, `CalculatePath`, `Raycast`, `FindClosestEdge`,
//!   `GetAreaFromName`, `GetAreaCost` / `SetAreaCost`, `AllAreas`.
//! - Callbacks on the agent's scripts: `OnOffMeshLinkStart(link)`, `OnOffMeshLinkEnd(link)`.

use glam::Vec3;
use hecs::Entity;
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::World;
use super::api::{LGameObject, LTransform, LVec3, with_world};
use crate::navigation::{
    ALL_AREAS, EdgeHit, NavMeshAgent, NavMeshObstacle, NavMeshSurface, NavPath, ObstacleAvoidance,
    RaycastHit,
};

#[derive(Clone, Copy)]
pub struct LNavMeshAgent(pub Entity);

#[derive(Clone, Copy)]
pub struct LNavMeshSurface(pub Entity);

#[derive(Clone, Copy)]
pub struct LNavMeshObstacle(pub Entity);

fn missing() -> mlua::Error {
    mlua::Error::runtime("object was destroyed or has no NavMeshAgent")
}

/// Lua integer → area mask (-1 = every area).
fn mask_of(v: Option<i64>) -> u32 {
    v.map_or(ALL_AREAS, |m| m as u32)
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

/// A path as a list of corners with `status` (and `corners` for the familiar spelling).
fn path_table(lua: &Lua, path: &NavPath) -> Result<Table> {
    let t: Table = lua.create_sequence_from(path.corners.iter().map(|c| LVec3(*c)))?;
    t.set("status", path.status.name())?;
    let corners: Table = lua.create_sequence_from(path.corners.iter().map(|c| LVec3(*c)))?;
    t.set("corners", corners)?;
    Ok(t)
}

fn raycast_table(lua: &Lua, h: &RaycastHit) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("hit", h.hit)?;
    t.set("position", LVec3(h.position))?;
    t.set("normal", LVec3(h.normal))?;
    t.set("distance", h.distance)?;
    t.set("mask", 1i64 << h.area)?;
    Ok(t)
}

fn edge_table(lua: &Lua, h: &EdgeHit) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("hit", true)?;
    t.set("position", LVec3(h.position))?;
    t.set("normal", LVec3(h.normal))?;
    t.set("distance", h.distance)?;
    Ok(t)
}

/// (agent type, area mask, position) of an agent for queries.
fn agent_query(lua: &Lua, e: Entity) -> Result<(u32, u32, Vec3)> {
    let (t, m) = settings(lua, e, |c| (c.agent_type, c.area_mask))?;
    let p = state(lua, e, |s| s.position)?.unwrap_or_default();
    Ok((t, m, p))
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
        setting!(
            f,
            "autoTraverseOffMeshLink",
            auto_traverse_off_mesh_link,
            bool
        );
        setting!(f, "autoRepath", auto_repath, bool);
        setting!(f, "agentTypeID", agent_type, u32);
        f.add_field_method_get("areaMask", |lua, a| {
            settings(lua, a.0, |c| c.area_mask as i32 as i64)
        });
        f.add_field_method_set("areaMask", |lua, a, v: i64| {
            settings(lua, a.0, |c| c.area_mask = v as u32)
        });
        f.add_field_method_get("obstacleAvoidanceType", |lua, a| {
            settings(lua, a.0, |c| {
                ObstacleAvoidance::ALL
                    .iter()
                    .position(|q| *q == c.obstacle_avoidance)
                    .unwrap_or(0) as i64
            })
        });
        f.add_field_method_set("obstacleAvoidanceType", |lua, a, v: i64| {
            settings(lua, a.0, |c| {
                c.obstacle_avoidance = ObstacleAvoidance::ALL[v.clamp(0, 4) as usize];
            })
        });
        f.add_field_method_get("destination", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.destination))?.unwrap_or(LVec3(Vec3::ZERO)))
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
        // Paths are planned at once, except while the nav mesh under them is rebuilding or the
        // agent is crossing a link.
        f.add_field_method_get("pathPending", |lua, a| {
            Ok(state(lua, a.0, |s| s.path_pending)?.unwrap_or(false))
        });
        f.add_field_method_get("pathStatus", |lua, a| {
            Ok(state(lua, a.0, |s| s.status.name())?.unwrap_or("PathInvalid"))
        });
        f.add_field_method_get("isOnNavMesh", |lua, a| {
            Ok(state(lua, a.0, |s| s.on_mesh)?.unwrap_or(false))
        });
        f.add_field_method_get("isOnOffMeshLink", |lua, a| {
            Ok(state(lua, a.0, |s| s.is_on_off_mesh_link())?.unwrap_or(false))
        });
        f.add_field_method_get("currentOffMeshLinkData", |lua, a| {
            let link = state(lua, a.0, |s| s.link.clone())?.flatten();
            let t = lua.create_table()?;
            t.set("valid", link.is_some())?;
            t.set("activated", link.is_some())?;
            if let Some(l) = link {
                t.set("startPos", LVec3(l.start))?;
                t.set("endPos", LVec3(l.end))?;
                t.set("area", l.area)?;
            }
            Ok(t)
        });
        f.add_field_method_get("isStopped", |lua, a| {
            Ok(state(lua, a.0, |s| s.is_stopped)?.unwrap_or(false))
        });
        f.add_field_method_set("isStopped", |lua, a, v: bool| {
            state(lua, a.0, |s| s.is_stopped = v).map(|_| ())
        });
        f.add_field_method_get("velocity", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.velocity))?.unwrap_or(LVec3(Vec3::ZERO)))
        });
        f.add_field_method_set("velocity", |lua, a, v: LVec3| {
            state(lua, a.0, |s| s.velocity = Vec3::new(v.0.x, 0.0, v.0.z)).map(|_| ())
        });
        f.add_field_method_get("desiredVelocity", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.desired_velocity))?.unwrap_or(LVec3(Vec3::ZERO)))
        });
        f.add_field_method_get("nextPosition", |lua, a| {
            Ok(state(lua, a.0, |s| LVec3(s.position))?.unwrap_or(LVec3(Vec3::ZERO)))
        });
        f.add_field_method_get("steeringTarget", |lua, a| {
            let t = state(lua, a.0, |s| {
                s.corners.first().copied().unwrap_or(s.position)
            })?;
            Ok(LVec3(t.unwrap_or_default()))
        });
        f.add_field_method_get("path", |lua, a| {
            let corners = with_world(lua, |w| w.navigation.path_of(a.0))?.unwrap_or_default();
            lua.create_sequence_from(corners.into_iter().map(LVec3))
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
        m.add_method("Move", |lua, a, offset: LVec3| {
            let p = state(lua, a.0, |s| s.position)?.unwrap_or_default();
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.warp(scene, a.0, p + offset.0)
            })
        });
        m.add_method("CompleteOffMeshLink", |lua, a, ()| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.complete_off_mesh_link(scene, a.0)
            })
        });
        m.add_method("SetAreaCost", |lua, a, (area, cost): (usize, f32)| {
            with_world(lua, |w| {
                let costs: [f32; crate::navigation::AREA_COUNT] =
                    std::array::from_fn(|i| w.navigation.area_cost(i));
                let World {
                    scene, navigation, ..
                } = w;
                if let Some(st) = navigation.agent(scene, a.0)
                    && area < crate::navigation::AREA_COUNT
                {
                    let c = st.area_costs.get_or_insert(costs);
                    c[area] = cost.max(1.0);
                }
            })
        });
        m.add_method("GetAreaCost", |lua, a, area: usize| {
            let own = state(lua, a.0, |s| s.area_costs.map(|c| c.get(area).copied()))?
                .flatten()
                .flatten();
            match own {
                Some(c) => Ok(c),
                None => with_world(lua, |w| w.navigation.area_cost(area)),
            }
        });
        m.add_method("CalculatePath", |lua, a, target: LVec3| {
            let (t, mask, p) = agent_query(lua, a.0)?;
            let path = with_world(lua, |w| {
                w.navigation.calculate_path_filtered(p, target.0, mask, t)
            })?;
            path_table(lua, &path)
        });
        m.add_method("Raycast", |lua, a, target: LVec3| {
            let (t, mask, p) = agent_query(lua, a.0)?;
            let hit = with_world(lua, |w| w.navigation.raycast(p, target.0, mask, t))?;
            match hit {
                Some(h) => Ok((h.hit, Value::Table(raycast_table(lua, &h)?))),
                None => Ok((false, Value::Nil)),
            }
        });
        m.add_method("FindClosestEdge", |lua, a, ()| {
            let (t, mask, p) = agent_query(lua, a.0)?;
            let hit = with_world(lua, |w| w.navigation.closest_edge(p, mask, t))?;
            hit.map(|h| edge_table(lua, &h)).transpose()
        });
    }
}

impl UserData for LNavMeshSurface {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, s| Ok(LGameObject(s.0)));
        f.add_field_method_get("agentTypeID", |lua, s| {
            with_world(lua, |w| {
                w.scene
                    .world
                    .get::<&NavMeshSurface>(s.0)
                    .map_or(0, |c| c.agent_type)
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Rebuild every tile (in the background).
        m.add_method("BuildNavMesh", |lua, s, ()| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.rebuild_surface(scene, s.0, true)
            })
        });
        // Rebuild only tiles whose objects changed (in the background).
        m.add_method("UpdateNavMesh", |lua, s, ()| {
            with_world(lua, |w| {
                let World {
                    scene, navigation, ..
                } = w;
                navigation.rebuild_surface(scene, s.0, false)
            })
        });
    }
}

fn obstacle<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut NavMeshObstacle) -> R) -> Result<R> {
    with_world(lua, |w| {
        let mut o =
            w.scene.world.get::<&mut NavMeshObstacle>(e).map_err(|_| {
                mlua::Error::runtime("object was destroyed or has no NavMeshObstacle")
            })?;
        Ok(f(&mut o))
    })?
}

impl UserData for LNavMeshObstacle {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, o| Ok(LGameObject(o.0)));
        f.add_field_method_get("carving", |lua, o| obstacle(lua, o.0, |c| c.carve));
        f.add_field_method_set("carving", |lua, o, v: bool| {
            obstacle(lua, o.0, |c| c.carve = v)
        });
        f.add_field_method_get("enabled", |lua, o| obstacle(lua, o.0, |c| c.enabled));
        f.add_field_method_set("enabled", |lua, o, v: bool| {
            obstacle(lua, o.0, |c| c.enabled = v)
        });
        f.add_field_method_get("carveOnlyStationary", |lua, o| {
            obstacle(lua, o.0, |c| c.carve_only_stationary)
        });
        f.add_field_method_set("carveOnlyStationary", |lua, o, v: bool| {
            obstacle(lua, o.0, |c| c.carve_only_stationary = v)
        });
    }
}

/// `GetComponent("NavMeshAgent" | "NavMeshSurface" | "NavMeshObstacle")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    let has = |lua: &Lua, which: &str| {
        with_world(lua, |w| match which {
            "NavMeshAgent" => w.scene.world.get::<&NavMeshAgent>(e).is_ok(),
            "NavMeshSurface" => w.scene.world.get::<&NavMeshSurface>(e).is_ok(),
            _ => w.scene.world.get::<&NavMeshObstacle>(e).is_ok(),
        })
    };
    let ud = match name {
        "NavMeshAgent" if has(lua, name)? => lua.create_userdata(LNavMeshAgent(e))?,
        "NavMeshSurface" if has(lua, name)? => lua.create_userdata(LNavMeshSurface(e))?,
        "NavMeshObstacle" if has(lua, name)? => lua.create_userdata(LNavMeshObstacle(e))?,
        _ => return Ok(None),
    };
    Ok(Some(Value::UserData(ud)))
}

/// Register the `NavMesh` global.
pub fn register(lua: &Lua) -> Result<()> {
    let nav = lua.create_table()?;
    nav.set("AllAreas", -1i64)?;
    // SamplePosition(position, maxDistance [, areaMask [, agentTypeID]]) →
    // { position, distance, hit, mask } or nil.
    nav.set(
        "SamplePosition",
        lua.create_function(
            |lua, (p, max, mask, agent): (LVec3, f32, Option<i64>, Option<u32>)| {
                let hit = with_world(lua, |w| {
                    let h =
                        w.navigation
                            .sample_position(p.0, max, mask_of(mask), agent.unwrap_or(0));
                    h.map(|h| {
                        let area = w
                            .navigation
                            .meshes()
                            .find(|(m, _)| m.valid(h.poly) && m.contains_xz(h.poly, h.position))
                            .map_or(0, |(m, _)| m.area_of(h.poly));
                        (h, area)
                    })
                })?;
                let Some((h, area)) = hit else {
                    return Ok(Value::Nil);
                };
                let t = lua.create_table()?;
                t.set("position", LVec3(h.position))?;
                t.set("distance", h.distance)?;
                t.set("hit", true)?;
                t.set("mask", 1i64 << area)?;
                Ok(Value::Table(t))
            },
        )?,
    )?;
    // CalculatePath(from, to [, areaMask [, agentTypeID]]) → list of corners with `status`.
    nav.set(
        "CalculatePath",
        lua.create_function(
            |lua, (a, b, mask, agent): (LVec3, LVec3, Option<i64>, Option<u32>)| {
                let path = with_world(lua, |w| {
                    w.navigation.calculate_path_filtered(
                        a.0,
                        b.0,
                        mask_of(mask),
                        agent.unwrap_or(0),
                    )
                })?;
                path_table(lua, &path)
            },
        )?,
    )?;
    // Raycast(from, to [, areaMask [, agentTypeID]]) → blocked, { hit, position, normal,
    // distance, mask } (nil when `from` isn't near the mesh).
    nav.set(
        "Raycast",
        lua.create_function(
            |lua, (a, b, mask, agent): (LVec3, LVec3, Option<i64>, Option<u32>)| {
                let hit = with_world(lua, |w| {
                    w.navigation
                        .raycast(a.0, b.0, mask_of(mask), agent.unwrap_or(0))
                })?;
                match hit {
                    Some(h) => Ok((h.hit, Value::Table(raycast_table(lua, &h)?))),
                    None => Ok((false, Value::Nil)),
                }
            },
        )?,
    )?;
    // FindClosestEdge(position [, areaMask [, agentTypeID]]) → { position, normal, distance }.
    nav.set(
        "FindClosestEdge",
        lua.create_function(|lua, (p, mask, agent): (LVec3, Option<i64>, Option<u32>)| {
            let hit = with_world(lua, |w| {
                w.navigation
                    .closest_edge(p.0, mask_of(mask), agent.unwrap_or(0))
            })?;
            hit.map(|h| edge_table(lua, &h)).transpose()
        })?,
    )?;
    nav.set(
        "GetAreaFromName",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| {
                w.navigation
                    .settings()
                    .area_from_name(&name)
                    .map_or(-1, |i| i as i64)
            })
        })?,
    )?;
    nav.set(
        "GetAreaCost",
        lua.create_function(|lua, area: usize| with_world(lua, |w| w.navigation.area_cost(area)))?,
    )?;
    nav.set(
        "SetAreaCost",
        lua.create_function(|lua, (area, cost): (usize, f32)| {
            with_world(lua, |w| w.navigation.set_area_cost(area, cost))
        })?,
    )?;
    lua.globals().set("NavMesh", nav)
}

impl super::ScriptHost {
    /// Off-mesh link callbacks: `OnOffMeshLinkStart(link)` and `OnOffMeshLinkEnd(link)` on the
    /// agent's scripts (`link` has `startPos`, `endPos`, `area`).
    pub(super) fn dispatch_navigation_events(&mut self) {
        let events = self.with_world(|w| w.navigation.take_events());
        for ev in events {
            let name = match ev.kind {
                crate::navigation::agent::NavEventKind::LinkStart => "OnOffMeshLinkStart",
                crate::navigation::agent::NavEventKind::LinkEnd => "OnOffMeshLinkEnd",
            };
            let arg = (|| -> Result<Value> {
                let t = self.lua.create_table()?;
                t.set("startPos", LVec3(ev.start))?;
                t.set("endPos", LVec3(ev.end))?;
                t.set("area", ev.area)?;
                Ok(Value::Table(t))
            })()
            .ok();
            for i in 0..self.instances.len() {
                if self.instances[i].entity == ev.entity && self.runnable(&self.instances[i]) {
                    self.call(i, name, arg.clone());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "navigation_api_tests.rs"]
mod tests;
