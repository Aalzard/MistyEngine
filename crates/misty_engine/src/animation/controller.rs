//! Animator Controller asset (`.controller`, JSON): parameters and layers. Each layer is a state
//! machine whose states play clips or blend trees, linked by transitions with conditions, and
//! which may nest sub-state machines. Layers above the base one override or add to it, limited
//! to the bones of an Avatar Mask; a synced layer reuses another layer's states with its own
//! motions.
//!
//! File format: the base layer's machine is stored at the top level (`states`, `transitions`,
//! `default_state`, `nodes`, `sub_machines`), so files from before layers existed load
//! unchanged; further layers are listed in `layers`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::blend;

/// File extension of animator controllers.
pub const CONTROLLER_EXTENSION: &str = "controller";

/// Name of layer 0.
pub const BASE_LAYER: &str = "Base Layer";

pub fn is_controller(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(CONTROLLER_EXTENSION))
}

/// Type of an Animator parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamKind {
    #[default]
    Float,
    Int,
    Bool,
    /// A bool that switches itself off once a transition uses it.
    Trigger,
}

impl ParamKind {
    pub const ALL: [Self; 4] = [Self::Float, Self::Int, Self::Bool, Self::Trigger];

    pub fn label(self) -> &'static str {
        match self {
            Self::Float => "Float",
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Trigger => "Trigger",
        }
    }

    /// Condition modes that make sense for this type.
    pub fn modes(self) -> &'static [ConditionMode] {
        use ConditionMode::*;
        match self {
            Self::Float => &[Greater, Less],
            Self::Int => &[Greater, Less, Equals, NotEqual],
            Self::Bool => &[If, IfNot],
            Self::Trigger => &[If],
        }
    }
}

/// A named value scripts set and transitions test. Bools and triggers are 0 or 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ParamKind,
    /// Value when Play starts.
    #[serde(default)]
    pub default: f32,
}

/// How a condition compares its parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionMode {
    /// Bool/trigger is on.
    #[default]
    If,
    IfNot,
    Greater,
    Less,
    Equals,
    NotEqual,
}

impl ConditionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::If => "true",
            Self::IfNot => "false",
            Self::Greater => "Greater",
            Self::Less => "Less",
            Self::Equals => "Equals",
            Self::NotEqual => "NotEqual",
        }
    }

    /// Whether the mode compares against a threshold.
    pub fn uses_threshold(self) -> bool {
        !matches!(self, Self::If | Self::IfNot)
    }

    /// Test a parameter value (ints are compared after rounding).
    pub fn test(self, value: f32, threshold: f32) -> bool {
        match self {
            Self::If => value >= 0.5,
            Self::IfNot => value < 0.5,
            Self::Greater => value > threshold,
            Self::Less => value < threshold,
            Self::Equals => value.round() == threshold.round(),
            Self::NotEqual => value.round() != threshold.round(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub parameter: String,
    pub mode: ConditionMode,
    #[serde(default)]
    pub threshold: f32,
}

/// One end of a transition: a state, a sub-state machine or one of the special nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Node {
    Entry,
    AnyState,
    Exit,
    State(String),
    /// A sub-state machine by path ("Combat", "Combat/Melee"). As a destination it enters the
    /// machine through its Entry; as a source its transitions are checked when the machine is
    /// left through its Exit.
    Machine(String),
    /// The "(Up)" node of a sub-state machine graph, standing for the parent machine. Only the
    /// graph editor uses it; transitions never store it.
    Up,
}

impl Node {
    pub fn state(name: &str) -> Self {
        Self::State(name.to_owned())
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Entry => "Entry",
            Self::AnyState => "Any State",
            Self::Exit => "Exit",
            Self::State(n) => n,
            Self::Machine(p) => machine_name(p),
            Self::Up => "(Up)",
        }
    }
}

/// Which transitions may interrupt a transition in progress (Any State transitions always can).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterruptionSource {
    #[default]
    None,
    /// Transitions leaving the state being left.
    CurrentState,
    /// Transitions leaving the state being entered.
    NextState,
    CurrentStateThenNextState,
    NextStateThenCurrentState,
}

