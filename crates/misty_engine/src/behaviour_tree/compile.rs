//! Turning an asset into the flat form the tick walks: nodes in depth-first order with their
//! parent, children and subtree end, blackboard keys resolved to slot numbers, and the list of
//! conditionals that watch keys for aborts. Problems (missing keys, wrong child counts) come
//! back as errors; nodes with an unusable setting fail when they run.

use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use super::asset::{AbortMode, BehaviourTree, Category, LogLevel, NodeKind, ParallelPolicy};
use super::blackboard::{BbValue, CompareOp, KeyDef, KeyType};

/// A blackboard slot; [`NO_SLOT`] when the key is missing.
pub type Slot = u16;
pub const NO_SLOT: Slot = u16::MAX;
/// Parent of the root.
pub const NO_PARENT: u16 = u16::MAX;

/// A node's settings with keys resolved.
#[derive(Clone, Debug, PartialEq)]
pub enum CKind {
    Root,
    Sequence,
    Selector,
    Parallel(ParallelPolicy),
    RandomSelector,
    Inverter,
    Repeat {
        count: u32,
        until_fail: bool,
    },
    Cooldown(f32),
    TimeLimit(f32),
    Conditional {
        key: Slot,
        op: CompareOp,
        value: BbValue,
        abort: AbortMode,
    },
    ForceSuccess,
    ForceFailure,
    MoveTo {
        target: Slot,
        acceptance: f32,
    },
    Wait {
        seconds: f32,
        random: f32,
    },
    PlayAnimation {
        state: Rc<str>,
        crossfade: f32,
    },
    SetAnimatorParameter {
        parameter: Rc<str>,
        value: BbValue,
        from: Option<Slot>,
    },
    LookAt {
        target: Slot,
        turn_speed: f32,
    },
    SetBlackboard {
        key: Slot,
        value: BbValue,
        from: Option<Slot>,
    },
    IsInRange {
        target: Slot,
        range: f32,
    },
    CanSeeTarget {
        target: Slot,
        range: f32,
        /// cos(fov / 2).
        cos_half_fov: f32,
        eye_height: f32,
        store: Option<Slot>,
    },
    HasLineOfSight {
        target: Slot,
        eye_height: f32,
    },
    RunScriptGraph {
        graph: Rc<str>,
        event: Rc<str>,
        wait: bool,
    },
    RunLua {
        function: Rc<str>,
    },
    CustomLua {
        script: Rc<str>,
        params: Rc<BTreeMap<String, String>>,
    },
    Log {
        message: Rc<str>,
        level: LogLevel,
    },
}

/// One node in depth-first order.
#[derive(Clone, Debug, PartialEq)]
pub struct CNode {
    /// The asset's node id.
    pub id: u32,
    pub kind: CKind,
    pub parent: u16,
    /// Its place among its parent's children.
    pub index_in_parent: u16,
    /// First child's index in [`Compiled::children`].
    pub first_child: u32,
    pub child_count: u16,
    /// One past the last node of its subtree.
    pub end: u16,
}

/// A problem found while compiling (node id, message).
#[derive(Clone, Debug, PartialEq)]
pub struct CompileError {
    pub node: Option<u32>,
    pub message: String,
}

/// A tree ready to run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compiled {
    pub nodes: Vec<CNode>,
    /// Child node indices, each node's run together.
    pub children: Vec<u16>,
    pub keys: Vec<KeyDef>,
    /// Conditionals with an abort mode (they watch their key).
    pub observers: Vec<u16>,
    pub errors: Vec<CompileError>,
    /// Script Graphs the tree sends events to (added to agents that don't run them).
    pub graphs: Vec<Rc<str>>,
}

impl Compiled {
    pub fn child(&self, node: usize, k: usize) -> usize {
        self.children[self.nodes[node].first_child as usize + k] as usize
    }

    /// Index of the node with asset id `id`.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// Slot of a key by name.
    pub fn slot(&self, name: &str) -> Option<Slot> {
        self.keys
            .iter()
            .position(|k| k.name == name)
            .map(|i| i as Slot)
    }
}

/// Most nodes a tree may have (slots and indices are 16-bit).
pub const MAX_NODES: usize = 60_000;

