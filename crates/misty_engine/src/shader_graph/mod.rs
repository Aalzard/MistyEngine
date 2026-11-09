//! Shader Graph assets (`.shadergraph`, JSON): nodes, connections, Blackboard properties and
//! keywords, groups, sticky notes and a target (Lit / Unlit / Decal, Opaque / Transparent, alpha
//! clip, two-sided, normal space). [`codegen`] turns a graph into the text of a `.shader` asset,
//! so a graph is used by materials exactly like a hand-written shader:
//! [`crate::shader_asset::load`] generates it when the path ends in `.shadergraph`.
//!
//! **Sub Graphs** (`.shadersubgraph`) are graphs whose Blackboard properties are inputs and whose
//! Output node's slots are outputs; a Sub Graph node inlines one. **Keywords** (Boolean or Enum)
//! pick a Keyword node's input per shader variant: each combination compiles separately (only the
//! nodes of that variant), at most [`MAX_VARIANTS`] per graph.
//!
//! [`eval`] evaluates the same graph on the CPU for the editor's small node previews.

pub mod codegen;
pub mod eval;
pub mod nodes;

#[cfg(test)]
mod feature_tests;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use codegen::{
    FileResolver, GraphError, Resolver, compile, compile_all, compile_variant, generate,
    generate_variant,
};
pub use nodes::{
    Block, CATEGORIES, FunctionSource, MASTER_PORTS, NodeKind, Port, PortDefault, PortType, Slot,
    Space, ValueType, intern, master,
};

use crate::shader_asset::TextureDefault;

/// File extension of Shader Graph assets.
pub const EXTENSION: &str = "shadergraph";
/// File extension of Sub Graph assets.
pub const SUB_EXTENSION: &str = "shadersubgraph";
/// Most shader variants a graph's keywords may make (Unity's default Shader Variant Limit).
pub const MAX_VARIANTS: usize = 128;
/// Id of the master stack node.
pub const MASTER: u32 = 0;
/// Current file version.
pub const VERSION: u32 = 1;

pub fn is_graph_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[EXTENSION])
}

pub fn is_sub_graph_path(path: &str) -> bool {
    crate::assets::has_extension(path, &[SUB_EXTENSION])
}

/// Lit (the engine's lighting), Unlit (Base Color is the final colour) or Decal (a Decal
/// Projector material: what the projector blends onto the surfaces in its box).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetMaterial {
    #[default]
    Lit,
    Unlit,
    Decal,
}

impl TargetMaterial {
    pub const ALL: [Self; 3] = [Self::Lit, Self::Unlit, Self::Decal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lit => "Lit",
            Self::Unlit => "Unlit",
            Self::Decal => "Decal",
        }
    }
}

/// Space of the master stack's fragment Normal (Unity's Fragment Normal Space).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NormalSpace {
    /// A normal map sample's space (+Z out of the surface).
    #[default]
    Tangent,
    Object,
    World,
}

impl NormalSpace {
    pub const ALL: [Self; 3] = [Self::Tangent, Self::Object, Self::World];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tangent => "Tangent",
            Self::Object => "Object",
            Self::World => "World",
        }
    }
}

/// A shader graph or a Sub Graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphKind {
    #[default]
    Shader,
    SubGraph,
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
    #[serde(skip_serializing_if = "is_default")]
    pub normal_space: NormalSpace,
    /// Lit: the Thickness and Subsurface Color blocks (wrap lighting, Standard Lit v2).
    #[serde(skip_serializing_if = "is_default")]
    pub subsurface: bool,
    /// Lit: the Coat Mask and Coat Smoothness blocks (a clear coat layer).
    #[serde(skip_serializing_if = "is_default")]
    pub clear_coat: bool,
    /// Lit: the Anisotropy and Anisotropy Direction blocks (stretched highlights).
    #[serde(skip_serializing_if = "is_default")]
    pub anisotropy: bool,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
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

/// A titled frame around nodes; it follows them and moves them when dragged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub title: String,
    pub nodes: Vec<u32>,
}

/// A Boolean keyword (On / Off) or an Enum keyword (one of its entries).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum KeywordKind {
    Boolean,
    Enum { entries: Vec<String> },
}

/// A Blackboard keyword: each value compiles its own shader variant. Materials store the value
/// under the reference name like a property (Boolean 0 / 1, Enum the entry index).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyword {
    /// Reference name (`_DETAIL_ON`), also the Keyword node's title fallback.
    pub name: String,
    /// Inspector label.
    pub display: String,
    #[serde(flatten)]
    pub kind: KeywordKind,
    /// Value new materials (and the previews) use.
    #[serde(default)]
    pub default: u32,
}

