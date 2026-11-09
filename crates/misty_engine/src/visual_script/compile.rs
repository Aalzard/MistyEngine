//! Checking a graph and compiling it to bytecode.
//!
//! Subgraphs are inlined first ([`flatten`]): their Graph Input and Graph Output nodes become
//! pass-through tunnels and their private variables get unique names. Then every exec entry
//! (an event's flow, or a node's exec input) becomes a run of ops: first the pure nodes its data
//! inputs need, in dependency order (each computed once per run of the node, never ahead of
//! time), then the node's own op, then a jump to the next node (or the next node's code placed
//! right after, when it isn't compiled yet). Flow control uses a small call stack: Sequence and
//! the loops `Call` their branches, and a branch ends with `Ret`.
//!
//! Registers hold constants (unconnected inputs), node outputs and node state (loop counters,
//! Do Once / Gate / Flip Flop flags). Register 0 is the graph's own GameObject.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::api::{self, Mode};
use super::natives::{self, Cx, NativeFn, Slot};
use super::nodes::{self, Ports, interface, names, subgraph_of};
use super::value::{Literal, PinType, Value};
use super::{Connection, Node, Resolver, ScriptGraph, VarScope, Variable};

/// Register of the graph's own GameObject.
pub const SELF: Slot = 0;
/// Address of the shared `Ret` (unconnected exec outputs jump here).
pub const END: u32 = 0;
/// Marker for a jump target not known yet.
const PENDING: u32 = u32::MAX;
/// An output nobody reads (Set Variable's Value when unwired).
pub const NO_SLOT: Slot = u32::MAX;

/// A problem found while compiling (shown as a red badge on the node).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphError {
    /// The node (top-level id), when the problem belongs to one.
    pub node: Option<u32>,
    pub message: String,
}

impl GraphError {
    fn at(node: u32, message: impl Into<String>) -> Self {
        Self {
            node: Some(node),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.node {
            Some(n) => write!(f, "node {n}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// One bytecode instruction.
#[derive(Clone, Copy)]
pub enum Op {
    /// A native function over registers listed in the pool: `ins` inputs then `outs` outputs.
    Native {
        f: NativeFn,
        at: u32,
        ins: u16,
        outs: u16,
        node: u32,
    },
    Jump(u32),
    JumpIf {
        cond: Slot,
        to: u32,
    },
    JumpIfNot {
        cond: Slot,
        to: u32,
    },
    /// Run the code at the address until it returns, then go on.
    Call(u32),
    /// Back to the caller, or the end of the flow.
    Ret,
    Copy {
        from: Slot,
        to: Slot,
    },
    SetBool {
        slot: Slot,
        value: bool,
    },
    Toggle {
        slot: Slot,
    },
    /// index = first; end = last.
    ForInit {
        first: Slot,
        last: Slot,
        index: Slot,
        end: Slot,
    },
    /// Jump to `exit` once index > end.
    ForTest {
        index: Slot,
        end: Slot,
        exit: u32,
    },
    Incr {
        slot: Slot,
    },
    /// copy = list; index = 0.
    ForEachInit {
        list: Slot,
        copy: Slot,
        index: Slot,
        node: u32,
    },
    /// item = copy[index], or jump to `exit` past the end.
    ForEachTest {
        copy: Slot,
        index: Slot,
        item: Slot,
        exit: u32,
    },
    /// counter = 0.
    GuardInit {
        counter: Slot,
    },
    /// counter += 1; fail when it passes max (a While that never ends).
    Guard {
        counter: Slot,
        max: Slot,
        node: u32,
    },
    /// Flip the state: on A go on, on B jump.
    FlipFlop {
        state: Slot,
        is_a: Slot,
        b: u32,
    },
    /// Unless pending: schedule `resume` after `duration` seconds. Then return.
    Delay {
        pending: Slot,
        duration: Slot,
        resume: u32,
    },
    /// Graph variable `var` (it lives in its register) = from.
    SetVar {
        var: u16,
        from: Slot,
        out: Slot,
    },
    GetObjVar {
        var: u16,
        out: Slot,
    },
    SetObjVar {
        var: u16,
        from: Slot,
        out: Slot,
    },
    /// Another object's variable by name.
    GetOtherVar {
        target: Slot,
        name: Slot,
        out: Slot,
        node: u32,
    },
    SetOtherVar {
        target: Slot,
        name: Slot,
        from: Slot,
        node: u32,
    },
    /// Float functions without register lists (the common math nodes).
    F1 {
        f: natives::F1,
        a: Slot,
        out: Slot,
    },
    F2 {
        f: natives::F2,
        a: Slot,
        b: Slot,
        out: Slot,
    },
    F3 {
        f: natives::F3,
        a: Slot,
        b: Slot,
        c: Slot,
        out: Slot,
    },
    /// A call into Lua ([`Program::lua`]); the runtime makes it.
    Lua(u32),
    /// Send a custom event ([`Program::sends`]).
    Send(u32),
}

impl Op {
    fn set_target(&mut self, pc: u32) {
        match self {
            Op::Jump(t) | Op::Call(t) => *t = pc,
            Op::JumpIf { to, .. } | Op::JumpIfNot { to, .. } => *to = pc,
            Op::ForTest { exit, .. } | Op::ForEachTest { exit, .. } => *exit = pc,
            Op::FlipFlop { b, .. } => *b = pc,
            Op::Delay { resume, .. } => *resume = pc,
            _ => {}
        }
    }
}

impl std::fmt::Debug for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Native {
                at,
                ins,
                outs,
                node,
                ..
            } => write!(f, "Native(node {node}, pool {at}, {ins} in, {outs} out)"),
            Op::Jump(t) => write!(f, "Jump({t})"),
            Op::JumpIf { cond, to } => write!(f, "JumpIf(r{cond} -> {to})"),
            Op::JumpIfNot { cond, to } => write!(f, "JumpIfNot(r{cond} -> {to})"),
            Op::Call(t) => write!(f, "Call({t})"),
            Op::Ret => write!(f, "Ret"),
            Op::Lua(i) => write!(f, "Lua({i})"),
            Op::Send(i) => write!(f, "Send({i})"),
            Op::F1 { .. } | Op::F2 { .. } | Op::F3 { .. } => write!(f, "Float"),
            other => write!(f, "{}", op_name(other)),
        }
    }
}

fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Copy { .. } => "Copy",
        Op::SetBool { .. } => "SetBool",
        Op::Toggle { .. } => "Toggle",
        Op::ForInit { .. } => "ForInit",
        Op::ForTest { .. } => "ForTest",
        Op::Incr { .. } => "Incr",
        Op::ForEachInit { .. } => "ForEachInit",
        Op::ForEachTest { .. } => "ForEachTest",
        Op::GuardInit { .. } => "GuardInit",
        Op::Guard { .. } => "Guard",
        Op::FlipFlop { .. } => "FlipFlop",
        Op::Delay { .. } => "Delay",
        Op::SetVar { .. } => "SetVar",
        Op::GetObjVar { .. } => "GetObjVar",
        Op::SetObjVar { .. } => "SetObjVar",
        Op::GetOtherVar { .. } => "GetOtherVar",
        Op::SetOtherVar { .. } => "SetOtherVar",
        _ => "?",
    }
}

