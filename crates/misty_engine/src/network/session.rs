//! One game's network state: the mode it runs in, the transport, connected clients (server)
//! or the connection phase (client), the spawned network objects, playback clocks and stats.
//!
//! The session turns transport traffic into [`Inbound`] items; the script host applies them to
//! the scene and the scripts (spawning, Network Variables, RPCs) and sends through the
//! session's helpers.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;

use hecs::Entity;

use super::components::{NetworkManager, NetworkTransform};
use super::discovery::{DISCOVERY_PORT, DiscoveryClient, DiscoveryResponder, LanGame};
use super::interpolation::{PlaybackClock, PredictionHistory, SnapshotBuffer};
use super::protocol::{Msg, PROTOCOL_VERSION, SpawnMsg, SpawnSource, TransformMsg, VarChange};
use super::transport::{
    Channel, ConnId, ConnStats, Endpoint, Link, MemoryNet, Simulator, TransportConfig,
    TransportEvent, UdpLink,
};
use super::value::NetValue;
use super::{NetVarSpec, RpcSpec, SERVER_CLIENT_ID};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetMode {
    #[default]
    Offline,
    Server,
    /// A server with a local client (client id 0).
    Host,
    Client,
}

impl NetMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offline => "Offline",
            Self::Server => "Server",
            Self::Host => "Host",
            Self::Client => "Client",
        }
    }
}

/// Settings a session starts with (from the scene's Network Manager).
#[derive(Clone, Debug)]
pub struct NetConfig {
    pub manager: NetworkManager,
    /// Sent to the server with the connect request (`Network.connectionData`).
    pub connection_data: String,
    /// Product name: LAN discovery only finds games of the same name.
    pub game_name: String,
    /// In-process links instead of UDP sockets (tests).
    pub memory: Option<MemoryNet>,
    /// Discovery port (tests use a free one).
    pub discovery_port: u16,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            manager: NetworkManager::default(),
            connection_data: String::new(),
            game_name: "Misty Game".into(),
            memory: None,
            discovery_port: DISCOVERY_PORT,
        }
    }
}

impl NetConfig {
    pub fn simulator(&self) -> Simulator {
        let m = &self.manager;
        Simulator {
            latency: (m.simulated_latency_ms / 1000.0).max(0.0),
            jitter: (m.simulated_jitter_ms / 1000.0).max(0.0),
            loss: (m.simulated_packet_loss / 100.0).clamp(0.0, 1.0),
        }
    }
    pub fn tick_interval(&self) -> f64 {
        1.0 / f64::from(self.manager.tick_rate.clamp(1, 240))
    }
}

/// A client as the server sees it.
#[derive(Clone, Debug, Default)]
pub struct ClientInfo {
    /// Transport connection (none for the host's own client).
    pub conn: Option<ConnId>,
    pub approved: bool,
    /// Has the server's scene loaded and gets objects.
    pub ready: bool,
    pub payload: String,
    /// Refused: disconnect at this time (after the reason was delivered).
    pub kick_at: Option<f64>,
    /// Joined (objects sent and `OnClientConnected` raised) at least once.
    pub joined: bool,
}

/// Connection events for the scripts' listeners (`Network.OnClientConnected`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetEvent {
    ServerStarted,
    ClientStarted,
    ServerStopped,
    ClientStopped,
    ClientConnected(u64),
    ClientDisconnected(u64),
}

/// Where a client is in joining.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClientPhase {
    #[default]
    Connecting,
    /// Approved; loading the server's scene.
    LoadingScene,
    /// Waiting for the server's objects.
    Syncing,
    Synced,
}

/// A replicated script field and its last synchronised value.
#[derive(Clone, Debug)]
pub struct NetVar {
    pub field: String,
    pub spec: NetVarSpec,
    pub value: NetValue,
}

