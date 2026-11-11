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
//! page starts, up to [`MAX_PAGES`]. That is all the room there is: a chunk no page can take
//! is refused (terrain_grass.rs scatters it coarser then), so the grass never holds more
//! than the pages. A run that needs fewer blades than it holds is trimmed ([`BladePool::trim`]),
//! and a last page left empty is dropped again.

use std::collections::BTreeMap;

use super::grass_cull::{best_run, copy_run, release_run, take_run};
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
    /// Bumped whenever a page buffer is replaced, added or dropped (bind groups are rebuilt).
    pub generation: u64,
    /// Blades pass through here when a run moves within its page ([`BladePool::trim`]).
    scratch: Option<wgpu::Buffer>,
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
        Self {
            pages: Vec::new(),
            max_page: Self::page_limit(device),
            generation: 0,
            scratch: None,
        }
    }

    /// Largest page this device can bind (blades).
    fn page_limit(device: &wgpu::Device) -> u32 {
        let limits = device.limits();
        let bytes = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        ((bytes / BLADE_BYTES).min(PAGE_LIMIT as u64) as u32) / GRAIN * GRAIN
    }

    /// Most blades the pages of this device can hold.
    pub fn limit(device: &wgpu::Device) -> u64 {
        Self::page_limit(device) as u64 * MAX_PAGES as u64
    }

    pub fn bytes(&self) -> u64 {
        self.pages.iter().map(|p| p.buffer.size()).sum::<u64>()
            + self.scratch.as_ref().map_or(0, wgpu::Buffer::size)
    }

    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// Blades the pages hold room for, how many of them are free and the longest free run.
    pub fn room(&self) -> (u64, u64, u32) {
        let capacity = self.pages.iter().map(|p| p.capacity as u64).sum();
        let free = (self.pages.iter())
            .flat_map(|p| p.free.values())
            .map(|n| *n as u64)
            .sum();
        let longest = (self.pages.iter())
            .flat_map(|p| p.free.values())
            .copied()
            .max()
            .unwrap_or(0);
        (capacity, free, longest)
    }

    pub fn buffer(&self, page: u32) -> &wgpu::Buffer {
        &self.pages[page as usize].buffer
    }

    /// A free run of `n` blades (a multiple of the grain) in the pages as they are: the run
    /// they fit best in over every page (the lowest page among equals).
    fn take_free(&mut self, n: u32) -> Option<Slot> {
        let (_, page) = (self.pages.iter().enumerate())
            .filter_map(|(i, p)| best_run(&p.free, n).map(|(_, len)| (len, i)))
            .min()?;
        let at = take_run(&mut self.pages[page].free, n)?;
        Some(Slot {
            page: page as u32,
            at,
        })
    }

    /// Room for `n` blades; None when no page can take them (the caller scatters the chunk
    /// coarser).
    pub fn alloc(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, n: u32) -> Option<Slot> {
        let n = n.max(1).next_multiple_of(GRAIN);
        if n > self.max_page {
            return None;
        }
        if let Some(slot) = self.take_free(n) {
            return Some(slot);
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

    /// Keep only the first `keep` blades of the run of `n` at `slot`: they move into a free
    /// run that fits them when there is one (the whole old run is given back, so the room a
    /// full chunk needs comes free in one piece), else the run's tail is given back. Where
    /// they are now. The copies are recorded into `encoder`.
    pub fn trim(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        slot: Slot,
        n: u32,
        keep: u32,
    ) -> Slot {
        let old = n.max(1).next_multiple_of(GRAIN);
        let new = keep.max(1).next_multiple_of(GRAIN);
        if new >= old || slot.page as usize >= self.pages.len() {
            return slot;
        }
        let Some(to) = self.take_free(new) else {
            release_run(
                &mut self.pages[slot.page as usize].free,
                slot.at + new,
                old - new,
            );
            return slot;
        };
        let Self { pages, scratch, .. } = self;
        copy_run(
            device,
            encoder,
            scratch,
            (
                &pages[slot.page as usize].buffer,
                slot.at as u64 * BLADE_BYTES,
            ),
            (&pages[to.page as usize].buffer, to.at as u64 * BLADE_BYTES),
            keep as u64 * BLADE_BYTES,
        );
        release_run(&mut pages[slot.page as usize].free, slot.at, old);
        to
    }

    /// The last page holds nothing and is not the only one (it can be dropped).
    pub fn spare_page(&self) -> bool {
        self.pages.len() > 1
            && (self.pages.last()).is_some_and(|p| p.free.get(&0) == Some(&p.capacity))
    }

    /// Drop the last page when it is spare; whether it went.
    pub fn drop_spare_page(&mut self) -> bool {
        if !self.spare_page() {
            return false;
        }
        self.pages.pop();
        self.generation += 1;
        true
    }

    /// Pages of at most `blades` from now on (tests: small pages fill up quickly).
    #[cfg(test)]
    pub fn set_page_limit(&mut self, blades: u32) {
        self.max_page = (blades / GRAIN * GRAIN).max(GRAIN);
    }

    /// Most blades the pages can hold (tests).
    #[cfg(test)]
    pub fn room_limit(&self) -> u64 {
        self.max_page as u64 * MAX_PAGES as u64
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

    #[test]
    fn small_runs_fill_small_holes() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let big = pool.alloc(&device, &queue, 64_000).expect("room");
        let _between = pool.alloc(&device, &queue, 64).expect("room");
        let small = pool.alloc(&device, &queue, 640).expect("room");
        let keep = pool.alloc(&device, &queue, 64).expect("room");
        pool.free(big, 64_000);
        pool.free(small, 640);
        // The hole it fits best in, not the first one: the big hole stays whole.
        assert_eq!(pool.alloc(&device, &queue, 600), Some(small));
        assert_eq!(pool.alloc(&device, &queue, 64_000), Some(big));
        assert_eq!((keep.page, keep.at), (0, 64_000 + 64 + 640));
    }

    #[test]
    fn a_trimmed_run_moves_out_of_the_way() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        let hole = pool.alloc(&device, &queue, 4096).expect("room");
        let big = pool.alloc(&device, &queue, 64_000).expect("room");
        let after = pool.alloc(&device, &queue, 64).expect("room");
        pool.free(hole, 4096);
        let (capacity, free, _) = pool.room();
        // Trimmed to a sixteenth: the blades kept move into the small hole and the whole run
        // comes free, in one piece (with what the hole had left over).
        let moved = pool.trim(&device, &mut encoder, big, 64_000, 4000);
        assert_eq!(moved, hole);
        let rest = capacity - (4096 + 64_000 + 64);
        assert_eq!(pool.room(), (capacity, free + 64_000 - 4032, rest as u32));
        let again = pool.alloc(&device, &queue, 64_000).expect("room");
        assert_eq!((again.page, again.at), (0, 4032));
        // No hole takes them: the run keeps its place and gives back its tail.
        let mut full = BladePool::new(&device);
        let all = full.alloc(&device, &queue, FIRST_PAGE).expect("room");
        assert_eq!(full.trim(&device, &mut encoder, all, FIRST_PAGE, 1000), all);
        assert_eq!(
            full.room(),
            (
                FIRST_PAGE as u64,
                FIRST_PAGE as u64 - 1024,
                FIRST_PAGE - 1024
            )
        );
        // Nothing to give back: untouched.
        assert_eq!(pool.trim(&device, &mut encoder, after, 64, 64), after);
        assert_eq!(pool.trim(&device, &mut encoder, after, 64, 1), after);
        queue.submit([encoder.finish()]);
    }

    #[test]
    fn full_pages_refuse_and_an_empty_last_page_goes() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        pool.set_page_limit(1024);
        assert_eq!(pool.room_limit(), 4096);
        let runs: Vec<Slot> = (0..MAX_PAGES)
            .map(|_| pool.alloc(&device, &queue, 1024).expect("room"))
            .collect();
        assert_eq!(pool.pages(), MAX_PAGES);
        // Every page full: no room, and no buffer outside the pages.
        assert_eq!(pool.alloc(&device, &queue, 64), None);
        assert_eq!(pool.bytes(), 4096 * BLADE_BYTES);
        // The last page empties: it can go; the ones before it stay (their runs keep their
        // places), and so does the only page left.
        assert!(!pool.spare_page());
        pool.free(runs[3], 1024);
        let generation = pool.generation;
        assert!(pool.spare_page() && pool.drop_spare_page());
        assert_eq!(pool.pages(), 3);
        assert!(pool.generation > generation);
        pool.free(runs[0], 1024);
        assert!(!pool.drop_spare_page());
        for r in &runs[1..3] {
            pool.free(*r, 1024);
        }
        while pool.drop_spare_page() {}
        assert_eq!(pool.pages(), 1);
        // And comes back when it is needed.
        for _ in 0..2 {
            pool.alloc(&device, &queue, 1024).expect("room");
        }
        assert_eq!(pool.pages(), 2);
    }
}
