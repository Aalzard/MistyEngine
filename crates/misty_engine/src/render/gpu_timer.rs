//! GPU frame time from timestamp queries: the first pass of a render writes a start time and
//! the last pass an end time; the pair is read back a frame or two later without stalling.
//! Only built when the device has `TIMESTAMP_QUERY`; otherwise GPU times are simply missing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Renders that may be in flight at once (more are not timed).
const SLOTS: usize = 8;
const FREE: u8 = 0;
const WAITING: u8 = 1;
const READY: u8 = 2;

struct Readback {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Profiler frame the render belonged to.
    frame: u64,
}

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readbacks: Vec<Readback>,
    /// Nanoseconds per timestamp tick.
    period: f32,
    last_ms: Option<f32>,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("misty gpu timer"),
            ty: wgpu::QueryType::Timestamp,
            count: 2 * SLOTS as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty gpu timer resolve"),
            size: wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT * SLOTS as u64,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readbacks = (0..SLOTS)
            .map(|_| Readback {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("misty gpu timer readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(FREE)),
                frame: 0,
            })
            .collect();
        Some(Self {
            queries,
            resolve,
            readbacks,
            period: queue.get_timestamp_period(),
            last_ms: None,
        })
    }

    /// A free query pair for the next render (`None` = all busy, the render isn't timed).
    pub fn begin(&mut self) -> Option<usize> {
        let slot = self
            .readbacks
            .iter()
            .position(|r| r.state.load(Ordering::Acquire) == FREE)?;
        self.readbacks[slot].state.store(WAITING, Ordering::Release);
        self.readbacks[slot].frame = crate::profiler::frame_index();
        Some(slot)
    }

    /// Timestamp writes for a pass: start of the render and/or its end.
    pub fn writes(
        &self,
        slot: usize,
        start: bool,
        end: bool,
    ) -> wgpu::RenderPassTimestampWrites<'_> {
        let base = 2 * slot as u32;
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: start.then_some(base),
            end_of_pass_write_index: end.then_some(base + 1),
        }
    }

    /// Copy the pair into its readback buffer (after the last pass, before submitting).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder, slot: usize) {
        let offset = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT * slot as u64;
        let base = 2 * slot as u32;
        encoder.resolve_query_set(&self.queries, base..base + 2, &self.resolve, offset);
        encoder.copy_buffer_to_buffer(&self.resolve, offset, &self.readbacks[slot].buffer, 0, 16);
    }

    /// Ask for the readback once the commands are submitted.
    pub fn submitted(&self, slot: usize) {
        let state = self.readbacks[slot].state.clone();
        self.readbacks[slot]
            .buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
            });
    }

    /// Collect finished measurements (non-blocking) and report them to the profiler.
    pub fn poll(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        for r in &self.readbacks {
            if r.state.load(Ordering::Acquire) != READY {
                continue;
            }
            let ticks = match r.buffer.slice(..).get_mapped_range() {
                Ok(data) => {
                    let word = |i: usize| {
                        u64::from_le_bytes(data[i..i + 8].try_into().unwrap_or_default())
                    };
                    word(8).saturating_sub(word(0))
                }
                Err(_) => u64::MAX,
            };
            r.buffer.unmap();
            r.state.store(FREE, Ordering::Release);
            let ms = ticks as f32 * self.period / 1.0e6;
            // Ignore nonsense from drivers that reset counters between passes.
            if ms < 1000.0 {
                self.last_ms = Some(ms);
                crate::profiler::add_gpu_time(r.frame, ms);
            }
        }
    }

    /// The most recent measurement.
    pub fn last_ms(&self) -> Option<f32> {
        self.last_ms
    }
}
