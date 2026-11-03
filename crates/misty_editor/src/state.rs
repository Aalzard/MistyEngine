//! Editor-wide state shared by every panel.

use std::collections::HashSet;

use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera, Light, LightType, MeshFilter, Scene};

use crate::actions::Action;
use crate::assets::AssetDatabase;
use crate::build_settings::BuildSettingsWindow;
use crate::console::Console;
use crate::handles::HandleState;
use crate::play::PlaySession;
use crate::project::ProjectBrowser;
use crate::project_settings::ProjectSettingsWindow;
use crate::scene_camera::SceneCamera;
use crate::scene_files::SceneFiles;
use crate::selection::Selection;
use crate::undo::UndoStack;
use misty_engine::build::BuildSettings;
use misty_engine::runtime::{Runtime, Time};
use misty_engine::scripting::Input;
use misty_engine::settings::ProjectSettings;

/// Active transform tool (Q W E R T Y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Hand,
    Move,
    Rotate,
    Scale,
    Rect,
    Transform,
}

impl Tool {
    pub const ALL: [Tool; 6] = [
        Tool::Hand,
        Tool::Move,
        Tool::Rotate,
        Tool::Scale,
        Tool::Rect,
        Tool::Transform,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Hand => "✋",
            Tool::Move => "✥",
            Tool::Rotate => "⟲",
            Tool::Scale => "⤡",
            Tool::Rect => "▭",
            Tool::Transform => "⊕",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Tool::Hand => "View Tool (Q)",
            Tool::Move => "Move Tool (W)",
            Tool::Rotate => "Rotate Tool (E)",
            Tool::Scale => "Scale Tool (R)",
            Tool::Rect => "Rect Tool (T)",
            Tool::Transform => "Transform Tool (Y)",
        }
    }

    pub fn key(self) -> egui::Key {
        match self {
            Tool::Hand => egui::Key::Q,
            Tool::Move => egui::Key::W,
            Tool::Rotate => egui::Key::E,
            Tool::Scale => egui::Key::R,
            Tool::Rect => egui::Key::T,
            Tool::Transform => egui::Key::Y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Edit,
    Playing,
    Paused,
}

impl PlayState {
    pub fn in_play_mode(self) -> bool {
        self != PlayState::Edit
    }
}

/// Hierarchy panel UI state.
#[derive(Default)]
pub struct HierarchyState {
    pub search: String,
    pub expanded: HashSet<Entity>,
    /// Object being renamed inline and the text typed so far.
    pub renaming: Option<(Entity, String)>,
    /// Keyboard focus: last click landed in the Hierarchy.
    pub focused: bool,
}

/// Icon shown for prefab assets and instance roots (drawn blue; plain meshes use the same grey).
pub const PREFAB_ICON: &str = "⬛";
pub const AUDIO_ICON: &str = "🔊";
pub const PARTICLE_ICON: &str = "✨";

/// Icon for an object, based on its components (instance roots show the prefab icon).
pub fn object_icon(scene: &Scene, e: Entity) -> &'static str {
    let w = &scene.world;
    if w.get::<&misty_engine::scene::PrefabLink>(e).is_ok() {
        return PREFAB_ICON;
    }
    if let Ok(light) = w.get::<&Light>(e) {
        return if light.kind == LightType::Directional {
            "☀"
        } else {
            "💡"
        };
    }
    if w.get::<&Camera>(e).is_ok() {
        return "🎥";
    }
    if w.get::<&misty_engine::scene::AudioSource>(e).is_ok() {
        return AUDIO_ICON;
    }
    if w.get::<&misty_engine::scene::ParticleSystem>(e).is_ok() {
        return PARTICLE_ICON;
    }
    if w.get::<&misty_engine::scene::Terrain>(e).is_ok() {
        return "⛰";
    }
    if w.get::<&MeshFilter>(e).is_ok_and(|m| m.mesh.is_some()) {
        return "⬛";
    }
    "⬚"
}

/// Scene view toolbar toggles.
pub struct SceneViewSettings {
    pub draw_mode: usize,
    pub is_2d: bool,
    pub lighting: bool,
    pub audio: bool,
    pub gizmos: bool,
    pub grid: bool,
    /// The effects toggle (☁): skybox, fog and post-processing as picked in `effects`.
    pub skybox: bool,
    /// Lock View to Selected: the camera pivot follows the selection.
    pub lock_to_selection: bool,
    pub effects: SceneEffects,
    pub search: String,
    pub camera: SceneCamera,
    /// Screen rect of the 3D view last frame.
    pub view_rect: egui::Rect,
}

