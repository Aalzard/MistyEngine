//! The Effect Graph document: contexts with their blocks, operator nodes, links from operator
//! outputs into block or operator inputs, and Blackboard properties. Saved as JSON (`.effect`)
//! with the asset GUID table of the textures it references.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::blocks::{BlockKind, ContextType};
use super::operators::OperatorKind;
use super::types::{Gradient, Port, PortType, Value, ValueType};

/// Current file version.
pub const VERSION: u32 = 1;
/// Most particles one system may hold (2M × 64 bytes = the default 128 MiB storage binding).
pub const MAX_CAPACITY: u32 = 2 * 1024 * 1024;

/// Where particles are simulated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SimulationSpace {
    /// Particles move with the object.
    #[default]
    Local,
    /// Particles stay where they were born.
    World,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InitSettings {
    /// Particles alive at most (GPU buffers are sized for it).
    pub capacity: u32,
    /// Culling bounds in the object's space.
    pub bounds_center: [f32; 3],
    pub bounds_size: [f32; 3],
    pub space: SimulationSpace,
}

impl Default for InitSettings {
    fn default() -> Self {
        Self {
            capacity: 4096,
            bounds_center: [0.0, 1.0, 0.0],
            bounds_size: [4.0, 4.0, 4.0],
            space: SimulationSpace::Local,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Move by velocity every step.
    pub integrate: bool,
    /// Add the step's time to the age.
    pub age: bool,
    /// Kill particles older than their lifetime.
    pub reap: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            integrate: true,
            age: true,
            reap: true,
        }
    }
}

/// What an Output context draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutputKind {
    #[default]
    Quad,
    /// One pixel per particle.
    Point,
    /// A segment along the velocity.
    Line,
}

impl OutputKind {
    pub const ALL: [Self; 3] = [Self::Quad, Self::Point, Self::Line];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quad => "Quad",
            Self::Point => "Point",
            Self::Line => "Line",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Additive,
    Alpha,
    /// Unlit opaque (alpha-clipped at 0.5, writes depth).
    Opaque,
}

impl BlendMode {
    pub const ALL: [Self; 3] = [Self::Additive, Self::Alpha, Self::Opaque];

    pub fn label(self) -> &'static str {
        match self {
            Self::Additive => "Additive",
            Self::Alpha => "Alpha Blend",
            Self::Opaque => "Opaque",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlipbookMode {
    /// The sheet plays once over each particle's life.
    #[default]
    OverLife,
    /// Frames per second, looping.
    Fps,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub kind: OutputKind,
    pub blend: BlendMode,
    /// Main texture (project path); None = a soft round dot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// A Texture property overriding the texture per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_property: Option<String>,
    /// Flipbook columns × rows (1×1 = a plain texture).
    pub flipbook: [u32; 2],
    pub flipbook_mode: FlipbookMode,
    pub flipbook_fps: f32,
    /// Fade where particles meet opaque geometry.
    pub soft_particles: bool,
    pub soft_distance: f32,
    /// Line length per unit of speed (Line output).
    pub line_length: f32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            kind: OutputKind::Quad,
            blend: BlendMode::Additive,
            texture: None,
            texture_property: None,
            flipbook: [1, 1],
            flipbook_mode: FlipbookMode::OverLife,
            flipbook_fps: 12.0,
            soft_particles: false,
            soft_distance: 0.5,
            line_length: 0.05,
        }
    }
}

/// A context and its settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContextKind {
    Spawn,
    Initialize(InitSettings),
    Update(UpdateSettings),
    Output(OutputSettings),
}

impl ContextKind {
    pub fn ty(&self) -> ContextType {
        match self {
            Self::Spawn => ContextType::Spawn,
            Self::Initialize(_) => ContextType::Initialize,
            Self::Update(_) => ContextType::Update,
            Self::Output(_) => ContextType::Output,
        }
    }

    pub fn new(ty: ContextType) -> Self {
        match ty {
            ContextType::Spawn => Self::Spawn,
            ContextType::Initialize => Self::Initialize(Default::default()),
            ContextType::Update => Self::Update(Default::default()),
            ContextType::Output => Self::Output(Default::default()),
        }
    }
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// One block of a context.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: u32,
    #[serde(flatten)]
    pub kind: BlockKind,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Inline values of unconnected inputs that differ from the defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Value>,
}

