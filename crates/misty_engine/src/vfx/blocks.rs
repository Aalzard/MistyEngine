//! The block catalog: what each block of a context does, its inputs and settings, and which
//! contexts accept it.

use serde::{Deserialize, Serialize};

use super::types::{AlphaKey, ColorKey, Curve, Gradient, Port, PortType, ValueType};

/// The four kinds of context of a particle system, in flow order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ContextType {
    Spawn,
    Initialize,
    Update,
    Output,
}

impl ContextType {
    pub const ALL: [Self; 4] = [Self::Spawn, Self::Initialize, Self::Update, Self::Output];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spawn => "Spawn",
            Self::Initialize => "Initialize Particle",
            Self::Update => "Update Particle",
            Self::Output => "Output Particle",
        }
    }
}

/// A particle attribute the Set Attribute block writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Attribute {
    Position,
    Velocity,
    Color,
    Alpha,
    Size,
    Angle,
    Lifetime,
    Age,
}

impl Attribute {
    pub const ALL: [Self; 8] = [
        Self::Position,
        Self::Velocity,
        Self::Color,
        Self::Alpha,
        Self::Size,
        Self::Angle,
        Self::Lifetime,
        Self::Age,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::Velocity => "Velocity",
            Self::Color => "Color",
            Self::Alpha => "Alpha",
            Self::Size => "Size",
            Self::Angle => "Angle",
            Self::Lifetime => "Lifetime",
            Self::Age => "Age",
        }
    }

    pub fn ty(self) -> ValueType {
        match self {
            Self::Position | Self::Velocity => ValueType::Vec3,
            Self::Color => ValueType::Color,
            _ => ValueType::Float,
        }
    }

    /// The WGSL lvalue in the kernels (`p` is the particle being written).
    pub fn field(self) -> &'static str {
        match self {
            Self::Position => "p.position",
            Self::Velocity => "p.velocity",
            Self::Color => "p.color",
            Self::Alpha => "p.color.a",
            Self::Size => "p.size",
            Self::Angle => "p.angle",
            Self::Lifetime => "p.lifetime",
            Self::Age => "p.age",
        }
    }

    fn default_value(self) -> super::types::Value {
        use super::types::Value as V;
        match self {
            Self::Position | Self::Velocity => V::Vec3([0.0; 3]),
            Self::Color => V::Color([1.0; 4]),
            Self::Alpha => V::Float(1.0),
            Self::Size => V::Float(0.1),
            Self::Angle => V::Float(0.0),
            Self::Lifetime => V::Float(1.0),
            Self::Age => V::Float(0.0),
        }
    }
}

/// How a written value combines with the attribute's current value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Composition {
    #[default]
    Overwrite,
    Add,
    Multiply,
}

impl Composition {
    pub const ALL: [Self; 3] = [Self::Overwrite, Self::Add, Self::Multiply];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overwrite => "Overwrite",
            Self::Add => "Add",
            Self::Multiply => "Multiply",
        }
    }
}

/// Shape blocks: fill the volume or only the surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShapeMode {
    #[default]
    Volume,
    Surface,
}

/// How Output particles face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrientMode {
    #[default]
    FaceCamera,
    /// Stretched along the velocity (sparks, rain).
    AlongVelocity,
    /// Lies flat facing world up.
    FaceUp,
}

impl OrientMode {
    pub const ALL: [Self; 3] = [Self::FaceCamera, Self::AlongVelocity, Self::FaceUp];

    pub fn label(self) -> &'static str {
        match self {
            Self::FaceCamera => "Face Camera",
            Self::AlongVelocity => "Along Velocity",
            Self::FaceUp => "Face Up",
        }
    }
}

