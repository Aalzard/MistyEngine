//! Procedural InputSystemDemo scene in `SampleProject`: the First Person Player driven by the
//! Input System instead of the Input Manager. Its Player Input plays
//! `Assets/Input/Player.inputactions` (Player map: Move, Look, Jump, Sprint, Crouch, Interact,
//! Menu; UI map: Close; Keyboard&Mouse and Gamepad schemes). `PlayerActions.lua` gets Player
//! Input messages (holding Interact rumbles the gamepad; the hint shows the control scheme),
//! and `RebindMenu.lua` is a small runtime rebinding menu (Tab or Start) whose overrides are
//! saved to PlayerPrefs.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_input_system_demo`; the other
//! tests check the committed files still match and play the scene with injected input.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::input::actions::asset::{GAMEPAD, KEYBOARD_MOUSE, default_schemes};
use misty_engine::input::actions::{
    Action, ActionMap, ActionType, Binding, ControlType, InputActionAsset, PlayerInput,
    PlayerInputBehavior,
};
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts,
    TextAnchor, Transform, UiImage, UiText,
};
use misty_engine::ui::presets;

const SCENE: &str = "Assets/Scenes/InputSystemDemo.scene";
const ACTIONS: &str = "Assets/Input/Player.inputactions";
const PLAYER_ACTIONS: &str = "Assets/Scripts/PlayerActions.lua";
const REBIND_MENU: &str = "Assets/Scripts/RebindMenu.lua";
const CHECKER: &str = "Assets/Materials/Checker.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const CRATE: &str = "Assets/Materials/AO Crate.mat";
/// Actions the rebinding menu lists.
const REBIND_ROWS: [&str; 4] = ["Jump", "Sprint", "Crouch", "Interact"];

const PLAYER_ACTIONS_LUA: &str = r#"-- PlayerActions: Player Input messages in the InputSystemDemo (Send Messages behaviour).
-- OnInteract runs when Interact performs (hold E, or X on a gamepad, for half a second): the
-- gamepad rumbles briefly. OnControlsChanged notes the control scheme in use. The hint text
-- shows the scheme and the current Jump binding. Moving, looking, jumping, sprinting and
-- crouching are read by FirstPersonController from the same Player Input.
local PlayerActions = {
    rumbleTime = 0.3, -- seconds
}

function PlayerActions:Start()
    self.player = self.gameObject:GetComponent("PlayerInput")
    self.jump = self.player.actions:FindAction("Player/Jump")
    local hint = GameObject.Find("Hint Text")
    self.hint = hint and hint:GetComponent("Text")
    self._rumble = 0
    self._interacts = 0
    self._changes = 0
end

function PlayerActions:OnInteract()
    self._interacts = self._interacts + 1
    local pad = self.player:GetDevice("Gamepad")
    if pad ~= nil then
        pad:SetMotorSpeeds(0.4, 0.9)
        self._rumble = self.rumbleTime
    end
end

function PlayerActions:OnControlsChanged(player)
    self._changes = self._changes + 1
end

function PlayerActions:Update()
    if self._rumble > 0 then
        self._rumble = self._rumble - Time.unscaledDeltaTime
        if self._rumble <= 0 then
            local pad = self.player:GetDevice("Gamepad")
            if pad ~= nil then pad:ResetHaptics() end
        end
    end
    if self.hint == nil then return end
    local text = "Jump: " .. self.jump:GetBindingDisplayString()
        .. "    Hold E / X: interact    Tab / Start: rebind menu\n"
        .. "Control scheme: " .. tostring(self.player.currentControlScheme)
        .. "    Interactions: " .. self._interacts
    if text ~= self._shown then
        self.hint.text = text
        self._shown = text
    end
end

return PlayerActions
"#;

const REBIND_MENU_LUA: &str = r#"-- RebindMenu: Tab (Start on a gamepad) opens a menu that rebinds the Player actions while
-- playing. Each row's button listens for the next key or gamepad button
-- (action:PerformInteractiveRebinding; Esc cancels) and rebinds the action's binding for the
-- control scheme in use. Overrides are saved to PlayerPrefs and loaded back at start; Reset
-- removes them. Opening the menu switches the Player Input to its UI map, so the player stops
-- and FirstPersonController leaves the cursor free.
local RebindMenu = {
    prefsKey = "InputSystemDemo.rebinds",
}

local ROWS = { "Jump", "Sprint", "Crouch", "Interact" }

