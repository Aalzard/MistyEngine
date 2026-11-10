//! The live editor's local connection: a loopback TCP listener with a token, and the
//! `misty --mcp-connect` stdio proxy that MCP clients start to reach it.
//!
//! The editor listens on `127.0.0.1` at a port the OS picks and writes `{port, token, pid}` to
//! `<project>/Library/MistyMcp.json` (removed when the bridge stops). A client connects, sends
//! one line `{"token": "…"}` and gets `{"ok": true}` back (or `{"ok": false, "error": …}` and
//! the connection closed); after that the connection carries MCP JSON-RPC messages, one per
//! line, both ways. Socket threads only move lines: every message is handled on the editor's
//! main thread (see [`super::live`]).

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Where the running editor says how to reach it, inside the project.
pub const INFO_FILE: &str = "Library/MistyMcp.json";
/// Longest message line accepted (requests are small; this only bounds memory).
const MAX_LINE: usize = 16 * 1024 * 1024;
/// How long a new connection has to send its token.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The contents of [`INFO_FILE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
}

impl Info {
    pub fn read(project: &Path) -> Result<Info, String> {
        let path = project.join(INFO_FILE);
        let text = std::fs::read_to_string(&path).map_err(|_| {
            format!(
                "no running Misty editor with AI tools on for {} ({INFO_FILE} is missing). {}",
                project.display(),
                "Open the project in the editor and turn on Edit > Preferences > AI Tools > Allow AI tools (MCP)."
            )
        })?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Something a connection thread hands the main thread.
pub enum Event {
    Connected(u64),
    /// One message line and where its replies go.
    Message {
        conn: u64,
        line: String,
        reply: Sender<String>,
    },
    Closed(u64),
}

/// The listener while the bridge is on. Dropping it stops listening and removes the file.
pub struct Server {
    pub events: Receiver<Event>,
    pub port: u16,
    stop: Arc<AtomicBool>,
    info_path: Option<PathBuf>,
    info: Info,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Listen on a free loopback port with a fresh token and write [`INFO_FILE`].
    /// `wake` is called (from socket threads) when a message arrives.
    pub fn start(
        project: &Path,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Server, String> {
        let token = super::random_token()?;
        let mut server = Self::listen(token, wake)?;
        let path = project.join(INFO_FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(&server.info).map_err(|e| e.to_string())?;
        write_private(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        server.info_path = Some(path);
        Ok(server)
    }

    /// Listen with `token`, without writing the info file (tests).
    pub fn listen(
        token: String,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Server, String> {
        // Loopback only: nothing off this computer can connect.
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|e| format!("could not listen on 127.0.0.1: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let info = Info {
            port,
            token: token.clone(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
        };
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("misty mcp listener".into())
                .spawn(move || accept_loop(listener, token, tx, wake, stop))
                .map_err(|e| e.to_string())?
        };
        Ok(Server {
            events: rx,
            port,
            stop,
            info_path: None,
            info,
            thread: Some(thread),
        })
    }

    #[cfg(test)]
    pub fn info(&self) -> &Info {
        &self.info
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(path) = &self.info_path
            // Only our own file: another editor may have taken the project over since.
            && Info::read(path.parent().and_then(Path::parent).unwrap_or(Path::new("")))
                .is_ok_and(|i| i.pid == self.info.pid && i.port == self.info.port)
        {
            // Windows refuses for a moment while another program (an indexer, a virus
            // scanner) has the new file open: try a few times.
            for _ in 0..20 {
                match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                    _ => break,
                }
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Write a file only the current user can read where the OS supports it (the token is in it).
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(text.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text)
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    tx: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    stop: Arc<AtomicBool>,
) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let conn = NEXT.fetch_add(1, Ordering::SeqCst);
                let (token, tx, wake, stop) =
                    (token.clone(), tx.clone(), wake.clone(), stop.clone());
                let _ = std::thread::Builder::new()
                    .name(format!("misty mcp connection {conn}"))
                    .spawn(move || serve_connection(stream, conn, &token, tx, wake, stop));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// Read one `\n`-terminated line (without it); `Ok(None)` at the end of the stream. Read
/// timeouts are retried until `stop` is set (a partial line stays in `buf` meanwhile).
fn read_line(
    r: &mut impl BufRead,
    buf: &mut Vec<u8>,
    stop: &AtomicBool,
) -> std::io::Result<Option<String>> {
    loop {
        // At most one byte past the limit: a client streaming bytes without a newline can't
        // grow the buffer without end (the limit used to be checked only on a timeout).
        let room = (MAX_LINE + 1).saturating_sub(buf.len()) as u64;
        match r.by_ref().take(room).read_until(b'\n', buf) {
            Ok(0) if buf.is_empty() => return Ok(None),
            Ok(_) if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') => {
                return Err(std::io::Error::new(ErrorKind::InvalidData, "line too long"));
            }
            // A whole line, or the last one without a newline.
            Ok(_) => {
                let mut line = std::mem::take(buf);
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| std::io::Error::new(ErrorKind::InvalidData, "not UTF-8"));
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if stop.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                if buf.len() > MAX_LINE {
                    return Err(std::io::Error::new(ErrorKind::InvalidData, "line too long"));
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn serve_connection(
    stream: TcpStream,
    conn: u64,
    token: &str,
    tx: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    stop: Arc<AtomicBool>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(&stream);
    let mut buf = Vec::new();
    // Handshake: the token first, within a few seconds.
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let never = AtomicBool::new(true);
    let hello = read_line(&mut reader, &mut buf, &never).ok().flatten();
    let given = hello
        .as_deref()
        .and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .and_then(|v| v.get("token").and_then(|t| t.as_str()).map(str::to_owned));
    let ok = given
        .as_deref()
        .is_some_and(|t| super::same_secret(t, token));
    let answer = if ok {
        json!({ "ok": true, "server": "Misty Engine", "version": env!("CARGO_PKG_VERSION") })
    } else {
        json!({ "ok": false, "error": "wrong or missing token" })
    };
    let _ = writeln!(writer, "{answer}");
    let _ = writer.flush();
    if !ok {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let (reply_tx, reply_rx) = channel::<String>();
    // Replies are written as they come (tool calls may finish out of order).
    let writer_thread = std::thread::Builder::new()
        .name(format!("misty mcp writer {conn}"))
        .spawn(move || {
            for line in reply_rx {
                if writeln!(writer, "{line}")
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            let _ = writer.shutdown(std::net::Shutdown::Write);
        });
    let _ = tx.send(Event::Connected(conn));
    wake();
    while let Ok(Some(line)) = read_line(&mut reader, &mut buf, &stop) {
        if line.trim().is_empty() {
            continue;
        }
        let msg = Event::Message {
            conn,
            line,
            reply: reply_tx.clone(),
        };
        if tx.send(msg).is_err() {
            break;
        }
        wake();
    }
    let _ = tx.send(Event::Closed(conn));
    wake();
    // The writer ends once the editor has sent (or dropped) every pending reply.
    drop(reply_tx);
    if let Ok(t) = writer_thread {
        let _ = t.join();
    }
}

// ---- the client side ---------------------------------------------------------------------------

/// Connect to the editor at `port` and authenticate.
pub fn connect(port: u16, token: &str) -> Result<TcpStream, String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| {
        format!("could not reach the editor on 127.0.0.1:{port}: {e} (is it still running?)")
    })?;
    let _ = stream.set_nodelay(true);
    let mut w = stream.try_clone().map_err(|e| e.to_string())?;
    writeln!(w, "{}", json!({ "token": token })).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let mut first = Vec::new();
    // Byte by byte: no buffer may swallow MCP messages that follow the answer.
    let mut byte = [0u8; 1];
    loop {
        match (&stream).read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => first.push(byte[0]),
            Err(e) => return Err(format!("no answer from the editor: {e}")),
        }
        if first.len() > 4096 {
            return Err("unexpected answer from the editor".into());
        }
    }
    let answer: serde_json::Value = serde_json::from_slice(&first)
        .map_err(|_| "unexpected answer from the editor".to_owned())?;
    if answer.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let why = answer
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("refused");
        return Err(format!("the editor refused the connection: {why}"));
    }
    let _ = stream.set_read_timeout(None);
    Ok(stream)
}

/// Copy lines from `input` to the editor and the editor's lines to `output`, until the editor
/// closes the connection (after `input` ends and the last replies are in).
pub fn proxy(
    stream: TcpStream,
    input: impl BufRead + Send + 'static,
    mut output: impl Write,
) -> Result<(), String> {
    let mut to_editor = stream.try_clone().map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("misty mcp proxy".into())
        .spawn(move || {
            for line in input.lines() {
                let Ok(line) = line else { break };
                if writeln!(to_editor, "{line}")
                    .and_then(|_| to_editor.flush())
                    .is_err()
                {
                    return;
                }
            }
            let _ = to_editor.shutdown(std::net::Shutdown::Write);
        })
        .map_err(|e| e.to_string())?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        writeln!(output, "{line}").map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub const CONNECT_USAGE: &str = "misty --mcp-connect [<project folder>]";

/// `misty --mcp-connect [project]`: a stdio MCP server that forwards to the running editor.
pub fn connect_main(args: &[String]) -> i32 {
    let project = match args {
        [] => std::env::current_dir().unwrap_or_default(),
        [p] => PathBuf::from(p),
        _ => {
            eprintln!("usage: {CONNECT_USAGE}");
            return 2;
        }
    };
    let run = || -> Result<(), String> {
        let info = Info::read(&project)?;
        let stream = connect(info.port, &info.token)?;
        eprintln!(
            "Connected to the Misty editor for {} (127.0.0.1:{}).",
            project.display(),
            info.port
        );
        let stdin = std::io::stdin();
        proxy(stream, BufReader::new(stdin), std::io::stdout())
    };
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("misty --mcp-connect: {e}");
            1
        }
    }
}

#[cfg(test)]
mod line_tests {
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;

    use super::{MAX_LINE, read_line};

    /// A line longer than the limit is refused once the limit is reached, without reading
    /// (and buffering) the rest of it.
    #[test]
    fn overlong_lines_are_refused_at_the_limit() {
        let stop = AtomicBool::new(false);
        let mut data = vec![b'a'; MAX_LINE + 4096];
        data.extend_from_slice(b"\n{}\n");
        let mut r = Cursor::new(data);
        let mut buf = Vec::new();
        assert!(read_line(&mut r, &mut buf, &stop).is_err());
        assert!(buf.len() <= MAX_LINE + 1, "{}", buf.len());
        assert!(r.position() as usize <= MAX_LINE + 1);
        // Normal lines still read, the last one without a newline too.
        let mut r = Cursor::new(b"{\"a\": 1}\r\nlast".to_vec());
        let mut buf = Vec::new();
        assert_eq!(read_line(&mut r, &mut buf, &stop).unwrap().unwrap(), "{\"a\": 1}");
        assert_eq!(read_line(&mut r, &mut buf, &stop).unwrap().unwrap(), "last");
        assert_eq!(read_line(&mut r, &mut buf, &stop).unwrap(), None);
    }
}
