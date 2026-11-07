//! In-process multiplayer: a host and clients, each a full game runtime, connected over
//! in-memory links (and once over real UDP), with Lua scripts using the network API.

use std::path::PathBuf;

use glam::Vec3;

use crate::audio::Audio;
use crate::network::transport::{MemoryNet, Simulator};
use crate::network::{AutoStart, NetAuthority, NetworkManager, NetworkObject, NetworkTransform};
use crate::runtime::{Runtime, Time};
use crate::scene::prefab::Prefab;
use crate::scene::{Scene, ScriptRef, Scripts};
use crate::scripting::{Input, LogKind};

const PLAYER: &str = r#"
local P = { score = 0, nickname = "", speed = 4, serverScale = 1 }
P._attributes = {
    score = NetworkVariable({ onChanged = "OnScoreChanged" }),
    nickname = NetworkVariable({ write = "Owner", onChanged = "OnNickChanged" }),
    secret = NetworkVariable({ read = "Owner" }),
}
P.secret = ""
function P:OnNetworkSpawn()
    Debug.Log("spawn", self.OwnerClientId, tostring(self.IsOwner), tostring(self.IsLocalPlayer))
    if self.IsOwner then self.nickname = "P" .. Network.LocalClientId end
    if self.IsServer then self.secret = "for " .. self.OwnerClientId end
end
function P:OnNetworkDespawn() Debug.Log("despawn", self.OwnerClientId) end
function P:OnScoreChanged(old, new) Debug.Log("score", old, new, Network.LocalClientId) end
function P:OnNickChanged(old, new) Debug.Log("nick", new, Network.LocalClientId) end
function P:Update()
    if self.IsOwner then
        if Input.GetKeyDown("h") then self:HitServerRpc(2) end
        if Input.GetKey("d") then
            local delta = Vector3.new(self.speed * Time.deltaTime, 0, 0)
            self.transform.position = self.transform.position + delta
            if not self.IsServer then self:MoveServerRpc(delta) end
        end
        if Input.GetKeyDown("x") then self.score = 99 end
    end
    if self.IsServer and Input.GetKeyDown("k") then self.serverScale = 0.5 end
    if self.IsServer and Input.GetKeyDown("f") and not self.IsOwner then
        self:FlashClientRpc("boom", RpcTarget.Owner)
    end
end
function P:HitServerRpc(n)
    self.score = self.score + n
    Debug.Log("hit from", Network.SenderClientId)
end
function P:MoveServerRpc(delta)
    self.transform.position = self.transform.position + delta * self.serverScale
end
function P:FlashClientRpc(msg) Debug.Log("flash", msg, Network.LocalClientId) end
return P
"#;

const PICKUP: &str = r#"
local Pickup = { speed = 0 }
function Pickup:Update()
    if self.IsServer then
        self.transform.position = self.transform.position + Vector3.new(self.speed * Time.deltaTime, 0, 0)
        if Input.GetKeyDown("o") then self.NetworkObject:ChangeOwnership(1) end
    end
end
function Pickup:OnGainedOwnership() Debug.Log("gained", Network.LocalClientId) end
function Pickup:OnLostOwnership() Debug.Log("lost", Network.LocalClientId) end
function Pickup:OnOwnershipChanged(old, new) Debug.Log("owner", old, new) end
return Pickup
"#;

const PREFAB: &str = "Assets/Prefabs/NetPlayer.prefab";

fn script(path: &str, fields: &[(&str, crate::scene::ScriptValue)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            enabled: true,
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }],
    }
}

/// A project with the player prefab and scripts, and its scene (host and clients load copies
/// of the same scene, as from one file).
fn project(name: &str, port: u16, predict: bool) -> (PathBuf, Scene) {
    let root = std::env::temp_dir().join(format!("misty_net_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/NetPlayer.lua"), PLAYER).unwrap();
    std::fs::write(root.join("Assets/Scripts/Pickup.lua"), PICKUP).unwrap();
    let mut p = Scene::new("prefab");
    let e = p.spawn("Net Player", None);
    p.add(e, NetworkObject::default());
    p.add(
        e,
        NetworkTransform {
            authority: if predict {
                NetAuthority::Server
            } else {
                NetAuthority::Owner
            },
            predict,
            ..Default::default()
        },
    );
    p.add(e, script("Assets/Scripts/NetPlayer.lua", &[]));
    Prefab::from_scene(&p, e).save(&root.join(PREFAB)).unwrap();

    let mut s = Scene::new("Net");
    s.set_scene_path(0, "Assets/Scenes/Net.scene");
    let m = s.spawn("Network Manager", None);
    s.add(
        m,
        NetworkManager {
            port,
            player_prefab: Some(PREFAB.into()),
            tick_rate: 60,
            ..Default::default()
        },
    );
    let pickup = s.spawn("Pickup", None);
    s.add(pickup, NetworkObject::default());
    s.add(pickup, NetworkTransform::default());
    s.add(pickup, script("Assets/Scripts/Pickup.lua", &[]));
    (root, s)
}