impl Block {
    /// The inline value of `port` (stored, else the default).
    pub fn value(&self, port: &Port) -> Value {
        self.values
            .get(port.name)
            .map(|v| v.converted(port.default.ty()))
            .unwrap_or_else(|| port.default.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub id: u32,
    #[serde(flatten)]
    pub kind: ContextKind,
    pub position: [f32; 2],
    #[serde(default)]
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operator {
    pub id: u32,
    #[serde(flatten)]
    pub kind: OperatorKind,
    pub position: [f32; 2],
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Value>,
}

impl Operator {
    pub fn value(&self, port: &Port) -> Value {
        self.values
            .get(port.name)
            .map(|v| v.converted(port.default.ty()))
            .unwrap_or_else(|| port.default.clone())
    }
}

/// An operator output wired into a block or operator input.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    pub from: u32,
    pub output: String,
    pub to: u32,
    pub input: String,
}

/// Makes a property's default value (the Blackboard's "+" menu).
pub type MakeProperty = fn() -> PropertyValue;

/// The value of an exposed property (its type follows the value).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    Float(f32),
    Vector3([f32; 3]),
    /// sRGB colour + alpha (HDR values above 1 allowed).
    Color([f32; 4]),
    Texture(Option<String>),
    Gradient(Gradient),
}

impl PropertyValue {
    pub const MENU: [(&'static str, MakeProperty); 5] = [
        ("Float", || PropertyValue::Float(0.0)),
        ("Vector3", || PropertyValue::Vector3([0.0; 3])),
        ("Color", || PropertyValue::Color([1.0; 4])),
        ("Texture", || PropertyValue::Texture(None)),
        ("Gradient", || PropertyValue::Gradient(Gradient::default())),
    ];

    pub fn type_label(&self) -> &'static str {
        match self {
            Self::Float(_) => "Float",
            Self::Vector3(_) => "Vector3",
            Self::Color(_) => "Color",
            Self::Texture(_) => "Texture",
            Self::Gradient(_) => "Gradient",
        }
    }

    /// What a Property node of this value outputs (None: textures don't wire into blocks).
    pub fn value_type(&self) -> Option<ValueType> {
        match self {
            Self::Float(_) => Some(ValueType::Float),
            Self::Vector3(_) => Some(ValueType::Vec3),
            Self::Color(_) => Some(ValueType::Color),
            Self::Texture(_) => None,
            Self::Gradient(_) => Some(ValueType::Gradient),
        }
    }

    pub fn same_type(&self, other: &PropertyValue) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// A Blackboard property: exposed on every Visual Effect using the graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProperty {
    pub name: String,
    pub value: PropertyValue,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tooltip: String,
}

/// A sticky note on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StickyNote {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectGraph {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub properties: Vec<EffectProperty>,
    #[serde(default)]
    pub contexts: Vec<Context>,
    #[serde(default)]
    pub operators: Vec<Operator>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<StickyNote>,
    /// Next free id (contexts, blocks and operators share one id space).
    #[serde(default)]
    pub next_id: u32,
}

impl Default for EffectGraph {
    fn default() -> Self {
        Self {
            version: VERSION,
            properties: Vec::new(),
            contexts: Vec::new(),
            operators: Vec::new(),
            links: Vec::new(),
            notes: Vec::new(),
            next_id: 1,
        }
    }
}

/// What an id names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Context(usize),
    Block(usize, usize),
    Operator(usize),
}

/// Resolved port types of every operator (dynamic ports follow their widest dynamic input).
#[derive(Default)]
pub struct Types {
    dynamic: HashMap<u32, ValueType>,
    outputs: HashMap<(u32, String), ValueType>,
}

impl Types {
    pub fn dynamic(&self, op: u32) -> ValueType {
        self.dynamic.get(&op).copied().unwrap_or(ValueType::Float)
    }

    pub fn output(&self, op: u32, port: &str) -> Option<ValueType> {
        self.outputs.get(&(op, port.to_owned())).copied()
    }
}

fn wider(a: ValueType, b: ValueType) -> ValueType {
    if b.dims() > a.dims() { b } else { a }
}