/// How an Input Action event listens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Performed,
    Held,
    Released,
}

/// What starts a flow.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Start,
    Update,
    FixedUpdate,
    LateUpdate,
    TriggerEnter,
    TriggerExit,
    CollisionEnter,
    CollisionExit,
    Destroy,
    Enable,
    Disable,
    Input { action: String, mode: InputMode },
    Custom { name: Rc<str> },
    Timer,
}

/// One event node's flow.
#[derive(Clone, Debug)]
pub struct EventEntry {
    pub kind: EventKind,
    /// Top-level node id.
    pub node: u32,
    /// Where the flow starts.
    pub pc: u32,
    /// Registers of the event's data outputs (Delta Time, Other, custom arguments…).
    pub outs: Vec<Slot>,
    /// Code computing the event's inputs (a timer's Interval, Repeat), if any.
    pub eval_pc: Option<u32>,
    pub ins: Vec<Slot>,
}

/// A variable of the program.
#[derive(Clone, Debug)]
pub struct VarInfo {
    pub name: Rc<str>,
    pub ty: PinType,
    pub default: Value,
    /// Its register (Graph variables).
    pub slot: Slot,
}

/// A Lua call made by the runtime.
#[derive(Clone, Debug)]
pub enum LuaOp {
    /// A Lua API member (`owner` a global table, `#Type` or empty).
    Api {
        owner: String,
        member: String,
        mode: Mode,
        target: Option<Slot>,
        args: Vec<Slot>,
        out: Option<Slot>,
        node: u32,
    },
    /// `function` of the `script` instance on the target.
    CallScript {
        script: String,
        function: String,
        target: Slot,
        args: Vec<Slot>,
        out: Slot,
        node: u32,
    },
    /// `object.field` of a Lua value (or a GameObject's component, script or field).
    GetField {
        object: Slot,
        field: Slot,
        out: Slot,
        node: u32,
    },
}

impl LuaOp {
    pub fn node(&self) -> u32 {
        match self {
            LuaOp::Api { node, .. }
            | LuaOp::CallScript { node, .. }
            | LuaOp::GetField { node, .. } => *node,
        }
    }
}

/// A custom event sent by a node.
#[derive(Clone, Debug)]
pub struct SendDef {
    pub name: Rc<str>,
    /// None = every object.
    pub target: Option<Slot>,
    pub args: Vec<Slot>,
}

/// Where the debugger reads a pin's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortSlot {
    Reg(Slot),
    ObjVar(u16),
}

/// A compiled graph.
#[derive(Debug)]
pub struct Program {
    pub code: Vec<Op>,
    /// Register lists of native ops.
    pub pool: Vec<Slot>,
    /// Starting registers (constants, typed defaults, state).
    pub init: Vec<Value>,
    pub vars: Vec<VarInfo>,
    pub object_vars: Vec<VarInfo>,
    pub events: Vec<EventEntry>,
    pub lua: Vec<LuaOp>,
    pub sends: Vec<SendDef>,
    /// Per address: the exec entries starting there (top-level node, input pin), for
    /// breakpoints, stepping and active wires.
    pub marks: Vec<Vec<(u32, Rc<str>)>>,
    /// Top-level (node, pin, is output) → where its value is, for the debugger.
    pub port_slots: HashMap<(u32, String, bool), PortSlot>,
    /// Top-level node titles (error messages).
    pub titles: HashMap<u32, String>,
    /// Indices of the Update events (the hot path).
    pub update: Vec<usize>,
    /// Has On Enable or On Disable events (checked every frame).
    pub watches_enable: bool,
}

impl Program {
    /// Events of a kind.
    pub fn events_of<'a>(&'a self, kind: &'a EventKind) -> impl Iterator<Item = &'a EventEntry> {
        self.events.iter().filter(move |e| &e.kind == kind)
    }

    pub fn has_event(&self, kind: &EventKind) -> bool {
        self.events.iter().any(|e| &e.kind == kind)
    }
}

// ---- flattening --------------------------------------------------------------------------------

/// A node of the flattened graph.
struct FNode {
    node: Node,
    /// The top-level node it comes from (itself, or the Subgraph node it was inlined from).
    origin: u32,
    ports: Ports,
}

struct Flat {
    nodes: Vec<FNode>,
    conns: Vec<Connection>,
    vars: Vec<Variable>,
    next: u32,
    errors: Vec<GraphError>,
}

