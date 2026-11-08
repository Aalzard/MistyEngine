//! GPU time per part of a render: timestamps written between the passes of a frame (and inside
//! the main pass when the device can). Needs `TIMESTAMP_QUERY` and
//! `TIMESTAMP_QUERY_INSIDE_ENCODERS` (see [`super::timestamp_features`]).
//!
//! Two modes:
//! - **Blocking** ([`PassTimer::new`], [`super::Renderer::set_pass_timing`]): one set of
//!   queries, read back with a wait at the end of each render (benchmarks and tests).
//! - **Profiler** ([`PassTimer::new_async`]): a ring of query sets, each read back without
//!   waiting a frame or two later and reported to the Profiler's GPU module
//!   ([`crate::profiler::add_gpu_passes`]); renders that find every set busy aren't timed.
//!   The renderer runs it while the Profiler records.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::profiler::GpuPass;

/// Most marks per render.
const CAPACITY: u32 = 64;
/// Renders that may be in flight at once in profiler mode.
const ASYNC_SLOTS: usize = 8;
const FREE: u8 = 0;
const RECORDING: u8 = 1;
const WAITING: u8 = 2;
const READY: u8 = 3;

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Labels of this render's marks so far (the first one is the start).
    labels: Mutex<Vec<&'static str>>,
    /// Profiler frame and view of the render (profiler mode).
    frame: u64,
    view: Cow<'static, str>,
}

impl Slot {
    fn new(device: &wgpu::Device) -> Self {
        let size = 8 * CAPACITY as u64;
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("misty pass timer"),
                ty: wgpu::QueryType::Timestamp,
                count: CAPACITY,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty pass timer resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty pass timer readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            labels: Mutex::new(Vec::new()),
            frame: 0,
            view: Cow::Borrowed(""),
        }
    }

    fn labels(&self) -> std::sync::MutexGuard<'_, Vec<&'static str>> {
        self.labels.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The parts of a read-back render: (label, start after the first mark, ms).
    fn parts(&self, labels: &[&'static str], period: f32) -> Vec<(&'static str, f32, f32)> {
        let mut out = Vec::new();
        let slice = self.readback.slice(..8 * labels.len() as u64);
        if let Ok(data) = slice.get_mapped_range() {
            let ticks: Vec<u64> = data
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| u64::from_le_bytes(*c))
                .collect();
            let ms = |a: u64, b: u64| b.saturating_sub(a) as f32 * period / 1.0e6;
            for i in 1..labels.len() {
                out.push((
                    labels[i],
                    ms(ticks[0], ticks[i - 1]),
                    ms(ticks[i - 1], ticks[i]),
                ));
            }
        }
        out
    }
}

pub struct PassTimer {
    slots: Vec<Slot>,
    /// The slot this render writes (`None` = this render isn't timed).
    current: Mutex<Option<usize>>,
    /// Profiler mode: read back later without waiting.
    profiler: bool,
    /// Nanoseconds per tick.
    period: f32,
    /// Timestamps may be written inside render passes too.
    inside_passes: bool,
    /// GPU time of each part of the last render read back: the time from the previous mark to
    /// the labelled one.
    last: Vec<(&'static str, f32)>,
}

/// The device can time each part of a render.
pub fn supported(device: &wgpu::Device) -> bool {
    let needed = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    device.features().contains(needed)
}

impl PassTimer {
    /// Blocking mode: each render's parts are read back by [`PassTimer::read`].
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        Self::with_slots(device, queue, 1, false)
    }

    /// Profiler mode: parts are read back by [`PassTimer::poll`] a frame or two later and sent
    /// to the Profiler.
    pub fn new_async(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        Self::with_slots(device, queue, ASYNC_SLOTS, true)
    }

    fn with_slots(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slots: usize,
        profiler: bool,
    ) -> Option<Self> {
        if !supported(device) {
            return None;
        }
        Some(Self {
            slots: (0..slots).map(|_| Slot::new(device)).collect(),
            current: Mutex::new(None),
            profiler,
            period: queue.get_timestamp_period(),
            inside_passes: device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            last: Vec::new(),
        })
    }

    /// Profiler mode (reads back later, reports to the Profiler).
    pub fn is_profiler(&self) -> bool {
        self.profiler
    }

