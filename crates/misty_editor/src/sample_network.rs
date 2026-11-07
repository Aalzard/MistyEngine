//! The NetworkDemo sample in `SampleProject`: coloured capsules (one per player) moving around a
//! plane, a gold coin the server moves when someone touches it, and a scoreboard with names and
//! scores. Press H to host and J to join (127.0.0.1), or tick players in Window ▸ Multiplayer ▸
//! Multiplayer Play Mode and press Play.
//!
//! - `Net Player.prefab`: Network Object, Network Transform (server authority with client-side
//!   prediction) and `NetPlayer.lua` (owner-written name, server-written score and colour,
//!   `MoveServerRpc`).
//! - `NetCoin.lua`: the server awards points and says so with `CollectedClientRpc`.
//! - `NetHud.lua`: host / join / leave keys and the scoreboard.
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_network_demo`; the other tests
//! check the committed files still match and play a host and a client in one process.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::network::{NetAuthority, NetworkManager, NetworkObject, NetworkTransform};
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts, Transform,
    UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/NetworkDemo.scene";
const PREFAB: &str = "Assets/Prefabs/Net Player.prefab";
const PLAYER: &str = "Assets/Scripts/NetPlayer.lua";
const COIN: &str = "Assets/Scripts/NetCoin.lua";
const HUD: &str = "Assets/Scripts/NetHud.lua";
const FLOOR: &str = "Assets/Materials/Checker.mat";
const WHITE: &str = "Assets/Materials/Lightmap White.mat";
const GOLD: &str = "Assets/Materials/Stress Gold.mat";
const TEAL: &str = "Assets/Materials/Stress Teal.mat";

const PLAYER_LUA: &str = r#"-- NetPlayer: one player's capsule. The owner moves it at once (client-side prediction) and
-- sends the move to the server, which moves the real one; the Network Transform corrects the
-- owner if the server disagrees. The name is written by the owner, score and colour by the server.
local NetPlayer = {
    speed = 5,     -- metres per second
    arena = 9,     -- half the floor's size
}
NetPlayer._attributes = {
    speed = Tooltip("Metres per second"),
    playerName = NetworkVariable({ write = "Owner" }),
    score = NetworkVariable(),
    colorIndex = NetworkVariable({ onChanged = "OnColorChanged" }),
}
NetPlayer.playerName = ""
NetPlayer.score = 0
NetPlayer.colorIndex = 0

local COLORS = {
    { 0.95, 0.35, 0.3 },
    { 0.3, 0.6, 1.0 },
    { 0.4, 0.85, 0.4 },
    { 1.0, 0.8, 0.25 },
}

function NetPlayer:OnNetworkSpawn()
    if self.IsServer then
        local slot = self.OwnerClientId % 4
        self.colorIndex = slot + 1
        self.transform.position = Vector3.new(-3 + slot * 2, 1, -3)
    end
    if self.IsOwner then
        self.playerName = "Player " .. (self.OwnerClientId + 1)
    end
    -- Only your own capsule wears the marker.
    self.transform:Find("You").gameObject:SetActive(self.IsOwner)
    self:OnColorChanged(0, self.colorIndex)
end

function NetPlayer:OnColorChanged(_, index)
    local c = COLORS[index]
    if c then
        self.gameObject:GetComponent("MeshRenderer").material.color = Color.new(c[1], c[2], c[3])
    end
end

function NetPlayer:Update()
    if not self.IsOwner then return end
    local h, v = Input.GetAxis("Horizontal"), Input.GetAxis("Vertical")
    if h == 0 and v == 0 then return end
    local delta = Vector3.new(h, 0, v) * (self.speed * Time.deltaTime)
    self:Move(delta)
    if not self.IsServer then self:MoveServerRpc(delta) end
end

function NetPlayer:Move(delta)
    local p = self.transform.position + delta
    p.x = math.max(-self.arena, math.min(self.arena, p.x))
    p.z = math.max(-self.arena, math.min(self.arena, p.z))
    self.transform.position = p
end

-- Runs on the server for the owner's input; too-long moves are ignored.
function NetPlayer:MoveServerRpc(delta)
    if delta.magnitude > self.speed * 0.1 then return end
    self:Move(delta)
end

return NetPlayer
"#;

const COIN_LUA: &str = r#"-- NetCoin: the server gives a point to the player who touches it and moves it elsewhere;
-- every client hears about it with a client RPC (the coin pops).
local NetCoin = {
    radius = 1.0,   -- pickup distance in metres
    arena = 8,
    spin = 120,     -- degrees per second (only for looks: not replicated)
}

function NetCoin:Update()
    self.transform:Rotate(0, self.spin * Time.deltaTime, 0)
    if self._pop and self._pop > 0 then
        self._pop = self._pop - Time.deltaTime
        local s = 0.6 + math.max(self._pop, 0) * 1.5
        self.transform.localScale = Vector3.new(s, s, s)
    end
    if not self.IsServer then return end
    for _, id in ipairs(Network.ConnectedClientsIds) do
        local go = Network.GetPlayerObject(id)
        if go and (go.transform.position - self.transform.position).magnitude < self.radius then
            local player = go:GetComponent("NetPlayer")
            player.score = player.score + 1
            self:CollectedClientRpc(player.playerName)
            local a = self.arena
            self.transform.position = Vector3.new(math.random() * 2 * a - a, 0.6, math.random() * 2 * a - a)
            return
        end
    end
end

function NetCoin:CollectedClientRpc(name)
    Debug.Log(name .. " picked up the coin")
    self._pop = 0.25
end

return NetCoin
"#;

const HUD_LUA: &str = r#"-- NetHud: host, join or leave with keys, and the scoreboard.
local NetHud = { address = "127.0.0.1" }

function NetHud:Start()
    self._text = self.gameObject:GetComponent("Text")
end

function NetHud:Update()
    if not Network.IsListening then
        if Input.GetKeyDown("h") then Network.StartHost() end
        if Input.GetKeyDown("j") then Network.StartClient(self.address) end
    elseif Input.GetKeyDown("l") then
        Network.Shutdown()
    end
    self._text.text = self:Describe()
end

function NetHud:Describe()
    if not Network.IsListening then
        return "H  host a game      J  join " .. self.address
            .. "\nor tick players in Window > Multiplayer > Multiplayer Play Mode and press Play"
    end
    if not (Network.IsServer or Network.IsConnectedClient) then
        return "Joining " .. self.address .. "..."
    end
    local lines = { Network.Mode .. "  -  WASD move, touch the coin, L leave" }
    local players = {}
    for _, go in ipairs(GameObject.FindGameObjectsWithTag("Player")) do
        local p = go:GetComponent("NetPlayer")
        if p and p.playerName ~= "" then players[#players + 1] = p end
    end
    table.sort(players, function(a, b) return a.score > b.score end)
    for _, p in ipairs(players) do
        lines[#lines + 1] = p.playerName .. "   " .. p.score
    end
    return table.concat(lines, "\n")
end

return NetHud
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            enabled: true,
            fields: Default::default(),
        }],
    }
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<misty_engine::hecs::Entity>,
    p: PrimitiveMesh,
    t: Transform,
    mat: &str,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(p));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

/// The player: a capsule with a marker above it.
fn prefab() -> Prefab {
    let mut s = Scene::new("Net Player");
    let root = mesh(
        &mut s,
        "Net Player",
        None,
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(0.0, 1.0, 0.0)),
        WHITE,
    );
    s.game_object_mut(root).tag = "Player".into();
    s.add(
        root,
        NetworkObject {
            global_id: "6e65747764656d6f706c617965720001".into(),
            destroy_with_owner: true,
        },
    );
    s.add(
        root,
        NetworkTransform {
            authority: NetAuthority::Server,
            predict: true,
            sync_rotation: false,
            ..Default::default()
        },
    );
    s.add(root, script(PLAYER));
    let mut t = Transform::from_position(Vec3::new(0.0, 1.45, 0.0));
    t.scale = Vec3::splat(0.3);
    mesh(&mut s, "You", Some(root), PrimitiveMesh::Sphere, t, TEAL);
    Prefab::from_scene(&s, root)
}

fn demo_scene() -> Scene {
    let mut st = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_network_demo_build"),
    ));
    st.scene = Scene::default_3d("NetworkDemo");
    let s = &mut st.scene;
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 13.0, -12.0))
                .with_euler(Vec3::new(48.0, 0.0, 0.0)),
        );
    }
    let floor = Transform {
        scale: Vec3::new(2.0, 1.0, 2.0),
        ..Default::default()
    };
    mesh(s, "Floor", None, PrimitiveMesh::Plane, floor, FLOOR);
    let manager = s.spawn("Network Manager", None);
    s.add(
        manager,
        NetworkManager {
            player_prefab: Some(PREFAB.into()),
            ..Default::default()
        },
    );
    let mut t = Transform::from_position(Vec3::new(3.0, 0.6, 2.0));
    t.scale = Vec3::splat(0.6);
    let coin = mesh(s, "Coin", None, PrimitiveMesh::Sphere, t, GOLD);
    s.add(
        coin,
        NetworkObject {
            global_id: "6e65747764656d6f636f696e00000001".into(),
            destroy_with_owner: true,
        },
    );
    s.add(
        coin,
        NetworkTransform {
            sync_rotation: false,
            ..Default::default()
        },
    );
    s.add(coin, script(COIN));

    crate::commands::apply(
        &mut st,
        Action::Create {
            what: Create::Ui(UiElement::Text),
            parent: None,
        },
    );
    let text = st.selection.active().expect("created text is selected");
    st.scene.game_object_mut(text).name = "Hud Text".into();
    let w = &mut st.scene.world;
    *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -20.0),
        size_delta: Vec2::new(1000.0, 260.0),
    };
    {
        let mut t = w.get::<&mut UiText>(text).unwrap();
        t.text = "H  host a game      J  join 127.0.0.1".into();
        t.font_size = 24.0;
        t.color = [1.0; 4];
    }
    st.scene.add(text, script(HUD));
    st.scene
}

fn scripts() -> [(&'static str, &'static str); 3] {
    [(PLAYER, PLAYER_LUA), (COIN, COIN_LUA), (HUD, HUD_LUA)]
}

/// A JSON asset without its GUID table (the editor adds one on save).
fn without_guids(text: &str) -> serde_json::Value {
    let mut v: serde_json::Value = serde_json::from_str(text).expect("json");
    if let Some(o) = v.as_object_mut() {
        o.remove(misty_engine::asset_db::refs::KEY);
    }
    v
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_network_demo() {
    let root = project();
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    prefab().save(&root.join(PREFAB)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn network_demo_matches_its_generator() {
    let root = project();
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let text = std::fs::read_to_string(root.join(PREFAB)).unwrap();
    assert_eq!(
        without_guids(&text),
        without_guids(&prefab().to_json()),
        "regenerate with write_network_demo"
    );
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_network_demo"
    );
    let (_, report) = Scene::from_json("NetworkDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// A host and a client in one process (in-memory links): both see both players, the client's
/// predicted moves reach the server, and touching the coin scores on every screen.
#[test]
fn network_demo_plays_host_and_client() {
    use misty_engine::audio::Audio;
    use misty_engine::network::AutoStart;
    use misty_engine::network::transport::MemoryNet;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let settings = ProjectSettings::load(&root).unwrap();
    let net = MemoryNet::default();
    struct Peer {
        rt: Runtime,
        scene: Scene,
        time: Time,
        logs: Vec<String>,
    }
    let mut peers: Vec<Peer> = (0..2)
        .map(|_| {
            let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
            scene.set_scene_path(0, SCENE);
            let rt = Runtime::start_with_audio(
                &mut scene,
                root.clone(),
                &settings,
                Audio::null(root.clone()).0,
            );
            rt.with_network(|s| s.config.memory = Some(net.clone()));
            Peer {
                rt,
                scene,
                time: Time::new(&settings.time),
                logs: Vec::new(),
            }
        })
        .collect();
    let starts = [AutoStart::Host, AutoStart::Client("127.0.0.1:7777".into())];
    for (p, how) in peers.iter_mut().zip(starts) {
        p.rt.network_start(&mut p.scene, &how).unwrap();
    }
    let frames = |peers: &mut Vec<Peer>, n: usize, client_input: &Input| {
        for _ in 0..n {
            for (i, p) in peers.iter_mut().enumerate() {
                p.time.advance(1.0 / 60.0);
                let input = if i == 1 {
                    client_input.clone()
                } else {
                    Input::default()
                };
                p.rt.update(&mut p.scene, &p.time, input);
                for (kind, text) in p.rt.take_logs() {
                    assert_ne!(kind, LogKind::Error, "{text}");
                    p.logs.push(text);
                }
            }
        }
    };
    frames(&mut peers, 40, &Input::default());
    for p in &peers {
        let players = GameObjects::tagged(&p.scene, "Player");
        assert_eq!(players.len(), 2, "{:?}", p.logs);
    }
    let hud = |p: &Peer| {
        let e = p.scene.find_by_name("Hud Text").unwrap();
        p.scene.world.get::<&UiText>(e).unwrap().text.clone()
    };
    assert!(
        hud(&peers[0]).contains("Player 2   0"),
        "{}",
        hud(&peers[0])
    );
    assert!(hud(&peers[1]).starts_with("Client"), "{}", hud(&peers[1]));

    // The client walks diagonally across the coin.
    let mut walk = Input::default();
    walk.held.insert("d".into());
    walk.held.insert("w".into());
    frames(&mut peers, 75, &walk);
    frames(&mut peers, 30, &Input::default());
    assert!(
        peers[1]
            .logs
            .iter()
            .any(|l| l == "Player 2 picked up the coin"),
        "{:?}",
        peers[1].logs
    );
    assert!(
        peers[0]
            .logs
            .iter()
            .any(|l| l == "Player 2 picked up the coin")
    );
    assert!(
        hud(&peers[1]).contains("Player 2   1"),
        "{}",
        hud(&peers[1])
    );
    assert!(
        hud(&peers[0]).contains("Player 2   1"),
        "{}",
        hud(&peers[0])
    );
}

/// Objects with a tag, in Hierarchy order.
struct GameObjects;

impl GameObjects {
    fn tagged(scene: &Scene, tag: &str) -> Vec<misty_engine::hecs::Entity> {
        scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| scene.game_object(*e).tag == tag)
            .collect()
    }
}
