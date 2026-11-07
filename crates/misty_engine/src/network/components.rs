//! The Netcode components: Network Manager (one per game: transport, tick rate, player prefab,
//! approval, scene sync), Network Object (an object the server spawns on every client) and
//! Network Transform (replicated position, rotation and scale).

use serde::{Deserialize, Serialize};

use crate::scene::registry::SceneComponent;

/// Network settings of the game; scripts start it with `Network.StartHost()` etc.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkManager {
    /// Address clients connect to.
    pub address: String,
    /// UDP port of the server (0 = any free port, for tests).
    pub port: u16,
    /// Address the server listens on ("0.0.0.0" = every network interface).
    pub listen_address: String,
    /// Network updates sent per second.
    pub tick_rate: u32,
    /// Prefab spawned for every connected client (owned by it), or none.
    pub player_prefab: Option<String>,
    /// Ask `Network.ConnectionApproval(clientId, payload)` before letting a client in.
    pub connection_approval: bool,
    /// Clients the server accepts at once.
    pub max_connections: u32,
    /// Seconds without packets before a connection counts as lost.
    pub timeout: f32,
    /// Clients load the server's scene when they connect (and when it changes).
    pub scene_management: bool,
    /// A listening server answers LAN discovery queries (`Network.FindLanGames`).
    pub lan_discovery: bool,
    /// Network simulator: extra one-way delay in milliseconds.
    pub simulated_latency_ms: f32,
    /// Network simulator: random extra delay up to this, milliseconds.
    pub simulated_jitter_ms: f32,
    /// Network simulator: share of packets dropped, percent.
    pub simulated_packet_loss: f32,
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self {
            address: "127.0.0.1".into(),
            port: 7777,
            listen_address: "0.0.0.0".into(),
            tick_rate: 30,
            player_prefab: None,
            connection_approval: false,
            max_connections: 16,
            timeout: 10.0,
            scene_management: true,
            lan_discovery: true,
            simulated_latency_ms: 0.0,
            simulated_jitter_ms: 0.0,
            simulated_packet_loss: 0.0,
        }
    }
}

impl SceneComponent for NetworkManager {
    const NAME: &'static str = "Network Manager";
    const CATEGORY: &'static str = "Netcode";
}

/// An object the server spawns on clients, with a network id and an owner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkObject {
    /// Stable id matching an object saved in a scene on server and clients (copies get a new
    /// one). Spawned prefab copies are matched by their prefab instead.
    pub global_id: String,
    /// Despawn the object when its owner disconnects (else the server takes it over).
    pub destroy_with_owner: bool,
}

impl Default for NetworkObject {
    fn default() -> Self {
        Self {
            global_id: crate::save::new_id(),
            destroy_with_owner: true,
        }
    }
}

impl SceneComponent for NetworkObject {
    const NAME: &'static str = "Network Object";
    const CATEGORY: &'static str = "Netcode";
}

/// Who may move a Network Transform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetAuthority {
    /// The server moves it; clients follow (owners may predict).
    #[default]
    Server,
    /// The owning client moves it; the server relays it.
    Owner,
}

impl NetAuthority {
    pub const ALL: [Self; 2] = [Self::Server, Self::Owner];
    pub fn label(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Owner => "Owner",
        }
    }
}

/// Replicates the object's position, rotation and scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkTransform {
    pub authority: NetAuthority,
    pub sync_position: bool,
    pub sync_rotation: bool,
    pub sync_scale: bool,
    /// Remote copies move smoothly between received states (else they jump to each).
    pub interpolate: bool,
    /// How far behind the newest state remote copies play, seconds (the jitter buffer).
    pub interpolation_delay: f32,
    /// Smallest movement sent, metres.
    pub position_threshold: f32,
    /// Smallest turn sent, degrees.
    pub rotation_threshold: f32,
    /// Smallest scale change sent.
    pub scale_threshold: f32,
    /// A jump longer than this teleports instead of sliding there, metres.
    pub snap_distance: f32,
    /// Server authority only: the owner moves its copy right away and corrects it when the
    /// server's state arrives (client-side prediction with reconciliation).
    pub predict: bool,
}

impl Default for NetworkTransform {
    fn default() -> Self {
        Self {
            authority: NetAuthority::Server,
            sync_position: true,
            sync_rotation: true,
            sync_scale: false,
            interpolate: true,
            interpolation_delay: 0.1,
            position_threshold: 0.001,
            rotation_threshold: 0.01,
            scale_threshold: 0.01,
            snap_distance: 3.0,
            predict: false,
        }
    }
}

impl SceneComponent for NetworkTransform {
    const NAME: &'static str = "Network Transform";
    const CATEGORY: &'static str = "Netcode";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;

    #[test]
    fn components_round_trip_and_copies_get_new_ids() {
        let mut s = Scene::new("t");
        let e = s.spawn("Player", None);
        s.add(e, NetworkObject::default());
        s.add(
            e,
            NetworkTransform {
                authority: NetAuthority::Owner,
                predict: true,
                ..Default::default()
            },
        );
        let m = s.spawn("Network", None);
        s.add(
            m,
            NetworkManager {
                player_prefab: Some("Assets/Prefabs/P.prefab".into()),
                ..Default::default()
            },
        );
        let (back, report) = Scene::from_json("t", &s.to_json()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let e2 = back.find_by_name("Player").unwrap();
        let id = s.world.get::<&NetworkObject>(e).unwrap().global_id.clone();
        assert_eq!(back.world.get::<&NetworkObject>(e2).unwrap().global_id, id);
        assert_eq!(
            back.world.get::<&NetworkTransform>(e2).unwrap().authority,
            NetAuthority::Owner
        );
        let copy = s.duplicate(e);
        assert_ne!(s.world.get::<&NetworkObject>(copy).unwrap().global_id, id);
        let fresh = NetworkObject::default().global_id;
        assert_eq!(fresh.len(), 32);
    }
}
