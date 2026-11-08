//! Lightweight profiler: scoped timings per frame, kept for the last [`HISTORY`] frames.
//!
//! `profile_scope!("Physics", Category::Physics)` times the rest of the enclosing block while
//! recording is on. When it is off a scope costs one relaxed atomic load and its name
//! expression is never evaluated.
//!
//! Samples are kept per thread. The thread that calls [`end_frame`] (the main loop) is the
//! Main Thread row; other threads (rayon workers, the background pool, the audio thread) hand
//! their finished top-level scopes to a shared buffer that [`end_frame`] collects into
//! [`FrameData::threads`] for the Timeline. Module counters ([`Counter`]: physics bodies, audio
//! voices, UI elements…) and the GPU time of each render pass ([`GpuPass`]) are attached to the
//! frame too.

pub mod capture;
mod counters;
mod frame;
mod gpu;
pub mod remote;
mod threads;

pub use counters::{Counter, CounterGroup, add_counter, set_counter};
pub use frame::{FrameData, HierarchyNode, HierarchyTree, Sample};
pub use gpu::{GpuGroup, GpuPass, GpuTiming, add_gpu_passes, gpu_timing, set_gpu_timing};
pub use threads::{ThreadKind, ThreadTrack, name_thread};

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::render::{RenderMemory, RenderStats};

/// Frames kept for the Profiler window.
pub const HISTORY: usize = 300;
/// Samples one thread may record per frame before new scopes are ignored.
const MAX_SAMPLES: usize = 200_000;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Index of the frame being recorded.
static FRAME: AtomicU64 = AtomicU64::new(0);
/// When the frame being recorded began ([`now_ns`]; the previous [`end_frame`]).
static FRAME_START: AtomicU64 = AtomicU64::new(0);
static FRAMES: Mutex<History> = Mutex::new(History {
    frames: VecDeque::new(),
    late: Vec::new(),
});

/// Profiler module a scope's time is charted under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Rendering,
    Scripts,
    Physics,
    Animation,
    Navigation,
    Audio,
    Ui,
    Others,
}

impl Category {
    pub const COUNT: usize = 8;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Rendering,
        Self::Scripts,
        Self::Physics,
        Self::Animation,
        Self::Navigation,
        Self::Audio,
        Self::Ui,
        Self::Others,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rendering => "Rendering",
            Self::Scripts => "Scripts",
            Self::Physics => "Physics",
            Self::Animation => "Animation",
            Self::Navigation => "Navigation",
            Self::Audio => "Audio",
            Self::Ui => "UI",
            Self::Others => "Others",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Engine/editor counters attached to a frame.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryStats {
    pub entities: usize,
    pub render: RenderMemory,
}

/// Is recording on?
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turn recording on or off (the Profiler window's Record button). Turning it on makes the
/// calling thread the Main Thread.
pub fn set_enabled(on: bool) {
    if on {
        threads::claim_main();
    }
    ENABLED.store(on, Ordering::Relaxed);
}

/// Index of the frame currently being recorded.
pub fn frame_index() -> u64 {
    FRAME.load(Ordering::Relaxed)
}

/// Nanoseconds since the profiler's clock started (the time base of every sample).
pub fn now_ns() -> u64 {
    static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);
    EPOCH.elapsed().as_nanos() as u64
}

/// A timed scope as recorded (absolute times; converted into a [`Sample`] per frame).
#[derive(Clone, Debug)]
pub(crate) struct RawSample {
    pub name: Cow<'static, str>,
    pub category: Category,
    pub depth: u16,
    pub start_ns: u64,
    /// 0 while the scope is open.
    pub duration_ns: u64,
}

impl RawSample {
    /// As a [`Sample`] of the frame that began at `frame_start` (ns).
    pub fn relative(self, frame_start: u64) -> Sample {
        Sample {
            name: self.name,
            category: self.category,
            depth: self.depth,
            start_ms: (self.start_ns as i64 - frame_start as i64) as f32 / 1.0e6,
            duration_ms: self.duration_ns as f32 / 1.0e6,
        }
    }
}

/// Samples of the frame being recorded on this thread.
#[derive(Default)]
struct Recorder {
    samples: Vec<RawSample>,
    /// Categories of the open scopes.
    stack: Vec<Category>,
    render: RenderStats,
    memory: MemoryStats,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder::default());
}

/// An open scope; records its duration when dropped.
pub struct Scope {
    index: usize,
    start_ns: u64,
}

