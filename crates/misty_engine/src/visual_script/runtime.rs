//! Running graphs: one [`GraphInstance`] per Script Graph on an object, the events the script
//! host routes to them, Object variables, custom events, and the debugger (breakpoints,
//! stepping, the exec entries reached for active wires, and pin values).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use hecs::Entity;
use mlua::{AppDataRefMut, Lua};

use super::compile::{EventKind, InputMode, PortSlot, Program};
use super::value::Value;
use super::vm::{self, Exit, Flow, Hook, State};
use super::{FileResolver, ScriptGraph};
use crate::scene::{ScriptRef, Scripts};
use crate::scripting::{LogKind, World};

/// A custom event on its way: to one object's graphs and Lua scripts, or to every object.
#[derive(Clone, Debug)]
pub struct CustomEvent {
    /// None = every object.
    pub target: Option<Entity>,
    pub name: Rc<str>,
    pub args: Vec<Value>,
}

/// Object variables: values stored per GameObject by name, shared by its graphs and Lua
/// scripts (`gameObject:GetVariable(name)`).
#[derive(Debug, Default)]
pub struct ObjectVariables {
    map: HashMap<Entity, Vec<(Rc<str>, Value)>>,
}

impl ObjectVariables {
    pub fn get(&self, e: Entity, name: &str) -> Option<Value> {
        self.map
            .get(&e)?
            .iter()
            .find(|(n, _)| &**n == name)
            .map(|(_, v)| v.clone())
    }

    pub fn set(&mut self, e: Entity, name: &str, v: Value) {
        let list = self.map.entry(e).or_default();
        match list.iter_mut().find(|(n, _)| &**n == name) {
            Some((_, x)) => *x = v,
            None => list.push((Rc::from(name), v)),
        }
    }

    /// Index of `name` on `e`, created with `init` when missing.
    pub fn ensure(&mut self, e: Entity, name: &Rc<str>, init: Value) -> u32 {
        let list = self.map.entry(e).or_default();
        match list.iter().position(|(n, _)| n == name) {
            Some(i) => i as u32,
            None => {
                list.push((name.clone(), init));
                (list.len() - 1) as u32
            }
        }
    }

    pub fn at(&self, e: Entity, i: usize) -> Value {
        self.map
            .get(&e)
            .and_then(|l| l.get(i))
            .map_or(Value::Nil, |(_, v)| v.clone())
    }

    pub fn set_at(&mut self, e: Entity, i: usize, v: Value) {
        if let Some(x) = self.map.get_mut(&e).and_then(|l| l.get_mut(i)) {
            x.1 = v;
        }
    }

    pub fn remove(&mut self, e: Entity) {
        self.map.remove(&e);
    }

