//! The session marker, `Library/session.lock`: which editor process has the project open,
//! since when, and whether it closed cleanly.
//!
//! Written when the project opens (`clean_exit: false`) and again on a normal exit
//! (`clean_exit: true`). Found with `clean_exit: false` on the next open, the last session
//! crashed or the machine lost power: its backups are offered back. A panic also records
//! `crashed: true`. Still `false` while that process runs (same pid, started before the marker
//! was written): another editor has the project open, and this one leaves the marker and the
//! backups alone.
//!
//! This is separate from the project lock (`Library/ProjectLock`, round 60), which stops two
//! editors from writing the same project; the marker only answers "did the last session end
//! cleanly?".

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The marker file, under the project.
pub const MARKER_FILE: &str = "Library/session.lock";

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A file's modification time in Unix ms.
pub fn mtime_ms(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// The contents of `Library/session.lock`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub pid: u32,
    /// When the session started (Unix ms).
    pub started_ms: u64,
    /// The session's id (also its backup folder's name).
    pub session: String,
    pub clean_exit: bool,
    #[serde(default)]
    pub crashed: bool,
    /// Editor version.
    #[serde(default)]
    pub version: String,
    /// The crash log written by a panic (project-relative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash_log: Option<String>,
}

impl Marker {
    pub fn load(root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(root.join(MARKER_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = root.join(MARKER_FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        misty_engine::asset_db::write_atomic(&path, text + "\n").map_err(|e| e.to_string())
    }
}

/// This editor's session of the project.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub pid: u32,
    pub started_ms: u64,
    /// `Library/Autosave/<id>`.
    pub dir: PathBuf,
}

impl Session {
    pub fn new(root: &Path, pid: u32, started_ms: u64) -> Self {
        let id = format!("{started_ms}-{pid}");
        let dir = root.join(super::AUTOSAVE_DIR).join(&id);
        Self {
            id,
            pid,
            started_ms,
            dir,
        }
    }

    pub fn marker(&self) -> Marker {
        Marker {
            pid: self.pid,
            started_ms: self.started_ms,
            session: self.id.clone(),
            clean_exit: false,
            crashed: false,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            crash_log: None,
        }
    }
}

/// What opening the project found.
#[derive(Clone, Debug)]
pub struct Started {
    pub session: Session,
    /// The marker of the session before this one, if there was one.
    pub previous: Option<Marker>,
    /// Another editor process has the project open (its marker is live): this one writes no
    /// marker and no backups.
    pub other_editor: bool,
}

/// Open a session: read the last marker, then write this one's.
pub fn start(root: &Path) -> Result<Started, String> {
    start_as(root, std::process::id(), now_ms())
}

/// [`start`] for process `pid` starting at `started_ms` (tests simulate sessions).
pub fn start_as(root: &Path, pid: u32, started_ms: u64) -> Result<Started, String> {
    let session = Session::new(root, pid, started_ms);
    let previous = Marker::load(root);
    let other_editor = previous
        .as_ref()
        .is_some_and(|m| !m.clean_exit && m.pid != pid && session_alive(m.pid, m.started_ms));
    if !other_editor {
        session.marker().save(root)?;
    }
    Ok(Started {
        session,
        previous,
        other_editor,
    })
}

/// A normal exit: the marker says so.
pub fn mark_clean(root: &Path, session: &Session) -> Result<(), String> {
    let mut m = Marker::load(root)
        .filter(|m| m.session == session.id)
        .unwrap_or_else(|| session.marker());
    m.clean_exit = true;
    m.save(root)
}

/// A panic: the marker says the session crashed (and where its log is).
pub fn mark_crashed(root: &Path, session: &Session, crash_log: Option<String>) -> Result<(), String> {
    let mut m = Marker::load(root)
        .filter(|m| m.session == session.id)
        .unwrap_or_else(|| session.marker());
    m.clean_exit = false;
    m.crashed = true;
    if crash_log.is_some() {
        m.crash_log = crash_log;
    }
    m.save(root)
}

/// Session folder name `<started ms>-<pid>` → (started ms, pid).
pub fn parse_session_id(id: &str) -> Option<(u64, u32)> {
    let (t, p) = id.split_once('-')?;
    Some((t.parse().ok()?, p.parse().ok()?))
}

/// Process `pid` runs and started before `started_ms` (so it is the process that wrote a marker
/// or a backup folder at that time, not a later one that got the same id after a reboot).
pub fn session_alive(pid: u32, started_ms: u64) -> bool {
    if pid == std::process::id() {
        return true;
    }
    process_start_ms(pid).is_some_and(|t| t <= started_ms + 2_000)
}

/// When process `pid` started (Unix ms), if it is running.
#[cfg(windows)]
fn process_start_ms(pid: u32) -> Option<u64> {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: plain Win32 calls; the handle is checked before use and closed once.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut code = 0u32;
        let (mut c, mut e, mut k, mut u) = Default::default();
        let running = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE;
        let timed = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0;
        CloseHandle(h);
        if !running || !timed {
            return None;
        }
        let ticks = (u64::from(c.high) << 32) | u64::from(c.low);
        // 100 ns ticks since 1601 → ms since 1970.
        Some(ticks.checked_sub(116_444_736_000_000_000)? / 10_000)
    }
}

#[cfg(not(windows))]
fn process_start_ms(pid: u32) -> Option<u64> {
    // Linux: /proc/<pid> exists while the process runs, made when it started.
    mtime_ms(&PathBuf::from(format!("/proc/{pid}")))
}

// ---- local time -----------------------------------------------------------------------------

/// A calendar date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Unix ms → UTC date and time.
pub fn utc(ms: u64) -> DateTime {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    DateTime {
        year,
        month,
        day,
        hour: (rem / 3600) as u32,
        minute: ((rem / 60) % 60) as u32,
        second: (rem % 60) as u32,
    }
}

/// Unix ms → local date and time (the system's time zone on Windows; UTC elsewhere).
pub fn local(ms: u64) -> DateTime {
    to_local(utc(ms)).unwrap_or_else(|| utc(ms))
}

#[cfg(windows)]
fn to_local(u: DateTime) -> Option<DateTime> {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime16 {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const c_void,
            utc: *const SystemTime16,
            local: *mut SystemTime16,
        ) -> i32;
    }
    let st = SystemTime16 {
        year: u.year as u16,
        month: u.month as u16,
        day: u.day as u16,
        hour: u.hour as u16,
        minute: u.minute as u16,
        second: u.second as u16,
        ..Default::default()
    };
    let mut out = SystemTime16::default();
    // SAFETY: both structs are valid SYSTEMTIMEs; a null zone means the current one.
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &st, &mut out) } != 0;
    ok.then(|| DateTime {
        year: i32::from(out.year),
        month: u32::from(out.month),
        day: u32::from(out.day),
        hour: u32::from(out.hour),
        minute: u32::from(out.minute),
        second: u32::from(out.second),
    })
}

#[cfg(not(windows))]
fn to_local(_: DateTime) -> Option<DateTime> {
    None
}

/// "14:32" (local time).
pub fn local_clock(ms: u64) -> String {
    let t = local(ms);
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// "2026-11-14 14:32" (local time).
pub fn local_date_time(ms: u64) -> String {
    let t = local(ms);
    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

/// "20261114-143205" (local time, for file names).
pub fn local_stamp(ms: u64) -> String {
    let t = local(ms);
    format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}
