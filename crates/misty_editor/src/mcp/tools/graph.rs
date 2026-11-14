//! `graph.*`: Script Graph (`.vgraph`) files: create one, read it with its pins and compile
//! problems, add and remove nodes and Blackboard variables, wire pins and set input values.
//! Attach a graph to an object with `scene.add_component` and its path.
//!
//! Edits are written to the file at once. A graph open in the Script Graph window is reloaded,
//! unless it has unsaved changes there (then the tool refuses, so neither side is lost).

use misty_engine::visual_script::{
    self, FileResolver, Literal, Node, PinType, ScriptGraph, VarScope, Variable, compile, nodes,
};
use serde_json::{Map, Value, json};

use super::{
    Cx, Kind, Outcome, Tool, ToolResult, assets_path, finite_numbers, object_schema, opt_bool,
    str_arg,
};
use crate::state::EditorState;

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "graph.create",
        title: "Create Script Graph",
        description: "Write a new Script Graph (.vgraph) under Assets/, with On Start and On Update events (\"empty\": true for none). Attach it to an object with scene.add_component and its path. Replacing a file needs \"overwrite\": true and the user's OK in the editor.",
        kind: Kind::Edit,
        idempotent: false,
        schema: create_schema,
        run: create,
    },
    Tool {
        name: "graph.read",
        title: "Read Script Graph",
        description: "A Script Graph's nodes (id, kind, title, settings, input values, input and output pins with types), wires, Blackboard variables and compile problems.",
        kind: Kind::Read,
        idempotent: true,
        schema: path_schema,
        run: read,
    },
    Tool {
        name: "graph.add_node",
        title: "Add Script Graph node",
        description: "Add a node and return its id and pins. \"kind\" is a node type (\"OnStart\", \"Branch\", \"AddFloat\", \"OnTriggerEnter2D\", \"OnCustomEvent\"…) or \"Api\" for an engine API member, with settings in \"params\" (Api: owner \"Physics2D\" or \"#Rigidbody2D\", member, mode get / set / call; OnCustomEvent: name, args; GetVariable / SetVariable: name).",
        kind: Kind::Edit,
        idempotent: false,
        schema: add_node_schema,
        run: add_node,
    },
    Tool {
        name: "graph.remove_node",
        title: "Remove Script Graph node",
        description: "Remove a node and its wires.",
        kind: Kind::Edit,
        idempotent: false,
        schema: node_schema,
        run: remove_node,
    },
    Tool {
        name: "graph.connect",
        title: "Wire Script Graph pins",
        description: "Wire an output pin to an input pin (types must match; exec pins are \"In\" / \"Out\" and the like). A data input takes one wire and an exec output starts one: an existing wire there is replaced.",
        kind: Kind::Edit,
        idempotent: true,
        schema: connect_schema,
        run: connect,
    },
    Tool {
        name: "graph.disconnect",
        title: "Unwire Script Graph pin",
        description: "Remove the wires of one pin (an input, or an output with \"output\": true).",
        kind: Kind::Edit,
        idempotent: true,
        schema: disconnect_schema,
        run: disconnect,
    },
    Tool {
        name: "graph.set_value",
        title: "Set Script Graph input",
        description: "Set the value an unconnected input pin uses: a boolean, number, string or [x, y, z] (Vector3; Euler degrees for a Quaternion).",
        kind: Kind::Edit,
        idempotent: true,
        schema: set_value_schema,
        run: set_value,
    },
    Tool {
        name: "graph.add_variable",
        title: "Add Script Graph variable",
        description: "Add a Blackboard variable: Graph scope (private to the running graph) or Object scope (on the GameObject, shared with its other graphs and Lua).",
        kind: Kind::Edit,
        idempotent: false,
        schema: variable_schema,
        run: add_variable,
    },
];

fn path_prop() -> Value {
    json!({ "type": "string", "description": "Project path of the graph (\"Assets/Graphs/Player.vgraph\")" })
}

fn node_prop(what: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "description": what })
}