/// A script instance of a network object (its scripts in hierarchy order).
#[derive(Clone, Debug)]
pub struct Behaviour {
    pub entity: Entity,
    /// Script name (`GetComponent` name) and file.
    pub script: String,
    pub path: String,
    pub vars: Vec<NetVar>,
    pub rpcs: HashMap<String, RpcSpec>,
}

/// Network Transform state of one object.
#[derive(Clone, Debug, Default)]
pub struct TransformSync {
    pub settings: NetworkTransform,
    pub buffer: SnapshotBuffer,
    /// Values last sent (authority).
    pub last_sent: Option<(glam::Vec3, glam::Quat, glam::Vec3)>,
    pub last_send_time: f64,
    pub last_sent_ack: u32,
    /// Predicting owner: poses by frame.
    pub history: PredictionHistory,
    /// Connection whose clock times the buffer (who sends us this object's states).
    pub clock_conn: Option<ConnId>,
    /// A Rigidbody was made kinematic because another peer moves it.
    pub made_kinematic: bool,
}

/// A spawned network object.
#[derive(Clone, Debug)]
pub struct NetObj {
    pub id: u64,
    pub entity: Entity,
    pub owner: u64,
    pub is_player: bool,
    pub source: SpawnSource,
    pub destroy_with_owner: bool,
    /// Script instances, filled once they exist (after `Awake`).
    pub behaviours: Vec<Behaviour>,
    pub behaviours_ready: bool,
    /// Server: the spawn went out to the ready clients.
    pub announced: bool,
    /// `OnNetworkSpawn` ran.
    pub spawn_called: bool,
    pub transform: Option<TransformSync>,
    /// Server: the owner's last input frame (from its RPCs), acknowledged in states.
    pub last_input_frame: u32,
    /// Variables received with the spawn, applied once the scripts exist.
    pub initial_vars: Vec<VarChange>,
}

/// Traffic for the script host to act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Inbound {
    // Server side.
    /// A client asked to join; decide with [`NetSession::approve`].
    Approve {
        client: u64,
        payload: String,
    },
    /// A client has the scene loaded: send it the objects.
    SceneReady {
        client: u64,
    },
    ClientLeft {
        client: u64,
        reason: String,
    },
    // Client side.
    Welcome {
        client_id: u64,
        scene: Option<String>,
    },
    Denied(String),
    LoadScene(String),
    Spawn(SpawnMsg),
    SyncDone,
    Despawn {
        id: u64,
        destroy: bool,
    },
    Ownership {
        id: u64,
        owner: u64,
    },
    /// Lost the server (or it refused us).
    Disconnected(String),
    // Both.
    Vars {
        from: u64,
        id: u64,
        changes: Vec<VarChange>,
    },
    Rpc {
        from: u64,
        id: u64,
        behaviour: u16,
        method: String,
        args: Vec<NetValue>,
        frame: u32,
    },
    Transform {
        from: u64,
        msg: TransformMsg,
    },
}

/// Traffic over the last second, for the stats overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetStats {
    pub mode: NetMode,
    /// Round trip to the server (client) or the slowest client (server), milliseconds.
    pub rtt_ms: f32,
    pub packets_in: f32,
    pub packets_out: f32,
    pub bytes_in: f32,
    pub bytes_out: f32,
    /// Share of packets never acknowledged, percent.
    pub loss: f32,
    /// Connected remote clients (server), or 1 when connected (client).
    pub connections: usize,
    pub objects: usize,
}

impl NetStats {
    /// "Host · 2 clients · RTT 34 ms · ↓ 60 pkt/s 3.1 KB/s · ↑ 58 pkt/s 2.4 KB/s".
    pub fn lines(&self) -> Vec<String> {
        let who = match self.mode {
            NetMode::Client => "Client".to_owned(),
            m => format!(
                "{} · {} client{}",
                m.label(),
                self.connections,
                if self.connections == 1 { "" } else { "s" }
            ),
        };
        vec![
            format!(
                "Network: {who} · {} objects · RTT {:.0} ms · loss {:.1}%",
                self.objects, self.rtt_ms, self.loss
            ),
            format!(
                "  In {:.0} pkt/s {:.1} KB/s   Out {:.0} pkt/s {:.1} KB/s",
                self.packets_in,
                self.bytes_in / 1024.0,
                self.packets_out,
                self.bytes_out / 1024.0
            ),
        ]
    }
}