/// Scene view effects dropdown (what the ☁ toggle turns on).
#[derive(Clone, Copy, Debug)]
pub struct SceneEffects {
    pub skybox: bool,
    pub fog: bool,
    pub post_processing: bool,
}

impl Default for SceneEffects {
    fn default() -> Self {
        Self {
            skybox: true,
            fog: true,
            post_processing: true,
        }
    }
}

/// Game view toolbar settings.
pub struct GameViewSettings {
    /// Keyboard/mouse go to scripts (click the Game view to focus it).
    pub focused: bool,
    /// Screen rect of the game image last frame.
    pub view_rect: egui::Rect,
    /// Game image size in pixels last frame (UI layout in the Scene view and Inspector).
    pub screen_px: glam::Vec2,
    pub aspect: usize,
    pub scale: f32,
    pub play_mode_target: usize,
    pub mute: bool,
    pub stats: bool,
    pub gizmos: bool,
}

pub struct EditorState {
    pub console: Console,
    pub tool: Tool,
    /// false = Pivot, true = Center.
    pub pivot_center: bool,
    /// false = Local, true = Global.
    pub global_space: bool,
    pub play: PlayState,
    /// Edit-mode snapshot while in Play mode.
    pub play_session: Option<PlaySession>,
    pub time: Time,
    /// Physics/scripts state while playing.
    pub runtime: Option<Runtime>,
    /// Inspector preview of an audio clip (device opened on first ▶).
    pub clip_preview: Option<misty_engine::audio::Audio>,
    /// Scene view Particle Effect overlay settings.
    pub particle_preview: crate::particle_preview::ParticlePreview,
    /// Terrain Inspector tool, brush settings and the stroke in progress.
    pub terrain_tools: crate::terrain_tools::TerrainTools,
    /// Keyboard/mouse for scripts this frame (filled while the Game view has focus).
    pub game_input: Input,
    /// Pause toggled before Play: Play starts paused.
    pub pause_on_play: bool,
    /// Step pressed while paused.
    pub step_requested: bool,
    pub scene: Scene,
    pub scene_dirty: bool,
    /// Open scene file, recent scenes, save prompt.
    pub files: SceneFiles,
    pub selection: Selection,
    pub undo: UndoStack,
    /// Animation clips being edited (written back to their files after each edit).
    pub anim_clips: crate::anim_clips::ClipStore,
    /// Animation window: clip, playhead, preview and Record mode.
    pub animation: crate::panels::animation::AnimationWindow,
    /// Animator window: controller graph, selection and view.
    pub animator: crate::panels::animator::AnimatorWindow,
    /// Audio Mixer window: shown mixer, snapshot, selected group/effect, meters.
    pub audio_mixer: crate::panels::audio_mixer::MixerWindow,
    pub hierarchy: HierarchyState,
    pub handles: HandleState,
    pub inspector_locked: bool,
    /// Add Component popup is open.
    pub inspector_add_open: bool,
    /// Objects the Inspector edits, active first (sticks while locked).
    pub inspector_targets: Vec<Entity>,
    /// Copied GameObjects (Edit ▸ Copy / Cut).
    pub object_clipboard: Option<crate::object_clipboard::ObjectClipboard>,
    /// Text to put on the system clipboard this frame.
    pub os_clipboard_out: Option<String>,
    /// Copy Component clipboard: component name and its values.
    pub component_clipboard: Option<(String, serde_json::Value)>,
    /// Scale fields keep their proportions.
    pub scale_locked: bool,
    /// Edit ▸ Grid and Snap values (saved in the project's Library).
    pub snap: crate::snap::SnapSettings,
    /// Grid and Snap window is open.
    pub snap_window_open: bool,
    pub project: ProjectBrowser,
    /// Edit ▸ Project Settings values (saved in the project).
    pub project_settings: ProjectSettings,
    pub project_settings_window: ProjectSettingsWindow,
    /// File ▸ Build Settings values (saved in the project).
    pub build_settings: BuildSettings,
    pub build_window: BuildSettingsWindow,
    pub assets: AssetDatabase,
    /// Asset shown in the Inspector when no scene object is selected.
    pub inspector_asset: Option<String>,
    pub scene_view: SceneViewSettings,
    pub game_view: GameViewSettings,
    /// Saved window layouts and the Save Layout prompt.
    pub layouts: crate::layouts::Layouts,
    /// Commands to apply after the UI pass.
    pub actions: Vec<Action>,
}