-- Index of the first plain binding of `action` in control scheme `scheme`.
local function bindingIndex(action, scheme)
    for i, b in ipairs(action.bindings) do
        local inScheme = scheme == nil or string.find(";" .. b.groups .. ";", ";" .. scheme .. ";", 1, true)
        if not b.isComposite and not b.isPartOfComposite and inScheme then
            return i - 1
        end
    end
    return 0
end

local function component(t, path, name)
    return t:Find(path).gameObject:GetComponent(name)
end

function RebindMenu:Start()
    self.player = GameObject.Find("Player"):GetComponent("PlayerInput")
    self.actions = self.player.actions
    local saved = PlayerPrefs.GetString(self.prefsKey, "")
    if saved ~= "" then
        self.actions:LoadBindingOverridesFromJson(saved)
    end
    local panel = self.transform:Find("Rebind Panel")
    self.panel = panel.gameObject
    self.status = component(panel, "Status", "Text")
    self.rows = {}
    for _, name in ipairs(ROWS) do
        local row = {
            action = self.actions:FindAction("Player/" .. name),
            label = component(panel, name .. " Binding/Text", "Text"),
        }
        component(panel, name .. " Binding", "Button").onClick:AddListener(function()
            self:_rebind(row)
        end)
        self.rows[#self.rows + 1] = row
    end
    component(panel, "Reset", "Button").onClick:AddListener(function() self:_reset() end)
    component(panel, "Close", "Button").onClick:AddListener(function() self:Close() end)
    self.actions:FindAction("Player/Menu").performed:AddListener(function() self:Open() end)
    self.actions:FindAction("UI/Close").performed:AddListener(function()
        if self._op == nil and (self._settle or 0) <= 0 then self:Close() end
    end)
    self._settle = 0
end

function RebindMenu:Update()
    if self._settle > 0 then self._settle = self._settle - 1 end
end

function RebindMenu:Open()
    self.panel:SetActive(true)
    self.player:SwitchCurrentActionMap("UI")
    Cursor.lockState = CursorLockMode.None
    self.status.text = "Click a binding, then press a key or a gamepad button"
    self:_refresh()
end

function RebindMenu:Close()
    if self._op ~= nil then self._op:Cancel() end
    self.panel:SetActive(false)
    self.player:SwitchCurrentActionMap("Player")
end

function RebindMenu:_refresh()
    local scheme = self.player.currentControlScheme
    for _, row in ipairs(self.rows) do
        row.label.text = row.action:GetBindingDisplayString(bindingIndex(row.action, scheme))
    end
end

function RebindMenu:_rebind(row)
    if self._op ~= nil then return end
    row.label.text = "Press a key or button..."
    self.status.text = "Listening (Esc cancels)"
    local index = bindingIndex(row.action, self.player.currentControlScheme)
    self._op = row.action:PerformInteractiveRebinding(index)
        :WithControlsExcluding("<Mouse>")
        :WithCancelingThrough("<Keyboard>/escape")
        :OnComplete(function(op) self:_done(op, true) end)
        :OnCancel(function(op) self:_done(op, false) end)
        :Start()
end

function RebindMenu:_done(op, completed)
    op:Dispose()
    self._op = nil
    self._settle = 2
    if completed then
        PlayerPrefs.SetString(self.prefsKey, self.actions:SaveBindingOverridesAsJson())
        PlayerPrefs.Save()
        self.status.text = "Saved to PlayerPrefs"
    else
        self.status.text = "Canceled"
    end
    self:_refresh()
end

function RebindMenu:_reset()
    self.actions:RemoveAllBindingOverrides()
    PlayerPrefs.SetString(self.prefsKey, "")
    PlayerPrefs.Save()
    self.status.text = "Bindings reset"
    self:_refresh()
end

return RebindMenu
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The demo's actions: the First Person Player's controls for keyboard and mouse and gamepads.
pub fn demo_actions() -> InputActionAsset {
    const KM: &str = KEYBOARD_MOUSE;
    const GP: &str = GAMEPAD;
    let with = |path: &str, groups: &str, processors: &str, interactions: &str| Binding {
        processors: processors.into(),
        interactions: interactions.into(),
        ..Binding::to(path, groups)
    };
    let dirs = |up: &str, down: &str, left: &str, right: &str| {
        [("up", up), ("down", down), ("left", left), ("right", right)]
            .map(|(p, k)| (p, k.to_owned()))
    };
    let composite = |name: &str, parts: [(&str, String); 4]| {
        let parts: Vec<(&str, &str)> = parts.iter().map(|(p, k)| (*p, k.as_str())).collect();
        Binding::composite(name, "2DVector", &parts, KM)
    };
    let player = ActionMap {
        name: "Player".into(),
        id: String::new(),
        actions: vec![
            Action::new("Move", ActionType::Value, ControlType::Vector2)
                .with(composite(
                    "WASD",
                    dirs(
                        "<Keyboard>/w",
                        "<Keyboard>/s",
                        "<Keyboard>/a",
                        "<Keyboard>/d",
                    ),
                ))
                .with(composite(
                    "Arrows",
                    dirs(
                        "<Keyboard>/upArrow",
                        "<Keyboard>/downArrow",
                        "<Keyboard>/leftArrow",
                        "<Keyboard>/rightArrow",
                    ),
                ))
                .with(with("<Gamepad>/leftStick", GP, "stickDeadzone", "")),
            // The mouse is scaled like the Mouse X / Mouse Y axes (0.1 per pixel).
            Action::new("Look", ActionType::Value, ControlType::Vector2)
                .with(with("<Mouse>/delta", KM, "scaleVector2(x=0.1,y=0.1)", ""))
                .with(with("<Gamepad>/rightStick", GP, "stickDeadzone", "")),
            Action::new("Jump", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/space", KM))
                .with(Binding::to("<Gamepad>/buttonSouth", GP)),
            Action::new("Sprint", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/leftShift", KM))
                .with(Binding::to("<Gamepad>/leftStickPress", GP)),
            Action::new("Crouch", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/leftCtrl", KM))
                .with(Binding::to("<Keyboard>/c", KM))
                .with(Binding::to("<Gamepad>/buttonEast", GP)),
            Action::new("Interact", ActionType::Button, ControlType::Button)
                .with(with("<Keyboard>/e", KM, "", "hold(duration=0.5)"))
                .with(with("<Gamepad>/buttonWest", GP, "", "hold(duration=0.5)")),
            Action::new("Menu", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/tab", KM))
                .with(Binding::to("<Gamepad>/start", GP)),
        ],
    };
    let ui = ActionMap {
        name: "UI".into(),
        id: String::new(),
        actions: vec![
            Action::new("Close", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/tab", KM))
                .with(Binding::to("<Gamepad>/start", GP))
                .with(Binding::to("<Gamepad>/buttonEast", GP)),
        ],
    };
    let mut a = InputActionAsset {
        name: "Player".into(),
        maps: vec![player, ui],
        // Keyboard&Mouse and Gamepad (no touch controls in this demo).
        control_schemes: default_schemes().into_iter().take(2).collect(),
    };
    a.ensure_ids();
    a
}

fn block(s: &mut Scene, name: &str, pos: Vec3, scale: Vec3, mat: &str) -> Entity {
    let e = s.spawn(name, None);
    let mut t = Transform::from_position(pos);
    t.scale = scale;
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    s.add(e, BoxCollider::default());
    e
}

fn script(path: &str) -> ScriptRef {
    ScriptRef {
        path: path.into(),
        ..Default::default()
    }
}

fn rect(pos: Vec2, size: Vec2) -> RectTransform {
    RectTransform {
        anchored_position: pos,
        ..RectTransform::sized(size.x, size.y)
    }
}

fn text(
    s: &mut Scene,
    name: &str,
    parent: Entity,
    rt: RectTransform,
    value: &str,
    size: f32,
) -> Entity {
    let t = presets::text(s, name, parent);
    *s.world.get::<&mut RectTransform>(t).unwrap() = rt;
    let mut c = s.world.get::<&mut UiText>(t).unwrap();
    c.text = value.into();
    c.font_size = size;
    c.color = [1.0; 4];
    t
}

fn button(s: &mut Scene, name: &str, parent: Entity, rt: RectTransform, label: &str) -> Entity {
    let b = presets::button(s, name, parent);
    *s.world.get::<&mut RectTransform>(b).unwrap() = rt;
    let t = s.children(b)[0];
    s.world.get::<&mut UiText>(t).unwrap().text = label.into();
    b
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("InputSystemDemo");
    // The player rig brings its own camera.
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.despawn(cam);
    }
    block(
        &mut s,
        "Floor",
        Vec3::new(0.0, -0.5, 8.0),
        Vec3::new(40.0, 1.0, 40.0),
        CHECKER,
    );
    for (i, (x, h)) in [(-4.0, 0.5), (-2.0, 1.0), (0.0, 1.5)]
        .into_iter()
        .enumerate()
    {
        block(
            &mut s,
            &format!("Step {}", i + 1),
            Vec3::new(x, h / 2.0, 8.0),
            Vec3::new(2.0, h, 2.0),
            WOOD,
        );
    }
    block(&mut s, "Crate", Vec3::new(4.0, 0.5, 5.0), Vec3::ONE, CRATE);
    // A low beam to crouch under.
    block(
        &mut s,
        "Beam",
        Vec3::new(5.0, 1.6, 10.0),
        Vec3::new(4.0, 0.4, 1.0),
        WOOD,
    );

    let player = crate::fps_player::build(&mut s, None);
    s.add(
        player,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            default_map: "Player".into(),
            behavior: PlayerInputBehavior::SendMessages,
            ..Default::default()
        },
    );
    s.world
        .get::<&mut Scripts>(player)
        .unwrap()
        .list
        .push(script(PLAYER_ACTIONS));

    // UI: the hint in the top-left corner and the rebinding menu (hidden until opened).
    let canvas = presets::spawn(&mut s, "Canvas", None, RectTransform::stretched());
    s.add(canvas, misty_engine::scene::Canvas::default());
    s.add(canvas, misty_engine::scene::CanvasScaler::default());
    s.add(
        canvas,
        Scripts {
            list: vec![script(REBIND_MENU)],
        },
    );
    presets::event_system(&mut s, "EventSystem");
    let corner = Vec2::new(0.0, 1.0);
    let hint = text(
        &mut s,
        "Hint Text",
        canvas,
        RectTransform {
            anchor_min: corner,
            anchor_max: corner,
            pivot: corner,
            anchored_position: Vec2::new(20.0, -20.0),
            size_delta: Vec2::new(1100.0, 70.0),
        },
        "Hold E / X: interact    Tab / Start: rebind menu",
        22.0,
    );
    s.world.get::<&mut UiText>(hint).unwrap().alignment = TextAnchor::UpperLeft;

    let panel = presets::panel(&mut s, "Rebind Panel", canvas);
    *s.world.get::<&mut RectTransform>(panel).unwrap() = rect(Vec2::ZERO, Vec2::new(560.0, 400.0));
    s.world.get::<&mut UiImage>(panel).unwrap().color = [0.08, 0.09, 0.12, 0.92];
    s.game_object_mut(panel).active = false;
    text(
        &mut s,
        "Title",
        panel,
        rect(Vec2::new(0.0, 165.0), Vec2::new(500.0, 40.0)),
        "Rebind Controls",
        28.0,
    );
    for (i, name) in REBIND_ROWS.iter().enumerate() {
        let y = 105.0 - i as f32 * 52.0;
        let l = text(
            &mut s,
            &format!("{name} Label"),
            panel,
            rect(Vec2::new(-130.0, y), Vec2::new(200.0, 40.0)),
            name,
            22.0,
        );
        s.world.get::<&mut UiText>(l).unwrap().alignment = TextAnchor::MiddleLeft;
        button(
            &mut s,
            &format!("{name} Binding"),
            panel,
            rect(Vec2::new(110.0, y), Vec2::new(240.0, 40.0)),
            "",
        );
    }
    text(
        &mut s,
        "Status",
        panel,
        rect(Vec2::new(0.0, -110.0), Vec2::new(500.0, 36.0)),
        "",
        18.0,
    );
    button(
        &mut s,
        "Reset",
        panel,
        rect(Vec2::new(-130.0, -160.0), Vec2::new(200.0, 40.0)),
        "Reset",
    );
    button(
        &mut s,
        "Close",
        panel,
        rect(Vec2::new(130.0, -160.0), Vec2::new(200.0, 40.0)),
        "Close",
    );
    s
}

fn files() -> [(&'static str, String); 3] {
    [
        (ACTIONS, demo_actions().to_json() + "\n"),
        (PLAYER_ACTIONS, PLAYER_ACTIONS_LUA.to_owned()),
        (REBIND_MENU, REBIND_MENU_LUA.to_owned()),
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_input_system_demo() {
    let root = project();
    for (rel, text) in files() {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn input_system_demo_matches_its_generator() {
    let root = project();
    for (rel, text) in files() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            text,
            "{rel}: regenerate with write_input_system_demo"
        );
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_input_system_demo",
    );
    let (_, report) = Scene::from_json("InputSystemDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Every binding path in the asset parses.
    let a = InputActionAsset::load(&root.join(ACTIONS)).unwrap();
    for m in &a.maps {
        for act in &m.actions {
            for b in &act.bindings {
                for p in std::iter::once(&b.path).chain(b.parts.iter().map(|p| &p.path)) {
                    if !p.is_empty() {
                        misty_engine::input::actions::path::ControlPath::parse(p).unwrap();
                    }
                }
            }
        }
    }
}

/// Playing with injected keys and a fake gamepad: W walks forward through the Move action,
/// Tab opens the rebinding menu (the Player map stops, so W doesn't walk) and closes it,
/// a gamepad switches the control scheme and holding X rumbles it.
#[test]
fn input_system_demo_plays_with_actions() {
    use misty_engine::audio::Audio;
    use misty_engine::input::Joystick;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    // No saved rebinds: a fresh prefs file.
    let _prefs = crate::test_util::PREFS_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    misty_engine::prefs::open(&std::env::temp_dir().join(format!(
        "misty_input_system_demo_prefs_{}.json",
        std::process::id()
    )));
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let settings = ProjectSettings::load(&root).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
    let mut time = Time::new(&settings.time);
    let player = scene.find_by_name("Player").unwrap();
    let panel = scene.find_by_name("Rebind Panel").unwrap();
    let hint = scene.find_by_name("Hint Text").unwrap();
    let mut frame = |scene: &mut Scene, input: &Input| {
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, input.clone());
        let errors: Vec<_> = rt
            .take_logs()
            .into_iter()
            .filter(|(k, _)| *k == LogKind::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        rt.take_rumble()
    };
    let keys = |prev: &[&str], held: &[&str]| {
        let mut i = Input::default();
        i.held.extend(held.iter().map(|k| (*k).to_owned()));
        i.pressed.extend(
            held.iter()
                .filter(|k| !prev.contains(k))
                .map(|k| (*k).to_owned()),
        );
        i
    };
    let idle = Input::default();
    for _ in 0..30 {
        frame(&mut scene, &idle);
    }
    let start = scene.world_position(player);
    let text = scene.world.get::<&UiText>(hint).unwrap().text.clone();
    assert!(
        text.contains("Jump: Space") && text.contains("Keyboard&Mouse"),
        "{text}"
    );

    // W through the WASD composite walks forward (+z).
    frame(&mut scene, &keys(&[], &["w"]));
    for _ in 0..40 {
        frame(&mut scene, &keys(&["w"], &["w"]));
    }
    let walked = scene.world_position(player).z - start.z;
    assert!(walked > 1.5, "walked {walked}");
    frame(&mut scene, &idle);

    // Tab opens the menu: the Player map is off, so W does nothing.
    frame(&mut scene, &keys(&[], &["tab"]));
    assert!(scene.game_object(panel).active, "menu open");
    for _ in 0..20 {
        frame(&mut scene, &idle);
    }
    let z = scene.world_position(player).z;
    frame(&mut scene, &keys(&[], &["w"]));
    for _ in 0..20 {
        frame(&mut scene, &keys(&["w"], &["w"]));
    }
    assert!(
        (scene.world_position(player).z - z).abs() < 0.05,
        "stands still"
    );
    frame(&mut scene, &idle);
    frame(&mut scene, &keys(&[], &["tab"]));
    assert!(!scene.game_object(panel).active, "menu closed");

    // A gamepad: its first button press switches the scheme; holding X 0.5 s rumbles it.
    let mut pad = Joystick {
        name: "Test Pad".into(),
        ..Default::default()
    };
    let mut rumble = None;
    for i in 0..45 {
        pad.held[2] = true;
        pad.pressed[2] = i == 0;
        let input = Input {
            joysticks: vec![pad.clone()],
            ..Default::default()
        };
        if let Some(r) = frame(&mut scene, &input) {
            rumble = Some(r);
        }
    }
    assert_eq!(rumble, Some(vec![(0, 0.4, 0.9)]));
    let text = scene.world.get::<&UiText>(hint).unwrap().text.clone();
    assert!(
        text.contains("Gamepad") && text.contains("Jump: Button South"),
        "{text}"
    );
    assert!(text.contains("Interactions: 1"), "{text}");
}
