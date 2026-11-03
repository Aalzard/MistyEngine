//! Profiler capture file: one JSON object per recorded frame (JSON Lines), written a few
//! frames late so the GPU times have arrived.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;

/// Frames a capture waits before writing one (GPU timings arrive late).
const LAG: u64 = 4;

struct Capture {
    out: BufWriter<File>,
    /// Frames with a lower index are written.
    next: u64,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

/// Start recording into `path` (recording is switched on).
pub fn start(path: &Path) -> std::io::Result<()> {
    let out = BufWriter::new(File::create(path)?);
    *lock() = Some(Capture {
        out,
        next: super::frame_index(),
    });
    super::set_enabled(true);
    Ok(())
}

/// Write the remaining frames and close the file.
pub fn finish() -> std::io::Result<()> {
    match lock().take() {
        Some(mut c) => {
            write_up_to(&mut c, u64::MAX)?;
            c.out.flush()
        }
        None => Ok(()),
    }
}

pub fn is_active() -> bool {
    lock().is_some()
}

pub(super) fn frame_ended() {
    let mut guard = lock();
    if let Some(c) = guard.as_mut() {
        let upto = super::frame_index().saturating_sub(LAG);
        if write_up_to(c, upto).is_err() {
            *guard = None;
        }
    }
}

fn write_up_to(c: &mut Capture, upto: u64) -> std::io::Result<()> {
    let lines = super::with_frames(|frames| {
        frames
            .iter()
            .filter(|f| f.index >= c.next && f.index < upto)
            .map(|f| (f.index, serde_json::to_string(f)))
            .collect::<Vec<_>>()
    });
    for (index, line) in lines {
        writeln!(c.out, "{}", line.map_err(std::io::Error::other)?)?;
        c.next = index + 1;
    }
    Ok(())
}

fn lock() -> std::sync::MutexGuard<'static, Option<Capture>> {
    CAPTURE.lock().unwrap_or_else(|e| e.into_inner())
}