/// What a block does, with its settings (inputs are separate, see [`BlockKind::inputs`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BlockKind {
    // Spawn.
    /// Particles per second.
    ConstantRate,
    /// One burst after a delay.
    SingleBurst,
    /// A burst every period.
    PeriodicBurst,
    // Attributes (Initialize, Update, Output).
    SetAttribute {
        attribute: Attribute,
        #[serde(default)]
        random: bool,
        #[serde(default)]
        composition: Composition,
    },
    // Initialize shapes and velocity.
    PositionSphere {
        #[serde(default)]
        mode: ShapeMode,
    },
    PositionBox {
        #[serde(default)]
        mode: ShapeMode,
    },
    PositionCone {
        #[serde(default)]
        mode: ShapeMode,
    },
    PositionCircle {
        #[serde(default)]
        mode: ShapeMode,
    },
    /// Velocity along the direction the shape blocks chose, with a random speed.
    VelocityFromDirection,
    // Update forces and collisions.
    Gravity,
    LinearDrag,
    Turbulence,
    ConformToSphere,
    Vortex,
    CollidePlane,
    CollideSphere {
        /// Keep particles inside the sphere instead of outside.
        #[serde(default)]
        inverted: bool,
    },
    /// Kill particles inside a sphere.
    KillSphere,
    // Over life (Update overwrites; Output multiplies).
    ColorOverLife,
    SizeOverLife,
    // Output.
    Orient {
        #[serde(default)]
        mode: OrientMode,
    },
}

/// Inputs read by the spawn system on the CPU (constants or properties).
pub mod spawn_ports {
    pub const RATE: &str = "Rate";
    pub const COUNT: &str = "Count";
    pub const DELAY: &str = "Delay";
    pub const PERIOD: &str = "Period";
}

fn fire_gradient() -> Gradient {
    Gradient {
        color_keys: vec![
            ColorKey {
                time: 0.0,
                color: [1.0, 1.0, 1.0],
            },
            ColorKey {
                time: 1.0,
                color: [1.0, 1.0, 1.0],
            },
        ],
        alpha_keys: vec![
            AlphaKey {
                time: 0.0,
                alpha: 1.0,
            },
            AlphaKey {
                time: 1.0,
                alpha: 0.0,
            },
        ],
    }
}

impl BlockKind {
    /// Every block the Add Block popup lists (default settings).
    pub fn catalog() -> Vec<BlockKind> {
        let mut v = vec![
            Self::ConstantRate,
            Self::SingleBurst,
            Self::PeriodicBurst,
            Self::PositionSphere {
                mode: ShapeMode::Volume,
            },
            Self::PositionBox {
                mode: ShapeMode::Volume,
            },
            Self::PositionCone {
                mode: ShapeMode::Volume,
            },
            Self::PositionCircle {
                mode: ShapeMode::Volume,
            },
            Self::VelocityFromDirection,
        ];
        for a in Attribute::ALL {
            for random in [false, true] {
                v.push(Self::SetAttribute {
                    attribute: a,
                    random,
                    composition: Composition::Overwrite,
                });
            }
        }
        v.extend([
            Self::Gravity,
            Self::LinearDrag,
            Self::Turbulence,
            Self::ConformToSphere,
            Self::Vortex,
            Self::CollidePlane,
            Self::CollideSphere { inverted: false },
            Self::KillSphere,
            Self::ColorOverLife,
            Self::SizeOverLife,
            Self::Orient {
                mode: OrientMode::FaceCamera,
            },
        ]);
        v
    }

    pub fn title(&self) -> String {
        let shape = |name: &str, mode: &ShapeMode| match mode {
            ShapeMode::Volume => format!("Set Position ({name})"),
            ShapeMode::Surface => format!("Set Position ({name} Surface)"),
        };
        match self {
            Self::ConstantRate => "Constant Spawn Rate".into(),
            Self::SingleBurst => "Single Burst".into(),
            Self::PeriodicBurst => "Periodic Burst".into(),
            Self::SetAttribute {
                attribute,
                random,
                composition,
            } => {
                let verb = match composition {
                    Composition::Overwrite => "Set",
                    Composition::Add => "Add",
                    Composition::Multiply => "Multiply",
                };
                let r = if *random { " Random" } else { "" };
                format!("{verb} {}{r}", attribute.label())
            }
            Self::PositionSphere { mode } => shape("Sphere", mode),
            Self::PositionBox { mode } => shape("Box", mode),
            Self::PositionCone { mode } => shape("Cone", mode),
            Self::PositionCircle { mode } => shape("Circle", mode),
            Self::VelocityFromDirection => "Set Velocity from Direction".into(),
            Self::Gravity => "Gravity".into(),
            Self::LinearDrag => "Linear Drag".into(),
            Self::Turbulence => "Turbulence".into(),
            Self::ConformToSphere => "Conform to Sphere".into(),
            Self::Vortex => "Vortex".into(),
            Self::CollidePlane => "Collide with Plane".into(),
            Self::CollideSphere { inverted: false } => "Collide with Sphere".into(),
            Self::CollideSphere { inverted: true } => "Collide with Sphere (Inside)".into(),
            Self::KillSphere => "Kill (Sphere)".into(),
            Self::ColorOverLife => "Set Color over Life".into(),
            Self::SizeOverLife => "Set Size over Life".into(),
            Self::Orient { mode } => format!("Orient: {}", mode.label()),
        }
    }