fn create_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Where to write it, under Assets/ (\".vgraph\" is added if missing)" },
            "empty": { "type": "boolean", "description": "No starting events" },
            "overwrite": { "type": "boolean", "description": "Replace an existing file (asks the user in the editor)" },
        }),
        &["path"],
    )
}

fn path_schema() -> Value {
    object_schema(json!({ "path": path_prop() }), &["path"])
}

fn add_node_schema() -> Value {
    object_schema(
        json!({
            "path": path_prop(),
            "kind": { "type": "string", "description": "Node type" },
            "params": { "type": "object", "description": "Node settings (string values)" },
            "position": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 2,
                "maxItems": 2,
                "description": "[x, y] on the canvas (default: right of the other nodes)",
            },
            "values": { "type": "object", "description": "Input pin values by pin name (as graph.set_value)" },
        }),
        &["path", "kind"],
    )
}

fn node_schema() -> Value {
    object_schema(
        json!({ "path": path_prop(), "node": node_prop("Node id") }),
        &["path", "node"],
    )
}

fn connect_schema() -> Value {
    object_schema(
        json!({
            "path": path_prop(),
            "from_node": node_prop("Node with the output"),
            "from_pin": { "type": "string", "description": "Output pin name" },
            "to_node": node_prop("Node with the input"),
            "to_pin": { "type": "string", "description": "Input pin name" },
        }),
        &["path", "from_node", "from_pin", "to_node", "to_pin"],
    )
}

fn disconnect_schema() -> Value {
    object_schema(
        json!({
            "path": path_prop(),
            "node": node_prop("Node id"),
            "pin": { "type": "string", "description": "Pin name" },
            "output": { "type": "boolean", "description": "The pin is an output (default: an input)" },
        }),
        &["path", "node", "pin"],
    )
}

fn set_value_schema() -> Value {
    object_schema(
        json!({
            "path": path_prop(),
            "node": node_prop("Node id"),
            "pin": { "type": "string", "description": "Input pin name" },
            "value": { "description": "Boolean, number, string or [x, y, z]" },
        }),
        &["path", "node", "pin", "value"],
    )
}

fn variable_schema() -> Value {
    let types: Vec<&str> = PinType::VARIABLE_TYPES.iter().map(|t| t.label()).collect();
    object_schema(
        json!({
            "path": path_prop(),
            "name": { "type": "string", "description": "Variable name" },
            "type": { "type": "string", "enum": types, "description": "Value type" },
            "scope": { "type": "string", "enum": ["Graph", "Object"], "description": "Graph (default) or Object" },
            "value": { "description": "Starting value (default: the type's zero)" },
        }),
        &["path", "name", "type"],
    )
}

// ---- file access -------------------------------------------------------------------------------

/// The graph's project path (`.vgraph` under Assets/).
fn graph_path(args: &Value) -> Result<String, String> {
    let rel = assets_path(str_arg(args, "path")?)?;
    if !visual_script::is_graph_path(&rel) {
        return Err(format!("{rel} is not a Script Graph (.vgraph)"));
    }
    Ok(rel)
}

fn load(s: &EditorState, rel: &str) -> Result<ScriptGraph, String> {
    let abs = s.project.root.join(rel);
    if !abs.is_file() {
        return Err(format!("no Script Graph at {rel} (graph.create makes one)"));
    }
    ScriptGraph::load(&abs)
}

/// Load, change and save the graph; refuses when the Script Graph window has unsaved edits of it.
fn edit<R>(
    s: &mut EditorState,
    rel: &str,
    f: impl FnOnce(&mut ScriptGraph, &FileResolver) -> Result<R, String>,
) -> Result<(R, ScriptGraph), String> {
    let open_here = s.script_graph.path.as_deref() == Some(rel);
    if open_here && s.script_graph.dirty() {
        return Err(format!(
            "{rel} has unsaved changes in the Script Graph window: save or revert them first"
        ));
    }
    let mut g = load(s, rel)?;
    let files = FileResolver(s.project.root.clone());
    let r = f(&mut g, &files)?;
    let abs = s.project.root.join(rel);
    g.save(&abs)?;
    if open_here {
        s.script_graph.open(&abs, rel);
    }
    Ok((r, g))
}