impl Keyword {
    /// The values' names: "Off", "On" for a Boolean, else the entries.
    pub fn options(&self) -> Vec<String> {
        match &self.kind {
            KeywordKind::Boolean => vec!["Off".into(), "On".into()],
            KeywordKind::Enum { entries } => entries.clone(),
        }
    }

    pub fn count(&self) -> usize {
        self.options().len().max(1)
    }

    /// The Keyword node's inputs: On / Off for a Boolean (Unity's order), else the entries.
    pub fn ports(&self) -> Vec<Port> {
        let names: Vec<String> = match &self.kind {
            KeywordKind::Boolean => vec!["On".into(), "Off".into()],
            KeywordKind::Enum { entries } => entries.clone(),
        };
        names
            .iter()
            .map(|n| Port {
                name: intern(n),
                ty: PortType::Dynamic,
                default: PortDefault::Value([0.0; 4]),
            })
            .collect()
    }

    /// The input a value selects.
    pub fn port_for(&self, value: u32) -> Option<&'static str> {
        match &self.kind {
            KeywordKind::Boolean => Some(if value > 0 { "On" } else { "Off" }),
            KeywordKind::Enum { entries } => entries
                .get(value as usize)
                .or(entries.first())
                .map(|e| intern(e)),
        }
    }
}

/// Keyword values of one variant (reference name → value); missing keywords use their default.
pub type Variant = BTreeMap<String, u32>;

/// `"_A=1,_B=2"`: the values that differ from the defaults ("" = the default variant).
pub fn variant_key(keywords: &[Keyword], variant: &Variant) -> String {
    keywords
        .iter()
        .filter_map(|k| {
            let v = variant.get(&k.name).copied().unwrap_or(k.default);
            (v != k.default).then(|| format!("{}={v}", k.name))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The variant a key names.
pub fn parse_variant_key(key: &str) -> Variant {
    key.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().parse().ok()?))
        })
        .collect()
}

/// Nodes (with their connections among themselves) and what they read, copied out of a graph:
/// pasted into the same or another graph by [`ShaderGraph::paste`]. Text form for the system
/// clipboard: [`Clipboard::to_text`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    pub nodes: Vec<Node>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub properties: Vec<GraphProperty>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub notes: Vec<StickyNote>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

/// Marks Shader Graph clipboard text.
const CLIPBOARD_TAG: &str = "misty-shadergraph-clipboard";

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.notes.is_empty()
    }

    pub fn to_text(&self) -> String {
        serde_json::json!({ CLIPBOARD_TAG: self }).to_string()
    }

    /// Clipboard text written by [`Self::to_text`] (None for anything else).
    pub fn from_text(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
        serde_json::from_value(v.get(CLIPBOARD_TAG)?.clone()).ok()
    }
}