    /// Popup category.
    pub fn category(&self) -> &'static str {
        match self {
            Self::ConstantRate | Self::SingleBurst | Self::PeriodicBurst => "Spawn",
            Self::SetAttribute { .. } => "Attribute",
            Self::PositionSphere { .. }
            | Self::PositionBox { .. }
            | Self::PositionCone { .. }
            | Self::PositionCircle { .. }
            | Self::VelocityFromDirection => "Position / Velocity",
            Self::Gravity
            | Self::LinearDrag
            | Self::Turbulence
            | Self::ConformToSphere
            | Self::Vortex => "Forces",
            Self::CollidePlane | Self::CollideSphere { .. } | Self::KillSphere => "Collision",
            Self::ColorOverLife | Self::SizeOverLife => "Over Life",
            Self::Orient { .. } => "Orientation",
        }
    }

    /// Extra search words.
    pub fn keywords(&self) -> &'static str {
        match self {
            Self::ConstantRate => "emit rate continuous",
            Self::SingleBurst | Self::PeriodicBurst => "emit burst explosion",
            Self::PositionSphere { .. }
            | Self::PositionBox { .. }
            | Self::PositionCone { .. }
            | Self::PositionCircle { .. } => "shape spawn emitter",
            Self::VelocityFromDirection => "speed direction",
            Self::Turbulence => "noise curl wind",
            Self::Vortex => "swirl rotate spin",
            Self::ConformToSphere => "attract magnet",
            Self::CollidePlane | Self::CollideSphere { .. } => "bounce floor ground",
            Self::KillSphere => "destroy reap",
            Self::ColorOverLife => "gradient fade",
            Self::SizeOverLife => "curve scale",
            Self::LinearDrag => "damping air friction",
            Self::Gravity => "fall force",
            Self::SetAttribute { .. } => "attribute random",
            Self::Orient { .. } => "billboard facing stretch",
        }
    }

    /// Contexts the block can be added to.
    pub fn allowed_in(&self, ctx: ContextType) -> bool {
        use ContextType as C;
        match self {
            Self::ConstantRate | Self::SingleBurst | Self::PeriodicBurst => ctx == C::Spawn,
            Self::SetAttribute { attribute, .. } => match ctx {
                C::Spawn => false,
                C::Initialize | C::Update => true,
                // Output blocks change what is drawn only (not the stored particle).
                C::Output => matches!(
                    attribute,
                    Attribute::Color | Attribute::Alpha | Attribute::Size | Attribute::Angle
                ),
            },
            Self::PositionSphere { .. }
            | Self::PositionBox { .. }
            | Self::PositionCone { .. }
            | Self::PositionCircle { .. }
            | Self::VelocityFromDirection => ctx == C::Initialize,
            Self::Gravity
            | Self::LinearDrag
            | Self::Turbulence
            | Self::ConformToSphere
            | Self::Vortex
            | Self::CollidePlane
            | Self::CollideSphere { .. }
            | Self::KillSphere => ctx == C::Update,
            Self::ColorOverLife | Self::SizeOverLife => matches!(ctx, C::Update | C::Output),
            Self::Orient { .. } => ctx == C::Output,
        }
    }

    /// The block's inputs with their defaults.
    pub fn inputs(&self) -> Vec<Port> {
        use spawn_ports::*;
        match self {
            Self::ConstantRate => vec![Port::float(RATE, 20.0)],
            Self::SingleBurst => vec![Port::float(COUNT, 50.0), Port::float(DELAY, 0.0)],
            Self::PeriodicBurst => vec![
                Port::float(COUNT, 30.0),
                Port::float(PERIOD, 1.0),
                Port::float(DELAY, 0.0),
            ],
            Self::SetAttribute {
                attribute, random, ..
            } => {
                let v = attribute.default_value();
                let ty = PortType::Fixed(attribute.ty());
                if *random {
                    let mut b = v.clone();
                    if let super::types::Value::Float(f) = &mut b {
                        *f = if *f == 0.0 { 1.0 } else { *f * 2.0 };
                    }
                    vec![
                        Port {
                            name: "A",
                            ty,
                            default: v,
                        },
                        Port {
                            name: "B",
                            ty,
                            default: b,
                        },
                    ]
                } else {
                    vec![Port {
                        name: attribute.label(),
                        ty,
                        default: v,
                    }]
                }
            }
            Self::PositionSphere { .. } => {
                vec![Port::vec3("Center", [0.0; 3]), Port::float("Radius", 0.5)]
            }
            Self::PositionBox { .. } => {
                vec![Port::vec3("Center", [0.0; 3]), Port::vec3("Size", [1.0; 3])]
            }
            Self::PositionCone { .. } => vec![
                Port::float("Radius", 0.3),
                Port::float("Angle", 25.0),
                Port::float("Height", 0.5),
            ],
            Self::PositionCircle { .. } => {
                vec![Port::vec3("Center", [0.0; 3]), Port::float("Radius", 0.5)]
            }
            Self::VelocityFromDirection => {
                vec![Port::float("Min Speed", 1.0), Port::float("Max Speed", 2.0)]
            }
            Self::Gravity => vec![Port::vec3("Force", [0.0, -9.81, 0.0])],
            Self::LinearDrag => vec![Port::float("Drag", 0.5)],
            Self::Turbulence => vec![
                Port::float("Intensity", 1.0),
                Port::float("Frequency", 1.0),
                Port::float("Scroll", 0.2),
            ],
            Self::ConformToSphere => vec![
                Port::vec3("Center", [0.0; 3]),
                Port::float("Radius", 1.0),
                Port::float("Attraction", 5.0),
                Port::float("Stick", 2.0),
            ],
            Self::Vortex => vec![
                Port::vec3("Center", [0.0; 3]),
                Port::vec3("Axis", [0.0, 1.0, 0.0]),
                Port::float("Speed", 3.0),
                Port::float("Pull", 1.0),
            ],
            Self::CollidePlane => vec![
                Port::vec3("Point", [0.0; 3]),
                Port::vec3("Normal", [0.0, 1.0, 0.0]),
                Port::float("Bounce", 0.4),
                Port::float("Friction", 0.2),
                Port::float("Lifetime Loss", 0.0),
            ],
            Self::CollideSphere { .. } => vec![
                Port::vec3("Center", [0.0; 3]),
                Port::float("Radius", 1.0),
                Port::float("Bounce", 0.4),
                Port::float("Friction", 0.2),
            ],
            Self::KillSphere => {
                vec![Port::vec3("Center", [0.0; 3]), Port::float("Radius", 0.5)]
            }
            Self::ColorOverLife => vec![Port::gradient("Gradient", fire_gradient())],
            Self::SizeOverLife => vec![Port::curve("Curve", Curve::linear(1.0, 0.0))],
            Self::Orient {
                mode: OrientMode::AlongVelocity,
            } => vec![Port::float("Stretch", 0.05)],
            Self::Orient { .. } => Vec::new(),
        }
    }

    /// The input called `name`.
    pub fn input(&self, name: &str) -> Option<Port> {
        self.inputs().into_iter().find(|p| p.name == name)
    }

    /// Does this block run on the CPU (the spawn system)?
    pub fn is_spawn(&self) -> bool {
        matches!(
            self,
            Self::ConstantRate | Self::SingleBurst | Self::PeriodicBurst
        )
    }
}