/// Inline subgraphs: nodes and wires of one graph with no Subgraph nodes left.
fn flatten(g: &ScriptGraph, files: &dyn Resolver) -> Flat {
    let mut f = Flat {
        nodes: Vec::new(),
        conns: Vec::new(),
        vars: g.variables.clone(),
        next: g.next_id().max(1) + 1_000_000,
        errors: Vec::new(),
    };
    let mut chain = Vec::new();
    expand(&mut f, g, None, &mut chain, files);
    f
}

fn expand(
    f: &mut Flat,
    g: &ScriptGraph,
    origin: Option<u32>,
    chain: &mut Vec<String>,
    files: &dyn Resolver,
) {
    let mut map: HashMap<u32, u32> = HashMap::new();
    for n in &g.nodes {
        let id = match origin {
            None => n.id,
            Some(_) => {
                f.next += 1;
                f.next
            }
        };
        map.insert(n.id, id);
    }
    // Private variables of an inlined subgraph get names unique to this use.
    let prefix = origin.map(|o| format!("{o}#{}/", chain.len()));
    if let Some(p) = &prefix {
        for v in &g.variables {
            if v.scope == VarScope::Graph {
                let mut v = v.clone();
                v.name = format!("{p}{}", v.name);
                f.vars.push(v);
            } else if !f.vars.iter().any(|x| x.name == v.name) {
                f.vars.push(v.clone());
            }
        }
    }
    // Subgraph node (flat id) → (input tunnel, output tunnel).
    let mut tunnels: HashMap<u32, (Option<u32>, Option<u32>)> = HashMap::new();
    for n in &g.nodes {
        let id = map[&n.id];
        let o = origin.unwrap_or(n.id);
        if n.kind == "Subgraph" {
            let sub = match subgraph_of(n, g, files) {
                Ok(s) => s,
                Err(e) => {
                    f.errors.push(GraphError::at(o, e));
                    continue;
                }
            };
            let key = match n.param("macro") {
                "" => n.param("path").to_owned(),
                m => format!("macro:{m}"),
            };
            if chain.contains(&key) || chain.len() >= 16 {
                f.errors.push(GraphError::at(
                    o,
                    format!("the subgraph '{key}' uses itself"),
                ));
                continue;
            }
            let before = f.nodes.len();
            chain.push(key);
            expand(f, &sub, Some(o), chain, files);
            chain.pop();
            let find = |kind: &str| {
                f.nodes[before..]
                    .iter()
                    .find(|x| x.node.kind == "Tunnel" && x.node.param("from") == kind)
                    .map(|x| x.node.id)
            };
            let (tin, tout) = (find("GraphInput"), find("GraphOutput"));
            // Values typed into the Subgraph node's inputs feed the Graph Input.
            if let Some(t) = tin
                && let Some(x) = f.nodes.iter_mut().find(|x| x.node.id == t)
            {
                x.node.values.extend(n.values.clone());
            }
            tunnels.insert(id, (tin, tout));
            continue;
        }
        let mut node = n.clone();
        node.id = id;
        let ports = if origin.is_some() && matches!(n.kind.as_str(), "GraphInput" | "GraphOutput") {
            node.params.insert("from".into(), n.kind.clone());
            node.kind = "Tunnel".into();
            nodes::ports(&node, g, files)
        } else {
            nodes::ports(n, g, files)
        };
        if let Some(p) = &prefix
            && matches!(n.kind.as_str(), "GetVariable" | "SetVariable")
            && g.variable(n.param("name"))
                .is_some_and(|v| v.scope == VarScope::Graph)
        {
            let renamed = format!("{p}{}", n.param("name"));
            node.params.insert("name".into(), renamed);
        }
        f.nodes.push(FNode {
            node,
            origin: o,
            ports,
        });
    }
    for c in &g.connections {
        let (Some(&from), Some(&to)) = (map.get(&c.from), map.get(&c.to)) else {
            continue;
        };
        let from = match tunnels.get(&from) {
            Some((_, Some(out))) => *out,
            Some((_, None)) => {
                f.errors.push(GraphError::at(
                    origin.unwrap_or(c.from),
                    "the subgraph has no Graph Output node",
                ));
                continue;
            }
            None => from,
        };
        let to = match tunnels.get(&to) {
            Some((Some(inp), _)) => *inp,
            Some((None, _)) => {
                f.errors.push(GraphError::at(
                    origin.unwrap_or(c.to),
                    "the subgraph has no Graph Input node",
                ));
                continue;
            }
            None => to,
        };
        f.conns.push(Connection {
            from,
            output: c.output.clone(),
            to,
            input: c.input.clone(),
        });
    }
}

// ---- code generation ---------------------------------------------------------------------------

type PinKey = (u32, Rc<str>);

struct Compiler<'a> {
    nodes: HashMap<u32, &'a FNode>,
    /// (node, input) → (source node, output).
    sources: HashMap<(u32, String), (u32, String)>,
    /// (node, exec output) → (target node, exec input).
    targets: HashMap<(u32, String), (u32, String)>,
    code: Vec<Op>,
    pool: Vec<Slot>,
    init: Vec<Value>,
    marks: Vec<Vec<(u32, Rc<str>)>>,
    /// Compiled exec entries.
    entries: HashMap<PinKey, u32>,
    /// Jumps waiting for an entry: (code index, entry).
    fixups: Vec<(usize, PinKey)>,
    /// Output registers by (node, output).
    outs: HashMap<(u32, String), Slot>,
    /// State registers by (node, name).
    state: HashMap<(u32, &'static str), Slot>,
    /// Pure nodes computed in the current run.
    emitted: HashSet<u32>,
    visiting: HashSet<u32>,
    vars: Vec<VarInfo>,
    var_index: HashMap<String, (VarScope, u16)>,
    object_vars: Vec<VarInfo>,
    lua: Vec<LuaOp>,
    sends: Vec<SendDef>,
    errors: Vec<GraphError>,
    reported: HashSet<(u32, String)>,
}

