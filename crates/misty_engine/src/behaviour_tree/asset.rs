//! Behaviour Tree assets (`.btree`, JSON): nodes with their settings and children, the
//! blackboard keys, comments and groups.
//!
//! The tree starts at the single **Root** node. A node's children run in the order they are
//! listed in `children` (the editor keeps that list sorted top to bottom as nodes are moved).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::blackboard::{BbValue, CompareOp, KeyDef, KeyType};

/// File extension of Behaviour Tree assets.
pub const EXTENSION: &str = "btree";
/// Current file version.
pub const VERSION: u32 = 1;

/// Is `path` a Behaviour Tree asset?
pub fn is_tree_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

/// When a Parallel finishes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParallelPolicy {
    /// Succeeds when every child succeeded; fails as soon as one fails.
    #[default]
    RequireAll,
    /// Succeeds as soon as one child succeeds; fails when every child failed.
    RequireOne,
}

impl ParallelPolicy {
    pub const ALL: [ParallelPolicy; 2] = [ParallelPolicy::RequireAll, ParallelPolicy::RequireOne];

    pub fn label(self) -> &'static str {
        match self {
            ParallelPolicy::RequireAll => "Require All",
            ParallelPolicy::RequireOne => "Require One",
        }
    }
}

/// What a Conditional does when its key changes while the tree runs elsewhere or below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbortMode {
    /// Checked only when the tree reaches it.
    #[default]
    None,
    /// Stops its own running branch when the condition turns false.
    SelfOnly,
    /// Stops a running lower-priority branch (to its right, under the same composite) when the
    /// condition turns true, so this branch runs instead.
    LowerPriority,
    /// Both.
    Both,
}

impl AbortMode {
    pub const ALL: [AbortMode; 4] = [
        AbortMode::None,
        AbortMode::SelfOnly,
        AbortMode::LowerPriority,
        AbortMode::Both,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AbortMode::None => "None",
            AbortMode::SelfOnly => "Self",
            AbortMode::LowerPriority => "Lower Priority",
            AbortMode::Both => "Both",
        }
    }

    pub fn aborts_self(self) -> bool {
        matches!(self, AbortMode::SelfOnly | AbortMode::Both)
    }

    pub fn aborts_lower(self) -> bool {
        matches!(self, AbortMode::LowerPriority | AbortMode::Both)
    }
}

/// Console level of a Log node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    #[default]
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub const ALL: [LogLevel; 3] = [LogLevel::Info, LogLevel::Warning, LogLevel::Error];

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Info => "Info",
            LogLevel::Warning => "Warning",
            LogLevel::Error => "Error",
        }
    }
}

fn one() -> f32 {
    1.0
}
fn half() -> f32 {
    0.5
}
fn yes() -> bool {
    true
}
fn eye() -> f32 {
    1.6
}
fn run_event() -> String {
    "Run".into()
}

