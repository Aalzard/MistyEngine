//! Multiplayer networking in the shape of a familiar netcode package: a Network Manager starts
//! a host, server or client; Network Objects are spawned by the server on every client with an
//! owner; Network Transforms replicate motion (interpolated, optionally predicted); scripts get
//! Network Variables and server / client RPCs (see `scripting::network_api`).
//!
//! - [`transport`]: UDP connections with reliable and unreliable channels, fragmentation,
//!   timeouts and a network simulator.
//! - [`discovery`]: finding games on the LAN by broadcast.
//! - [`protocol`]: the messages; [`value`]: what script values become on the wire.
//! - [`session`]: one game's connection state, clients and spawned objects.
//! - [`interpolation`]: smoothing remote motion, prediction history.
//! - [`components`]: Network Manager, Network Object, Network Transform.

pub mod components;
pub mod discovery;
pub mod interpolation;
pub mod protocol;
pub mod session;
pub mod transport;
pub mod value;

pub use components::{NetAuthority, NetworkManager, NetworkObject, NetworkTransform};
pub use session::{NetMode, NetSession, NetStats};
pub use value::NetValue;

/// The client id of the server (and the host's own player).
pub const SERVER_CLIENT_ID: u64 = 0;

/// Who may read a Network Variable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadPermission {
    #[default]
    Everyone,
    /// The owner (and the server).
    Owner,
}

/// Who may write a Network Variable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WritePermission {
    #[default]
    Server,
    Owner,
}

/// A script field replicated by the network (`NetworkVariable()` in the field's attributes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetVarSpec {
    pub read: ReadPermission,
    pub write: WritePermission,
    /// Method called with (previous, current) when the value changes.
    pub on_changed: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcKind {
    /// A client asks the server to run it.
    Server,
    /// The server runs it on clients.
    Client,
}

/// A script function called across the network (`…ServerRpc` / `…ClientRpc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpcSpec {
    pub kind: RpcKind,
    /// Server RPCs: only the object's owner may call it.
    pub require_ownership: bool,
    /// Sent reliably (in order with spawns and variables), else on the unreliable channel.
    pub reliable: bool,
}

impl RpcSpec {
    pub fn new(kind: RpcKind) -> Self {
        Self {
            kind,
            require_ownership: true,
            reliable: true,
        }
    }

    /// The kind a function name asks for by its suffix.
    pub fn from_name(name: &str) -> Option<Self> {
        if name.ends_with("ServerRpc") {
            Some(Self::new(RpcKind::Server))
        } else if name.ends_with("ClientRpc") {
            Some(Self::new(RpcKind::Client))
        } else {
            None
        }
    }
}

/// Where the editor or the player asks a game to start networking at Play (Multiplayer Play
/// Mode, `--net-client`): applied by the runtime after the scene's `Awake`.
#[derive(Clone, Debug, PartialEq)]
pub enum AutoStart {
    Host,
    Server,
    /// Connect to this address ("127.0.0.1:7777"; the port may be left out).
    Client(String),
}

/// Parse "host[:port]" with `default_port`.
pub fn parse_address(text: &str, default_port: u16) -> Result<std::net::SocketAddr, String> {
    use std::net::ToSocketAddrs;
    let text = text.trim();
    let with_port = if text.parse::<std::net::SocketAddr>().is_ok() || text.contains(':') {
        text.to_owned()
    } else {
        format!("{text}:{default_port}")
    };
    with_port
        .to_socket_addrs()
        .map_err(|e| format!("'{text}': {e}"))?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| format!("'{text}' has no IPv4 address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_rpc_names() {
        assert_eq!(
            parse_address("127.0.0.1", 7777).unwrap(),
            "127.0.0.1:7777".parse().unwrap()
        );
        assert_eq!(
            parse_address(" 10.0.0.2:9000 ", 7777).unwrap(),
            "10.0.0.2:9000".parse().unwrap()
        );
        assert_eq!(parse_address("localhost", 1).unwrap().port(), 1);
        assert!(parse_address("not a host name!", 1).is_err());
        assert_eq!(
            RpcSpec::from_name("MoveServerRpc").map(|r| r.kind),
            Some(RpcKind::Server)
        );
        assert_eq!(
            RpcSpec::from_name("ShowHitClientRpc").map(|r| r.kind),
            Some(RpcKind::Client)
        );
        assert_eq!(RpcSpec::from_name("Update"), None);
    }
}
