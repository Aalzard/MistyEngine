//! Editor commands queued by menus, toolbar and shortcuts, applied once per frame.

use crate::layout::Tab;

/// Primitive shapes offered by GameObject ▸ 3D Object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
    Quad,
}

impl Primitive {
    pub const ALL: [Primitive; 6] = [
        Primitive::Cube,
        Primitive::Sphere,
        Primitive::Capsule,
        Primitive::Cylinder,
        Primitive::Plane,
        Primitive::Quad,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Primitive::Cube => "Cube",
            Primitive::Sphere => "Sphere",
            Primitive::Capsule => "Capsule",
            Primitive::Cylinder => "Cylinder",
            Primitive::Plane => "Plane",
            Primitive::Quad => "Quad",
        }
    }
}

/// Light types offered by GameObject ▸ Light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

impl LightKind {
    pub const ALL: [LightKind; 3] = [LightKind::Directional, LightKind::Point, LightKind::Spot];

    pub fn name(self) -> &'static str {
        match self {
            LightKind::Directional => "Directional Light",
            LightKind::Point => "Point Light",
            LightKind::Spot => "Spot Light",
        }
    }
}

/// One editor command.
#[derive(Clone, Debug)]
pub enum Action {
    FocusTab(Tab),
    ResetLayout,
    Quit,
    NewScene,
    CreateEmpty,
    CreatePrimitive(Primitive),
    CreateLight(LightKind),
    CreateCamera,
    DeleteSelected,
    DuplicateSelected,
    DeselectAll,
    FrameSelected,
    TogglePlay,
    TogglePause,
    Step,
    /// Menu item that exists for layout parity but is not built yet.
    NotImplemented(&'static str),
}