/// What a node is, with its settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    /// Where the tree starts (one child).
    Root,
    // ---- composites ----
    /// Runs children in order until one fails.
    Sequence,
    /// Runs children in order until one succeeds.
    Selector,
    /// Runs every child at once.
    Parallel {
        #[serde(default)]
        policy: ParallelPolicy,
    },
    /// Like Selector, in a random order each time it starts.
    RandomSelector,
    // ---- decorators ----
    /// Success ↔ failure.
    Inverter,
    /// Runs its child `count` times (0 = forever), or until it fails.
    Repeat {
        #[serde(default)]
        count: u32,
        #[serde(default)]
        until_fail: bool,
    },
    /// After its child finishes, fails for `seconds` without running it.
    Cooldown {
        #[serde(default = "one")]
        seconds: f32,
    },
    /// Stops its child and fails when it runs longer than `seconds`.
    TimeLimit {
        #[serde(default = "one")]
        seconds: f32,
    },
    /// Runs its child only while `key <op> value` holds.
    Conditional {
        #[serde(default)]
        key: String,
        #[serde(default)]
        op: CompareOp,
        #[serde(default)]
        value: BbValue,
        #[serde(default)]
        abort: AbortMode,
    },
    /// Succeeds whatever its child did (once it finished).
    ForceSuccess,
    /// Fails whatever its child did (once it finished).
    ForceFailure,
    // ---- actions ----
    /// Walks the Nav Mesh Agent to a Vector or Object key.
    MoveTo {
        #[serde(default)]
        target: String,
        #[serde(default = "half")]
        acceptance: f32,
    },
    /// Waits `seconds` (± `random`).
    Wait {
        #[serde(default = "one")]
        seconds: f32,
        #[serde(default)]
        random: f32,
    },
    /// Plays (or cross-fades to) an Animator state.
    PlayAnimation {
        #[serde(default)]
        state: String,
        #[serde(default)]
        crossfade: f32,
    },
    /// Sets an Animator parameter to a value or a key's value.
    SetAnimatorParameter {
        #[serde(default)]
        parameter: String,
        #[serde(default)]
        value: BbValue,
        #[serde(default)]
        from_key: String,
    },
    /// Turns to face a Vector or Object key (`turn_speed` degrees per second, 0 = at once).
    LookAt {
        #[serde(default)]
        target: String,
        #[serde(default)]
        turn_speed: f32,
    },
    /// Writes a value or another key's value into a key.
    SetBlackboard {
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: BbValue,
        #[serde(default)]
        from_key: String,
    },
    // ---- conditions ----
    /// Succeeds when the target is within `range`.
    IsInRange {
        #[serde(default)]
        target: String,
        #[serde(default = "one")]
        range: f32,
    },
    /// Succeeds when the target is within `range`, inside the field of view and not hidden
    /// behind colliders; writes the answer to the Bool key `store` when one is named.
    CanSeeTarget {
        #[serde(default)]
        target: String,
        #[serde(default = "default_sight")]
        range: f32,
        #[serde(default = "default_fov")]
        fov: f32,
        #[serde(default = "eye")]
        eye_height: f32,
        #[serde(default)]
        store: String,
    },
    /// Succeeds when no collider is between the eyes and the target.
    HasLineOfSight {
        #[serde(default)]
        target: String,
        #[serde(default = "eye")]
        eye_height: f32,
    },
    // ---- scripting ----
    /// Sends the custom event `event` to the object's Script Graphs (and Lua scripts), adding
    /// `graph` to the object when it doesn't run it. With `wait`, the node runs until the graph
    /// calls `BehaviourTree.Finish`.
    RunScriptGraph {
        #[serde(default)]
        graph: String,
        #[serde(default = "run_event")]
        event: String,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// Calls `function` on the object's Lua scripts every tick while it runs: returning
    /// "running" keeps it running, false or "failure" fails, anything else succeeds.
    RunLua {
        #[serde(default)]
        function: String,
    },
    /// A leaf written in Lua: a script returning a table with `OnStart`, `OnTick`, `OnAbort`.
    CustomLua {
        #[serde(default)]
        script: String,
        #[serde(default)]
        params: BTreeMap<String, String>,
    },
    /// Writes a message to the Console (`{key}` is replaced by the key's value).
    Log {
        #[serde(default)]
        message: String,
        #[serde(default)]
        level: LogLevel,
    },
}

fn default_sight() -> f32 {
    15.0
}
fn default_fov() -> f32 {
    110.0
}

/// The four families of nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Root,
    Composite,
    Decorator,
    Action,
    Condition,
    Scripting,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Root => "Root",
            Category::Composite => "Composites",
            Category::Decorator => "Decorators",
            Category::Action => "Actions",
            Category::Condition => "Conditions",
            Category::Scripting => "Scripting",
        }
    }
}

impl NodeKind {
    /// One of each node users can add, with default settings (the node palette).
    pub fn palette() -> Vec<NodeKind> {
        vec![
            NodeKind::Sequence,
            NodeKind::Selector,
            NodeKind::Parallel {
                policy: ParallelPolicy::RequireAll,
            },
            NodeKind::RandomSelector,
            NodeKind::Inverter,
            NodeKind::Repeat {
                count: 0,
                until_fail: false,
            },
            NodeKind::Cooldown { seconds: 2.0 },
            NodeKind::TimeLimit { seconds: 5.0 },
            NodeKind::Conditional {
                key: String::new(),
                op: CompareOp::IsSet,
                value: BbValue::Bool(true),
                abort: AbortMode::None,
            },
            NodeKind::ForceSuccess,
            NodeKind::ForceFailure,
            NodeKind::MoveTo {
                target: String::new(),
                acceptance: 0.5,
            },
            NodeKind::Wait {
                seconds: 1.0,
                random: 0.0,
            },
            NodeKind::PlayAnimation {
                state: String::new(),
                crossfade: 0.2,
            },
            NodeKind::SetAnimatorParameter {
                parameter: String::new(),
                value: BbValue::Float(0.0),
                from_key: String::new(),
            },
            NodeKind::LookAt {
                target: String::new(),
                turn_speed: 360.0,
            },
            NodeKind::SetBlackboard {
                key: String::new(),
                value: BbValue::Bool(true),
                from_key: String::new(),
            },
            NodeKind::IsInRange {
                target: String::new(),
                range: 2.0,
            },
            NodeKind::CanSeeTarget {
                target: String::new(),
                range: default_sight(),
                fov: default_fov(),
                eye_height: eye(),
                store: String::new(),
            },
            NodeKind::HasLineOfSight {
                target: String::new(),
                eye_height: eye(),
            },
            NodeKind::RunScriptGraph {
                graph: String::new(),
                event: run_event(),
                wait: true,
            },
            NodeKind::RunLua {
                function: String::new(),
            },
            NodeKind::CustomLua {
                script: String::new(),
                params: BTreeMap::new(),
            },
            NodeKind::Log {
                message: "Hello".into(),
                level: LogLevel::Info,
            },
        ]
    }

