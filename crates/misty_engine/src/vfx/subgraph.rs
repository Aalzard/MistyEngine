//! Effect Sub-graphs (`.vfxoperator`): a reusable chain of operators saved as its own asset.
//! The sub-graph's Blackboard properties are the node's inputs and its **Output** nodes the
//! node's outputs. Graphs using one are flattened before compiling ([`expand`]): the sub-graph's
//! operators are copied in with fresh ids, its property reads replaced by what feeds the node's
//! inputs (or constants of the node's inline values), and links from the node's outputs start
//! at what feeds the matching Output node. Errors inside a sub-graph land on the node.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use super::codegen::GraphError;
use super::graph::{EffectGraph, Link, Operator, PropertyValue};
use super::operators::OperatorKind;
use super::types::{Port, PortType, Value};

/// File extension of sub-graph assets.
pub const EXTENSION: &str = "vfxoperator";
/// Sub-graphs inside sub-graphs, at most this deep.
const MAX_DEPTH: usize = 8;

pub fn is_subgraph_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

/// What a sub-graph node shows: inputs (from the properties) and outputs (from Output nodes).
#[derive(Clone, Debug)]
pub struct SubgraphInfo {
    pub graph: EffectGraph,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    modified: Option<SystemTime>,
}

/// Port names live as long as the process (ports name their inputs with `&'static str`).
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut map = NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(s) = map.get(name) {
        return s;
    }
    let s: &'static str = Box::leak(name.to_owned().into_boxed_str());
    map.insert(name.to_owned(), s);
    s
}

fn cache() -> &'static Mutex<HashMap<String, Arc<SubgraphInfo>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<SubgraphInfo>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn file_of(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        crate::assets::absolute(path)
    }
}

fn mtime(path: &str) -> Option<SystemTime> {
    std::fs::metadata(file_of(path))
        .and_then(|m| m.modified())
        .ok()
}

/// The node's ports of a sub-graph document.
pub fn ports_of(g: &EffectGraph) -> (Vec<Port>, Vec<Port>) {
    let mut inputs = Vec::new();
    for p in &g.properties {
        let name = intern(&p.name);
        inputs.push(match &p.value {
            PropertyValue::Float(v) => Port::float(name, *v),
            PropertyValue::Vector3(v) => Port::vec3(name, *v),
            PropertyValue::Color(c) => Port::color(name, *c),
            PropertyValue::Gradient(gr) => Port::gradient(name, gr.clone()),
            PropertyValue::Texture(_) => continue,
        });
    }
    let types = g.types();
    let mut outputs = Vec::new();
    for o in &g.operators {
        if let OperatorKind::SubgraphOutput { name } = &o.kind {
            let ty = g
                .input_link(o.id, "Value")
                .and_then(|l| types.output(l.from, &l.output))
                .unwrap_or_else(|| o.value(&Port::dynamic("Value", 0.0)).ty());
            outputs.push(Port::out(intern(name), PortType::Fixed(ty)));
        }
    }
    (inputs, outputs)
}

/// Make a sub-graph available under `path` without a file (tests, the editor's live copy).
pub fn register(path: &str, graph: EffectGraph) {
    let (inputs, outputs) = ports_of(&graph);
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(
        path.to_owned(),
        Arc::new(SubgraphInfo {
            graph,
            inputs,
            outputs,
            modified: None,
        }),
    );
}

/// Drop a registered copy (the file is read again on next use).
pub fn forget(path: &str) {
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
}

/// The sub-graph at a project path (re-read when the file changes; None when unreadable).
pub fn info(path: &str) -> Option<Arc<SubgraphInfo>> {
    let now = mtime(path);
    if let Some(i) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(path)
        && (i.modified.is_none() || i.modified == now)
    {
        return Some(i.clone());
    }
    let text = crate::vfs::read_to_string(file_of(path)).ok()?;
    let graph = EffectGraph::from_json(&text).ok()?;
    let (inputs, outputs) = ports_of(&graph);
    let info = Arc::new(SubgraphInfo {
        graph,
        inputs,
        outputs,
        modified: now,
    });
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_owned(), info.clone());
    Some(info)
}

/// Modification times of every sub-graph `g` uses (directly or nested), to notice edits.
pub fn dependencies(g: &EffectGraph) -> Vec<(String, Option<SystemTime>)> {
    let mut out: Vec<(String, Option<SystemTime>)> = Vec::new();
    let mut stack: Vec<(EffectGraph, usize)> = vec![(g.clone(), 0)];
    while let Some((graph, depth)) = stack.pop() {
        for o in &graph.operators {
            if let OperatorKind::Subgraph { path } = &o.kind
                && !out.iter().any(|(p, _)| p == path)
            {
                out.push((path.clone(), mtime(path)));
                if depth < MAX_DEPTH
                    && let Some(i) = info(path)
                {
                    stack.push((i.graph.clone(), depth + 1));
                }
            }
        }
    }
    out
}

/// Display name of a sub-graph node: the file name without extension.
pub fn title(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Sub-graph".into())
}

fn err(node: u32, message: impl Into<String>) -> GraphError {
    GraphError {
        message: message.into(),
        node: Some(node),
    }
}

fn constant(v: &Value) -> (OperatorKind, Value) {
    match v {
        Value::Vec3(_) => (OperatorKind::Vector3, v.clone()),
        Value::Color(_) => (OperatorKind::Color, v.clone()),
        _ => (OperatorKind::Float, Value::Float(v.as_float())),
    }
}

