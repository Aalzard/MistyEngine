//! wgpu errors that no error scope captured (validation, out of memory, internal).
//!
//! wgpu's default handler panics on whichever thread meets the error. Since frame encoding
//! was split (`frame_encoder.rs`) that can be a worker of rayon's pool finishing a part, and a
//! panic in `rayon::spawn` aborts the whole process: no panic hook, no flush of pending saves,
//! no crash log. [`install`] replaces that handler on the editor's and the player's devices:
//! the error is printed to stderr, counted and queued for the Console / the player's log
//! ([`take_new`]), and the program keeps running (wgpu skips an invalid command buffer at
//! submit; the next frame is recorded afresh). Each distinct message is reported once, so an
//! error repeated every frame doesn't flood the Console.
//!
//! Test devices keep the default (a panic), so a GPU test can't pass over a validation error.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Reported {
    /// Messages reported so far (at most [`SEEN_MAX`]; past that nothing new is reported).
    seen: HashSet<String>,
    /// Reported and not taken by [`take_new`] yet (at most [`QUEUED_MAX`]).
    queued: Vec<String>,
}

static REPORTED: Mutex<Option<Reported>> = Mutex::new(None);

const SEEN_MAX: usize = 256;
const QUEUED_MAX: usize = 64;

/// Handle `device`'s uncaptured errors as described in the module docs.
pub fn install(device: &wgpu::Device) {
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| record(&e)));
}

fn record(e: &wgpu::Error) {
    COUNT.fetch_add(1, Ordering::Relaxed);
    let kind = match e {
        wgpu::Error::OutOfMemory { .. } => "out of GPU memory",
        wgpu::Error::Validation { .. } => "GPU validation error",
        wgpu::Error::Internal { .. } => "internal GPU error",
    };
    let message = format!("{kind}: {e}");
    let mut guard = REPORTED.lock().unwrap_or_else(|e| e.into_inner());
    let r = guard.get_or_insert_with(Default::default);
    if r.seen.len() >= SEEN_MAX || !r.seen.insert(message.clone()) {
        return;
    }
    eprintln!("{message}");
    if r.queued.len() < QUEUED_MAX {
        r.queued.push(message);
    }
}

/// Uncaptured errors since the process started (repeats included).
pub fn count() -> u64 {
    COUNT.load(Ordering::Relaxed)
}

/// Errors reported since the last call (each distinct message once).
pub fn take_new() -> Vec<String> {
    let mut guard = REPORTED.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_mut()
        .map(|r| std::mem::take(&mut r.queued))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A validation error in a part of a frame finished on a rayon worker: reported and
    /// counted, and the process (this test) carries on and can submit the next frame. With
    /// wgpu's default handler the worker panicked, which aborts the process.
    #[test]
    fn a_validation_error_in_a_worker_finished_part_is_reported_not_fatal() {
        let Some((device, queue)) = super::super::gpu_tests::headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        install(&device);
        let before = count();
        let buffer = |size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty gpu error test"),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let src = buffer(1024, wgpu::BufferUsages::COPY_SRC);
        let small = buffer(16, wgpu::BufferUsages::COPY_DST);
        let mut frame = super::super::frame_encoder::FrameEncoder::new(&device);
        // Copies past the end of `small`: invalid.
        frame.copy_buffer_to_buffer(&src, 0, &small, 0, 1024);
        frame.split(&device);
        frame.copy_buffer_to_buffer(&src, 0, &small, 0, 16);
        queue.submit(frame.finish());
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(count() > before, "the error was not reported");
        let new = take_new();
        assert!(
            new.iter().any(|m| m.starts_with("GPU validation error")),
            "{new:?}"
        );
        // The device still works: a valid frame goes through.
        let mut frame = super::super::frame_encoder::FrameEncoder::new(&device);
        frame.copy_buffer_to_buffer(&src, 0, &small, 0, 16);
        frame.split(&device);
        queue.submit(frame.finish());
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        // The same error again is counted but not reported a second time.
        let mut frame = super::super::frame_encoder::FrameEncoder::new(&device);
        frame.copy_buffer_to_buffer(&src, 0, &small, 0, 1024);
        frame.split(&device);
        let repeated = count();
        queue.submit(frame.finish());
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(count() > repeated);
        let again = take_new();
        assert!(
            again.iter().all(|m| !new.contains(m)),
            "a repeated error was reported again: {again:?}"
        );
    }
}
