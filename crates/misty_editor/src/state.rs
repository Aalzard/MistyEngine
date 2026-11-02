//! Editor-wide state shared by every panel.

use crate::actions::Action;
use crate::console::Console;
use crate::project::ProjectBrowser;

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

/// Stand-in scene object until the ECS lands in step 3.
#[derive(Clone, Debug)]
pub struct PlaceholderObject {
    pub name: String,
    pub icon: &'static str,
    pub active: bool,
    pub is_static: bool,
    pub tag: String,
    pub layer: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl PlaceholderObject {
    pub fn new(name: &str, icon: &'static str, position: [f32; 3], rotation: [f32; 3]) -> Self {
        Self {
            name: name.to_owned(),
            icon,
            active: true,
            is_static: false,
            tag: "Untagged".to_owned(),
            layer: "Default".to_owned(),
            position,
            rotation,
            scale: [1.0; 3],
        }
    }
}

/// Scene view toolbar toggles.
pub struct SceneViewSettings {
    pub draw_mode: usize,
    pub is_2d: bool,
    pub lighting: bool,
    pub audio: bool,
    pub gizmos: bool,
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
    pub scene_name: String,
    pub scene_dirty: bool,
    pub objects: Vec<PlaceholderObject>,
    pub selected: Option<usize>,
    pub hierarchy_search: String,
    pub inspector_locked: bool,
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
            scene_name: "SampleScene".to_owned(),
            scene_dirty: false,
            objects: default_scene(),
            selected: None,
            hierarchy_search: String::new(),
            inspector_locked: false,
            project,
            scene_view: SceneViewSettings {
                draw_mode: 0,
                is_2d: false,
                lighting: true,
                audio: false,
                gizmos: true,
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
}

/// A new scene holds a camera and a directional light.
pub fn default_scene() -> Vec<PlaceholderObject> {
    vec![
        PlaceholderObject::new("Main Camera", "🎥", [0.0, 1.0, -10.0], [0.0; 3]),
        PlaceholderObject::new(
            "Directional Light",
            "☀",
            [0.0, 3.0, 0.0],
            [50.0, -30.0, 0.0],
        ),
    ]
}