/// One game: its runtime, scene and clock.
struct Peer {
    rt: Runtime,
    scene: Scene,
    time: Time,
    logs: Vec<String>,
}

impl Peer {
    fn new(root: &PathBuf, scene: &Scene, net: &MemoryNet) -> Self {
        let mut scene = scene.clone();
        let rt = Runtime::start_with_audio(
            &mut scene,
            root.clone(),
            &Default::default(),
            Audio::null(root.clone()).0,
        );
        rt.with_network(|s| s.config.memory = Some(net.clone()));
        Self {
            rt,
            scene,
            time: Time::default(),
            logs: Vec::new(),
        }
    }

    fn start(&mut self, how: AutoStart) {
        self.rt.network_start(&mut self.scene, &how).unwrap();
        self.collect();
    }

    fn frame(&mut self, input: Input) {
        self.time.advance(1.0 / 60.0);
        self.rt.update(&mut self.scene, &self.time, input);
        self.collect();
    }

    fn collect(&mut self) {
        for (kind, text) in self.rt.take_logs() {
            assert_ne!(kind, LogKind::Error, "{text}");
            self.logs.push(text);
        }
    }

    fn has(&self, line: &str) -> bool {
        self.logs.iter().any(|l| l == line)
    }

    fn named(&self, name: &str) -> Vec<hecs::Entity> {
        self.scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| self.scene.name(*e) == name)
            .collect()
    }

    fn x(&self, name: &str) -> f32 {
        self.scene.world_position(self.named(name)[0]).x
    }
}

fn keys(pressed: &[&str], held: &[&str]) -> Input {
    let mut i = Input::default();
    for k in pressed {
        i.pressed.insert((*k).into());
        i.held.insert((*k).into());
    }
    for k in held {
        i.held.insert((*k).into());
    }
    i
}

/// Run every peer `frames` times; `input` picks each peer's input per frame.
fn run(peers: &mut [&mut Peer], frames: usize) {
    for _ in 0..frames {
        for p in peers.iter_mut() {
            p.frame(Input::default());
        }
    }
}

