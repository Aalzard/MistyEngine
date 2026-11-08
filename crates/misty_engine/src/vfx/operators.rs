//! The operator catalog: nodes wired into block inputs (math, random, time, noise, attribute
//! reads, sampling gradients and curves, Blackboard properties).

use serde::{Deserialize, Serialize};

use super::types::{Curve, Gradient, Port, PortType, ValueType};

/// A particle value an operator can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttributeRead {
    Position,
    Velocity,
    Color,
    Alpha,
    Size,
    Angle,
    Age,
    Lifetime,
    /// Age / Lifetime (0..1).
    AgeOverLifetime,
    /// A random number per particle, constant over its life (0..1).
    ParticleRandom,
}

impl AttributeRead {
    pub const ALL: [Self; 10] = [
        Self::Position,
        Self::Velocity,
        Self::Color,
        Self::Alpha,
        Self::Size,
        Self::Angle,
        Self::Age,
        Self::Lifetime,
        Self::AgeOverLifetime,
        Self::ParticleRandom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::Velocity => "Velocity",
            Self::Color => "Color",
            Self::Alpha => "Alpha",
            Self::Size => "Size",
            Self::Angle => "Angle",
            Self::Age => "Age",
            Self::Lifetime => "Lifetime",
            Self::AgeOverLifetime => "Age Over Lifetime",
            Self::ParticleRandom => "Particle Random",
        }
    }

    pub fn ty(self) -> ValueType {
        match self {
            Self::Position | Self::Velocity => ValueType::Vec3,
            Self::Color => ValueType::Color,
            _ => ValueType::Float,
        }
    }

    /// WGSL expression reading it from the particle `p`.
    pub fn expr(self) -> &'static str {
        match self {
            Self::Position => "p.position",
            Self::Velocity => "p.velocity",
            Self::Color => "p.color",
            Self::Alpha => "p.color.a",
            Self::Size => "p.size",
            Self::Angle => "p.angle",
            Self::Age => "p.age",
            Self::Lifetime => "p.lifetime",
            Self::AgeOverLifetime => "clamp(p.age / max(p.lifetime, 0.0001f), 0.0f, 1.0f)",
            Self::ParticleRandom => "vfx_unit(vfx_hash(p.seed ^ 0x5bd1e995u))",
        }
    }
}

/// Random operator: a new number every evaluation, or one fixed per particle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RandomMode {
    #[default]
    PerEvaluation,
    PerParticle,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OperatorKind {
    /// Reads a Blackboard property.
    Property {
        name: String,
    },
    // Constants (their value is the inline input).
    Float,
    Vector3,
    Color,
    // Inputs.
    Time,
    GetAttribute {
        attribute: AttributeRead,
    },
    // Math (dynamic).
    Add,
    Subtract,
    Multiply,
    Divide,
    Minimum,
    Maximum,
    Power,
    Lerp,
    Clamp,
    Remap,
    Absolute,
    Negate,
    OneMinus,
    Fraction,
    Sine,
    Cosine,
    // Vectors.
    Length,
    Normalize,
    Distance,
    Dot,
    Cross,
    Combine,
    Split,
    // Random and noise.
    Random {
        #[serde(default)]
        mode: RandomMode,
    },
    Noise,
    CurlNoise,
    // Sampling.
    SampleGradient,
    SampleCurve,
    /// A sub-graph asset (`.vfxoperator`): its properties are inputs, its Output nodes outputs.
    Subgraph {
        path: String,
    },
    /// Inside a sub-graph asset: one output of the sub-graph.
    SubgraphOutput {
        name: String,
    },
}

