//! Behaviour trees in a running game: the per-frame tick of every Behaviour Tree Agent, the host
//! that lets trees move agents, play animations, look, call Lua and send events to Script
//! Graphs, and the `BehaviourTree` Lua table (also the Script Graph nodes of that name).
//!
//! - `BehaviourTree.GetValue(gameObject, key)` / `SetValue(gameObject, key, value)` /
//!   `HasKey(gameObject, key)`: the agent's blackboard (shared keys reach the share group).
//! - `BehaviourTree.Start(gameObject)` / `Stop` / `Restart`, `IsRunning`, `GetStatus`.
//! - `BehaviourTree.Finish(gameObject, success)`: ends a waiting Run Script Graph node.
//!
//! Custom Lua leaves are scripts that `return` a table with `OnStart(self)`, `OnTick(self, dt)`
//! and `OnAbort(self)`; `self` holds `gameObject`, `transform`, `params` (the node's settings)
//! and `node`. Returning "running" keeps the leaf running, false or "failure" fails it, and
//! anything else (or nothing) succeeds.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::{Quat, Vec3};
use hecs::Entity;
use mlua::{AnyUserData, Function, Lua, Result, Table, Value};

use super::api::{LGameObject, LTransform, LVec3, with_world};
use super::{LogKind, ScriptHost, World};
use crate::behaviour_tree::asset::LogLevel;
use crate::behaviour_tree::blackboard::{BbValue, Blackboard, ObjRef, SharedBlackboards};
use crate::behaviour_tree::exec::{Host, LeafPhase, Status};
use crate::behaviour_tree::runtime::{Command, RunState};
use crate::behaviour_tree::{BehaviourTreeAgent, BtRuntime};

/// Registry tables: custom leaf modules by path, and leaf instances by object and node.
const MODULES: &str = "misty.bt.modules";
const LEAVES: &str = "misty.bt.leaves";

fn entity_of(go: &AnyUserData) -> Result<Entity> {
    Ok(go.borrow::<LGameObject>()?.0)
}

fn no_agent() -> mlua::Error {
    mlua::Error::runtime("the object has no running Behaviour Tree Agent")
}

/// A blackboard value for Lua.
fn to_lua(lua: &Lua, w: &World, v: &BbValue) -> Result<Value> {
    Ok(match v {
        BbValue::Bool(b) => Value::Boolean(*b),
        BbValue::Int(i) => Value::Integer(*i),
        BbValue::Float(f) => Value::Number(f64::from(*f)),
        BbValue::Vector(v) => Value::UserData(lua.create_userdata(LVec3(*v))?),
        BbValue::Object(ObjRef(Some(e))) if w.scene.contains(*e) => {
            Value::UserData(lua.create_userdata(LGameObject(*e))?)
        }
        BbValue::Object(_) => Value::Nil,
        BbValue::String(s) => Value::String(lua.create_string(s)?),
    })
}

/// A Lua value as a blackboard value (nil = no object).
fn from_lua(v: &Value) -> Result<BbValue> {
    Ok(match v {
        Value::Nil => BbValue::Object(ObjRef(None)),
        Value::Boolean(b) => BbValue::Bool(*b),
        Value::Integer(i) => BbValue::Int(*i),
        Value::Number(n) => BbValue::Float(*n as f32),
        Value::String(s) => BbValue::String(s.to_str()?.to_owned()),
        Value::UserData(ud) => {
            if let Ok(v) = ud.borrow::<LVec3>() {
                BbValue::Vector(v.0)
            } else if let Ok(g) = ud.borrow::<LGameObject>() {
                BbValue::Object(ObjRef(Some(g.0)))
            } else {
                return Err(mlua::Error::runtime(
                    "a blackboard value is a bool, number, string, Vector3 or GameObject",
                ));
            }
        }
        _ => {
            return Err(mlua::Error::runtime(
                "a blackboard value is a bool, number, string, Vector3 or GameObject",
            ));
        }
    })
}

/// Lua's answer from a leaf as a status (`nil_is` for nothing returned).
fn status_of(v: &Value, nil_is: Status) -> Status {
    match v {
        Value::Nil => nil_is,
        Value::Boolean(true) => Status::Success,
        Value::Boolean(false) => Status::Failure,
        Value::String(s) => match s.to_str().map(|s| s.to_ascii_lowercase()).as_deref() {
            Ok("running") => Status::Running,
            Ok("failure" | "failed" | "fail") => Status::Failure,
            _ => Status::Success,
        },
        _ => Status::Success,
    }
}