    fn current(&self) -> Option<usize> {
        *self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The next free query of this render, labelled (`None` = full or not timed).
    fn next(&self, label: &'static str) -> Option<(&wgpu::QuerySet, u32)> {
        let slot = &self.slots[self.current()?];
        let mut labels = slot.labels();
        let i = labels.len() as u32;
        (i < CAPACITY).then(|| {
            labels.push(label);
            (&slot.queries, i)
        })
    }

    /// Start a render's timing (between passes).
    pub fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        // A render that ended without submitting left its slot recording.
        for s in &self.slots {
            let _ = s
                .state
                .compare_exchange(RECORDING, FREE, Ordering::AcqRel, Ordering::Relaxed);
        }
        let slot = self
            .slots
            .iter()
            .position(|s| s.state.load(Ordering::Acquire) == FREE);
        if let Some(i) = slot {
            self.slots[i].state.store(RECORDING, Ordering::Release);
            self.slots[i].labels().clear();
        }
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = slot;
        self.mark(encoder, "start");
    }

    /// End of the part named `label` (between passes).
    pub fn mark(&self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        if let Some((queries, i)) = self.next(label) {
            encoder.write_timestamp(queries, i);
        }
    }

    /// End of the part named `label` inside a render pass (skipped when the device can't).
    pub fn mark_pass(&self, pass: &mut wgpu::RenderPass, label: &'static str) {
        if self.inside_passes
            && let Some((queries, i)) = self.next(label)
        {
            pass.write_timestamp(queries, i);
        }
    }

    /// Copy this render's timestamps out (after the last pass, before submitting).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(slot) = self.current().map(|i| &self.slots[i]) else {
            return;
        };
        let n = slot.labels().len() as u32;
        if n < 2 {
            return;
        }
        encoder.resolve_query_set(&slot.queries, 0..n, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, 8 * n as u64);
    }

    /// Blocking mode: wait for the submitted render and read its parts back.
    pub fn read(&mut self, device: &wgpu::Device) {
        self.last.clear();
        let Some(i) = self
            .current
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let slot = &self.slots[i];
        let labels = std::mem::take(&mut *slot.labels());
        slot.state.store(FREE, Ordering::Release);
        if labels.len() < 2 {
            return;
        }
        let slice = slot.readback.slice(..8 * labels.len() as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if rx.recv().unwrap_or(false) {
            let parts = slot.parts(&labels, self.period);
            self.last = parts.into_iter().map(|(l, _, ms)| (l, ms)).collect();
            slot.readback.unmap();
        }
    }

    /// Profiler mode: the render was submitted; ask for its readback (no wait).
    pub fn submitted(&mut self, frame: u64, view: Cow<'static, str>) {
        let Some(i) = self
            .current
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let slot = &mut self.slots[i];
        let n = slot.labels().len() as u64;
        if n < 2 {
            slot.state.store(FREE, Ordering::Release);
            return;
        }
        slot.frame = frame;
        slot.view = view;
        slot.state.store(WAITING, Ordering::Release);
        let state = slot.state.clone();
        slot.readback
            .slice(..8 * n)
            .map_async(wgpu::MapMode::Read, move |r| {
                state.store(if r.is_ok() { READY } else { FREE }, Ordering::Release);
            });
    }

    /// Profiler mode: collect finished renders (no wait) and report them to the Profiler.
    pub fn poll(&mut self, device: &wgpu::Device) {
        let state = |s: &Slot| s.state.load(Ordering::Acquire);
        if self.slots.iter().any(|s| state(s) == WAITING) {
            let _ = device.poll(wgpu::PollType::Poll);
        }
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) != READY {
                continue;
            }
            let labels = std::mem::take(&mut *slot.labels());
            let parts = slot.parts(&labels, self.period);
            slot.readback.unmap();
            slot.state.store(FREE, Ordering::Release);
            // Drivers that reset counters between passes give nonsense: skip those renders.
            if parts.iter().any(|p| p.2 > 1000.0) {
                continue;
            }
            self.last = parts.iter().map(|&(l, _, ms)| (l, ms)).collect();
            let passes = parts
                .into_iter()
                .map(|(name, start_ms, ms)| GpuPass {
                    view: slot.view.clone(),
                    name: Cow::Borrowed(name),
                    start_ms,
                    ms,
                })
                .collect();
            crate::profiler::add_gpu_passes(slot.frame, passes);
        }
    }

    /// GPU time per part of the last render read back.
    pub fn last(&self) -> &[(&'static str, f32)] {
        &self.last
    }
}
