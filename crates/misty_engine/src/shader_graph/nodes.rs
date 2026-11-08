//! The node catalog: every node kind's title, category, input and output ports.
//!
//! Ports are identified by name in the file (`"A"`, `"UV"`, `"Base Color"`), so adding ports
//! later keeps old graphs loading. `Dynamic` ports take the widest vector type connected to any
//! of the node's dynamic inputs (a float otherwise), like the reference editor's dynamic vectors.

use serde::{Deserialize, Serialize};

/// A value flowing along a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ValueType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    /// A `texture_2d<f32>` (only from Texture2D properties, only into texture inputs).
    Texture,
}

impl ValueType {
    /// Number of components (0 for textures).
    pub fn dims(self) -> usize {
        match self {
            Self::Float => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
            Self::Texture => 0,
        }
    }

    /// The vector type with `n` components (1 = float).
    pub fn with_dims(n: usize) -> Self {
        match n {
            0 | 1 => Self::Float,
            2 => Self::Vec2,
            3 => Self::Vec3,
            _ => Self::Vec4,
        }
    }

    pub fn wgsl(self) -> &'static str {
        match self {
            Self::Float => "f32",
            Self::Vec2 => "vec2<f32>",
            Self::Vec3 => "vec3<f32>",
            Self::Vec4 => "vec4<f32>",
            Self::Texture => "texture_2d<f32>",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Float => "Float",
            Self::Vec2 => "Vector 2",
            Self::Vec3 => "Vector 3",
            Self::Vec4 => "Vector 4",
            Self::Texture => "Texture 2D",
        }
    }

    /// Every type a user-defined slot (Custom Function, Sub Graph) can have.
    pub const ALL: [Self; 5] = [
        Self::Float,
        Self::Vec2,
        Self::Vec3,
        Self::Vec4,
        Self::Texture,
    ];

    /// Can a value of this type feed an input of type `to`? Vectors and floats convert into
    /// each other (floats splat, wider vectors truncate, narrower ones pad with zeros); textures
    /// only connect to textures.
    pub fn converts_to(self, to: ValueType) -> bool {
        (self == Self::Texture) == (to == Self::Texture)
    }
}

/// A `'static` copy of a port name. Ports of Custom Function, Sub Graph and Keyword nodes are
/// named by the graph rather than the catalog; each distinct name is stored once for the life of
/// the process (a few bytes per name ever typed), so [`Port`] stays a small `Copy` value.
pub fn intern(name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(n) = names.get(name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// A user-defined port of a Custom Function or Sub Graph node: name, type and the value an
/// unconnected input reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    pub ty: ValueType,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub value: [f32; 4],
}

fn is_zero(v: &[f32; 4]) -> bool {
    *v == [0.0; 4]
}

impl Slot {
    pub fn new(name: &str, ty: ValueType) -> Self {
        Self {
            name: name.to_owned(),
            ty,
            value: [0.0; 4],
        }
    }

    pub fn with_value(mut self, value: [f32; 4]) -> Self {
        self.value = value;
        self
    }

    /// As an input port (textures must be connected).
    pub fn input_port(&self) -> Port {
        let default = if self.ty == ValueType::Texture {
            PortDefault::None
        } else {
            PortDefault::Value(self.value)
        };
        port(intern(&self.name), PortType::Fixed(self.ty), default)
    }

    pub fn output_port(&self) -> Port {
        out(intern(&self.name), PortType::Fixed(self.ty))
    }
}

/// Where a Custom Function's code comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FunctionSource {
    /// The body is written on the node: it reads the inputs and assigns the outputs by name.
    #[default]
    Inline,
    /// A `.wgsl` file in the project defines the function (see `codegen` for the contract).
    File,
}

/// Declared type of a port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Fixed(ValueType),
    /// Resolved per node from what is connected (see the module docs).
    Dynamic,
}

impl PortType {
    pub const FLOAT: Self = Self::Fixed(ValueType::Float);
    pub const VEC2: Self = Self::Fixed(ValueType::Vec2);
    pub const VEC3: Self = Self::Fixed(ValueType::Vec3);
    pub const VEC4: Self = Self::Fixed(ValueType::Vec4);
    pub const TEXTURE: Self = Self::Fixed(ValueType::Texture);