fn agent_command(lua: &Lua, go: AnyUserData, cmd: Command) -> Result<()> {
    let e = entity_of(&go)?;
    with_world(lua, |w| {
        let a = w.behaviour.agent_mut(e).ok_or_else(no_agent)?;
        a.command = Some(cmd);
        Ok(())
    })?
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    let t = lua.create_table()?;
    // GetValue(gameObject, key): the value of a blackboard key (nil when unset or missing).
    t.set(
        "GetValue",
        lua.create_function(|lua, (go, key): (AnyUserData, String)| {
            let e = entity_of(&go)?;
            with_world(lua, |w| {
                let Some(a) = w.behaviour.agent(e) else {
                    return Ok(Value::Nil);
                };
                match a.bb.get_named(&w.behaviour.shared, &key) {
                    Some(v) => to_lua(lua, w, v),
                    None => Ok(Value::Nil),
                }
            })?
        })?,
    )?;
    // SetValue(gameObject, key, value): write a blackboard key (numbers convert between Int and
    // Float; aborts watching the key react on the next tick).
    t.set(
        "SetValue",
        lua.create_function(|lua, (go, key, value): (AnyUserData, String, Value)| {
            let e = entity_of(&go)?;
            let v = from_lua(&value)?;
            with_world(lua, |w| {
                let BtRuntime { agents, shared, .. } = &mut w.behaviour;
                let i = agents
                    .iter()
                    .position(|a| a.entity == e)
                    .ok_or_else(no_agent)?;
                agents[i]
                    .bb
                    .set_named(shared, &key, &v)
                    .map(|_| ())
                    .map_err(mlua::Error::runtime)
            })?
        })?,
    )?;
    // HasKey(gameObject, key): whether the agent's tree declares the key.
    t.set(
        "HasKey",
        lua.create_function(|lua, (go, key): (AnyUserData, String)| {
            let e = entity_of(&go)?;
            with_world(lua, |w| {
                w.behaviour
                    .agent(e)
                    .is_some_and(|a| a.bb.index(&key).is_some())
            })
        })?,
    )?;
    // Start(gameObject): start a stopped or finished tree (from the Root) on the next tick.
    t.set(
        "Start",
        lua.create_function(|lua, go: AnyUserData| agent_command(lua, go, Command::Start))?,
    )?;
    // Stop(gameObject): stop the tree (running actions stop) until Start or Restart.
    t.set(
        "Stop",
        lua.create_function(|lua, go: AnyUserData| agent_command(lua, go, Command::Stop))?,
    )?;
    // Restart(gameObject): start the tree over from the Root.
    t.set(
        "Restart",
        lua.create_function(|lua, go: AnyUserData| agent_command(lua, go, Command::Restart))?,
    )?;
    // IsRunning(gameObject): the tree is ticking (not stopped or finished).
    t.set(
        "IsRunning",
        lua.create_function(|lua, go: AnyUserData| {
            let e = entity_of(&go)?;
            with_world(lua, |w| {
                w.behaviour
                    .agent(e)
                    .is_some_and(|a| a.run == RunState::Running)
            })
        })?,
    )?;
    // GetStatus(gameObject): "Running", "Success", "Failure", "Stopped" or "Idle".
    t.set(
        "GetStatus",
        lua.create_function(|lua, go: AnyUserData| {
            let e = entity_of(&go)?;
            with_world(lua, |w| match w.behaviour.agent(e) {
                Some(a) => match a.run {
                    RunState::Stopped => "Stopped",
                    RunState::Done(s) => s.label(),
                    RunState::Running => a.exec.result.label(),
                },
                None => "Idle",
            })
        })?,
    )?;
    // Finish(gameObject, success): end the agent's Run Script Graph node that waits.
    t.set(
        "Finish",
        lua.create_function(|lua, (go, success): (AnyUserData, Option<bool>)| {
            let e = entity_of(&go)?;
            with_world(lua, |w| {
                let a = w.behaviour.agent_mut(e).ok_or_else(no_agent)?;
                a.finishes.push((0, success.unwrap_or(true)));
                Ok(())
            })?
        })?,
    )?;
    g.set("BehaviourTree", t)?;
    Ok(())
}

