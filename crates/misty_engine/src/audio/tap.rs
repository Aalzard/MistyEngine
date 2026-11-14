//! Taps on the final mix: a copy of what the graph sends to the speakers, for the Recorder.
//!
//! [`tap`] makes a pair: the [`TapWriter`] goes to the graph ([`super::Audio::capture_tap`]),
//! the [`MixTap`] stays with whoever records. Every block the graph renders is pushed into each
//! tap's lock-free ring together with the mix's sample clock, so the reader knows exactly where
//! in time its samples belong. With no tap the graph does nothing extra (an empty list).
//!
//! While the Recorder runs, the graph also runs in capture time ([`super::Audio::start_capture`]):
//! it renders exactly as many samples as the game advanced, so a recording slower or faster
//! than real time still gets one sample period of sound per sample period of game time, and
//! the tap's sample clock counts capture time (sample `n` = second `n / rate` of capture time).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::dsp::Frame;

/// Seconds of sound a tap holds before its reader must have taken them (more is dropped and
/// replaced by silence, so the sample count, and so the sync, stays right).
pub const TAP_SECONDS: f32 = 4.0;

/// Set by the audio thread on the first block, read by the recorder.
struct Shared {
    /// Sample clock of the first sample in the tap (`u64::MAX` until the first block).
    start: AtomicU64,
    /// Sample rate (f32 bits).
    rate: AtomicU32,
    /// Samples that did not fit (the reader adds silence for them).
    dropped: AtomicU64,
}

/// The graph's end of a tap.
pub struct TapWriter {
    tx: rtrb::Producer<Frame>,
    shared: Arc<Shared>,
}

/// The reader's end of a tap: the mix as it is rendered, with its sample clock.
pub struct MixTap {
    rx: rtrb::Consumer<Frame>,
    shared: Arc<Shared>,
}

/// A tap holding `frames` samples at most.
pub fn tap(frames: usize) -> (TapWriter, MixTap) {
    let (tx, rx) = rtrb::RingBuffer::new(frames.max(1));
    let shared = Arc::new(Shared {
        start: AtomicU64::new(u64::MAX),
        rate: AtomicU32::new(0),
        dropped: AtomicU64::new(0),
    });
    (
        TapWriter {
            tx,
            shared: shared.clone(),
        },
        MixTap { rx, shared },
    )
}

/// A tap sized for [`TAP_SECONDS`] at 48 kHz (enough at any common device rate).
pub fn tap_for_recording() -> (TapWriter, MixTap) {
    tap((48_000.0 * TAP_SECONDS) as usize)
}

impl TapWriter {
    /// The reader is gone: the graph drops this tap.
    pub fn abandoned(&self) -> bool {
        self.tx.is_abandoned()
    }

    /// Copy one rendered block whose first sample is number `clock` of the mix (audio thread:
    /// no allocation, no locks).
    pub fn write(&mut self, clock: u64, rate: f32, frames: &[Frame]) {
        if self.shared.start.load(Ordering::Relaxed) == u64::MAX {
            self.shared.rate.store(rate.to_bits(), Ordering::Relaxed);
            self.shared.start.store(clock, Ordering::Release);
        }
        let (_, rest) = self.tx.push_partial_slice(frames);
        if !rest.is_empty() {
            self.shared
                .dropped
                .fetch_add(rest.len() as u64, Ordering::Relaxed);
        }
    }
}

impl MixTap {
    /// Sample clock of the tap's first sample (`None` until the graph rendered into it).
    pub fn start(&self) -> Option<u64> {
        match self.shared.start.load(Ordering::Acquire) {
            u64::MAX => None,
            s => Some(s),
        }
    }

    /// The mix's sample rate (`None` until the graph rendered into it).
    pub fn sample_rate(&self) -> Option<f32> {
        self.start()?;
        Some(f32::from_bits(self.shared.rate.load(Ordering::Relaxed)))
    }

    /// Append every sample waiting in the tap to `out` (silence for any that were dropped);
    /// returns how many were added.
    pub fn read(&mut self, out: &mut Vec<Frame>) -> usize {
        let before = out.len();
        let n = self.rx.slots();
        if let Ok(chunk) = self.rx.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            chunk.commit_all();
        }
        let dropped = self.shared.dropped.swap(0, Ordering::Relaxed) as usize;
        out.resize(out.len() + dropped, Frame::ZERO);
        out.len() - before
    }

    /// The graph that fed this tap is gone (its sound stopped for good).
    pub fn closed(&self) -> bool {
        self.rx.is_abandoned()
    }
}
