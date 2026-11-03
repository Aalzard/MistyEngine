//! Lua Input Manager, tag and layer API tests (input is injected, no devices needed).

use glam::Vec3;

use super::*;
use crate::input::Joystick;
use crate::physics::Physics;
use crate::scene::ScriptRef;
use crate::settings::ProjectSettings;

/// Run `source` (a script class) on one object for each input, with project settings.
fn play(
    source: &str,
    settings: &ProjectSettings,
    frames: &[Input],
    setup: impl FnOnce(&mut Scene),
) -> Vec<(LogKind, String)> {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_input_test_{n}"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Probe.lua"), source).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Probe", None);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Probe.lua".into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    setup(&mut scene);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let mut host = ScriptHost::start_configured(&mut scene, physics, root, audio, settings);
    let mut time = Time::default();
    for input in frames {
        time.advance(0.1);
        host.update(&mut scene, &time, input.clone());
    }
    host.take_logs()
}

fn texts(logs: &[(LogKind, String)]) -> Vec<String> {
    logs.iter().map(|(_, t)| t.clone()).collect()
}

#[test]
fn axes_buttons_and_any_key_from_lua() {
    let src = r#"
local P = {}
function P:Update()
    Debug.Log(string.format("%.2f %.0f %s %s %s %s %s",
        Input.GetAxis("Horizontal"), Input.GetAxisRaw("Horizontal"),
        tostring(Input.GetButton("Jump")), tostring(Input.GetButtonDown("Jump")),
        tostring(Input.GetButtonUp("Fire1")), tostring(Input.anyKey), tostring(Input.anyKeyDown)))
end
return P
"#;
    let mut first = Input::default();
    first.held.extend(["d".to_string(), "space".to_string()]);
    first.pressed.insert("space".into());
    let mut second = first.clone();
    second.pressed.clear();
    second.mouse_released[0] = true;
    let logs = play(
        src,
        &ProjectSettings::default(),
        &[first, second, Input::default()],
        |_| {},
    );
    assert_eq!(
        texts(&logs),
        [
            "0.30 1 true true false true true",
            "0.60 1 true false true true false",
            "0.30 0 false false false false false",
        ]
    );
}

#[test]
fn unknown_axes_are_errors_and_settings_axes_are_used() {
    let src = r#"
local P = {}
function P:Update()
    Debug.Log(Input.GetAxisRaw("Dash"))
    Input.GetAxis("Nope")
end
return P
"#;
    let mut settings = ProjectSettings::default();
    settings.input.axes.push(crate::input::InputAxis {
        name: "Dash".into(),
        positive_button: "joystick 1 button 5".into(),
        ..Default::default()
    });
    let mut pad = Joystick::default();
    pad.held[5] = true;
    let input = Input {
        joysticks: vec![pad],
        ..Default::default()
    };
    let logs = play(src, &settings, &[input], |_| {});
    assert_eq!(logs[0].1, "1");
    assert!(
        logs[1].0 == LogKind::Error && logs[1].1.contains("Input Axis Nope is not setup"),
        "{logs:?}"
    );
}

#[test]
fn joystick_names_scroll_and_joystick_keys() {
    let src = r#"
local P = {}
function P:Update()
    local names = Input.GetJoystickNames()
    Debug.Log(#names, names[1], Input.mouseScrollDelta.y, tostring(Input.GetKey("joystick button 0")),
        Input.mousePosition.x)
end
return P
"#;
    let mut pad = Joystick {
        name: "Fake Pad".into(),
        ..Default::default()
    };
    pad.held[0] = true;
    let input = Input {
        joysticks: vec![pad],
        scroll_delta: glam::Vec2::new(0.0, -50.0),
        mouse_position: Some(glam::Vec2::new(12.0, 0.0)),
        screen_size: glam::Vec2::new(100.0, 100.0),
        ..Default::default()
    };
    let logs = play(src, &ProjectSettings::default(), &[input], |_| {});
    assert_eq!(texts(&logs), ["1 Fake Pad -1 true 12"]);
}

#[test]
fn tags_and_user_layers() {
    let src = r#"
local P = {}
function P:Start()
    Debug.Log(LayerMask.NameToLayer("Hidden"), LayerMask.LayerToName(9), LayerMask.GetMask("Hidden", "UI"))
    Debug.Log(#GameObject.FindGameObjectsWithTag("Enemy"), GameObject.FindWithTag("Enemy").name,
        tostring(GameObject.FindWithTag("Player")), tostring(self.gameObject:CompareTag("Untagged")))
    GameObject.FindWithTag("Boss")
end
return P
"#;
    let mut settings = ProjectSettings::default();
    settings.tags_and_layers.tags.push("Enemy".into());
    settings.tags_and_layers.layers[9] = "Hidden".into();
    let logs = play(src, &settings, &[Input::default()], |s| {
        for name in ["Orc", "Troll"] {
            let e = s.spawn(name, None);
            s.game_object_mut(e).tag = "Enemy".into();
        }
        let off = s.spawn("Sleeping", None);
        s.game_object_mut(off).tag = "Enemy".into();
        s.game_object_mut(off).active = false;
    });
    assert_eq!(logs[0].1, format!("9 Hidden {}", (1 << 9) | (1 << 4)));
    assert_eq!(logs[1].1, "2 Orc nil true");
    assert!(logs[2].1.contains("Tag: Boss is not defined"), "{logs:?}");
}
