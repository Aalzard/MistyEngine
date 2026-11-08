//! Value types of the Effect Graph: port types, inline values, curves and the conversions
//! between them.

use serde::{Deserialize, Serialize};

pub use crate::particles::{AlphaKey, ColorKey, Curve, Gradient};

/// Type a port carries. `Color` is an RGBA vector (sRGB in files and fields, linear on the GPU).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    Float,
    Vec3,
    Color,
    Gradient,
    Curve,
}

impl ValueType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Float => "Float",
            Self::Vec3 => "Vector3",
            Self::Color => "Color",
            Self::Gradient => "Gradient",
            Self::Curve => "Curve",
        }
    }

    /// Number of float components (0 for gradients and curves).
    pub fn dims(self) -> usize {
        match self {
            Self::Float => 1,
            Self::Vec3 => 3,
            Self::Color => 4,
            Self::Gradient | Self::Curve => 0,
        }
    }

    /// WGSL type of a value of this type.
    pub fn wgsl(self) -> &'static str {
        match self {
            Self::Float => "f32",
            Self::Vec3 => "vec3<f32>",
            Self::Color => "vec4<f32>",
            // Ramps are referenced by row index.
            Self::Gradient | Self::Curve => "u32",
        }
    }

    /// Implicit conversion: floats splat, vectors truncate or pad (alpha 1), ramps only to the
    /// same kind of ramp.
    pub fn converts_to(self, to: ValueType) -> bool {
        match (self, to) {
            (a, b) if a == b => true,
            (Self::Gradient | Self::Curve, _) | (_, Self::Gradient | Self::Curve) => false,
            _ => true,
        }
    }
}

/// Type of a port: fixed, or dynamic (the widest connected dynamic input, Float by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortType {
    Fixed(ValueType),
    Dynamic,
}

impl PortType {
    pub fn resolve(self, dynamic: ValueType) -> ValueType {
        match self {
            Self::Fixed(t) => t,
            Self::Dynamic => dynamic,
        }
    }
}

/// An inline value of an unconnected input (or a property's default).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Float(f32),
    Vec3([f32; 3]),
    Color([f32; 4]),
    Gradient(Gradient),
    Curve(Curve),
}

impl Value {
    pub fn ty(&self) -> ValueType {
        match self {
            Self::Float(_) => ValueType::Float,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Color(_) => ValueType::Color,
            Self::Gradient(_) => ValueType::Gradient,
            Self::Curve(_) => ValueType::Curve,
        }
    }

    /// The four components (floats splat, vectors pad with alpha 1).
    pub fn components(&self) -> [f32; 4] {
        match self {
            Self::Float(v) => [*v; 4],
            Self::Vec3(v) => [v[0], v[1], v[2], 1.0],
            Self::Color(c) => *c,
            Self::Gradient(_) | Self::Curve(_) => [0.0; 4],
        }
    }

    /// The value converted to `ty` (as far as it converts; ramps keep their own default).
    pub fn converted(&self, ty: ValueType) -> Value {
        if self.ty() == ty {
            return self.clone();
        }
        let c = self.components();
        match ty {
            ValueType::Float => Value::Float(c[0]),
            ValueType::Vec3 => Value::Vec3([c[0], c[1], c[2]]),
            ValueType::Color => Value::Color(c),
            ValueType::Gradient => Value::Gradient(Gradient::default()),
            ValueType::Curve => Value::Curve(Curve::default()),
        }
    }

    pub fn as_float(&self) -> f32 {
        self.components()[0]
    }

    pub fn default_of(ty: ValueType) -> Value {
        match ty {
            ValueType::Float => Value::Float(0.0),
            ValueType::Vec3 => Value::Vec3([0.0; 3]),
            ValueType::Color => Value::Color([1.0; 4]),
            ValueType::Gradient => Value::Gradient(Gradient::default()),
            ValueType::Curve => Value::Curve(Curve::default()),
        }
    }
}

/// A named input or output of a block or operator.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub name: &'static str,
    pub ty: PortType,
    /// Inline value when unconnected (inputs only).
    pub default: Value,
}

impl Port {
    pub fn float(name: &'static str, v: f32) -> Self {
        Self {
            name,
            ty: PortType::Fixed(ValueType::Float),
            default: Value::Float(v),
        }
    }

    pub fn vec3(name: &'static str, v: [f32; 3]) -> Self {
        Self {
            name,
            ty: PortType::Fixed(ValueType::Vec3),
            default: Value::Vec3(v),
        }
    }

    pub fn color(name: &'static str, v: [f32; 4]) -> Self {
        Self {
            name,
            ty: PortType::Fixed(ValueType::Color),
            default: Value::Color(v),
        }
    }

    pub fn gradient(name: &'static str, g: Gradient) -> Self {
        Self {
            name,
            ty: PortType::Fixed(ValueType::Gradient),
            default: Value::Gradient(g),
        }
    }

    pub fn curve(name: &'static str, c: Curve) -> Self {
        Self {
            name,
            ty: PortType::Fixed(ValueType::Curve),
            default: Value::Curve(c),
        }
    }

    pub fn dynamic(name: &'static str, v: f32) -> Self {
        Self {
            name,
            ty: PortType::Dynamic,
            default: Value::Float(v),
        }
    }

    /// An output port.
    pub fn out(name: &'static str, ty: PortType) -> Self {
        Self {
            name,
            ty,
            default: Value::Float(0.0),
        }
    }
}

/// sRGB → linear for one channel (values above 1 stay HDR).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.0 { 0.0 } else { c.powf(2.2) }
}

/// sRGB colour + alpha → linear colour + alpha.
pub fn color_to_linear(c: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(c[0]),
        srgb_to_linear(c[1]),
        srgb_to_linear(c[2]),
        c[3],
    ]
}

/// Format a float as a typed WGSL literal (`1.0f`), finite values only.
pub fn lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        format!("{s}f")
    } else {
        format!("{s}.0f")
    }
}