// ---- descriptions ------------------------------------------------------------------------------

fn pins(list: &[visual_script::Pin]) -> Value {
    list.iter()
        .map(|p| {
            let mut o = json!({ "name": p.name, "type": p.ty.label() });
            if p.optional {
                o["optional"] = json!(true);
            }
            o
        })
        .collect()
}

fn describe_node(g: &ScriptGraph, n: &Node, files: &FileResolver) -> Value {
    let ports = nodes::ports(n, g, files);
    let mut v = json!({
        "id": n.id,
        "kind": n.kind,
        "title": nodes::title(n, g),
        "position": n.position,
        "inputs": pins(&ports.inputs),
        "outputs": pins(&ports.outputs),
    });
    if !n.params.is_empty() {
        v["params"] = json!(n.params);
    }
    if !n.values.is_empty() {
        let values: Map<String, Value> = n
            .values
            .iter()
            .map(|(k, l)| (k.clone(), literal_json(l)))
            .collect();
        v["values"] = Value::Object(values);
    }
    if let Some(p) = ports.problem {
        v["problem"] = json!(p);
    }
    v
}

fn literal_json(l: &Literal) -> Value {
    match l {
        Literal::None => Value::Null,
        Literal::Bool(b) => json!(b),
        Literal::Int(i) => json!(i),
        Literal::Float(f) => json!(f),
        Literal::String(s) => json!(s),
        Literal::Vector3(v) | Literal::Quaternion(v) => json!(v),
    }
}

/// A JSON value as a literal for a pin of type `ty`.
fn literal(v: &Value, ty: PinType) -> Result<Literal, String> {
    finite_numbers(v)?;
    let lit = match v {
        Value::Bool(b) => Literal::Bool(*b),
        Value::Number(n) if n.is_i64() && ty != PinType::Float => {
            Literal::Int(n.as_i64().unwrap_or(0))
        }
        Value::Number(n) => Literal::Float(n.as_f64().unwrap_or(0.0) as f32),
        Value::String(s) => Literal::String(s.clone()),
        Value::Array(a) => {
            let xs: Option<Vec<f32>> = a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect();
            match xs.as_deref() {
                Some([x, y, z]) if ty == PinType::Quaternion => Literal::Quaternion([*x, *y, *z]),
                Some([x, y, z]) => Literal::Vector3([*x, *y, *z]),
                _ => return Err("a vector value must be three numbers [x, y, z]".into()),
            }
        }
        Value::Null => Literal::None,
        Value::Object(_) => return Err("a pin value can't be an object".into()),
    };
    let fits = matches!(
        (ty, &lit),
        (PinType::Any, _)
            | (PinType::Bool, Literal::Bool(_))
            | (PinType::Int, Literal::Int(_))
            | (PinType::Float, Literal::Float(_) | Literal::Int(_))
            | (PinType::String, Literal::String(_))
            | (PinType::Vector3, Literal::Vector3(_))
            | (PinType::Quaternion, Literal::Quaternion(_))
            | (_, Literal::None)
    );
    if !fits {
        return Err(format!("that value doesn't fit a {} pin", ty.label()));
    }
    Ok(lit.coerce(ty))
}

/// Set an input's value, checked against its type.
fn set_input(
    g: &mut ScriptGraph,
    files: &FileResolver,
    node: u32,
    pin: &str,
    v: &Value,
) -> Result<(), String> {
    let n = g.node(node).ok_or_else(|| format!("no node {node}"))?;
    let ports = nodes::ports(n, g, files);
    let p = ports.input(pin).ok_or_else(|| {
        let names: Vec<&str> = ports
            .inputs
            .iter()
            .filter(|p| !p.ty.is_exec())
            .map(|p| p.name.as_str())
            .collect();
        format!(
            "{} has no input '{pin}' (inputs: {})",
            n.kind,
            names.join(", ")
        )
    })?;
    if p.ty.is_exec() {
        return Err(format!(
            "'{pin}' is an exec pin: wire it with graph.connect"
        ));
    }
    let lit = literal(v, p.ty)?;
    g.set_value(node, pin, lit);
    Ok(())
}