impl EditorState {
    pub fn new(project: ProjectBrowser) -> Self {
        let mut console = Console::default();
        console.info(format!(
            "Misty Engine started. Project: {}",
            project.root.display()
        ));
        let mut s = Self {
            console,
            tool: Tool::Move,
            pivot_center: false,
            global_space: true,
            play: PlayState::Edit,
            play_session: None,
            time: Time::default(),
            runtime: None,
            clip_preview: None,
            particle_preview: Default::default(),
            terrain_tools: Default::default(),
            game_input: Input::default(),
            pause_on_play: false,
            step_requested: false,
            scene: Scene::default_3d("SampleScene"),
            scene_dirty: false,
            files: SceneFiles::default(),
            selection: Selection::default(),
            undo: UndoStack::default(),
            anim_clips: crate::anim_clips::ClipStore::new(project.root.clone()),
            animation: Default::default(),
            animator: Default::default(),
            audio_mixer: Default::default(),
            hierarchy: HierarchyState::default(),
            handles: HandleState::default(),
            inspector_locked: false,
            inspector_add_open: false,
            inspector_targets: Vec::new(),
            component_clipboard: None,
            object_clipboard: None,
            os_clipboard_out: None,
            scale_locked: false,
            snap: crate::snap::SnapSettings::load(&project.root),
            snap_window_open: false,
            assets: AssetDatabase::open(&project.root),
            inspector_asset: None,
            project,
            project_settings: ProjectSettings::default(),
            project_settings_window: ProjectSettingsWindow::default(),
            build_settings: BuildSettings::default(),
            build_window: BuildSettingsWindow::default(),
            scene_view: SceneViewSettings {
                draw_mode: 0,
                is_2d: false,
                lighting: true,
                audio: false,
                gizmos: true,
                grid: true,
                skybox: true,
                lock_to_selection: false,
                effects: SceneEffects::default(),
                search: String::new(),

                camera: SceneCamera::default(),
                view_rect: egui::Rect::NOTHING,
            },
            game_view: GameViewSettings {
                focused: false,
                view_rect: egui::Rect::NOTHING,
                screen_px: glam::Vec2::new(1920.0, 1080.0),
                aspect: 0,
                scale: 1.0,
                play_mode_target: 0,
                mute: false,
                stats: false,
                gizmos: false,
            },
            layouts: Default::default(),
            actions: Vec::new(),
        };
        crate::project_settings::load(&mut s);
        crate::build_settings::load(&mut s);
        s
    }

    pub fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    /// Snapshot for undo before a structural change, and mark the scene dirty.
    pub fn record(&mut self, label: &str) {
        self.undo.push(label, &self.scene, &self.selection);
        self.scene_dirty = true;
    }

    /// Note a continuous edit (Inspector drag, typing) for grouped undo.
    pub fn mark_edited(&mut self, what: &str) {
        self.undo.mark_edited(what);
        self.scene_dirty = true;
    }

    /// Re-scan the Project window and the asset database (Assets ▸ Refresh, Ctrl+R).
    pub fn refresh_assets(&mut self) {
        self.project.refresh();
        self.assets.refresh();
    }

    /// Select one object and reveal it in the Hierarchy.
    pub fn select(&mut self, e: Entity) {
        self.selection.set(e);
        self.reveal(e);
    }

    /// Give `e`'s Mesh Renderer a material (drag-and-drop from the Project window).
    pub fn assign_material(&mut self, e: Entity, rel: &str) {
        if self
            .scene
            .world
            .get::<&misty_engine::scene::MeshRenderer>(e)
            .is_err()
        {
            self.console
                .warn(format!("'{}' has no Mesh Renderer.", self.scene.name(e)));
            return;
        }
        self.record("Assign Material");
        if let Ok(mut r) = self
            .scene
            .world
            .get::<&mut misty_engine::scene::MeshRenderer>(e)
        {
            r.material = Some(rel.to_owned());
        }
    }

    /// Expand all parents of `e`.
    pub fn reveal(&mut self, e: Entity) {
        let mut cur = self.scene.parent(e);
        while let Some(p) = cur {
            self.hierarchy.expanded.insert(p);
            cur = self.scene.parent(p);
        }
    }
}
