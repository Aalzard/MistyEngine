//! Lua Input System tests: Player Input messages, `InputActions.Load`, polling, listeners,
//! map switching, rebinding saved to PlayerPrefs, Active Input Handling, touches and rumble.

use glam::Vec3;

use super::super::*;
use crate::input::actions::{InputActionAsset, PlayerInput, PlayerInputBehavior, PlayerInputCall};
use crate::physics::Physics;
use crate::scene::ScriptRef;
use crate::settings::ProjectSettings;

const ACTIONS: &str = "Assets/Player.inputactions";

fn root(tag: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_actions_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join(ACTIONS))
        .unwrap();
    root
}

fn scripted(scene: &mut Scene, name: &str, script: &str) -> hecs::Entity {
    let e = scene.spawn(name, None);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: script.into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    e
}

/// Play `scene` for each input frame (0.1 s apart); returns the host and the logs.
fn play(
    root: &std::path::Path,
    scene: &mut Scene,
    settings: &ProjectSettings,
    frames: &[Input],
) -> (ScriptHost, Vec<String>) {
    let physics = Physics::build(scene, Vec3::ZERO);
    let (audio, _) = crate::audio::Audio::null(root.to_path_buf());
    let mut host =
        ScriptHost::start_configured(scene, physics, root.to_path_buf(), audio, settings);
    let mut time = Time::default();
    for input in frames {
        time.advance(0.1);
        host.update(scene, &time, input.clone());
    }
    let logs = host
        .take_logs()
        .into_iter()
        .map(|(k, t)| match k {
            LogKind::Error => format!("ERROR {t}"),
            _ => t,
        })
        .collect();
    (host, logs)
}