fn errors(g: &ScriptGraph, files: &FileResolver) -> Value {
    compile::check(g, files)
        .into_iter()
        .map(|e| json!({ "node": e.node, "message": e.message }))
        .collect()
}

// ---- tools -------------------------------------------------------------------------------------

fn create(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let mut rel = assets_path(str_arg(args, "path")?)?;
    if !visual_script::is_graph_path(&rel) {
        rel.push_str(".vgraph");
    }
    let stem = std::path::Path::new(&rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::asset_ops::valid_name(&stem)?;
    let g = if opt_bool(args, "empty").unwrap_or(false) {
        ScriptGraph::default()
    } else {
        ScriptGraph::new_default()
    };
    let abs = cx.s.project.root.join(&rel);
    if abs.is_dir() {
        return Err(format!("{rel} is a folder"));
    }
    let replacing = abs.exists();
    let write = move |cx: &mut Cx<'_>| {
        if let Some(dir) = abs.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return Outcome::Done(ToolResult::error(format!("{}: {e}", dir.display())));
        }
        if let Err(e) = g.save(&abs) {
            return Outcome::Done(ToolResult::error(e));
        }
        cx.s.refresh_assets();
        cx.s.assets.imported.push(rel.clone());
        if cx.s.script_graph.path.as_deref() == Some(rel.as_str()) {
            cx.s.script_graph.open(&abs, &rel);
        }
        let files = FileResolver(cx.s.project.root.clone());
        let nodes: Vec<Value> = g
            .nodes
            .iter()
            .map(|n| describe_node(&g, n, &files))
            .collect();
        Outcome::ok(json!({ "path": rel, "replaced": replacing, "nodes": nodes }))
    };
    if !replacing {
        return Ok(write(cx));
    }
    if !opt_bool(args, "overwrite").unwrap_or(false) {
        return Err(format!(
            "{} already exists: pass \"overwrite\": true to replace it",
            str_arg(args, "path")?
        ));
    }
    Ok(Outcome::Confirm {
        what: format!("overwrite {}", str_arg(args, "path")?),
        then: Box::new(write),
    })
}

fn read(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let g = load(cx.s, &rel)?;
    let files = FileResolver(cx.s.project.root.clone());
    let nodes: Vec<Value> = g
        .nodes
        .iter()
        .map(|n| describe_node(&g, n, &files))
        .collect();
    let wires: Vec<Value> = g
        .connections
        .iter()
        .map(|c| json!({ "from_node": c.from, "from_pin": c.output, "to_node": c.to, "to_pin": c.input }))
        .collect();
    let variables: Vec<Value> = g
        .variables
        .iter()
        .map(|v| {
            json!({
                "name": v.name,
                "type": v.ty.label(),
                "scope": if v.scope == VarScope::Object { "Object" } else { "Graph" },
                "value": literal_json(&v.value),
            })
        })
        .collect();
    Ok(Outcome::ok(json!({
        "path": rel,
        "nodes": nodes,
        "wires": wires,
        "variables": variables,
        "errors": errors(&g, &files),
    })))
}