    /// The type name ("MoveTo"), as the file writes it.
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Root => "Root",
            NodeKind::Sequence => "Sequence",
            NodeKind::Selector => "Selector",
            NodeKind::Parallel { .. } => "Parallel",
            NodeKind::RandomSelector => "RandomSelector",
            NodeKind::Inverter => "Inverter",
            NodeKind::Repeat { .. } => "Repeat",
            NodeKind::Cooldown { .. } => "Cooldown",
            NodeKind::TimeLimit { .. } => "TimeLimit",
            NodeKind::Conditional { .. } => "Conditional",
            NodeKind::ForceSuccess => "ForceSuccess",
            NodeKind::ForceFailure => "ForceFailure",
            NodeKind::MoveTo { .. } => "MoveTo",
            NodeKind::Wait { .. } => "Wait",
            NodeKind::PlayAnimation { .. } => "PlayAnimation",
            NodeKind::SetAnimatorParameter { .. } => "SetAnimatorParameter",
            NodeKind::LookAt { .. } => "LookAt",
            NodeKind::SetBlackboard { .. } => "SetBlackboard",
            NodeKind::IsInRange { .. } => "IsInRange",
            NodeKind::CanSeeTarget { .. } => "CanSeeTarget",
            NodeKind::HasLineOfSight { .. } => "HasLineOfSight",
            NodeKind::RunScriptGraph { .. } => "RunScriptGraph",
            NodeKind::RunLua { .. } => "RunLua",
            NodeKind::CustomLua { .. } => "CustomLua",
            NodeKind::Log { .. } => "Log",
        }
    }

    /// The name shown on the node and in the palette.
    pub fn label(&self) -> &'static str {
        match self {
            NodeKind::Root => "Root",
            NodeKind::Sequence => "Sequence",
            NodeKind::Selector => "Selector",
            NodeKind::Parallel { .. } => "Parallel",
            NodeKind::RandomSelector => "Random Selector",
            NodeKind::Inverter => "Inverter",
            NodeKind::Repeat { .. } => "Repeat",
            NodeKind::Cooldown { .. } => "Cooldown",
            NodeKind::TimeLimit { .. } => "Time Limit",
            NodeKind::Conditional { .. } => "Conditional",
            NodeKind::ForceSuccess => "Force Success",
            NodeKind::ForceFailure => "Force Failure",
            NodeKind::MoveTo { .. } => "Move To",
            NodeKind::Wait { .. } => "Wait",
            NodeKind::PlayAnimation { .. } => "Play Animation",
            NodeKind::SetAnimatorParameter { .. } => "Set Animator Parameter",
            NodeKind::LookAt { .. } => "Look At",
            NodeKind::SetBlackboard { .. } => "Set Blackboard",
            NodeKind::IsInRange { .. } => "Is In Range",
            NodeKind::CanSeeTarget { .. } => "Can See Target",
            NodeKind::HasLineOfSight { .. } => "Has Line of Sight",
            NodeKind::RunScriptGraph { .. } => "Run Script Graph",
            NodeKind::RunLua { .. } => "Run Lua",
            NodeKind::CustomLua { .. } => "Custom Lua Leaf",
            NodeKind::Log { .. } => "Log",
        }
    }

    /// One line on what it does (palette tooltip).
    pub fn help(&self) -> &'static str {
        match self {
            NodeKind::Root => "Where the tree starts.",
            NodeKind::Sequence => "Runs its children in order until one fails.",
            NodeKind::Selector => "Runs its children in order until one succeeds.",
            NodeKind::Parallel { .. } => "Runs all its children at the same time.",
            NodeKind::RandomSelector => "Tries its children in a random order until one succeeds.",
            NodeKind::Inverter => "Turns success into failure and failure into success.",
            NodeKind::Repeat { .. } => {
                "Runs its child again: a number of times, forever, or until it fails."
            }
            NodeKind::Cooldown { .. } => {
                "After its child finishes, fails for a while without running it."
            }
            NodeKind::TimeLimit { .. } => "Stops its child and fails when it takes too long.",
            NodeKind::Conditional { .. } => {
                "Runs its child only while a blackboard key matches; can stop other branches when the key changes."
            }
            NodeKind::ForceSuccess => "Succeeds whatever its child did.",
            NodeKind::ForceFailure => "Fails whatever its child did.",
            NodeKind::MoveTo { .. } => "Walks the Nav Mesh Agent to a position or object.",
            NodeKind::Wait { .. } => "Waits for a number of seconds.",
            NodeKind::PlayAnimation { .. } => "Plays an Animator state.",
            NodeKind::SetAnimatorParameter { .. } => "Sets an Animator parameter.",
            NodeKind::LookAt { .. } => "Turns to face a position or object.",
            NodeKind::SetBlackboard { .. } => "Writes a value into a blackboard key.",
            NodeKind::IsInRange { .. } => "Succeeds when the target is close enough.",
            NodeKind::CanSeeTarget { .. } => {
                "Succeeds when the target is in range, in the field of view and not hidden."
            }
            NodeKind::HasLineOfSight { .. } => {
                "Succeeds when nothing blocks the view of the target."
            }
            NodeKind::RunScriptGraph { .. } => {
                "Sends a custom event to a Script Graph and waits for it to finish."
            }
            NodeKind::RunLua { .. } => "Calls a function of the object's Lua script.",
            NodeKind::CustomLua { .. } => "A leaf written in Lua (OnStart, OnTick, OnAbort).",
            NodeKind::Log { .. } => "Writes a message to the Console.",
        }
    }

    pub fn category(&self) -> Category {
        match self {
            NodeKind::Root => Category::Root,
            NodeKind::Sequence
            | NodeKind::Selector
            | NodeKind::Parallel { .. }
            | NodeKind::RandomSelector => Category::Composite,
            NodeKind::Inverter
            | NodeKind::Repeat { .. }
            | NodeKind::Cooldown { .. }
            | NodeKind::TimeLimit { .. }
            | NodeKind::Conditional { .. }
            | NodeKind::ForceSuccess
            | NodeKind::ForceFailure => Category::Decorator,
            NodeKind::IsInRange { .. }
            | NodeKind::CanSeeTarget { .. }
            | NodeKind::HasLineOfSight { .. } => Category::Condition,
            NodeKind::RunScriptGraph { .. }
            | NodeKind::RunLua { .. }
            | NodeKind::CustomLua { .. } => Category::Scripting,
            _ => Category::Action,
        }
    }

    /// How many children it takes: (min, max).
    pub fn child_limits(&self) -> (usize, usize) {
        match self.category() {
            Category::Root | Category::Decorator => (1, 1),
            Category::Composite => (1, usize::MAX),
            _ => (0, 0),
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.child_limits().1 == 0
    }

    /// Blackboard keys it names: (key, types it accepts, what for).
    pub fn key_uses(&self) -> Vec<(&str, &'static [KeyType], &'static str)> {
        const TARGET: &[KeyType] = &[KeyType::Vector, KeyType::Object];
        const ANY: &[KeyType] = &KeyType::ALL;
        const BOOL: &[KeyType] = &[KeyType::Bool];
        let mut v = Vec::new();
        match self {
            NodeKind::Conditional { key, .. } => v.push((key.as_str(), ANY, "Key")),
            NodeKind::MoveTo { target, .. }
            | NodeKind::LookAt { target, .. }
            | NodeKind::IsInRange { target, .. }
            | NodeKind::HasLineOfSight { target, .. } => {
                v.push((target.as_str(), TARGET, "Target"))
            }
            NodeKind::CanSeeTarget { target, store, .. } => {
                v.push((target.as_str(), TARGET, "Target"));
                if !store.is_empty() {
                    v.push((store.as_str(), BOOL, "Store"));
                }
            }
            NodeKind::SetBlackboard { key, from_key, .. } => {
                v.push((key.as_str(), ANY, "Key"));
                if !from_key.is_empty() {
                    v.push((from_key.as_str(), ANY, "From Key"));
                }
            }
            NodeKind::SetAnimatorParameter { from_key, .. } if !from_key.is_empty() => {
                v.push((from_key.as_str(), ANY, "From Key"));
            }
            _ => {}
        }
        v
    }
}

