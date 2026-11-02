//! Editor-wide state shared by every panel.

use std::collections::HashSet;

use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera, Light, LightType, MeshFilter, Scene};

use crate::actions::Action;
use crate::console::Console;
use crate::project::ProjectBrowser;
use crate::scene_camera::SceneCamera;
use crate::selection::Selection;
use crate::undo::UndoStack;

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

/// Icon for an object, based on its components.
pub fn object_icon(scene: &Scene, e: Entity) -> &'static str {
    let w = &scene.world;
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
    pub skybox: bool,
    pub search: String,
    pub camera: SceneCamera,
    /// Screen rect of the 3D view last frame.
    pub view_rect: egui::Rect,
}

/// Game view toolbar settings.
pub struct GameViewSettings {
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
    pub scene: Scene,
    pub scene_dirty: bool,
    pub selection: Selection,
    pub undo: UndoStack,
    pub hierarchy: HierarchyState,
    pub inspector_locked: bool,
    /// Add Component popup is open.
    pub inspector_add_open: bool,
    /// Object the Inspector shows (sticks while locked).
    pub inspector_target: Option<Entity>,
    pub project: ProjectBrowser,
    pub scene_view: SceneViewSettings,
    pub game_view: GameViewSettings,
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
        Self {
            console,
            tool: Tool::Move,
            pivot_center: false,
            global_space: true,
            play: PlayState::Edit,
            scene: Scene::default_3d("SampleScene"),
            scene_dirty: false,
            selection: Selection::default(),
            undo: UndoStack::default(),
            hierarchy: HierarchyState::default(),
            inspector_locked: false,
            inspector_add_open: false,
            inspector_target: None,
            project,
            scene_view: SceneViewSettings {
                draw_mode: 0,
                is_2d: false,
                lighting: true,
                audio: false,
                gizmos: true,
                grid: true,
                skybox: true,
                search: String::new(),
                camera: SceneCamera::default(),
                view_rect: egui::Rect::NOTHING,
            },
            game_view: GameViewSettings {
                aspect: 0,
                scale: 1.0,
                play_mode_target: 0,
                mute: false,
                stats: false,
                gizmos: false,
            },
            actions: Vec::new(),
        }
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

    /// Select one object and reveal it in the Hierarchy.
    pub fn select(&mut self, e: Entity) {
        self.selection.set(e);
        self.reveal(e);
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
