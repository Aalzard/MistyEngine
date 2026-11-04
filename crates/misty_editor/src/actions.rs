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
    AudioSource,
    /// GameObject ▸ UI ▸ … (see `ui_create`).
    Ui(crate::ui_create::UiElement),
    /// GameObject ▸ Effects ▸ Particle System.
    ParticleSystem,
    /// GameObject ▸ 3D Object ▸ Terrain (see `terrain_tools`).
    Terrain,
    /// GameObject ▸ 2D Object ▸ Sprite (None) or Sprites ▸ a built-in shape.
    Sprite(Option<&'static str>),
    /// GameObject ▸ Water ▸ Ocean / Lake / River.
    Water(misty_engine::water::WaterKind),
}

impl Create {
    pub fn default_name(self) -> &'static str {
        match self {
            Create::Ui(el) => el.name(),
            Create::Empty => "GameObject",
            Create::Primitive(p) => p.name(),
            Create::Light(LightType::Directional) => "Directional Light",
            Create::Light(LightType::Point) => "Point Light",
            Create::Light(LightType::Spot) => "Spot Light",
            Create::Camera => "Camera",
            Create::AudioSource => "Audio Source",
            Create::ParticleSystem => "Particle System",
            Create::Terrain => "Terrain",
            Create::Sprite(None) => "New Sprite",
            Create::Sprite(Some(shape)) => shape,
            Create::Water(kind) => kind.label(),
        }
    }
}

/// What Assets ▸ Create makes in the Project window's current folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Folder,
    Material,
    Script,
    Scene,
    Animation,
    AnimatorController,
    AudioMixer,
    /// Shader ▸ Surface Shader (lit, from the template).
    SurfaceShader,
    /// Shader ▸ Unlit Shader.
    UnlitShader,
    /// A material using the shader selected in the Project window.
    MaterialFromShader,
}

impl AssetKind {
    pub const ALL: [AssetKind; 7] = [
        AssetKind::Folder,
        AssetKind::Material,
        AssetKind::Script,
        AssetKind::Scene,
        AssetKind::Animation,
        AssetKind::AnimatorController,
        AssetKind::AudioMixer,
    ];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Folder => "Folder",
            AssetKind::Material => "Material",
            AssetKind::Script => "Lua Script",
            AssetKind::Scene => "Scene",
            AssetKind::Animation => "Animation",
            AssetKind::AnimatorController => "Animator Controller",
            AssetKind::AudioMixer => "Audio Mixer",
            AssetKind::SurfaceShader => "Surface Shader",
            AssetKind::UnlitShader => "Unlit Shader",
            AssetKind::MaterialFromShader => "Material from Shader",
        }
    }
}

/// One editor command.
#[derive(Clone, Debug)]
pub enum Action {
    FocusTab(Tab),
    /// Edit ▸ Project Settings.
    OpenProjectSettings,
    /// File ▸ Build Settings.
    OpenBuildSettings,
    /// Build into a folder picked now.
    Build,
    /// Build into the last build folder (asks the first time), then run it.
    BuildAndRun,
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
    /// Edit ▸ Copy / Cut of the selected objects (deep).
    CopyObjects,
    CutObjects,
    /// Edit ▸ Paste: from clipboard `text` (the OS paste event) or the editor's own copy.
    PasteObjects {
        text: Option<String>,
        as_child: bool,
    },
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
    /// Edit ▸ Grid and Snap window.
    OpenGridAndSnap,
    /// Round the selection's world positions to the grid.
    AlignToGrid,
    /// Edit ▸ Lock View to Selected (Shift+F): the Scene camera follows the selection.
    ToggleLockView,
    AlignWithView,
    AlignViewToSelected,
    Undo,
    Redo,
    /// Add a component (registry name) to every selected object.
    AddComponent(&'static str),
    /// Component context menu (see `component_ops`); list commands act on every Inspector target.
    RemoveComponent {
        entities: Vec<Entity>,
        name: String,
    },
    ResetComponent {
        entities: Vec<Entity>,
        name: String,
    },
    MoveComponent {
        entity: Entity,
        name: String,
        dir: i32,
    },
    CopyComponent {
        entity: Entity,
        name: String,
    },
    PasteComponentValues {
        entities: Vec<Entity>,
        name: String,
    },
    PasteComponentAsNew {
        entities: Vec<Entity>,
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
    /// Save an object as a prefab asset in `folder` and link it (Hierarchy → Project drag).
    CreatePrefab {
        entity: Entity,
        folder: std::path::PathBuf,
    },
    /// Instantiate a prefab asset under `parent`, or at the Scene view pivot.
    InstantiatePrefab {
        path: String,
        parent: Option<Entity>,
    },
    /// Instantiate a model file (one object per node) under `parent`, or at the Scene view pivot.
    InstantiateModel {
        path: String,
        parent: Option<Entity>,
    },
    /// Instance root actions (Inspector Prefab row, Hierarchy menu).
    SelectPrefabAsset(Entity),
    RevertPrefab(Entity),
    ApplyPrefab(Entity),
    UnpackPrefab(Entity),
    /// Project window (see `asset_actions`); paths are absolute.
    CreateAsset(AssetKind),
    RenameAsset {
        path: std::path::PathBuf,
        name: String,
    },
    MoveAsset {
        path: std::path::PathBuf,
        folder: std::path::PathBuf,
    },
    DuplicateAsset(std::path::PathBuf),
    /// Move to the OS trash (after the confirm dialog).
    DeleteAsset(std::path::PathBuf),
    ShowInExplorer(std::path::PathBuf),
    /// Assets ▸ Refresh (Ctrl+R): re-scan the project.
    Refresh,
    /// Component > Add... (opens the Inspector's search popup).
    OpenAddComponent,
    TogglePlay,
    TogglePause,
    Step,
    /// Menu item that exists for layout parity but is not built yet.
    NotImplemented(&'static str),
}
