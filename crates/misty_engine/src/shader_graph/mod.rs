//! Shader Graph assets (`.shadergraph`, JSON): nodes, connections, Blackboard properties and a
//! target (Lit / Unlit, Opaque / Transparent, alpha clip, two-sided). [`codegen`] turns a graph
//! into the text of a `.shader` asset, so a graph is used by materials exactly like a hand-written
//! shader: [`crate::shader_asset::load`] generates it when the path ends in `.shadergraph`.
//!
//! [`eval`] evaluates the same graph on the CPU for the editor's small node previews.

pub mod codegen;
pub mod eval;
pub mod nodes;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use codegen::{GraphError, compile, generate};
pub use nodes::{
    Block, CATEGORIES, MASTER_PORTS, NodeKind, Port, PortDefault, PortType, Space, ValueType,
    master,
};

use crate::shader_asset::TextureDefault;

/// File extension of Shader Graph assets.
pub const EXTENSION: &str = "shadergraph";
/// Id of the master stack node.
pub const MASTER: u32 = 0;
/// Current file version.
pub const VERSION: u32 = 1;

pub fn is_graph_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

/// Lit (the engine's lighting) or Unlit (Base Color is the final colour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetMaterial {
    #[default]
    Lit,
    Unlit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    #[default]
    Opaque,
    Transparent,
}

/// Graph Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    pub material: TargetMaterial,
    pub surface: Surface,
    /// Discard pixels whose Alpha is below the Alpha Clip Threshold.
    pub alpha_clip: bool,
    /// Draw both faces (back faces get flipped normals).
    pub two_sided: bool,
}

/// Fallback of an unassigned Texture2D property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextureFallback {
    #[default]
    White,
    Black,
    Grey,
    /// Flat normal (read as linear data, fits Sample Texture 2D (Normal)).
    Normal,
}

impl TextureFallback {
    pub const ALL: [Self; 4] = [Self::White, Self::Black, Self::Grey, Self::Normal];

    pub fn texture_default(self) -> TextureDefault {
        match self {
            Self::White => TextureDefault::White,
            Self::Black => TextureDefault::Black,
            Self::Grey => TextureDefault::Grey,
            Self::Normal => TextureDefault::Normal,
        }
    }
}

/// Type of a Blackboard property.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PropertyType {
    Float,
    /// A slider between the bounds.
    Range {
        min: f32,
        max: f32,
    },
    /// sRGB colour + alpha in the Inspector (linear in the shader).
    Color,
    Vector,
    Texture2D {
        #[serde(default)]
        fallback: TextureFallback,
    },
    /// A checkbox; the shader reads 0 or 1.
    Boolean,
}

impl PropertyType {
    /// The Blackboard's "+" menu.
    pub const MENU: [(&'static str, PropertyType); 6] = [
        ("Float", PropertyType::Float),
        ("Range", PropertyType::Range { min: 0.0, max: 1.0 }),
        ("Color", PropertyType::Color),
        ("Vector", PropertyType::Vector),
        (
            "Texture2D",
            PropertyType::Texture2D {
                fallback: TextureFallback::White,
            },
        ),
        ("Boolean", PropertyType::Boolean),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Float => "Float",
            Self::Range { .. } => "Range",
            Self::Color => "Color",
            Self::Vector => "Vector",
            Self::Texture2D { .. } => "Texture2D",
            Self::Boolean => "Boolean",
        }
    }

    /// What a Property node of this type outputs.
    pub fn value_type(self) -> ValueType {
        match self {
            Self::Float | Self::Range { .. } | Self::Boolean => ValueType::Float,
            Self::Color | Self::Vector => ValueType::Vec4,
            Self::Texture2D { .. } => ValueType::Texture,
        }
    }

    fn default_value(self) -> [f32; 4] {
        match self {
            Self::Color => [1.0; 4],
            Self::Range { min, max } => [(min + max) * 0.5; 4],
            _ => [0.0; 4],
        }
    }
}

