//! The blades of every grass chunk of a terrain, in a few big storage buffers ("pages")
//! instead of one small buffer per chunk. A chunk is a run of blades in one page (best-fit
//! free list per page), so:
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
//! than the pages.
//!
//! Short runs (far chunks: a few thousand blades each, held for hundreds of metres) and long
//! runs (the chunks around the camera: up to a few hundred thousand, replaced every few
//! metres walked) live in pages of their own kind, so a short run never lands in the middle
//! of the room the next long run needs. A run that needs fewer blades than it holds is
//! trimmed ([`BladePool::trim`]), and a page left empty gives its memory back
//! ([`BladePool::release_spare_page`]).

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
/// Runs shorter than this are short runs (they go to the pages for short runs): the chunks
/// a camera is not near, at the two or three coarsest density levels.
const SHORT_RUN: u32 = 8 * 1024;

struct Page {
    buffer: wgpu::Buffer,
    /// In blades (0: the page gave its memory back and holds a stand-in buffer).
    capacity: u32,
    /// Free runs: start → length (blades).
    free: BTreeMap<u32, u32>,
    /// A page for short runs (else for long ones).
    short: bool,
}

impl Page {
    /// The page has memory and nothing in it.
    fn spare(&self) -> bool {
        self.capacity > 0 && self.free.get(&0) == Some(&self.capacity)
    }
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
    /// Up to [`MAX_PAGES`]; a page keeps its number for good (runs name their page).
    pages: Vec<Page>,
    /// Largest page this device can bind (blades).
    max_page: u32,
    /// Bumped whenever a page buffer is replaced or added (bind groups are rebuilt).
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

    /// Pages there are buffers for (the passes and the visibility resolve bind each).
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// Pages holding memory (tests).
    #[cfg(test)]
    pub fn live_pages(&self) -> usize {
        self.pages.iter().filter(|p| p.capacity > 0).count()
    }

    /// Blades the pages hold room for, how many of them are free and the longest free run
    /// (tests).
    #[cfg(test)]
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

    /// Where the cull record of the first blade at `slot` goes: the records mirror the pages
    /// (every page has the records of a full page set aside, grass_cull.rs `Arena`).
    pub fn record_at(&self, slot: Slot) -> u32 {
        slot.page * self.max_page + slot.at
    }

    /// The end of the records of page `page` as it is now.
    pub fn records_end(&self, page: u32) -> u64 {
        let capacity = self.pages.get(page as usize).map_or(0, |p| p.capacity);
        page as u64 * self.max_page as u64 + capacity as u64
    }

    /// A free run of `n` blades (a multiple of the grain) in the pages as they are: the run
    /// they fit best in over the pages of one kind (`short`; None: of any kind), the lowest
    /// page among equals.
    fn take_free(&mut self, n: u32, short: Option<bool>) -> Option<Slot> {
        let (_, page) = (self.pages.iter().enumerate())
            .filter(|(_, p)| short.is_none_or(|s| p.short == s))
            .filter_map(|(i, p)| best_run(&p.free, n).map(|(_, len)| (len, i)))
            .min()?;
        let at = take_run(&mut self.pages[page].free, n)?;
        Some(Slot {
            page: page as u32,
            at,
        })
    }