    /// The type with dynamic ports resolved to `dynamic`.
    pub fn resolve(self, dynamic: ValueType) -> ValueType {
        match self {
            Self::Fixed(t) => t,
            Self::Dynamic => dynamic,
        }
    }
}

/// What an unconnected input reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortDefault {
    /// A constant (editable on the node; stored in `Node::values` when changed).
    Value([f32; 4]),
    /// The mesh UV.
    Uv,
    /// World-space normal.
    WorldNormal,
    /// World-space direction to the camera.
    ViewDir,
    /// World-space position.
    WorldPos,
    /// Nothing (texture inputs must be connected).
    None,
}

impl PortDefault {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Uv => Some("UV0"),
            Self::WorldNormal => Some("World Normal"),
            Self::ViewDir => Some("View Dir"),
            Self::WorldPos => Some("World Pos"),
            Self::Value(_) | Self::None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Port {
    pub name: &'static str,
    pub ty: PortType,
    pub default: PortDefault,
}

const fn port(name: &'static str, ty: PortType, default: PortDefault) -> Port {
    Port { name, ty, default }
}

const fn value(name: &'static str, ty: PortType, v: [f32; 4]) -> Port {
    port(name, ty, PortDefault::Value(v))
}

const fn out(name: &'static str, ty: PortType) -> Port {
    port(name, ty, PortDefault::None)
}

const ZERO: [f32; 4] = [0.0; 4];
const ONE: [f32; 4] = [1.0; 4];

/// Object or world space for the geometry inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Space {
    Object,
    #[default]
    World,
}

/// Which master-stack block a port belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Vertex,
    Fragment,
    /// A Sub Graph's Output node.
    Output,
}

/// Names of the master stack's ports.
pub mod master {
    pub const POSITION: &str = "Position Offset";
    pub const BASE_COLOR: &str = "Base Color";
    pub const NORMAL: &str = "Normal";
    pub const METALLIC: &str = "Metallic";
    pub const SMOOTHNESS: &str = "Smoothness";
    pub const EMISSION: &str = "Emission";
    pub const ALPHA: &str = "Alpha";
    pub const ALPHA_CLIP: &str = "Alpha Clip Threshold";
    pub const OCCLUSION: &str = "Ambient Occlusion";
    /// Light coming through thin surfaces from behind, tinted (leaves, paper, cloth).
    pub const TRANSMISSION: &str = "Transmission";
}

/// Every master-stack port with its block (which ones apply depends on the target).
pub const MASTER_PORTS: [(Block, Port); 10] = [
    (Block::Vertex, value(master::POSITION, PortType::VEC3, ZERO)),
    (
        Block::Fragment,
        value(master::BASE_COLOR, PortType::VEC3, [0.5, 0.5, 0.5, 1.0]),
    ),
    (
        Block::Fragment,
        value(master::NORMAL, PortType::VEC3, [0.0, 0.0, 1.0, 0.0]),
    ),
    (
        Block::Fragment,
        value(master::METALLIC, PortType::FLOAT, ZERO),
    ),
    (
        Block::Fragment,
        value(master::SMOOTHNESS, PortType::FLOAT, [0.5; 4]),
    ),
    (
        Block::Fragment,
        value(master::EMISSION, PortType::VEC3, ZERO),
    ),
    (
        Block::Fragment,
        value(master::OCCLUSION, PortType::FLOAT, ONE),
    ),
    (
        Block::Fragment,
        value(master::TRANSMISSION, PortType::VEC3, ZERO),
    ),
    (Block::Fragment, value(master::ALPHA, PortType::FLOAT, ONE)),
    (
        Block::Fragment,
        value(master::ALPHA_CLIP, PortType::FLOAT, [0.5; 4]),
    ),
];