fn keys(prev: &[&str], held: &[&str]) -> Input {
    let mut i = Input::default();
    i.held.extend(held.iter().map(|k| (*k).to_owned()));
    i.pressed.extend(
        held.iter()
            .filter(|k| !prev.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i.released.extend(
        prev.iter()
            .filter(|k| !held.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i
}

const PLAYER_LUA: &str = r#"
local P = {}
function P:OnMove(v) Debug.Log(string.format("move %.1f %.1f", v.x, v.y)) end
function P:OnJump() Debug.Log("jump") end
return P
"#;

const WATCHER_LUA: &str = r#"
local W = {}
function W:Start()
    self.actions = InputActions.Load("Assets/Player.inputactions")
    self.jump = self.actions:FindAction("Player/Jump")
    self.move = self.actions["Move"]
    self.jump.started:AddListener(function(ctx) Debug.Log("started " .. ctx.phase .. " " .. ctx.control) end)
    self.jump.performed:AddListener(function(ctx) Debug.Log("performed " .. tostring(ctx:ReadValue())) end)
    self.jump.canceled:AddListener(function(ctx) Debug.Log("canceled") end)
    local pi = GameObject.Find("Player"):GetComponent("PlayerInput")
    Debug.Log("scheme " .. tostring(pi.currentControlScheme) .. " map " .. pi.currentActionMap.name
        .. " index " .. pi.playerIndex .. " same " .. tostring(pi.actions:FindAction("Jump") == self.jump))
end
function W:Update()
    Debug.Log(string.format("poll %s %s %s %.1f", tostring(self.jump:WasPressedThisFrame()),
        tostring(self.jump:IsPressed()), tostring(self.jump:WasReleasedThisFrame()), self.move:ReadValue().y))
end
return W
"#;

#[test]
fn player_input_messages_listeners_and_polling() {
    let root = root("messages");
    std::fs::write(root.join("Player.lua"), PLAYER_LUA).unwrap();
    std::fs::write(root.join("Watcher.lua"), WATCHER_LUA).unwrap();
    let mut scene = Scene::new("t");
    let player = scripted(&mut scene, "Player", "Player.lua");
    scene.add(
        player,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            default_map: "Player".into(),
            ..Default::default()
        },
    );
    scripted(&mut scene, "Watcher", "Watcher.lua");
    let frames = [
        keys(&[], &["w", "space"]),
        keys(&["w", "space"], &["w", "space"]),
        keys(&["w", "space"], &[]),
    ];
    let (_, logs) = play(&root, &mut scene, &ProjectSettings::default(), &frames);
    assert_eq!(
        logs,
        [
            "scheme Keyboard&Mouse map Player index 0 same true",
            "move 0.0 1.0",
            "started Started <Keyboard>/space",
            "performed 1.0",
            "jump",
            "poll true true false 1.0",
            "poll false true false 1.0",
            "move 0.0 0.0",
            "canceled",
            "poll false false true 0.0",
        ]
    );
}

const EVENTS_LUA: &str = r#"
local E = {}
function E:Start()
    local pi = self.gameObject:GetComponent("PlayerInput")
    self.pi = pi
    pi.onActionTriggered:AddListener(function(ctx)
        if ctx.performed then Debug.Log("triggered " .. tostring(ctx.action)) end
    end)
    self.ui = pi.actions:FindActionMap("UI")
    Debug.Log("ui enabled " .. tostring(self.ui.enabled))
end
function E:Fire(ctx) Debug.Log("Fire " .. ctx.phase .. " " .. string.format("%.2f", ctx.duration)) end
function E:Update()
    if Input.GetKeyDown("tab") then
        self.pi:SwitchCurrentActionMap("UI")
        Debug.Log("map " .. self.pi.currentActionMap.name .. " ui " .. tostring(self.ui.enabled))
    end
end
return E
"#;

#[test]
fn invoke_events_hold_interaction_and_map_switching() {
    let root = root("events");
    // Interact holds for 0.4 s (the starter asset); events call E:Fire with the context.
    std::fs::write(root.join("Events.lua"), EVENTS_LUA).unwrap();
    let mut scene = Scene::new("t");
    let e = scripted(&mut scene, "Player", "Events.lua");
    scene.add(
        e,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            behavior: PlayerInputBehavior::InvokeEvents,
            events: vec![PlayerInputCall {
                action: "Player/Interact".into(),
                target: Some(e),
                method: "Fire".into(),
            }],
            ..Default::default()
        },
    );
    let mut frames = vec![keys(&[], &["e"])];
    for _ in 0..5 {
        frames.push(keys(&["e"], &["e"]));
    }
    frames.push(keys(&["e"], &[]));
    frames.push(keys(&[], &["tab"]));
    // In the UI map the E key does nothing; Enter submits.
    frames.push(keys(&["tab"], &["e", "return"]));
    let (_, logs) = play(&root, &mut scene, &ProjectSettings::default(), &frames);
    assert_eq!(
        logs,
        [
            "ui enabled false",
            "Fire Started 0.00",
            "triggered Player/Interact",
            "Fire Performed 0.40",
            "Fire Canceled 0.60",
            "map UI ui true",
            "triggered UI/Submit",
        ]
    );
}

const REBIND_LUA: &str = r#"
local R = {}
function R:Start()
    self.actions = InputActions.Load("Assets/Player.inputactions")
    self.actions:Enable()
    local saved = PlayerPrefs.GetString("rebind_test.bindings", "")
    if saved ~= "" then self.actions:LoadBindingOverridesFromJson(saved) end
    self.jump = self.actions:FindAction("Jump")
    Debug.Log("binding " .. self.jump:GetBindingDisplayString(0))
    if saved == "" then
        self.jump:PerformInteractiveRebinding(0)
            :WithCancelingThrough("<Keyboard>/escape")
            :OnComplete(function(op)
                Debug.Log("rebound " .. op.selectedControl .. " " .. tostring(op.completed))
                PlayerPrefs.SetString("rebind_test.bindings", self.actions:SaveBindingOverridesAsJson())
                PlayerPrefs.Save()
                op:Dispose()
            end)
            :Start()
    end
end
function R:Update()
    if self.jump:WasPressedThisFrame() then Debug.Log("jump pressed") end
end
return R
"#;

#[test]
fn interactive_rebinding_saves_to_player_prefs_and_loads_next_run() {
    let _guard = crate::prefs::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = root("rebind");
    let prefs = root.join("prefs.json");
    crate::prefs::open(&prefs);
    std::fs::write(root.join("Rebind.lua"), REBIND_LUA).unwrap();
    let make = || {
        let mut scene = Scene::new("t");
        scripted(&mut scene, "Menu", "Rebind.lua");
        scene
    };
    let frames = [
        Input::default(),
        keys(&[], &["j"]),
        Input::default(),
        keys(&[], &["space"]),
        keys(&["space"], &["j"]),
    ];
    let (_, logs) = play(&root, &mut make(), &ProjectSettings::default(), &frames);
    assert_eq!(
        logs,
        ["binding Space", "rebound <Keyboard>/j true", "jump pressed"]
    );
    // A new run reads the override back from the prefs file.
    crate::prefs::open(&prefs);
    // (Start loads the actions after the first frame's input update.)
    let frames = [Input::default(), keys(&[], &["j"])];
    let (_, logs) = play(&root, &mut make(), &ProjectSettings::default(), &frames);
    assert_eq!(logs, ["binding J", "jump pressed"]);
}

#[test]
fn active_input_handling_gates_the_input_manager() {
    let root = root("handling");
    std::fs::write(
        root.join("Probe.lua"),
        r#"
local P = {}
function P:Start() self.a = InputActions.Load("Assets/Player.inputactions"); self.a:Enable() end
function P:Update()
    Debug.Log("jump " .. tostring(self.a:FindAction("Jump"):IsPressed()))
    Debug.Log("key " .. tostring(Input.GetKey("space")))
end
return P
"#,
    )
    .unwrap();
    let mut settings = ProjectSettings::default();
    settings.input.active_handling = crate::input::actions::ActiveInputHandling::InputSystem;
    let mut scene = Scene::new("t");
    scripted(&mut scene, "Probe", "Probe.lua");
    let frames = [Input::default(), keys(&[], &["space"])];
    let (_, logs) = play(&root, &mut scene, &settings, &frames);
    assert_eq!(logs[2], "jump true", "{logs:?}");
    assert!(
        logs[3].starts_with("ERROR") && logs[3].contains("Active Input Handling"),
        "{logs:?}"
    );

    settings.input.active_handling = crate::input::actions::ActiveInputHandling::InputManager;
    let mut scene = Scene::new("t");
    scripted(&mut scene, "Probe", "Probe.lua");
    let (_, logs) = play(&root, &mut scene, &settings, &frames);
    assert_eq!(logs, ["jump false", "key false", "jump false", "key true"]);
}

#[test]
fn touches_devices_and_rumble_from_lua() {
    let root = root("devices");
    std::fs::write(
        root.join("Probe.lua"),
        r#"
local P = {}
function P:Update()
    Debug.Log("touches " .. Input.touchCount .. " screen " .. tostring(Touchscreen.current ~= nil))
    if Input.touchCount > 0 then
        local t = Input.GetTouch(0)
        Debug.Log(string.format("%s %.0f %.0f", t.phase, t.position.x, t.position.y))
    end
    local pad = Gamepad.current
    if pad ~= nil then
        Debug.Log(pad.name .. " south " .. tostring(pad.buttonSouth.isPressed)
            .. string.format(" stick %.1f", pad.leftStick.y))
        pad:SetMotorSpeeds(0.25, 0.75)
    end
    Debug.Log("space " .. tostring(Keyboard.current.spaceKey.isPressed)
        .. " down " .. tostring(Keyboard.current.space.wasPressedThisFrame))
end
return P
"#,
    )
    .unwrap();
    let mut scene = Scene::new("t");
    scripted(&mut scene, "Probe", "Probe.lua");
    let mut f = keys(&[], &["space"]);
    f.screen_size = glam::Vec2::new(800.0, 600.0);
    f.touches = vec![crate::input::Touch {
        finger_id: 3,
        position: glam::Vec2::new(100.0, 200.0),
        ..Default::default()
    }];
    let mut pad = crate::input::Joystick {
        name: "Test Pad".into(),
        ..Default::default()
    };
    pad.held[0] = true;
    pad.axes[1] = -1.0;
    f.joysticks = vec![pad];
    let (host, logs) = play(&root, &mut scene, &ProjectSettings::default(), &[f]);
    assert_eq!(
        logs,
        [
            "touches 1 screen true",
            "Began 100 400",
            "Test Pad south true stick 1.0",
            "space true down true",
        ]
    );
    assert_eq!(host.take_rumble(), Some(vec![(0, 0.25, 0.75)]));
}

#[test]
fn rebinding_to_a_missing_binding_is_an_error() {
    let root = root("retarget");
    std::fs::write(
        root.join("Retarget.lua"),
        r#"
local R = {}
function R:Start()
    local jump = InputActions.Load("Assets/Player.inputactions"):FindAction("Jump")
    local op = jump:PerformInteractiveRebinding(0)
    Debug.Log("far " .. tostring(pcall(function() op:WithTargetBinding(99) end)))
    Debug.Log("first " .. tostring(pcall(function() op:WithTargetBinding(0) end)))
end
return R
"#,
    )
    .unwrap();
    let mut scene = Scene::new("t");
    scripted(&mut scene, "Menu", "Retarget.lua");
    let frames = [Input::default(), Input::default()];
    let (_, logs) = play(&root, &mut scene, &ProjectSettings::default(), &frames);
    assert_eq!(logs, ["far false", "first true"]);
}