#[test]
fn host_and_client_connect_spawn_and_sync_variables_and_rpcs() {
    let (root, scene) = project("sync", 7201, false);
    let net = MemoryNet::default();
    let mut host = Peer::new(&root, &scene, &net);
    let mut client = Peer::new(&root, &scene, &net);
    host.start(AutoStart::Host);
    client.start(AutoStart::Client("127.0.0.1:7201".into()));
    run(&mut [&mut host, &mut client], 30);

    // Both players exist on both sides; each side owns its own.
    assert_eq!(host.named("Net Player(Clone)").len(), 2, "{:?}", host.logs);
    assert_eq!(
        client.named("Net Player(Clone)").len(),
        2,
        "{:?}",
        client.logs
    );
    assert!(host.has("spawn 0 true true"));
    assert!(host.has("spawn 1 false false"));
    assert!(client.has("spawn 1 true true"));
    assert!(client.has("spawn 0 false false"));
    // The client's owner-written nickname reached the server; the host's reached the client.
    assert!(host.has("nick P1 0"), "{:?}", host.logs);
    assert!(client.has("nick P0 1"), "{:?}", client.logs);
    client.rt.with_network(|s| {
        assert_eq!(s.local_client_id, 1);
        assert!(s.is_connected_client());
        assert_eq!(s.objects.len(), 3, "two players and the pickup");
    });
    host.rt
        .with_network(|s| assert_eq!(s.connected_client_ids(), [0, 1]));

    // Server RPC from the client: the server adds to the score, which syncs back.
    client.frame(keys(&["h"], &[]));
    run(&mut [&mut host, &mut client], 10);
    assert!(host.has("hit from 1"));
    assert!(host.has("score 0 2 0"));
    assert!(client.has("score 0 2 1"), "{:?}", client.logs);
    // Server RPC from the host on its own player runs at once.
    host.frame(keys(&["h"], &[]));
    run(&mut [&mut host, &mut client], 10);
    assert!(host.has("hit from 0"));
    // Client RPC to the owner only.
    host.frame(keys(&["f"], &[]));
    run(&mut [&mut host, &mut client], 10);
    assert!(client.has("flash boom 1"));
    assert!(!host.has("flash boom 0"), "targets the owner only");
    // A client may not write a server variable: undone with a warning.
    client.frame(keys(&["x"], &[]));
    run(&mut [&mut host, &mut client], 10);
    assert!(
        client
            .logs
            .iter()
            .any(|l| l.contains("only the server may write")),
        "{:?}",
        client.logs
    );
    assert!(
        !host
            .logs
            .iter()
            .any(|l| l.starts_with("score") && l.contains("99"))
    );
    // Owner-only variables reach the owner, not the others.
    client.rt.with_network(|s| {
        let mine = s
            .objects
            .values()
            .find(|o| o.owner == 1 && o.is_player)
            .unwrap();
        let theirs = s
            .objects
            .values()
            .find(|o| o.owner == 0 && o.is_player)
            .unwrap();
        let secret = |o: &crate::network::session::NetObj| {
            o.behaviours[0]
                .vars
                .iter()
                .find(|v| v.field == "secret")
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(secret(mine), crate::network::NetValue::Text("for 1".into()));
        assert_eq!(
            secret(theirs),
            crate::network::NetValue::Text(String::new())
        );
    });
}

#[test]
fn transforms_interpolate_and_ownership_moves() {
    let (root, scene) = project("motion", 7202, false);
    let pickup = scene.find_by_name("Pickup").unwrap();
    scene.world.get::<&mut Scripts>(pickup).unwrap().list[0]
        .fields
        .insert("speed".into(), crate::scene::ScriptValue::Number(3.0));
    let net = MemoryNet::default();
    let mut host = Peer::new(&root, &scene, &net);
    let mut client = Peer::new(&root, &scene, &net);
    host.start(AutoStart::Host);
    client.start(AutoStart::Client("127.0.0.1:7202".into()));
    run(&mut [&mut host, &mut client], 30);
    // The server moves the pickup at 3 m/s: the client follows smoothly, a little behind.
    let mut last = client.x("Pickup");
    for _ in 0..60 {
        host.frame(Input::default());
        client.frame(Input::default());
        let x = client.x("Pickup");
        assert!(x >= last - 1e-5, "never jumps back: {x} after {last}");
        last = x;
    }
    let lag = host.x("Pickup") - client.x("Pickup");
    assert!(
        (0.1..0.6).contains(&lag),
        "about the interpolation delay behind: {lag}"
    );

    // The owner (client) moves its player with owner authority: the host follows.
    let before = host.x("Net Player(Clone)");
    for _ in 0..30 {
        client.frame(keys(&[], &["d"]));
        host.frame(Input::default());
    }
    run(&mut [&mut host, &mut client], 30);
    let mine = client.rt.with_network(|s| s.local_player).unwrap();
    let on_client = client.rt.with_network(|s| s.objects[&mine].entity);
    let on_host = host.rt.with_network(|s| s.objects[&mine].entity);
    let (cx, hx) = (
        client.scene.world_position(on_client).x,
        host.scene.world_position(on_host).x,
    );
    assert!(
        (cx - 2.0).abs() < 0.1,
        "moved 4 m/s for half a second: {cx}"
    );
    assert!((hx - cx).abs() < 0.01, "host caught up: {hx} vs {cx}");
    assert!(before.abs() < 1e-4);

    // Ownership of the pickup goes to the client.
    host.frame(keys(&["o"], &[]));
    run(&mut [&mut host, &mut client], 10);
    assert!(client.has("gained 1"), "{:?}", client.logs);
    assert!(host.has("lost 0"));
    assert!(host.has("owner 0 1") && client.has("owner 0 1"));
}

#[test]
fn prediction_moves_at_once_and_reconciles_with_the_server() {
    let (root, scene) = project("predict", 7203, true);
    let net = MemoryNet::default();
    let mut host = Peer::new(&root, &scene, &net);
    let mut client = Peer::new(&root, &scene, &net);
    host.start(AutoStart::Host);
    client.start(AutoStart::Client("127.0.0.1:7203".into()));
    run(&mut [&mut host, &mut client], 30);
    for p in [&mut host, &mut client] {
        p.rt.with_network(|s| {
            s.set_simulator(Simulator {
                latency: 0.05,
                ..Default::default()
            })
        });
    }
    let mine = client.rt.with_network(|s| s.local_player).unwrap();
    let on_client = client.rt.with_network(|s| s.objects[&mine].entity);
    let on_host = host.rt.with_network(|s| s.objects[&mine].entity);
    // The client moves right away, before any round trip.
    client.frame(keys(&[], &["d"]));
    host.frame(Input::default());
    assert!(client.scene.world_position(on_client).x > 0.05);
    for _ in 0..29 {
        client.frame(keys(&[], &["d"]));
        host.frame(Input::default());
    }
    run(&mut [&mut host, &mut client], 40);
    let cx = client.scene.world_position(on_client).x;
    let hx = host.scene.world_position(on_host).x;
    assert!((cx - 2.0).abs() < 0.05, "{cx}");
    assert!((hx - cx).abs() < 1e-3, "server agrees: {hx} vs {cx}");

    // The server only accepts half of each move: the client is corrected to the server.
    host.frame(keys(&["k"], &[]));
    client.frame(Input::default());
    for _ in 0..30 {
        client.frame(keys(&[], &["d"]));
        host.frame(Input::default());
    }
    run(&mut [&mut host, &mut client], 40);
    let cx = client.scene.world_position(on_client).x;
    let hx = host.scene.world_position(on_host).x;
    assert!((hx - 3.0).abs() < 0.05, "server took half of 2 m: {hx}");
    assert!((hx - cx).abs() < 1e-3, "client reconciled: {hx} vs {cx}");
}

#[test]
fn disconnects_timeouts_and_bad_networks() {
    let (root, scene) = project("drop", 7204, false);
    let m = scene.find_by_name("Network Manager").unwrap();
    scene.world.get::<&mut NetworkManager>(m).unwrap().timeout = 1.0;
    let net = MemoryNet::default();
    let mut host = Peer::new(&root, &scene, &net);
    let mut a = Peer::new(&root, &scene, &net);
    let mut b = Peer::new(&root, &scene, &net);
    host.start(AutoStart::Host);
    // A lossy, slow network: everything still arrives.
    for p in [&mut host, &mut a, &mut b] {
        p.rt.with_network(|s| {
            s.config.manager.simulated_latency_ms = 60.0;
            s.config.manager.simulated_jitter_ms = 30.0;
            s.config.manager.simulated_packet_loss = 20.0;
            s.set_simulator(s.config.simulator());
        });
    }
    a.start(AutoStart::Client("127.0.0.1:7204".into()));
    b.start(AutoStart::Client("127.0.0.1:7204".into()));
    for p in [&mut a, &mut b] {
        p.rt.with_network(|s| s.set_simulator(s.config.simulator()));
    }
    run(&mut [&mut host, &mut a, &mut b], 180);
    assert_eq!(host.named("Net Player(Clone)").len(), 3, "{:?}", host.logs);
    assert_eq!(a.named("Net Player(Clone)").len(), 3, "{:?}", a.logs);
    assert_eq!(b.named("Net Player(Clone)").len(), 3);
    a.frame(keys(&["h"], &[]));
    run(&mut [&mut host, &mut a, &mut b], 120);
    assert!(b.has("score 0 2 2"), "b sees a's score: {:?}", b.logs);

    // `a` leaves politely: its player goes everywhere.
    a.rt.network_shutdown(&mut a.scene);
    a.collect();
    assert_eq!(a.named("Net Player(Clone)").len(), 0, "spawned copies go");
    assert_eq!(a.named("Pickup").len(), 1, "scene objects stay");
    run(&mut [&mut host, &mut b], 120);
    assert!(
        host.logs
            .iter()
            .any(|l| l.contains("client 1 disconnected"))
    );
    assert_eq!(host.named("Net Player(Clone)").len(), 2);
    assert_eq!(b.named("Net Player(Clone)").len(), 2);

    // `b` goes silent: the server times it out, and `b` times out the server.
    b.rt.with_network(|s| {
        s.set_simulator(Simulator {
            loss: 1.0,
            ..Default::default()
        })
    });
    run(&mut [&mut host, &mut b], 150);
    assert!(
        host.logs
            .iter()
            .any(|l| l.contains("client 2 disconnected (timed out)")),
        "{:?}",
        host.logs
    );
    assert!(
        b.logs.iter().any(|l| l.contains("timed out")),
        "{:?}",
        b.logs
    );
    assert!(!b.rt.with_network(|s| s.is_running()));
    assert_eq!(host.named("Net Player(Clone)").len(), 1);
}

#[test]
fn lua_network_api_starts_approves_and_reports_events() {
    let lobby = r#"
local Lobby = { role = "host", address = "" }
function Lobby:Start()
    Network.OnServerStarted:AddListener(function() Debug.Log("server started") end)
    Network.OnClientConnected:AddListener(function(id) Debug.Log("connected", id, Network.LocalClientId) end)
    Network.OnClientDisconnected:AddListener(function(id) Debug.Log("disconnected", id) end)
    Network.OnServerStopped:AddListener(function() Debug.Log("server stopped") end)
    if self.role == "host" then
        Network.ConnectionApproval = function(id, payload)
            Debug.Log("asked", id, payload)
            if payload == "let me in" then return true end
            return false, "wrong password"
        end
        Debug.Log("host", tostring(Network.StartHost()), tostring(Network.IsHost), Network.Mode)
    else
        Network.connectionData = self.role == "good" and "let me in" or "nope"
        Debug.Log("client", tostring(Network.StartClient(self.address)), tostring(Network.IsServer))
    end
end
function Lobby:Update()
    if Input.GetKeyDown("q") then
        Debug.Log("ids", table.concat(Network.ConnectedClientsIds, ","))
        Network.Shutdown()
    end
end
return Lobby
"#;
    let (root, base) = project("lua", 7205, false);
    std::fs::write(root.join("Assets/Scripts/Lobby.lua"), lobby).unwrap();
    let m = base.find_by_name("Network Manager").unwrap();
    let make = |role: &str| {
        let mut s = base.clone();
        s.world
            .get::<&mut NetworkManager>(m)
            .unwrap()
            .connection_approval = true;
        let e = s.spawn("Lobby", None);
        s.add(e, script_ref(role));
        s
    };
    fn script_ref(role: &str) -> Scripts {
        script(
            "Assets/Scripts/Lobby.lua",
            &[
                ("role", crate::scene::ScriptValue::Text(role.into())),
                (
                    "address",
                    crate::scene::ScriptValue::Text("127.0.0.1:7205".into()),
                ),
            ],
        )
    }
    let net = MemoryNet::default();
    let mut host = Peer::new(&root, &make("host"), &net);
    let mut good = Peer::new(&root, &make("good"), &net);
    let mut bad = Peer::new(&root, &make("bad"), &net);
    run(&mut [&mut host, &mut good, &mut bad], 60);
    assert!(host.has("host true true Host"), "{:?}", host.logs);
    assert!(host.has("server started"));
    assert!(host.has("connected 0 0"), "the host's own client");
    assert!(host.has("asked 1 let me in") || host.has("asked 2 let me in"));
    assert!(good.has("client true false"));
    assert!(
        good.logs
            .iter()
            .any(|l| l.starts_with("connected ") && l.ends_with(" 1")
                || l.starts_with("connected ") && l.ends_with(" 2")),
        "{:?}",
        good.logs
    );
    assert!(
        bad.logs.iter().any(|l| l.contains("wrong password")),
        "{:?}",
        bad.logs
    );
    assert!(!bad.rt.with_network(|s| s.is_running()));
    host.frame(keys(&["q"], &[]));
    run(&mut [&mut host, &mut good], 30);
    assert!(
        host.logs.iter().any(|l| l.starts_with("ids 0,")),
        "{:?}",
        host.logs
    );
    assert!(host.has("server stopped"));
    assert!(!host.rt.with_network(|s| s.is_running()));
    assert!(good.logs.iter().any(|l| l.starts_with("disconnected")));
}

#[test]
fn a_real_udp_session_on_localhost() {
    let (root, scene) = project("udp", 0, false);
    let m = scene.find_by_name("Network Manager").unwrap();
    scene
        .world
        .get::<&mut NetworkManager>(m)
        .unwrap()
        .lan_discovery = false;
    let mut host_scene = scene.clone();
    let mut host = Runtime::start_with_audio(
        &mut host_scene,
        root.clone(),
        &Default::default(),
        Audio::null(root.clone()).0,
    );
    host.network_start(&mut host_scene, &AutoStart::Host)
        .unwrap();
    let port = host.with_network(|s| s.local_port()).unwrap();
    let mut client_scene = scene.clone();
    let mut client = Runtime::start_with_audio(
        &mut client_scene,
        root.clone(),
        &Default::default(),
        Audio::null(root.clone()).0,
    );
    client
        .network_start(
            &mut client_scene,
            &AutoStart::Client(format!("127.0.0.1:{port}")),
        )
        .unwrap();
    let start = std::time::Instant::now();
    let mut time = Time::default();
    while start.elapsed().as_secs() < 20 {
        time.advance(1.0 / 60.0);
        host.update(&mut host_scene, &time, Input::default());
        client.update(&mut client_scene, &time, Input::default());
        if client.with_network(|s| s.is_connected_client() && s.objects.len() == 3) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        client.with_network(|s| s.is_connected_client() && s.objects.len() == 3),
        "{:?}",
        client.take_logs()
    );
    let stats = client.network_stats().unwrap();
    assert_eq!(stats.connections, 1);
    let _ = Vec3::ZERO;
}
