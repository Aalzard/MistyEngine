//! GPU time per part of a render (profiling only, off by default): timestamps written between
//! the passes of a frame (and inside the main pass when the device can), read back with a
//! wait at the end of the render. Turned on with [`super::Renderer::set_pass_timing`]; needs
//! `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS` (see
//! [`super::timestamp_features`]).

use std::sync::Mutex;

/// Most marks per render.
const CAPACITY: u32 = 64;

pub struct PassTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Nanoseconds per tick.
    period: f32,
    /// Timestamps may be written inside render passes too.
    inside_passes: bool,
    /// Labels of this render's marks so far (the first one is the start).
    labels: Mutex<Vec<&'static str>>,
    /// GPU time of each part of the last render: the time from the previous mark to the
    /// labelled one.
    last: Vec<(&'static str, f32)>,
}

impl PassTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let needed =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !device.features().contains(needed) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("misty pass timer"),
            ty: wgpu::QueryType::Timestamp,
            count: CAPACITY,
        });
        let size = 8 * CAPACITY as u64;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty pass timer resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty pass timer readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let inside_passes = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        Some(Self {
            queries,
            resolve,
            readback,
            period: queue.get_timestamp_period(),
            inside_passes,
            labels: Mutex::new(Vec::new()),
            last: Vec::new(),
        })
    }

    /// The next free query, labelled (`None` = full).
    fn next(&self, label: &'static str) -> Option<u32> {
        let mut labels = self.labels.lock().unwrap_or_else(|e| e.into_inner());
        let i = labels.len() as u32;
        (i < CAPACITY).then(|| {
            labels.push(label);
            i
        })
    }

    /// Start a render's timing (between passes).
    pub fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        self.labels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.mark(encoder, "start");
    }

    /// End of the part named `label` (between passes).
    pub fn mark(&self, encoder: &mut wgpu::CommandEncoder, label: &'static str) {
        if let Some(i) = self.next(label) {
            encoder.write_timestamp(&self.queries, i);
        }
    }

    /// End of the part named `label` inside a render pass (skipped when the device can't).
    pub fn mark_pass(&self, pass: &mut wgpu::RenderPass, label: &'static str) {
        if self.inside_passes
            && let Some(i) = self.next(label)
        {
            pass.write_timestamp(&self.queries, i);
        }
    }

    /// Copy this render's timestamps out (after the last pass, before submitting).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let n = self.labels.lock().unwrap_or_else(|e| e.into_inner()).len() as u32;
        if n < 2 {
            return;
        }
        encoder.resolve_query_set(&self.queries, 0..n, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 8 * n as u64);
    }

    /// Wait for the submitted render and read its parts back.
    pub fn read(&mut self, device: &wgpu::Device) {
        let labels = std::mem::take(&mut *self.labels.lock().unwrap_or_else(|e| e.into_inner()));
        self.last.clear();
        if labels.len() < 2 {
            return;
        }
        let slice = self.readback.slice(..8 * labels.len() as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if rx.recv().unwrap_or(false) {
            if let Ok(data) = slice.get_mapped_range() {
                let ticks: Vec<u64> = data
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| u64::from_le_bytes(*c))
                    .collect();
                for i in 1..labels.len() {
                    let dt = ticks[i].saturating_sub(ticks[i - 1]);
                    self.last.push((labels[i], dt as f32 * self.period / 1.0e6));
                }
            }
            self.readback.unmap();
        }
    }

    /// GPU time per part of the last render.
    pub fn last(&self) -> &[(&'static str, f32)] {
        &self.last
    }
}