impl InterruptionSource {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::CurrentState,
        Self::NextState,
        Self::CurrentStateThenNextState,
        Self::NextStateThenCurrentState,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::CurrentState => "Current State",
            Self::NextState => "Next State",
            Self::CurrentStateThenNextState => "Current State Then Next State",
            Self::NextStateThenCurrentState => "Next State Then Current State",
        }
    }

    /// The groups checked, in order: `false` = current (source) state, `true` = next state.
    pub fn order(self) -> &'static [bool] {
        match self {
            Self::None => &[],
            Self::CurrentState => &[false],
            Self::NextState => &[true],
            Self::CurrentStateThenNextState => &[false, true],
            Self::NextStateThenCurrentState => &[true, false],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transition {
    /// A state, Any State, Entry or a sub-state machine.
    pub from: Node,
    /// A state, a sub-state machine or Exit (leave the machine the source is in).
    pub to: Node,
    /// Wait until the source state reaches `exit_time` (normalised) before leaving.
    pub has_exit_time: bool,
    pub exit_time: f32,
    /// Duration counts seconds (true) or source-state lengths (false).
    pub fixed_duration: bool,
    /// Cross-fade length.
    pub duration: f32,
    /// Normalised time the destination starts at.
    pub offset: f32,
    /// All must hold (none = only the exit time decides).
    pub conditions: Vec<Condition>,
    /// What may interrupt this transition while it blends.
    pub interruption_source: InterruptionSource,
    /// Only transitions listed before this one (of the current state) may interrupt it.
    pub ordered_interruption: bool,
    /// Any State only: may restart the state that is already playing.
    pub can_transition_to_self: bool,
    /// Never taken.
    pub mute: bool,
    /// When any transition of a state is solo, only solo ones are taken.
    pub solo: bool,
    /// Sub-state machine graph the transition belongs to, for Entry and Any State transitions
    /// ("" = the layer's top machine).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub machine: String,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            from: Node::AnyState,
            to: Node::Exit,
            has_exit_time: true,
            exit_time: 0.75,
            fixed_duration: true,
            duration: 0.25,
            offset: 0.0,
            conditions: Vec::new(),
            interruption_source: InterruptionSource::None,
            ordered_interruption: true,
            can_transition_to_self: false,
            mute: false,
            solo: false,
            machine: String::new(),
        }
    }
}

impl Transition {
    pub fn new(from: Node, to: Node) -> Self {
        Self {
            // Any State and Entry transitions fire on a condition, not at an exit time.
            has_exit_time: !matches!(from, Node::AnyState | Node::Entry | Node::Machine(_)),
            from,
            to,
            ..Default::default()
        }
    }

    /// "Idle -> Walk".
    pub fn label(&self) -> String {
        format!("{} -> {}", self.from.label(), self.to.label())
    }
}

/// How a blend tree turns its parameters into child weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendType {
    /// One float, children at thresholds along a line.
    #[default]
    Simple1D,
    /// Two floats, each child a direction (one may be at the centre).
    SimpleDirectional2D,
    /// Two floats, children at directions and speeds (several per direction allowed).
    FreeformDirectional2D,
    /// Two floats, children anywhere on the plane.
    FreeformCartesian2D,
    /// Each child's weight is its own parameter.
    Direct,
}

impl BlendType {
    pub const ALL: [Self; 5] = [
        Self::Simple1D,
        Self::SimpleDirectional2D,
        Self::FreeformDirectional2D,
        Self::FreeformCartesian2D,
        Self::Direct,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple1D => "1D",
            Self::SimpleDirectional2D => "2D Simple Directional",
            Self::FreeformDirectional2D => "2D Freeform Directional",
            Self::FreeformCartesian2D => "2D Freeform Cartesian",
            Self::Direct => "Direct",
        }
    }

    pub fn is_2d(self) -> bool {
        matches!(
            self,
            Self::SimpleDirectional2D | Self::FreeformDirectional2D | Self::FreeformCartesian2D
        )
    }
}

