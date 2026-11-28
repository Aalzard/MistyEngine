//! Player Input Manager: joining by button press, device pairing, split-screen rects and the
//! OnPlayerJoined / OnPlayerLeft messages.

use std::path::PathBuf;

use glam::Vec3;
use hecs::Entity;

use crate::input::Joystick;
use crate::input::actions::{InputActionAsset, PlayerInput, PlayerInputManager};
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{Camera, Scene, ScriptRef, Scripts};
use crate::scripting::{Input, ScriptHost};

const LOBBY: &str = r#"
local L = {}
function L:OnPlayerJoined(p)
    Debug.Log("joined " .. p.name .. " count " .. self.gameObject:GetComponent("PlayerInputManager").playerCount)
    self.first = self.first or p
end
function L:OnPlayerLeft(p)
    Debug.Log("left " .. tostring(p))
end
function L:Update()
    if Input.GetKeyDown("x") and self.first then Destroy(self.first); self.first = nil end
end
return L
"#;

fn project() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_player_manager_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Lobby.lua"), LOBBY).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    let mut asset = Scene::new("asset");
    let player = asset.spawn("Player", None);
    asset.add(
        player,
        PlayerInput {
            actions: Some("Assets/Player.inputactions".into()),
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

struct Rig {
    scene: Scene,
    host: ScriptHost,
    time: Time,
}

impl Rig {
    fn frame(&mut self, a: bool, b: bool, keys: &[&str]) -> Vec<String> {
        self.time.advance(1.0 / 60.0);
        let mut input = Input {
            joysticks: vec![pad("Pad A", a), pad("Pad B", b)],
            ..Default::default()
        };
        for k in keys {
            input.held.insert((*k).into());
            input.pressed.insert((*k).into());
        }
        self.host.update(&mut self.scene, &self.time, input);
        self.host.take_logs().into_iter().map(|(_, t)| t).collect()
    }

    fn players(&self) -> Vec<Entity> {
        self.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.scene.world.get::<&PlayerInput>(*e).is_ok())
            .collect()
    }

    fn viewport(&self, player: Entity) -> [f32; 4] {
        let cam = self
            .scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .find(|e| {
                self.scene.is_ancestor(player, *e) && self.scene.world.get::<&Camera>(*e).is_ok()
            })
            .unwrap();
        self.scene.world.get::<&Camera>(cam).unwrap().viewport
    }

    fn devices(&self, player: Entity) -> String {
        self.host
            .player_input_info(player)
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == "Devices")
            .unwrap()
            .1
    }
}

