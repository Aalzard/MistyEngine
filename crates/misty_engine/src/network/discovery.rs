//! LAN discovery: a listening game answers broadcast queries on a fixed UDP port, so clients
//! find games on the local network without typing an address.
//!
//! A query names the game (its product name), so different games on one network don't list
//! each other. Both sides are non-blocking sockets polled once per frame.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use super::value::{Reader, Writer};

/// UDP port servers listen on for discovery queries.
pub const DISCOVERY_PORT: u16 = 47777;
const MAGIC: u32 = 0x444C_4E4D; // "MNLD"
const QUERY: u8 = 1;
const REPLY: u8 = 2;

/// A game found on the network.
#[derive(Clone, Debug, PartialEq)]
pub struct LanGame {
    /// Where to connect (the replying machine and its game port).
    pub address: SocketAddr,
    /// The host's display name (its machine or player name).
    pub name: String,
    pub players: u16,
    pub max_players: u16,
}

/// Answers queries while a server runs.
pub struct DiscoveryResponder {
    socket: UdpSocket,
    game: String,
    /// Game port, host name and player counts sent in replies.
    pub port: u16,
    pub name: String,
    pub players: u16,
    pub max_players: u16,
}

impl DiscoveryResponder {
    /// Listen on `bind` (normally 0.0.0.0:[`DISCOVERY_PORT`]) for queries naming `game`.
    pub fn bind(bind: SocketAddr, game: &str, port: u16, name: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            game: game.to_owned(),
            port,
            name: name.to_owned(),
            players: 0,
            max_players: 0,
        })
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.local_addr().ok()
    }

    /// Answer the queries that arrived.
    pub fn poll(&mut self) {
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let mut r = Reader::new(&buf[..n]);
            if r.u32() != Some(MAGIC) || r.u8() != Some(QUERY) {
                continue;
            }
            if r.str().as_deref() != Some(self.game.as_str()) {
                continue;
            }
            let mut w = Writer::new();
            w.u32(MAGIC);
            w.u8(REPLY);
            w.str(&self.game);
            w.u16(self.port);
            w.str(&self.name);
            w.u16(self.players);
            w.u16(self.max_players);
            let _ = self.socket.send_to(&w.buf, from);
        }
    }
}

/// Sends queries and collects the replies.
pub struct DiscoveryClient {
    socket: UdpSocket,
    game: String,
    /// Games found since the last [`DiscoveryClient::search`].
    pub found: Vec<LanGame>,
}

impl DiscoveryClient {
    pub fn new(game: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            game: game.to_owned(),
            found: Vec::new(),
        })
    }

    /// Ask the network (broadcast) and this machine; replies arrive with [`DiscoveryClient::poll`].
    pub fn search(&mut self, port: u16) {
        self.found.clear();
        let targets = [
            SocketAddr::from((Ipv4Addr::BROADCAST, port)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        ];
        for t in targets {
            self.query(t);
        }
    }

    /// Ask one address (tests, known hosts).
    pub fn query(&mut self, to: SocketAddr) {
        let mut w = Writer::new();
        w.u32(MAGIC);
        w.u8(QUERY);
        w.str(&self.game);
        let _ = self.socket.send_to(&w.buf, to);
    }

    /// Read replies; true when something new was found.
    pub fn poll(&mut self) -> bool {
        let mut buf = [0u8; 512];
        let mut new = false;
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let mut r = Reader::new(&buf[..n]);
            if r.u32() != Some(MAGIC) || r.u8() != Some(REPLY) {
                continue;
            }
            if r.str().as_deref() != Some(self.game.as_str()) {
                continue;
            }
            let (Some(port), Some(name), Some(players), Some(max)) =
                (r.u16(), r.str(), r.u16(), r.u16())
            else {
                continue;
            };
            let ip: IpAddr = from.ip();
            let address = SocketAddr::new(ip, port);
            // Broadcast and localhost queries can both reach a local host.
            let same_host = |g: &LanGame| {
                g.address == address
                    || (g.address.port() == port && g.name == name && ip.is_loopback())
            };
            if self.found.iter().any(same_host) {
                continue;
            }
            self.found.push(LanGame {
                address,
                name,
                players,
                max_players: max,
            });
            new = true;
        }
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_finds_the_listening_game_and_ignores_other_games() {
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut server = DiscoveryResponder::bind(local, "Net Demo", 7777, "Rayan's PC").unwrap();
        server.players = 2;
        server.max_players = 8;
        let mut other = DiscoveryResponder::bind(local, "Another Game", 9000, "x").unwrap();
        let mut client = DiscoveryClient::new("Net Demo").unwrap();
        let start = std::time::Instant::now();
        while client.found.is_empty() && start.elapsed().as_secs() < 10 {
            client.query(server.local_addr().unwrap());
            client.query(other.local_addr().unwrap());
            std::thread::sleep(std::time::Duration::from_millis(5));
            server.poll();
            other.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
            client.poll();
        }
        assert_eq!(
            client.found,
            [LanGame {
                address: SocketAddr::from((Ipv4Addr::LOCALHOST, 7777)),
                name: "Rayan's PC".into(),
                players: 2,
                max_players: 8,
            }]
        );
    }
}
