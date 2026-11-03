//! Animator Controller asset (`.controller`, JSON): parameters and one base-layer state machine
//! whose states play clips (or a 1D blend tree of clips), linked by transitions with conditions.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File extension of animator controllers.
pub const CONTROLLER_EXTENSION: &str = "controller";

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

/// One end of a transition: a state or one of the special nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Node {
    Entry,
    AnyState,
    Exit,
    State(String),
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transition {
    /// A state or Any State.
    pub from: Node,
    /// A state or Exit (back to the default state).
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
        }
    }
}

impl Transition {
    pub fn new(from: Node, to: Node) -> Self {
        Self {
            // Any State transitions usually fire on a condition, not at an exit time.
            has_exit_time: from != Node::AnyState,
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

/// One child of a 1D blend tree: a clip placed at a parameter value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendChild {
    pub motion: Option<String>,
    pub threshold: f32,
}

/// Blends clips by one float parameter (neighbouring children are mixed linearly).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendTree {
    pub parameter: String,
    pub children: Vec<BlendChild>,
}

impl BlendTree {
    /// Weights of the children for a parameter value (sorted by threshold, clamped at the ends).
    pub fn weights(&self, value: f32) -> Vec<(usize, f32)> {
        let mut order: Vec<usize> = (0..self.children.len()).collect();
        order.sort_by(|a, b| {
            self.children[*a]
                .threshold
                .total_cmp(&self.children[*b].threshold)
        });
        let th = |i: usize| self.children[order[i]].threshold;
        let n = order.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 || value <= th(0) {
            return vec![(order[0], 1.0)];
        }
        if value >= th(n - 1) {
            return vec![(order[n - 1], 1.0)];
        }
        let i = (0..n - 1).find(|&i| value <= th(i + 1)).unwrap_or(n - 2);
        let span = th(i + 1) - th(i);
        let w = if span > 0.0 {
            (value - th(i)) / span
        } else {
            1.0
        };
        vec![(order[i], 1.0 - w), (order[i + 1], w)]
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
    /// Wrap at the end of the clip; otherwise hold the last pose.
    #[serde(rename = "loop")]
    pub looping: bool,
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
            looping: true,
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
}

impl Default for NodePositions {
    fn default() -> Self {
        Self {
            entry: [-250.0, 0.0],
            any_state: [-250.0, -80.0],
            exit: [350.0, 0.0],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimatorController {
    pub parameters: Vec<Parameter>,
    pub states: Vec<State>,
    /// Name of the state Entry goes to.
    pub default_state: Option<String>,
    pub transitions: Vec<Transition>,
    pub nodes: NodePositions,
}

impl AnimatorController {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut c: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
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
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Fix what a hand-edited file may get wrong: a missing default state, dangling transitions.
    fn sanitize(&mut self) {
        if self
            .default_state
            .as_ref()
            .is_none_or(|d| self.state_index(d).is_none())
        {
            self.default_state = self.states.first().map(|s| s.name.clone());
        }
        let names: Vec<String> = self.states.iter().map(|s| s.name.clone()).collect();
        let valid = |n: &Node| match n {
            Node::State(s) => names.contains(s),
            _ => true,
        };
        self.transitions.retain(|t| {
            valid(&t.from) && valid(&t.to) && !matches!(t.from, Node::Exit | Node::Entry)
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

    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name == name)
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

    /// Add a state with a unique name at a graph position; the first state becomes the default.
    pub fn add_state(&mut self, base: &str, position: [f32; 2]) -> String {
        let name = Self::unique(base, |n| self.state_index(n).is_some());
        self.states.push(State {
            name: name.clone(),
            position,
            ..Default::default()
        });
        if self.default_state.is_none() {
            self.default_state = Some(name.clone());
        }
        name
    }

    /// Rename a state (made unique) and follow it in transitions and the default state.
    pub fn rename_state(&mut self, old: &str, new: &str) -> Option<String> {
        let i = self.state_index(old)?;
        let new = new.trim();
        if new.is_empty() || new == old {
            return Some(old.to_owned());
        }
        let name = Self::unique(new, |n| n != old && self.state_index(n).is_some());
        self.states[i].name = name.clone();
        for t in &mut self.transitions {
            for n in [&mut t.from, &mut t.to] {
                if *n == Node::state(old) {
                    *n = Node::state(&name);
                }
            }
        }
        if self.default_state.as_deref() == Some(old) {
            self.default_state = Some(name.clone());
        }
        Some(name)
    }

    /// Remove a state and its transitions; the default moves to the first remaining state.
    pub fn remove_state(&mut self, name: &str) {
        self.states.retain(|s| s.name != name);
        let node = Node::state(name);
        self.transitions.retain(|t| t.from != node && t.to != node);
        if self.default_state.as_deref() == Some(name) {
            self.default_state = self.states.first().map(|s| s.name.clone());
        }
    }

    /// Add a parameter with a unique name ("New Float", "New Bool 1", …).
    pub fn add_parameter(&mut self, kind: ParamKind) -> String {
        let name = Self::unique(&format!("New {}", kind.label()), |n| {
            self.parameter(n).is_some()
        });
        self.parameters.push(Parameter {
            name: name.clone(),
            kind,
            default: 0.0,
        });
        name
    }

    /// Rename a parameter (made unique) and follow it in conditions and blend trees.
    pub fn rename_parameter(&mut self, old: &str, new: &str) -> Option<String> {
        let i = self.parameters.iter().position(|p| p.name == old)?;
        let new = new.trim();
        if new.is_empty() || new == old {
            return Some(old.to_owned());
        }
        let name = Self::unique(new, |n| n != old && self.parameter(n).is_some());
        self.parameters[i].name = name.clone();
        for c in self.transitions.iter_mut().flat_map(|t| &mut t.conditions) {
            if c.parameter == old {
                c.parameter = name.clone();
            }
        }
        for b in self.states.iter_mut().filter_map(|s| s.blend_tree.as_mut()) {
            if b.parameter == old {
                b.parameter = name.clone();
            }
        }
        Some(name)
    }

    /// Remove a parameter and the conditions that test it.
    pub fn remove_parameter(&mut self, name: &str) {
        self.parameters.retain(|p| p.name != name);
        for t in &mut self.transitions {
            t.conditions.retain(|c| c.parameter != name);
        }
    }

    /// Transitions leaving `from`, in evaluation order.
    pub fn transitions_from<'a>(
        &'a self,
        from: &'a Node,
    ) -> impl Iterator<Item = (usize, &'a Transition)> {
        self.transitions
            .iter()
            .enumerate()
            .filter(move |(_, t)| t.from == *from)
    }

    /// Every clip the controller plays (states and blend tree children).
    pub fn motions(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.states {
            let clips = s.motion.iter().chain(
                s.blend_tree
                    .iter()
                    .flat_map(|b| b.children.iter().filter_map(|c| c.motion.as_ref())),
            );
            for c in clips {
                if !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        out
    }
}
