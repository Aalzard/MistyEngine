//! C# input (`csharp_input.rs`): `EventSystem.current` and `OnSelect` / `OnDeselect`, touches,
//! the Player Input Manager with split-screen camera rects, Player Input messages
//! (`OnMove(InputValue)`), action events and polling, `Gamepad` / `Keyboard` / `Mouse`.
//! Compiles real `.cs` files with the .NET SDK; skipped (with a note) when none is installed.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::input::actions::{
    InputActionAsset, PlayerInput, PlayerInputBehavior, PlayerInputCall, PlayerInputManager,
};
use crate::input::{Joystick, Touch, TouchPhase};
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    Camera, Canvas, CanvasScaler, EventSystem, RectTransform, Scene, ScriptRef, Scripts, UiButton,
    UiImage, UiScaleMode,
};
use crate::scripting::{Input, LogKind, ScriptHost, csharp};

const ACTIONS: &str = "Assets/Player.inputactions";

fn skip() -> bool {
    if csharp::dotnet_available() {
        return false;
    }
    eprintln!("no .NET runtime installed: C# test skipped");
    true
}

fn project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_cs_input_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, src) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    InputActionAsset::starter("Player")
        .save(&root.join(ACTIONS))
        .unwrap();
    root
}

fn attach(scene: &mut Scene, e: Entity, file: &str) {
    let mut list = scene
        .world
        .get::<&Scripts>(e)
        .map(|s| s.list.clone())
        .unwrap_or_default();
    list.push(ScriptRef {
        path: format!("Assets/Scripts/{file}"),
        ..Default::default()
    });
    scene.add(e, Scripts { list });
}

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

    /// One frame; errors come back as "ERROR …".
    fn frame(&mut self, input: Input) -> Vec<String> {
        self.time.advance(0.1);
        self.host.update(&mut self.scene, &self.time, input);
        self.host
            .take_logs()
            .into_iter()
            .map(|(k, t)| match k {
                LogKind::Error => format!("ERROR {t}"),
                _ => t,
            })
            .collect()
    }
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

// ---- EventSystem ---------------------------------------------------------------------------

const RUNNER: &str = r#"
using Misty;

public class Runner : MonoBehaviour
{
    int n;

    void Start()
    {
        var es = EventSystem.current;
        Debug.Log("before " + (es.currentSelectedGameObject == null ? "null" : "set"));
        es.SetSelectedGameObject(GameObject.Find("Quit"));
        Debug.Log("now " + es.currentSelectedGameObject.name);
        es.firstSelectedGameObject = GameObject.Find("Play");
        Debug.Log("first " + es.firstSelectedGameObject.name + " nav " + es.sendNavigationEvents + " es " + es.gameObject.name);
    }

    void Update()
    {
        n++;
        var es = EventSystem.current;
        if (n <= 2) Debug.Log("over " + es.IsPointerOverGameObject());
        if (n == 3) GameObject.Find("Play").GetComponent<Button>().Select();
        if (n == 4) es.SetSelectedGameObject(null);
        if (n == 5)
        {
            es.sendNavigationEvents = false;
            Debug.Log("nav " + es.sendNavigationEvents + " selected " + (es.currentSelectedGameObject == null ? "null" : "set"));
        }
    }
}
"#;

const LOGGER: &str = r#"
using Misty;

public class Logger : MonoBehaviour, ISelectHandler
{
    public void OnSelect(BaseEventData data)
    {
        Debug.Log("select " + name + " data " + (data.selectedObject == null ? "null" : data.selectedObject.name));
    }

    void OnDeselect() { Debug.Log("deselect " + name); }
}
"#;

/// A canvas with buttons "Play" (above the centre) and "Quit" (below), each with Logger.cs,
/// and an Event System.
fn menu() -> (Scene, Entity, Entity, Entity) {
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
        attach(&mut s, b, "Logger.cs");
        b
    };
    let play = button("Play", 40.0);
    let quit = button("Quit", -40.0);
    let es = s.spawn("EventSystem", None);
    s.add(es, EventSystem::default());
    (s, play, quit, es)
}

fn pointer(at: Vec2) -> Input {
    Input {
        screen_size: Vec2::new(800.0, 600.0),
        mouse_position: Some(at),
        ..Default::default()
    }
}

