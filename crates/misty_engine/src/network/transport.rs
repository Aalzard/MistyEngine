//! The UDP transport: connections over datagrams with three channels (reliable ordered, reliable
//! unordered, unreliable), a connect handshake, keep-alives and timeouts, fragmentation of
//! messages larger than one datagram, and a network simulator (latency, jitter, loss) for
//! testing.
//!
//! Reliability is per message on top of per-packet acknowledgements: every data packet carries
//! a sequence number plus the latest received sequence and a 32-bit history of the ones before
//! it, so one packet acknowledges up to 33. Reliable messages stay queued until a packet that
//! carried them is acknowledged, and are sent again after about one and a half round trips.
//!
//! The socket is read on its own thread ([`UdpLink`]); everything else runs when the owner calls
//! [`Endpoint::update`] with its clock, so tests can drive time (and an in-memory
//! [`MemoryNet`] replaces sockets where determinism matters).

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use super::value::{Reader, Writer};

/// Largest datagram sent (fits typical internet paths without IP fragmentation).
pub const MTU: usize = 1200;
/// Payload bytes per fragment of a large message.
pub const FRAGMENT_SIZE: usize = 1024;
/// Largest message accepted (bigger sends are refused).
pub const MAX_MESSAGE: usize = 4 << 20;
/// First bytes of every Misty packet ("MN") and the protocol version.
const MAGIC: u16 = 0x4E4D;
const VERSION: u8 = 1;
/// Reliable messages sent but not yet acknowledged, per channel; more wait in the queue.
const SEND_WINDOW: usize = 2048;
/// Datagrams one connection may send per update.
const MAX_PACKETS_PER_UPDATE: usize = 256;

/// Delivery guarantees of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Arrives once, in the order sent (spawns, Network Variables, RPCs).
    ReliableOrdered = 0,
    /// Arrives once, in any order.
    ReliableUnordered = 1,
    /// May be lost or reordered (transform snapshots).
    Unreliable = 2,
}

impl Channel {
    fn from_bits(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::ReliableOrdered),
            1 => Some(Self::ReliableUnordered),
            2 => Some(Self::Unreliable),
            _ => None,
        }
    }
    fn reliable(self) -> bool {
        self != Self::Unreliable
    }
}

/// A connection's number on this endpoint (a client calls its server 0).
pub type ConnId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    /// Nothing arrived for the timeout.
    Timeout,
    /// The other side said goodbye.
    Closed,
    /// The server never answered the connect request.
    ConnectFailed,
    /// The server is full.
    ServerFull,
}

impl DisconnectReason {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Timeout => "timed out",
            Self::Closed => "closed by the other side",
            Self::ConnectFailed => "the server did not answer",
            Self::ServerFull => "the server is full",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransportEvent {
    Connected(ConnId),
    Disconnected(ConnId, DisconnectReason),
    Message(ConnId, Channel, Vec<u8>),
}

/// Bad network conditions applied to outgoing datagrams (the network simulator).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Simulator {
    /// One-way delay added to every datagram, seconds.
    pub latency: f32,
    /// Extra random delay up to this, seconds (reorders packets).
    pub jitter: f32,
    /// Share of datagrams dropped, 0..1.
    pub loss: f32,
}

impl Simulator {
    pub fn active(&self) -> bool {
        self.latency > 0.0 || self.jitter > 0.0 || self.loss > 0.0
    }
}

/// Timeouts and keep-alive interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransportConfig {
    /// A client gives up connecting after this many seconds.
    pub connect_timeout: f32,
    /// A connection is dropped after this many seconds without a packet.
    pub timeout: f32,
    /// Seconds between connect requests.
    pub connect_interval: f32,
    /// An empty packet goes out when nothing was sent for this long.
    pub keepalive: f32,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: 10.0,
            timeout: 10.0,
            connect_interval: 0.2,
            keepalive: 0.1,
        }
    }
}

/// Counters of one connection (or the whole endpoint).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConnStats {
    /// Smoothed round-trip time in seconds (0 until measured).
    pub rtt: f32,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// Packets never acknowledged.
    pub packets_lost: u64,
    /// Reliable messages sent again.
    pub resends: u64,
}

// ---- links ----------------------------------------------------------------------------------

/// Where datagrams go: a UDP socket, or memory in tests.
pub trait Link: Send {
    fn send(&mut self, to: SocketAddr, data: &[u8]);
    fn recv(&mut self) -> Option<(SocketAddr, Vec<u8>)>;
    fn local_addr(&self) -> SocketAddr;
}

/// A UDP socket read by a background thread.
pub struct UdpLink {
    socket: Arc<UdpSocket>,
    rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    local: SocketAddr,
}

impl UdpLink {
    /// Bind `addr` (port 0 = any free port) and start the receive thread.
    pub fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_read_timeout(Some(Duration::from_millis(20)))?;
        let local = socket.local_addr()?;
        let socket = Arc::new(socket);
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let socket = socket.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("misty-net".into())
                .spawn(move || {
                    let mut buf = vec![0u8; 2048];
                    while !stop.load(Ordering::Relaxed) {
                        match socket.recv_from(&mut buf) {
                            Ok((n, from)) => {
                                if tx.send((from, buf[..n].to_vec())).is_err() {
                                    return;
                                }
                            }
                            // Timeouts, and on Windows the "port unreachable" a send to a
                            // closed port reports on the next receive: keep listening.
                            Err(_) => continue,
                        }
                    }
                })?
        };
        Ok(Self {
            socket,
            rx,
            stop,
            thread: Some(thread),
            local,
        })
    }
}