/// One child of a blend tree: a clip placed at a threshold (1D), a position (2D) or driven by
/// its own parameter (Direct).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendChild {
    pub motion: Option<String>,
    pub threshold: f32,
    /// 2D position (x = first parameter, y = second).
    pub position: [f32; 2],
    /// Direct blend trees: the float parameter giving this child's weight.
    pub direct_parameter: String,
    /// Playback speed of this child (its share of the blended length is divided by it).
    pub time_scale: f32,
}

impl Default for BlendChild {
    fn default() -> Self {
        Self {
            motion: None,
            threshold: 0.0,
            position: [0.0, 0.0],
            direct_parameter: String::new(),
            time_scale: 1.0,
        }
    }
}

impl BlendChild {
    pub fn clip(motion: &str) -> Self {
        Self {
            motion: Some(motion.to_owned()),
            ..Default::default()
        }
    }
}

/// Blends clips by parameters; see [`BlendType`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendTree {
    pub blend_type: BlendType,
    /// The (first) float parameter.
    pub parameter: String,
    /// The second float parameter of 2D trees.
    pub parameter_y: String,
    /// Direct trees: scale the weights so they sum to 1.
    pub normalize: bool,
    pub children: Vec<BlendChild>,
}

impl BlendTree {
    /// Weight of every child for parameter values `x` (and `y` for 2D trees); Direct trees read
    /// each child's parameter through `param`.
    pub fn weights_at(&self, x: f32, y: f32, param: &dyn Fn(&str) -> f32) -> Vec<f32> {
        let points = || -> Vec<glam::Vec2> {
            self.children
                .iter()
                .map(|c| glam::Vec2::from(c.position))
                .collect()
        };
        let p = glam::Vec2::new(x, y);
        match self.blend_type {
            BlendType::Simple1D => {
                let th: Vec<f32> = self.children.iter().map(|c| c.threshold).collect();
                blend::simple_1d(&th, x)
            }
            BlendType::SimpleDirectional2D => blend::simple_directional(&points(), p),
            BlendType::FreeformDirectional2D => blend::freeform_directional(&points(), p),
            BlendType::FreeformCartesian2D => blend::freeform_cartesian(&points(), p),
            BlendType::Direct => {
                let v: Vec<f32> = self
                    .children
                    .iter()
                    .map(|c| param(&c.direct_parameter))
                    .collect();
                blend::direct(&v, self.normalize)
            }
        }
    }

    /// Children with a weight, for the parameter values `param` gives.
    pub fn weights(&self, param: &dyn Fn(&str) -> f32) -> Vec<(usize, f32)> {
        let (x, y) = (param(&self.parameter), param(&self.parameter_y));
        self.weights_at(x, y, param)
            .into_iter()
            .enumerate()
            .filter(|(_, w)| *w > 1e-6)
            .collect()
    }

    /// Every parameter name the tree reads.
    pub fn parameters_mut(&mut self) -> Vec<&mut String> {
        let mut out = vec![&mut self.parameter, &mut self.parameter_y];
        out.extend(self.children.iter_mut().map(|c| &mut c.direct_parameter));
        out
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub name: String,
    /// Clip asset path; ignored when the state has a blend tree.
    pub motion: Option<String>,
    pub blend_tree: Option<BlendTree>,
    /// Playback speed multiplier (negative plays backwards).
    pub speed: f32,
    /// Float parameter multiplying `speed` ("" = none).
    pub speed_parameter: String,
    /// Wrap at the end of the clip; otherwise hold the last pose.
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Free text scripts can test (`AnimatorStateInfo:IsTag`).
    pub tag: String,
    /// Sub-state machine the state lives in ("" = the layer's top machine).
    pub machine: String,
    /// Node position in the Animator window graph.
    pub position: [f32; 2],
}

impl Default for State {
    fn default() -> Self {
        Self {
            name: "New State".into(),
            motion: None,
            blend_tree: None,
            speed: 1.0,
            speed_parameter: String::new(),
            looping: true,
            tag: String::new(),
            machine: String::new(),
            position: [0.0, 0.0],
        }
    }
}

/// Graph positions of the special nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodePositions {
    pub entry: [f32; 2],
    pub any_state: [f32; 2],
    pub exit: [f32; 2],
    /// "(Up)" node (sub-state machines only).
    pub up: [f32; 2],
}