/// A node's kind and its settings. Serialized with a `"type"` tag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    /// The master stack (exactly one per graph, id 0).
    Master,
    // ---- inputs ----
    /// Reads a Blackboard property.
    Property {
        name: String,
    },
    /// A constant colour (sRGB + alpha; the shader gets it in linear space).
    Color {
        value: [f32; 4],
    },
    Float,
    Vector2,
    Vector3,
    Vector4,
    Time,
    #[serde(rename = "UV")]
    Uv,
    Position {
        #[serde(default)]
        space: Space,
    },
    Normal {
        #[serde(default)]
        space: Space,
    },
    ViewDirection {
        #[serde(default)]
        space: Space,
    },
    VertexColor,
    ScreenPosition,
    // ---- math ----
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Minimum,
    Maximum,
    Lerp,
    Clamp,
    Saturate,
    OneMinus,
    Negate,
    Remap,
    Step,
    Smoothstep,
    Sine,
    Cosine,
    Absolute,
    Floor,
    Fraction,
    Dot,
    Cross,
    Normalize,
    Length,
    Split,
    Combine,
    // ---- texture ----
    SampleTexture2D {
        /// Unpack the sample as a tangent-space normal map.
        #[serde(default)]
        normal_map: bool,
    },
    TilingAndOffset,
    /// A tangent-space normal from a height texture's slope.
    NormalFromTexture,
    NormalStrength,
    // ---- procedural ----
    GradientNoise,
    Voronoi,
    Checkerboard,
    Fresnel,
    /// Projects a texture along the world (or object) axes and blends by the normal: no UVs
    /// needed (rocks, cliffs, props without good UVs).
    Triplanar {
        #[serde(default)]
        space: Space,
    },
    // ---- utility ----
    /// Passes its input through, with a large preview.
    Preview,
    /// Picks one input per shader variant (a Blackboard keyword: Boolean or Enum).
    Keyword {
        name: String,
    },
    /// The graph's own WGSL: an inline body or a function in a `.wgsl` file.
    CustomFunction {
        /// Function name (for a file: the function it defines).
        function: String,
        #[serde(default)]
        source: FunctionSource,
        /// Inline code: reads the inputs, assigns the outputs (all by name).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        body: String,
        /// Project path of the `.wgsl` file (File source).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        file: String,
        #[serde(default)]
        inputs: Vec<Slot>,
        #[serde(default)]
        outputs: Vec<Slot>,
    },
    /// A Sub Graph asset (`.shadersubgraph`): its properties are the inputs, its Output node's
    /// slots the outputs. The interface is cached here and refreshed from the asset.
    SubGraph {
        path: String,
        #[serde(default)]
        inputs: Vec<Slot>,
        #[serde(default)]
        outputs: Vec<Slot>,
    },
}

/// Node categories of the Create Node menu.
pub const CATEGORIES: [&str; 6] = [
    "Input",
    "Math",
    "Texture",
    "Procedural",
    "Utility",
    "Sub Graphs",
];

impl NodeKind {
    /// Every node the Create Node menu offers (the Property node comes from the Blackboard).
    pub fn catalog() -> Vec<NodeKind> {
        use NodeKind::*;
        vec![
            Color {
                value: [1.0, 1.0, 1.0, 1.0],
            },
            Float,
            Vector2,
            Vector3,
            Vector4,
            Time,
            Uv,
            Position {
                space: Space::World,
            },
            Position {
                space: Space::Object,
            },
            Normal {
                space: Space::World,
            },
            Normal {
                space: Space::Object,
            },
            ViewDirection {
                space: Space::World,
            },
            VertexColor,
            ScreenPosition,
            Add,
            Subtract,
            Multiply,
            Divide,
            Power,
            Minimum,
            Maximum,
            Lerp,
            Clamp,
            Saturate,
            OneMinus,
            Negate,
            Remap,
            Step,
            Smoothstep,
            Sine,
            Cosine,
            Absolute,
            Floor,
            Fraction,
            Dot,
            Cross,
            Normalize,
            Length,
            Split,
            Combine,
            SampleTexture2D { normal_map: false },
            SampleTexture2D { normal_map: true },
            TilingAndOffset,
            NormalFromTexture,
            NormalStrength,
            GradientNoise,
            Voronoi,
            Checkerboard,
            Fresnel,
            Triplanar {
                space: Space::World,
            },
            Preview,
            CustomFunction {
                function: "MyFunction".into(),
                source: FunctionSource::Inline,
                body: "Out = In;".into(),
                file: String::new(),
                inputs: vec![Slot::new("In", ValueType::Vec3)],
                outputs: vec![Slot::new("Out", ValueType::Vec3)],
            },
        ]
    }