impl Link for UdpLink {
    fn send(&mut self, to: SocketAddr, data: &[u8]) {
        let _ = self.socket.send_to(data, to);
    }
    fn recv(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        self.rx.try_recv().ok()
    }
    fn local_addr(&self) -> SocketAddr {
        self.local
    }
}

impl Drop for UdpLink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type Queue = VecDeque<(SocketAddr, Vec<u8>)>;

/// An in-process "network" of [`MemoryLink`]s addressed like 127.0.0.1:port (tests).
#[derive(Clone, Default)]
pub struct MemoryNet {
    inner: Arc<Mutex<(HashMap<SocketAddr, Queue>, u16)>>,
}

impl std::fmt::Debug for MemoryNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryNet")
    }
}

impl MemoryNet {
    /// A link at `port` (0 = the next free one).
    pub fn link(&self, port: u16) -> MemoryLink {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let port = if port == 0 {
            g.1 = g.1.max(40000) + 1;
            g.1
        } else {
            port
        };
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        g.0.insert(addr, VecDeque::new());
        MemoryLink {
            net: self.clone(),
            addr,
        }
    }
}

pub struct MemoryLink {
    net: MemoryNet,
    addr: SocketAddr,
}

impl Link for MemoryLink {
    fn send(&mut self, to: SocketAddr, data: &[u8]) {
        let mut g = self.net.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(q) = g.0.get_mut(&to) {
            q.push_back((self.addr, data.to_vec()));
        }
    }
    fn recv(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        let mut g = self.net.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0.get_mut(&self.addr)?.pop_front()
    }
    fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for MemoryLink {
    fn drop(&mut self) {
        let mut g = self.net.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0.remove(&self.addr);
    }
}

// ---- sequence numbers -------------------------------------------------------------------------

/// `a` comes after `b`, with wrap-around.
fn seq_gt(a: u16, b: u16) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000
}

// ---- packets ----------------------------------------------------------------------------------

const KIND_CONNECT: u8 = 1;
const KIND_ACCEPT: u8 = 2;
const KIND_DENY: u8 = 3;
const KIND_DATA: u8 = 4;
const KIND_DISCONNECT: u8 = 5;

const FLAG_FRAGMENT: u8 = 0x10;

/// One message (or fragment) ready to go into a packet.
#[derive(Clone)]
struct Part {
    channel: Channel,
    /// Reliable message id (0 for unreliable).
    id: u16,
    /// (group, index, count) of a fragment.
    fragment: Option<(u16, u16, u16)>,
    data: Arc<[u8]>,
}

impl Part {
    fn encoded_len(&self) -> usize {
        1 + if self.channel.reliable() { 2 } else { 0 }
            + if self.fragment.is_some() { 6 } else { 0 }
            + varint_len(self.data.len())
            + self.data.len()
    }
    fn write(&self, w: &mut Writer) {
        let flags = if self.fragment.is_some() {
            FLAG_FRAGMENT
        } else {
            0
        };
        w.u8(self.channel as u8 | flags);
        if self.channel.reliable() {
            w.u16(self.id);
        }
        if let Some((g, i, n)) = self.fragment {
            w.u16(g);
            w.u16(i);
            w.u16(n);
        }
        w.bytes(&self.data);
    }
    fn read(r: &mut Reader) -> Option<Part> {
        let b = r.u8()?;
        let channel = Channel::from_bits(b & 0x0f)?;
        let id = if channel.reliable() { r.u16()? } else { 0 };
        let fragment = if b & FLAG_FRAGMENT != 0 {
            let (g, i, n) = (r.u16()?, r.u16()?, r.u16()?);
            if n == 0 || i >= n {
                return None;
            }
            Some((g, i, n))
        } else {
            None
        };
        let data: Arc<[u8]> = r.bytes()?.into();
        Some(Part {
            channel,
            id,
            fragment,
            data,
        })
    }
}

fn varint_len(n: usize) -> usize {
    let mut n = n as u64;
    let mut len = 1;
    while n >= 0x80 {
        n >>= 7;
        len += 1;
    }
    len
}

/// Packet header size of a data packet (magic, version, kind, token, seq, ack, ack bits).
const DATA_HEADER: usize = 2 + 1 + 1 + 4 + 2 + 2 + 4;

fn header(w: &mut Writer, kind: u8) {
    w.u16(MAGIC);
    w.u8(VERSION);
    w.u8(kind);
}

// ---- reliable state ---------------------------------------------------------------------------

struct Pending {
    part: Part,
    last_sent: Option<f64>,
}

#[derive(Default)]
struct ReliableSend {
    next_id: u16,
    /// Waiting or in flight, in send order.
    order: VecDeque<u16>,
    pending: HashMap<u16, Pending>,
}

#[derive(Default)]
struct ReliableRecv {
    /// Ordered: the next id to deliver.
    next: u16,
    /// Ordered: arrived ahead of `next`.
    buffered: HashMap<u16, Part>,
    /// Unordered: ids seen recently (with when), against duplicates.
    seen: HashMap<u16, f64>,
    /// Fragments being put back together, by group.
    assembling: HashMap<u16, Assembly>,
}

struct Assembly {
    started: f64,
    parts: Vec<Option<Arc<[u8]>>>,
    have: usize,
}

