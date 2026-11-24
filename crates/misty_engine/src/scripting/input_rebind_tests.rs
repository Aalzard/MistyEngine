//! Lua interactive rebinding with a binding group, `bindings`, overrides saved as JSON and
//! loaded back, canceling through Escape; `tapCount` on touches.

use glam::{Vec2, Vec3};

use super::super::*;
use crate::input::actions::InputActionAsset;
use crate::input::{Joystick, Touch, TouchPhase};
use crate::physics::Physics;
use crate::scene::ScriptRef;

fn project(tag: &str, script: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_lua_rebind_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    std::fs::write(root.join("Probe.lua"), script).unwrap();
    root
}

fn play(root: &std::path::Path, frames: &[Input]) -> Vec<String> {
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
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root.to_path_buf());
    let mut time = Time::default();
    for input in frames {
        time.advance(0.1);
        host.update(&mut scene, &time, input.clone());
    }
    host.take_logs()
        .into_iter()
        .map(|(k, t)| match k {
            LogKind::Error => format!("ERROR {t}"),
            _ => t,
        })
        .collect()
}

fn key(k: &str) -> Input {
    let mut i = Input::default();
    i.held.insert(k.into());
    i.pressed.insert(k.into());
    i
}

fn pad_button(b: usize) -> Input {
    let mut j = Joystick {
        name: "Pad".into(),
        ..Default::default()
    };
    j.held[b] = true;
    j.pressed[b] = true;
    Input {
        joysticks: vec![j],
        ..Default::default()
    }
}

const REBIND: &str = r#"
local R = {}
function R:Start()
    self.actions = InputActions.Load("Assets/Player.inputactions")
    self.actions:Enable()
    self.jump = self.actions:FindAction("Jump")
    self.n = 0
end
function R:Update()
    self.n = self.n + 1
    local jump, actions = self.jump, self.actions
    if self.n == 1 then
        self.op = jump:PerformInteractiveRebinding()
            :WithBindingGroup("Gamepad")
            :OnComplete(function(op)
                local b = jump.bindings[2]
                Debug.Log("complete " .. op.selectedControl .. " " .. tostring(op.completed)
                    .. " " .. b.effectivePath .. " " .. tostring(b.overridePath) .. " " .. jump:GetBindingDisplayString(1))
                local json = actions:SaveBindingOverridesAsJson()
                actions:RemoveAllBindingOverrides()
                Debug.Log("removed " .. jump.bindings[2].effectivePath)
                actions:LoadBindingOverridesFromJson(json)
                Debug.Log("loaded " .. jump.bindings[2].effectivePath)
            end)
            :OnCancel(function() Debug.Log("canceled early") end)
        Debug.Log("target " .. self.op.bindingIndex)
        self.op:Start()
        Debug.Log("listening " .. tostring(self.op.started) .. " enabled " .. tostring(jump.enabled))
    elseif self.n == 4 then
        Debug.Log("enabled " .. tostring(jump.enabled))
        self.op2 = jump:PerformInteractiveRebinding(0)
            :OnComplete(function(op) Debug.Log("completed? " .. op.selectedControl) end)
            :OnCancel(function(op) Debug.Log("canceled " .. tostring(op.canceled)) end)
            :Start()
    elseif self.n == 6 then
        Debug.Log("space " .. jump.bindings[1].effectivePath .. " " .. tostring(jump.bindings[1].overridePath))
    end
end
return R
"#;

#[test]
fn lua_rebinding_with_a_group_json_round_trip_and_escape() {
    let root = project("flow", REBIND);
    let frames = [
        Input::default(),
        key("k"),      // keyboard: not a gamepad control
        pad_button(3), // buttonNorth
        Input::default(),
        key("escape"),
        Input::default(),
    ];
    let logs = play(&root, &frames);
    assert_eq!(
        logs,
        [
            "target 1",
            "listening true enabled false",
            "complete <Gamepad>/buttonNorth true <Gamepad>/buttonNorth <Gamepad>/buttonNorth Button North",
            "removed <Gamepad>/buttonSouth",
            "loaded <Gamepad>/buttonNorth",
            "enabled true",
            "canceled true",
            "space <Keyboard>/space nil",
        ]
    );
}

fn touch(id: u64, x: f32, phase: TouchPhase) -> Input {
    Input {
        screen_size: Vec2::new(800.0, 600.0),
        touches: vec![Touch {
            finger_id: id,
            position: Vec2::new(x, 300.0),
            phase,
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn lua_tap_count_counts_taps_in_a_row() {
    let root = project(
        "taps",
        r#"
local P = {}
function P:Update()
    for _, t in ipairs(Input.touches) do
        Debug.Log(t.phase .. " " .. t.tapCount .. " " .. Touchscreen.current.primaryTouch.tapCount)
    end
end
return P
"#,
    );
    let frames = [
        touch(1, 100.0, TouchPhase::Began),
        touch(1, 100.0, TouchPhase::Ended),
        touch(2, 104.0, TouchPhase::Began),
        touch(2, 104.0, TouchPhase::Ended),
        Input::default(),
        Input::default(),
        Input::default(),
        Input::default(),
        Input::default(),
        Input::default(),
        // Held 0.6 s: not a tap.
        touch(3, 100.0, TouchPhase::Began),
        touch(3, 100.0, TouchPhase::Stationary),
        touch(3, 100.0, TouchPhase::Stationary),
        touch(3, 100.0, TouchPhase::Stationary),
        touch(3, 100.0, TouchPhase::Stationary),
        touch(3, 100.0, TouchPhase::Stationary),
        touch(3, 100.0, TouchPhase::Ended),
    ];
    let logs = play(&root, &frames);
    assert_eq!(
        logs,
        [
            "Began 1 1",
            "Ended 1 1",
            "Began 2 2",
            "Ended 2 2",
            "Began 1 1",
            "Stationary 1 1",
            "Stationary 1 1",
            "Stationary 1 1",
            "Stationary 1 1",
            "Stationary 1 1",
            "Ended 0 0",
        ]
    );
}