impl Scope {
    /// Open a scope if recording is on; `name` is only called then.
    #[inline]
    pub fn begin_lazy<N: Into<Cow<'static, str>>>(
        name: impl FnOnce() -> N,
        category: Option<Category>,
    ) -> Option<Self> {
        if !enabled() {
            return None;
        }
        Self::begin(name().into(), category)
    }

    /// Open a scope; it inherits the parent's category when `category` is `None`.
    pub fn begin(name: Cow<'static, str>, category: Option<Category>) -> Option<Self> {
        RECORDER.with_borrow_mut(|r| {
            if r.samples.len() >= MAX_SAMPLES {
                return None;
            }
            let start_ns = now_ns();
            let category = category
                .or(r.stack.last().copied())
                .unwrap_or(Category::Others);
            let index = r.samples.len();
            r.samples.push(RawSample {
                name,
                category,
                depth: r.stack.len() as u16,
                start_ns,
                duration_ns: 0,
            });
            r.stack.push(category);
            Some(Self { index, start_ns })
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let duration = now_ns().saturating_sub(self.start_ns).max(1);
        RECORDER.with_borrow_mut(|r| {
            if let Some(s) = r.samples.get_mut(self.index) {
                s.duration_ns = duration;
            }
            r.stack.pop();
            // A worker finished its outermost scope: hand the samples to the Timeline.
            if r.stack.is_empty() && !threads::is_main() {
                threads::flush(&mut r.samples);
            }
        });
    }
}

/// Time the rest of the enclosing block: `profile_scope!("Physics", Category::Physics)`,
/// or `profile_scope!(name)` to inherit the parent scope's category. The name may be any
/// expression giving `&'static str` or `String`; it is only evaluated while recording.
#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        let _profile_scope = $crate::profiler::Scope::begin_lazy(|| $name, None);
    };
    ($name:expr, $category:expr) => {
        let _profile_scope = $crate::profiler::Scope::begin_lazy(|| $name, Some($category));
    };
}

/// Rendering counters of the frame being recorded (the view the Profiler reports on).
pub fn set_render_stats(stats: RenderStats) {
    if enabled() {
        RECORDER.with_borrow_mut(|r| r.render = stats);
    }
}

/// Memory counters of the frame being recorded.
pub fn set_memory(memory: MemoryStats) {
    if enabled() {
        RECORDER.with_borrow_mut(|r| r.memory = memory);
    }
}

/// GPU time measured for frame `index` (arrives a frame or two later).
pub fn add_gpu_time(index: u64, ms: f32) {
    if !enabled() {
        return;
    }
    late(index, |gpu_ms, _| *gpu_ms.get_or_insert(0.0) += ms);
}

/// GPU results that arrived before their frame ended.
#[derive(Default)]
struct Late {
    index: u64,
    gpu_ms: Option<f32>,
    passes: Vec<GpuPass>,
}

/// Apply late data (GPU time, GPU passes) to frame `index`: to the recorded frame when it is in
/// the history, else keep it until the frame ends.
fn late(index: u64, apply: impl FnOnce(&mut Option<f32>, &mut Vec<GpuPass>)) {
    let mut h = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = h.frames.iter_mut().rev().find(|f| f.index == index) {
        apply(&mut f.gpu_ms, &mut f.gpu_passes);
    } else if index >= frame_index() {
        let slot = match h.late.iter().position(|l| l.index == index) {
            Some(i) => i,
            None => {
                h.late.push(Late {
                    index,
                    ..Default::default()
                });
                h.late.len() - 1
            }
        };
        let l = &mut h.late[slot];
        apply(&mut l.gpu_ms, &mut l.passes);
    }
}

struct History {
    frames: VecDeque<FrameData>,
    /// GPU results for frames that haven't ended yet.
    late: Vec<Late>,
}

/// Close the frame recorded on this thread (which becomes the Main Thread): its samples,
/// counters, other threads' samples and wall-clock time since the previous call go into the
/// history (while recording).
pub fn end_frame() {
    let now = now_ns();
    let started = FRAME_START.swap(now, Ordering::Relaxed);
    threads::claim_main();
    let index = FRAME.fetch_add(1, Ordering::Relaxed);
    let recorded = RECORDER.with_borrow_mut(std::mem::take);
    let counters = counters::take();
    let tracks = threads::collect(started);
    if !enabled() {
        return;
    }
    let cpu_ms = match started {
        0 => 0.0,
        s => (now - s) as f32 / 1.0e6,
    };
    let samples = recorded
        .samples
        .into_iter()
        .map(|s| s.relative(started))
        .collect();
    let mut frame = FrameData::new(index, samples, cpu_ms);
    frame.render = recorded.render;
    frame.memory = recorded.memory;
    frame.counters = counters;
    frame.threads = tracks;
    frame.gpu_timing = gpu_timing();
    let mut h = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    h.late.retain_mut(|l| {
        if l.index == index {
            if let Some(ms) = l.gpu_ms {
                *frame.gpu_ms.get_or_insert(0.0) += ms;
            }
            frame.gpu_passes.append(&mut l.passes);
        }
        l.index > index
    });
    h.frames.push_back(frame);
    while h.frames.len() > HISTORY {
        h.frames.pop_front();
    }
    drop(h);
    capture::frame_ended();
}

/// Read the recorded frames (oldest first).
pub fn with_frames<R>(f: impl FnOnce(&VecDeque<FrameData>) -> R) -> R {
    f(&FRAMES.lock().unwrap_or_else(|e| e.into_inner()).frames)
}

/// Forget every recorded frame (the Profiler window's Clear button).
pub fn clear() {
    let mut h = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    h.frames.clear();
    h.late.clear();
}

/// Recording, the frame counter and the history are process-wide: tests that switch
/// recording on or off, end frames or read the history take turns through this (the profiler's
/// own tests and the GPU timing tests, which run in the same test process).
#[cfg(test)]
pub(crate) fn test_serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;