/// A Blackboard property: exposed to materials using the graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphProperty {
    /// Reference name used in the shader and the `.mat` file (`_Speed`).
    pub name: String,
    /// Inspector label.
    pub display: String,
    #[serde(flatten)]
    pub ty: PropertyType,
    /// Default value (x for floats and booleans; colours are sRGB).
    #[serde(default)]
    pub value: [f32; 4],
    /// Texture used by the previews and copied into new materials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl GraphProperty {
    pub fn new(name: &str, display: &str, ty: PropertyType) -> Self {
        Self {
            name: name.to_owned(),
            display: display.to_owned(),
            ty,
            value: ty.default_value(),
            texture: None,
        }
    }

    pub fn with_value(mut self, value: [f32; 4]) -> Self {
        self.value = value;
        self
    }

    /// The material value of the default.
    pub fn default_value(&self) -> crate::shader_asset::PropertyValue {
        use crate::shader_asset::PropertyValue as V;
        match self.ty {
            PropertyType::Float | PropertyType::Range { .. } | PropertyType::Boolean => {
                V::Float(self.value[0])
            }
            PropertyType::Color | PropertyType::Vector => V::Vector(self.value),
            PropertyType::Texture2D { .. } => V::Texture(self.texture.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    #[serde(flatten)]
    pub kind: NodeKind,
    pub position: [f32; 2],
    /// Values of unconnected inputs that differ from the port defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, [f32; 4]>,
    /// Preview thumbnail folded away.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_preview: bool,
}

impl Node {
    pub fn input(&self, name: &str) -> Option<Port> {
        self.kind.inputs().into_iter().find(|p| p.name == name)
    }

    /// Value of an unconnected constant input.
    pub fn value(&self, port: &Port) -> [f32; 4] {
        match (self.values.get(port.name), port.default) {
            (Some(v), _) => *v,
            (None, PortDefault::Value(v)) => v,
            _ => [0.0; 4],
        }
    }
}

/// An edge from one node's output to another node's input.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Connection {
    pub from: u32,
    pub output: String,
    pub to: u32,
    pub input: String,
}

/// A text note on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StickyNote {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
}

/// A Shader Graph asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShaderGraph {
    pub version: u32,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub properties: Vec<GraphProperty>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<StickyNote>,
}

impl Default for ShaderGraph {
    fn default() -> Self {
        Self::new(TargetMaterial::Lit)
    }
}

/// Types of every output and each node's resolved dynamic type.
#[derive(Clone, Debug, Default)]
pub struct Types {
    pub dynamic: HashMap<u32, ValueType>,
    pub outputs: HashMap<(u32, String), ValueType>,
}

impl Types {
    pub fn output(&self, node: u32, port: &str) -> Option<ValueType> {
        self.outputs.get(&(node, port.to_owned())).copied()
    }

    pub fn dynamic(&self, node: u32) -> ValueType {
        self.dynamic.get(&node).copied().unwrap_or(ValueType::Float)
    }
}

impl ShaderGraph {
    /// An empty graph: just the master stack.
    pub fn new(material: TargetMaterial) -> Self {
        Self {
            version: VERSION,
            target: Target {
                material,
                ..Default::default()
            },
            properties: Vec::new(),
            nodes: vec![Node {
                id: MASTER,
                kind: NodeKind::Master,
                position: [300.0, 0.0],
                values: BTreeMap::new(),
                hide_preview: false,
            }],
            connections: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("graphs serialize")
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut g: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| format!("not a valid Shader Graph: {e}"))?;
        if !g.nodes.iter().any(|n| n.id == MASTER) {
            g.nodes
                .insert(0, Self::new(g.target.material).nodes.remove(0));
        }
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

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn property(&self, name: &str) -> Option<&GraphProperty> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// Add a node; returns its id.
    pub fn add_node(&mut self, kind: NodeKind, position: [f32; 2]) -> u32 {
        let id = self.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        self.nodes.push(Node {
            id,
            kind,
            position,
            values: BTreeMap::new(),
            hide_preview: false,
        });
        id
    }

    /// Remove a node and its connections (the master stack stays).
    pub fn remove_node(&mut self, id: u32) -> bool {
        if id == MASTER {
            return false;
        }
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        self.connections.retain(|c| c.from != id && c.to != id);
        self.nodes.len() != before
    }

    /// The connection feeding an input.
    pub fn input_connection(&self, node: u32, input: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.to == node && c.input == input)
    }

