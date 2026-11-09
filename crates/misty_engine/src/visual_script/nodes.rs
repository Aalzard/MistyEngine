//! The node types: their pins (which may depend on the node's settings and the graph), titles
//! and categories, and the catalog the editor's search lists.
//!
//! Most nodes are [`natives::SIMPLE`] functions. The rest are listed in [`SPECIAL`]: events,
//! flow control, variables, constants, lists, Lua calls, custom events and subgraphs; and the
//! `Api` node, one Lua binding member (see [`super::api`]).

use super::natives::{self, D};
use super::value::{Literal, PinType};
use super::{Node, Resolver, ScriptGraph};

/// One pin of a node.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub name: String,
    pub ty: PinType,
    /// Value of an unconnected input.
    pub default: Literal,
}

impl Pin {
    pub fn exec(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ty: PinType::Exec,
            default: Literal::None,
        }
    }

    pub fn data(name: &str, ty: PinType) -> Self {
        Self {
            name: name.to_owned(),
            ty,
            default: ty.default_literal(),
        }
    }

    pub fn with(name: &str, ty: PinType, default: Literal) -> Self {
        Self {
            name: name.to_owned(),
            ty,
            default,
        }
    }
}

/// A node's pins, plus why it is broken (unknown type, missing variable or subgraph).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ports {
    pub inputs: Vec<Pin>,
    pub outputs: Vec<Pin>,
    pub problem: Option<String>,
}

impl Ports {
    pub fn input(&self, name: &str) -> Option<&Pin> {
        self.inputs.iter().find(|p| p.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&Pin> {
        self.outputs.iter().find(|p| p.name == name)
    }

    /// Has exec pins (an event, flow or action node) rather than being pure.
    pub fn has_exec(&self) -> bool {
        self.inputs.iter().chain(&self.outputs).any(|p| p.ty.is_exec())
    }

    fn broken(problem: String) -> Self {
        Self {
            problem: Some(problem),
            ..Default::default()
        }
    }
}

/// A node type that isn't a simple function.
pub struct Special {
    pub kind: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub keywords: &'static str,
}

/// Event node types (no exec input; the runtime starts flows from them).
pub const EVENTS: [&str; 14] = [
    "OnStart",
    "OnUpdate",
    "OnFixedUpdate",
    "OnLateUpdate",
    "OnTriggerEnter",
    "OnTriggerExit",
    "OnCollisionEnter",
    "OnCollisionExit",
    "OnInputAction",
    "OnCustomEvent",
    "OnTimer",
    "OnDestroy",
    "OnEnable",
    "OnDisable",
];

pub fn is_event(kind: &str) -> bool {
    EVENTS.contains(&kind)
}

macro_rules! special {
    ($kind:literal, $title:literal, $cat:literal, $kw:literal) => {
        Special {
            kind: $kind,
            title: $title,
            category: $cat,
            keywords: $kw,
        }
    };
}

/// Node types handled by the compiler itself.
pub static SPECIAL: &[Special] = &[
    special!("OnStart", "On Start", "Events", "begin play awake"),
    special!("OnUpdate", "On Update", "Events", "tick frame every"),
    special!("OnFixedUpdate", "On Fixed Update", "Events", "physics step"),
    special!("OnLateUpdate", "On Late Update", "Events", "after"),
    special!("OnTriggerEnter", "On Trigger Enter", "Events", "overlap begin physics"),
    special!("OnTriggerExit", "On Trigger Exit", "Events", "overlap end physics"),
    special!("OnCollisionEnter", "On Collision Enter", "Events", "hit physics"),
    special!("OnCollisionExit", "On Collision Exit", "Events", "physics"),
    special!("OnInputAction", "On Input Action", "Events", "input system key button performed held pressed released"),
    special!("OnCustomEvent", "Custom Event", "Events", "receive message"),
    special!("OnTimer", "On Timer", "Events", "every interval repeat seconds delay"),
    special!("OnDestroy", "On Destroy", "Events", "removed"),
    special!("OnEnable", "On Enable", "Events", "activated"),
    special!("OnDisable", "On Disable", "Events", "deactivated"),
    special!("Branch", "Branch", "Flow", "if else condition"),
    special!("Sequence", "Sequence", "Flow", "then order"),
    special!("ForLoop", "For Loop", "Flow", "repeat index count"),
    special!("ForEach", "For Each", "Flow", "list loop items"),
    special!("While", "While", "Flow", "loop condition"),
    special!("DoOnce", "Do Once", "Flow", "single first"),
    special!("Gate", "Gate", "Flow", "open close toggle"),
    special!("FlipFlop", "Flip Flop", "Flow", "toggle alternate"),
    special!("Delay", "Delay", "Flow", "wait seconds timer"),
    special!("GetVariable", "Get Variable", "Variables", "read blackboard"),
    special!("SetVariable", "Set Variable", "Variables", "write blackboard assign"),
    special!("GetObjectVariable", "Get Object Variable", "Variables", "other gameobject read"),
    special!("SetObjectVariable", "Set Object Variable", "Variables", "other gameobject write"),
    special!("Self", "Self", "Variables", "this gameobject owner me"),
    special!("Float", "Float", "Constants", "number literal"),
    special!("Int", "Int", "Constants", "integer literal"),
    special!("Bool", "Bool", "Constants", "boolean true false literal"),
    special!("String", "String", "Constants", "text literal"),
    special!("Vector3", "Vector3", "Constants", "literal position direction"),
    special!("Quaternion", "Quaternion", "Constants", "literal rotation euler"),
    special!("MakeList", "Make List", "List", "array create"),
    special!("CallLua", "Call Lua Function", "Lua", "script method invoke"),
    special!("GetLuaField", "Get Lua Field", "Lua", "table member property"),
    special!("SendCustomEvent", "Send Custom Event", "Events", "trigger message send lua"),
    special!("BroadcastCustomEvent", "Broadcast Custom Event", "Events", "trigger message all objects"),
    special!("Subgraph", "Subgraph", "Subgraphs", "macro function"),
    special!("GraphInput", "Graph Input", "Subgraphs", "subgraph inputs parameters"),
    special!("GraphOutput", "Graph Output", "Subgraphs", "subgraph outputs return"),
];

pub fn special(kind: &str) -> Option<&'static Special> {
    SPECIAL.iter().find(|s| s.kind == kind)
}

/// `"In:exec, Speed:float"` → pins (unknown types read as Any).
pub fn parse_pins(s: &str) -> Vec<(String, PinType)> {
    s.split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            match p.split_once(':') {
                Some((n, t)) => Some((
                    n.trim().to_owned(),
                    PinType::parse(t).unwrap_or(PinType::Any),
                )),
                None => Some((p.to_owned(), PinType::Any)),
            }
        })
        .collect()
}