/// A node of the tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BtNode {
    pub id: u32,
    #[serde(flatten)]
    pub kind: NodeKind,
    /// Title shown instead of the kind's name (empty = the kind's name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Position on the canvas (graph units).
    #[serde(default)]
    pub position: [f32; 2],
    /// Children in the order they run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
}

impl BtNode {
    pub fn new(id: u32, kind: NodeKind, position: [f32; 2]) -> Self {
        Self {
            id,
            kind,
            name: String::new(),
            position,
            children: Vec::new(),
        }
    }

    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            self.kind.label()
        } else {
            &self.name
        }
    }
}

/// A sticky note on the canvas.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Comment {
    pub text: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
}

/// A titled frame around nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub title: String,
    pub nodes: Vec<u32>,
}

/// A Behaviour Tree asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviourTree {
    pub version: u32,
    pub nodes: Vec<BtNode>,
    pub keys: Vec<KeyDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

impl Default for BehaviourTree {
    fn default() -> Self {
        Self {
            version: VERSION,
            nodes: Vec::new(),
            keys: Vec::new(),
            comments: Vec::new(),
            groups: Vec::new(),
        }
    }
}

impl BehaviourTree {
    /// A new asset: a Root with a Selector under it.
    pub fn new_default() -> Self {
        let mut t = BehaviourTree::default();
        let mut root = BtNode::new(1, NodeKind::Root, [0.0, 0.0]);
        root.children.push(2);
        t.nodes.push(root);
        t.nodes
            .push(BtNode::new(2, NodeKind::Selector, [220.0, 0.0]));
        t
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let t: Self = serde_json::from_str(text).map_err(|e| format!("Behaviour Tree: {e}"))?;
        if t.version > VERSION {
            return Err(format!(
                "Behaviour Tree: file version {} is newer than this editor ({VERSION})",
                t.version
            ));
        }
        Ok(t)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        crate::asset_db::write_asset(path, self.to_json())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn node(&self, id: u32) -> Option<&BtNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut BtNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// An id no node has.
    pub fn next_id(&self) -> u32 {
        self.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1
    }

    pub fn root(&self) -> Option<&BtNode> {
        self.nodes.iter().find(|n| n.kind == NodeKind::Root)
    }

    /// The node whose children list holds `id`.
    pub fn parent_of(&self, id: u32) -> Option<u32> {
        self.nodes
            .iter()
            .find(|n| n.children.contains(&id))
            .map(|n| n.id)
    }

    pub fn key(&self, name: &str) -> Option<&KeyDef> {
        self.keys.iter().find(|k| k.name == name)
    }

    /// Remove a node (its children stay, unparented) and every link to it.
    pub fn remove_node(&mut self, id: u32) {
        self.nodes.retain(|n| n.id != id);
        for n in &mut self.nodes {
            n.children.retain(|c| *c != id);
        }
        for g in &mut self.groups {
            g.nodes.retain(|n| *n != id);
        }
        self.groups.retain(|g| !g.nodes.is_empty());
    }

    /// Is `a` the same as `b` or above it?
    pub fn is_ancestor(&self, a: u32, b: u32) -> bool {
        let mut cur = Some(b);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            guard += 1;
            if guard > self.nodes.len() {
                return false;
            }
            cur = self.parent_of(c);
        }
        false
    }