#[test]
fn csharp_event_system_selection_and_on_select() {
    if skip() {
        return;
    }
    let root = project("es", &[("Runner.cs", RUNNER), ("Logger.cs", LOGGER)]);
    let (mut scene, play, quit, es) = menu();
    let runner = scene.spawn("Runner", None);
    attach(&mut scene, runner, "Runner.cs");
    let mut rig = Rig::new(scene, root);
    // Start selects Quit before the first UI pass; the mouse is over Quit (y down from the
    // top: the centre is 300, Quit 40 below it).
    assert_eq!(
        rig.frame(pointer(Vec2::new(400.0, 340.0))),
        [
            "before null",
            "now Quit",
            "first Play nav True es EventSystem",
            "select Quit data Quit",
            "over True",
        ]
    );
    assert_eq!(rig.host.ui_interaction().selected, Some(quit));
    assert_eq!(rig.frame(pointer(Vec2::new(5.0, 5.0))), ["over False"]);
    // Frame 3's Update calls Play's Button.Select(); it takes effect in frame 4's UI pass.
    assert!(rig.frame(pointer(Vec2::new(5.0, 5.0))).is_empty());
    assert_eq!(
        rig.frame(pointer(Vec2::new(5.0, 5.0))),
        ["deselect Quit", "select Play data Play"]
    );
    assert_eq!(rig.host.ui_interaction().selected, Some(play));
    // Frame 4 cleared it; frame 5 turns navigation off.
    assert_eq!(
        rig.frame(pointer(Vec2::new(5.0, 5.0))),
        ["deselect Play", "nav False selected null"]
    );
    assert_eq!(rig.host.ui_interaction().selected, None);
    let settings = rig.scene.world.get::<&EventSystem>(es).unwrap().clone();
    assert!(!settings.send_navigation_events);
    assert_eq!(settings.first_selected, Some(play));
}

// ---- touches -------------------------------------------------------------------------------

const TOUCH_READER: &str = r#"
using Misty;

public class TouchReader : MonoBehaviour
{
    void Update()
    {
        var ts = Touchscreen.current;
        Debug.Log($"count {Input.touchCount} supported {Input.touchSupported} screen {(ts == null ? "none" : "yes")}");
        if (Input.touchCount > 0)
        {
            var t = Input.GetTouch(0);
            Debug.Log($"touch {t.fingerId} {t.phase} {t.position} {t.deltaPosition} n {Input.touches.Length}");
            var p = ts.primaryTouch;
            Debug.Log($"primary {p.touchId.ReadValue()} {p.position.ReadValue()} began {p.press.wasPressedThisFrame} down {p.press.isPressed} {p.phase.ReadValue()} all {ts.touches.Length}");
        }
        try { Input.GetTouch(5); } catch (System.ArgumentException) { Debug.Log("no touch 5"); }
    }
}
"#;

