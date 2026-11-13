//! GPU to CPU frame copies through a ring of staging buffers. Each captured frame is copied
//! into the next free buffer and mapped asynchronously; the pixels are picked up a frame or two
//! later when the GPU is done, so recording doesn't wait on the GPU every frame. Only when every
//! buffer is still in flight does a capture wait for the oldest one (frames are never dropped).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use misty_engine::render::RenderTarget;
use misty_engine::wgpu;

use crate::Frame;

/// Staging buffers in the ring.
pub const SLOTS: usize = 4;

struct Slot {
    buffer: wgpu::Buffer,
    ready: Arc<AtomicBool>,
    busy: bool,
}

pub struct Readback {
    width: u32,
    height: u32,
    /// Bytes a row in the buffers (padded to wgpu's copy alignment).
    row: u32,
    /// The target's image is copied here first (same size and format as the ring).
    copy: wgpu::Texture,
    slots: Vec<Slot>,
    /// Slots in flight, oldest first, with their frame numbers.
    queue: VecDeque<(usize, u64)>,
}

impl Readback {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let copy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("recorder copy"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: misty_engine::render::COLOR_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let slots = (0..SLOTS)
            .map(|i| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("recorder staging {i}")),
                    size: (row * height) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                ready: Arc::new(AtomicBool::new(false)),
                busy: false,
            })
            .collect();
        Self {
            width,
            height,
            row,
            copy,
            slots,
            queue: VecDeque::new(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Whether `target` can be captured directly (same size as the ring).
    pub fn fits(&self, target: &RenderTarget) -> bool {
        target.output_size() == (self.width, self.height)
    }

    /// Frames copied but not yet picked up.
    pub fn in_flight(&self) -> usize {
        self.queue.len()
    }

    /// Copy `target`'s last image as frame `index`. Finished frames (older ones) land in `out`.
    /// `target` must [`fit`](Self::fits).
    pub fn capture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        index: u64,
        out: &mut Vec<Frame>,
    ) {
        // Make room: pick up whatever is done, and wait for the oldest if the ring is full.
        self.collect(device, false, out);
        let slot = loop {
            if let Some(i) = self.slots.iter().position(|s| !s.busy) {
                break i;
            }
            self.collect_oldest(device, out);
        };
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        let s = &mut self.slots[slot];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("recorder readback"),
        });
        target.copy_output(&mut encoder, &self.copy);
        encoder.copy_texture_to_buffer(
            self.copy.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &s.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.row),
                    rows_per_image: Some(self.height),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        s.busy = true;
        s.ready.store(false, Ordering::Release);
        let ready = s.ready.clone();
        s.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            // A failed map still frees the slot (its frame comes out black).
            let _ = r;
            ready.store(true, Ordering::Release);
        });
        self.queue.push_back((slot, index));
    }

    /// Pick up finished frames, oldest first. `wait`: block until every frame in flight is done
    /// (end of a recording).
    pub fn collect(&mut self, device: &wgpu::Device, wait: bool, out: &mut Vec<Frame>) {
        if self.queue.is_empty() {
            return;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        while let Some(&(slot, _)) = self.queue.front() {
            if self.slots[slot].ready.load(Ordering::Acquire) {
                out.push(self.take_front());
            } else if wait {
                self.collect_oldest(device, out);
            } else {
                break;
            }
        }
    }

    /// Wait for the oldest frame in flight and pick it up.
    fn collect_oldest(&mut self, device: &wgpu::Device, out: &mut Vec<Frame>) {
        let Some(&(slot, _)) = self.queue.front() else {
            return;
        };
        while !self.slots[slot].ready.load(Ordering::Acquire) {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
        out.push(self.take_front());
    }

    fn take_front(&mut self) -> Frame {
        let (slot, index) = self.queue.pop_front().expect("a frame in flight");
        let s = &mut self.slots[slot];
        let (w, h, row) = (self.width as usize, self.height as usize, self.row as usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        {
            let slice = s.buffer.slice(..);
            match slice.get_mapped_range() {
                Ok(data) => {
                    for y in 0..h {
                        rgba.extend_from_slice(&data[y * row..y * row + w * 4]);
                    }
                }
                Err(_) => rgba.resize(w * h * 4, 0),
            }
        }
        s.buffer.unmap();
        s.busy = false;
        opaque(&mut rgba);
        Frame {
            index,
            width: self.width,
            height: self.height,
            rgba,
        }
    }
}

/// The views' alpha is whatever the last pass left; recordings are opaque.
pub fn opaque(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        p[3] = 255;
    }
}

/// `rgba` (`w` x `h`) scaled to `to_w` x `to_h` (an Editor Scene view resized mid-recording).
pub fn fit(rgba: Vec<u8>, w: u32, h: u32, to_w: u32, to_h: u32) -> Vec<u8> {
    if (w, h) == (to_w, to_h) {
        return rgba;
    }
    match image::RgbaImage::from_raw(w, h, rgba) {
        Some(img) => {
            image::imageops::resize(&img, to_w, to_h, image::imageops::FilterType::Triangle)
                .into_raw()
        }
        None => vec![0; (to_w * to_h * 4) as usize],
    }
}
