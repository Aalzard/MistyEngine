//! `EventSystem.current`, `Selectable.Select()` and `OnSelect` / `OnDeselect` from Lua, with
//! gamepad navigation moving the selection.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::input::Joystick;
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    Canvas, CanvasScaler, EventSystem, RectTransform, Scene, ScriptRef, Scripts, UiButton, UiImage,
    UiScaleMode,
};
use crate::scripting::{Input, ScriptHost};

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_event_system_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, src) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    root
}

fn script(scene: &mut Scene, e: Entity, file: &str) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                ..Default::default()
            }],
        },
    );
}

/// A canvas with buttons "Play" (top) and "Quit" (below it), each with the Logger script.
fn menu() -> (Scene, Entity, Entity) {
    let mut s = Scene::new("Menu");
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let mut button = |name: &str, y: f32| {
        let b = s.spawn(name, Some(canvas));
        let mut rt = RectTransform::sized(200.0, 50.0);
        rt.anchored_position = Vec2::new(0.0, y);
        s.add(b, rt);
        s.add(b, UiImage::default());
        s.add(b, UiButton::default());
        script(&mut s, b, "Logger.lua");
        b
    };
    let play = button("Play", 40.0);
    let quit = button("Quit", -40.0);
    (s, play, quit)
}

const LOGGER: &str = r#"
local L = {}
function L:OnSelect() Debug.Log("select " .. self.gameObject.name) end
function L:OnDeselect() Debug.Log("deselect " .. self.gameObject.name) end
return L
"#;

struct Rig {
    scene: Scene,
    host: ScriptHost,
    time: Time,
}

impl Rig {
    fn new(mut scene: Scene, root: PathBuf) -> Self {
        let physics = Physics::build(&scene, Vec3::ZERO);
        let host = ScriptHost::start(&mut scene, physics, root);
        Self {
            scene,
            host,
            time: Time::default(),
        }
    }

    /// One frame; `dpad` is the first gamepad's d-pad (y up), `south` its South button.
    fn frame(&mut self, dpad: Vec2, south: bool) -> Vec<String> {
        self.time.advance(1.0 / 60.0);
        let mut pad = Joystick {
            name: "Pad".into(),
            ..Default::default()
        };
        pad.axes[5] = dpad.x;
        pad.axes[6] = dpad.y;
        pad.held[0] = south;
        pad.pressed[0] = south;
        let input = Input {
            screen_size: Vec2::new(800.0, 600.0),
            joysticks: vec![pad],
            ..Default::default()
        };
        self.host.update(&mut self.scene, &self.time, input);
        self.host.take_logs().into_iter().map(|(_, t)| t).collect()
    }

    fn selected(&self) -> Option<Entity> {
        self.host.ui_interaction().selected
    }
}

#[test]
fn set_selected_game_object_selects_and_scripts_hear_on_select() {
    let runner_src = r#"
local R = {}
function R:Start()
    local es = EventSystem.current
    Debug.Log("before " .. tostring(es.currentSelectedGameObject))
    es:SetSelectedGameObject(GameObject.Find("Quit"))
    Debug.Log("now " .. es.currentSelectedGameObject.name)
end
function R:Update()
    self.n = (self.n or 0) + 1
    if self.n == 4 then GameObject.Find("Quit"):GetComponent("Button"):Select() end
    if self.n == 5 then EventSystem.current:SetSelectedGameObject(nil) end
end
return R
"#;
    let root = project(&[("Runner.lua", runner_src), ("Logger.lua", LOGGER)]);
    let (mut scene, play, quit) = menu();
    let runner = scene.spawn("Runner", None);
    script(&mut scene, runner, "Runner.lua");
    let mut rig = Rig::new(scene, root);
    // Start runs before the first UI pass, so the selection is there on the first frame.
    let logs = rig.frame(Vec2::ZERO, false);
    assert_eq!(logs, ["before nil", "now Quit", "select Quit"]);
    assert_eq!(rig.selected(), Some(quit));
    // Gamepad d-pad up moves to Play.
    assert_eq!(
        rig.frame(Vec2::new(0.0, 1.0), false),
        ["deselect Quit", "select Play"]
    );
    assert_eq!(rig.selected(), Some(play));
    rig.frame(Vec2::ZERO, false);
    // Frame 4's Update calls Quit's Button:Select(), frame 5's clears the selection.
    assert!(rig.frame(Vec2::ZERO, false).is_empty());
    assert_eq!(
        rig.frame(Vec2::ZERO, false),
        ["deselect Play", "select Quit"]
    );
    assert_eq!(rig.frame(Vec2::ZERO, false), ["deselect Quit"]);
    assert_eq!(rig.selected(), None);
}

#[test]
fn first_gamepad_move_enters_a_menu_without_first_selected() {
    let root = project(&[("Logger.lua", LOGGER)]);
    let (mut scene, play, quit) = menu();
    let es = scene.spawn("EventSystem", None);
    scene.add(es, EventSystem::default());
    let mut rig = Rig::new(scene, root);
    rig.frame(Vec2::ZERO, false);
    assert_eq!(rig.selected(), None);
    // Any direction picks the top-left button first; the next move navigates from it.
    assert_eq!(rig.frame(Vec2::new(0.0, -1.0), false), ["select Play"]);
    rig.frame(Vec2::ZERO, false);
    rig.frame(Vec2::new(0.0, -1.0), false);
    assert_eq!(rig.selected(), Some(quit));
    let _ = play;
}

#[test]
fn first_selected_and_send_navigation_events_from_lua() {
    let runner_src = r#"
local R = {}
function R:Start()
    local es = EventSystem.current
    es.firstSelectedGameObject = GameObject.Find("Quit")
    Debug.Log(es.firstSelectedGameObject.name, tostring(es.sendNavigationEvents))
    es.sendNavigationEvents = false
    Debug.Log(tostring(es.sendNavigationEvents), es.gameObject.name)
end
return R
"#;
    let root = project(&[("Runner.lua", runner_src), ("Logger.lua", LOGGER)]);
    let (mut scene, _play, quit) = menu();
    let es = scene.spawn("EventSystem", None);
    scene.add(es, EventSystem::default());
    let runner = scene.spawn("Runner", None);
    script(&mut scene, runner, "Runner.lua");
    let mut rig = Rig::new(scene, root);
    assert_eq!(
        rig.frame(Vec2::ZERO, false),
        ["Quit true", "false EventSystem", "select Quit"]
    );
    // Navigation is off: the d-pad doesn't move the selection.
    rig.frame(Vec2::new(0.0, 1.0), false);
    assert_eq!(rig.selected(), Some(quit));
    let es_now = rig.scene.world.get::<&EventSystem>(es).unwrap().clone();
    assert!(!es_now.send_navigation_events);
}