/// One game's network state (see the module docs).
#[derive(Default)]
pub struct NetSession {
    pub mode: NetMode,
    pub config: NetConfig,
    endpoint: Option<Endpoint>,
    pub local_client_id: u64,
    /// Network clock (real seconds since the session was created).
    pub now: f64,
    /// Local frame counter (RPCs carry it; prediction keys poses by it).
    pub frame: u32,
    pub phase: ClientPhase,
    /// Client: the server's tick rate.
    pub server_tick_rate: u32,
    /// Server: the scene clients load (sent with `Welcome` and `LoadScene`).
    pub scene_path: Option<String>,
    /// Server: clients by id (the host's own client is 0).
    pub clients: BTreeMap<u64, ClientInfo>,
    conn_client: HashMap<ConnId, u64>,
    next_client_id: u64,
    pub objects: BTreeMap<u64, NetObj>,
    pub by_entity: HashMap<Entity, u64>,
    next_object_id: u64,
    clocks: HashMap<ConnId, PlaybackClock>,
    tick_accum: f64,
    /// This frame sends a network tick (states, variables).
    pub tick_due: bool,
    stats_since: f64,
    stats_base: ConnStats,
    pub stats: NetStats,
    responder: Option<DiscoveryResponder>,
    discovery: Option<DiscoveryClient>,
    /// Problems to show in the Console.
    pub logs: Vec<String>,
    /// Client: our own spawned player object.
    pub local_player: Option<u64>,
    /// Connection events waiting for the scripts.
    pub events: Vec<NetEvent>,
    /// The client id of the caller while a server RPC runs (`Network.SenderClientId`).
    pub rpc_sender: u64,
    /// A script called `Network.Shutdown()`; done after its callback.
    pub shutdown_requested: bool,
    /// Ownership changes (object, previous, new) whose callbacks haven't run yet.
    pub owner_changes: Vec<(u64, u64, u64)>,
    /// Objects to despawn without destroying (`NetworkObject:Despawn(false)`).
    pub despawns: Vec<u64>,
}

impl NetSession {
    pub fn is_server(&self) -> bool {
        matches!(self.mode, NetMode::Server | NetMode::Host)
    }
    pub fn is_client(&self) -> bool {
        matches!(self.mode, NetMode::Client | NetMode::Host)
    }
    pub fn is_host(&self) -> bool {
        self.mode == NetMode::Host
    }
    pub fn is_running(&self) -> bool {
        self.mode != NetMode::Offline
    }
    /// A client that finished joining (always true for a host).
    pub fn is_connected_client(&self) -> bool {
        match self.mode {
            NetMode::Host => true,
            NetMode::Client => self.phase == ClientPhase::Synced,
            _ => false,
        }
    }

    /// The port the server listens on (or the client's own port).
    pub fn local_port(&self) -> Option<u16> {
        self.endpoint.as_ref().map(|e| e.local_addr().port())
    }

    fn link(&self, addr: SocketAddr) -> Result<Box<dyn Link>, String> {
        match &self.config.memory {
            Some(net) => Ok(Box::new(net.link(addr.port()))),
            None => UdpLink::bind(addr)
                .map(|l| Box::new(l) as Box<dyn Link>)
                .map_err(|e| format!("could not open UDP port {}: {e}", addr.port())),
        }
    }

    fn transport_config(&self) -> TransportConfig {
        TransportConfig {
            timeout: self.config.manager.timeout.max(0.5),
            connect_timeout: self.config.manager.timeout.max(0.5),
            ..Default::default()
        }
    }

