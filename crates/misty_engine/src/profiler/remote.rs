//! Profiling a running Player from the editor. The editor's Profiler listens on
//! [`DEFAULT_ADDR`] (Target ▸ Player); a Player started with
//! `MISTY_PROFILER_CONNECT=127.0.0.1:54998` (or `--profiler-connect <addr>`) connects, turns
//! recording on and streams each recorded frame as one JSON line, a few frames late so its GPU
//! times are in. Serialising and sending happen on a sender thread; when it falls behind,
//! frames are dropped rather than slowing the game.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{FrameData, HISTORY};

/// Where the editor listens by default.
pub const DEFAULT_ADDR: &str = "127.0.0.1:54998";
/// Environment variable a Player reads to connect.
pub const ENV: &str = "MISTY_PROFILER_CONNECT";

struct Sender {
    tx: SyncSender<FrameData>,
    /// Frames with a lower index were sent (or dropped).
    next: u64,
}

static SENDER: Mutex<Option<Sender>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Connect to an editor listening at `addr` and stream every recorded frame to it (recording is
/// switched on).
pub fn connect(addr: &str) -> std::io::Result<()> {
    let target = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other(format!("no address for {addr}")))?;
    let stream = TcpStream::connect_timeout(&target, Duration::from_secs(2))?;
    stream.set_nodelay(true)?;
    let (tx, rx) = std::sync::mpsc::sync_channel::<FrameData>(32);
    std::thread::Builder::new()
        .name("misty profiler sender".into())
        .spawn(move || {
            let mut out = BufWriter::new(stream);
            for frame in rx {
                let ok = serde_json::to_writer(&mut out, &frame).is_ok()
                    && out.write_all(b"\n").is_ok()
                    && out.flush().is_ok();
                if !ok {
                    break;
                }
            }
        })?;
    *lock(&SENDER) = Some(Sender {
        tx,
        next: super::frame_index(),
    });
    super::set_enabled(true);
    Ok(())
}

/// Streaming to an editor.
pub fn is_connected() -> bool {
    lock(&SENDER).is_some()
}

/// Stop streaming.
pub fn disconnect() {
    *lock(&SENDER) = None;
}

pub(super) fn frame_ended() {
    let mut guard = lock(&SENDER);
    let Some(s) = guard.as_mut() else {
        return;
    };
    let upto = super::frame_index().saturating_sub(super::capture::LAG);
    let next = s.next;
    let frames: Vec<FrameData> = super::with_frames(|frames| {
        frames
            .iter()
            .filter(|f| f.index >= next && f.index < upto)
            .cloned()
            .collect()
    });
    for f in frames {
        s.next = f.index + 1;
        match s.tx.try_send(f) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                *guard = None;
                return;
            }
        }
    }
}

/// The editor's end: accepts Player connections and keeps the frames they send.
pub struct Listener {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    frames: Mutex<VecDeque<FrameData>>,
    /// Address of the connected Player.
    peer: Mutex<Option<String>>,
    stop: AtomicBool,
}

impl Listener {
    /// Listen at `addr` ("127.0.0.1:54998"; port 0 picks a free one).
    pub fn bind(addr: &str) -> std::io::Result<Self> {
        let socket = TcpListener::bind(addr)?;
        socket.set_nonblocking(true)?;
        let addr = socket.local_addr()?;
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("misty profiler listener".into())
            .spawn(move || accept_loop(socket, worker))?;
        Ok(Self { addr, shared })
    }

    /// Read the received frames (oldest first).
    pub fn with_frames<R>(&self, f: impl FnOnce(&VecDeque<FrameData>) -> R) -> R {
        f(&lock(&self.shared.frames))
    }

    /// Address of the connected Player (`None` while waiting).
    pub fn peer(&self) -> Option<String> {
        lock(&self.shared.peer).clone()
    }

    pub fn clear(&self) {
        lock(&self.shared.frames).clear();
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn accept_loop(socket: TcpListener, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match socket.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                *lock(&shared.peer) = Some(peer.to_string());
                read_frames(stream, &shared);
                *lock(&shared.peer) = None;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// Frames from one Player until it disconnects (or the listener is dropped).
fn read_frames(stream: TcpStream, shared: &Shared) {
    let mut reader = BufReader::new(stream);
    // Bytes, not a String: a read timeout may split a UTF-8 character.
    let mut line: Vec<u8> = Vec::new();
    while !shared.stop.load(Ordering::Relaxed) {
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return,
            Ok(_) => {
                if line.ends_with(b"\n") {
                    if let Ok(f) = serde_json::from_slice::<FrameData>(&line) {
                        let mut frames = lock(&shared.frames);
                        frames.push_back(f);
                        while frames.len() > HISTORY {
                            frames.pop_front();
                        }
                    }
                    line.clear();
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}