    /// Grow a page of the kind `short` that is not at its limit yet (copying what is there)
    /// and take `n` blades from it.
    fn grow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        n: u32,
        short: bool,
    ) -> Option<Slot> {
        let max = self.max_page;
        let page = (self.pages.iter())
            .position(|p| p.short == short && p.capacity > 0 && p.capacity < max)?;
        let p = &mut self.pages[page];
        let want = (p.capacity as u64 * 2).max(p.capacity as u64 + n as u64);
        let capacity = (want.min(max as u64) as u32) / GRAIN * GRAIN;
        if capacity < p.capacity + n && capacity < max {
            return None;
        }
        let buffer = page_buffer(device, capacity);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty grass blade page grow"),
        });
        encoder.copy_buffer_to_buffer(&p.buffer, 0, &buffer, 0, p.buffer.size());
        queue.submit([encoder.finish()]);
        let old = p.capacity;
        p.buffer = buffer;
        p.capacity = capacity;
        release_run(&mut p.free, old, capacity - old);
        self.generation += 1;
        let at = take_run(&mut self.pages[page].free, n)?;
        Some(Slot {
            page: page as u32,
            at,
        })
    }

    /// Start a page for the kind `short` (in the place of one that gave its memory back,
    /// else a new one while there are fewer than [`MAX_PAGES`]) and take `n` blades from it.
    fn open(&mut self, device: &wgpu::Device, n: u32, short: bool) -> Option<Slot> {
        let capacity = FIRST_PAGE.max(n).min(self.max_page);
        let page = match self.pages.iter().position(|p| p.capacity == 0) {
            Some(page) => page,
            None if self.pages.len() < MAX_PAGES => self.pages.len(),
            None => return None,
        };
        let made = Page {
            buffer: page_buffer(device, capacity),
            capacity,
            free: BTreeMap::from([(0, capacity)]),
            short,
        };
        match self.pages.get_mut(page) {
            Some(p) => *p = made,
            None => self.pages.push(made),
        }
        self.generation += 1;
        let at = take_run(&mut self.pages[page].free, n)?;
        Some(Slot {
            page: page as u32,
            at,
        })
    }

    /// Room for `n` blades; None when no page can take them (the caller scatters the chunk
    /// coarser). In a page of the run's kind when one has or can make room, else wherever
    /// room is left.
    pub fn alloc(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, n: u32) -> Option<Slot> {
        let n = n.max(1).next_multiple_of(GRAIN);
        if n > self.max_page {
            return None;
        }
        let short = n < SHORT_RUN;
        (self.take_free(n, Some(short)))
            .or_else(|| self.grow(device, queue, n, short))
            // (A page grown to its limit may still be short of a run this long.)
            .or_else(|| self.open(device, n, short))
            .or_else(|| self.take_free(n, None))
            .or_else(|| self.grow(device, queue, n, !short))
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
    /// run that fits them in a page of their kind when there is one (the whole old run is
    /// given back, so the room a full chunk needs comes free in one piece), else the run's
    /// tail is given back. Where they are now. The copies are recorded into `encoder`.
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
        let Some(to) = self.take_free(new, Some(new < SHORT_RUN)) else {
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

    /// Some page has memory and nothing in it.
    pub fn spare_page(&self) -> bool {
        self.pages.iter().any(Page::spare)
    }

    /// The last page with memory and nothing in it gives the memory back (the page keeps
    /// its number and a stand-in buffer, and can start again); whether one did.
    pub fn release_spare_page(&mut self, device: &wgpu::Device) -> bool {
        let Some(page) = self.pages.iter().rposition(Page::spare) else {
            return false;
        };
        let p = &mut self.pages[page];
        p.buffer = page_buffer(device, GRAIN);
        p.capacity = 0;
        p.free.clear();
        self.generation += 1;
        true
    }

    /// Pages of at most `blades` (tests: small pages fill up quickly). Before the first run:
    /// the cull records' places follow the page size.
    #[cfg(test)]
    pub fn set_page_limit(&mut self, blades: u32) {
        assert!(self.pages.is_empty());
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

    /// A long run's length (blades).
    const LONG: u32 = 4 * SHORT_RUN;

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
        // A long run starts a page of its own kind.
        let long = pool.alloc(&device, &queue, LONG).expect("room");
        assert_eq!((long.page, long.at), (1, 0));
        // More than that page holds: it grows (copying), its runs stay.
        let generation = pool.generation;
        let big = pool.alloc(&device, &queue, FIRST_PAGE).expect("room");
        assert_eq!((big.page, big.at), (1, LONG));
        assert!(pool.generation > generation);
        // A run bigger than any page can't be held.
        assert_eq!(pool.alloc(&device, &queue, pool.max_page + 1), None);
        // Fill that page up: the next long run opens another page.
        let rest = pool.max_page - (big.at + FIRST_PAGE);
        if rest >= GRAIN {
            pool.alloc(&device, &queue, rest).expect("room");
        }
        let next = pool.alloc(&device, &queue, LONG).expect("room");
        assert_eq!(next.page, 2);
        assert_eq!((pool.pages(), pool.live_pages()), (3, 3));
        // Short runs still go to their page.
        assert_eq!(pool.alloc(&device, &queue, 640).map(|s| s.page), Some(0));
    }

    #[test]
    fn small_runs_fill_small_holes() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let big = pool.alloc(&device, &queue, 8000).expect("room");
        let _between = pool.alloc(&device, &queue, 64).expect("room");
        let small = pool.alloc(&device, &queue, 640).expect("room");
        let keep = pool.alloc(&device, &queue, 64).expect("room");
        pool.free(big, 8000);
        pool.free(small, 640);
        // The hole it fits best in, not the first one: the big hole stays whole.
        assert_eq!(pool.alloc(&device, &queue, 600), Some(small));
        assert_eq!(pool.alloc(&device, &queue, 8000), Some(big));
        assert_eq!((keep.page, keep.at), (0, 8000 + 64 + 640));
    }

    #[test]
    fn short_and_long_runs_keep_to_their_pages() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        // A chunk near the camera, then far chunks coming and going around it.
        let near = pool.alloc(&device, &queue, LONG).expect("room");
        let far: Vec<Slot> = (0..40)
            .map(|_| pool.alloc(&device, &queue, 3200).expect("room"))
            .collect();
        assert!(far.iter().all(|s| s.page != near.page));
        // The near chunk goes: its room is whole for the next one, whatever short runs
        // were asked for meanwhile.
        pool.free(near, LONG);
        for _ in 0..40 {
            let s = pool.alloc(&device, &queue, 3200).expect("room");
            assert_ne!(s.page, near.page);
        }
        assert_eq!(pool.alloc(&device, &queue, LONG), Some(near));
        // With every page of the other kind full, a run goes wherever room is left.
        let mut tight = BladePool::new(&device);
        tight.set_page_limit(LONG);
        for _ in 0..MAX_PAGES - 1 {
            tight.alloc(&device, &queue, LONG).expect("room");
        }
        let short = tight.alloc(&device, &queue, 640).expect("room");
        assert_eq!(short.page as usize, MAX_PAGES - 1);
        let long = tight.alloc(&device, &queue, LONG - 640).expect("room");
        assert_eq!((long.page, long.at), (short.page, 640));
        assert_eq!(tight.alloc(&device, &queue, 64), None);
    }

    #[test]
    fn a_trimmed_run_moves_out_of_the_way() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let mut encoder = device.create_command_encoder(&Default::default());
        let hole = pool.alloc(&device, &queue, LONG).expect("room");
        let big = pool.alloc(&device, &queue, 2 * LONG).expect("room");
        let after = pool.alloc(&device, &queue, LONG).expect("room");
        let far = pool.alloc(&device, &queue, 640).expect("room");
        pool.free(hole, LONG);
        let (capacity, free, _) = pool.room();
        // Trimmed to half: the blades kept move into the hole and the whole run comes free,
        // in one piece.
        let moved = pool.trim(&device, &mut encoder, big, 2 * LONG, LONG - 100);
        assert_eq!(moved, hole);
        assert_eq!(pool.room().0, capacity);
        assert_eq!(pool.room().1, free + 2 * LONG as u64 - (LONG as u64 - 64));
        let again = pool.alloc(&device, &queue, 2 * LONG).expect("room");
        assert_eq!((again.page, again.at), (big.page, LONG - 64));
        // Trimmed to a short run: it moves to the page for short runs.
        let short = pool.trim(&device, &mut encoder, again, 2 * LONG, 3000);
        assert_eq!((short.page, short.at), (far.page, 640));
        assert_eq!(pool.alloc(&device, &queue, 2 * LONG), Some(again));
        // No hole takes them: the run keeps its place and gives back its tail.
        let mut full = BladePool::new(&device);
        let all = full.alloc(&device, &queue, FIRST_PAGE).expect("room");
        let kept = full.trim(&device, &mut encoder, all, FIRST_PAGE, LONG);
        assert_eq!(kept, all);
        assert_eq!(
            full.room(),
            (
                FIRST_PAGE as u64,
                (FIRST_PAGE - LONG) as u64,
                FIRST_PAGE - LONG
            )
        );
        // Nothing to give back: untouched.
        assert_eq!(pool.trim(&device, &mut encoder, after, LONG, LONG), after);
        assert_eq!(
            pool.trim(&device, &mut encoder, after, LONG, LONG - 1),
            after
        );
        queue.submit([encoder.finish()]);
    }

    #[test]
    fn full_pages_refuse_and_an_empty_page_gives_its_memory_back() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        pool.set_page_limit(LONG);
        assert_eq!(pool.room_limit(), 4 * LONG as u64);
        let runs: Vec<Slot> = (0..MAX_PAGES)
            .map(|_| pool.alloc(&device, &queue, LONG).expect("room"))
            .collect();
        assert_eq!(pool.pages(), MAX_PAGES);
        // Every page full: no room, and no buffer outside the pages.
        assert_eq!(pool.alloc(&device, &queue, 64), None);
        assert_eq!(pool.alloc(&device, &queue, LONG), None);
        assert_eq!(pool.bytes(), 4 * LONG as u64 * BLADE_BYTES);
        // A page empties: its memory can go; the page keeps its number (the others' runs
        // name theirs).
        assert!(!pool.spare_page());
        pool.free(runs[1], LONG);
        let generation = pool.generation;
        assert!(pool.spare_page() && pool.release_spare_page(&device));
        assert!(!pool.spare_page() && !pool.release_spare_page(&device));
        assert_eq!(
            (pool.pages(), pool.live_pages()),
            (MAX_PAGES, MAX_PAGES - 1)
        );
        assert!(pool.generation > generation);
        assert!(pool.bytes() < (3 * LONG as u64 + 2 * GRAIN as u64) * BLADE_BYTES);
        // And comes back when it is needed, in the same place.
        assert_eq!(pool.alloc(&device, &queue, LONG), Some(runs[1]));
        assert_eq!(pool.live_pages(), MAX_PAGES);
    }

    #[test]
    fn records_mirror_the_pages() {
        let Some((device, queue)) = crate::render::gpu_tests::headless() else {
            return;
        };
        let mut pool = BladePool::new(&device);
        let short = pool.alloc(&device, &queue, 640).expect("room");
        let _first = pool.alloc(&device, &queue, LONG).expect("room");
        let long = pool.alloc(&device, &queue, LONG).expect("room");
        assert_eq!(pool.record_at(short), 0);
        assert_eq!(pool.record_at(long), pool.max_page + LONG);
        assert_eq!(pool.records_end(0), FIRST_PAGE as u64);
        assert_eq!(pool.records_end(1), (pool.max_page + FIRST_PAGE) as u64);
        // Every run's records fit under the limit of the arena on this device.
        let last = Slot {
            page: MAX_PAGES as u32 - 1,
            at: pool.max_page - GRAIN,
        };
        let end = pool.record_at(last) as u64 + GRAIN as u64;
        assert!(end <= crate::render::grass_cull::Arena::limit(&device));
    }
}