    /// Start listening as a server (with a local client when `host`).
    pub fn start_server(&mut self, host: bool) -> Result<(), String> {
        if self.is_running() {
            return Err(format!("already running as {}", self.mode.label()));
        }
        let m = &self.config.manager;
        let ip = if self.config.memory.is_some() {
            "127.0.0.1"
        } else {
            m.listen_address.as_str()
        };
        let addr = super::parse_address(ip, m.port).map(|mut a| {
            a.set_port(m.port);
            a
        })?;
        let mut endpoint = Endpoint::server(self.link(addr)?, m.max_connections.max(1) as usize);
        endpoint.simulator = self.config.simulator();
        endpoint.config = self.transport_config();
        let port = endpoint.local_addr().port();
        self.endpoint = Some(endpoint);
        self.mode = if host { NetMode::Host } else { NetMode::Server };
        self.local_client_id = SERVER_CLIENT_ID;
        self.next_client_id = 1;
        self.clients.clear();
        if host {
            self.clients.insert(
                SERVER_CLIENT_ID,
                ClientInfo {
                    conn: None,
                    approved: true,
                    ready: true,
                    ..Default::default()
                },
            );
        }
        self.phase = ClientPhase::Synced;
        if self.config.manager.lan_discovery && self.config.memory.is_none() {
            let bind = SocketAddr::from(([0, 0, 0, 0], self.config.discovery_port));
            let name = host_name();
            match DiscoveryResponder::bind(bind, &self.config.game_name, port, &name) {
                Ok(mut r) => {
                    r.max_players = self.config.manager.max_connections as u16;
                    self.responder = Some(r);
                }
                // Another game on this machine already answers: it is still found by address.
                Err(_) => self.responder = None,
            }
        }
        Ok(())
    }

    /// Connect to a server at `address` ("host[:port]", the Network Manager's port by default).
    pub fn start_client(&mut self, address: &str) -> Result<(), String> {
        if self.is_running() {
            return Err(format!("already running as {}", self.mode.label()));
        }
        let server = if self.config.memory.is_some() {
            let port = address
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(self.config.manager.port);
            SocketAddr::from(([127, 0, 0, 1], port))
        } else {
            super::parse_address(address, self.config.manager.port)?
        };
        let mut endpoint = Endpoint::client(
            self.link(SocketAddr::from(([0, 0, 0, 0], 0)))?,
            server,
            self.now,
        );
        endpoint.simulator = self.config.simulator();
        endpoint.config = self.transport_config();
        self.endpoint = Some(endpoint);
        self.mode = NetMode::Client;
        self.phase = ClientPhase::Connecting;
        self.local_client_id = SERVER_CLIENT_ID;
        Ok(())
    }

    /// Stop networking: connections close; spawned-object bookkeeping is cleared (the script
    /// host despawns the objects first).
    pub fn shutdown(&mut self) {
        self.endpoint = None;
        self.responder = None;
        self.mode = NetMode::Offline;
        self.phase = ClientPhase::Connecting;
        self.clients.clear();
        self.conn_client.clear();
        self.objects.clear();
        self.by_entity.clear();
        self.clocks.clear();
        self.local_player = None;
        self.local_client_id = SERVER_CLIENT_ID;
        self.stats = NetStats::default();
    }