/// Flatten every sub-graph node of `g`. Returns the flat graph and, for each id added by the
/// expansion, the node of `g` it came from (so errors can point at the sub-graph node).
pub fn expand(g: &EffectGraph) -> Result<(EffectGraph, HashMap<u32, u32>), Vec<GraphError>> {
    let mut out = g.clone();
    let mut origin: HashMap<u32, u32> = HashMap::new();
    let mut errors = Vec::new();
    for _ in 0..MAX_DEPTH {
        let nodes: Vec<(u32, String)> = out
            .operators
            .iter()
            .filter_map(|o| match &o.kind {
                OperatorKind::Subgraph { path } => Some((o.id, path.clone())),
                _ => None,
            })
            .collect();
        if nodes.is_empty() {
            return if errors.is_empty() {
                Ok((out, origin))
            } else {
                Err(errors)
            };
        }
        for (node, path) in nodes {
            let root = origin.get(&node).copied().unwrap_or(node);
            match info(&path) {
                None => {
                    errors.push(err(root, format!("can't read sub-graph {path}")));
                    out.remove(node);
                }
                Some(i) => inline_one(&mut out, node, &i.graph, root, &mut origin),
            }
        }
    }
    errors.push(GraphError {
        message: format!("sub-graphs nested more than {MAX_DEPTH} deep (a sub-graph uses itself?)"),
        node: None,
    });
    Err(errors)
}

/// Replace the node `node` of `out` by the operators of `sub`.
fn inline_one(
    out: &mut EffectGraph,
    node: u32,
    sub: &EffectGraph,
    root: u32,
    origin: &mut HashMap<u32, u32>,
) {
    let Some(op) = out.operator(node).cloned() else {
        return;
    };
    let position = op.position;
    let inputs = op.kind.inputs();
    // Fresh ids for the copied operators.
    let mut ids: HashMap<u32, u32> = HashMap::new();
    for o in &sub.operators {
        let id = out.next_id.max(1);
        out.next_id = id + 1;
        ids.insert(o.id, id);
        origin.insert(id, root);
    }
    // What feeds each property of the sub-graph: the node's input link, or a constant.
    enum Feed {
        Link(u32, String),
        Inline(Value),
    }
    let mut feeds: HashMap<String, Feed> = HashMap::new();
    for p in &inputs {
        let feed = match out.input_link(node, p.name) {
            Some(l) => Feed::Link(l.from, l.output.clone()),
            None => {
                let v = op.value(p);
                if matches!(v, Value::Gradient(_) | Value::Curve(_)) {
                    Feed::Inline(v)
                } else {
                    let (kind, value) = constant(&v);
                    let id = out.next_id.max(1);
                    out.next_id = id + 1;
                    origin.insert(id, root);
                    out.operators.push(Operator {
                        id,
                        kind,
                        position,
                        values: BTreeMap::from([("Value".to_owned(), value)]),
                    });
                    Feed::Link(id, "Out".into())
                }
            }
        };
        feeds.insert(p.name.to_owned(), feed);
    }
    // Copy operators (property reads and Output nodes are dissolved into links).
    for o in &sub.operators {
        if matches!(
            o.kind,
            OperatorKind::Property { .. } | OperatorKind::SubgraphOutput { .. }
        ) {
            continue;
        }
        out.operators.push(Operator {
            id: ids[&o.id],
            kind: o.kind.clone(),
            position,
            values: o.values.clone(),
        });
    }
    // Where a link starting at sub-graph operator `from.port` now starts.
    let source = |from: u32, port: &str| -> Option<Result<(u32, String), Value>> {
        let o = sub.operator(from)?;
        match &o.kind {
            OperatorKind::Property { name } => match feeds.get(name)? {
                Feed::Link(f, p) => Some(Ok((*f, p.clone()))),
                Feed::Inline(v) => Some(Err(v.clone())),
            },
            _ => Some(Ok((ids[&from], port.to_owned()))),
        }
    };
    for l in &sub.links {
        let Some(to) = sub.operator(l.to) else {
            continue;
        };
        if matches!(to.kind, OperatorKind::SubgraphOutput { .. }) {
            continue;
        }
        match source(l.from, &l.output) {
            Some(Ok((from, output))) => out.links.push(Link {
                from,
                output,
                to: ids[&l.to],
                input: l.input.clone(),
            }),
            Some(Err(v)) => {
                if let Some(o) = out.operator_mut(ids[&l.to]) {
                    o.values.insert(l.input.clone(), v);
                }
            }
            None => {}
        }
    }
    // Links leaving the node now leave what feeds the matching Output node.
    let leaving: Vec<Link> = out
        .links
        .iter()
        .filter(|l| l.from == node)
        .cloned()
        .collect();
    out.links.retain(|l| l.from != node && l.to != node);
    for l in leaving {
        let Some(o) = sub.operators.iter().find(
            |o| matches!(&o.kind, OperatorKind::SubgraphOutput { name } if *name == l.output),
        ) else {
            continue;
        };
        let feed = match sub.input_link(o.id, "Value") {
            Some(sl) => source(sl.from, &sl.output),
            None => Some(Err(o.value(&Port::dynamic("Value", 0.0)))),
        };
        match feed {
            Some(Ok((from, output))) => out.links.push(Link {
                from,
                output,
                to: l.to,
                input: l.input,
            }),
            Some(Err(v)) => {
                // A constant output: a constant node of its value.
                let (kind, value) = constant(&v);
                let id = out.next_id.max(1);
                out.next_id = id + 1;
                origin.insert(id, root);
                out.operators.push(Operator {
                    id,
                    kind,
                    position,
                    values: BTreeMap::from([("Value".to_owned(), value)]),
                });
                out.links.push(Link {
                    from: id,
                    output: "Out".into(),
                    to: l.to,
                    input: l.input,
                });
            }
            None => {}
        }
    }
    out.operators.retain(|o| o.id != node);
}
