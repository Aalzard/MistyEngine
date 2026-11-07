//! The blades of every grass chunk of a terrain, in a few big storage buffers ("pages")
//! instead of one small buffer per chunk. A chunk is a run of blades in one page (first-fit
//! free list per page, like the cull records' arena), so:
//!
//! - a pass binds a page once for all the chunks in it (the culled lists hold page-local
//!   blade indices, grass_cull.wgsl `cs_write`),
//! - the grass visibility buffer's resolve can read any visible blade from its id (page and
//!   index, see grass_vis.rs), which per-chunk buffers can't offer without bindless,
//! - uploading a chunk is a write into memory that already exists (no buffer creation on the
//!   render thread), split into pieces across frames (terrain_grass.rs `upload`).
//!
//! Pages grow by copying into a buffer twice as big up to the binding limit; past that a new
//! page starts, up to [`MAX_PAGES`].

use std::collections::BTreeMap;

use super::grass_cull::{release_run, take_run};
use crate::terrain::details::Blade;

/// Bytes of one blade (terrain/details.rs `Blade`, grass_blade.wgsl `grass_fetch`).
pub(super) const BLADE_BYTES: u64 = std::mem::size_of::<Blade>() as u64;
/// Most blades in a page: the visibility id keeps 21 bits for the index (grass_vis.wgsl),
/// and a page stays under the default 128 MiB storage binding size.
const PAGE_LIMIT: u32 = 1 << 21;
/// Pages a terrain may use (the visibility resolve binds them all).
pub(super) const MAX_PAGES: usize = 4;
/// Allocations are rounded up to this many blades (keeps offsets 256-byte aligned for the
/// per-chunk bindings of the CPU path: 64 × 72 = 18 × 256).
const GRAIN: u32 = 64;
/// Blades of a new page.
const FIRST_PAGE: u32 = 1 << 18;

struct Page {
    buffer: wgpu::Buffer,
    /// In blades.
    capacity: u32,
    /// Free runs: start → length (blades).
    free: BTreeMap<u32, u32>,
}

/// Where a chunk's blades are: page and first blade in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub page: u32,
    pub at: u32,
}

impl Slot {
    /// The value the culled lists carry for this chunk's first blade (page in the top bits,
    /// grass_blade.wgsl `grass_fetch` masks it off).
    pub fn list_base(self) -> u32 {
        (self.page << 28) | self.at
    }
}

pub(super) struct BladePool {
    pages: Vec<Page>,
    /// Largest page this device can bind (blades).
    max_page: u32,
    /// Bumped whenever a page buffer is replaced or added (bind groups are rebuilt).
    pub generation: u64,
}