/// The world as one agent's tree sees it.
struct GameHost<'a> {
    lua: &'a Lua,
    agent: usize,
    e: Entity,
    root: &'a PathBuf,
}

impl GameHost<'_> {
    fn world<R>(&self, f: impl FnOnce(&mut World) -> R) -> R {
        let mut w = self.lua.app_data_mut::<World>().expect("world is installed");
        f(&mut w)
    }

    fn name(&self) -> String {
        self.world(|w| w.scene.name(self.e))
    }

    /// The object with the Animator: this one, else a child.
    fn animated(&self, w: &World) -> Entity {
        if w.scene
            .world
            .get::<&crate::scene::Animator>(self.e)
            .is_ok()
        {
            return self.e;
        }
        w.scene
            .world
            .get::<&crate::scene::Node>(self.e)
            .ok()
            .and_then(|n| {
                n.children
                    .iter()
                    .copied()
                    .find(|c| w.scene.world.get::<&crate::scene::Animator>(*c).is_ok())
            })
            .unwrap_or(self.e)
    }

    /// The custom leaf module at `script` (loaded once; nil after an error).
    fn module(&self, script: &str) -> Option<Table> {
        let lua = self.lua;
        let modules: Table = match lua.named_registry_value::<Option<Table>>(MODULES).ok()? {
            Some(t) => t,
            None => {
                let t = lua.create_table().ok()?;
                lua.set_named_registry_value(MODULES, t.clone()).ok()?;
                t
            }
        };
        match modules.get::<Value>(script).ok()? {
            Value::Table(t) => return Some(t),
            Value::Boolean(false) => return None,
            _ => {}
        }
        match super::load_class(lua, self.root, script) {
            Ok(t) => {
                let _ = modules.set(script, t.clone());
                Some(t)
            }
            Err(e) => {
                self.world(|w| {
                    w.logs
                        .push((LogKind::Error, format!("Behaviour Tree leaf {e}")))
                });
                let _ = modules.set(script, false);
                None
            }
        }
    }

    /// The leaf instance table of (object, node), made on Start.
    fn instance(
        &self,
        node: u32,
        module: &Table,
        params: &BTreeMap<String, String>,
        create: bool,
    ) -> Option<Table> {
        let lua = self.lua;
        let all: Table = match lua.named_registry_value::<Option<Table>>(LEAVES).ok()? {
            Some(t) => t,
            None => {
                let t = lua.create_table().ok()?;
                lua.set_named_registry_value(LEAVES, t.clone()).ok()?;
                t
            }
        };
        let key = self.e.to_bits().get() as i64;
        let mine: Table = match all.get::<Option<Table>>(key).ok()? {
            Some(t) => t,
            None => {
                let t = lua.create_table().ok()?;
                all.set(key, t.clone()).ok()?;
                t
            }
        };
        if !create && let Ok(Some(t)) = mine.get::<Option<Table>>(node) {
            return Some(t);
        }
        let build = || -> Result<Table> {
            let t = lua.create_table()?;
            t.set("gameObject", LGameObject(self.e))?;
            t.set("transform", LTransform(self.e))?;
            t.set("node", node)?;
            let p = lua.create_table()?;
            for (k, v) in params {
                let value = if let Ok(n) = v.trim().parse::<f64>() {
                    Value::Number(n)
                } else if v == "true" || v == "false" {
                    Value::Boolean(v == "true")
                } else {
                    Value::String(lua.create_string(v)?)
                };
                p.set(k.as_str(), value)?;
            }
            t.set("params", p)?;
            let meta = lua.create_table()?;
            meta.set("__index", module.clone())?;
            t.set_metatable(Some(meta))?;
            mine.set(node, t.clone())?;
            Ok(t)
        };
        build().ok()
    }

    fn call(&self, what: &str, f: Function, args: impl mlua::IntoLuaMulti) -> Option<Value> {
        match f.call::<Value>(args) {
            Ok(v) => Some(v),
            Err(e) => {
                let name = self.name();
                self.world(|w| {
                    w.logs
                        .push((LogKind::Error, format!("'{name}' Behaviour Tree {what}: {e}")))
                });
                None
            }
        }
    }
}