impl OperatorKind {
    /// Every operator the Create Node popup lists (properties are added per Blackboard entry).
    pub fn catalog() -> Vec<OperatorKind> {
        let mut v = vec![Self::Float, Self::Vector3, Self::Color, Self::Time];
        for a in AttributeRead::ALL {
            v.push(Self::GetAttribute { attribute: a });
        }
        v.extend([
            Self::Add,
            Self::Subtract,
            Self::Multiply,
            Self::Divide,
            Self::Minimum,
            Self::Maximum,
            Self::Power,
            Self::Lerp,
            Self::Clamp,
            Self::Remap,
            Self::Absolute,
            Self::Negate,
            Self::OneMinus,
            Self::Fraction,
            Self::Sine,
            Self::Cosine,
            Self::Length,
            Self::Normalize,
            Self::Distance,
            Self::Dot,
            Self::Cross,
            Self::Combine,
            Self::Split,
            Self::Random {
                mode: RandomMode::PerEvaluation,
            },
            Self::Random {
                mode: RandomMode::PerParticle,
            },
            Self::Noise,
            Self::CurlNoise,
            Self::SampleGradient,
            Self::SampleCurve,
        ]);
        v
    }

    pub fn title(&self) -> String {
        match self {
            Self::Property { name } => name.clone(),
            Self::GetAttribute { attribute } => format!("Get {}", attribute.label()),
            Self::Random {
                mode: RandomMode::PerEvaluation,
            } => "Random Number".into(),
            Self::Random {
                mode: RandomMode::PerParticle,
            } => "Random Number (Per Particle)".into(),
            Self::OneMinus => "One Minus".into(),
            Self::CurlNoise => "Curl Noise".into(),
            Self::SampleGradient => "Sample Gradient".into(),
            Self::SampleCurve => "Sample Curve".into(),
            Self::Vector3 => "Vector3".into(),
            Self::Subgraph { path } => super::subgraph::title(path),
            Self::SubgraphOutput { name } => format!("Output: {name}"),
            k => format!("{k:?}"),
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::Property { .. } => "Properties",
            Self::Float | Self::Vector3 | Self::Color => "Constants",
            Self::Time | Self::GetAttribute { .. } => "Inputs",
            Self::Length
            | Self::Normalize
            | Self::Distance
            | Self::Dot
            | Self::Cross
            | Self::Combine
            | Self::Split => "Vector",
            Self::Random { .. } | Self::Noise | Self::CurlNoise => "Random / Noise",
            Self::SampleGradient | Self::SampleCurve => "Sampling",
            Self::Subgraph { .. } | Self::SubgraphOutput { .. } => "Sub-graph",
            _ => "Math",
        }
    }