    /// Ids of connected clients (server: approved ones, the host's own included).
    pub fn connected_client_ids(&self) -> Vec<u64> {
        self.clients
            .iter()
            .filter(|(_, c)| c.approved && c.kick_at.is_none())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Remote clients that have the scene and receive objects.
    pub fn ready_remote_clients(&self) -> Vec<u64> {
        self.clients
            .iter()
            .filter(|(_, c)| c.conn.is_some() && c.ready && c.kick_at.is_none())
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn new_object_id(&mut self) -> u64 {
        self.next_object_id += 1;
        self.next_object_id
    }

    /// The network object `e` belongs to: itself or its nearest spawned ancestor.
    pub fn object_of(&self, scene: &crate::scene::Scene, e: Entity) -> Option<u64> {
        let mut cur = Some(e);
        while let Some(x) = cur {
            if let Some(id) = self.by_entity.get(&x) {
                return Some(*id);
            }
            cur = scene.parent(x);
        }
        None
    }

    /// Send `msg` to client `client` (server) or to the server (client).
    pub fn send(&mut self, client: u64, channel: Channel, msg: &Msg) {
        let conn = if self.mode == NetMode::Client {
            Some(0)
        } else {
            self.clients.get(&client).and_then(|c| c.conn)
        };
        if let (Some(conn), Some(e)) = (conn, self.endpoint.as_mut()) {
            e.send(conn, channel, &msg.encode());
        }
    }

    pub fn send_to_server(&mut self, channel: Channel, msg: &Msg) {
        if self.mode == NetMode::Client {
            self.send(SERVER_CLIENT_ID, channel, msg);
        }
    }

    /// Send `msg` to every ready remote client `filter` accepts.
    pub fn broadcast(&mut self, channel: Channel, msg: &Msg, filter: impl Fn(u64) -> bool) {
        let bytes = msg.encode();
        let targets: Vec<ConnId> = self
            .clients
            .iter()
            .filter(|(id, c)| c.ready && c.kick_at.is_none() && filter(**id))
            .filter_map(|(_, c)| c.conn)
            .collect();
        if let Some(e) = self.endpoint.as_mut() {
            for conn in targets {
                e.send(conn, channel, &bytes);
            }
        }
    }

    /// Let a client in (with `Welcome`) or refuse it (it is disconnected shortly after).
    pub fn approve(&mut self, client: u64, approved: bool, reason: &str) {
        let Some(info) = self.clients.get_mut(&client) else {
            return;
        };
        if approved {
            info.approved = true;
            let msg = Msg::Welcome {
                client_id: client,
                scene: self
                    .config
                    .manager
                    .scene_management
                    .then(|| self.scene_path.clone())
                    .flatten(),
                tick_rate: self.config.manager.tick_rate,
            };
            self.send(client, Channel::ReliableOrdered, &msg);
        } else {
            info.kick_at = Some(self.now + 0.5);
            let reason = if reason.is_empty() {
                "connection refused"
            } else {
                reason
            };
            let msg = Msg::Denied {
                reason: reason.to_owned(),
            };
            self.send(client, Channel::ReliableOrdered, &msg);
        }
    }

    /// Disconnect a client (server).
    pub fn disconnect_client(&mut self, client: u64) -> bool {
        let Some(info) = self.clients.remove(&client) else {
            return false;
        };
        if let Some(conn) = info.conn {
            self.conn_client.remove(&conn);
            self.clocks.remove(&conn);
            if let Some(e) = self.endpoint.as_mut() {
                e.disconnect(conn);
            }
        }
        true
    }

    /// The playback clock of states received from `from` (server: a client; client: the
    /// server), sampled with `time`.
    pub fn sample_clock(&mut self, from: u64, time: f64) -> Option<ConnId> {
        let conn = self.conn_of(from)?;
        self.clocks.entry(conn).or_default().sample(time);
        Some(conn)
    }

    pub fn clock_now(&self, conn: Option<ConnId>) -> Option<f64> {
        let c = self.clocks.get(&conn?)?;
        c.started().then(|| c.now())
    }

    fn conn_of(&self, client: u64) -> Option<ConnId> {
        if self.mode == NetMode::Client {
            Some(0)
        } else {
            self.clients.get(&client).and_then(|c| c.conn)
        }
    }

    /// Advance the clock by `dt` real seconds, receive, and turn traffic into [`Inbound`]s.
    pub fn receive(&mut self, dt: f64) -> Vec<Inbound> {
        self.now += dt.max(0.0);
        for c in self.clocks.values_mut() {
            c.advance(dt);
        }
        let mut out = Vec::new();
        if !self.is_running() {
            self.poll_discovery();
            return out;
        }
        let interval = self.config.tick_interval();
        self.tick_accum += dt;
        self.tick_due = self.tick_accum >= interval;
        if self.tick_due {
            self.tick_accum = (self.tick_accum - interval).min(interval);
        }
        // Refused clients go once their reason had time to arrive.
        let kicked: Vec<u64> = self
            .clients
            .iter()
            .filter(|(_, c)| c.kick_at.is_some_and(|t| self.now >= t))
            .map(|(id, _)| *id)
            .collect();
        for id in kicked {
            self.disconnect_client(id);
        }
        let now = self.now;
        let Some(endpoint) = self.endpoint.as_mut() else {
            return out;
        };
        endpoint.update(now);
        let mut events = Vec::new();
        while let Some(e) = endpoint.poll() {
            events.push(e);
        }
        for e in events {
            self.on_transport(e, &mut out);
        }
        if let Some(r) = self.responder.as_mut() {
            r.players = self
                .clients
                .values()
                .filter(|c| c.approved)
                .count()
                .min(u16::MAX as usize) as u16;
            r.poll();
        }
        self.poll_discovery();
        self.update_stats();
        out
    }

    /// Send everything queued this frame.
    pub fn flush(&mut self) {
        let now = self.now;
        if let Some(e) = self.endpoint.as_mut() {
            e.update(now);
        }
        self.frame = self.frame.wrapping_add(1);
    }

    fn on_transport(&mut self, event: TransportEvent, out: &mut Vec<Inbound>) {
        match event {
            TransportEvent::Connected(conn) => {
                if self.mode == NetMode::Client {
                    let msg = Msg::Hello {
                        version: PROTOCOL_VERSION,
                        payload: self.config.connection_data.clone(),
                    };
                    self.send_to_server(Channel::ReliableOrdered, &msg);
                } else {
                    // Waits for its Hello before it becomes a client.
                    self.clocks.remove(&conn);
                }
            }
            TransportEvent::Disconnected(conn, reason) => {
                if self.mode == NetMode::Client {
                    let text = match (reason, self.phase) {
                        (super::transport::DisconnectReason::ConnectFailed, _) => {
                            "could not connect: the server did not answer".to_owned()
                        }
                        _ => format!("disconnected: {}", reason.describe()),
                    };
                    out.push(Inbound::Disconnected(text));
                } else if let Some(client) = self.conn_client.remove(&conn) {
                    self.clocks.remove(&conn);
                    let was_in = self.clients.remove(&client).is_some_and(|c| c.approved);
                    if was_in {
                        out.push(Inbound::ClientLeft {
                            client,
                            reason: reason.describe().to_owned(),
                        });
                    }
                }
            }
            TransportEvent::Message(conn, _, bytes) => {
                let Some(msg) = Msg::decode(&bytes) else {
                    return;
                };
                if self.mode == NetMode::Client {
                    self.on_server_message(msg, out);
                } else {
                    self.on_client_message(conn, msg, out);
                }
            }
        }
    }

    fn on_client_message(&mut self, conn: ConnId, msg: Msg, out: &mut Vec<Inbound>) {
        let client = self.conn_client.get(&conn).copied();
        match (msg, client) {
            (Msg::Hello { version, payload }, None) => {
                let id = self.next_client_id;
                self.next_client_id += 1;
                self.conn_client.insert(conn, id);
                self.clients.insert(
                    id,
                    ClientInfo {
                        conn: Some(conn),
                        payload: payload.clone(),
                        ..Default::default()
                    },
                );
                if version != PROTOCOL_VERSION {
                    self.approve(id, false, "a different game version");
                } else if self.config.manager.connection_approval {
                    out.push(Inbound::Approve {
                        client: id,
                        payload,
                    });
                } else {
                    self.approve(id, true, "");
                }
            }
            (msg, Some(client)) => {
                let approved = self.clients.get(&client).is_some_and(|c| c.approved);
                if !approved {
                    return;
                }
                match msg {
                    Msg::SceneReady => {
                        if let Some(c) = self.clients.get_mut(&client) {
                            c.ready = true;
                        }
                        out.push(Inbound::SceneReady { client });
                    }
                    Msg::Vars { id, changes } => out.push(Inbound::Vars {
                        from: client,
                        id,
                        changes,
                    }),
                    Msg::Rpc {
                        id,
                        behaviour,
                        method,
                        args,
                        frame,
                    } => out.push(Inbound::Rpc {
                        from: client,
                        id,
                        behaviour,
                        method,
                        args,
                        frame,
                    }),
                    Msg::Transform(t) => out.push(Inbound::Transform {
                        from: client,
                        msg: t,
                    }),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn on_server_message(&mut self, msg: Msg, out: &mut Vec<Inbound>) {
        let from = SERVER_CLIENT_ID;
        out.push(match msg {
            Msg::Welcome {
                client_id,
                scene,
                tick_rate,
            } => {
                self.local_client_id = client_id;
                self.server_tick_rate = tick_rate;
                Inbound::Welcome { client_id, scene }
            }
            Msg::Denied { reason } => Inbound::Denied(reason),
            Msg::LoadScene { path } => Inbound::LoadScene(path),
            Msg::Spawn(s) => Inbound::Spawn(s),
            Msg::SyncDone => Inbound::SyncDone,
            Msg::Despawn { id, destroy } => Inbound::Despawn { id, destroy },
            Msg::Ownership { id, owner } => Inbound::Ownership { id, owner },
            Msg::Vars { id, changes } => Inbound::Vars { from, id, changes },
            Msg::Rpc {
                id,
                behaviour,
                method,
                args,
                frame,
            } => Inbound::Rpc {
                from,
                id,
                behaviour,
                method,
                args,
                frame,
            },
            Msg::Transform(t) => Inbound::Transform { from, msg: t },
            Msg::Hello { .. } | Msg::SceneReady => return,
        });
    }

    fn update_stats(&mut self) {
        let Some(e) = self.endpoint.as_ref() else {
            return;
        };
        let totals = e.totals();
        self.stats.mode = self.mode;
        self.stats.rtt_ms = totals.rtt * 1000.0;
        self.stats.connections = match self.mode {
            NetMode::Client => usize::from(e.connections().contains(&0)),
            _ => e.connections().len(),
        };
        self.stats.objects = self.objects.len();
        // Rates over (about) the last second.
        let span = self.now - self.stats_since;
        if span < 1.0 && self.stats_since > 0.0 {
            return;
        }
        let base = self.stats_base;
        let per = |a: u64, b: u64| (a.saturating_sub(b)) as f32 / span.max(1e-3) as f32;
        let sent = totals.packets_sent.saturating_sub(base.packets_sent);
        let lost = totals.packets_lost.saturating_sub(base.packets_lost);
        self.stats.packets_in = per(totals.packets_received, base.packets_received);
        self.stats.packets_out = per(totals.packets_sent, base.packets_sent);
        self.stats.bytes_in = per(totals.bytes_received, base.bytes_received);
        self.stats.bytes_out = per(totals.bytes_sent, base.bytes_sent);
        self.stats.loss = if sent > 0 {
            (lost as f32 / sent as f32 * 100.0).min(100.0)
        } else {
            0.0
        };
        self.stats_since = self.now;
        self.stats_base = totals;
    }

    /// Change the network simulator while running (tests, the Network Manager Inspector).
    pub fn set_simulator(&mut self, sim: Simulator) {
        if let Some(e) = self.endpoint.as_mut() {
            e.simulator = sim;
        }
    }

    /// Endpoint counters (tests).
    pub fn transport_totals(&self) -> ConnStats {
        self.endpoint
            .as_ref()
            .map(Endpoint::totals)
            .unwrap_or_default()
    }

    // ---- LAN discovery ---------------------------------------------------------------------

    /// Look for games on the LAN; results gather in [`NetSession::lan_games`].
    pub fn find_lan_games(&mut self) -> Result<(), String> {
        if self.discovery.is_none() {
            let c = DiscoveryClient::new(&self.config.game_name)
                .map_err(|e| format!("LAN discovery: {e}"))?;
            self.discovery = Some(c);
        }
        let port = self.config.discovery_port;
        if let Some(d) = self.discovery.as_mut() {
            d.search(port);
        }
        Ok(())
    }

    pub fn lan_games(&self) -> Vec<LanGame> {
        self.discovery
            .as_ref()
            .map(|d| d.found.clone())
            .unwrap_or_default()
    }

    fn poll_discovery(&mut self) {
        if let Some(d) = self.discovery.as_mut() {
            d.poll();
        }
    }
}

/// This machine's name for LAN listings.
fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Misty Host".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump(a: &mut NetSession, b: &mut NetSession, frames: usize) -> (Vec<Inbound>, Vec<Inbound>) {
        let (mut ia, mut ib) = (Vec::new(), Vec::new());
        for _ in 0..frames {
            ia.extend(a.receive(1.0 / 60.0));
            ib.extend(b.receive(1.0 / 60.0));
            a.flush();
            b.flush();
        }
        (ia, ib)
    }

    fn session(net: &MemoryNet, approval: bool) -> NetSession {
        NetSession {
            config: NetConfig {
                memory: Some(net.clone()),
                manager: NetworkManager {
                    connection_approval: approval,
                    port: 7100,
                    ..Default::default()
                },
                connection_data: "Bo".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn clients_join_with_approval_and_leave() {
        let net = MemoryNet::default();
        let mut server = session(&net, true);
        server.scene_path = Some("Assets/Scenes/Net.scene".into());
        server.start_server(true).unwrap();
        let mut client = session(&net, true);
        client.start_client("127.0.0.1:7100").unwrap();
        let (is, _) = pump(&mut server, &mut client, 10);
        assert_eq!(
            is,
            [Inbound::Approve {
                client: 1,
                payload: "Bo".into()
            }]
        );
        server.approve(1, true, "");
        let (_, ic) = pump(&mut server, &mut client, 10);
        assert_eq!(
            ic,
            [Inbound::Welcome {
                client_id: 1,
                scene: Some("Assets/Scenes/Net.scene".into())
            }]
        );
        assert_eq!(client.local_client_id, 1);
        client.send_to_server(Channel::ReliableOrdered, &Msg::SceneReady);
        let (is, _) = pump(&mut server, &mut client, 10);
        assert_eq!(is, [Inbound::SceneReady { client: 1 }]);
        assert_eq!(server.ready_remote_clients(), [1]);
        assert_eq!(server.connected_client_ids(), [0, 1]);
        client.shutdown();
        let (is, _) = pump(&mut server, &mut client, 5);
        assert_eq!(
            is,
            [Inbound::ClientLeft {
                client: 1,
                reason: "closed by the other side".into()
            }]
        );
        // A refused client hears why, then is dropped.
        let mut late = session(&net, true);
        late.start_client("127.0.0.1:7100").unwrap();
        pump(&mut server, &mut late, 10);
        server.approve(2, false, "no room");
        let (_, il) = pump(&mut server, &mut late, 60);
        assert!(il.contains(&Inbound::Denied("no room".into())), "{il:?}");
        assert!(il.iter().any(|i| matches!(i, Inbound::Disconnected(_))));
        assert!(server.clients.get(&2).is_none());
        assert!(server.stats.packets_in > 0.0);
    }
}
