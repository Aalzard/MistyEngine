//! Editor commands queued by menus, toolbar and shortcuts, applied once per frame.

use misty_engine::hecs::Entity;
use misty_engine::scene::{LightType, PrimitiveMesh};

use crate::layout::Tab;
use crate::layouts::LayoutRef;

/// What a create command makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Create {
    Empty,
    Primitive(PrimitiveMesh),
    Light(LightType),
    Camera,
}

impl Create {
    pub fn default_name(self) -> &'static str {
        match self {
            Create::Empty => "GameObject",
            Create::Primitive(p) => p.name(),
            Create::Light(LightType::Directional) => "Directional Light",
            Create::Light(LightType::Point) => "Point Light",
            Create::Light(LightType::Spot) => "Spot Light",
            Create::Camera => "Camera",
        }
    }
}

/// One editor command.
#[derive(Clone, Debug)]
pub enum Action {
    FocusTab(Tab),
    /// Window ▸ Layouts: apply a built-in or saved layout.
    LoadLayout(LayoutRef),
    /// Save the current arrangement under a name.
    SaveLayout(String),
    DeleteLayout(String),
    /// Delete user layouts and go back to Default.
    ResetAllLayouts,
    Quit,
    NewScene,
    /// File ▸ Open Scene (file dialog).
    OpenScene,
    /// Open a scene by project-relative path (Project window, Open Recent).
    OpenScenePath(String),
    SaveScene,
    SaveSceneAs,
    /// Create an object; `parent` = Hierarchy row it was created from (None = scene root).
    Create {
        what: Create,
        parent: Option<Entity>,
    },
    CreateEmptyParent,
    DeleteSelected,
    DuplicateSelected,
    RenameSelected,
    SelectAll,
    DeselectAll,
    FrameSelected,
    ToggleActive,
    ClearParent,
    SetSibling {
        first: bool,
    },
    MoveToView,
    AlignWithView,
    AlignViewToSelected,
    Undo,
    Redo,
    /// Add a component (registry name) to every selected object.
    AddComponent(&'static str),
    RemoveComponent {
        entity: Entity,
        name: String,
    },
    ResetComponent {
        entity: Entity,
        name: String,
    },
    MoveComponent {
        entity: Entity,
        name: String,
        dir: i32,
    },
    PasteComponentValues {
        entity: Entity,
        name: String,
    },
    /// Attach a script asset ("Assets/…/Name.lua") to an object.
    AddScript {
        entity: Entity,
        path: String,
    },
    RemoveScript {
        entity: Entity,
        slot: usize,
    },
    /// Clear a script's Inspector overrides (back to the script defaults).
    ResetScript {
        entity: Entity,
        slot: usize,
    },
    /// Component > Add... (opens the Inspector's search popup).
    OpenAddComponent,
    TogglePlay,
    TogglePause,
    Step,
    /// Menu item that exists for layout parity but is not built yet.
    NotImplemented(&'static str),
}