/// Compile `tree`. A tree without a Root compiles to nothing (it never runs).
pub fn compile(tree: &BehaviourTree) -> Compiled {
    let mut out = Compiled {
        keys: tree.keys.clone(),
        ..Default::default()
    };
    let mut errors: Vec<CompileError> = Vec::new();
    let mut err = |node: Option<u32>, message: String| {
        errors.push(CompileError { node, message });
    };
    // Duplicate key names: the first one wins.
    let mut names = HashSet::new();
    for k in &tree.keys {
        if k.name.trim().is_empty() {
            err(None, "a blackboard key has no name".into());
        } else if !names.insert(k.name.as_str()) {
            err(
                None,
                format!("blackboard key '{}' is declared twice", k.name),
            );
        }
    }
    let roots: Vec<&super::asset::BtNode> = tree
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Root)
        .collect();
    let Some(root) = roots.first() else {
        err(None, "the tree has no Root node".into());
        out.errors = errors;
        return out;
    };
    if roots.len() > 1 {
        err(Some(roots[1].id), "a tree has one Root".into());
    }
    if tree.nodes.len() > MAX_NODES {
        err(None, format!("a tree has at most {MAX_NODES} nodes"));
        out.errors = errors;
        return out;
    }

    // Depth-first order with an explicit stack: (node id, parent index, index in parent).
    let mut seen = HashSet::new();
    let mut stack: Vec<(u32, u16, u16)> = vec![(root.id, NO_PARENT, 0)];
    let mut order: Vec<(u32, u16, u16)> = Vec::new();
    while let Some((id, parent, k)) = stack.pop() {
        if !seen.insert(id) {
            err(Some(id), "a node is used twice (loop)".into());
            continue;
        }
        let Some(n) = tree.node(id) else {
            continue;
        };
        let me = order.len() as u16;
        order.push((id, parent, k));
        let kids: Vec<u32> = n
            .children
            .iter()
            .copied()
            .filter(|c| tree.node(*c).is_some())
            .collect();
        for (i, c) in kids.iter().enumerate().rev() {
            stack.push((*c, me, i as u16));
        }
    }

    let key_slot = |name: &str| -> Slot {
        tree.keys
            .iter()
            .position(|k| k.name == name)
            .map_or(NO_SLOT, |i| i as Slot)
    };
    let key_type = |slot: Slot| tree.keys.get(slot as usize).map(|k| k.ty);
    for (idx, (id, parent, k)) in order.iter().enumerate() {
        let n = tree.node(*id).expect("ordered nodes exist");
        let (min, max) = n.kind.child_limits();
        let count = n
            .children
            .iter()
            .filter(|c| tree.node(**c).is_some())
            .count();
        if count < min {
            err(Some(n.id), format!("{} needs a child", n.title()));
        } else if count > max {
            err(
                Some(n.id),
                format!("{} takes at most {max} child", n.title()),
            );
        }
        for (key, types, what) in n.kind.key_uses() {
            if key.is_empty() {
                err(Some(n.id), format!("{}: {what} has no key", n.title()));
                continue;
            }
            match tree.key(key) {
                None => err(
                    Some(n.id),
                    format!("{}: no blackboard key '{key}'", n.title()),
                ),
                Some(def) if !types.contains(&def.ty) => err(
                    Some(n.id),
                    format!(
                        "{}: key '{key}' is {}, {what} needs {}",
                        n.title(),
                        def.ty.label(),
                        types
                            .iter()
                            .map(|t| t.label())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    ),
                ),
                _ => {}
            }
        }
        let opt = |name: &str| (!name.is_empty()).then(|| key_slot(name));
        let kind = match &n.kind {
            NodeKind::Root => CKind::Root,
            NodeKind::Sequence => CKind::Sequence,
            NodeKind::Selector => CKind::Selector,
            NodeKind::Parallel { policy } => CKind::Parallel(*policy),
            NodeKind::RandomSelector => CKind::RandomSelector,
            NodeKind::Inverter => CKind::Inverter,
            NodeKind::Repeat { count, until_fail } => CKind::Repeat {
                count: *count,
                until_fail: *until_fail,
            },
            NodeKind::Cooldown { seconds } => CKind::Cooldown(seconds.max(0.0)),
            NodeKind::TimeLimit { seconds } => CKind::TimeLimit(seconds.max(0.0)),
            NodeKind::Conditional {
                key,
                op,
                value,
                abort,
            } => {
                let slot = key_slot(key);
                // The value is compared in the key's type.
                let value = key_type(slot)
                    .and_then(|t| {
                        if op.needs_value() && t != KeyType::Object {
                            value.convert(t)
                        } else {
                            Some(value.clone())
                        }
                    })
                    .unwrap_or_else(|| value.clone());
                if *abort != AbortMode::None && slot != NO_SLOT {
                    out.observers.push(idx as u16);
                }
                if abort.aborts_lower() {
                    // Lower priority needs a composite above (through decorators).
                    let mut p = *parent;
                    while let Some((pid, pp, _)) = order.get(p as usize)
                        && tree
                            .node(*pid)
                            .is_some_and(|x| x.kind.category() == Category::Decorator)
                    {
                        p = *pp;
                    }
                    let composite = order.get(p as usize).is_some_and(|(pid, _, _)| {
                        tree.node(*pid)
                            .is_some_and(|x| x.kind.category() == Category::Composite)
                    });
                    if !composite {
                        err(
                            Some(n.id),
                            format!(
                                "{}: Lower Priority aborts need a composite above",
                                n.title()
                            ),
                        );
                    }
                }
                CKind::Conditional {
                    key: slot,
                    op: *op,
                    value,
                    abort: *abort,
                }
            }
            NodeKind::ForceSuccess => CKind::ForceSuccess,
            NodeKind::ForceFailure => CKind::ForceFailure,
            NodeKind::MoveTo { target, acceptance } => CKind::MoveTo {
                target: key_slot(target),
                acceptance: acceptance.max(0.01),
            },
            NodeKind::Wait { seconds, random } => CKind::Wait {
                seconds: seconds.max(0.0),
                random: random.max(0.0),
            },
            NodeKind::PlayAnimation { state, crossfade } => {
                if state.is_empty() {
                    err(Some(n.id), format!("{}: no state named", n.title()));
                }
                CKind::PlayAnimation {
                    state: Rc::from(state.as_str()),
                    crossfade: crossfade.max(0.0),
                }
            }
            NodeKind::SetAnimatorParameter {
                parameter,
                value,
                from_key,
            } => {
                if parameter.is_empty() {
                    err(Some(n.id), format!("{}: no parameter named", n.title()));
                }
                CKind::SetAnimatorParameter {
                    parameter: Rc::from(parameter.as_str()),
                    value: value.clone(),
                    from: opt(from_key),
                }
            }
            NodeKind::LookAt { target, turn_speed } => CKind::LookAt {
                target: key_slot(target),
                turn_speed: turn_speed.max(0.0),
            },
            NodeKind::SetBlackboard {
                key,
                value,
                from_key,
            } => {
                let slot = key_slot(key);
                if from_key.is_empty()
                    && let Some(t) = key_type(slot)
                    && t != KeyType::Object
                    && value.convert(t).is_none()
                {
                    err(
                        Some(n.id),
                        format!("{}: the value doesn't fit key '{key}'", n.title()),
                    );
                }
                CKind::SetBlackboard {
                    key: slot,
                    value: value.clone(),
                    from: opt(from_key),
                }
            }
            NodeKind::IsInRange { target, range } => CKind::IsInRange {
                target: key_slot(target),
                range: range.max(0.0),
            },
            NodeKind::CanSeeTarget {
                target,
                range,
                fov,
                eye_height,
                store,
            } => CKind::CanSeeTarget {
                target: key_slot(target),
                range: range.max(0.0),
                cos_half_fov: (fov.clamp(0.0, 360.0) * 0.5).to_radians().cos(),
                eye_height: *eye_height,
                store: opt(store),
            },
            NodeKind::HasLineOfSight { target, eye_height } => CKind::HasLineOfSight {
                target: key_slot(target),
                eye_height: *eye_height,
            },
            NodeKind::RunScriptGraph { graph, event, wait } => {
                if event.trim().is_empty() {
                    err(Some(n.id), format!("{}: no event named", n.title()));
                }
                let g: Rc<str> = Rc::from(graph.as_str());
                if !graph.is_empty() && !out.graphs.iter().any(|x| **x == *graph) {
                    out.graphs.push(g.clone());
                }
                CKind::RunScriptGraph {
                    graph: g,
                    event: Rc::from(event.as_str()),
                    wait: *wait,
                }
            }
            NodeKind::RunLua { function } => {
                if function.trim().is_empty() {
                    err(Some(n.id), format!("{}: no function named", n.title()));
                }
                CKind::RunLua {
                    function: Rc::from(function.as_str()),
                }
            }
            NodeKind::CustomLua { script, params } => {
                if script.is_empty() {
                    err(Some(n.id), format!("{}: no script chosen", n.title()));
                }
                CKind::CustomLua {
                    script: Rc::from(script.as_str()),
                    params: Rc::new(params.clone()),
                }
            }
            NodeKind::Log { message, level } => CKind::Log {
                message: Rc::from(message.as_str()),
                level: *level,
            },
        };
        out.nodes.push(CNode {
            id: n.id,
            kind,
            parent: *parent,
            index_in_parent: *k,
            first_child: 0,
            child_count: 0,
            end: 0,
        });
    }
    out.errors = errors;
    // Children lists and subtree ends.
    let len = out.nodes.len();
    let mut lists: Vec<Vec<u16>> = vec![Vec::new(); len];
    for c in 1..len {
        let p = out.nodes[c].parent as usize;
        if p < len {
            lists[p].push(c as u16);
        }
    }
    for (i, kids) in lists.into_iter().enumerate() {
        out.nodes[i].first_child = out.children.len() as u32;
        out.nodes[i].child_count = kids.len() as u16;
        out.children.extend(kids);
    }
    for i in (0..len).rev() {
        let end = if out.nodes[i].child_count == 0 {
            i + 1
        } else {
            let last = out.child(i, out.nodes[i].child_count as usize - 1);
            out.nodes[last].end as usize
        };
        out.nodes[i].end = end as u16;
    }
    let unreachable = tree.nodes.iter().filter(|n| !seen.contains(&n.id)).count();
    if unreachable > 0 {
        out.errors.push(CompileError {
            node: None,
            message: format!("{unreachable} node(s) are not connected to the Root and never run"),
        });
    }
    out
}

/// Errors that stop a tree from making sense (shown in red); the rest are warnings.
pub fn is_warning(e: &CompileError) -> bool {
    e.message.contains("never run")
}