impl Default for NodePositions {
    fn default() -> Self {
        Self {
            entry: [-250.0, 0.0],
            any_state: [-250.0, -80.0],
            exit: [350.0, 0.0],
            up: [350.0, -80.0],
        }
    }
}

/// A sub-state machine: a group of states with its own Entry, Exit and default state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubMachine {
    /// Parent machines and name joined by '/' ("Combat/Melee").
    pub path: String,
    /// Node position in the parent machine's graph.
    pub position: [f32; 2],
    /// State Entry goes to (it may be inside a nested machine).
    pub default_state: Option<String>,
    pub nodes: NodePositions,
}

/// Last segment of a machine path ("Combat/Melee" → "Melee").
pub fn machine_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Parent of a machine path ("Combat/Melee" → "Combat", "Combat" → "").
pub fn machine_parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Whether machine `inner` is `outer` or nested in it ("" contains everything).
pub fn machine_within(inner: &str, outer: &str) -> bool {
    outer.is_empty()
        || inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// `base`, or `base 1`, `base 2`… if taken by `taken`.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (0..)
        .map(|n| match n {
            0 => base.to_owned(),
            n => format!("{base} {n}"),
        })
        .find(|n| !taken(n))
        .expect("some name is free")
}

/// One layer's state machine: states (in any sub-machine), transitions and sub-machines.
/// State names are unique across the whole layer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateMachine {
    pub states: Vec<State>,
    /// Name of the state the top machine's Entry goes to.
    pub default_state: Option<String>,
    pub transitions: Vec<Transition>,
    pub nodes: NodePositions,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sub_machines: Vec<SubMachine>,
}

impl StateMachine {
    /// Fix what a hand-edited file may get wrong: missing parents and default states,
    /// states in machines that don't exist, dangling transitions.
    fn sanitize(&mut self) {
        self.sub_machines.retain(|m| !m.path.is_empty());
        let mut i = 0;
        while i < self.sub_machines.len() {
            let parent = machine_parent(&self.sub_machines[i].path).to_owned();
            if !parent.is_empty() && !self.machine_exists(&parent) {
                self.sub_machines.push(SubMachine {
                    path: parent,
                    ..Default::default()
                });
            }
            i += 1;
        }
        for s in &mut self.states {
            if !s.machine.is_empty() && !self.sub_machines.iter().any(|m| m.path == s.machine) {
                s.machine.clear();
            }
        }
        let paths: Vec<String> = self.sub_machines.iter().map(|m| m.path.clone()).collect();
        for p in std::iter::once(String::new()).chain(paths) {
            let ok = self.default_of(&p).is_some_and(|d| {
                self.state(d)
                    .is_some_and(|s| machine_within(&s.machine, &p))
            });
            if !ok {
                let first = self
                    .states
                    .iter()
                    .find(|s| machine_within(&s.machine, &p))
                    .map(|s| s.name.clone());
                self.set_default(&p, first);
            }
        }
        let names: Vec<String> = self.states.iter().map(|s| s.name.clone()).collect();
        let machines: Vec<String> = self.sub_machines.iter().map(|m| m.path.clone()).collect();
        let valid = |n: &Node| match n {
            Node::State(s) => names.contains(s),
            Node::Machine(m) => machines.contains(m),
            Node::Up => false,
            _ => true,
        };
        self.transitions.retain(|t| {
            valid(&t.from)
                && valid(&t.to)
                && !matches!(t.from, Node::Exit)
                && !matches!(t.to, Node::Entry | Node::AnyState)
        });
    }