/// `[("In", Exec)]` → `"In:exec"`.
pub fn format_pins(pins: &[(String, PinType)]) -> String {
    pins.iter()
        .map(|(n, t)| format!("{n}:{}", t.key()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Names in a comma list (`args` settings).
pub fn names(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_owned())
        .filter(|x| !x.is_empty())
        .collect()
}

fn count(node: &Node, key: &str, default: usize) -> usize {
    node.params
        .get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
        .clamp(1, 32)
}

/// The interface (inputs, outputs) of a subgraph: its Graph Input node's pins and its Graph
/// Output node's pins.
pub fn interface(g: &ScriptGraph) -> (Vec<(String, PinType)>, Vec<(String, PinType)>) {
    let find = |kind: &str| {
        g.nodes
            .iter()
            .find(|n| n.kind == kind)
            .map(|n| parse_pins(n.param("pins")))
            .unwrap_or_default()
    };
    (find("GraphInput"), find("GraphOutput"))
}

/// The graph a Subgraph node uses (a macro of `owner`, or an asset).
pub fn subgraph_of(
    node: &Node,
    owner: &ScriptGraph,
    files: &dyn Resolver,
) -> Result<ScriptGraph, String> {
    let m = node.param("macro");
    if !m.is_empty() {
        return owner
            .macro_graph(m)
            .cloned()
            .ok_or_else(|| format!("no macro named '{m}' in this graph"));
    }
    let path = node.param("path");
    if path.is_empty() {
        return Err("choose a subgraph asset".into());
    }
    files.load(path)
}

/// The pins of `node` in graph `g`.
pub fn ports(node: &Node, g: &ScriptGraph, files: &dyn Resolver) -> Ports {
    use PinType::*;
    if let Some(s) = natives::simple(&node.kind) {
        let mut p = Ports::default();
        if s.exec {
            p.inputs.push(Pin::exec("In"));
            p.outputs.push(Pin::exec("Out"));
        }
        p.inputs.extend(
            s.inputs
                .iter()
                .map(|(n, t, d)| Pin::with(n, *t, d.literal())),
        );
        p.outputs
            .extend(s.outputs.iter().map(|(n, t)| Pin::data(n, *t)));
        return p;
    }
    let mut p = Ports::default();
    let exec_in = |p: &mut Ports| p.inputs.push(Pin::exec("In"));
    let exec_out = |p: &mut Ports, name: &str| p.outputs.push(Pin::exec(name));
    match node.kind.as_str() {
        "OnStart" | "OnLateUpdate" | "OnDestroy" | "OnEnable" | "OnDisable" => {
            exec_out(&mut p, "Out")
        }
        "OnUpdate" | "OnFixedUpdate" => {
            exec_out(&mut p, "Out");
            p.outputs.push(Pin::data("Delta Time", Float));
        }
        "OnTriggerEnter" | "OnTriggerExit" | "OnCollisionEnter" | "OnCollisionExit" => {
            exec_out(&mut p, "Out");
            p.outputs.push(Pin::data("Other", GameObject));
        }
        "OnInputAction" => {
            exec_out(&mut p, "Out");
            p.outputs.push(Pin::data("Value", Float));
            p.outputs.push(Pin::data("Vector", Vector3));
            if node.param("action").is_empty() {
                p.problem = Some("name the input action (for example Jump or Player/Jump)".into());
            }
        }
        "OnCustomEvent" => {
            exec_out(&mut p, "Out");
            for a in names(node.param("args")) {
                p.outputs.push(Pin::data(&a, Any));
            }
            if node.param("name").trim().is_empty() {
                p.problem = Some("name the custom event".into());
            }
        }
        "OnTimer" => {
            exec_out(&mut p, "Out");
            p.inputs
                .push(Pin::with("Interval", Float, Literal::Float(1.0)));
            p.inputs
                .push(Pin::with("Repeat", Bool, Literal::Bool(true)));
        }
        "Branch" => {
            exec_in(&mut p);
            p.inputs.push(Pin::data("Condition", Bool));
            exec_out(&mut p, "True");
            exec_out(&mut p, "False");
        }
        "Sequence" => {
            exec_in(&mut p);
            for i in 0..count(node, "count", 2) {
                exec_out(&mut p, &format!("Then {i}"));
            }
        }
        "ForLoop" => {
            exec_in(&mut p);
            p.inputs.push(Pin::exec("Break"));
            p.inputs.push(Pin::with("First", Int, Literal::Int(0)));
            p.inputs.push(Pin::with("Last", Int, Literal::Int(9)));
            exec_out(&mut p, "Loop Body");
            p.outputs.push(Pin::data("Index", Int));
            exec_out(&mut p, "Completed");
        }
        "ForEach" => {
            exec_in(&mut p);
            p.inputs.push(Pin::data("List", List));
            exec_out(&mut p, "Loop Body");
            p.outputs.push(Pin::data("Item", Any));
            p.outputs.push(Pin::data("Index", Int));
            exec_out(&mut p, "Completed");
        }
        "While" => {
            exec_in(&mut p);
            p.inputs.push(Pin::data("Condition", Bool));
            p.inputs
                .push(Pin::with("Max Iterations", Int, Literal::Int(10_000)));
            exec_out(&mut p, "Loop Body");
            exec_out(&mut p, "Completed");
        }
        "DoOnce" => {
            exec_in(&mut p);
            p.inputs.push(Pin::exec("Reset"));
            exec_out(&mut p, "Completed");
        }
        "Gate" => {
            for n in ["Enter", "Open", "Close", "Toggle"] {
                p.inputs.push(Pin::exec(n));
            }
            exec_out(&mut p, "Exit");
        }
        "FlipFlop" => {
            exec_in(&mut p);
            exec_out(&mut p, "A");
            exec_out(&mut p, "B");
            p.outputs.push(Pin::data("Is A", Bool));
        }
        "Delay" => {
            exec_in(&mut p);
            p.inputs
                .push(Pin::with("Duration", Float, Literal::Float(1.0)));
            exec_out(&mut p, "Completed");
        }
        "GetVariable" | "SetVariable" => {
            let name = node.param("name");
            let ty = match g.variable(name) {
                Some(v) => v.ty,
                None => {
                    p.problem = Some(if name.is_empty() {
                        "choose a variable".into()
                    } else {
                        format!("no variable named '{name}' on the Blackboard")
                    });
                    Any
                }
            };
            if node.kind == "SetVariable" {
                exec_in(&mut p);
                p.inputs.push(Pin::data("Value", ty));
                exec_out(&mut p, "Out");
            }
            p.outputs.push(Pin::data("Value", ty));
        }
        "GetObjectVariable" => {
            p.inputs.push(Pin::data("Target", GameObject));
            p.inputs.push(Pin::data("Name", String));
            p.outputs.push(Pin::data("Value", Any));
        }
        "SetObjectVariable" => {
            exec_in(&mut p);
            p.inputs.push(Pin::data("Target", GameObject));
            p.inputs.push(Pin::data("Name", String));
            p.inputs.push(Pin::data("Value", Any));
            exec_out(&mut p, "Out");
        }
        "Self" => p.outputs.push(Pin::data("Self", GameObject)),
        "Float" | "Int" | "Bool" | "String" | "Vector3" | "Quaternion" => {
            let ty = PinType::parse(&node.kind).unwrap_or(Any);
            p.inputs.push(Pin::data("Value", ty));
            p.outputs.push(Pin::data("Value", ty));
        }
        "MakeList" => {
            for i in 0..count(node, "count", 3) {
                p.inputs.push(Pin::data(&format!("Item {i}"), Any));
            }
            p.outputs.push(Pin::data("List", List));
        }
        "CallLua" => {
            exec_in(&mut p);
            p.inputs.push(Pin::data("Target", GameObject));
            for a in names(node.param("args")) {
                p.inputs.push(Pin::data(&a, Any));
            }
            exec_out(&mut p, "Out");
            p.outputs.push(Pin::data("Result", Any));
            if node.param("script").is_empty() || node.param("function").is_empty() {
                p.problem = Some("name the script and the function to call".into());
            }
        }
        "GetLuaField" => {
            p.inputs.push(Pin::data("Object", Any));
            p.inputs.push(Pin::data("Field", String));
            p.outputs.push(Pin::data("Value", Any));
        }
        "SendCustomEvent" | "BroadcastCustomEvent" => {
            exec_in(&mut p);
            if node.kind == "SendCustomEvent" {
                p.inputs.push(Pin::data("Target", GameObject));
            }
            for a in names(node.param("args")) {
                p.inputs.push(Pin::data(&a, Any));
            }
            exec_out(&mut p, "Out");
            if node.param("name").trim().is_empty() {
                p.problem = Some("name the custom event".into());
            }
        }
        "GraphInput" => {
            for (n, t) in parse_pins(node.param("pins")) {
                p.outputs.push(Pin::data(&n, t));
            }
        }
        "GraphOutput" => {
            for (n, t) in parse_pins(node.param("pins")) {
                p.inputs.push(Pin::data(&n, t));
            }
        }
        "Tunnel" => {
            for (n, t) in parse_pins(node.param("pins")) {
                p.inputs.push(Pin::data(&n, t));
                p.outputs.push(Pin::data(&n, t));
            }
        }
        "Subgraph" => match subgraph_of(node, g, files) {
            Ok(sub) => {
                let (ins, outs) = interface(&sub);
                p.inputs = ins.iter().map(|(n, t)| Pin::data(n, *t)).collect();
                p.outputs = outs.iter().map(|(n, t)| Pin::data(n, *t)).collect();
            }
            Err(e) => p.problem = Some(e),
        },
        "Api" => return super::api::ports(node),
        other => return Ports::broken(format!("unknown node type '{other}'")),
    }
    p
}

/// A node's title in the canvas.
pub fn title(node: &Node, g: &ScriptGraph) -> String {
    let _ = g;
    if let Some(s) = natives::simple(&node.kind) {
        return s.title.to_owned();
    }
    match node.kind.as_str() {
        "GetVariable" => format!("Get {}", node.param("name")),
        "SetVariable" => format!("Set {}", node.param("name")),
        "OnCustomEvent" => format!("Custom Event: {}", node.param("name")),
        "SendCustomEvent" => format!("Send Event: {}", node.param("name")),
        "BroadcastCustomEvent" => format!("Broadcast Event: {}", node.param("name")),
        "OnInputAction" => {
            let mode = match node.param("mode") {
                "held" => "Held",
                "released" => "Released",
                _ => "Performed",
            };
            format!("On {} {mode}", node.param("action"))
        }
        "CallLua" => format!("{}:{}", node.param("script"), node.param("function")),
        "Subgraph" => {
            let m = node.param("macro");
            if !m.is_empty() {
                format!("Macro: {m}")
            } else {
                let path = node.param("path");
                format!("Subgraph: {}", crate::scene::clip_name(path))
            }
        }
        "Api" => super::api::title(node),
        k => special(k).map_or_else(|| k.to_owned(), |s| s.title.to_owned()),
    }
}

/// Category of a node (search folders, header colours).
pub fn category(node: &Node) -> String {
    if let Some(s) = natives::simple(&node.kind) {
        return s.category.to_owned();
    }
    if node.kind == "Api" {
        return super::api::category(node);
    }
    special(&node.kind).map_or_else(|| "Other".to_owned(), |s| s.category.to_owned())
}

/// One creatable node for the editor's search.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    pub kind: String,
    pub params: Vec<(String, String)>,
    pub title: String,
    pub category: String,
    pub keywords: String,
}

impl CatalogEntry {
    fn new(kind: &str, params: &[(&str, &str)], title: &str, category: &str, kw: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            title: title.to_owned(),
            category: category.to_owned(),
            keywords: kw.to_owned(),
        }
    }
}

