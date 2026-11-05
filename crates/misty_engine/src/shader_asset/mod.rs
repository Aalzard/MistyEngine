//! User shader assets (`.shader`, WGSL): a comment header declaring properties and render state,
//! followed by a `surface` (lit) or `fragment` (unlit) function the renderer wraps into its
//! pipeline. See `docs/shaders.md` for the syntax and `render::custom_shader` for the wrapping.
//!
//! Loaded shaders are cached by project path and re-read when their file changes
//! ([`poll_changes`]), which is what makes editing a shader hot-reload.

mod library;
mod parse;
pub mod templates;

pub use library::{generation, load, poll_changes, reload, set_source};
pub use parse::parse;

use serde::{Deserialize, Serialize};

/// File extension of shader assets.
pub const EXTENSION: &str = "shader";

/// Does a material's `shader` field name a shader asset (rather than a built-in shader)?
/// Shader Graphs (`.shadergraph`) count: they load as the shader they generate.
pub fn is_shader_path(shader: &str) -> bool {
    crate::assets::has_extension(shader, &[EXTENSION, crate::shader_graph::EXTENSION])
}

/// Lit surface shaders go through the engine's lighting; unlit ones output the final colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShaderModel {
    /// `fn surface(input: SurfaceInput) -> SurfaceOutput`, lit like the Standard shader.
    #[default]
    Surface,
    /// `fn fragment(input: SurfaceInput) -> vec4<f32>`, the final linear colour and alpha.
    Unlit,
}

/// Blend mode of the colour output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Opaque.
    #[default]
    Off,
    /// Straight alpha (the Standard shader's Fade).
    Alpha,
    /// Premultiplied alpha (Transparent: only the diffuse part fades).
    Premultiply,
    /// Colour × alpha added to what is behind (glows, beams).
    Additive,
}

/// Which faces are culled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cull {
    #[default]
    Back,
    Front,
    /// Both faces drawn (back faces get flipped normals).
    Off,
}

/// Render state from the header (`@blend`, `@cull`, `@zwrite`, `@queue`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShaderSettings {
    pub model: ShaderModel,
    pub blend: Blend,
    pub cull: Cull,
    pub zwrite: bool,
    /// Render queue: below 3000 draws with the opaque geometry, 3000 and up after it, back to
    /// front, where the opaque scene's colour and depth can be sampled.
    pub queue: u32,
}

impl Default for ShaderSettings {
    fn default() -> Self {
        Self {
            model: ShaderModel::Surface,
            blend: Blend::Off,
            cull: Cull::Back,
            zwrite: true,
            queue: 2000,
        }
    }
}

/// Fallback for unassigned texture properties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureDefault {
    #[default]
    White,
    Black,
    Grey,
    /// Flat tangent-space normal (0.5, 0.5, 1).
    Normal,
}

impl TextureDefault {
    pub fn rgba(self) -> [u8; 4] {
        match self {
            Self::White => [255; 4],
            Self::Black => [0, 0, 0, 255],
            Self::Grey => [128, 128, 128, 255],
            Self::Normal => [128, 128, 255, 255],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Black => "black",
            Self::Grey => "grey",
            Self::Normal => "normal",
        }
    }
}

/// Type of a declared property (and its inspector widget).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropertyKind {
    Float,
    /// Slider between the bounds.
    Range(f32, f32),
    /// sRGB colour + alpha in the Inspector; the shader receives it in linear space.
    Color,
    Vector,
    /// `texture_2d<f32>` named after the property; colour textures are sRGB-decoded unless the
    /// default is `normal`.
    Texture(TextureDefault),
    /// A checkbox in the Inspector; the shader reads an `f32` that is 0 or 1.
    Toggle,
}

impl PropertyKind {
    /// WGSL type in the generated `MaterialProperties` struct (`None` = a texture binding).
    pub fn wgsl(self) -> Option<&'static str> {
        match self {
            Self::Float | Self::Range(..) | Self::Toggle => Some("f32"),
            Self::Color | Self::Vector => Some("vec4<f32>"),
            Self::Texture(_) => None,
        }
    }
}

/// A material value stored in the `.mat` file (`"_Speed": 2.0`, `"_Color": [1, 0, 0, 1]`,
/// `"_MainTex": "Assets/Textures/Grid.png"`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Float(f32),
    Vector([f32; 4]),
    Texture(Option<String>),
}

impl PropertyValue {
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<[f32; 4]> {
        match self {
            Self::Vector(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_texture(&self) -> Option<&str> {
        match self {
            Self::Texture(t) => t.as_deref(),
            _ => None,
        }
    }

    /// Does this value fit a property of `kind`?
    pub fn fits(&self, kind: PropertyKind) -> bool {
        matches!(
            (self, kind),
            (
                Self::Float(_),
                PropertyKind::Float | PropertyKind::Range(..) | PropertyKind::Toggle
            ) | (Self::Vector(_), PropertyKind::Color | PropertyKind::Vector)
                | (Self::Texture(_), PropertyKind::Texture(_))
        )
    }
}

/// One `// @property` line.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderProperty {
    /// Identifier used in WGSL (`_Color` → `material._Color`).
    pub name: String,
    /// Inspector label (the quoted name, else the identifier without its leading `_`).
    pub display: String,
    pub kind: PropertyKind,
    pub default: PropertyValue,
    /// Inspector tooltip (`// @property … // tooltip text`).
    pub tooltip: Option<String>,
    /// 1-based line in the file.
    pub line: usize,
}

/// A parsed shader file.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderAsset {
    /// The whole file (the header lines are WGSL comments, so it compiles as is).
    pub source: String,
    pub settings: ShaderSettings,
    pub properties: Vec<ShaderProperty>,
    /// Defines `fn vertex(v: VertexData) -> VertexData` (vertex displacement).
    pub has_vertex: bool,
    /// Reads `scene_color` / `scene_depth` (needs the opaque scene copied first).
    pub uses_scene: bool,
}

impl ShaderAsset {
    pub fn property(&self, name: &str) -> Option<&ShaderProperty> {
        self.properties.iter().find(|p| p.name == name)
    }
}

/// A problem in a shader file: 1-based line and column (0 = unknown) and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for ShaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 {
            write!(f, "{}:{}: {}", self.line, self.column.max(1), self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}