fn add_node(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let kind = str_arg(args, "kind")?.trim().to_owned();
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(p) = args.get("params").and_then(Value::as_object) {
        for (k, v) in p {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Number(_) | Value::Bool(_) => v.to_string(),
                _ => return Err(format!("setting \"{k}\" must be a string")),
            };
            params.push((k.clone(), text));
        }
    }
    let position = match args.get("position").and_then(Value::as_array) {
        Some(p) => {
            finite_numbers(&Value::Array(p.clone()))?;
            let f = |i: usize| p.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            Some([f(0), f(1)])
        }
        None => None,
    };
    let values = args.get("values").and_then(Value::as_object).cloned();
    let (id, g) = edit(cx.s, &rel, |g, files| {
        let at = position.unwrap_or_else(|| {
            let right = g
                .nodes
                .iter()
                .map(|n| n.position[0])
                .fold(-260.0f32, f32::max);
            [right + 260.0, 0.0]
        });
        let refs: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let id = g.add_node_with(&kind, &refs, at);
        let problem = g.node(id).and_then(|n| nodes::ports(n, g, files).problem);
        let unknown = problem
            .as_deref()
            .is_some_and(|p| p.contains("unknown node type"));
        if unknown {
            return Err(format!(
                "unknown node type \"{kind}\" (graph.read shows the kinds of existing nodes)"
            ));
        }
        for (pin, v) in values.iter().flatten() {
            set_input(g, files, id, pin, v)?;
        }
        Ok(id)
    })?;
    let files = FileResolver(cx.s.project.root.clone());
    let node = g.node(id).map(|n| describe_node(&g, n, &files));
    Ok(Outcome::ok(json!({ "path": rel, "node": node })))
}

fn remove_node(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let id = node_arg(args, "node")?;
    edit(cx.s, &rel, |g, _| {
        if g.node(id).is_none() {
            return Err(format!("no node {id}"));
        }
        g.remove_node(id);
        Ok(())
    })?;
    Ok(Outcome::ok(json!({ "path": rel, "removed": id })))
}

fn connect(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let (from, to) = (node_arg(args, "from_node")?, node_arg(args, "to_node")?);
    let (out, inp) = (str_arg(args, "from_pin")?, str_arg(args, "to_pin")?);
    let (_, g) = edit(cx.s, &rel, |g, files| g.connect(from, out, to, inp, files))?;
    let files = FileResolver(cx.s.project.root.clone());
    Ok(Outcome::ok(json!({
        "path": rel,
        "wire": { "from_node": from, "from_pin": out, "to_node": to, "to_pin": inp },
        "errors": errors(&g, &files),
    })))
}

fn disconnect(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let id = node_arg(args, "node")?;
    let pin = str_arg(args, "pin")?;
    let output = opt_bool(args, "output").unwrap_or(false);
    edit(cx.s, &rel, |g, _| {
        if g.node(id).is_none() {
            return Err(format!("no node {id}"));
        }
        g.disconnect(id, pin, output);
        Ok(())
    })?;
    Ok(Outcome::ok(json!({ "path": rel, "node": id, "pin": pin })))
}

fn set_value(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let id = node_arg(args, "node")?;
    let pin = str_arg(args, "pin")?;
    let v = args.get("value").cloned().unwrap_or(Value::Null);
    let (_, g) = edit(cx.s, &rel, |g, files| set_input(g, files, id, pin, &v))?;
    let files = FileResolver(cx.s.project.root.clone());
    let node = g.node(id).map(|n| describe_node(&g, n, &files));
    Ok(Outcome::ok(json!({ "path": rel, "node": node })))
}

fn add_variable(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = graph_path(args)?;
    let name = str_arg(args, "name")?.trim().to_owned();
    if name.is_empty() {
        return Err("the variable needs a name".into());
    }
    let ty_name = str_arg(args, "type")?;
    let ty = PinType::VARIABLE_TYPES
        .into_iter()
        .find(|t| t.label() == ty_name)
        .ok_or_else(|| format!("no variable type \"{ty_name}\""))?;
    let object = args.get("scope").and_then(Value::as_str) == Some("Object");
    let value = match args.get("value").filter(|v| !v.is_null()) {
        Some(v) => literal(v, ty)?,
        None => ty.default_literal(),
    };
    edit(cx.s, &rel, |g, _| {
        if g.variables.iter().any(|v| v.name == name) {
            return Err(format!("{rel} already has a variable '{name}'"));
        }
        let var = Variable::new(&name, ty, value);
        g.variables.push(if object { var.object() } else { var });
        Ok(())
    })?;
    Ok(Outcome::ok(json!({
        "path": rel,
        "variable": name,
        "type": ty.label(),
        "scope": if object { "Object" } else { "Graph" },
    })))
}

fn node_arg(args: &Value, name: &str) -> Result<u32, String> {
    args.get(name)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("\"{name}\" must be a node id"))
}