    /// Can `child` go under `parent`? Err holds why not.
    pub fn can_link(&self, parent: u32, child: u32) -> Result<(), String> {
        let p = self.node(parent).ok_or("no such node")?;
        let c = self.node(child).ok_or("no such node")?;
        if c.kind == NodeKind::Root {
            return Err("the Root can't have a parent".into());
        }
        if p.kind.is_leaf() {
            return Err(format!("{} can't have children", p.kind.label()));
        }
        if self.is_ancestor(child, parent) {
            return Err("that would make a loop".into());
        }
        Ok(())
    }

    /// Put `child` under `parent` (leaving its old parent; a single-child node's previous child
    /// is unlinked), keeping children sorted top to bottom.
    pub fn link(&mut self, parent: u32, child: u32) -> Result<(), String> {
        self.can_link(parent, child)?;
        for n in &mut self.nodes {
            n.children.retain(|c| *c != child);
        }
        let single = self
            .node(parent)
            .is_some_and(|p| p.kind.child_limits().1 == 1);
        if let Some(p) = self.node_mut(parent) {
            if single {
                p.children.clear();
            }
            p.children.push(child);
        }
        self.sort_children(parent);
        Ok(())
    }

    /// Remove the link to `child` from its parent.
    pub fn unlink(&mut self, child: u32) {
        for n in &mut self.nodes {
            n.children.retain(|c| *c != child);
        }
    }