fn page_buffer(device: &wgpu::Device, blades: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty grass blade page"),
        size: blades as u64 * BLADE_BYTES,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

impl BladePool {
    pub fn new(device: &wgpu::Device) -> Self {
        let limits = device.limits();
        let bytes = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let max_page = ((bytes / BLADE_BYTES).min(PAGE_LIMIT as u64) as u32) / GRAIN * GRAIN;
        Self {
            pages: Vec::new(),
            max_page,
            generation: 0,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.pages.iter().map(|p| p.buffer.size()).sum()
    }

    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    pub fn buffer(&self, page: u32) -> &wgpu::Buffer {
        &self.pages[page as usize].buffer
    }

    /// Room for `n` blades; None when no page can take them (the chunk keeps a buffer of its
    /// own then).
    pub fn alloc(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, n: u32) -> Option<Slot> {
        let n = n.max(1).next_multiple_of(GRAIN);
        if n > self.max_page {
            return None;
        }
        for (i, p) in self.pages.iter_mut().enumerate() {
            if let Some(at) = take_run(&mut p.free, n) {
                return Some(Slot { page: i as u32, at });
            }
        }
        // Grow the last page (copying what is there), else start a new one.
        if let Some(last) = self.pages.last_mut()
            && last.capacity < self.max_page
        {
            let want = (last.capacity as u64 * 2).max(last.capacity as u64 + n as u64);
            let capacity = (want.min(self.max_page as u64) as u32) / GRAIN * GRAIN;
            if capacity >= last.capacity + n || capacity == self.max_page {
                let buffer = page_buffer(device, capacity);
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("misty grass blade page grow"),
                });
                encoder.copy_buffer_to_buffer(&last.buffer, 0, &buffer, 0, last.buffer.size());
                queue.submit([encoder.finish()]);
                let old = last.capacity;
                last.buffer = buffer;
                last.capacity = capacity;
                release_run(&mut last.free, old, capacity - old);
                self.generation += 1;
                let page = self.pages.len() as u32 - 1;
                if let Some(at) = take_run(&mut self.pages[page as usize].free, n) {
                    return Some(Slot { page, at });
                }
            }
        }
        if self.pages.len() >= MAX_PAGES {
            return None;
        }
        let capacity = FIRST_PAGE.max(n).min(self.max_page);
        self.pages.push(Page {
            buffer: page_buffer(device, capacity),
            capacity,
            free: BTreeMap::from([(0, capacity)]),
        });
        self.generation += 1;
        let page = self.pages.len() as u32 - 1;
        let at = take_run(&mut self.pages[page as usize].free, n)?;
        Some(Slot { page, at })
    }

    /// Write `blades` at blade `offset` of the run at `slot`.
    pub fn write(&self, queue: &wgpu::Queue, slot: Slot, offset: u32, blades: &[Blade]) {
        let at = (slot.at + offset) as u64 * BLADE_BYTES;
        queue.write_buffer(self.buffer(slot.page), at, bytemuck::cast_slice(blades));
    }

    /// Give back the `n` blades at `slot` (as allocated).
    pub fn free(&mut self, slot: Slot, n: u32) {
        if let Some(p) = self.pages.get_mut(slot.page as usize) {
            release_run(&mut p.free, slot.at, n.max(1).next_multiple_of(GRAIN));
        }
    }

    /// A chunk's run as a binding (the CPU path binds each chunk's blades on their own).
    pub fn binding(&self, slot: Slot, n: u32) -> wgpu::BufferBinding<'_> {
        wgpu::BufferBinding {
            buffer: self.buffer(slot.page),
            offset: slot.at as u64 * BLADE_BYTES,
            size: wgpu::BufferSize::new(n.max(1) as u64 * BLADE_BYTES),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_base_keeps_page_and_index_apart() {
        let s = Slot {
            page: 3,
            at: PAGE_LIMIT - GRAIN,
        };
        let v = s.list_base();
        assert_eq!(v >> 28, 3);
        assert_eq!(v & 0x0fff_ffff, PAGE_LIMIT - GRAIN);
        // Offsets of runs are 256-byte aligned (the CPU path binds them).
        assert_eq!((GRAIN as u64 * BLADE_BYTES) % 256, 0);
    }

    #[test]
    fn pool_reuses_runs_grows_and_opens_pages() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let a = pool.alloc(&device, &queue, 100).expect("room");
        let b = pool.alloc(&device, &queue, 1000).expect("room");
        assert_eq!((a.page, a.at), (0, 0));
        // Runs are rounded up to the grain.
        assert_eq!((b.page, b.at), (0, 128));
        pool.free(a, 100);
        // The hole is reused first.
        assert_eq!(
            pool.alloc(&device, &queue, 64),
            Some(Slot { page: 0, at: 0 })
        );
        // More than the first page holds: the page grows (copying), its runs stay.
        let generation = pool.generation;
        let big = pool.alloc(&device, &queue, FIRST_PAGE).expect("room");
        assert_eq!(big.page, 0);
        assert!(pool.generation > generation);
        // A run bigger than any page can't be held.
        assert_eq!(pool.alloc(&device, &queue, pool.max_page + 1), None);
        // Fill the first page up: the next run opens a second page.
        let rest = pool.max_page - (big.at + FIRST_PAGE);
        if rest >= GRAIN {
            pool.alloc(&device, &queue, rest).expect("room");
        }
        let next = pool.alloc(&device, &queue, 1000).expect("room");
        assert_eq!(next.page, 1);
        assert_eq!(pool.pages(), 2);
    }
}