impl Host for GameHost<'_> {
    fn with_bb<R>(&mut self, f: impl FnOnce(&mut Blackboard, &mut SharedBlackboards) -> R) -> R {
        let i = self.agent;
        self.world(|w| {
            let BtRuntime { agents, shared, .. } = &mut w.behaviour;
            f(&mut agents[i].bb, shared)
        })
    }

    fn position(&mut self) -> Vec3 {
        self.world(|w| w.scene.world_position(self.e))
    }

    fn forward(&mut self) -> Vec3 {
        self.world(|w| w.scene.world_rotation(self.e) * Vec3::Z)
    }

    fn object_position(&mut self, e: Entity) -> Option<Vec3> {
        self.world(|w| {
            (w.scene.contains(e) && w.scene.active_in_hierarchy(e))
                .then(|| w.scene.world_position(e))
        })
    }

    fn move_start(&mut self, dest: Vec3) -> bool {
        self.world(|w| {
            let World {
                scene, navigation, ..
            } = w;
            if let Some(st) = navigation.agent(scene, self.e) {
                st.is_stopped = false;
            }
            navigation.set_destination(scene, self.e, dest)
        })
    }

    fn move_progress(&mut self) -> (f32, bool) {
        self.world(|w| {
            let stop = w
                .scene
                .world
                .get::<&crate::navigation::NavMeshAgent>(self.e)
                .map_or(0.0, |a| a.stopping_distance.max(0.0));
            match w.navigation.state(self.e) {
                Some(st) if st.path_pending => (f32::INFINITY, true),
                Some(st) if st.has_path() => ((st.remaining_distance() - stop).max(0.0), true),
                _ => (0.0, false),
            }
        })
    }

    fn move_stop(&mut self) {
        self.world(|w| {
            let World {
                scene, navigation, ..
            } = w;
            navigation.reset_path(scene, self.e);
        });
    }

    fn turn_towards(&mut self, point: Vec3, max_degrees: f32) -> f32 {
        self.world(|w| {
            let here = w.scene.world_position(self.e);
            let to = Vec3::new(point.x - here.x, 0.0, point.z - here.z);
            if to.length_squared() < 1e-6 {
                return 0.0;
            }
            let rot = w.scene.world_rotation(self.e);
            let fwd = rot * Vec3::Z;
            let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
            let want = to.normalize();
            let (yaw_now, yaw_want) = (flat.x.atan2(flat.z), want.x.atan2(want.z));
            let mut diff = (yaw_want - yaw_now).to_degrees();
            while diff > 180.0 {
                diff -= 360.0;
            }
            while diff < -180.0 {
                diff += 360.0;
            }
            let step = diff.clamp(-max_degrees, max_degrees);
            if step != 0.0 {
                let q = Quat::from_rotation_y(step.to_radians()) * rot;
                w.scene.set_world_rotation(self.e, q);
            }
            (diff - step).abs()
        })
    }

    fn line_clear(&mut self, from: Vec3, to: Vec3, target: Option<Entity>) -> bool {
        let me = self.e;
        self.world(|w| {
            let scene = &w.scene;
            let ignored = |e: Entity| {
                let mut cur = Some(e);
                // The agent's and the target's own colliders (and their children's) don't block.
                for _ in 0..32 {
                    let Some(c) = cur else { break };
                    if c == me || Some(c) == target {
                        return true;
                    }
                    cur = scene.parent(c);
                }
                false
            };
            w.physics
                .line_hit(from, to, crate::physics::DEFAULT_RAYCAST_LAYERS, |h| {
                    !ignored(h.entity) && !h.body.is_some_and(ignored)
                })
                .is_none()
        })
    }

    fn play_animation(&mut self, state: &str, crossfade: f32) -> bool {
        self.world(|w| {
            let e = self.animated(w);
            let World {
                scene, animator, ..
            } = w;
            if crossfade > 0.0 {
                animator.cross_fade(scene, e, state, crossfade, true, -1, 0.0)
            } else {
                animator.play(scene, e, state, -1, None)
            }
        })
    }

    fn set_animator_parameter(&mut self, name: &str, value: &BbValue) -> bool {
        let Some(v) = value.as_f64() else {
            return false;
        };
        self.world(|w| {
            let e = self.animated(w);
            let World {
                scene, animator, ..
            } = w;
            use crate::animation::controller::ParamKind;
            animator.set_param(scene, e, name, &ParamKind::ALL, v as f32);
            true
        })
    }

    fn send_event(&mut self, _node: u32, _graph: &str, event: &str) {
        let e = self.e;
        self.world(|w| {
            w.graph_events
                .push(crate::visual_script::runtime::CustomEvent {
                    target: Some(e),
                    name: std::rc::Rc::from(event),
                    args: Vec::new(),
                })
        });
    }

    fn take_finish(&mut self, node: u32) -> Option<bool> {
        let i = self.agent;
        self.world(|w| {
            let f = &mut w.behaviour.agents[i].finishes;
            let k = f.iter().position(|(n, _)| *n == node || *n == 0)?;
            Some(f.swap_remove(k).1)
        })
    }

    fn call_lua(&mut self, node: u32, function: &str) -> Status {
        let lua = self.lua;
        let found = (|| -> Option<(Table, Function)> {
            let all: Table = lua.named_registry_value(super::SCRIPT_INSTANCES).ok()?;
            let mine: Table = all.get(self.e.to_bits().get() as i64).ok()?;
            for pair in mine.pairs::<Value, Table>() {
                let (_, t) = pair.ok()?;
                if let Ok(Some(f)) = t.get::<Option<Function>>(function) {
                    return Some((t, f));
                }
            }
            None
        })();
        let Some((t, f)) = found else {
            let name = self.name();
            self.world(|w| {
                w.logs.push((
                    LogKind::Warning,
                    format!("'{name}': Run Lua: no script on the object has a function '{function}' (node {node})"),
                ))
            });
            return Status::Failure;
        };
        let dt = self.world(|w| w.time.delta);
        match self.call(function, f, (t, dt)) {
            Some(v) => status_of(&v, Status::Success),
            None => Status::Failure,
        }
    }

    fn custom_lua(
        &mut self,
        node: u32,
        phase: LeafPhase,
        script: &str,
        params: &BTreeMap<String, String>,
        dt: f32,
    ) -> Status {
        let Some(module) = self.module(script) else {
            return Status::Failure;
        };
        let Some(inst) = self.instance(node, &module, params, phase == LeafPhase::Start) else {
            return Status::Failure;
        };
        let (name, nil_is) = match phase {
            LeafPhase::Start => ("OnStart", Status::Running),
            LeafPhase::Tick => ("OnTick", Status::Success),
            LeafPhase::Abort => ("OnAbort", Status::Failure),
        };
        let r = match module.get::<Option<Function>>(name) {
            Ok(Some(f)) => match self.call(name, f, (inst, dt)) {
                Some(v) => status_of(&v, nil_is),
                None => Status::Failure,
            },
            _ => nil_is,
        };
        if phase == LeafPhase::Abort || (phase == LeafPhase::Tick && r.finished()) {
            // Done with this run: the next Start makes a fresh instance.
            if let Ok(Some(all)) = self.lua.named_registry_value::<Option<Table>>(LEAVES)
                && let Ok(Some(mine)) = all.get::<Option<Table>>(self.e.to_bits().get() as i64)
            {
                let _ = mine.set(node, Value::Nil);
            }
        }
        r
    }

    fn log(&mut self, level: LogLevel, text: &str) {
        let kind = match level {
            LogLevel::Info => LogKind::Info,
            LogLevel::Warning => LogKind::Warning,
            LogLevel::Error => LogKind::Error,
        };
        let text = text.to_owned();
        self.world(|w| w.logs.push((kind, text)));
    }
}

