//! Lua UI API tests: Text/Image/Button components, Color, and Button On Click calls.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    ButtonCall, Canvas, CanvasScaler, RectTransform, Scene, ScriptRef, Scripts, UiButton, UiImage,
    UiScaleMode, UiText,
};
use crate::scripting::{Input, LogKind, ScriptHost};

fn project(name: &str, source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_ui_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts").join(name), source).unwrap();
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

/// Canvas (constant pixel size) with a 200×50 button (Image + Button) in the middle and a Text.
fn ui_scene() -> (Scene, Entity, Entity) {
    let mut s = Scene::new("UI");
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let button = s.spawn("Button", Some(canvas));
    s.add(button, RectTransform::sized(200.0, 50.0));
    s.add(button, UiImage::default());
    s.add(button, UiButton::default());
    let label = s.spawn("Label", Some(canvas));
    s.add(
        label,
        RectTransform {
            anchored_position: Vec2::new(0.0, 200.0),
            ..RectTransform::sized(300.0, 40.0)
        },
    );
    s.add(label, UiText::default());
    (s, button, label)
}

fn host(scene: &mut Scene, root: PathBuf) -> ScriptHost {
    let physics = Physics::build(scene, Vec3::ZERO);
    ScriptHost::start(scene, physics, root)
}

#[test]
fn text_image_button_and_color_from_lua() {
    let src = r#"
local T = {}
function T:Start()
    local label = GameObject.Find("Label"):GetComponent("Text")
    label.text = "Hello " .. 42
    label.fontSize = 20
    local b = GameObject.Find("Button")
    local img = b:GetComponent("Image")
    img.color = Color.new(1, 0, 0, 0.5)
    local c = img.color
    Debug.Log(c.r, c.g, c.a, tostring(Color.red == Color.new(1, 0, 0)))
    local button = b:GetComponent("Button")
    Debug.Log(tostring(button.interactable))
    button.interactable = false
    Debug.Log(tostring(self.gameObject:GetComponent("Text")), label.gameObject.name)
end
return T
"#;
    let root = project("T.lua", src);
    let (mut scene, button, label) = ui_scene();
    let runner = scene.spawn("Runner", None);
    script(&mut scene, runner, "T.lua");
    let mut h = host(&mut scene, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    h.update(&mut scene, &time, Input::default());
    let logs: Vec<String> = h.take_logs().into_iter().map(|(_, t)| t).collect();
    assert_eq!(logs, ["1 0 0.5 true", "true", "nil Label"]);
    let t = scene.world.get::<&UiText>(label).unwrap();
    assert_eq!((t.text.as_str(), t.font_size), ("Hello 42", 20.0));
    assert_eq!(
        scene.world.get::<&UiImage>(button).unwrap().color,
        [1.0, 0.0, 0.0, 0.5]
    );
    assert!(!scene.world.get::<&UiButton>(button).unwrap().interactable);
}

#[test]
fn on_click_calls_the_target_method_once_per_click() {
    let src = r#"
local Counter = {}
function Counter:Start() self._n = 0 end
function Counter:Clicked()
    self._n = self._n + 1
    GameObject.Find("Label"):GetComponent("Text").text = "clicks " .. self._n
end
return Counter
"#;
    let root = project("Counter.lua", src);
    let (mut scene, button, label) = ui_scene();
    let counter = scene.spawn("Counter", None);
    script(&mut scene, counter, "Counter.lua");
    scene.world.get::<&mut UiButton>(button).unwrap().on_click = vec![
        ButtonCall {
            target: Some(counter),
            method: "Clicked".into(),
        },
        ButtonCall {
            target: Some(counter),
            method: "Missing".into(),
        },
    ];
    let mut h = host(&mut scene, root);
    let mut time = Time::default();
    let screen = Vec2::new(800.0, 600.0);
    let mut frame = |scene: &mut Scene, pos: Vec2, down: bool, up: bool| {
        time.advance(1.0 / 60.0);
        let input = Input {
            mouse_position: Some(pos),
            screen_size: screen,
            mouse_pressed: [down, false, false],
            mouse_released: [up, false, false],
            ..Default::default()
        };
        h.update(scene, &time, input);
    };
    let centre = Vec2::new(400.0, 300.0);
    frame(&mut scene, centre, false, false);
    frame(&mut scene, centre, true, false);
    let text = |s: &Scene| s.world.get::<&UiText>(label).unwrap().text.clone();
    assert_eq!(text(&scene), "New Text", "no click on press");
    frame(&mut scene, centre, false, true);
    assert_eq!(text(&scene), "clicks 1");
    // Press on the button, release outside: no click.
    frame(&mut scene, centre, true, false);
    frame(&mut scene, Vec2::new(5.0, 5.0), false, true);
    assert_eq!(text(&scene), "clicks 1");
    let warnings: Vec<String> = h
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k == LogKind::Warning)
        .map(|(_, t)| t)
        .collect();
    assert_eq!(
        warnings,
        ["On Click: no script on 'Counter' has a method 'Missing'"]
    );
    assert_eq!(h.ui_interaction().pressed, None);
}

/// On Click and On Value Changed run a Script Graph's Custom Event of the method's name (the
/// value as its first argument), with no "no script has a method" warning.
#[test]
fn on_click_and_on_value_changed_run_script_graph_custom_events() {
    use crate::scene::UiToggle;
    use crate::visual_script::value::Literal;
    use crate::visual_script::{NoFiles, ScriptGraph};
    let root = project("Unused.lua", "return {}\n");
    let mut g = ScriptGraph::default();
    let log = |g: &mut ScriptGraph, x: f32| {
        g.add_node_with(
            "Api",
            &[("owner", "Debug"), ("member", "Log"), ("mode", "call")],
            [x, 0.0],
        )
    };
    let play = g.add_node_with("OnCustomEvent", &[("name", "Play")], [0.0, 0.0]);
    let l1 = log(&mut g, 100.0);
    g.set_value(l1, "Message", Literal::String("play".into()));
    g.connect(play, "Out", l1, "In", &NoFiles).unwrap();
    let mute = g.add_node_with(
        "OnCustomEvent",
        &[("name", "Mute"), ("args", "on")],
        [0.0, 100.0],
    );
    let text = g.add_node("Append", [100.0, 100.0]);
    g.set_value(text, "A", Literal::String("mute ".into()));
    g.connect(mute, "on", text, "B", &NoFiles).unwrap();
    let l2 = log(&mut g, 200.0);
    g.connect(text, "Result", l2, "Message", &NoFiles).unwrap();
    g.connect(mute, "Out", l2, "In", &NoFiles).unwrap();
    g.save(&root.join("Assets/Game.vgraph")).unwrap();

    let (mut scene, button, _) = ui_scene();
    let game = scene.spawn("Game", None);
    scene.add(
        game,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Game.vgraph".into(),
                ..Default::default()
            }],
        },
    );
    scene.world.get::<&mut UiButton>(button).unwrap().on_click = vec![ButtonCall {
        target: Some(game),
        method: "Play".into(),
    }];
    let canvas = scene.find_by_name("Canvas").unwrap();
    let toggle = scene.spawn("Toggle", Some(canvas));
    scene.add(
        toggle,
        RectTransform {
            anchored_position: Vec2::new(0.0, -200.0),
            ..RectTransform::sized(40.0, 40.0)
        },
    );
    scene.add(
        toggle,
        UiToggle {
            on_value_changed: vec![ButtonCall {
                target: Some(game),
                method: "Mute".into(),
            }],
            ..Default::default()
        },
    );
    let mut h = host(&mut scene, root);
    let mut time = Time::default();
    let screen = Vec2::new(800.0, 600.0);
    let mut frame = |scene: &mut Scene, down: bool, up: bool| {
        time.advance(1.0 / 60.0);
        let input = Input {
            mouse_position: Some(Vec2::new(400.0, 300.0)),
            screen_size: screen,
            mouse_pressed: [down, false, false],
            mouse_released: [up, false, false],
            ..Default::default()
        };
        h.update(scene, &time, input);
    };
    frame(&mut scene, false, false);
    frame(&mut scene, true, false);
    frame(&mut scene, false, true);
    // A script switching the toggle off sends On Value Changed(false).
    scene.world.get::<&mut UiToggle>(toggle).unwrap().is_on = false;
    frame(&mut scene, false, false);
    let logs = h.take_logs();
    let warnings: Vec<&String> = logs
        .iter()
        .filter(|(k, _)| *k != LogKind::Info)
        .map(|(_, t)| t)
        .collect();
    assert!(warnings.is_empty(), "{warnings:?}");
    let infos: Vec<&str> = logs.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(infos, ["play", "mute false"]);
}
