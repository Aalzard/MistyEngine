//! Lightweight CPU profiler: scoped timings per frame, kept for the last [`HISTORY`] frames.
//!
//! `profile_scope!("Physics", Category::Physics)` times the rest of the enclosing block while
//! recording is on. When it is off a scope costs one relaxed atomic load and its name
//! expression is never evaluated. Samples are kept per thread and collected by [`end_frame`]
//! on the main loop's thread; scopes on other threads are capped and dropped.

pub mod capture;
mod frame;

pub use frame::{FrameData, HierarchyNode, HierarchyTree, Sample};

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;

use crate::render::{RenderMemory, RenderStats};

/// Frames kept for the Profiler window.
pub const HISTORY: usize = 300;
/// Samples one thread may record per frame before new scopes are ignored.
const MAX_SAMPLES: usize = 200_000;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Index of the frame being recorded.
static FRAME: AtomicU64 = AtomicU64::new(0);
static FRAMES: Mutex<History> = Mutex::new(History {
    frames: VecDeque::new(),
    gpu_pending: Vec::new(),
});

/// Profiler module a scope's time is charted under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
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
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct MemoryStats {
    pub entities: usize,
    pub render: RenderMemory,
}

/// Is recording on?
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turn recording on or off (the Profiler window's Record button).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Index of the frame currently being recorded.
pub fn frame_index() -> u64 {
    FRAME.load(Ordering::Relaxed)
}

/// Samples of the frame being recorded on this thread.
#[derive(Default)]
struct Recorder {
    samples: Vec<Sample>,
    /// Categories of the open scopes.
    stack: Vec<Category>,
    frame_start: Option<Instant>,
    render: RenderStats,
    memory: MemoryStats,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder::default());
}

/// An open scope; records its duration when dropped.
pub struct Scope {
    index: usize,
    start: Instant,
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
            let start = Instant::now();
            let frame_start = *r.frame_start.get_or_insert(start);
            let category = category
                .or(r.stack.last().copied())
                .unwrap_or(Category::Others);
            let index = r.samples.len();
            r.samples.push(Sample {
                name,
                category,
                depth: r.stack.len() as u16,
                start_ms: ms(start - frame_start),
                duration_ms: 0.0,
            });
            r.stack.push(category);
            Some(Self { index, start })
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let duration = ms(self.start.elapsed());
        RECORDER.with_borrow_mut(|r| {
            if let Some(s) = r.samples.get_mut(self.index) {
                s.duration_ms = duration;
            }
            r.stack.pop();
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

fn ms(d: std::time::Duration) -> f32 {
    d.as_secs_f32() * 1000.0
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
    let mut h = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    match h.frames.iter_mut().rev().find(|f| f.index == index) {
        Some(f) => *f.gpu_ms.get_or_insert(0.0) += ms,
        None if index >= frame_index() => h.gpu_pending.push((index, ms)),
        None => {}
    }
}

struct History {
    frames: VecDeque<FrameData>,
    /// GPU times for frames that haven't ended yet.
    gpu_pending: Vec<(u64, f32)>,
}

/// Close the frame recorded on this thread: its samples, counters and wall-clock time since
/// the previous call go into the history (while recording).
pub fn end_frame() {
    thread_local! {
        static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }
    let now = Instant::now();
    let cpu_ms = LAST.with(|l| l.replace(Some(now))).map(|t| ms(now - t));
    let index = FRAME.fetch_add(1, Ordering::Relaxed);
    let recorded = RECORDER.with_borrow_mut(std::mem::take);
    if !enabled() {
        return;
    }
    let mut frame = FrameData::new(index, recorded.samples, cpu_ms.unwrap_or(0.0));
    frame.render = recorded.render;
    frame.memory = recorded.memory;
    let mut h = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    h.gpu_pending.retain(|(i, ms)| {
        if *i == index {
            *frame.gpu_ms.get_or_insert(0.0) += ms;
        }
        *i > index
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
    h.gpu_pending.clear();
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