impl ScriptHost {
    /// Pick up new Behaviour Tree Agents: blackboards, and the Script Graphs their trees send
    /// events to are added to the object when it doesn't run them yet.
    pub(super) fn sync_behaviour_trees(&mut self) {
        let fresh = self.with_world(|w| {
            let World {
                scene, behaviour, ..
            } = w;
            let fresh = behaviour.sync(scene);
            for l in behaviour.take_logs() {
                w.logs.push((LogKind::Warning, l));
            }
            fresh
        });
        for (e, graphs) in fresh {
            for g in graphs {
                let slot = self.with_world(|w| {
                    let has = w
                        .scene
                        .world
                        .get::<&crate::scene::Scripts>(e)
                        .is_ok_and(|s| s.list.iter().any(|r| *r.path == *g));
                    if has {
                        return None;
                    }
                    let r = crate::scene::ScriptRef {
                        path: g.to_string(),
                        ..Default::default()
                    };
                    match w.scene.world.get::<&mut crate::scene::Scripts>(e) {
                        Ok(mut s) => {
                            s.list.push(r.clone());
                            Some((s.list.len() - 1, r))
                        }
                        Err(_) => {
                            let s = crate::scene::Scripts { list: vec![r.clone()] };
                            let _ = w.scene.world.insert_one(e, s);
                            Some((0, r))
                        }
                    }
                });
                if let Some((slot, r)) = slot {
                    self.instantiate(e, slot, &r);
                }
            }
        }
    }