    /// Every variable of an object (name, value).
    pub fn of(&self, e: Entity) -> Vec<(Rc<str>, Value)> {
        self.map.get(&e).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
struct Timer {
    /// Index into the program's events.
    event: usize,
    next: f64,
    active: bool,
}

/// One Script Graph running on an object.
pub struct GraphInstance {
    pub entity: Entity,
    /// Index in the object's Scripts list.
    pub slot: usize,
    pub path: Rc<str>,
    pub prog: Rc<Program>,
    pub st: State,
    /// Program Object variable → index in the object's variables.
    pub obj_index: Vec<u32>,
    pub started: bool,
    /// Could run last time it was checked (OnEnable / OnDisable).
    pub enabled: bool,
    timers: Vec<Timer>,
}

/// A flow stopped at a breakpoint or step.
#[derive(Debug)]
pub struct Paused {
    pub entity: Entity,
    pub slot: usize,
    /// The node it stopped before.
    pub node: u32,
    flow: Flow,
}

/// What the editor's debugger asked for and what it reads back.
#[derive(Debug, Default)]
pub struct Debugger {
    /// The graph being debugged (the Script Graph window's graph); None = off.
    pub path: Option<String>,
    pub breakpoints: HashSet<u32>,
    /// Object whose instance the editor shows (else the first instance of the graph).
    pub watch: Option<Entity>,
    /// The instance shown: (entity, slot).
    pub watched: Option<(Entity, usize)>,
    pub paused: Option<Paused>,
    /// Exec entries the watched instance reached: (node, input pin) → frame.
    pub hits: HashMap<(u32, Rc<str>), u64>,
    /// Frames counted by the runtime (for `hits`).
    pub frame: u64,
    /// Run-time errors of the debugged graph: node → message.
    pub errors: HashMap<u32, String>,
}

/// Every Script Graph instance of a running game.
pub struct GraphRuntime {
    programs: HashMap<String, Option<Rc<Program>>>,
    pub instances: Vec<GraphInstance>,
    rng: u64,
    pub debug: Debugger,
    /// Errors already logged per (graph, node): at most [`MESSAGES_PER_NODE`] different
    /// messages each, since messages may carry run-time values ("index 41 is outside…").
    logged: HashMap<(Rc<str>, u32), HashSet<String>>,
    hits: Vec<(u32, Rc<str>)>,
}

impl Default for GraphRuntime {
    fn default() -> Self {
        Self {
            programs: HashMap::new(),
            instances: Vec::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
            debug: Debugger::default(),
            logged: HashMap::new(),
            hits: Vec::new(),
        }
    }
}

/// Object alive, active in the hierarchy and its Scripts entry enabled.
pub fn runnable(w: &World, e: Entity, slot: usize) -> bool {
    // A missing Scripts component also means the object is gone.
    w.scene
        .world
        .get::<&Scripts>(e)
        .ok()
        .and_then(|s| s.list.get(slot).map(|r| r.enabled))
        .unwrap_or(false)
        && w.scene.active_in_hierarchy(e)
}

fn with_world<R>(lua: &Lua, f: impl FnOnce(&mut World) -> R) -> Option<R> {
    let mut w = lua.app_data_mut::<World>()?;
    Some(f(&mut w))
}

fn same_kind(a: &EventKind, b: &EventKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Different error messages logged for one node before it goes quiet.
const MESSAGES_PER_NODE: usize = 4;

impl GraphRuntime {
    /// The compiled graph at `path`; problems are logged once.
    fn program(&mut self, lua: &Lua, root: &Path, path: &str) -> Option<Rc<Program>> {
        if let Some(p) = self.programs.get(path) {
            return p.clone();
        }
        let compiled = crate::vfs::project_file(root, path)
            .and_then(|file| ScriptGraph::load(&file))
            .map_err(|e| vec![format!("{path}: {e}")])
            .and_then(|g| {
                super::compile(&g, &FileResolver(root.to_path_buf())).map_err(|errs| {
                    errs.iter()
                        .map(|e| {
                            let title = e
                                .node
                                .and_then(|n| g.node(n))
                                .map(|n| format!(" ('{}')", super::nodes::title(n, &g)))
                                .unwrap_or_default();
                            match e.node {
                                Some(n) => format!("{path}: node {n}{title}: {}", e.message),
                                None => format!("{path}: {}", e.message),
                            }
                        })
                        .collect()
                })
            });
        let prog = match compiled {
            Ok(p) => Some(Rc::new(p)),
            Err(msgs) => {
                with_world(lua, |w| {
                    for m in msgs {
                        w.logs.push((
                            LogKind::Error,
                            format!("Script Graph does not compile: {m}"),
                        ));
                    }
                });
                None
            }
        };
        self.programs.insert(path.to_owned(), prog.clone());
        prog
    }

    /// Use an already compiled program for `path` (tests and benchmarks).
    pub fn insert_program(&mut self, path: &str, prog: Program) {
        self.programs.insert(path.to_owned(), Some(Rc::new(prog)));
    }

    /// Start a graph on an object (its Scripts entry `slot`). Object variables take the
    /// Inspector's values.
    pub fn instantiate(
        &mut self,
        lua: &Lua,
        root: &Path,
        e: Entity,
        slot: usize,
        script: &ScriptRef,
    ) {
        let Some(prog) = self.program(lua, root, &script.path) else {
            return;
        };
        let st = State::new(&prog, e);
        let obj_index = with_world(lua, |w| {
            prog.object_vars
                .iter()
                .map(|v| {
                    let init = script
                        .fields
                        .get(&*v.name)
                        .map_or_else(|| v.default.clone(), |f| Value::from_script_value(f, v.ty));
                    w.graph_vars.ensure(e, &v.name, init)
                })
                .collect()
        })
        .unwrap_or_default();
        self.instances.push(GraphInstance {
            entity: e,
            slot,
            path: Rc::from(script.path.as_str()),
            prog,
            st,
            obj_index,
            started: false,
            enabled: false,
            timers: Vec::new(),
        });
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    fn index_of(&self, e: Entity, slot: usize) -> Option<usize> {
        self.instances
            .iter()
            .position(|i| i.entity == e && i.slot == slot)
    }

    /// Run a flow of instance `i` to its end (making its Lua calls).
    fn run_flow(&mut self, lua: &Lua, i: usize, flow: Flow, step: bool, skip: Option<u32>) {
        self.run_flow_with(lua, None, i, flow, step, skip);
    }

    /// [`Self::run_flow`] starting with the world already borrowed (the hot path borrows it
    /// once per instance).
    fn run_flow_with(
        &mut self,
        lua: &Lua,
        mut held: Option<AppDataRefMut<'_, World>>,
        i: usize,
        mut flow: Flow,
        step: bool,
        mut skip: Option<u32>,
    ) {
        let empty = HashSet::new();
        loop {
            let exit = {
                let mut w = match held.take() {
                    Some(w) => w,
                    None => match lua.app_data_mut::<World>() {
                        Some(w) => w,
                        None => return,
                    },
                };
                let inst = &mut self.instances[i];
                let debugging = self.debug.path.as_deref() == Some(&*inst.path);
                let watched = debugging && self.debug.watched == Some((inst.entity, inst.slot));
                let can_break = debugging && self.debug.paused.is_none();
                let hook = (debugging && (can_break || watched)).then_some(Hook {
                    breakpoints: if can_break {
                        &self.debug.breakpoints
                    } else {
                        &empty
                    },
                    step: step && can_break,
                    skip: if can_break { skip } else { None },
                    hits: if watched { Some(&mut self.hits) } else { None },
                });
                vm::run(
                    &inst.prog,
                    &mut inst.st,
                    &inst.obj_index,
                    &mut w,
                    inst.entity,
                    &mut self.rng,
                    &mut flow,
                    hook,
                )
            };
            skip = None;
            if !self.hits.is_empty() {
                let frame = self.debug.frame;
                for h in self.hits.drain(..) {
                    self.debug.hits.insert(h, frame);
                }
            }
            match exit {
                Exit::Done => return,
                Exit::Lua(op) => {
                    let inst = &mut self.instances[i];
                    let prog = inst.prog.clone();
                    let call = &prog.lua[op as usize];
                    if let Err(e) =
                        super::lua_bridge::exec(lua, call, &mut inst.st.regs, inst.entity)
                    {
                        self.report(lua, i, call.node(), e);
                        return;
                    }
                }
                Exit::Error { node, message } => {
                    self.report(lua, i, node, message);
                    return;
                }
                Exit::Break { node } => {
                    let inst = &self.instances[i];
                    self.debug.watched = Some((inst.entity, inst.slot));
                    self.debug.paused = Some(Paused {
                        entity: inst.entity,
                        slot: inst.slot,
                        node,
                        flow,
                    });
                    return;
                }
            }
        }
    }

    /// Log a run-time error once per graph, node and message.
    fn report(&mut self, lua: &Lua, i: usize, node: u32, message: String) {
        let inst = &self.instances[i];
        if self.debug.path.as_deref() == Some(&*inst.path) {
            self.debug.errors.insert(node, message.clone());
        }
        let seen = self.logged.entry((inst.path.clone(), node)).or_default();
        if seen.len() >= MESSAGES_PER_NODE || !seen.insert(message.clone()) {
            return;
        }
        let title = inst.prog.titles.get(&node).cloned().unwrap_or_default();
        let (path, e) = (inst.path.clone(), inst.entity);
        with_world(lua, |w| {
            let name = if w.scene.contains(e) {
                w.scene.name(e)
            } else {
                "a destroyed object".into()
            };
            w.logs.push((
                LogKind::Error,
                format!("{name}: Script Graph {path}, node '{title}' ({node}): {message}"),
            ));
        });
    }

    /// Start event `event` of instance `i` with these output values.
    fn fire(&mut self, lua: &Lua, i: usize, event: usize, args: &[Value]) {
        let inst = &mut self.instances[i];
        let ev = &inst.prog.events[event];
        let pc = ev.pc;
        for (k, slot) in ev.outs.iter().enumerate() {
            inst.st.regs[*slot as usize] = args.get(k).cloned().unwrap_or(Value::Nil);
        }
        self.run_flow(lua, i, Flow::at(pc), false, None);
    }

    /// Fire every event of `kind` on instance `i`.
    fn fire_kind(&mut self, lua: &Lua, i: usize, kind: &EventKind, args: &[Value]) {
        let n = self.instances[i].prog.events.len();
        for k in 0..n {
            if &self.instances[i].prog.events[k].kind == kind {
                self.fire(lua, i, k, args);
            }
        }
    }

    fn can_run(&self, lua: &Lua, i: usize) -> bool {
        let inst = &self.instances[i];
        let (e, slot) = (inst.entity, inst.slot);
        with_world(lua, |w| runnable(w, e, slot)).unwrap_or(false)
    }

    /// Evaluate a timer's Interval and Repeat: (interval, repeat).
    fn timer_inputs(&mut self, lua: &Lua, i: usize, event: usize) -> (f64, bool) {
        let ev = self.instances[i].prog.events[event].clone();
        if let Some(pc) = ev.eval_pc {
            self.run_flow(lua, i, Flow::at(pc), false, None);
        }
        let regs = &self.instances[i].st.regs;
        let interval = ev
            .ins
            .first()
            .map_or(1.0, |s| f64::from(regs[*s as usize].as_float()));
        let repeat = ev.ins.get(1).is_none_or(|s| regs[*s as usize].as_bool());
        (interval.max(0.0), repeat)
    }

    fn now(lua: &Lua) -> f64 {
        with_world(lua, |w| w.time.time).unwrap_or(0.0)
    }

    /// The first graph pass of a frame: a flow left at a breakpoint goes on, OnEnable /
    /// OnDisable for instances that changed, then Start for instances that can run for the
    /// first time.
    pub fn start_phase(&mut self, lua: &Lua) {
        self.debug.frame += 1;
        if self.debug.paused.is_some() {
            self.resume(lua);
        }
        // The watched instance.
        self.debug.watched = self.debug.path.as_deref().and_then(|p| {
            let mine = |i: &&GraphInstance| &*i.path == p;
            self.instances
                .iter()
                .filter(mine)
                .find(|i| Some(i.entity) == self.debug.watch)
                .or_else(|| self.instances.iter().find(mine))
                .map(|i| (i.entity, i.slot))
        });
        for i in 0..self.instances.len() {
            // Started instances without enable events need no check here (Update checks).
            let inst = &self.instances[i];
            if inst.started && !inst.prog.watches_enable {
                continue;
            }
            let run = self.can_run(lua, i);
            if run != self.instances[i].enabled {
                self.instances[i].enabled = run;
                let kind = if run {
                    EventKind::Enable
                } else {
                    EventKind::Disable
                };
                self.fire_kind(lua, i, &kind, &[]);
            }
            if run && !self.instances[i].started {
                self.instances[i].started = true;
                let now = Self::now(lua);
                let timers: Vec<usize> = (0..self.instances[i].prog.events.len())
                    .filter(|k| self.instances[i].prog.events[*k].kind == EventKind::Timer)
                    .collect();
                for event in timers {
                    let (interval, _) = self.timer_inputs(lua, i, event);
                    self.instances[i].timers.push(Timer {
                        event,
                        next: now + interval,
                        active: true,
                    });
                }
                self.fire_kind(lua, i, &EventKind::Start, &[]);
            }
        }
    }

    /// Update: the Update events, then Delays that are due, then timers.
    pub fn update_phase(&mut self, lua: &Lua, dt: f32) {
        let arg = Value::Float(dt);
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            if !inst.started
                || (inst.prog.update.is_empty()
                    && inst.st.delays.is_empty()
                    && inst.timers.is_empty())
            {
                continue;
            }
            let (e, slot) = (inst.entity, inst.slot);
            // One world borrow checks the object and runs its first Update flow.
            let Some(w) = lua.app_data_mut::<World>() else {
                return;
            };
            if !runnable(&w, e, slot) {
                continue;
            }
            let mut held = Some(w);
            let prog = self.instances[i].prog.clone();
            for k in &prog.update {
                let ev = &prog.events[*k];
                if let Some(out) = ev.outs.first() {
                    self.instances[i].st.regs[*out as usize] = arg.clone();
                }
                self.run_flow_with(lua, held.take(), i, Flow::at(ev.pc), false, None);
            }
            drop(held);
            if !self.instances[i].st.delays.is_empty() {
                let now = Self::now(lua);
                let delays = &mut self.instances[i].st.delays;
                let mut due = Vec::new();
                delays.retain(|d| {
                    if d.at <= now {
                        due.push(d.pc);
                        false
                    } else {
                        true
                    }
                });
                for pc in due {
                    self.run_flow(lua, i, Flow::at(pc), false, None);
                }
            }
            for t in 0..self.instances[i].timers.len() {
                let now = Self::now(lua);
                let timer = self.instances[i].timers[t];
                if !timer.active || now < timer.next {
                    continue;
                }
                self.fire(lua, i, timer.event, &[]);
                let (interval, repeat) = self.timer_inputs(lua, i, timer.event);
                let tm = &mut self.instances[i].timers[t];
                if repeat && interval > 0.0 {
                    tm.next += interval;
                    if tm.next <= now {
                        tm.next = now + interval;
                    }
                } else {
                    tm.active = false;
                }
            }
        }
    }

    /// Fire `kind` on every started instance that can run (FixedUpdate, LateUpdate).
    pub fn fire_all(&mut self, lua: &Lua, kind: &EventKind, args: &[Value]) {
        for i in 0..self.instances.len() {
            if !self.instances[i].started || !self.instances[i].prog.has_event(kind) {
                continue;
            }
            if self.can_run(lua, i) {
                self.fire_kind(lua, i, kind, args);
            }
        }
    }

    /// A trigger or collision of `me` with `other`.
    pub fn contact(&mut self, lua: &Lua, me: Entity, other: Entity, kind: &EventKind) {
        for i in 0..self.instances.len() {
            if self.instances[i].entity != me || !self.instances[i].prog.has_event(kind) {
                continue;
            }
            if self.can_run(lua, i) {
                self.fire_kind(lua, i, kind, &[Value::Object(other)]);
            }
        }
    }

    /// Input Action events: performed, held (every frame while pressed) and released.
    pub fn input_phase(&mut self, lua: &Lua) {
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            if !inst.started
                || !inst
                    .prog
                    .events
                    .iter()
                    .any(|e| matches!(e.kind, EventKind::Input { .. }))
                || !self.can_run(lua, i)
            {
                continue;
            }
            let n = self.instances[i].prog.events.len();
            for k in 0..n {
                let EventKind::Input { action, mode } =
                    self.instances[i].prog.events[k].kind.clone()
                else {
                    continue;
                };
                let fired = with_world(lua, |w| {
                    let st = w.input_system.action_state(&action)?;
                    let on = st.enabled
                        && match mode {
                            InputMode::Performed => st.performed_this_frame,
                            InputMode::Held => st.pressed,
                            InputMode::Released => st.released_this_frame,
                        };
                    let v = st.value.as_vec2();
                    on.then_some((st.magnitude, v))
                })
                .flatten();
                if let Some((magnitude, v)) = fired {
                    let args = [
                        Value::Float(magnitude),
                        Value::Vec3(glam::Vec3::new(v.x, v.y, 0.0)),
                    ];
                    self.fire(lua, i, k, &args);
                }
            }
        }
    }

    /// Deliver a custom event to graphs; true when one had a matching Custom Event node.
    pub fn custom(&mut self, lua: &Lua, ev: &CustomEvent) -> bool {
        let kind = EventKind::Custom {
            name: ev.name.clone(),
        };
        let mut heard = false;
        for i in 0..self.instances.len() {
            if ev.target.is_some_and(|t| t != self.instances[i].entity)
                || !self.instances[i].prog.has_event(&kind)
                || !self.can_run(lua, i)
            {
                continue;
            }
            heard = true;
            self.fire_kind(lua, i, &kind, &ev.args);
        }
        heard
    }

    /// Objects about to be removed: their graphs get OnDestroy, then stop.
    pub fn destroy(&mut self, lua: &Lua, doomed: &[Entity]) {
        if doomed.is_empty() {
            return;
        }
        let set: HashSet<Entity> = doomed.iter().copied().collect();
        for i in 0..self.instances.len() {
            if set.contains(&self.instances[i].entity)
                && self.instances[i].started
                && self.instances[i].prog.has_event(&EventKind::Destroy)
            {
                self.fire_kind(lua, i, &EventKind::Destroy, &[]);
            }
        }
        self.instances.retain(|i| !set.contains(&i.entity));
        if self
            .debug
            .paused
            .as_ref()
            .is_some_and(|p| set.contains(&p.entity))
        {
            self.debug.paused = None;
        }
        with_world(lua, |w| {
            for e in doomed {
                w.graph_vars.remove(*e);
            }
        });
    }

    /// Step the paused flow to the next node (it stays paused there, or ends).
    pub fn step(&mut self, lua: &Lua) {
        let Some(p) = self.debug.paused.take() else {
            return;
        };
        if let Some(i) = self.index_of(p.entity, p.slot) {
            let skip = Some(p.flow.pc);
            self.run_flow(lua, i, p.flow, true, skip);
        }
    }

    /// Let the paused flow go on (until it ends or reaches another breakpoint).
    pub fn resume(&mut self, lua: &Lua) {
        let Some(p) = self.debug.paused.take() else {
            return;
        };
        if let Some(i) = self.index_of(p.entity, p.slot) {
            let skip = Some(p.flow.pc);
            self.run_flow(lua, i, p.flow, false, skip);
        }
    }

    /// The value on a pin of the watched instance (the debugger's hover).
    pub fn value(&self, w: &World, node: u32, pin: &str, output: bool) -> Option<Value> {
        let (e, slot) = self.debug.watched?;
        let inst = self
            .instances
            .iter()
            .find(|i| i.entity == e && i.slot == slot)?;
        let at = inst
            .prog
            .port_slots
            .get(&(node, pin.to_owned(), output))
            .copied()?;
        Some(match at {
            PortSlot::Reg(s) => inst.st.regs.get(s as usize)?.clone(),
            PortSlot::ObjVar(v) => w
                .graph_vars
                .at(e, *inst.obj_index.get(v as usize)? as usize),
        })
    }

    /// Graph variables of the watched instance (name, value), for the Blackboard while playing.
    pub fn watched_variables(&self, w: &World) -> Vec<(String, Value)> {
        let Some((e, slot)) = self.debug.watched else {
            return Vec::new();
        };
        let Some(inst) = self
            .instances
            .iter()
            .find(|i| i.entity == e && i.slot == slot)
        else {
            return Vec::new();
        };
        let mut out: Vec<(String, Value)> = inst
            .prog
            .vars
            .iter()
            .filter(|v| !v.name.contains('/'))
            .map(|v| (v.name.to_string(), inst.st.regs[v.slot as usize].clone()))
            .collect();
        for (k, v) in inst.prog.object_vars.iter().enumerate() {
            let i = inst.obj_index.get(k).copied().unwrap_or(0) as usize;
            out.push((v.name.to_string(), w.graph_vars.at(e, i)));
        }
        out
    }

    /// Forget compiled graphs (an edited graph recompiles for new instances).
    pub fn forget_programs(&mut self) {
        self.programs.clear();
    }

    /// Whether any instance has an event of `kind` (cheap skip for the host).
    pub fn any_event(&self, kind: &EventKind) -> bool {
        self.instances
            .iter()
            .any(|i| i.prog.events.iter().any(|e| same_kind(&e.kind, kind)))
    }
}