impl<'a> Compiler<'a> {
    fn node(&self, id: u32) -> &'a FNode {
        self.nodes[&id]
    }

    fn error(&mut self, id: u32, message: impl Into<String>) {
        let origin = self.nodes.get(&id).map_or(id, |n| n.origin);
        let message = message.into();
        let inner = self.nodes.get(&id).is_some_and(|n| n.origin != n.node.id);
        let message = if inner {
            format!("inside the subgraph: {message}")
        } else {
            message
        };
        if self.reported.insert((origin, message.clone())) {
            self.errors.push(GraphError::at(origin, message));
        }
    }

    fn alloc(&mut self, v: Value) -> Slot {
        self.init.push(v);
        (self.init.len() - 1) as Slot
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.push(op);
        self.code.len() - 1
    }

    fn pc(&self) -> u32 {
        self.code.len() as u32
    }

    fn state_slot(&mut self, node: u32, name: &'static str, init: Value) -> Slot {
        if let Some(s) = self.state.get(&(node, name)) {
            return *s;
        }
        let s = self.alloc(init);
        self.state.insert((node, name), s);
        s
    }

    /// The register an output's value is written to.
    fn out_slot(&mut self, node: u32, pin: &str) -> Slot {
        if let Some(s) = self.outs.get(&(node, pin.to_owned())) {
            return *s;
        }
        let f = self.node(node);
        let s = match (f.node.kind.as_str(), pin) {
            ("ForLoop" | "ForEach", "Index") => self.state_slot(node, "index", Value::Int(0)),
            ("ForEach", "Item") => self.state_slot(node, "item", Value::Nil),
            ("FlipFlop", "Is A") => self.state_slot(node, "is_a", Value::Bool(false)),
            _ => {
                let ty = f.ports.output(pin).map_or(PinType::Any, |p| p.ty);
                self.alloc(ty.default_value())
            }
        };
        self.outs.insert((node, pin.to_owned()), s);
        s
    }

    /// A constant register for an unconnected input.
    fn constant(&mut self, node: u32, pin: &str) -> Slot {
        let f = self.node(node);
        let Some(p) = f.ports.input(pin) else {
            return self.alloc(Value::Nil);
        };
        let lit = f
            .node
            .values
            .get(pin)
            .map_or_else(|| p.default.clone(), |l| l.coerce(p.ty));
        if p.ty == PinType::GameObject && lit == Literal::None {
            return SELF;
        }
        let v = lit.to_value();
        self.alloc(v)
    }

    /// Make the value of `node.input` available (computing the pure nodes it needs) and return
    /// its register.
    fn pull(&mut self, node: u32, input: &str) -> Slot {
        let Some((src, out)) = self.sources.get(&(node, input.to_owned())).cloned() else {
            return self.constant(node, input);
        };
        let out = out.as_str();
        let s = self.node(src);
        match s.node.kind.as_str() {
            "Self" => return SELF,
            "Float" | "Int" | "Bool" | "String" | "Vector3" | "Quaternion" => {
                return self.pull(src, "Value");
            }
            "Tunnel" => return self.pull(src, out),
            // A Graph variable lives in a register: read it there.
            "GetVariable" => {
                if let Some((VarScope::Graph, var)) =
                    self.var_index.get(s.node.param("name")).copied()
                {
                    return self.vars[var as usize].slot;
                }
            }
            _ => {}
        }
        if self.is_pure(src) && !self.emitted.contains(&src) {
            if !self.visiting.insert(src) {
                self.error(src, "data wires loop back into this node");
                return self.alloc(Value::Nil);
            }
            self.emit_pure(src);
            self.visiting.remove(&src);
            self.emitted.insert(src);
        }
        self.out_slot(src, out)
    }

    fn is_pure(&self, id: u32) -> bool {
        let f = self.node(id);
        !f.ports.has_exec() && !matches!(f.node.kind.as_str(), "GraphInput" | "Tunnel")
    }

    /// A simple node: its direct float form when it has one.
    fn simple_op(&mut self, s: &natives::Simple, ins: &[Slot], outs: &[Slot], node: u32) {
        use natives::Fast;
        match (natives::fast(s.kind), ins, outs) {
            (Some(Fast::F1(f)), [a], [out]) => {
                self.emit(Op::F1 {
                    f,
                    a: *a,
                    out: *out,
                });
            }
            (Some(Fast::F2(f)), [a, b], [out]) => {
                self.emit(Op::F2 {
                    f,
                    a: *a,
                    b: *b,
                    out: *out,
                });
            }
            (Some(Fast::F3(f)), [a, b, c], [out]) => {
                self.emit(Op::F3 {
                    f,
                    a: *a,
                    b: *b,
                    c: *c,
                    out: *out,
                });
            }
            _ => self.native(s.f, ins, outs, node),
        }
    }

    fn native(&mut self, f: NativeFn, ins: &[Slot], outs: &[Slot], node: u32) {
        let at = self.pool.len() as u32;
        self.pool.extend_from_slice(ins);
        self.pool.extend_from_slice(outs);
        let origin = self.node(node).origin;
        self.emit(Op::Native {
            f,
            at,
            ins: ins.len() as u16,
            outs: outs.len() as u16,
            node: origin,
        });
    }

    /// Data inputs of a node, pulled in pin order (exec pins skipped).
    fn pull_inputs(&mut self, id: u32, skip: &[&str]) -> Vec<Slot> {
        let names: Vec<String> = self
            .node(id)
            .ports
            .inputs
            .iter()
            .filter(|p| !p.ty.is_exec() && !skip.contains(&p.name.as_str()))
            .map(|p| p.name.clone())
            .collect();
        names.iter().map(|n| self.pull(id, n)).collect()
    }

    fn data_outs(&mut self, id: u32) -> Vec<Slot> {
        let names: Vec<String> = self
            .node(id)
            .ports
            .outputs
            .iter()
            .filter(|p| !p.ty.is_exec())
            .map(|p| p.name.clone())
            .collect();
        names.iter().map(|n| self.out_slot(id, n)).collect()
    }

    fn var(&mut self, id: u32) -> Option<(VarScope, u16)> {
        let name = self.node(id).node.param("name").to_owned();
        let v = self.var_index.get(&name).copied();
        if v.is_none() {
            self.error(id, format!("no variable named '{name}' on the Blackboard"));
        }
        v
    }

    fn lua_op(&mut self, op: LuaOp) {
        self.lua.push(op);
        self.emit(Op::Lua((self.lua.len() - 1) as u32));
    }

    /// Code of a pure node.
    fn emit_pure(&mut self, id: u32) {
        let f = self.node(id);
        let kind = f.node.kind.as_str();
        let origin = f.origin;
        if let Some(s) = natives::simple(kind) {
            let ins = self.pull_inputs(id, &[]);
            let outs = self.data_outs(id);
            self.simple_op(s, &ins, &outs, id);
            return;
        }
        match kind {
            "Api" => self.emit_api(id),
            "GetVariable" => {
                let out = self.out_slot(id, "Value");
                match self.var(id) {
                    Some((VarScope::Graph, var)) => {
                        // (Normally read straight from the variable's register; see `pull`.)
                        let from = self.vars[var as usize].slot;
                        self.emit(Op::Copy { from, to: out });
                    }
                    Some((VarScope::Object, var)) => {
                        self.emit(Op::GetObjVar { var, out });
                    }
                    None => {}
                }
            }
            "GetObjectVariable" => {
                let target = self.pull(id, "Target");
                let name = self.pull(id, "Name");
                let out = self.out_slot(id, "Value");
                self.emit(Op::GetOtherVar {
                    target,
                    name,
                    out,
                    node: origin,
                });
            }
            "GetLuaField" => {
                let object = self.pull(id, "Object");
                let field = self.pull(id, "Field");
                let out = self.out_slot(id, "Value");
                self.lua_op(LuaOp::GetField {
                    object,
                    field,
                    out,
                    node: origin,
                });
            }
            "MakeList" => {
                let ins = self.pull_inputs(id, &[]);
                let out = self.out_slot(id, "List");
                self.native(make_list, &ins, &[out], id);
            }
            // Events, flow nodes and unknown nodes have nothing to compute here.
            _ => {}
        }
    }

    /// An `Api` node (pure or action).
    fn emit_api(&mut self, id: u32) {
        let f = self.node(id);
        let a = api::api_ref(&f.node);
        let (owner, member, mode) = (a.owner.to_owned(), a.member.to_owned(), a.mode);
        let target = owner.starts_with('#').then(|| self.pull(id, "Target"));
        let args = self.pull_inputs(id, &["Target"]);
        if let Some(n) = api::native(&owner, &member, mode) {
            let mut ins: Vec<Slot> = target.into_iter().collect();
            ins.extend(args);
            let outs = self.data_outs(id);
            self.native(n.f, &ins, &outs, id);
            return;
        }
        let out = match mode {
            Mode::Set => None,
            Mode::Get => Some(self.out_slot(id, "Value")),
            Mode::Call => Some(self.out_slot(id, "Result")),
        };
        let origin = f.origin;
        self.lua_op(LuaOp::Api {
            owner,
            member,
            mode,
            target,
            args,
            out,
            node: origin,
        });
    }

    /// Mark the start of an entry for the debugger.
    fn mark(&mut self, pc: u32, node: u32, pin: &str) {
        let origin = self.node(node).origin;
        // Inlined nodes report as their Subgraph node, once per address.
        while self.marks.len() <= pc as usize {
            self.marks.push(Vec::new());
        }
        let list = &mut self.marks[pc as usize];
        if !list.iter().any(|(n, _)| *n == origin) {
            list.push((origin, Rc::from(pin)));
        }
    }

    /// Continue the flow out of `node.pin`: jump to the next node's code, or compile it right
    /// here when it isn't compiled yet.
    fn goto(&mut self, node: u32, pin: &str) {
        match self.targets.get(&(node, pin.to_owned())).cloned() {
            None => {
                self.emit(Op::Ret);
            }
            Some((n, p)) => {
                let key = (n, Rc::from(p.as_str()));
                match self.entries.get(&key) {
                    Some(&pc) => {
                        self.emit(Op::Jump(pc));
                    }
                    None => self.compile_entry(n, &p),
                }
            }
        }
    }

    /// Point the target of the op at `code[at]` to the code continuing out of `node.pin`.
    fn link(&mut self, at: usize, node: u32, pin: &str) {
        match self.targets.get(&(node, pin.to_owned())).cloned() {
            None => self.code[at].set_target(END),
            Some((n, p)) => {
                let key: PinKey = (n, Rc::from(p.as_str()));
                match self.entries.get(&key) {
                    Some(&pc) => self.code[at].set_target(pc),
                    None => self.fixups.push((at, key)),
                }
            }
        }
    }

    /// Compile the code run when the flow enters `node.pin`.
    fn compile_entry(&mut self, node: u32, pin: &str) {
        let pc = self.pc();
        self.entries.insert((node, Rc::from(pin)), pc);
        self.mark(pc, node, pin);
        self.emitted.clear();
        let f = self.node(node);
        let kind = f.node.kind.clone();
        if let Some(s) = natives::simple(&kind) {
            let ins = self.pull_inputs(node, &[]);
            let outs = self.data_outs(node);
            self.native(s.f, &ins, &outs, node);
            self.goto(node, "Out");
            return;
        }
        match (kind.as_str(), pin) {
            ("Api", _) => {
                self.emit_api(node);
                self.goto(node, "Out");
            }
            ("Branch", _) => {
                let cond = self.pull(node, "Condition");
                let at = self.emit(Op::JumpIfNot { cond, to: PENDING });
                self.link(at, node, "False");
                self.goto(node, "True");
            }
            ("Sequence", _) => {
                let outs: Vec<String> = f
                    .ports
                    .outputs
                    .iter()
                    .filter(|p| p.ty.is_exec())
                    .map(|p| p.name.clone())
                    .collect();
                if let Some((last, rest)) = outs.split_last() {
                    for o in rest {
                        let at = self.emit(Op::Call(PENDING));
                        self.link(at, node, o);
                    }
                    self.goto(node, last);
                } else {
                    self.emit(Op::Ret);
                }
            }
            ("ForLoop", "Break") => {
                let index = self.state_slot(node, "index", Value::Int(0));
                let end = self.state_slot(node, "end", Value::Int(0));
                self.emit(Op::Copy {
                    from: index,
                    to: end,
                });
                self.emit(Op::Ret);
            }
            ("ForLoop", _) => {
                let first = self.pull(node, "First");
                let last = self.pull(node, "Last");
                let index = self.state_slot(node, "index", Value::Int(0));
                let end = self.state_slot(node, "end", Value::Int(0));
                self.emit(Op::ForInit {
                    first,
                    last,
                    index,
                    end,
                });
                let top = self.pc();
                let test = self.emit(Op::ForTest {
                    index,
                    end,
                    exit: PENDING,
                });
                let call = self.emit(Op::Call(PENDING));
                self.link(call, node, "Loop Body");
                self.emit(Op::Incr { slot: index });
                self.emit(Op::Jump(top));
                let exit = self.pc();
                self.code[test].set_target(exit);
                self.goto(node, "Completed");
            }
            ("ForEach", _) => {
                if !self.sources.contains_key(&(node, "List".to_owned())) {
                    self.error(node, "connect a List to loop over");
                }
                let list = self.pull(node, "List");
                let copy = self.state_slot(node, "copy", Value::Nil);
                let index = self.state_slot(node, "index", Value::Int(0));
                let item = self.state_slot(node, "item", Value::Nil);
                let origin = f.origin;
                self.emit(Op::ForEachInit {
                    list,
                    copy,
                    index,
                    node: origin,
                });
                let top = self.pc();
                let test = self.emit(Op::ForEachTest {
                    copy,
                    index,
                    item,
                    exit: PENDING,
                });
                let call = self.emit(Op::Call(PENDING));
                self.link(call, node, "Loop Body");
                self.emit(Op::Incr { slot: index });
                self.emit(Op::Jump(top));
                let exit = self.pc();
                self.code[test].set_target(exit);
                self.goto(node, "Completed");
            }
            ("While", _) => {
                let counter = self.state_slot(node, "counter", Value::Int(0));
                let max = self.pull(node, "Max Iterations");
                self.emit(Op::GuardInit { counter });
                let top = self.pc();
                // The condition is computed again before every pass.
                self.emitted.clear();
                let cond = self.pull(node, "Condition");
                let test = self.emit(Op::JumpIfNot { cond, to: PENDING });
                let origin = f.origin;
                self.emit(Op::Guard {
                    counter,
                    max,
                    node: origin,
                });
                let call = self.emit(Op::Call(PENDING));
                self.link(call, node, "Loop Body");
                self.emit(Op::Jump(top));
                let exit = self.pc();
                self.code[test].set_target(exit);
                self.goto(node, "Completed");
            }
            ("DoOnce", "Reset") => {
                let done = self.do_once_slot(node);
                self.emit(Op::SetBool {
                    slot: done,
                    value: false,
                });
                self.emit(Op::Ret);
            }
            ("DoOnce", _) => {
                let done = self.do_once_slot(node);
                self.emit(Op::JumpIf {
                    cond: done,
                    to: END,
                });
                self.emit(Op::SetBool {
                    slot: done,
                    value: true,
                });
                self.goto(node, "Completed");
            }
            ("Gate", p) => {
                let closed = f.node.param("start_closed") == "true";
                let open = self.state_slot(node, "open", Value::Bool(!closed));
                match p {
                    "Enter" => {
                        self.emit(Op::JumpIfNot {
                            cond: open,
                            to: END,
                        });
                        self.goto(node, "Exit");
                    }
                    "Open" | "Close" => {
                        self.emit(Op::SetBool {
                            slot: open,
                            value: p == "Open",
                        });
                        self.emit(Op::Ret);
                    }
                    _ => {
                        self.emit(Op::Toggle { slot: open });
                        self.emit(Op::Ret);
                    }
                }
            }
            ("FlipFlop", _) => {
                let state = self.state_slot(node, "state", Value::Bool(false));
                let is_a = self.state_slot(node, "is_a", Value::Bool(false));
                let at = self.emit(Op::FlipFlop {
                    state,
                    is_a,
                    b: PENDING,
                });
                self.link(at, node, "B");
                self.goto(node, "A");
            }
            ("Delay", _) => {
                let duration = self.pull(node, "Duration");
                let pending = self.state_slot(node, "pending", Value::Bool(false));
                let at = self.emit(Op::Delay {
                    pending,
                    duration,
                    resume: PENDING,
                });
                let resume = self.pc();
                self.code[at].set_target(resume);
                self.emit(Op::SetBool {
                    slot: pending,
                    value: false,
                });
                self.goto(node, "Completed");
            }
            ("SetVariable", _) => {
                let from = self.pull(node, "Value");
                let read = self
                    .sources
                    .values()
                    .any(|(n, p)| *n == node && p == "Value");
                let out = if read {
                    self.out_slot(node, "Value")
                } else {
                    NO_SLOT
                };
                match self.var(node) {
                    Some((VarScope::Graph, var)) => {
                        self.emit(Op::SetVar { var, from, out });
                    }
                    Some((VarScope::Object, var)) => {
                        self.emit(Op::SetObjVar { var, from, out });
                    }
                    None => {}
                }
                self.goto(node, "Out");
            }
            ("SetObjectVariable", _) => {
                let target = self.pull(node, "Target");
                let name = self.pull(node, "Name");
                let from = self.pull(node, "Value");
                let origin = f.origin;
                self.emit(Op::SetOtherVar {
                    target,
                    name,
                    from,
                    node: origin,
                });
                self.goto(node, "Out");
            }
            ("CallLua", _) => {
                let target = self.pull(node, "Target");
                let args = self.pull_inputs(node, &["Target"]);
                let out = self.out_slot(node, "Result");
                let origin = f.origin;
                self.lua_op(LuaOp::CallScript {
                    script: f.node.param("script").to_owned(),
                    function: f.node.param("function").to_owned(),
                    target,
                    args,
                    out,
                    node: origin,
                });
                self.goto(node, "Out");
            }
            ("SendCustomEvent" | "BroadcastCustomEvent", _) => {
                let target = (kind == "SendCustomEvent").then(|| self.pull(node, "Target"));
                let args = self.pull_inputs(node, &["Target"]);
                self.sends.push(SendDef {
                    name: Rc::from(f.node.param("name").trim()),
                    target,
                    args,
                });
                self.emit(Op::Send((self.sends.len() - 1) as u32));
                self.goto(node, "Out");
            }
            ("Tunnel", p) => {
                let p = p.to_owned();
                self.goto(node, &p);
            }
            _ => {
                self.emit(Op::Ret);
            }
        }
    }

    fn do_once_slot(&mut self, node: u32) -> Slot {
        let closed = self.node(node).node.param("start_closed") == "true";
        self.state_slot(node, "done", Value::Bool(closed))
    }

    /// Address of an entry, compiling it at the end of the code when needed.
    fn entry_of(&mut self, node: u32, pin: &str) -> u32 {
        let key: PinKey = (node, Rc::from(pin));
        if let Some(&pc) = self.entries.get(&key) {
            return pc;
        }
        self.compile_entry(node, pin);
        self.entries[&key]
    }

    /// Compile what jumps still wait for.
    fn resolve_fixups(&mut self) {
        while let Some((at, key)) = self.fixups.pop() {
            let pc = match self.entries.get(&key) {
                Some(&pc) => pc,
                None => {
                    let (n, p) = key.clone();
                    self.entry_of(n, &p)
                }
            };
            self.code[at].set_target(pc);
        }
    }
}