/// A Shader Graph (or Sub Graph) asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShaderGraph {
    pub version: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: GraphKind,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub properties: Vec<GraphProperty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<Keyword>,
    /// A Sub Graph's outputs (its Output node's inputs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Slot>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
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
            kind: GraphKind::Shader,
            target: Target {
                material,
                ..Default::default()
            },
            properties: Vec::new(),
            keywords: Vec::new(),
            outputs: Vec::new(),
            nodes: vec![Node {
                id: MASTER,
                kind: NodeKind::Master,
                position: [300.0, 0.0],
                values: BTreeMap::new(),
                hide_preview: false,
            }],
            connections: Vec::new(),
            groups: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// An empty Sub Graph: an Output node with one Vector 4 output.
    pub fn new_sub_graph() -> Self {
        Self {
            kind: GraphKind::SubGraph,
            outputs: vec![Slot::new("Out", ValueType::Vec4)],
            ..Self::new(TargetMaterial::Lit)
        }
    }

    pub fn is_sub_graph(&self) -> bool {
        self.kind == GraphKind::SubGraph
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
        for g in &mut self.groups {
            g.nodes.retain(|n| *n != id);
        }
        self.groups.retain(|g| !g.nodes.is_empty());
        self.nodes.len() != before
    }

    pub fn keyword(&self, name: &str) -> Option<&Keyword> {
        self.keywords.iter().find(|k| k.name == name)
    }

    /// Input ports of a node (the master stack's follow the target or the Sub Graph's outputs;
    /// a Keyword node's follow its keyword).
    pub fn inputs(&self, node: &Node) -> Vec<Port> {
        match &node.kind {
            NodeKind::Master if self.is_sub_graph() => {
                self.outputs.iter().map(Slot::input_port).collect()
            }
            // Every port, also those the target hides (a connection there is kept, unused).
            NodeKind::Master => MASTER_PORTS.iter().map(|(_, p)| *p).collect(),
            NodeKind::Keyword { name } => {
                self.keyword(name).map(Keyword::ports).unwrap_or_default()
            }
            k => k.inputs(),
        }
    }

    /// One input port of a node by name.
    pub fn input_port(&self, node: &Node, name: &str) -> Option<Port> {
        self.inputs(node).into_iter().find(|p| p.name == name)
    }

    /// Put nodes in a new group; returns its index.
    pub fn group(&mut self, title: &str, ids: &[u32]) -> Option<usize> {
        let ids: Vec<u32> = ids
            .iter()
            .copied()
            .filter(|&id| id != MASTER && self.node(id).is_some())
            .collect();
        if ids.is_empty() {
            return None;
        }
        for g in &mut self.groups {
            g.nodes.retain(|n| !ids.contains(n));
        }
        self.groups.retain(|g| !g.nodes.is_empty());
        self.groups.push(Group {
            title: title.to_owned(),
            nodes: ids,
        });
        Some(self.groups.len() - 1)
    }

    /// Take nodes out of their groups (empty groups go).
    pub fn ungroup(&mut self, ids: &[u32]) {
        for g in &mut self.groups {
            g.nodes.retain(|n| !ids.contains(n));
        }
        self.groups.retain(|g| !g.nodes.is_empty());
    }

    /// Copy nodes (never the master stack), the connections between them, and the properties
    /// and keywords they read.
    pub fn copy(&self, ids: &[u32]) -> Clipboard {
        let nodes: Vec<Node> = self
            .nodes
            .iter()
            .filter(|n| n.id != MASTER && ids.contains(&n.id))
            .cloned()
            .collect();
        let has = |id: u32| nodes.iter().any(|n| n.id == id);
        let connections = self
            .connections
            .iter()
            .filter(|c| has(c.from) && has(c.to))
            .cloned()
            .collect();
        let mut properties = Vec::new();
        let mut keywords = Vec::new();
        for n in &nodes {
            match &n.kind {
                NodeKind::Property { name } => {
                    if let Some(p) = self.property(name)
                        && !properties.contains(p)
                    {
                        properties.push(p.clone());
                    }
                }
                NodeKind::Keyword { name } => {
                    if let Some(k) = self.keyword(name)
                        && !keywords.contains(k)
                    {
                        keywords.push(k.clone());
                    }
                }
                _ => {}
            }
        }
        let groups = self
            .groups
            .iter()
            .filter_map(|g| {
                let inner: Vec<u32> = g.nodes.iter().copied().filter(|&n| has(n)).collect();
                (!inner.is_empty()).then(|| Group {
                    title: g.title.clone(),
                    nodes: inner,
                })
            })
            .collect();
        Clipboard {
            nodes,
            connections,
            properties,
            keywords,
            notes: Vec::new(),
            groups,
        }
    }

    /// Paste copied nodes, moved by `offset`; returns the new node ids. Properties and keywords
    /// the nodes read are added when the graph has none of that name (a same-named one of
    /// another type is kept and the copy gets a new name), so pasting into another graph works.
    pub fn paste(&mut self, clip: &Clipboard, offset: [f32; 2]) -> Vec<u32> {
        let mut prop_names: HashMap<String, String> = HashMap::new();
        for p in &clip.properties {
            let name = match self.property(&p.name) {
                Some(q) if q.ty.value_type() == p.ty.value_type() => p.name.clone(),
                Some(_) => {
                    let name = self.unique_property_name(&p.name);
                    self.properties.push(GraphProperty {
                        name: name.clone(),
                        ..p.clone()
                    });
                    name
                }
                None if self.keyword(&p.name).is_some() => {
                    let name = self.unique_property_name(&p.name);
                    self.properties.push(GraphProperty {
                        name: name.clone(),
                        ..p.clone()
                    });
                    name
                }
                None => {
                    self.properties.push(p.clone());
                    p.name.clone()
                }
            };
            prop_names.insert(p.name.clone(), name);
        }
        let mut kw_names: HashMap<String, String> = HashMap::new();
        for k in &clip.keywords {
            let name = match self.keyword(&k.name) {
                Some(q) if q.kind == k.kind => k.name.clone(),
                Some(_) => {
                    let name = self.unique_property_name(&k.name);
                    self.keywords.push(Keyword {
                        name: name.clone(),
                        ..k.clone()
                    });
                    name
                }
                None if self.property(&k.name).is_some() => {
                    let name = self.unique_property_name(&k.name);
                    self.keywords.push(Keyword {
                        name: name.clone(),
                        ..k.clone()
                    });
                    name
                }
                None => {
                    self.keywords.push(k.clone());
                    k.name.clone()
                }
            };
            kw_names.insert(k.name.clone(), name);
        }
        let mut ids: HashMap<u32, u32> = HashMap::new();
        let mut out = Vec::new();
        for n in &clip.nodes {
            if n.kind == NodeKind::Master {
                continue;
            }
            let mut kind = n.kind.clone();
            match &mut kind {
                NodeKind::Property { name } => {
                    if let Some(new) = prop_names.get(name) {
                        *name = new.clone();
                    }
                }
                NodeKind::Keyword { name } => {
                    if let Some(new) = kw_names.get(name) {
                        *name = new.clone();
                    }
                }
                _ => {}
            }
            let id = self.add_node(kind, [n.position[0] + offset[0], n.position[1] + offset[1]]);
            if let Some(m) = self.node_mut(id) {
                m.values = n.values.clone();
                m.hide_preview = n.hide_preview;
            }
            ids.insert(n.id, id);
            out.push(id);
        }
        for c in &clip.connections {
            if let (Some(&from), Some(&to)) = (ids.get(&c.from), ids.get(&c.to)) {
                let _ = self.connect(from, &c.output, to, &c.input);
            }
        }
        for g in &clip.groups {
            let members: Vec<u32> = g.nodes.iter().filter_map(|n| ids.get(n).copied()).collect();
            self.group(&g.title, &members);
        }
        for n in &clip.notes {
            self.notes.push(StickyNote {
                position: [n.position[0] + offset[0], n.position[1] + offset[1]],
                ..n.clone()
            });
        }
        out
    }

    /// Add a Blackboard keyword; returns its reference name.
    pub fn add_keyword(&mut self, label: &str, kind: KeywordKind) -> String {
        let base = label.to_ascii_uppercase().replace(' ', "_");
        let mut name = self.unique_property_name(&base);
        if !name.ends_with("_ON") && kind == KeywordKind::Boolean {
            name = self.unique_property_name(&format!("{name}_ON"));
        }
        let mut display = label.to_owned();
        let mut i = 1;
        while self.keywords.iter().any(|k| k.display == display) {
            display = format!("{label} ({i})");
            i += 1;
        }
        self.keywords.push(Keyword {
            name: name.clone(),
            display,
            kind,
            default: 0,
        });
        name
    }

    /// Rename a keyword's reference; its Keyword nodes follow.
    pub fn rename_keyword(&mut self, old: &str, new: &str) -> Result<(), String> {
        if old == new {
            return Ok(());
        }
        if !codegen::valid_ident(new) {
            return Err(format!("'{new}' is not a valid shader name"));
        }
        if self.property(new).is_some() || self.keyword(new).is_some() {
            return Err(format!("{new} already exists"));
        }
        let Some(k) = self.keywords.iter_mut().find(|k| k.name == old) else {
            return Err(format!("no keyword {old}"));
        };
        k.name = new.to_owned();
        for n in &mut self.nodes {
            if let NodeKind::Keyword { name } = &mut n.kind
                && name == old
            {
                *name = new.to_owned();
            }
        }
        Ok(())
    }

    /// Remove a keyword and its nodes.
    pub fn remove_keyword(&mut self, name: &str) {
        self.keywords.retain(|k| k.name != name);
        let ids: Vec<u32> = self
            .nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Keyword { name: k } if k == name))
            .map(|n| n.id)
            .collect();
        for id in ids {
            self.remove_node(id);
        }
    }

    /// Variants the keywords make (saturating).
    pub fn variant_count(&self) -> usize {
        variant_count(&self.keywords)
    }

    /// Every variant (in order; empty when there are more than [`MAX_VARIANTS`]).
    pub fn variants(&self) -> Vec<Variant> {
        all_variants(&self.keywords)
    }

    /// The keyword defaults.
    pub fn default_variant(&self) -> Variant {
        self.keywords
            .iter()
            .map(|k| (k.name.clone(), k.default))
            .collect()
    }

    /// A Sub Graph's interface: (inputs from its properties, outputs).
    pub fn sub_graph_interface(&self) -> (Vec<Slot>, Vec<Slot>) {
        let inputs = self
            .properties
            .iter()
            .map(|p| {
                let value = match p.ty {
                    PropertyType::Color => codegen::linear_color(p.value),
                    PropertyType::Float | PropertyType::Range { .. } | PropertyType::Boolean => {
                        [p.value[0]; 4]
                    }
                    _ => p.value,
                };
                Slot::new(&p.display, p.ty.value_type()).with_value(value)
            })
            .collect();
        (inputs, self.outputs.clone())
    }

    /// Re-read every Sub Graph node's interface from its asset; connections to ports that went
    /// away are dropped. Returns whether anything changed.
    pub fn refresh_sub_graphs(&mut self, resolver: &dyn Resolver) -> bool {
        let mut changed = false;
        for i in 0..self.nodes.len() {
            let NodeKind::SubGraph { path, .. } = &self.nodes[i].kind else {
                continue;
            };
            let Ok(sub) = resolver.graph(path) else {
                continue;
            };
            let (ins, outs) = sub.sub_graph_interface();
            if let NodeKind::SubGraph {
                inputs, outputs, ..
            } = &mut self.nodes[i].kind
                && (*inputs != ins || *outputs != outs)
            {
                *inputs = ins;
                *outputs = outs;
                changed = true;
            }
        }
        if changed {
            let ports: Vec<(u32, Vec<String>, Vec<String>)> = self
                .nodes
                .iter()
                .map(|n| {
                    (
                        n.id,
                        self.inputs(n).iter().map(|p| p.name.to_owned()).collect(),
                        self.outputs(n).iter().map(|p| p.name.to_owned()).collect(),
                    )
                })
                .collect();
            self.connections.retain(|c| {
                let ok_in = ports
                    .iter()
                    .any(|(id, ins, _)| *id == c.to && ins.contains(&c.input));
                let ok_out = ports
                    .iter()
                    .any(|(id, _, outs)| *id == c.from && outs.contains(&c.output));
                ok_in && ok_out
            });
        }
        changed
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
        let Some(inp) = self.input_port(b, input) else {
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
        while self.property(&name).is_some() || self.keyword(&name).is_some() {
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
        if self.property(new).is_some() || self.keyword(new).is_some() {
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

    /// Master-stack ports that apply to the target, with their block (a Sub Graph's Output
    /// node: its outputs).
    pub fn master_ports(&self) -> Vec<(Block, Port)> {
        if self.is_sub_graph() {
            return self
                .outputs
                .iter()
                .map(|s| (Block::Output, s.input_port()))
                .collect();
        }
        let m = self.target.material;
        let lit = m == TargetMaterial::Lit;
        let decal = m == TargetMaterial::Decal;
        MASTER_PORTS
            .iter()
            .copied()
            .filter(|(_, p)| match p.name {
                master::NORMAL | master::METALLIC | master::SMOOTHNESS | master::OCCLUSION => {
                    lit || decal
                }
                master::TRANSMISSION => lit,
                master::THICKNESS | master::SUBSURFACE => lit && self.target.subsurface,
                master::COAT_MASK | master::COAT_SMOOTHNESS => lit && self.target.clear_coat,
                master::ANISOTROPY | master::ANISOTROPY_DIRECTION => lit && self.target.anisotropy,
                master::POSITION => !decal,
                master::ALPHA_CLIP => self.target.alpha_clip && !decal,
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
            for p in self.inputs(node) {
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

    /// Material values for the graph's properties' defaults (new materials, the main preview);
    /// keywords at their defaults too.
    pub fn default_values(&self) -> BTreeMap<String, crate::shader_asset::PropertyValue> {
        let mut out: BTreeMap<String, crate::shader_asset::PropertyValue> = self
            .properties
            .iter()
            .map(|p| (p.name.clone(), p.default_value()))
            .collect();
        for k in &self.keywords {
            out.insert(
                k.name.clone(),
                crate::shader_asset::PropertyValue::Float(k.default as f32),
            );
        }
        out
    }
}

/// Variants a keyword list makes (saturating).
pub fn variant_count(keywords: &[Keyword]) -> usize {
    keywords
        .iter()
        .fold(1usize, |n, k| n.saturating_mul(k.count()))
}

/// Every variant of a keyword list (empty when there are more than [`MAX_VARIANTS`]).
pub fn all_variants(keywords: &[Keyword]) -> Vec<Variant> {
    if variant_count(keywords) > MAX_VARIANTS {
        return Vec::new();
    }
    let mut out = vec![Variant::new()];
    for k in keywords {
        let mut next = Vec::with_capacity(out.len() * k.count());
        for v in &out {
            for i in 0..k.count() as u32 {
                let mut w = v.clone();
                w.insert(k.name.clone(), i);
                next.push(w);
            }
        }
        out = next;
    }
    out
}