/// The built-in nodes (not engine API members, variables or project subgraphs, which the
/// editor adds).
pub fn catalog() -> Vec<CatalogEntry> {
    let mut out = Vec::new();
    for s in SPECIAL {
        match s.kind {
            // Listed per variable / per mode / per asset instead.
            "GetVariable" | "SetVariable" | "Subgraph" => continue,
            "OnInputAction" => {
                for (mode, label) in [
                    ("performed", "On Input Action Performed"),
                    ("held", "On Input Action Held"),
                    ("released", "On Input Action Released"),
                ] {
                    out.push(CatalogEntry::new(
                        s.kind,
                        &[("action", "Jump"), ("mode", mode)],
                        label,
                        s.category,
                        s.keywords,
                    ));
                }
                continue;
            }
            "OnCustomEvent" | "SendCustomEvent" | "BroadcastCustomEvent" => {
                out.push(CatalogEntry::new(
                    s.kind,
                    &[("name", "MyEvent")],
                    s.title,
                    s.category,
                    s.keywords,
                ));
                continue;
            }
            "CallLua" => {
                out.push(CatalogEntry::new(
                    s.kind,
                    &[("script", "MyScript"), ("function", "DoSomething")],
                    s.title,
                    s.category,
                    s.keywords,
                ));
                continue;
            }
            "GraphInput" => {
                out.push(CatalogEntry::new(
                    s.kind,
                    &[("pins", "In:exec, Value:float")],
                    s.title,
                    s.category,
                    s.keywords,
                ));
                continue;
            }
            "GraphOutput" => {
                out.push(CatalogEntry::new(
                    s.kind,
                    &[("pins", "Out:exec, Result:float")],
                    s.title,
                    s.category,
                    s.keywords,
                ));
                continue;
            }
            _ => {}
        }
        out.push(CatalogEntry::new(
            s.kind,
            &[],
            s.title,
            s.category,
            s.keywords,
        ));
    }
    for s in natives::SIMPLE {
        out.push(CatalogEntry::new(
            s.kind,
            &[],
            s.title,
            s.category,
            s.keywords,
        ));
    }
    out
}

/// Default pin value as a [`Literal`] (for D tables in other modules).
pub fn default_of(d: D) -> Literal {
    d.literal()
}