fn make_list(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let items: Vec<Value> = i.iter().map(|s| cx.get(*s).clone()).collect();
    cx.set(o[0], Value::List(Rc::new(items)));
    Ok(())
}

/// The problems of a graph (an empty list = it compiles).
pub fn check(g: &ScriptGraph, files: &dyn Resolver) -> Vec<GraphError> {
    compile(g, files).err().unwrap_or_default()
}

/// Check and compile a graph.
pub fn compile(g: &ScriptGraph, files: &dyn Resolver) -> Result<Program, Vec<GraphError>> {
    let flat = flatten(g, files);
    let mut errors = flat.errors;
    let nodes: HashMap<u32, &FNode> = flat.nodes.iter().map(|n| (n.node.id, n)).collect();
    // Node problems (unknown types, missing variables, unnamed events…).
    let mut reported = HashSet::new();
    for n in &flat.nodes {
        if let Some(p) = &n.ports.problem {
            let msg = if n.origin != n.node.id {
                format!("inside the subgraph: {p}")
            } else {
                p.clone()
            };
            if reported.insert((n.origin, msg.clone())) {
                errors.push(GraphError::at(n.origin, msg));
            }
        }
    }
    // Wires: both ends exist and the types fit.
    let mut sources = HashMap::new();
    let mut targets = HashMap::new();
    for c in &flat.conns {
        let (Some(a), Some(b)) = (nodes.get(&c.from), nodes.get(&c.to)) else {
            continue;
        };
        if a.ports.problem.is_some() || b.ports.problem.is_some() {
            continue;
        }
        let (Some(out), Some(inp)) = (a.ports.output(&c.output), b.ports.input(&c.input)) else {
            let msg = if a.ports.output(&c.output).is_none() {
                format!("a wire comes from a missing output '{}'", c.output)
            } else {
                format!("a wire goes into a missing input '{}'", c.input)
            };
            let at = if a.ports.output(&c.output).is_none() {
                a.origin
            } else {
                b.origin
            };
            if reported.insert((at, msg.clone())) {
                errors.push(GraphError::at(at, msg));
            }
            continue;
        };
        if !out.ty.connects_to(inp.ty) {
            let msg = format!(
                "'{}' ({}) cannot take '{}' ({})",
                inp.name,
                inp.ty.label(),
                out.name,
                out.ty.label()
            );
            if reported.insert((b.origin, msg.clone())) {
                errors.push(GraphError::at(b.origin, msg));
            }
            continue;
        }
        if out.ty.is_exec() {
            targets.insert((c.from, c.output.clone()), (c.to, c.input.clone()));
        } else {
            sources.insert((c.to, c.input.clone()), (c.from, c.output.clone()));
        }
    }
    // Variables.
    let mut vars = Vec::new();
    let mut object_vars = Vec::new();
    let mut var_index = HashMap::new();
    for v in &flat.vars {
        if var_index.contains_key(&v.name) {
            errors.push(GraphError {
                node: None,
                message: format!("two variables are named '{}'", v.name),
            });
            continue;
        }
        let info = VarInfo {
            name: Rc::from(v.name.as_str()),
            ty: v.ty,
            default: v.value.coerce(v.ty).to_value(),
            slot: 0,
        };
        let list = match v.scope {
            VarScope::Graph => &mut vars,
            VarScope::Object => &mut object_vars,
        };
        list.push(info);
        var_index.insert(v.name.clone(), (v.scope, (list.len() - 1) as u16));
    }
    let mut c = Compiler {
        nodes,
        sources,
        targets,
        code: Vec::new(),
        pool: Vec::new(),
        init: vec![Value::Nil],
        marks: Vec::new(),
        entries: HashMap::new(),
        fixups: Vec::new(),
        outs: HashMap::new(),
        state: HashMap::new(),
        emitted: HashSet::new(),
        visiting: HashSet::new(),
        vars,
        var_index,
        object_vars,
        lua: Vec::new(),
        sends: Vec::new(),
        errors,
        reported,
    };
    c.emit(Op::Ret);
    // Graph variables live in registers.
    for k in 0..c.vars.len() {
        let d = c.vars[k].default.clone();
        c.vars[k].slot = c.alloc(d);
    }
    let mut events = Vec::new();
    let mut ids: Vec<u32> = flat.nodes.iter().map(|n| n.node.id).collect();
    ids.sort_unstable();
    for id in ids {
        let f = c.node(id);
        if f.ports.problem.is_some() || !nodes::is_event(&f.node.kind) {
            continue;
        }
        let kind = match f.node.kind.as_str() {
            "OnStart" => EventKind::Start,
            "OnUpdate" => EventKind::Update,
            "OnFixedUpdate" => EventKind::FixedUpdate,
            "OnLateUpdate" => EventKind::LateUpdate,
            "OnTriggerEnter" => EventKind::TriggerEnter,
            "OnTriggerExit" => EventKind::TriggerExit,
            "OnCollisionEnter" => EventKind::CollisionEnter,
            "OnCollisionExit" => EventKind::CollisionExit,
            "OnDestroy" => EventKind::Destroy,
            "OnEnable" => EventKind::Enable,
            "OnDisable" => EventKind::Disable,
            "OnTimer" => EventKind::Timer,
            "OnInputAction" => EventKind::Input {
                action: f.node.param("action").trim().to_owned(),
                mode: match f.node.param("mode") {
                    "held" => InputMode::Held,
                    "released" => InputMode::Released,
                    _ => InputMode::Performed,
                },
            },
            "OnCustomEvent" => EventKind::Custom {
                name: Rc::from(f.node.param("name").trim()),
            },
            _ => continue,
        };
        let Some((n, p)) = c.targets.get(&(id, "Out".to_owned())).cloned() else {
            continue;
        };
        let pc = c.entry_of(n, &p);
        let outs = c.data_outs(id);
        let (eval_pc, ins) = if kind == EventKind::Timer {
            let pc = c.pc();
            c.emitted.clear();
            let interval = c.pull(id, "Interval");
            let repeat = c.pull(id, "Repeat");
            c.emit(Op::Ret);
            (Some(pc), vec![interval, repeat])
        } else {
            (None, Vec::new())
        };
        events.push(EventEntry {
            kind,
            node: f.origin,
            pc,
            outs,
            eval_pc,
            ins,
        });
        c.resolve_fixups();
    }
    c.resolve_fixups();
    if !c.errors.is_empty() {
        return Err(c.errors);
    }
    // Debug map: top-level pins → registers.
    let mut port_slots = HashMap::new();
    let mut titles = HashMap::new();
    for n in &g.nodes {
        titles.insert(n.id, nodes::title(n, g));
    }
    for ((node, pin), slot) in &c.outs {
        if c.nodes.get(node).is_some_and(|f| f.origin == *node) {
            port_slots.insert((*node, pin.clone(), true), PortSlot::Reg(*slot));
        }
    }
    for f in c.nodes.values() {
        if f.origin != f.node.id {
            continue;
        }
        let id = f.node.id;
        if matches!(f.node.kind.as_str(), "GetVariable" | "SetVariable")
            && let Some((scope, var)) = c.var_index.get(f.node.param("name")).copied()
        {
            let slot = match scope {
                VarScope::Graph => PortSlot::Reg(c.vars[var as usize].slot),
                VarScope::Object => PortSlot::ObjVar(var),
            };
            port_slots.insert((id, "Value".to_owned(), true), slot);
        }
    }
    for ((node, input), (src, out)) in &c.sources {
        if c.nodes.get(node).is_some_and(|f| f.origin == *node)
            && let Some(s) = port_slots.get(&(*src, out.clone(), true)).copied()
        {
            port_slots.insert((*node, input.clone(), false), s);
        }
    }
    let marks = {
        let mut m = c.marks;
        m.resize(c.code.len(), Vec::new());
        m
    };
    let update = (0..events.len())
        .filter(|k| events[*k].kind == EventKind::Update)
        .collect();
    let watches_enable = events
        .iter()
        .any(|e| matches!(e.kind, EventKind::Enable | EventKind::Disable));
    Ok(Program {
        code: c.code,
        pool: c.pool,
        init: c.init,
        vars: c.vars,
        object_vars: c.object_vars,
        events,
        lua: c.lua,
        sends: c.sends,
        marks,
        port_slots,
        titles,
        update,
        watches_enable,
    })
}

/// The pins of a Subgraph node's interface as node settings (`pins`), for macros made from a
/// selection.
pub fn interface_text(g: &ScriptGraph) -> (String, String) {
    let (i, o) = interface(g);
    (nodes::format_pins(&i), nodes::format_pins(&o))
}

/// Comma names helper re-exported for the editor.
pub fn arg_names(s: &str) -> Vec<String> {
    names(s)
}