    pub fn title(&self) -> String {
        use NodeKind::*;
        let space = |s: &Space| match s {
            Space::Object => "Object",
            Space::World => "World",
        };
        match self {
            Master => "Master Stack".into(),
            Property { name } => name.clone(),
            Color { .. } => "Color".into(),
            Float => "Float".into(),
            Vector2 => "Vector 2".into(),
            Vector3 => "Vector 3".into(),
            Vector4 => "Vector 4".into(),
            Time => "Time".into(),
            Uv => "UV".into(),
            Position { space: s } => format!("Position ({})", space(s)),
            Normal { space: s } => format!("Normal ({})", space(s)),
            ViewDirection { space: s } => format!("View Direction ({})", space(s)),
            VertexColor => "Vertex Color".into(),
            ScreenPosition => "Screen Position".into(),
            Add => "Add".into(),
            Subtract => "Subtract".into(),
            Multiply => "Multiply".into(),
            Divide => "Divide".into(),
            Power => "Power".into(),
            Minimum => "Minimum".into(),
            Maximum => "Maximum".into(),
            Lerp => "Lerp".into(),
            Clamp => "Clamp".into(),
            Saturate => "Saturate".into(),
            OneMinus => "One Minus".into(),
            Negate => "Negate".into(),
            Remap => "Remap".into(),
            Step => "Step".into(),
            Smoothstep => "Smoothstep".into(),
            Sine => "Sine".into(),
            Cosine => "Cosine".into(),
            Absolute => "Absolute".into(),
            Floor => "Floor".into(),
            Fraction => "Fraction".into(),
            Dot => "Dot Product".into(),
            Cross => "Cross Product".into(),
            Normalize => "Normalize".into(),
            Length => "Length".into(),
            Split => "Split".into(),
            Combine => "Combine".into(),
            SampleTexture2D { normal_map: false } => "Sample Texture 2D".into(),
            SampleTexture2D { normal_map: true } => "Sample Texture 2D (Normal)".into(),
            TilingAndOffset => "Tiling And Offset".into(),
            NormalFromTexture => "Normal From Texture".into(),
            NormalStrength => "Normal Strength".into(),
            GradientNoise => "Gradient Noise".into(),
            Voronoi => "Voronoi".into(),
            Checkerboard => "Checkerboard".into(),
            Fresnel => "Fresnel Effect".into(),
            Triplanar { .. } => "Triplanar".into(),
            Preview => "Preview".into(),
            Keyword { name } => name.clone(),
            CustomFunction { function, .. } => format!("Custom Function ({function})"),
            SubGraph { path, .. } => std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Sub Graph".into()),
        }
    }