    pub fn keywords(&self) -> &'static str {
        match self {
            Self::Add => "plus sum +",
            Self::Subtract => "minus -",
            Self::Multiply => "times scale *",
            Self::Divide => "/",
            Self::Lerp => "mix blend interpolate",
            Self::Time => "seconds delta",
            Self::Noise => "perlin value",
            Self::CurlNoise => "turbulence flow",
            Self::Random { .. } => "rand",
            Self::SampleGradient => "color ramp",
            Self::SampleCurve => "animation curve",
            Self::Combine => "vector3 compose append",
            Self::Split => "components xyz",
            Self::GetAttribute { .. } => "attribute particle read",
            Self::Subgraph { .. } => "subgraph function macro",
            Self::SubgraphOutput { .. } => "subgraph result return",
            _ => "",
        }
    }

    /// Inputs (dynamic ones resolve to the widest connected one).
    pub fn inputs(&self) -> Vec<Port> {
        match self {
            Self::Property { .. } | Self::Time | Self::GetAttribute { .. } => Vec::new(),
            Self::Float => vec![Port::float("Value", 1.0)],
            Self::Vector3 => vec![Port::vec3("Value", [0.0; 3])],
            Self::Color => vec![Port::color("Value", [1.0; 4])],
            Self::Add | Self::Subtract => {
                vec![Port::dynamic("A", 0.0), Port::dynamic("B", 0.0)]
            }
            Self::Multiply | Self::Divide | Self::Power => {
                vec![Port::dynamic("A", 1.0), Port::dynamic("B", 1.0)]
            }
            Self::Minimum | Self::Maximum => {
                vec![Port::dynamic("A", 0.0), Port::dynamic("B", 1.0)]
            }
            Self::Lerp => vec![
                Port::dynamic("A", 0.0),
                Port::dynamic("B", 1.0),
                Port::float("T", 0.5),
            ],
            Self::Clamp => vec![
                Port::dynamic("Input", 0.0),
                Port::dynamic("Min", 0.0),
                Port::dynamic("Max", 1.0),
            ],
            Self::Remap => vec![
                Port::dynamic("Input", 0.5),
                Port::float("In Min", 0.0),
                Port::float("In Max", 1.0),
                Port::float("Out Min", 0.0),
                Port::float("Out Max", 1.0),
            ],
            Self::Absolute
            | Self::Negate
            | Self::OneMinus
            | Self::Fraction
            | Self::Sine
            | Self::Cosine => vec![Port::dynamic("Input", 0.0)],
            Self::Length | Self::Normalize => vec![Port::vec3("Input", [0.0, 1.0, 0.0])],
            Self::Distance | Self::Dot | Self::Cross => {
                vec![Port::vec3("A", [0.0; 3]), Port::vec3("B", [0.0, 1.0, 0.0])]
            }
            Self::Combine => vec![
                Port::float("X", 0.0),
                Port::float("Y", 0.0),
                Port::float("Z", 0.0),
            ],
            Self::Split => vec![Port::vec3("Input", [0.0; 3])],
            Self::Random { .. } => vec![Port::dynamic("Min", 0.0), Port::dynamic("Max", 1.0)],
            Self::Noise | Self::CurlNoise => vec![
                Port::vec3("Coordinate", [0.0; 3]),
                Port::float("Frequency", 1.0),
            ],
            Self::SampleGradient => vec![
                Port::gradient("Gradient", Gradient::default()),
                Port::float("Time", 0.0),
            ],
            Self::SampleCurve => vec![
                Port::curve("Curve", Curve::default()),
                Port::float("Time", 0.0),
            ],
            Self::Subgraph { path } => super::subgraph::info(path)
                .map(|i| i.inputs.clone())
                .unwrap_or_default(),
            Self::SubgraphOutput { .. } => vec![Port::dynamic("Value", 0.0)],
        }
    }

    /// Outputs; `property` = the type of the Blackboard property a Property node reads.
    pub fn outputs(&self, property: Option<ValueType>) -> Vec<Port> {
        let f = |t| PortType::Fixed(t);
        match self {
            Self::Property { .. } => {
                property.map_or_else(Vec::new, |t| vec![Port::out("Out", f(t))])
            }
            Self::Float => vec![Port::out("Out", f(ValueType::Float))],
            Self::Vector3 => vec![Port::out("Out", f(ValueType::Vec3))],
            Self::Color => vec![Port::out("Out", f(ValueType::Color))],
            Self::Time => vec![
                Port::out("Total", f(ValueType::Float)),
                Port::out("Delta", f(ValueType::Float)),
            ],
            Self::GetAttribute { attribute } => vec![Port::out("Out", f(attribute.ty()))],
            Self::Length | Self::Distance | Self::Dot | Self::Noise => {
                vec![Port::out("Out", f(ValueType::Float))]
            }
            Self::Normalize | Self::Cross | Self::Combine | Self::CurlNoise => {
                vec![Port::out("Out", f(ValueType::Vec3))]
            }
            Self::Split => vec![
                Port::out("X", f(ValueType::Float)),
                Port::out("Y", f(ValueType::Float)),
                Port::out("Z", f(ValueType::Float)),
            ],
            Self::SampleGradient => vec![Port::out("Out", f(ValueType::Color))],
            Self::SampleCurve => vec![Port::out("Out", f(ValueType::Float))],
            Self::Subgraph { path } => super::subgraph::info(path)
                .map(|i| i.outputs.clone())
                .unwrap_or_default(),
            Self::SubgraphOutput { .. } => Vec::new(),
            _ => vec![Port::out("Out", PortType::Dynamic)],
        }
    }

    pub fn input(&self, name: &str) -> Option<Port> {
        self.inputs().into_iter().find(|p| p.name == name)
    }
}