impl EffectGraph {
    /// The default system: Spawn (constant rate) → Initialize → Update → Output (quads).
    pub fn new_default() -> Self {
        use super::blocks::{Attribute, Composition, ShapeMode};
        let mut g = Self::default();
        let spawn = g.add_context(ContextType::Spawn, [0.0, 0.0]);
        g.add_block(spawn, BlockKind::ConstantRate);
        let init = g.add_context(ContextType::Initialize, [0.0, 170.0]);
        g.add_block(
            init,
            BlockKind::PositionSphere {
                mode: ShapeMode::Volume,
            },
        );
        g.add_block(init, BlockKind::VelocityFromDirection);
        g.add_block(
            init,
            BlockKind::SetAttribute {
                attribute: Attribute::Lifetime,
                random: true,
                composition: Composition::Overwrite,
            },
        );
        let update = g.add_context(ContextType::Update, [0.0, 470.0]);
        g.add_block(update, BlockKind::LinearDrag);
        let out = g.add_context(ContextType::Output, [0.0, 640.0]);
        g.add_block(out, BlockKind::ColorOverLife);
        g
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("effect graphs serialize")
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut g: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| format!("not a valid Effect Graph: {e}"))?;
        // Keep the id counter ahead of every id (hand-edited files).
        let max = g.all_ids().max().unwrap_or(0);
        g.next_id = g.next_id.max(max + 1);
        Ok(g)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json(&text)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| e.to_string())
    }

    fn all_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.contexts
            .iter()
            .flat_map(|c| std::iter::once(c.id).chain(c.blocks.iter().map(|b| b.id)))
            .chain(self.operators.iter().map(|o| o.id))
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Where an id lives.
    pub fn find(&self, id: u32) -> Option<Item> {
        for (ci, c) in self.contexts.iter().enumerate() {
            if c.id == id {
                return Some(Item::Context(ci));
            }
            if let Some(bi) = c.blocks.iter().position(|b| b.id == id) {
                return Some(Item::Block(ci, bi));
            }
        }
        self.operators
            .iter()
            .position(|o| o.id == id)
            .map(Item::Operator)
    }

    pub fn context(&self, id: u32) -> Option<&Context> {
        self.contexts.iter().find(|c| c.id == id)
    }

    pub fn context_mut(&mut self, id: u32) -> Option<&mut Context> {
        self.contexts.iter_mut().find(|c| c.id == id)
    }

    /// The first context of a type.
    pub fn context_of(&self, ty: ContextType) -> Option<&Context> {
        self.contexts.iter().find(|c| c.kind.ty() == ty)
    }

    pub fn block(&self, id: u32) -> Option<&Block> {
        self.contexts
            .iter()
            .flat_map(|c| c.blocks.iter())
            .find(|b| b.id == id)
    }

    pub fn block_mut(&mut self, id: u32) -> Option<&mut Block> {
        self.contexts
            .iter_mut()
            .flat_map(|c| c.blocks.iter_mut())
            .find(|b| b.id == id)
    }

    /// The context holding block `id`.
    pub fn block_context(&self, id: u32) -> Option<&Context> {
        self.contexts
            .iter()
            .find(|c| c.blocks.iter().any(|b| b.id == id))
    }

    pub fn operator(&self, id: u32) -> Option<&Operator> {
        self.operators.iter().find(|o| o.id == id)
    }

    pub fn operator_mut(&mut self, id: u32) -> Option<&mut Operator> {
        self.operators.iter_mut().find(|o| o.id == id)
    }

    pub fn property(&self, name: &str) -> Option<&EffectProperty> {
        self.properties.iter().find(|p| p.name == name)
    }

    pub fn add_context(&mut self, ty: ContextType, position: [f32; 2]) -> u32 {
        let id = self.take_id();
        self.contexts.push(Context {
            id,
            kind: ContextKind::new(ty),
            position,
            blocks: Vec::new(),
        });
        id
    }

    /// Append a block to a context (None when the context does not accept it).
    pub fn add_block(&mut self, context: u32, kind: BlockKind) -> Option<u32> {
        self.insert_block(context, usize::MAX, kind)
    }

    /// Insert a block at `index` (clamped) of a context.
    pub fn insert_block(&mut self, context: u32, index: usize, kind: BlockKind) -> Option<u32> {
        let ty = self.context(context)?.kind.ty();
        if !kind.allowed_in(ty) {
            return None;
        }
        let id = self.take_id();
        let c = self.context_mut(context)?;
        let at = index.min(c.blocks.len());
        c.blocks.insert(
            at,
            Block {
                id,
                kind,
                enabled: true,
                values: BTreeMap::new(),
            },
        );
        Some(id)
    }

    pub fn add_operator(&mut self, kind: OperatorKind, position: [f32; 2]) -> u32 {
        let id = self.take_id();
        self.operators.push(Operator {
            id,
            kind,
            position,
            values: BTreeMap::new(),
        });
        id
    }

    /// Remove a context, block or operator (and the links touching it or its blocks).
    pub fn remove(&mut self, id: u32) {
        let mut gone: HashSet<u32> = [id].into();
        match self.find(id) {
            Some(Item::Context(ci)) => {
                let c = self.contexts.remove(ci);
                gone.extend(c.blocks.iter().map(|b| b.id));
            }
            Some(Item::Block(ci, bi)) => {
                self.contexts[ci].blocks.remove(bi);
            }
            Some(Item::Operator(oi)) => {
                self.operators.remove(oi);
            }
            None => return,
        }
        self.links
            .retain(|l| !gone.contains(&l.from) && !gone.contains(&l.to));
    }

    /// Move a block within its context or into another one that accepts it.
    pub fn move_block(&mut self, block: u32, context: u32, index: usize) -> bool {
        let Some(Item::Block(ci, bi)) = self.find(block) else {
            return false;
        };
        let Some(target) = self.contexts.iter().position(|c| c.id == context) else {
            return false;
        };
        if !self.contexts[ci].blocks[bi]
            .kind
            .allowed_in(self.contexts[target].kind.ty())
        {
            return false;
        }
        let b = self.contexts[ci].blocks.remove(bi);
        let mut at = index;
        if ci == target && bi < index {
            at -= 1;
        }
        let list = &mut self.contexts[target].blocks;
        list.insert(at.min(list.len()), b);
        true
    }

    /// Inputs of a block or operator.
    pub fn inputs(&self, id: u32) -> Vec<Port> {
        match self.find(id) {
            Some(Item::Block(ci, bi)) => self.contexts[ci].blocks[bi].kind.inputs(),
            Some(Item::Operator(oi)) => self.operators[oi].kind.inputs(),
            _ => Vec::new(),
        }
    }

    /// Outputs of an operator.
    pub fn outputs(&self, op: &Operator) -> Vec<Port> {
        let prop = match &op.kind {
            OperatorKind::Property { name } => {
                self.property(name).and_then(|p| p.value.value_type())
            }
            _ => None,
        };
        op.kind.outputs(prop)
    }

    /// The link into an input.
    pub fn input_link(&self, to: u32, input: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to == to && l.input == input)
    }

    /// Why `from.output → to.input` cannot be linked (None = it can).
    pub fn check_link(&self, from: u32, output: &str, to: u32, input: &str) -> Option<String> {
        let Some(op) = self.operator(from) else {
            return Some("links start at an operator output".into());
        };
        if !self.outputs(op).iter().any(|p| p.name == output) {
            return Some(format!("{} has no output {output}", op.kind.title()));
        }
        let Some(port) = self.inputs(to).into_iter().find(|p| p.name == input) else {
            return Some(format!("no input {input}"));
        };
        if from == to || self.reaches(to, from) {
            return Some("that would make a loop".into());
        }
        let types = self.types();
        let out_ty = types.output(from, output).unwrap_or(ValueType::Float);
        let in_ty = match port.ty {
            PortType::Fixed(t) => t,
            PortType::Dynamic => {
                if matches!(out_ty, ValueType::Gradient | ValueType::Curve) {
                    return Some(format!("a {} can't feed {input}", out_ty.label()));
                }
                out_ty
            }
        };
        if !out_ty.converts_to(in_ty) {
            return Some(format!(
                "{} doesn't convert to {}",
                out_ty.label(),
                in_ty.label()
            ));
        }
        // Spawn blocks run on the CPU: only properties and constants.
        if let Some(Item::Block(ci, _)) = self.find(to)
            && self.contexts[ci].kind.ty() == ContextType::Spawn
            && !self.cpu_evaluable(from)
        {
            return Some("Spawn blocks only take properties and constants".into());
        }
        None
    }

    /// Can the spawn system evaluate this operator chain on the CPU?
    pub fn cpu_evaluable(&self, op: u32) -> bool {
        let Some(o) = self.operator(op) else {
            return false;
        };
        let ok = matches!(
            o.kind,
            OperatorKind::Property { .. }
                | OperatorKind::Float
                | OperatorKind::Add
                | OperatorKind::Subtract
                | OperatorKind::Multiply
                | OperatorKind::Divide
        );
        ok && self
            .links
            .iter()
            .filter(|l| l.to == op)
            .all(|l| self.cpu_evaluable(l.from))
    }

    /// Does data flow from operator `from` into `to` (through links)?
    fn reaches(&self, from: u32, to: u32) -> bool {
        let mut stack = vec![from];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            stack.extend(self.links.iter().filter(|l| l.from == n).map(|l| l.to));
        }
        false
    }

    /// Link an operator output into an input (replacing what fed it).
    pub fn connect(&mut self, from: u32, output: &str, to: u32, input: &str) -> Result<(), String> {
        if let Some(e) = self.check_link(from, output, to, input) {
            return Err(e);
        }
        self.disconnect(to, input);
        self.links.push(Link {
            from,
            output: output.to_owned(),
            to,
            input: input.to_owned(),
        });
        Ok(())
    }

    pub fn disconnect(&mut self, to: u32, input: &str) {
        self.links.retain(|l| !(l.to == to && l.input == input));
    }

    /// Operators in dependency order (inputs before the nodes reading them).
    pub fn operator_order(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut state: HashMap<u32, u8> = HashMap::new();
        fn visit(g: &EffectGraph, id: u32, state: &mut HashMap<u32, u8>, out: &mut Vec<u32>) {
            match state.get(&id) {
                Some(1) | Some(2) => return,
                _ => {}
            }
            state.insert(id, 1);
            for l in g.links.iter().filter(|l| l.to == id) {
                visit(g, l.from, state, out);
            }
            state.insert(id, 2);
            out.push(id);
        }
        for o in &self.operators {
            visit(self, o.id, &mut state, &mut out);
        }
        out
    }

    /// Resolve every operator's port types.
    pub fn types(&self) -> Types {
        let mut t = Types::default();
        for id in self.operator_order() {
            let Some(op) = self.operator(id) else {
                continue;
            };
            let mut dynamic: Option<ValueType> = None;
            for p in op.kind.inputs() {
                if p.ty != PortType::Dynamic {
                    continue;
                }
                let ty = match self.input_link(id, p.name) {
                    Some(l) => t.output(l.from, &l.output).unwrap_or(ValueType::Float),
                    None => op.value(&p).ty(),
                };
                if ty.dims() > 0 {
                    dynamic = Some(dynamic.map_or(ty, |d| wider(d, ty)));
                }
            }
            let dynamic = dynamic.unwrap_or(ValueType::Float);
            t.dynamic.insert(id, dynamic);
            for p in self.outputs(op) {
                t.outputs
                    .insert((id, p.name.to_owned()), p.ty.resolve(dynamic));
            }
        }
        t
    }

    /// Add a property with a free name based on `base`.
    pub fn add_property(&mut self, base: &str, value: PropertyValue) -> String {
        let mut name = base.to_owned();
        let mut n = 1;
        while self.property(&name).is_some() {
            name = format!("{base} {n}");
            n += 1;
        }
        self.properties.push(EffectProperty {
            name: name.clone(),
            value,
            tooltip: String::new(),
        });
        name
    }

    /// Rename a property and the nodes reading it. False when the name is taken or empty.
    pub fn rename_property(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || (new != old && self.property(new).is_some()) {
            return false;
        }
        for p in &mut self.properties {
            if p.name == old {
                p.name = new.to_owned();
            }
        }
        for o in &mut self.operators {
            if let OperatorKind::Property { name } = &mut o.kind
                && name == old
            {
                *name = new.to_owned();
            }
        }
        for c in &mut self.contexts {
            if let ContextKind::Output(s) = &mut c.kind
                && s.texture_property.as_deref() == Some(old)
            {
                s.texture_property = Some(new.to_owned());
            }
        }
        true
    }

    /// Remove a property and the nodes reading it.
    pub fn remove_property(&mut self, name: &str) {
        self.properties.retain(|p| p.name != name);
        let readers: Vec<u32> = self
            .operators
            .iter()
            .filter(|o| matches!(&o.kind, OperatorKind::Property { name: n } if n == name))
            .map(|o| o.id)
            .collect();
        for id in readers {
            self.remove(id);
        }
        for c in &mut self.contexts {
            if let ContextKind::Output(s) = &mut c.kind
                && s.texture_property.as_deref() == Some(name)
            {
                s.texture_property = None;
            }
        }
    }

    /// Asset paths the graph references (textures).
    pub fn references(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.contexts {
            if let ContextKind::Output(s) = &c.kind
                && let Some(t) = &s.texture
            {
                out.push(t.clone());
            }
        }
        for p in &self.properties {
            if let PropertyValue::Texture(Some(t)) = &p.value {
                out.push(t.clone());
            }
        }
        out
    }
}