    pub fn category(&self) -> &'static str {
        use NodeKind::*;
        match self {
            Master => "Master",
            Property { .. }
            | Color { .. }
            | Float
            | Vector2
            | Vector3
            | Vector4
            | Time
            | Uv
            | Position { .. }
            | Normal { .. }
            | ViewDirection { .. }
            | VertexColor
            | ScreenPosition => "Input",
            Keyword { .. } => "Input",
            SampleTexture2D { .. }
            | TilingAndOffset
            | NormalFromTexture
            | NormalStrength
            | Triplanar { .. } => "Texture",
            GradientNoise | Voronoi | Checkerboard | Fresnel => "Procedural",
            Preview | CustomFunction { .. } => "Utility",
            SubGraph { .. } => "Sub Graphs",
            _ => "Math",
        }
    }

    /// Extra words the Create Node search matches.
    pub fn keywords(&self) -> &'static str {
        use NodeKind::*;
        match self {
            Add => "plus sum +",
            Subtract => "minus -",
            Multiply => "times product *",
            Divide => "/",
            Lerp => "mix blend interpolate",
            Fraction => "fract",
            Absolute => "abs",
            Sine => "sin wave",
            Cosine => "cos wave",
            Saturate => "clamp01",
            Split => "swizzle channels rgba",
            Combine => "append merge rgba",
            Fresnel => "rim",
            GradientNoise => "perlin noise",
            Voronoi => "cells worley noise",
            Time => "seconds animate",
            Uv => "texcoord",
            SampleTexture2D { .. } => "texture tex2d",
            Color { .. } => "colour constant",
            Float => "number constant scalar",
            Triplanar { .. } => "box mapping world projection",
            CustomFunction { .. } => "code wgsl hlsl script",
            VertexColor => "vertex colour paint",
            _ => "",
        }
    }

    /// Input ports (the master stack's come from [`MASTER_PORTS`]; a Keyword node's from its
    /// keyword: use `ShaderGraph::inputs`, which knows both).
    pub fn inputs(&self) -> Vec<Port> {
        use NodeKind::*;
        use PortType as T;
        let d = PortType::Dynamic;
        match self {
            Master => MASTER_PORTS.iter().map(|(_, p)| *p).collect(),
            Property { .. }
            | Color { .. }
            | Time
            | Uv
            | Position { .. }
            | Normal { .. }
            | ViewDirection { .. }
            | VertexColor
            | ScreenPosition
            | Keyword { .. } => vec![],
            CustomFunction { inputs, .. } | SubGraph { inputs, .. } => {
                inputs.iter().map(Slot::input_port).collect()
            }
            Triplanar { .. } => vec![
                port("Texture", T::TEXTURE, PortDefault::None),
                port("Position", T::VEC3, PortDefault::WorldPos),
                port("Normal", T::VEC3, PortDefault::WorldNormal),
                value("Tile", T::FLOAT, ONE),
                value("Blend", T::FLOAT, [4.0; 4]),
            ],
            Float => vec![value("X", T::FLOAT, ZERO)],
            Vector2 => vec![value("X", T::FLOAT, ZERO), value("Y", T::FLOAT, ZERO)],
            Vector3 => vec![
                value("X", T::FLOAT, ZERO),
                value("Y", T::FLOAT, ZERO),
                value("Z", T::FLOAT, ZERO),
            ],
            Vector4 => vec![
                value("X", T::FLOAT, ZERO),
                value("Y", T::FLOAT, ZERO),
                value("Z", T::FLOAT, ZERO),
                value("W", T::FLOAT, ZERO),
            ],
            Add | Subtract | Minimum | Maximum => vec![value("A", d, ZERO), value("B", d, ONE)],
            Multiply | Divide => vec![value("A", d, ZERO), value("B", d, [2.0; 4])],
            Power => vec![value("A", d, ZERO), value("B", d, [2.0; 4])],
            Lerp => vec![
                value("A", d, ZERO),
                value("B", d, ONE),
                value("T", d, [0.5; 4]),
            ],
            Clamp => vec![
                value("In", d, ZERO),
                value("Min", d, ZERO),
                value("Max", d, ONE),
            ],
            Saturate | OneMinus | Negate | Sine | Cosine | Absolute | Floor | Fraction
            | Normalize | Length | Preview => vec![value("In", d, ZERO)],
            Remap => vec![
                value("In", d, ZERO),
                value("In Min Max", T::VEC2, [-1.0, 1.0, 0.0, 0.0]),
                value("Out Min Max", T::VEC2, [0.0, 1.0, 0.0, 0.0]),
            ],
            Step => vec![value("Edge", d, ONE), value("In", d, ZERO)],
            Smoothstep => vec![
                value("Edge1", d, ZERO),
                value("Edge2", d, ONE),
                value("In", d, ZERO),
            ],
            Dot => vec![value("A", d, ZERO), value("B", d, ZERO)],
            Cross => vec![
                value("A", T::VEC3, ZERO),
                value("B", T::VEC3, [0.0, 1.0, 0.0, 0.0]),
            ],
            Split => vec![value("In", d, ZERO)],
            Combine => vec![
                value("R", T::FLOAT, ZERO),
                value("G", T::FLOAT, ZERO),
                value("B", T::FLOAT, ZERO),
                value("A", T::FLOAT, ZERO),
            ],
            SampleTexture2D { .. } => vec![
                port("Texture", T::TEXTURE, PortDefault::None),
                port("UV", T::VEC2, PortDefault::Uv),
            ],
            TilingAndOffset => vec![
                port("UV", T::VEC2, PortDefault::Uv),
                value("Tiling", T::VEC2, ONE),
                value("Offset", T::VEC2, ZERO),
            ],
            NormalFromTexture => vec![
                port("Texture", T::TEXTURE, PortDefault::None),
                port("UV", T::VEC2, PortDefault::Uv),
                value("Offset", T::FLOAT, [0.5; 4]),
                value("Strength", T::FLOAT, [8.0; 4]),
            ],
            NormalStrength => vec![
                value("In", T::VEC3, [0.0, 0.0, 1.0, 0.0]),
                value("Strength", T::FLOAT, ONE),
            ],
            GradientNoise => vec![
                port("UV", T::VEC2, PortDefault::Uv),
                value("Scale", T::FLOAT, [10.0; 4]),
            ],
            Voronoi => vec![
                port("UV", T::VEC2, PortDefault::Uv),
                value("Angle Offset", T::FLOAT, [2.0; 4]),
                value("Cell Density", T::FLOAT, [5.0; 4]),
            ],
            Checkerboard => vec![
                port("UV", T::VEC2, PortDefault::Uv),
                value("Color A", T::VEC3, [0.2, 0.2, 0.2, 0.0]),
                value("Color B", T::VEC3, [0.7, 0.7, 0.7, 0.0]),
                value("Frequency", T::VEC2, [4.0; 4]),
            ],
            Fresnel => vec![
                port("Normal", T::VEC3, PortDefault::WorldNormal),
                port("View Dir", T::VEC3, PortDefault::ViewDir),
                value("Power", T::FLOAT, [2.0; 4]),
            ],
        }
    }

    /// Output ports. A Property node's type comes from its property (`property_type`).
    pub fn outputs(&self, property_type: Option<ValueType>) -> Vec<Port> {
        use NodeKind::*;
        use PortType as T;
        let d = PortType::Dynamic;
        match self {
            Master => vec![],
            Property { .. } => vec![out(
                "Out",
                T::Fixed(property_type.unwrap_or(ValueType::Float)),
            )],
            Color { .. } | VertexColor | ScreenPosition => vec![out("Out", T::VEC4)],
            Float => vec![out("Out", T::FLOAT)],
            Vector2 => vec![out("Out", T::VEC2)],
            Vector3 | Position { .. } | Normal { .. } | ViewDirection { .. } => {
                vec![out("Out", T::VEC3)]
            }
            Vector4 => vec![out("Out", T::VEC4)],
            Time => vec![
                out("Time", T::FLOAT),
                out("Sine Time", T::FLOAT),
                out("Cosine Time", T::FLOAT),
                out("Delta Time", T::FLOAT),
            ],
            Uv => vec![out("Out", T::VEC2)],
            Dot | Length => vec![out("Out", T::FLOAT)],
            Cross => vec![out("Out", T::VEC3)],
            Split => vec![
                out("R", T::FLOAT),
                out("G", T::FLOAT),
                out("B", T::FLOAT),
                out("A", T::FLOAT),
            ],
            Combine => vec![
                out("RGBA", T::VEC4),
                out("RGB", T::VEC3),
                out("RG", T::VEC2),
            ],
            SampleTexture2D { .. } => vec![
                out("RGBA", T::VEC4),
                out("R", T::FLOAT),
                out("G", T::FLOAT),
                out("B", T::FLOAT),
                out("A", T::FLOAT),
            ],
            TilingAndOffset => vec![out("Out", T::VEC2)],
            NormalFromTexture | NormalStrength => vec![out("Out", T::VEC3)],
            GradientNoise | Fresnel => vec![out("Out", T::FLOAT)],
            Voronoi => vec![out("Out", T::FLOAT), out("Cells", T::FLOAT)],
            Checkerboard => vec![out("Out", T::VEC3)],
            Triplanar { .. } => vec![out("Out", T::VEC4)],
            CustomFunction { outputs, .. } | SubGraph { outputs, .. } => {
                outputs.iter().map(Slot::output_port).collect()
            }
            _ => vec![out("Out", d)],
        }
    }

    /// Nodes whose preview reads 3D geometry draw it on a sphere rather than a flat quad.
    pub fn is_3d_input(&self) -> bool {
        matches!(
            self,
            NodeKind::Position { .. }
                | NodeKind::Normal { .. }
                | NodeKind::ViewDirection { .. }
                | NodeKind::Fresnel
                | NodeKind::Triplanar { .. }
        )
    }
}