    /// Output ports of a node (Property nodes take their property's type).
    pub fn outputs(&self, node: &Node) -> Vec<Port> {
        let prop = match &node.kind {
            NodeKind::Property { name } => self.property(name).map(|p| p.ty.value_type()),
            _ => None,
        };
        node.kind.outputs(prop)
    }

    /// Does `from` (transitively) feed `to`?
    pub fn feeds(&self, from: u32, to: u32) -> bool {
        let mut stack = vec![from];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if seen.insert(n) {
                stack.extend(
                    self.connections
                        .iter()
                        .filter(|c| c.from == n)
                        .map(|c| c.to),
                );
            }
        }
        false
    }

    /// Why a connection is not allowed (None = it is).
    pub fn check_connection(
        &self,
        from: u32,
        output: &str,
        to: u32,
        input: &str,
    ) -> Option<String> {
        let (Some(a), Some(b)) = (self.node(from), self.node(to)) else {
            return Some("no such node".into());
        };
        if from == to {
            return Some("a node cannot connect to itself".into());
        }
        let Some(out) = self.outputs(a).into_iter().find(|p| p.name == output) else {
            return Some(format!("{} has no output {output}", a.kind.title()));
        };
        let Some(inp) = b.input(input) else {
            return Some(format!("{} has no input {input}", b.kind.title()));
        };
        let types = self.types();
        let out_ty = types
            .output(from, output)
            .unwrap_or(out.ty.resolve(ValueType::Float));
        let in_ty = inp.ty.resolve(ValueType::Float);
        if !out_ty.converts_to(in_ty) {
            return Some(format!(
                "{} does not connect to {}",
                out_ty.label(),
                in_ty.label()
            ));
        }
        if self.feeds(to, from) {
            return Some("that connection would make a loop".into());
        }
        None
    }

    /// Connect an output to an input (replacing what fed that input).
    pub fn connect(&mut self, from: u32, output: &str, to: u32, input: &str) -> Result<(), String> {
        if let Some(e) = self.check_connection(from, output, to, input) {
            return Err(e);
        }
        self.connections
            .retain(|c| !(c.to == to && c.input == input));
        self.connections.push(Connection {
            from,
            output: output.to_owned(),
            to,
            input: input.to_owned(),
        });
        Ok(())
    }

    /// Remove whatever feeds an input.
    pub fn disconnect(&mut self, to: u32, input: &str) -> bool {
        let before = self.connections.len();
        self.connections
            .retain(|c| !(c.to == to && c.input == input));
        self.connections.len() != before
    }

    /// A reference name not used yet (`_Float`, `_Float_1`, …).
    pub fn unique_property_name(&self, display: &str) -> String {
        let base: String = display
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let base = if base.is_empty() {
            "_Property".to_owned()
        } else if base.starts_with('_') {
            base
        } else {
            format!("_{base}")
        };
        let mut name = base.clone();
        let mut i = 1;
        while self.property(&name).is_some() {
            name = format!("{base}_{i}");
            i += 1;
        }
        name
    }

    /// Add a Blackboard property; returns its reference name.
    pub fn add_property(&mut self, label: &str, ty: PropertyType) -> String {
        let name = self.unique_property_name(label);
        let mut display = label.to_owned();
        let mut i = 1;
        while self.properties.iter().any(|p| p.display == display) {
            display = format!("{label} ({i})");
            i += 1;
        }
        self.properties
            .push(GraphProperty::new(&name, &display, ty));
        name
    }

    /// Rename a property's reference; its Property nodes follow.
    pub fn rename_property(&mut self, old: &str, new: &str) -> Result<(), String> {
        if old == new {
            return Ok(());
        }
        if !codegen::valid_ident(new) {
            return Err(format!("'{new}' is not a valid shader name"));
        }
        if self.property(new).is_some() {
            return Err(format!("{new} already exists"));
        }
        let Some(p) = self.properties.iter_mut().find(|p| p.name == old) else {
            return Err(format!("no property {old}"));
        };
        p.name = new.to_owned();
        for n in &mut self.nodes {
            if let NodeKind::Property { name } = &mut n.kind
                && name == old
            {
                *name = new.to_owned();
            }
        }
        Ok(())
    }

    /// Remove a property and the nodes reading it.
    pub fn remove_property(&mut self, name: &str) {
        self.properties.retain(|p| p.name != name);
        let ids: Vec<u32> = self
            .nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Property { name: p } if p == name))
            .map(|n| n.id)
            .collect();
        for id in ids {
            self.remove_node(id);
        }
    }

    /// Master-stack ports that apply to the target, with their block.
    pub fn master_ports(&self) -> Vec<(Block, Port)> {
        let lit = self.target.material == TargetMaterial::Lit;
        MASTER_PORTS
            .iter()
            .copied()
            .filter(|(_, p)| match p.name {
                master::NORMAL | master::METALLIC | master::SMOOTHNESS => lit,
                master::ALPHA_CLIP => self.target.alpha_clip,
                _ => true,
            })
            .collect()
    }

    /// Nodes in dependency order (inputs first). Err holds a node on a loop.
    pub fn topo_order(&self) -> Result<Vec<u32>, u32> {
        let mut state: HashMap<u32, u8> = HashMap::new(); // 1 = visiting, 2 = done
        let mut order = Vec::with_capacity(self.nodes.len());
        fn visit(
            g: &ShaderGraph,
            id: u32,
            state: &mut HashMap<u32, u8>,
            order: &mut Vec<u32>,
        ) -> Result<(), u32> {
            match state.get(&id) {
                Some(2) => return Ok(()),
                Some(_) => return Err(id),
                None => {}
            }
            state.insert(id, 1);
            for c in g.connections.iter().filter(|c| c.to == id) {
                if g.node(c.from).is_some() {
                    visit(g, c.from, state, order)?;
                }
            }
            state.insert(id, 2);
            order.push(id);
            Ok(())
        }
        for n in &self.nodes {
            visit(self, n.id, &mut state, &mut order)?;
        }
        Ok(order)
    }

    /// Resolve every output's type (dynamic ports take the widest connected dynamic input).
    /// Nodes on a loop keep float types.
    pub fn types(&self) -> Types {
        let order = self
            .topo_order()
            .unwrap_or_else(|_| self.nodes.iter().map(|n| n.id).collect());
        let mut t = Types::default();
        for id in order {
            let Some(node) = self.node(id) else { continue };
            let mut dims = 1;
            for p in node.kind.inputs() {
                if p.ty != PortType::Dynamic {
                    continue;
                }
                if let Some(c) = self.input_connection(id, p.name)
                    && let Some(ty) = t.output(c.from, &c.output)
                {
                    dims = dims.max(ty.dims());
                }
            }
            let dynamic = ValueType::with_dims(dims);
            t.dynamic.insert(id, dynamic);
            for p in self.outputs(node) {
                t.outputs
                    .insert((id, p.name.to_owned()), p.ty.resolve(dynamic));
            }
        }
        t
    }

    /// Material values for the graph's properties' defaults (new materials, the main preview).
    pub fn default_values(&self) -> BTreeMap<String, crate::shader_asset::PropertyValue> {
        self.properties
            .iter()
            .map(|p| (p.name.clone(), p.default_value()))
            .collect()
    }
}