    /// Order `parent`'s children by their height on the canvas (top first), then left.
    pub fn sort_children(&mut self, parent: u32) {
        let Some(p) = self.node(parent) else {
            return;
        };
        let mut kids: Vec<(f32, f32, u32)> = p
            .children
            .iter()
            .filter_map(|c| self.node(*c).map(|n| (n.position[1], n.position[0], n.id)))
            .collect();
        kids.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if let Some(p) = self.node_mut(parent) {
            p.children = kids.into_iter().map(|k| k.2).collect();
        }
    }

    /// Re-sort every node's children (after nodes moved).
    pub fn sort_all_children(&mut self) {
        let ids: Vec<u32> = self.nodes.iter().map(|n| n.id).collect();
        for id in ids {
            self.sort_children(id);
        }
    }

    /// Lay the tree out from the Root: one column per depth, leaves one under another and each
    /// parent level with the middle of its children (nodes not under the Root are left alone).
    pub fn arrange(&mut self) {
        const COLUMN: f32 = 250.0;
        const ROW: f32 = 78.0;
        let Some(root) = self.root().map(|n| n.id) else {
            return;
        };
        let origin = self.node(root).map_or([0.0, 0.0], |n| n.position);
        let mut next_row = 0.0f32;
        let mut placed: std::collections::HashMap<u32, [f32; 2]> = Default::default();
        // Post-order with an explicit stack: (node, depth, children done).
        let mut stack = vec![(root, 0usize, false)];
        let mut guard = 0;
        while let Some((id, depth, done)) = stack.pop() {
            guard += 1;
            if guard > self.nodes.len() * 4 + 8 {
                break;
            }
            let kids: Vec<u32> = self
                .node(id)
                .map(|n| n.children.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|c| self.node(*c).is_some() && !placed.contains_key(c))
                .collect();
            if !done && !kids.is_empty() {
                stack.push((id, depth, true));
                for c in kids.iter().rev() {
                    stack.push((*c, depth + 1, false));
                }
                continue;
            }
            let x = origin[0] + depth as f32 * COLUMN;
            let ys: Vec<f32> = self
                .node(id)
                .map(|n| n.children.clone())
                .unwrap_or_default()
                .iter()
                .filter_map(|c| placed.get(c).map(|p| p[1]))
                .collect();
            let y = if ys.is_empty() {
                let y = origin[1] + next_row * ROW;
                next_row += 1.0;
                y
            } else {
                (ys[0] + ys[ys.len() - 1]) * 0.5
            };
            placed.insert(id, [x, y]);
        }
        // Centre the tree on the Root's old height.
        let shift = placed.get(&root).map_or(0.0, |p| origin[1] - p[1]);
        for n in &mut self.nodes {
            if let Some(p) = placed.get(&n.id) {
                n.position = [p[0], p[1] + shift];
            }
        }
    }

    /// Rename a key everywhere nodes use it.
    pub fn rename_key(&mut self, old: &str, new: &str) {
        for k in &mut self.keys {
            if k.name == old {
                k.name = new.to_owned();
            }
        }
        let fix = |s: &mut String| {
            if s == old {
                *s = new.to_owned();
            }
        };
        for n in &mut self.nodes {
            match &mut n.kind {
                NodeKind::Conditional { key, .. } => fix(key),
                NodeKind::MoveTo { target, .. }
                | NodeKind::LookAt { target, .. }
                | NodeKind::IsInRange { target, .. }
                | NodeKind::HasLineOfSight { target, .. } => fix(target),
                NodeKind::CanSeeTarget { target, store, .. } => {
                    fix(target);
                    fix(store);
                }
                NodeKind::SetBlackboard { key, from_key, .. } => {
                    fix(key);
                    fix(from_key);
                }
                NodeKind::SetAnimatorParameter { from_key, .. } => fix(from_key),
                _ => {}
            }
        }
    }
}
