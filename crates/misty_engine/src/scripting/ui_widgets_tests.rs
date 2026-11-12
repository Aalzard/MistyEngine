//! Lua widget API tests: values, options, listeners and On Value Changed calls.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    ButtonCall, Canvas, CanvasScaler, RectTransform, Scene, ScriptRef, Scripts, UiDropdown,
    UiInputField, UiScaleMode, UiSlider, UiText, UiToggle,
};
use crate::scripting::{Input, LogKind, ScriptHost};
use crate::ui::presets;

fn project(source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_ui_widgets_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/W.lua"), source).unwrap();
    root
}

struct Ui {
    scene: Scene,
    slider: Entity,
    toggle: Entity,
    field: Entity,
    dropdown: Entity,
    label: Entity,
    runner: Entity,
}

/// Canvas with a slider, toggle, input field, dropdown and a Text, plus a scripted Runner.
fn ui() -> Ui {
    let mut s = Scene::new("UI");
    let c = s.spawn("Canvas", None);
    s.add(c, RectTransform::stretched());
    s.add(c, Canvas::default());
    s.add(
        c,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let place = |s: &mut Scene, e: Entity, y: f32| {
        s.world
            .get::<&mut RectTransform>(e)
            .unwrap()
            .anchored_position = Vec2::new(0.0, y);
    };
    let slider = presets::slider(&mut s, "Slider", c);
    place(&mut s, slider, 100.0);
    let toggle = presets::toggle(&mut s, "Toggle", c);
    place(&mut s, toggle, 50.0);
    let field = presets::input_field(&mut s, "Field", c);
    let dropdown = presets::dropdown(&mut s, "Dropdown", c);
    place(&mut s, dropdown, -50.0);
    let label = presets::text(&mut s, "Output", c);
    place(&mut s, label, -200.0);
    let runner = s.spawn("Runner", None);
    s.add(
        runner,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/W.lua".into(),
                ..Default::default()
            }],
        },
    );
    Ui {
        scene: s,
        slider,
        toggle,
        field,
        dropdown,
        label,
        runner,
    }
}

fn frame(h: &mut ScriptHost, s: &mut Scene, time: &mut Time, input: Input) {
    time.advance(1.0 / 60.0);
    let input = Input {
        screen_size: Vec2::new(800.0, 600.0),
        ..input
    };
    h.update(s, time, input);
}

fn click(h: &mut ScriptHost, s: &mut Scene, time: &mut Time, p: Vec2) {
    let press = Input {
        mouse_position: Some(p),
        mouse_held: [true, false, false],
        mouse_pressed: [true, false, false],
        ..Default::default()
    };
    frame(h, s, time, press);
    let release = Input {
        mouse_position: Some(p),
        mouse_released: [true, false, false],
        ..Default::default()
    };
    frame(h, s, time, release);
}

fn centre(s: &Scene, e: Entity) -> Vec2 {
    crate::ui::layout(s, Vec2::new(800.0, 600.0))
        .node(e)
        .unwrap()
        .screen
        .center()
}

