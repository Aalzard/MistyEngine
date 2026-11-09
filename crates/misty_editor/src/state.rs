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

    /// The designer's icon for the tool, when there is one.
    pub fn brand_icon(self) -> Option<crate::branding::icons::Icon> {
        use crate::branding::icons::Icon;
        match self {
            Tool::Hand => Some(Icon::Hand),
            Tool::Move => Some(Icon::Move),
            Tool::Rotate => Some(Icon::Rotate),
            Tool::Scale => Some(Icon::Scale),
            Tool::Rect | Tool::Transform => None,
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
    /// Scene headers folded closed.
    pub collapsed_scenes: HashSet<misty_engine::scene::SceneId>,
    /// Alt+right-click on a folder row: the pastel colour wheel for that folder, and where.
    pub folder_picker: Option<(Entity, egui::Pos2)>,
}

/// Icon shown for prefab assets and instance roots (drawn blue; plain meshes use the same grey).
pub const PREFAB_ICON: &str = "⬛";
pub const AUDIO_ICON: &str = "🔊";
pub const PARTICLE_ICON: &str = "✨";

/// Icon for an object, based on its components (instance roots show the prefab icon).
/// The designer's icon for an object, where one exists (Hierarchy, Inspector, Scene view);
/// [`object_icon`]'s glyph otherwise. Prefab instances keep the prefab glyph.
pub fn object_brand_icon(scene: &Scene, e: Entity) -> Option<crate::branding::icons::Icon> {
    use crate::branding::icons::Icon;
    let w = &scene.world;
    if w.get::<&misty_engine::scene::PrefabLink>(e).is_ok() {
        return None;
    }
    if w.get::<&misty_engine::scene::HierarchyFolder>(e).is_ok() {
        return Some(Icon::Folder);
    }
    if let Ok(light) = w.get::<&Light>(e) {
        return (light.kind == LightType::Directional).then_some(Icon::SunLight);
    }
    if w.get::<&Camera>(e).is_ok() {
        return Some(Icon::Camera);
    }
    if w.get::<&misty_engine::scene::CharacterController>(e)
        .is_ok()
    {
        return Some(Icon::CharacterController);
    }
    if w.get::<&misty_engine::scene::PostProcessVolume>(e).is_ok() {
        return Some(Icon::SkyVolume);
    }
    None
}

pub fn object_icon(scene: &Scene, e: Entity) -> &'static str {
    let w = &scene.world;
    if w.get::<&misty_engine::scene::PrefabLink>(e).is_ok() {
        return PREFAB_ICON;
    }
    if w.get::<&misty_engine::scene::HierarchyFolder>(e).is_ok() {
        return "📁";
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
    if w.get::<&misty_engine::scene::FogVolume>(e).is_ok() {
        return "☁";
    }
    if w.get::<&misty_engine::scene::DecalProjector>(e).is_ok() {
        return "▣";
    }
    if w.get::<&misty_engine::scene::Terrain>(e).is_ok() {
        return "⛰";
    }
    if w.get::<&misty_engine::scene::ReflectionProbe>(e).is_ok() {
        return "◍";
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
    /// Icon gizmo size and tint (Gizmos ▾).
    pub icons: crate::scene_icons::IconSettings,
    /// One-shot: open the Gizmos ▾ dropdown next frame (screenshot hook).
    pub open_icon_menu: bool,
    /// One-shot: open the Quality dropdown next frame (screenshot hook).
    pub open_quality_menu: bool,
    pub grid: bool,
    /// The effects toggle (☁): skybox, fog, clouds and post-processing as picked in `effects`.
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
    /// Volumetric clouds of the Post-process Volume.
    pub clouds: bool,
    /// Light halos and lens flares (also need Post Processing and Lighting).
    pub glows: bool,
    /// Screen Space Global Illumination and Reflections (also need Post Processing).
    pub screen_space: bool,
    pub post_processing: bool,
}

impl Default for SceneEffects {
    fn default() -> Self {
        Self {
            skybox: true,
            fog: true,
            clouds: true,
            glows: true,
            screen_space: true,
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
    /// Frame times and hitches shown by the Stats overlay (kept while it is on).
    pub frame_history: misty_engine::render::FrameHistory,
    pub gizmos: bool,
    /// The VSync preference as saved (`None` = not read yet); the window's present mode
    /// follows it (`app::apply_vsync`).
    pub vsync: Option<bool>,
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
    /// Draw lists of the Game and Scene views, kept between frames (only changes rebuild).
    pub game_draws: misty_engine::draw_cache::DrawCache,
    pub scene_draws: misty_engine::draw_cache::DrawCache,
    /// Inspector preview of an audio clip (device opened on first ▶).
    pub clip_preview: Option<misty_engine::audio::Audio>,
    /// Scene view Particle Effect overlay settings.
    pub particle_preview: crate::particle_preview::ParticlePreview,
    /// Terrain Inspector tool, brush settings and the stroke in progress.
    pub terrain_tools: crate::terrain_tools::TerrainTools,
    /// Mesh Builder edit mode, element selection and operation settings.
    pub mesh_builder: crate::mesh_builder::MeshBuilder,
    /// Tile Palette window: palette, brush, tool and the stroke in progress.
    pub tile_palette: crate::tile_palette::TilePaletteState,
    /// Navigation window settings and the bake in progress.
    pub navigation: crate::panels::navigation::NavigationWindow,
    pub occlusion: crate::panels::occlusion::OcclusionWindow,
    /// Asset Groups window: addressables settings, last build / analysis.
    pub asset_groups: crate::panels::asset_groups::AssetGroupsWindow,
    /// Package Manager window: packages, manifest, lock, registry.
    pub packages: crate::packages::PackagesWindow,
    /// Reflection probe generator settings, the bake in progress and Realtime probes.
    pub probes: crate::probe_tools::ProbeTools,
    /// Lightmap bakes (Lighting window ▸ Generate Lighting).
    pub lightmapper: crate::lightmap_tools::LightmapTools,
    /// Light probe editing (Edit Light Probes) and the bake in progress.
    pub light_probes: crate::light_probe_tools::LightProbeTools,
    /// World Builder window state and the generation in progress.
    pub world_builder: crate::panels::world_builder::WorldBuilderWindow,
    /// World Partition window: Edit-mode streaming, gizmo and overlay toggles.
    pub world_partition: crate::panels::world_partition::WorldPartitionWindow,
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
    /// In-app updater (off until the editor app starts it).
    pub updater: crate::updater::Updater,
    pub selection: Selection,
    pub undo: UndoStack,
    /// Undo History window (search text).
    pub undo_history: crate::panels::undo_history::UndoHistoryWindow,
    /// Animation clips being edited (written back to their files after each edit).
    pub anim_clips: crate::anim_clips::ClipStore,
    /// Animation window: clip, playhead, preview and Record mode.
    pub animation: crate::panels::animation::AnimationWindow,
    /// Animator window: controller graph, selection and view.
    pub animator: crate::panels::animator::AnimatorWindow,
    /// Timeline window: the director being edited, playhead, preview and selection.
    pub timeline: crate::panels::timeline::TimelineWindow,
    /// Shader Graph window: the graph being edited, its undo and view.
    pub shader_graph: crate::panels::shader_graph::ShaderGraphWindow,
    /// Effect Graph window: the effect being edited, its undo and view.
    pub effect_graph: crate::panels::effect_graph::EffectGraphWindow,
    /// The Script Graph window (visual scripting).
    pub script_graph: crate::panels::script_graph::ScriptGraphWindow,
    /// Audio Mixer window: shown mixer, snapshot, selected group/effect, meters.
    pub audio_mixer: crate::panels::audio_mixer::MixerWindow,
    /// Input Actions window: the asset being edited.
    pub input_actions: crate::panels::input_actions::InputActionsWindow,
    /// Code Editor window: open scripts, find state, preferences.
    pub code_editor: crate::panels::code_editor::CodeEditor,
    pub hierarchy: HierarchyState,
    pub handles: HandleState,
    /// Scene-view spline editing: knot selection and the Draw Spline tool.
    pub splines: crate::spline_tools::SplineEdit,
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
    /// The open Select Preset window.
    pub preset_picker: Option<crate::presets::PresetPicker>,
    /// Open the picker of this component or asset kind when its button is next drawn
    /// (`MISTY_PRESET_PICKER` screenshots).
    pub preset_picker_request: Option<String>,
    /// Help ▸ About Misty Engine window.
    pub about_open: bool,
    /// Scale fields keep their proportions.
    pub scale_locked: bool,
    /// Edit ▸ Grid and Snap values (saved in the project's Library).
    pub snap: crate::snap::SnapSettings,
    /// Grid and Snap window is open.
    pub snap_window_open: bool,
    /// Tools ▸ Rendering ▸ Convert … to Virtual Geometry and the build progress.
    pub vgeo: crate::vgeo_convert::VgeoTools,
    /// Save Data and Localization Tables windows.
    pub saveloc: crate::saveloc::SaveLocWindows,
    /// Window ▸ Multiplayer ▸ Multiplayer Play Mode and the player windows it opened.
    pub multiplayer: crate::multiplayer::MultiplayerPlayMode,
    pub project: ProjectBrowser,
    /// Edit ▸ Project Settings values (saved in the project).
    pub project_settings: ProjectSettings,
    pub project_settings_window: ProjectSettingsWindow,
    /// File ▸ Build Settings values (saved in the project).
    pub build_settings: BuildSettings,
    /// File ▸ Build Profiles: the platform and custom profiles (saved as assets).
    pub build_profiles: misty_engine::build_profile::BuildProfiles,
    pub build_window: BuildSettingsWindow,
    pub assets: AssetDatabase,
    /// Asset shown in the Inspector when no scene object is selected.
    pub inspector_asset: Option<String>,
    pub scene_view: SceneViewSettings,
    pub game_view: GameViewSettings,
    /// Window ▸ Analysis ▸ Profiler state.
    pub profiler: crate::panels::profiler::ProfilerWindow,
    /// Window ▸ Analysis ▸ Frame Debugger state (the captured frame and selected event).
    pub frame_debugger: crate::panels::frame_debugger::FrameDebuggerWindow,
    /// Window ▸ Analysis ▸ Memory Profiler snapshots.
    pub memory_profiler: crate::panels::memory_profiler::MemoryProfilerWindow,
    /// Window ▸ General ▸ Test Runner: test list, results and the run in progress.
    pub test_runner: crate::panels::test_runner::TestRunnerWindow,
    /// Saved window layouts and the Save Layout prompt.
    pub layouts: crate::layouts::Layouts,
    /// Ctrl+K search palette, its asset index and recent searches.
    pub search: crate::search::SearchPalette,
    /// Help ▸ Tutorials: the running tutorial and finished ones.
    pub tutorials: crate::tutorials::Tutorials,
    /// Scenes and assets reloading when their files change; dev-build restarts.
    pub live: crate::live_reload::LiveReload,
    /// Editor scripts (`Editor` folders): menu items, tool windows, Inspectors, gizmos.
    pub editor_scripts: crate::editor_scripts::EditorScripts,
    /// Prefab Mode levels, outermost first (empty: editing the scenes).
    pub prefab_stages: Vec<crate::prefab_mode::PrefabStage>,
    /// AI tools (MCP) in the running editor: the loopback bridge and requests in flight.
    pub mcp: crate::mcp::live::Bridge,
    /// Git status badges and the Version Control window.
    pub vcs: crate::vcs::Vcs,
    /// Edit ▸ Preferences ▸ UI Scale (the editor app loads the saved value).
    pub ui_scale: crate::a11y::UiScale,
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
        let root = project.root.clone();
        let mut s = Self {
            console,
            tool: Tool::Move,
            pivot_center: false,
            global_space: true,
            play: PlayState::Edit,
            play_session: None,
            time: Time::default(),
            runtime: None,
            game_draws: Default::default(),
            scene_draws: Default::default(),
            clip_preview: None,
            particle_preview: Default::default(),
            terrain_tools: Default::default(),
            mesh_builder: Default::default(),
            tile_palette: Default::default(),
            navigation: Default::default(),
            occlusion: Default::default(),
            asset_groups: Default::default(),
            packages: Default::default(),
            probes: Default::default(),
            lightmapper: Default::default(),
            light_probes: Default::default(),
            world_builder: Default::default(),
            world_partition: Default::default(),
            game_input: Input::default(),
            pause_on_play: false,
            step_requested: false,
            scene: Scene::default_3d("SampleScene"),
            scene_dirty: false,
            files: SceneFiles::default(),
            updater: crate::updater::Updater::disabled(),
            selection: Selection::default(),
            undo: UndoStack::default(),
            undo_history: Default::default(),
            anim_clips: crate::anim_clips::ClipStore::new(project.root.clone()),
            animation: Default::default(),
            animator: Default::default(),
            timeline: Default::default(),
            shader_graph: Default::default(),
            effect_graph: Default::default(),
            script_graph: Default::default(),
            audio_mixer: Default::default(),
            input_actions: Default::default(),
            code_editor: crate::panels::code_editor::CodeEditor::new(&project.root),
            hierarchy: HierarchyState::default(),
            handles: HandleState::default(),
            splines: Default::default(),
            inspector_locked: false,
            inspector_add_open: false,
            inspector_targets: Vec::new(),
            component_clipboard: None,
            preset_picker: None,
            preset_picker_request: None,
            about_open: false,
            object_clipboard: None,
            os_clipboard_out: None,
            scale_locked: false,
            snap: crate::snap::SnapSettings::load(&project.root),
            snap_window_open: false,
            vgeo: Default::default(),
            saveloc: Default::default(),
            multiplayer: Default::default(),
            assets: AssetDatabase::open(&project.root),
            inspector_asset: None,
            project,
            project_settings: ProjectSettings::default(),
            project_settings_window: ProjectSettingsWindow::default(),
            build_settings: BuildSettings::default(),
            build_profiles: Default::default(),
            build_window: BuildSettingsWindow::default(),
            scene_view: SceneViewSettings {
                draw_mode: 0,
                is_2d: false,
                lighting: true,
                audio: false,
                gizmos: true,
                icons: Default::default(),
                open_icon_menu: false,
                open_quality_menu: false,
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
                frame_history: Default::default(),
                gizmos: false,
                vsync: None,
            },
            profiler: Default::default(),
            frame_debugger: Default::default(),
            memory_profiler: Default::default(),
            test_runner: Default::default(),
            layouts: Default::default(),
            search: crate::search::SearchPalette::new(&root),
            tutorials: Default::default(),
            live: Default::default(),
            editor_scripts: crate::editor_scripts::EditorScripts::new(&root),
            prefab_stages: Vec::new(),
            mcp: Default::default(),
            vcs: Default::default(),
            ui_scale: Default::default(),
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

    /// Note a continuous edit (Inspector drag, typing) for grouped undo; `label` names the
    /// step ("Change Light Intensity"). The first label of an interaction wins.
    pub fn mark_edited(&mut self, label: &str) {
        self.undo.mark_edited(label);
        self.scene_dirty = true;
    }

    /// "Cube" or "3 Objects": the selection, for step names like "Delete Cube".
    pub fn selection_label(&self) -> String {
        crate::undo_names::objects(&self.scene, &self.selection)
    }

    /// Where undo and redo write back.
    fn undo_targets(&mut self) -> (&mut UndoStack, crate::undo::Restore<'_>) {
        let restore = crate::undo::Restore {
            scene: &mut self.scene,
            selection: &mut self.selection,
            clips: &mut self.anim_clips,
            assets: &mut self.assets,
            project_settings: &mut self.project_settings,
        };
        (&mut self.undo, restore)
    }

    /// Undo one step; returns its name.
    pub fn undo_step(&mut self) -> Option<String> {
        let (undo, mut to) = self.undo_targets();
        undo.undo(&mut to)
    }

    /// Redo one step; returns its name.
    pub fn redo_step(&mut self) -> Option<String> {
        let (undo, mut to) = self.undo_targets();
        undo.redo(&mut to)
    }

    /// Undo/redo until `position` steps of the history are applied; returns steps moved.
    pub fn undo_jump(&mut self, position: usize) -> usize {
        let (undo, mut to) = self.undo_targets();
        undo.jump(position, &mut to)
    }

    /// Re-scan the Project window and the asset database (Assets ▸ Refresh, Ctrl+R).
    pub fn refresh_assets(&mut self) {
        self.project.refresh();
        self.assets.refresh();
        self.search.index.invalidate();
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