impl Assembly {
    fn new(count: u16, now: f64) -> Self {
        Self {
            started: now,
            parts: vec![None; count as usize],
            have: 0,
        }
    }
    /// Store fragment `i`; the whole message once complete.
    fn add(&mut self, i: u16, data: Arc<[u8]>) -> Option<Vec<u8>> {
        let slot = self.parts.get_mut(i as usize)?;
        if slot.is_none() {
            *slot = Some(data);
            self.have += 1;
        }
        (self.have == self.parts.len()).then(|| {
            let mut out = Vec::new();
            for p in self.parts.iter().flatten() {
                out.extend_from_slice(p);
            }
            out
        })
    }
}

struct SentPacket {
    time: f64,
    reliable: Vec<(Channel, u16)>,
}

#[derive(PartialEq)]
enum State {
    Connecting { started: f64, next_request: f64 },
    Connected,
}

struct Connection {
    id: ConnId,
    addr: SocketAddr,
    nonce: u64,
    state: State,
    last_recv: f64,
    last_send: f64,
    next_seq: u16,
    remote_seq: u16,
    recv_bits: u32,
    have_remote: bool,
    /// Something arrived that still needs acknowledging.
    ack_due: bool,
    sent: HashMap<u16, SentPacket>,
    send: [ReliableSend; 2],
    recv: [ReliableRecv; 2],
    unreliable: Vec<Part>,
    unreliable_group: u16,
    unreliable_frags: HashMap<u16, Assembly>,
    stats: ConnStats,
}

impl Connection {
    fn new(id: ConnId, addr: SocketAddr, nonce: u64, state: State, now: f64) -> Self {
        Self {
            id,
            addr,
            nonce,
            state,
            last_recv: now,
            last_send: f64::NEG_INFINITY,
            next_seq: 0,
            remote_seq: 0,
            recv_bits: 0,
            have_remote: false,
            ack_due: false,
            sent: HashMap::new(),
            send: Default::default(),
            recv: Default::default(),
            unreliable: Vec::new(),
            unreliable_group: 0,
            unreliable_frags: HashMap::new(),
            stats: ConnStats::default(),
        }
    }

    fn token(&self) -> u32 {
        self.nonce as u32
    }

    /// Queue `data` on `channel`, split into fragments when large.
    fn queue(&mut self, channel: Channel, data: &[u8]) {
        let chunks: Vec<&[u8]> = if data.len() <= FRAGMENT_SIZE {
            vec![data]
        } else {
            data.chunks(FRAGMENT_SIZE).collect()
        };
        let count = chunks.len() as u16;
        let fragmented = chunks.len() > 1;
        if channel.reliable() {
            let s = &mut self.send[channel as usize];
            let group = s.next_id;
            for (i, c) in chunks.into_iter().enumerate() {
                let id = s.next_id;
                s.next_id = s.next_id.wrapping_add(1);
                let part = Part {
                    channel,
                    id,
                    fragment: fragmented.then_some((group, i as u16, count)),
                    data: c.into(),
                };
                s.order.push_back(id);
                s.pending.insert(
                    id,
                    Pending {
                        part,
                        last_sent: None,
                    },
                );
            }
        } else {
            let group = self.unreliable_group;
            if fragmented {
                self.unreliable_group = self.unreliable_group.wrapping_add(1);
            }
            for (i, c) in chunks.into_iter().enumerate() {
                self.unreliable.push(Part {
                    channel,
                    id: 0,
                    fragment: fragmented.then_some((group, i as u16, count)),
                    data: c.into(),
                });
            }
        }
    }

    fn resend_after(&self) -> f64 {
        let rtt = if self.stats.rtt > 0.0 {
            self.stats.rtt
        } else {
            0.2
        };
        (rtt as f64 * 1.5).clamp(0.03, 1.0)
    }

    /// Acknowledge packet `seq`: its reliable messages are delivered.
    fn on_ack(&mut self, seq: u16, now: f64) {
        let Some(p) = self.sent.remove(&seq) else {
            return;
        };
        let sample = (now - p.time).max(0.0) as f32;
        self.stats.rtt = if self.stats.rtt == 0.0 {
            sample
        } else {
            self.stats.rtt * 0.9 + sample * 0.1
        };
        for (ch, id) in p.reliable {
            self.send[ch as usize].pending.remove(&id);
        }
    }

    /// Record receipt of packet `seq`; false if it was seen before.
    fn on_receive_seq(&mut self, seq: u16) -> bool {
        if !self.have_remote {
            self.have_remote = true;
            self.remote_seq = seq;
            self.recv_bits = 0;
            return true;
        }
        if seq_gt(seq, self.remote_seq) {
            let shift = seq.wrapping_sub(self.remote_seq) as u32;
            self.recv_bits = if shift > 32 {
                0
            } else {
                // The old latest becomes bit `shift - 1`.
                let shifted = self.recv_bits.checked_shl(shift).unwrap_or(0);
                shifted | (1u32 << (shift - 1))
            };
            self.remote_seq = seq;
            true
        } else {
            let back = self.remote_seq.wrapping_sub(seq) as u32;
            if back == 0 || back > 32 {
                return false;
            }
            let bit = 1u32 << (back - 1);
            if self.recv_bits & bit != 0 {
                return false;
            }
            self.recv_bits |= bit;
            true
        }
    }