fn rig(max: u32) -> Rig {
    let root = project();
    let mut scene = Scene::new("Lobby");
    let m = scene.spawn("Manager", None);
    scene.add(
        m,
        PlayerInputManager {
            player_prefab: Some("Assets/Prefabs/Player.prefab".into()),
            max_player_count: max,
            ..Default::default()
        },
    );
    scene.add(
        m,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Lobby.lua".into(),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::ZERO);
    let host = ScriptHost::start(&mut scene, physics, root);
    Rig {
        scene,
        host,
        time: Time::default(),
    }
}

#[test]
fn pressing_a_button_on_a_free_device_joins_a_player_with_it() {
    let mut r = rig(3);
    assert!(r.frame(false, false, &[]).is_empty());
    // Pad B presses first: player 1 is Pad B's.
    assert_eq!(r.frame(false, true, &[]), ["joined Player(Clone) count 1"]);
    r.frame(false, false, &[]);
    let p = r.players();
    assert_eq!(p.len(), 1);
    assert_eq!(r.devices(p[0]), "Gamepad 2 (Pad B)");
    assert_eq!(r.viewport(p[0]), [0.0, 0.0, 1.0, 1.0]);
    // Pad B again: already a player's, nobody joins. Pad A: player 2, side by side.
    assert!(r.frame(false, true, &[]).is_empty());
    assert_eq!(r.frame(true, false, &[]), ["joined Player(Clone) count 2"]);
    r.frame(false, false, &[]);
    let p = r.players();
    assert_eq!(
        r.devices(p[0]),
        "Gamepad 2 (Pad B)",
        "player 1 keeps its pad"
    );
    assert_eq!(r.devices(p[1]), "Gamepad 1 (Pad A)");
    assert_eq!(r.viewport(p[0]), [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(r.viewport(p[1]), [0.5, 0.0, 0.5, 1.0]);
    // The keyboard: player 3, a 2×2 grid. Then the manager is full.
    assert_eq!(
        r.frame(false, false, &["space"]),
        ["joined Player(Clone) count 3"]
    );
    r.frame(false, false, &[]);
    let p = r.players();
    assert_eq!(p.len(), 3);
    assert!(r.devices(p[2]).contains("Keyboard"), "{}", r.devices(p[2]));
    assert_eq!(r.viewport(p[0]), [0.0, 0.5, 0.5, 0.5]);
    assert_eq!(r.viewport(p[2]), [0.0, 0.0, 0.5, 0.5]);
    // Player 1 leaves (destroyed): the others take over the halves.
    let logs = r.frame(false, false, &["x"]);
    assert!(logs.is_empty(), "{logs:?}");
    let logs = r.frame(false, false, &[]);
    assert_eq!(logs, ["left nil"]);
    let p = r.players();
    assert_eq!(p.len(), 2);
    assert_eq!(r.viewport(p[0]), [0.0, 0.0, 0.5, 1.0]);
    assert_eq!(r.devices(p[0]), "Gamepad 1 (Pad A)");
}

#[test]
fn joining_can_be_turned_off_and_players_added_from_lua() {
    let mut r = rig(0);
    let m = r.scene.find_by_name("Manager").unwrap();
    r.scene
        .world
        .get::<&mut PlayerInputManager>(m)
        .unwrap()
        .joining_enabled = false;
    r.frame(false, false, &[]);
    assert!(r.frame(true, false, &[]).is_empty(), "joining is off");
    assert!(r.players().is_empty());
    // A script joins one by hand; it gets the free devices.
    std::fs::write(
        r.host_root().join("Assets/Scripts/Adder.lua"),
        r#"
local A = {}
function A:Start()
    local m = PlayerInputManager.instance
    Debug.Log(tostring(m.joiningEnabled), m.maxPlayerCount)
    local p = m:JoinPlayer()
    Debug.Log("added " .. p.name)
    m:EnableJoining()
end
return A
"#,
    )
    .unwrap();
    let adder = r.scene.spawn("Adder", None);
    r.scene.add(
        adder,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Adder.lua".into(),
                ..Default::default()
            }],
        },
    );
    r.host.add_objects(&mut r.scene, &[adder]);
    let logs = r.frame(false, false, &[]);
    assert!(logs.contains(&"false 0".to_owned()), "{logs:?}");
    assert!(logs.contains(&"added Player(Clone)".to_owned()), "{logs:?}");
    let logs = r.frame(false, false, &[]);
    assert_eq!(logs, ["joined Player(Clone) count 1"]);
    // Joining is back on: a pad the new player didn't take joins another.
    r.frame(false, false, &[]);
    let p = r.players();
    let mine = r.devices(p[0]);
    let free_pad_b = !mine.contains("Pad B");
    let logs = r.frame(!free_pad_b, free_pad_b, &[]);
    assert_eq!(logs, ["joined Player(Clone) count 2"]);
}

impl Rig {
    fn host_root(&self) -> PathBuf {
        self.host.root.clone()
    }
}

/// Two players joining in the same frame (two pads at once, or two `JoinPlayer` calls) are
/// reported one by one: `playerCount` is 1 in the first `OnPlayerJoined` and 2 in the second,
/// so a lobby script can place each player by its count.
#[test]
fn players_joining_in_one_frame_hear_their_own_count() {
    let mut r = rig(4);
    r.frame(false, false, &[]);
    assert_eq!(
        r.frame(true, true, &[]),
        [
            "joined Player(Clone) count 1",
            "joined Player(Clone) count 2"
        ]
    );
    // After the messages, the count is the real one again.
    std::fs::write(
        r.host_root().join("Assets/Scripts/Count.lua"),
        "local C = {}\nfunction C:Start() Debug.Log(\"now \" .. PlayerInputManager.instance.playerCount) end\nreturn C\n",
    )
    .unwrap();
    let c = r.scene.spawn("Counter", None);
    r.scene.add(
        c,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Count.lua".into(),
                ..Default::default()
            }],
        },
    );
    r.host.add_objects(&mut r.scene, &[c]);
    assert_eq!(r.frame(false, false, &[]), ["now 2"]);
}