#[test]
fn widget_properties_from_lua() {
    let src = r#"
local W = {}
function W:Start()
    local s = GameObject.Find("Slider"):GetComponent("Slider")
    s.minValue = 0; s.maxValue = 100; s.wholeNumbers = true
    s.value = 42.4
    s.value = 300
    local t = GameObject.Find("Toggle"):GetComponent("Toggle")
    t.isOn = false
    local f = GameObject.Find("Field"):GetComponent("InputField")
    f.contentType = "IntegerNumber"
    f.text = "12ab3"
    local d = GameObject.Find("Dropdown"):GetComponent("Dropdown")
    d:ClearOptions()
    d:AddOptions({ "Low", "High" })
    d.value = 7
    local o = d.options
    Debug.Log(s.value, tostring(t.isOn), f.text, f.contentType, #o, o[2], d.value, d.captionText)
end
return W
"#;
    let mut u = ui();
    let physics = Physics::build(&u.scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut u.scene, physics, project(src));
    let mut time = Time::default();
    frame(&mut h, &mut u.scene, &mut time, Input::default());
    let logs: Vec<String> = h.take_logs().into_iter().map(|(_, t)| t).collect();
    assert_eq!(logs, ["100 false 123 IntegerNumber 2 High 1 High"]);
    assert!(!u.scene.world.get::<&UiToggle>(u.toggle).unwrap().is_on);
}

#[test]
fn listeners_and_on_value_changed_calls_get_the_new_value() {
    let src = r#"
local W = {}
function W:Start()
    local label = GameObject.Find("Output"):GetComponent("Text")
    local s = GameObject.Find("Slider"):GetComponent("Slider")
    s.onValueChanged:AddListener(function(v) label.text = "slider " .. v end)
    local t = GameObject.Find("Toggle"):GetComponent("Toggle")
    local function onToggle(on) Debug.Log("toggle", tostring(on)) end
    t.onValueChanged:AddListener(onToggle)
    t.onValueChanged:AddListener(function() Debug.Log("second") end)
    self._t = t
    self._f = onToggle
end
function W:Picked(i) Debug.Log("picked", i) end
function W:Typed(text) Debug.Log("typed", text) end
function W:Update()
    if Input.GetKeyDown("x") then self._t.onValueChanged:RemoveListener(self._f) end
end
return W
"#;
    let mut u = ui();
    u.scene
        .world
        .get::<&mut UiDropdown>(u.dropdown)
        .unwrap()
        .on_value_changed = vec![ButtonCall {
        target: Some(u.runner),
        method: "Picked".into(),
    }];
    u.scene
        .world
        .get::<&mut UiInputField>(u.field)
        .unwrap()
        .on_end_edit = vec![ButtonCall {
        target: Some(u.runner),
        method: "Typed".into(),
    }];
    let physics = Physics::build(&u.scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut u.scene, physics, project(src));
    let mut time = Time::default();
    let s = &mut u.scene;
    frame(&mut h, s, &mut time, Input::default());
    // Slider: click at 3/4 of its handle area (x 330..470).
    click(
        &mut h,
        s,
        &mut time,
        Vec2::new(435.0, centre(s, u.slider).y),
    );
    assert_eq!(s.world.get::<&UiSlider>(u.slider).unwrap().value, 0.75);
    let early = h.take_logs();
    assert_eq!(
        s.world.get::<&UiText>(u.label).unwrap().text,
        "slider 0.75",
        "{early:?}"
    );
    let p = centre(s, u.toggle);
    click(&mut h, s, &mut time, p);
    // Dropdown: open, pick the second option.
    let p = centre(s, u.dropdown);
    click(&mut h, s, &mut time, p);
    let item = s.find_by_name("Item 1: Option B").expect("list open");
    let p = centre(s, item);
    click(&mut h, s, &mut time, p);
    // Input field: type, press Return.
    let p = centre(s, u.field);
    click(&mut h, s, &mut time, p);
    let typing = Input {
        text: "hi".into(),
        ..Default::default()
    };
    frame(&mut h, s, &mut time, typing);
    let enter = Input {
        typed: vec!["return".into()],
        ..Default::default()
    };
    frame(&mut h, s, &mut time, enter);
    // Remove the first toggle listener, then toggle back on.
    let mut x = Input::default();
    x.pressed.insert("x".into());
    frame(&mut h, s, &mut time, x);
    let p = centre(s, u.toggle);
    click(&mut h, s, &mut time, p);
    let logs: Vec<(LogKind, String)> = h.take_logs();
    let text: Vec<&str> = logs.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        text,
        ["toggle false", "second", "picked 1", "typed hi", "second"],
        "{logs:?}"
    );
}

#[test]
fn listeners_of_destroyed_objects_are_dropped() {
    let src = r#"
local W = {}
function W:Update()
    local copy = Instantiate(GameObject.Find("Slider"))
    copy:GetComponent("Slider").onValueChanged:AddListener(function() Debug.Log(self) end)
    Destroy(copy)
end
return W
"#;
    let mut u = ui();
    let physics = Physics::build(&u.scene, Vec3::ZERO);
    let mut h = ScriptHost::start(&mut u.scene, physics, project(src));
    let mut time = Time::default();
    for _ in 0..50 {
        frame(&mut h, &mut u.scene, &mut time, Input::default());
    }
    let errors: Vec<_> = h
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k == LogKind::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let n = crate::scripting::ui_widgets_api::listener_count(&h.lua);
    assert!(n <= 1, "{n} listener lists kept for destroyed objects");
}