fn touches(list: &[(u64, Vec2, Vec2, TouchPhase)]) -> Input {
    Input {
        screen_size: Vec2::new(800.0, 600.0),
        touches: list
            .iter()
            .map(|(id, p, d, phase)| Touch {
                finger_id: *id,
                position: *p,
                delta: *d,
                phase: *phase,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn csharp_touches_read_like_unity_from_the_bottom_left() {
    if skip() {
        return;
    }
    let root = project("touch", &[("TouchReader.cs", TOUCH_READER)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Reader", None);
    attach(&mut scene, e, "TouchReader.cs");
    let mut rig = Rig::new(scene, root);
    assert_eq!(
        rig.frame(touches(&[])),
        ["count 0 supported False screen none", "no touch 5"]
    );
    let p = Vec2::new(100.0, 150.0);
    assert_eq!(
        rig.frame(touches(&[(3, p, Vec2::ZERO, TouchPhase::Began)])),
        [
            "count 1 supported True screen yes",
            "touch 3 Began (100.00, 450.00) (0.00, 0.00) n 1",
            "primary 3 (100.00, 450.00) began True down True Began all 1",
            "no touch 5",
        ]
    );
    // Window y grows down; Unity's grows up: a drag 2 px up the window is +2.
    assert_eq!(
        rig.frame(touches(&[(
            3,
            Vec2::new(105.0, 148.0),
            Vec2::new(5.0, -2.0),
            TouchPhase::Moved
        )])),
        [
            "count 1 supported True screen yes",
            "touch 3 Moved (105.00, 452.00) (5.00, 2.00) n 1",
            "primary 3 (105.00, 452.00) began False down True Moved all 1",
            "no touch 5",
        ]
    );
    let logs = rig.frame(touches(&[(
        3,
        Vec2::new(105.0, 148.0),
        Vec2::ZERO,
        TouchPhase::Ended,
    )]));
    assert_eq!(
        logs[2],
        "primary 3 (105.00, 452.00) began False down False Ended all 1"
    );
}

// ---- Player Input Manager and split screen ---------------------------------------------------

const LOBBY: &str = r#"
using Misty;

public class Lobby : MonoBehaviour
{
    void Start()
    {
        var m = PlayerInputManager.instance;
        Debug.Log($"lobby {m.playerCount} split {m.splitScreen} max {m.maxPlayerCount} joining {m.joiningEnabled} {m.joinBehavior} screens {m.fixedNumberOfSplitScreens} same {m == GetComponent<PlayerInputManager>()}");
        m.onPlayerJoined += p => Debug.Log("event joined " + p.playerIndex);
        m.onPlayerLeft += p => Debug.Log("event left");
    }

    void OnPlayerJoined(PlayerInput p)
    {
        var cam = p.camera;
        var pad = p.GetDevice<Gamepad>();
        Debug.Log($"joined {p.name} index {p.playerIndex} cam {cam.name} rect {cam.rect} devices {string.Join(",", System.Array.ConvertAll(p.devices, d => d.name))} pad {(pad == null ? "none" : pad.name)} all {PlayerInput.all.Length} count {PlayerInputManager.instance.playerCount}");
    }

    void OnPlayerLeft(PlayerInput p) { Debug.Log("left " + (p == null ? "null" : "alive")); }

    int n;

    // Steps by frame: any key pressed on the keyboard would join a keyboard player.
    void Update()
    {
        n++;
        var m = PlayerInputManager.instance;
        if (n == 5) { m.splitScreen = false; Debug.Log("split " + m.splitScreen); }
        if (n == 6)
        {
            var cam = PlayerInput.GetPlayerByIndex(1).camera;
            cam.rect = new Rect(0.25f, 0.25f, 0.5f, 0.5f);
            Debug.Log("rect " + cam.rect + " pixels " + cam.pixelRect);
        }
        if (n == 8) Destroy(PlayerInput.GetPlayerByIndex(0).gameObject);
        if (n == 10) { var p = m.JoinPlayer(); Debug.Log("added " + p.name); }
        if (n == 12) { m.DisableJoining(); Debug.Log("joining " + m.joiningEnabled); }
    }
}
"#;

fn manager_project() -> PathBuf {
    let root = project("manager", &[("Lobby.cs", LOBBY)]);
    let mut asset = Scene::new("asset");
    let player = asset.spawn("Player", None);
    asset.add(
        player,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            ..Default::default()
        },
    );
    let cam = asset.spawn("Cam", Some(player));
    asset.add(cam, Camera::default());
    crate::scene::prefab::Prefab::from_scene(&asset, player)
        .save(&root.join("Assets/Prefabs/Player.prefab"))
        .unwrap();
    root
}

fn pad(name: &str, south: bool) -> Joystick {
    let mut j = Joystick {
        name: name.into(),
        ..Default::default()
    };
    j.held[0] = south;
    j.pressed[0] = south;
    j
}

fn pads(a: bool, b: bool) -> Input {
    Input {
        joysticks: vec![pad("Pad A", a), pad("Pad B", b)],
        ..Default::default()
    }
}

fn players(scene: &Scene) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.world.get::<&PlayerInput>(*e).is_ok())
        .collect()
}

fn viewport(scene: &Scene, player: Entity) -> [f32; 4] {
    let cam = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| scene.is_ancestor(player, *e) && scene.world.get::<&Camera>(*e).is_ok())
        .unwrap();
    scene.world.get::<&Camera>(cam).unwrap().viewport
}

#[test]
fn csharp_player_input_manager_joins_players_with_split_screen_rects() {
    if skip() {
        return;
    }
    let root = manager_project();
    let mut scene = Scene::new("Lobby");
    let m = scene.spawn("Manager", None);
    scene.add(
        m,
        PlayerInputManager {
            player_prefab: Some("Assets/Prefabs/Player.prefab".into()),
            max_player_count: 3,
            ..Default::default()
        },
    );
    attach(&mut scene, m, "Lobby.cs");
    let mut rig = Rig::new(scene, root);
    assert_eq!(
        rig.frame(pads(false, false)),
        [
            "lobby 0 split True max 3 joining True JoinPlayersWhenButtonIsPressed screens -1 same True"
        ]
    );
    // Pad B presses first: player 1 is Pad B's, with the whole screen.
    assert_eq!(
        rig.frame(pads(false, true)),
        [
            "joined Player(Clone) index 0 cam Cam rect (x:0.00, y:0.00, width:1.00, height:1.00) devices Pad B pad Pad B all 1 count 1",
            "event joined 0",
        ]
    );
    rig.frame(pads(false, false));
    // Pad A: player 2, side by side.
    assert_eq!(
        rig.frame(pads(true, false)),
        [
            "joined Player(Clone) index 1 cam Cam rect (x:0.50, y:0.00, width:0.50, height:1.00) devices Pad A pad Pad A all 2 count 2",
            "event joined 1",
        ]
    );
    let p = players(&rig.scene);
    assert_eq!(viewport(&rig.scene, p[0]), [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(viewport(&rig.scene, p[1]), [0.5, 0.0, 0.5, 1.0]);
    // Split screen off from C#: the rects stay as they are and can be set by hand.
    assert_eq!(rig.frame(pads(false, false)), ["split False"]);
    assert!(
        !rig.scene
            .world
            .get::<&PlayerInputManager>(m)
            .unwrap()
            .split_screen
    );
    assert_eq!(
        rig.frame(pads(false, false)),
        [
            "rect (x:0.25, y:0.25, width:0.50, height:0.50) pixels (x:480.00, y:270.00, width:960.00, height:540.00)"
        ]
    );
    rig.frame(pads(false, false));
    assert_eq!(viewport(&rig.scene, p[1]), [0.25, 0.25, 0.5, 0.5]);
    // Player 1 leaves (destroyed): the manager hears it with the gone player.
    assert!(rig.frame(pads(false, false)).is_empty());
    assert_eq!(rig.frame(pads(false, false)), ["left null", "event left"]);
    // A script joins one; then joining is turned off and a free pad doesn't join.
    assert_eq!(rig.frame(pads(false, false)), ["added Player(Clone)"]);
    let logs = rig.frame(pads(false, false));
    assert_eq!(logs.len(), 2, "{logs:?}");
    assert!(
        logs[0].starts_with("joined Player(Clone) index 1"),
        "{logs:?}"
    );
    assert_eq!(players(&rig.scene).len(), 2);
    assert_eq!(rig.frame(pads(false, false)), ["joining False"]);
    rig.frame(pads(false, false));
    assert!(rig.frame(pads(true, true)).is_empty());
    assert_eq!(players(&rig.scene).len(), 2);
}

// ---- Player Input messages, action events, polling and devices -------------------------------

const PLAYER_CTL: &str = r#"
using Misty;

public class PlayerCtl : MonoBehaviour
{
    PlayerInput pi;
    InputAction jump, move;

    void Start()
    {
        pi = GetComponent<PlayerInput>();
        jump = pi.actions.FindAction("Player/Jump");
        move = pi.actions["Move"];
        jump.started += ctx => Debug.Log($"started {ctx.phase} {ctx.control.path}");
        jump.performed += OnJumpPerformed;
        jump.canceled += ctx => Debug.Log("canceled " + ctx.canceled);
        pi.onActionTriggered += ctx =>
        {
            if (ctx.performed && ctx.action.name == "Move") Debug.Log("triggered " + ctx.action + " " + ctx.ReadValue<Vector2>());
        };
        Debug.Log($"scheme {pi.currentControlScheme} map {pi.currentActionMap.name} index {pi.playerIndex} same {pi.actions.FindAction("Jump") == jump} type {jump.type} maps {pi.actions.actionMaps.Length} schemes {pi.actions.controlSchemes.Length} active {pi.inputIsActive} behaviour {pi.notificationBehavior} missing {pi.actions.FindAction("Nope") == null}");
    }

    void OnJumpPerformed(InputAction.CallbackContext ctx) => Debug.Log($"performed {ctx.ReadValue<float>()} {ctx.ReadValueAsButton()}");

    void OnMove(InputValue v) => Debug.Log("move " + v.Get<Vector2>());

    void OnJump(InputValue v) => Debug.Log("jump " + v.isPressed);

    void Update()
    {
        Debug.Log($"poll {jump.WasPressedThisFrame()} {jump.IsPressed()} {jump.WasReleasedThisFrame()} {move.ReadValue<Vector2>().y:F1}");
        var kb = Keyboard.current;
        if (kb.spaceKey.wasPressedThisFrame) Debug.Log($"kb space {kb[Key.W].isPressed} {kb.aKey.isPressed}");
        if (Input.GetKeyDown("tab"))
        {
            pi.SwitchCurrentActionMap("UI");
            Debug.Log("map " + pi.currentActionMap.name + " jump enabled " + jump.enabled);
        }
    }
}
"#;

#[test]
fn csharp_player_input_messages_events_and_polling() {
    if skip() {
        return;
    }
    let root = project("messages", &[("PlayerCtl.cs", PLAYER_CTL)]);
    let mut scene = Scene::new("t");
    let player = scene.spawn("Player", None);
    scene.add(
        player,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            default_map: "Player".into(),
            ..Default::default()
        },
    );
    attach(&mut scene, player, "PlayerCtl.cs");
    let mut rig = Rig::new(scene, root);
    let logs = rig.frame(keys(&[], &["w", "space"]));
    assert_eq!(
        logs,
        [
            "scheme Keyboard&Mouse map Player index 0 same True type Button maps 2 schemes 3 active True behaviour SendMessages missing True",
            "triggered Player/Move (0.00, 1.00)",
            "move (0.00, 1.00)",
            "started Started <Keyboard>/space",
            "performed 1 True",
            "jump True",
            "poll True True False 1.0",
            "kb space True False",
        ]
    );
    assert_eq!(
        rig.frame(keys(&["w", "space"], &["w", "space"])),
        ["poll False True False 1.0"]
    );
    assert_eq!(
        rig.frame(keys(&["w", "space"], &[])),
        [
            "move (0.00, 0.00)",
            "canceled True",
            "poll False False True 0.0"
        ]
    );
    // The UI map: Jump is off.
    let logs = rig.frame(keys(&[], &["tab"]));
    assert!(
        logs.contains(&"map UI jump enabled False".to_owned()),
        "{logs:?}"
    );
}

const FIRE: &str = r#"
using Misty;

public class Fire : MonoBehaviour
{
    InputActionAsset shared;

    void Start()
    {
        shared = InputActionAsset.Load("Assets/Player.inputactions");
        shared.Enable();
        Debug.Log($"loaded {shared.name} enabled {shared.enabled} {shared.FindActionMap("UI").actions.Length}");
        try { InputActionAsset.Load("Assets/Nope.inputactions"); }
        catch (System.InvalidOperationException) { Debug.Log("missing asset"); }
    }

    void OnInteract(InputAction.CallbackContext ctx) =>
        Debug.Log($"interact {ctx.phase} {ctx.duration:F1} {ctx.interactionName}");

    void Update()
    {
        var pad = Gamepad.current;
        if (pad != null && pad.buttonSouth.wasPressedThisFrame)
            Debug.Log($"pad {pad.name} stick {pad.leftStick.ReadValue()} trigger {pad.rightTrigger.ReadValue():F1} dpad {pad.dpad.ReadValue()} up {pad[GamepadButton.DpadUp].isPressed} all {Gamepad.all.Length} same {pad == Gamepad.all[0]}");
        var mouse = Mouse.current;
        if (mouse.leftButton.wasPressedThisFrame)
            Debug.Log($"mouse {mouse.position.ReadValue()} jump {shared["Jump"].ReadValue<float>()}");
    }
}
"#;

#[test]
fn csharp_invoke_events_context_shared_asset_and_devices() {
    if skip() {
        return;
    }
    let root = project("fire", &[("Fire.cs", FIRE)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Player", None);
    scene.add(
        e,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            behavior: PlayerInputBehavior::InvokeEvents,
            events: vec![PlayerInputCall {
                action: "Player/Interact".into(),
                target: Some(e),
                method: "OnInteract".into(),
            }],
            ..Default::default()
        },
    );
    attach(&mut scene, e, "Fire.cs");
    let mut rig = Rig::new(scene, root);
    let logs = rig.frame(keys(&[], &[]));
    assert_eq!(logs, ["loaded Player enabled True 5", "missing asset"]);
    // Interact holds for 0.4 s: started at once, performed after the hold.
    let logs = rig.frame(keys(&[], &["e"]));
    assert_eq!(logs, ["interact Started 0.0 Hold"]);
    let mut all = Vec::new();
    for _ in 0..5 {
        all.extend(rig.frame(keys(&["e"], &["e"])));
    }
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0].starts_with("interact Performed 0.4 Hold"), "{all:?}");
    // A gamepad (stick +1 down in joystick axes, so -0.5 reads up) and the mouse.
    let mut j = pad("Pad A", true);
    j.axes[0] = 0.25;
    j.axes[1] = -0.5;
    j.axes[9] = 0.75;
    j.held[11] = true;
    let mut input = keys(&["e"], &[]);
    input.joysticks = vec![j];
    input.screen_size = Vec2::new(800.0, 600.0);
    input.mouse_position = Some(Vec2::new(10.0, 20.0));
    input.mouse_held[0] = true;
    input.mouse_pressed[0] = true;
    input.held.insert("space".into());
    input.pressed.insert("space".into());
    let logs = rig.frame(input);
    assert!(
        logs.contains(
            &"pad Pad A stick (0.25, 0.50) trigger 0.8 dpad (0.00, 1.00) up True all 1 same True"
                .to_owned()
        ),
        "{logs:?}"
    );
    assert!(
        logs.contains(&"mouse (10.00, 580.00) jump 1".to_owned()),
        "{logs:?}"
    );
}