    pub fn state_index(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|s| s.name == name)
    }

    pub fn state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }

    pub fn state_mut(&mut self, name: &str) -> Option<&mut State> {
        self.states.iter_mut().find(|s| s.name == name)
    }

    pub fn machine_exists(&self, path: &str) -> bool {
        path.is_empty() || self.sub_machines.iter().any(|m| m.path == path)
    }

    pub fn sub(&self, path: &str) -> Option<&SubMachine> {
        self.sub_machines.iter().find(|m| m.path == path)
    }

    pub fn sub_mut(&mut self, path: &str) -> Option<&mut SubMachine> {
        self.sub_machines.iter_mut().find(|m| m.path == path)
    }

    /// Default state of a machine ("" = the top machine).
    pub fn default_of(&self, machine: &str) -> Option<&str> {
        if machine.is_empty() {
            self.default_state.as_deref()
        } else {
            self.sub(machine).and_then(|m| m.default_state.as_deref())
        }
    }

    pub fn set_default(&mut self, machine: &str, name: Option<String>) {
        if machine.is_empty() {
            self.default_state = name;
        } else if let Some(m) = self.sub_mut(machine) {
            m.default_state = name;
        }
    }

    /// Special node positions of a machine's graph.
    pub fn nodes_of(&self, machine: &str) -> &NodePositions {
        self.sub(machine).map_or(&self.nodes, |m| &m.nodes)
    }

    pub fn nodes_of_mut(&mut self, machine: &str) -> &mut NodePositions {
        if machine.is_empty() || !self.machine_exists(machine) {
            return &mut self.nodes;
        }
        &mut self.sub_mut(machine).expect("exists").nodes
    }

    /// Sub-machines directly inside `machine`.
    pub fn children_of<'a>(&'a self, machine: &'a str) -> impl Iterator<Item = &'a SubMachine> {
        self.sub_machines
            .iter()
            .filter(move |m| machine_parent(&m.path) == machine)
    }

    /// Machine a transition is evaluated and drawn in: the source state's machine, the parent of
    /// a source sub-machine, or the `machine` field for Entry and Any State.
    pub fn context(&self, t: &Transition) -> String {
        match &t.from {
            Node::State(s) => self.state(s).map(|s| s.machine.clone()).unwrap_or_default(),
            Node::Machine(m) => machine_parent(m).to_owned(),
            _ => t.machine.clone(),
        }
    }

    /// Add a state with a unique name to the top machine; the first state becomes the default.
    pub fn add_state(&mut self, base: &str, position: [f32; 2]) -> String {
        self.add_state_in(base, "", position)
    }

    /// Add a state to a machine; it becomes that machine's default if it has none.
    pub fn add_state_in(&mut self, base: &str, machine: &str, position: [f32; 2]) -> String {
        let name = unique(base, |n| self.state_index(n).is_some());
        self.states.push(State {
            name: name.clone(),
            machine: machine.to_owned(),
            position,
            ..Default::default()
        });
        if self.default_of(machine).is_none() {
            self.set_default(machine, Some(name.clone()));
        }
        if !machine.is_empty() && self.default_state.is_none() {
            self.default_state = Some(name.clone());
        }
        name
    }

    /// Rename a state (made unique) and follow it in transitions and default states.
    pub fn rename_state(&mut self, old: &str, new: &str) -> Option<String> {
        let i = self.state_index(old)?;
        let new = new.trim();
        if new.is_empty() || new == old {
            return Some(old.to_owned());
        }
        let name = unique(new, |n| n != old && self.state_index(n).is_some());
        self.states[i].name = name.clone();
        for t in &mut self.transitions {
            for n in [&mut t.from, &mut t.to] {
                if *n == Node::state(old) {
                    *n = Node::state(&name);
                }
            }
        }
        let defaults = std::iter::once(&mut self.default_state)
            .chain(self.sub_machines.iter_mut().map(|m| &mut m.default_state));
        for d in defaults {
            if d.as_deref() == Some(old) {
                *d = Some(name.clone());
            }
        }
        Some(name)
    }

    /// Remove a state and its transitions; defaults move to the first remaining state.
    pub fn remove_state(&mut self, name: &str) {
        self.states.retain(|s| s.name != name);
        let node = Node::state(name);
        self.transitions.retain(|t| t.from != node && t.to != node);
        self.repair_defaults();
    }

    fn repair_defaults(&mut self) {
        let paths: Vec<String> = self.sub_machines.iter().map(|m| m.path.clone()).collect();
        for p in std::iter::once(String::new()).chain(paths) {
            let ok = self.default_of(&p).is_some_and(|d| self.state(d).is_some());
            if !ok {
                let first = self
                    .states
                    .iter()
                    .find(|s| machine_within(&s.machine, &p))
                    .map(|s| s.name.clone());
                self.set_default(&p, first);
            }
        }
    }

    /// Move a state into another machine (its transitions stay).
    pub fn move_state(&mut self, name: &str, machine: &str) {
        if !self.machine_exists(machine) {
            return;
        }
        if let Some(s) = self.state_mut(name) {
            s.machine = machine.to_owned();
        }
        let wrong = |m: &StateMachine, p: &str| {
            m.default_of(p)
                .is_some_and(|d| m.state(d).is_some_and(|s| !machine_within(&s.machine, p)))
        };
        let paths: Vec<String> = self.sub_machines.iter().map(|m| m.path.clone()).collect();
        for p in paths {
            if wrong(self, &p) {
                self.set_default(&p, None);
            }
        }
        if self.default_of(machine).is_none() {
            self.set_default(machine, Some(name.to_owned()));
        }
        self.repair_defaults();
    }

    /// Add a sub-state machine inside `parent`; returns its path.
    pub fn add_sub_machine(&mut self, base: &str, parent: &str, position: [f32; 2]) -> String {
        let join = |n: &str| {
            if parent.is_empty() {
                n.to_owned()
            } else {
                format!("{parent}/{n}")
            }
        };
        let name = unique(base, |n| self.machine_exists(&join(n)));
        let path = join(&name);
        self.sub_machines.push(SubMachine {
            path: path.clone(),
            position,
            ..Default::default()
        });
        path
    }

    /// Rename a sub-machine (its last segment); nested machines, states and transitions follow.
    pub fn rename_sub_machine(&mut self, path: &str, new: &str) -> Option<String> {
        let new = new.trim().replace('/', " ");
        self.sub(path)?;
        if new.is_empty() || new == machine_name(path) {
            return Some(path.to_owned());
        }
        let parent = machine_parent(path).to_owned();
        let join = |n: &str| {
            if parent.is_empty() {
                n.to_owned()
            } else {
                format!("{parent}/{n}")
            }
        };
        let name = unique(&new, |n| join(n) != path && self.machine_exists(&join(n)));
        let to = join(&name);
        let remap = |p: &mut String| {
            if machine_within(p, path) {
                *p = format!("{to}{}", &p[path.len()..]);
            }
        };
        for m in &mut self.sub_machines {
            remap(&mut m.path);
        }
        for s in &mut self.states {
            if !s.machine.is_empty() {
                remap(&mut s.machine);
            }
        }
        for t in &mut self.transitions {
            for n in [&mut t.from, &mut t.to] {
                if let Node::Machine(p) = n {
                    remap(p);
                }
            }
            if !t.machine.is_empty() {
                remap(&mut t.machine);
            }
        }
        Some(to)
    }

    /// Remove a sub-machine with everything inside it.
    pub fn remove_sub_machine(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        let gone: Vec<String> = self
            .states
            .iter()
            .filter(|s| machine_within(&s.machine, path))
            .map(|s| s.name.clone())
            .collect();
        self.states.retain(|s| !machine_within(&s.machine, path));
        self.sub_machines.retain(|m| !machine_within(&m.path, path));
        self.transitions.retain(|t| {
            let dead = |n: &Node| match n {
                Node::State(s) => gone.contains(s),
                Node::Machine(m) => machine_within(m, path),
                _ => false,
            };
            !dead(&t.from)
                && !dead(&t.to)
                && (t.machine.is_empty() || !machine_within(&t.machine, path))
        });
        self.repair_defaults();
    }

    /// Transitions leaving `from`, in evaluation order (muted ones dropped; when any is solo,
    /// only solo ones).
    pub fn transitions_from<'a>(
        &'a self,
        from: &'a Node,
    ) -> impl Iterator<Item = (usize, &'a Transition)> {
        let solo = self
            .transitions
            .iter()
            .any(|t| t.from == *from && t.solo && !t.mute);
        self.transitions
            .iter()
            .enumerate()
            .filter(move |(_, t)| t.from == *from && !t.mute && (!solo || t.solo))
    }

    /// Clips the machine's states play.
    fn motions_mut(&mut self) -> Vec<&mut String> {
        let mut out = Vec::new();
        for s in &mut self.states {
            out.extend(s.motion.as_mut());
            if let Some(b) = s.blend_tree.as_mut() {
                out.extend(b.children.iter_mut().filter_map(|c| c.motion.as_mut()));
            }
        }
        out
    }
}