    /// Hand a received part to its channel; complete messages go to `out`.
    fn on_part(&mut self, part: Part, now: f64, out: &mut Vec<(Channel, Vec<u8>)>) {
        match part.channel {
            Channel::Unreliable => match part.fragment {
                None => out.push((part.channel, part.data.to_vec())),
                Some((g, i, n)) => {
                    let a = self
                        .unreliable_frags
                        .entry(g)
                        .or_insert_with(|| Assembly::new(n, now));
                    if a.parts.len() != n as usize {
                        *a = Assembly::new(n, now);
                    }
                    if let Some(msg) = a.add(i, part.data) {
                        self.unreliable_frags.remove(&g);
                        out.push((part.channel, msg));
                    }
                }
            },
            Channel::ReliableUnordered => {
                let r = &mut self.recv[1];
                if r.seen.contains_key(&part.id) {
                    return;
                }
                r.seen.insert(part.id, now);
                deliver_part(r, part, now, out);
            }
            Channel::ReliableOrdered => {
                let r = &mut self.recv[0];
                if part.id != r.next && !seq_gt(part.id, r.next) {
                    return; // already delivered
                }
                r.buffered.insert(part.id, part);
                while let Some(p) = r.buffered.remove(&r.next) {
                    r.next = r.next.wrapping_add(1);
                    deliver_part(r, p, now, out);
                }
            }
        }
    }

    /// Drop stale bookkeeping: unacknowledged packet records, old duplicate ids, incomplete
    /// unreliable fragments.
    fn prune(&mut self, now: f64) {
        let before = self.sent.len();
        self.sent.retain(|_, p| now - p.time < 2.0);
        self.stats.packets_lost += (before - self.sent.len()) as u64;
        self.recv[1].seen.retain(|_, t| now - *t < 5.0);
        self.unreliable_frags.retain(|_, a| now - a.started < 2.0);
    }
}

fn deliver_part(r: &mut ReliableRecv, part: Part, now: f64, out: &mut Vec<(Channel, Vec<u8>)>) {
    match part.fragment {
        None => out.push((part.channel, part.data.to_vec())),
        Some((g, i, n)) => {
            let a = r
                .assembling
                .entry(g)
                .or_insert_with(|| Assembly::new(n, now));
            if let Some(msg) = a.add(i, part.data) {
                r.assembling.remove(&g);
                out.push((part.channel, msg));
            }
        }
    }
}

// ---- endpoint ---------------------------------------------------------------------------------

/// One side of the transport: a server accepting clients, or a client of one server.
pub struct Endpoint {
    link: Box<dyn Link>,
    server: Option<SocketAddr>,
    max_connections: usize,
    conns: Vec<Connection>,
    next_id: ConnId,
    events: VecDeque<TransportEvent>,
    pub simulator: Simulator,
    pub config: TransportConfig,
    /// Datagrams held back by the simulator: (due time, destination, bytes).
    delayed: Vec<(f64, SocketAddr, Vec<u8>)>,
    rng: u64,
    now: f64,
    /// Counters of connections that are gone (totals keep them).
    retired: ConnStats,
}

impl Endpoint {
    /// A server accepting up to `max_connections` clients on `link`.
    pub fn server(link: Box<dyn Link>, max_connections: usize) -> Self {
        Self::new(link, None, max_connections)
    }

    /// A client connecting to `server` (connect requests start with the first update).
    pub fn client(link: Box<dyn Link>, server: SocketAddr, now: f64) -> Self {
        let mut e = Self::new(link, Some(server), 1);
        let nonce = e.random() | 1;
        let state = State::Connecting {
            started: now,
            next_request: now,
        };
        e.conns.push(Connection::new(0, server, nonce, state, now));
        e.now = now;
        e
    }