    /// One frame of every Behaviour Tree Agent (after Script Graphs' Update, before navigation).
    pub(super) fn update_behaviour_trees(&mut self, dt: f32) {
        self.sync_behaviour_trees();
        let count = self.with_world(|w| w.behaviour.agents.len());
        if count == 0 {
            return;
        }
        let root = self.root.clone();
        for i in 0..count {
            // Take the agent's tree memory out so the host can reach the world (and Lua the
            // blackboard) while it ticks.
            let job = self.with_world(|w| {
                let e = w.behaviour.agents[i].entity;
                let (enabled, interval) = match w.scene.world.get::<&BehaviourTreeAgent>(e) {
                    Ok(c) => (c.enabled, c.interval()),
                    Err(_) => (false, 0.0),
                };
                let restart = w
                    .scene
                    .world
                    .get::<&BehaviourTreeAgent>(e)
                    .is_ok_and(|c| c.restart_when_done);
                let active = enabled && w.scene.contains(e) && w.scene.active_in_hierarchy(e);
                let a = &mut w.behaviour.agents[i];
                let exec = std::mem::take(&mut a.exec);
                (e, a.tree.clone(), exec, a.command.take(), active, interval, restart)
            });
            let (e, tree, mut exec, command, active, interval, restart) = job;
            let mut host = GameHost {
                lua: &self.lua,
                agent: i,
                e,
                root: &root,
            };
            let mut run = self.with_world(|w| w.behaviour.agents[i].run);
            match command {
                Some(Command::Stop) => {
                    exec.reset(&tree, &mut host);
                    run = RunState::Stopped;
                }
                Some(Command::Restart) => {
                    exec.reset(&tree, &mut host);
                    run = RunState::Running;
                }
                Some(Command::Start) if run != RunState::Running => {
                    exec.reset(&tree, &mut host);
                    run = RunState::Running;
                }
                _ => {}
            }
            let mut ticked = None;
            if !active {
                // A disabled agent stops what it was doing and starts over when enabled.
                if exec.result == Status::Running {
                    exec.reset(&tree, &mut host);
                }
            } else if run == RunState::Running {
                let accum = self.with_world(|w| {
                    let a = &mut w.behaviour.agents[i];
                    a.accum += dt;
                    a.accum
                });
                if accum >= interval {
                    ticked = Some(exec.tick(&tree, &mut host, accum));
                    self.with_world(|w| w.behaviour.agents[i].accum = 0.0);
                }
            }
            if let Some(r) = ticked
                && r.finished()
                && !restart
            {
                run = RunState::Done(r);
            }
            self.with_world(|w| {
                let a = &mut w.behaviour.agents[i];
                a.exec = exec;
                a.run = run;
            });
        }
        // Events the trees sent reach graphs and scripts now.
        self.flush_spawned();
    }

    /// The debugger's view of the agent on `watch` running `path` (else the first one).
    pub fn behaviour_debug(
        &self,
        path: &str,
        watch: Option<Entity>,
    ) -> Option<crate::behaviour_tree::runtime::DebugView> {
        self.with_world(|w| w.behaviour.debug_view(path, watch))
    }

    /// Objects running the tree at `path`.
    pub fn behaviour_agents(&self, path: &str) -> Vec<Entity> {
        self.with_world(|w| w.behaviour.agents_of(path))
    }

    /// Run `f` on the game's behaviour trees between frames (tests, tools).
    pub fn with_behaviour<R>(&self, f: impl FnOnce(&mut BtRuntime) -> R) -> R {
        self.with_world(|w| f(&mut w.behaviour))
    }
}