/// How a layer combines with the layers below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerBlending {
    /// Replace the pose below by the layer weight.
    #[default]
    Override,
    /// Add the layer's motion (relative to each clip's first frame), scaled by the weight.
    Additive,
}

impl LayerBlending {
    pub const ALL: [Self; 2] = [Self::Override, Self::Additive];

    pub fn label(self) -> &'static str {
        match self {
            Self::Override => "Override",
            Self::Additive => "Additive",
        }
    }
}

/// A layer above the base layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    pub name: String,
    /// Weight when Play starts (scripts change it with `SetLayerWeight`).
    pub weight: f32,
    pub blending: LayerBlending,
    /// Avatar Mask asset limiting the layer to some bones (None = everything).
    pub mask: Option<String>,
    /// Reuse this layer's state machine (its states and transitions) with the motions below.
    pub sync_source: Option<usize>,
    /// Synced layers: clip each source state plays on this layer (missing = nothing).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sync_motions: BTreeMap<String, String>,
    #[serde(flatten)]
    pub machine: StateMachine,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            name: "New Layer".into(),
            weight: 1.0,
            blending: LayerBlending::Override,
            mask: None,
            sync_source: None,
            sync_motions: BTreeMap::new(),
            machine: StateMachine::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimatorController {
    pub parameters: Vec<Parameter>,
    /// Layer 0 ("Base Layer"), stored at the top level of the file.
    #[serde(flatten)]
    pub base: StateMachine,
    /// Layers 1, 2, … in evaluation order (later layers win).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

impl AnimatorController {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut c: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        c.sanitize();
        Ok(c)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn sanitize(&mut self) {
        self.base.sanitize();
        for l in &mut self.layers {
            l.machine.sanitize();
        }
        let n = self.layer_count();
        for i in 1..n {
            let src = self.layers[i - 1].sync_source;
            let ok = src.is_some_and(|s| {
                s != i && s < n && (s == 0 || self.layers[s - 1].sync_source.is_none())
            });
            if !ok {
                self.layers[i - 1].sync_source = None;
            }
        }
    }

    /// Base layer plus the extra layers.
    pub fn layer_count(&self) -> usize {
        1 + self.layers.len()
    }

    pub fn layer_name(&self, layer: usize) -> &str {
        match layer {
            0 => BASE_LAYER,
            i => self.layers.get(i - 1).map_or("", |l| l.name.as_str()),
        }
    }

    pub fn layer_index(&self, name: &str) -> Option<usize> {
        (0..self.layer_count()).find(|&i| self.layer_name(i) == name)
    }

    /// Settings of layer `i` ≥ 1.
    pub fn layer(&self, i: usize) -> Option<&Layer> {
        i.checked_sub(1).and_then(|i| self.layers.get(i))
    }

    pub fn layer_mut(&mut self, i: usize) -> Option<&mut Layer> {
        i.checked_sub(1).and_then(|i| self.layers.get_mut(i))
    }

    /// The state machine a layer owns (a synced layer's own machine is unused).
    pub fn machine(&self, layer: usize) -> Option<&StateMachine> {
        match layer {
            0 => Some(&self.base),
            i => self.layer(i).map(|l| &l.machine),
        }
    }

    pub fn machine_mut(&mut self, layer: usize) -> Option<&mut StateMachine> {
        match layer {
            0 => Some(&mut self.base),
            i => self.layer_mut(i).map(|l| &mut l.machine),
        }
    }

    /// Layer whose state machine runs `layer` (its sync source, or itself).
    pub fn machine_layer(&self, layer: usize) -> usize {
        self.layer(layer)
            .and_then(|l| l.sync_source)
            .unwrap_or(layer)
    }

    /// Add a layer with a unique name; returns its index.
    pub fn add_layer(&mut self, base: &str) -> usize {
        let name = unique(base, |n| self.layer_index(n).is_some());
        self.layers.push(Layer {
            name,
            ..Default::default()
        });
        self.layers.len()
    }

    /// Remove layer `i` ≥ 1; layers synced to it stop syncing, later sync indices follow.
    pub fn remove_layer(&mut self, i: usize) {
        if i == 0 || i > self.layers.len() {
            return;
        }
        self.layers.remove(i - 1);
        for l in &mut self.layers {
            l.sync_source = match l.sync_source {
                Some(s) if s == i => None,
                Some(s) if s > i => Some(s - 1),
                s => s,
            };
        }
    }

    /// Rename layer `i` ≥ 1 (made unique).
    pub fn rename_layer(&mut self, i: usize, new: &str) -> Option<String> {
        let new = new.trim();
        let old = self.layer(i)?.name.clone();
        if new.is_empty() || new == old {
            return Some(old);
        }
        let name = unique(new, |n| n != old && self.layer_index(n).is_some());
        self.layer_mut(i)?.name = name.clone();
        Some(name)
    }

    /// Rename a state of a layer; layers synced to it follow.
    pub fn rename_state(&mut self, layer: usize, old: &str, new: &str) -> Option<String> {
        let name = self.machine_mut(layer)?.rename_state(old, new)?;
        for l in &mut self.layers {
            if l.sync_source == Some(layer)
                && let Some(m) = l.sync_motions.remove(old)
            {
                l.sync_motions.insert(name.clone(), m);
            }
        }
        Some(name)
    }

    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Add a parameter with a unique name ("New Float", "New Bool 1", …).
    pub fn add_parameter(&mut self, kind: ParamKind) -> String {
        let name = unique(&format!("New {}", kind.label()), |n| {
            self.parameter(n).is_some()
        });
        self.parameters.push(Parameter {
            name: name.clone(),
            kind,
            default: 0.0,
        });
        name
    }

    fn machines_mut(&mut self) -> impl Iterator<Item = &mut StateMachine> {
        std::iter::once(&mut self.base).chain(self.layers.iter_mut().map(|l| &mut l.machine))
    }

    /// Rename a parameter (made unique) and follow it in conditions, blend trees and speed
    /// multipliers of every layer.
    pub fn rename_parameter(&mut self, old: &str, new: &str) -> Option<String> {
        let i = self.parameters.iter().position(|p| p.name == old)?;
        let new = new.trim();
        if new.is_empty() || new == old {
            return Some(old.to_owned());
        }
        let name = unique(new, |n| n != old && self.parameter(n).is_some());
        self.parameters[i].name = name.clone();
        for m in self.machines_mut() {
            for c in m.transitions.iter_mut().flat_map(|t| &mut t.conditions) {
                if c.parameter == old {
                    c.parameter = name.clone();
                }
            }
            for s in &mut m.states {
                if s.speed_parameter == old {
                    s.speed_parameter = name.clone();
                }
                for p in s.blend_tree.iter_mut().flat_map(|b| b.parameters_mut()) {
                    if p == old {
                        *p = name.clone();
                    }
                }
            }
        }
        Some(name)
    }

    /// Remove a parameter and the conditions that test it.
    pub fn remove_parameter(&mut self, name: &str) {
        self.parameters.retain(|p| p.name != name);
        for m in self.machines_mut() {
            for t in &mut m.transitions {
                t.conditions.retain(|c| c.parameter != name);
            }
        }
    }

    /// Every asset path the controller names (clips, masks), for following renames.
    pub fn asset_paths_mut(&mut self) -> Vec<&mut String> {
        let mut out = self.base.motions_mut();
        for l in &mut self.layers {
            out.extend(l.mask.as_mut());
            out.extend(l.sync_motions.values_mut());
            out.extend(l.machine.motions_mut());
        }
        out
    }

    /// Every clip the controller plays (states, blend tree children, synced motions).
    pub fn motions(&self) -> Vec<String> {
        let mut c = self.clone();
        let mut out: Vec<String> = Vec::new();
        for p in c.asset_paths_mut() {
            if super::is_clip(p) && !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }
}