    fn new(link: Box<dyn Link>, server: Option<SocketAddr>, max: usize) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9, |d| d.as_nanos() as u64)
            ^ u64::from(link.local_addr().port()) << 32;
        Self {
            link,
            server,
            max_connections: max.max(1),
            conns: Vec::new(),
            next_id: 1,
            events: VecDeque::new(),
            simulator: Simulator::default(),
            config: TransportConfig::default(),
            delayed: Vec::new(),
            rng: seed | 1,
            now: 0.0,
            retired: ConnStats::default(),
        }
    }

    /// Seed the simulator's random numbers (reproducible loss in tests).
    pub fn seed(&mut self, seed: u64) {
        self.rng = seed | 1;
    }

    fn random(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn random01(&mut self) -> f32 {
        (self.random() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.link.local_addr()
    }

    pub fn is_server(&self) -> bool {
        self.server.is_none()
    }

    /// Connected (handshake done) connections.
    pub fn connections(&self) -> Vec<ConnId> {
        self.conns
            .iter()
            .filter(|c| c.state == State::Connected)
            .map(|c| c.id)
            .collect()
    }

    /// The address of connection `id`.
    pub fn addr(&self, id: ConnId) -> Option<SocketAddr> {
        self.conns.iter().find(|c| c.id == id).map(|c| c.addr)
    }

    pub fn stats(&self, id: ConnId) -> Option<ConnStats> {
        self.conns.iter().find(|c| c.id == id).map(|c| c.stats)
    }

    /// Counters summed over every connection, past and present (RTT: the worst current one).
    pub fn totals(&self) -> ConnStats {
        let mut t = self.retired;
        t.rtt = 0.0;
        for c in &self.conns {
            let s = c.stats;
            t.rtt = t.rtt.max(s.rtt);
            t.packets_sent += s.packets_sent;
            t.packets_received += s.packets_received;
            t.bytes_sent += s.bytes_sent;
            t.bytes_received += s.bytes_received;
            t.packets_lost += s.packets_lost;
            t.resends += s.resends;
        }
        t
    }

    /// Queue a message; it goes out with the next [`Endpoint::update`]. False when the
    /// connection doesn't exist or the message is too large.
    pub fn send(&mut self, id: ConnId, channel: Channel, data: &[u8]) -> bool {
        if data.len() > MAX_MESSAGE {
            return false;
        }
        match self.conns.iter_mut().find(|c| c.id == id) {
            Some(c) => {
                c.queue(channel, data);
                true
            }
            None => false,
        }
    }

    pub fn poll(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    /// Close connection `id` (the other side is told; no event here).
    pub fn disconnect(&mut self, id: ConnId) {
        if let Some(i) = self.conns.iter().position(|c| c.id == id) {
            let c = self.conns.remove(i);
            self.say_goodbye(&c);
            self.retire(&c);
        }
    }

    /// Close every connection.
    pub fn shutdown(&mut self) {
        for c in std::mem::take(&mut self.conns) {
            self.say_goodbye(&c);
            self.retire(&c);
        }
        // Goodbyes skip the simulator's delay: the endpoint is going away.
        for (_, to, data) in std::mem::take(&mut self.delayed) {
            self.link.send(to, &data);
        }
    }

    fn say_goodbye(&mut self, c: &Connection) {
        let mut w = Writer::new();
        header(&mut w, KIND_DISCONNECT);
        w.u32(c.token());
        for _ in 0..3 {
            self.link.send(c.addr, &w.buf);
        }
    }

    fn retire(&mut self, c: &Connection) {
        let s = c.stats;
        let t = &mut self.retired;
        t.packets_sent += s.packets_sent;
        t.packets_received += s.packets_received;
        t.bytes_sent += s.bytes_sent;
        t.bytes_received += s.bytes_received;
        t.packets_lost += s.packets_lost;
        t.resends += s.resends;
    }

    /// Send through the simulator.
    fn out(&mut self, to: SocketAddr, data: Vec<u8>) {
        if self.simulator.loss > 0.0 && self.random01() < self.simulator.loss {
            return;
        }
        if self.simulator.latency > 0.0 || self.simulator.jitter > 0.0 {
            let delay = self.simulator.latency + self.simulator.jitter * self.random01();
            self.delayed.push((self.now + delay as f64, to, data));
        } else {
            self.link.send(to, &data);
        }
    }

    /// Receive, acknowledge, resend, keep alive, time out and send: once per frame.
    pub fn update(&mut self, now: f64) {
        self.now = now;
        // Datagrams the simulator held back.
        if !self.delayed.is_empty() {
            let mut due = Vec::new();
            self.delayed.retain(|(t, to, data)| {
                if *t <= now {
                    due.push((*t, *to, data.clone()));
                    false
                } else {
                    true
                }
            });
            due.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, to, data) in due {
                self.link.send(to, &data);
            }
        }
        while let Some((from, data)) = self.link.recv() {
            self.receive(from, &data, now);
        }
        let mut gone = Vec::new();
        for i in 0..self.conns.len() {
            let c = &self.conns[i];
            match c.state {
                State::Connecting {
                    started,
                    next_request,
                } => {
                    if now - started > self.config.connect_timeout as f64 {
                        gone.push((c.id, DisconnectReason::ConnectFailed));
                        continue;
                    }
                    if now >= next_request {
                        let (addr, nonce) = (c.addr, c.nonce);
                        self.conns[i].state = State::Connecting {
                            started,
                            next_request: now + self.config.connect_interval as f64,
                        };
                        let mut w = Writer::new();
                        header(&mut w, KIND_CONNECT);
                        w.u64(nonce);
                        // Requests are padded to a full datagram, so a spoofed one can't make
                        // the server answer with more bytes than it received.
                        w.buf.resize(MTU.min(256), 0);
                        self.out(addr, w.buf);
                    }
                }
                State::Connected => {
                    if now - c.last_recv > self.config.timeout as f64 {
                        gone.push((c.id, DisconnectReason::Timeout));
                        continue;
                    }
                    self.flush(i, now);
                }
            }
        }
        for (id, reason) in gone {
            if let Some(i) = self.conns.iter().position(|c| c.id == id) {
                let c = self.conns.remove(i);
                self.retire(&c);
            }
            self.events
                .push_back(TransportEvent::Disconnected(id, reason));
        }
    }

    /// Build and send the data packets of connection `i`.
    fn flush(&mut self, i: usize, now: f64) {
        self.conns[i].prune(now);
        let resend_after = self.conns[i].resend_after();
        let mut parts: Vec<Part> = Vec::new();
        let mut resent = 0u64;
        {
            let c = &mut self.conns[i];
            for ch in 0..2 {
                let s = &mut c.send[ch];
                s.order.retain(|id| s.pending.contains_key(id));
                let mut in_flight = 0;
                for id in &s.order {
                    let Some(p) = s.pending.get_mut(id) else {
                        continue;
                    };
                    match p.last_sent {
                        Some(t) => {
                            in_flight += 1;
                            if now - t >= resend_after {
                                p.last_sent = Some(now);
                                parts.push(p.part.clone());
                                resent += 1;
                            }
                        }
                        None if in_flight < SEND_WINDOW => {
                            in_flight += 1;
                            p.last_sent = Some(now);
                            parts.push(p.part.clone());
                        }
                        None => break,
                    }
                    if parts.len() > MAX_PACKETS_PER_UPDATE * 2 {
                        break;
                    }
                }
            }
            parts.append(&mut c.unreliable);
            c.stats.resends += resent;
        }
        let c = &self.conns[i];
        let idle = now - c.last_send >= self.config.keepalive as f64;
        if parts.is_empty() && !c.ack_due && !idle {
            return;
        }
        let mut packets: Vec<(Vec<u8>, Vec<(Channel, u16)>)> = Vec::new();
        let mut current: Option<(Writer, Vec<(Channel, u16)>)> = None;
        for part in parts {
            let len = part.encoded_len();
            let fits = current
                .as_ref()
                .is_some_and(|(w, _)| DATA_HEADER + w.buf.len() + len <= MTU);
            if !fits {
                if let Some((w, rel)) = current.take() {
                    packets.push((w.buf, rel));
                }
                if packets.len() >= MAX_PACKETS_PER_UPDATE {
                    break;
                }
                current = Some((Writer::new(), Vec::new()));
            }
            let (w, rel) = current.as_mut().expect("packet started");
            part.write(w);
            if part.channel.reliable() {
                rel.push((part.channel, part.id));
            }
        }
        if let Some((w, rel)) = current.take() {
            packets.push((w.buf, rel));
        }
        if packets.is_empty() {
            packets.push((Vec::new(), Vec::new()));
        }
        for (body, reliable) in packets {
            let c = &mut self.conns[i];
            let seq = c.next_seq;
            c.next_seq = c.next_seq.wrapping_add(1);
            let mut w = Writer::new();
            header(&mut w, KIND_DATA);
            w.u32(c.token());
            w.u16(seq);
            w.u16(c.remote_seq);
            w.u32(c.recv_bits);
            debug_assert_eq!(w.buf.len(), DATA_HEADER);
            w.buf.extend_from_slice(&body);
            c.sent.insert(
                seq,
                SentPacket {
                    time: now,
                    reliable,
                },
            );
            c.stats.packets_sent += 1;
            c.stats.bytes_sent += w.buf.len() as u64;
            c.last_send = now;
            c.ack_due = false;
            let addr = c.addr;
            self.out(addr, w.buf);
        }
    }

    fn receive(&mut self, from: SocketAddr, data: &[u8], now: f64) {
        let mut r = Reader::new(data);
        if r.u16() != Some(MAGIC) || r.u8() != Some(VERSION) {
            return;
        }
        let Some(kind) = r.u8() else { return };
        match kind {
            KIND_CONNECT if self.is_server() => {
                let Some(nonce) = r.u64() else { return };
                self.on_connect_request(from, nonce, now);
            }
            KIND_ACCEPT | KIND_DENY if !self.is_server() => {
                let Some(nonce) = r.u64() else { return };
                let Some(i) = self
                    .conns
                    .iter()
                    .position(|c| c.addr == from && c.nonce == nonce)
                else {
                    return;
                };
                if kind == KIND_DENY {
                    let c = self.conns.remove(i);
                    self.retire(&c);
                    self.events.push_back(TransportEvent::Disconnected(
                        c.id,
                        DisconnectReason::ServerFull,
                    ));
                    return;
                }
                self.accept_client_side(i, now);
            }
            KIND_DATA => {
                let Some(token) = r.u32() else { return };
                let Some(i) = self
                    .conns
                    .iter()
                    .position(|c| c.addr == from && c.token() == token)
                else {
                    return;
                };
                // Data before the accept means the accept got lost: we're in.
                if !self.is_server() && self.conns[i].state != State::Connected {
                    self.accept_client_side(i, now);
                }
                self.on_data(i, &mut r, data.len(), now);
            }
            KIND_DISCONNECT => {
                let Some(token) = r.u32() else { return };
                if let Some(i) = self
                    .conns
                    .iter()
                    .position(|c| c.addr == from && c.token() == token)
                {
                    let c = self.conns.remove(i);
                    self.retire(&c);
                    if c.state == State::Connected {
                        self.events.push_back(TransportEvent::Disconnected(
                            c.id,
                            DisconnectReason::Closed,
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    fn accept_client_side(&mut self, i: usize, now: f64) {
        let c = &mut self.conns[i];
        if c.state == State::Connected {
            return;
        }
        c.state = State::Connected;
        c.last_recv = now;
        self.events.push_back(TransportEvent::Connected(c.id));
    }

    fn on_connect_request(&mut self, from: SocketAddr, nonce: u64, now: f64) {
        let answer = |kind: u8| {
            let mut w = Writer::new();
            header(&mut w, kind);
            w.u64(nonce);
            w.buf
        };
        if let Some(i) = self.conns.iter().position(|c| c.addr == from) {
            if self.conns[i].nonce == nonce {
                // The accept was lost: say it again.
                self.conns[i].last_recv = now;
                self.out(from, answer(KIND_ACCEPT));
                return;
            }
            // The same address with a new nonce: that client restarted.
            let old = self.conns.remove(i);
            self.retire(&old);
            self.events.push_back(TransportEvent::Disconnected(
                old.id,
                DisconnectReason::Closed,
            ));
        }
        if self.conns.len() >= self.max_connections {
            self.out(from, answer(KIND_DENY));
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.conns
            .push(Connection::new(id, from, nonce, State::Connected, now));
        self.events.push_back(TransportEvent::Connected(id));
        self.out(from, answer(KIND_ACCEPT));
    }

    fn on_data(&mut self, i: usize, r: &mut Reader, size: usize, now: f64) {
        let (Some(seq), Some(ack), Some(bits)) = (r.u16(), r.u16(), r.u32()) else {
            return;
        };
        let c = &mut self.conns[i];
        c.last_recv = now;
        c.stats.packets_received += 1;
        c.stats.bytes_received += size as u64;
        c.on_ack(ack, now);
        for b in 0..32 {
            if bits & (1 << b) != 0 {
                c.on_ack(ack.wrapping_sub(b + 1), now);
            }
        }
        if !c.on_receive_seq(seq) {
            return; // duplicate packet
        }
        c.ack_due = true;
        let mut out = Vec::new();
        while !r.is_empty() {
            let Some(part) = Part::read(r) else { break };
            c.on_part(part, now, &mut out);
        }
        let id = c.id;
        for (ch, msg) in out {
            self.events.push_back(TransportEvent::Message(id, ch, msg));
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Seconds-precision helper for tests and tools: run `a` and `b` for `seconds` of simulated
/// time in steps of `dt`, collecting their events.
#[cfg(test)]
pub(crate) fn pump(
    a: &mut Endpoint,
    b: &mut Endpoint,
    now: &mut f64,
    seconds: f64,
    dt: f64,
) -> (Vec<TransportEvent>, Vec<TransportEvent>) {
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    let end = *now + seconds;
    while *now < end {
        *now += dt;
        a.update(*now);
        b.update(*now);
        while let Some(e) = a.poll() {
            ea.push(e);
        }
        while let Some(e) = b.poll() {
            eb.push(e);
        }
    }
    (ea, eb)
}

/// The payloads of `channel` among `events` (tests).
#[cfg(test)]
fn messages(events: &[TransportEvent], channel: Channel) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            TransportEvent::Message(_, ch, m) if *ch == channel => Some(m.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn pair(net: &MemoryNet) -> (Endpoint, Endpoint, f64) {
        let server = Endpoint::server(Box::new(net.link(7777)), 8);
        let addr = server.local_addr();
        let client = Endpoint::client(Box::new(net.link(0)), addr, 0.0);
        (server, client, 0.0)
    }

    #[test]
    fn handshake_connects_and_messages_flow_both_ways() {
        let net = MemoryNet::default();
        let (mut s, mut c, mut now) = pair(&net);
        let (es, ec) = pump(&mut s, &mut c, &mut now, 0.1, 1.0 / 60.0);
        assert_eq!(es, [TransportEvent::Connected(1)]);
        assert_eq!(ec, [TransportEvent::Connected(0)]);
        s.send(1, Channel::ReliableOrdered, b"hello");
        c.send(0, Channel::Unreliable, b"hi");
        let (es, ec) = pump(&mut s, &mut c, &mut now, 0.1, 1.0 / 60.0);
        assert_eq!(messages(&ec, Channel::ReliableOrdered), [b"hello".to_vec()]);
        assert_eq!(messages(&es, Channel::Unreliable), [b"hi".to_vec()]);
        assert!(c.stats(0).unwrap().rtt > 0.0, "acks measure the round trip");
    }

    #[test]
    fn reliable_channels_survive_loss_and_ordered_stays_in_order() {
        let net = MemoryNet::default();
        let (mut s, mut c, mut now) = pair(&net);
        pump(&mut s, &mut c, &mut now, 0.2, 1.0 / 60.0);
        for e in [&mut s, &mut c] {
            e.simulator = Simulator {
                latency: 0.03,
                jitter: 0.04,
                loss: 0.3,
            };
        }
        s.seed(7);
        c.seed(11);
        let mut unordered = HashSet::new();
        let mut ec = Vec::new();
        for i in 0..200u32 {
            s.send(1, Channel::ReliableOrdered, &i.to_le_bytes());
            s.send(1, Channel::ReliableUnordered, &(1000 + i).to_le_bytes());
            unordered.insert((1000 + i).to_le_bytes().to_vec());
            if i % 20 == 0 {
                ec.extend(pump(&mut s, &mut c, &mut now, 1.0 / 60.0, 1.0 / 60.0).1);
            }
        }
        ec.extend(pump(&mut s, &mut c, &mut now, 5.0, 1.0 / 60.0).1);
        let ordered: Vec<u32> = messages(&ec, Channel::ReliableOrdered)
            .iter()
            .map(|m| u32::from_le_bytes(m[..4].try_into().unwrap()))
            .collect();
        assert_eq!(ordered, (0..200).collect::<Vec<_>>());
        let got: Vec<Vec<u8>> = messages(&ec, Channel::ReliableUnordered);
        assert_eq!(got.len(), 200, "each exactly once");
        assert_eq!(got.into_iter().collect::<HashSet<_>>(), unordered);
        let st = s.stats(1).unwrap();
        assert!(st.resends > 0 && st.packets_lost > 0, "{st:?}");
    }

    #[test]
    fn large_messages_are_fragmented_and_put_back_together() {
        let net = MemoryNet::default();
        let (mut s, mut c, mut now) = pair(&net);
        pump(&mut s, &mut c, &mut now, 0.2, 1.0 / 60.0);
        let big: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect();
        s.send(1, Channel::ReliableOrdered, &big);
        s.send(1, Channel::ReliableOrdered, b"after");
        s.send(1, Channel::Unreliable, &big[..5000]);
        s.simulator.loss = 0.2;
        s.seed(3);
        let (_, ec) = pump(&mut s, &mut c, &mut now, 3.0, 1.0 / 60.0);
        let rel = messages(&ec, Channel::ReliableOrdered);
        assert_eq!(rel.len(), 2);
        assert_eq!(rel[0], big);
        assert_eq!(rel[1], b"after");
        // Unreliable fragments arrive whole or not at all.
        for m in messages(&ec, Channel::Unreliable) {
            assert_eq!(m, big[..5000]);
        }
        assert!(s.send(1, Channel::ReliableOrdered, &vec![0; MAX_MESSAGE]));
        assert!(!s.send(1, Channel::ReliableOrdered, &vec![0; MAX_MESSAGE + 1]));
    }

    #[test]
    fn silence_times_out_and_goodbye_disconnects() {
        let net = MemoryNet::default();
        let (mut s, mut c, mut now) = pair(&net);
        pump(&mut s, &mut c, &mut now, 0.2, 1.0 / 60.0);
        // The client stops updating: the server times it out.
        s.config.timeout = 1.0;
        let mut events = Vec::new();
        for _ in 0..90 {
            now += 1.0 / 60.0;
            s.update(now);
            while let Some(e) = s.poll() {
                events.push(e);
            }
        }
        assert_eq!(
            events,
            [TransportEvent::Disconnected(1, DisconnectReason::Timeout)]
        );
        // A fresh client connects, then leaves politely.
        let mut c2 = Endpoint::client(Box::new(net.link(0)), s.local_addr(), now);
        let (es, _) = pump(&mut s, &mut c2, &mut now, 0.2, 1.0 / 60.0);
        assert_eq!(es, [TransportEvent::Connected(2)]);
        drop(c2);
        let mut got = Vec::new();
        now += 0.02;
        s.update(now);
        while let Some(e) = s.poll() {
            got.push(e);
        }
        assert_eq!(
            got,
            [TransportEvent::Disconnected(2, DisconnectReason::Closed)]
        );
        // Nobody answers: connecting fails after the connect timeout.
        drop(s);
        let mut lonely = Endpoint::client(
            Box::new(net.link(0)),
            SocketAddr::from(([127, 0, 0, 1], 7777)),
            now,
        );
        lonely.config.connect_timeout = 0.5;
        let mut got = Vec::new();
        for _ in 0..60 {
            now += 1.0 / 60.0;
            lonely.update(now);
            while let Some(e) = lonely.poll() {
                got.push(e);
            }
        }
        assert_eq!(
            got,
            [TransportEvent::Disconnected(
                0,
                DisconnectReason::ConnectFailed
            )]
        );
    }

    #[test]
    fn a_full_server_refuses_and_latency_delays_delivery() {
        let net = MemoryNet::default();
        let mut s = Endpoint::server(Box::new(net.link(7000)), 1);
        let mut a = Endpoint::client(Box::new(net.link(0)), s.local_addr(), 0.0);
        let mut b = Endpoint::client(Box::new(net.link(0)), s.local_addr(), 0.0);
        let mut now = 0.0;
        let mut eb = Vec::new();
        for _ in 0..30 {
            now += 1.0 / 60.0;
            for e in [&mut s, &mut a, &mut b] {
                e.update(now);
            }
            while s.poll().is_some() {}
            while a.poll().is_some() {}
            while let Some(e) = b.poll() {
                eb.push(e);
            }
        }
        assert_eq!(s.connections(), [1]);
        assert_eq!(
            eb,
            [TransportEvent::Disconnected(
                0,
                DisconnectReason::ServerFull
            )]
        );
        // 100 ms of one-way latency: a message takes about that long.
        s.simulator.latency = 0.1;
        s.send(1, Channel::ReliableOrdered, b"slow");
        let sent_at = now;
        let mut arrived = None;
        for _ in 0..30 {
            now += 1.0 / 120.0;
            s.update(now);
            a.update(now);
            while let Some(e) = a.poll() {
                if matches!(e, TransportEvent::Message(..)) {
                    arrived.get_or_insert(now);
                }
            }
        }
        let took = arrived.expect("delivered") - sent_at;
        assert!((0.09..0.13).contains(&took), "{took}");
    }

    #[test]
    fn real_udp_sockets_on_localhost() {
        let any = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut s = Endpoint::server(Box::new(UdpLink::bind(any).unwrap()), 4);
        let mut c = Endpoint::client(Box::new(UdpLink::bind(any).unwrap()), s.local_addr(), 0.0);
        let start = std::time::Instant::now();
        let mut now = 0.0;
        let mut got = None;
        let mut sent = false;
        while start.elapsed() < Duration::from_secs(10) && got.is_none() {
            now += 1.0 / 60.0;
            s.update(now);
            c.update(now);
            while let Some(e) = s.poll() {
                if let TransportEvent::Connected(id) = e {
                    s.send(id, Channel::ReliableOrdered, b"over udp");
                    sent = true;
                }
            }
            while let Some(e) = c.poll() {
                if let TransportEvent::Message(_, _, m) = e {
                    got = Some(m);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(sent);
        assert_eq!(got.as_deref(), Some(&b"over udp"[..]));
    }

    #[test]
    fn sequence_numbers_wrap() {
        assert!(seq_gt(1, 0));
        assert!(seq_gt(0, 65535));
        assert!(!seq_gt(65535, 0));
        assert!(!seq_gt(5, 5));
        let mut c = Connection::new(
            0,
            SocketAddr::from(([127, 0, 0, 1], 1)),
            1,
            State::Connected,
            0.0,
        );
        assert!(c.on_receive_seq(65534));
        assert!(c.on_receive_seq(1));
        assert!(c.on_receive_seq(65535));
        assert!(!c.on_receive_seq(65535), "duplicate");
        assert_eq!(c.remote_seq, 1);
        assert_eq!(c.recv_bits & 0b111, 0b110);
    }
}
