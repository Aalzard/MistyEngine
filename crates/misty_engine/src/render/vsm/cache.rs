//! The physical page cache of the virtual shadow map (CPU side): which global pages are
//! wanted (requested by visible pixels within the last frames, or by the coarse safety
//! level), which physical page each cached one lives in, which need rendering (new, or
//! invalidated by a moving caster), and least-recently-wanted eviction when the pool is full.
//! Pure bookkeeping, no GPU objects, so it is unit-tested on its own.

use std::collections::HashMap;

use glam::IVec2;

/// A global page of one clipmap level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageKey {
    pub level: u8,
    pub page: IVec2Key,
}

/// `IVec2` with the ordering and hashing the cache needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IVec2Key {
    pub y: i32,
    pub x: i32,
}

impl From<IVec2> for IVec2Key {
    fn from(v: IVec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<IVec2Key> for IVec2 {
    fn from(v: IVec2Key) -> Self {
        IVec2::new(v.x, v.y)
    }
}

impl PageKey {
    pub fn new(level: usize, page: IVec2) -> Self {
        Self {
            level: level as u8,
            page: page.into(),
        }
    }

    pub fn page(&self) -> IVec2 {
        self.page.into()
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    phys: u32,
    /// Last frame the page was wanted.
    used: u64,
    /// Holds a rendered depth page (else it must not be addressed).
    rendered: bool,
    /// Rendered, but a caster over it moved since: render again.
    dirty: bool,
}

/// Counters of the last update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Pages wanted this frame.
    pub wanted: u32,
    /// Pages holding a physical page.
    pub resident: u32,
    /// Allocated this frame.
    pub allocated: u32,
    /// Evicted (least recently wanted) to make room this frame.
    pub evicted: u32,
    /// Wanted pages left without a physical page (pool full of wanted pages).
    pub overflow: u32,
}

pub struct PageCache {
    capacity: u32,
    free: Vec<u32>,
    pages: HashMap<PageKey, Entry>,
    /// Frame each page was last requested.
    wanted: HashMap<PageKey, u64>,
    pub stats: CacheStats,
}

impl PageCache {
    pub fn new(capacity: u32) -> Self {
        Self {
            capacity,
            free: (0..capacity).rev().collect(),
            pages: HashMap::new(),
            wanted: HashMap::new(),
            stats: CacheStats::default(),
        }
    }

    /// Forget every page (the light turned, the depth range changed).
    pub fn clear(&mut self) {
        self.pages.clear();
        self.free = (0..self.capacity).rev().collect();
    }

    /// A page is wanted as of `frame`.
    pub fn request(&mut self, key: PageKey, frame: u64) {
        let e = self.wanted.entry(key).or_insert(frame);
        *e = (*e).max(frame);
    }

    /// Render the cached pages of `level` within `lo..=hi` again (a caster over them moved).
    pub fn invalidate(&mut self, level: usize, lo: IVec2, hi: IVec2) {
        let area = (hi - lo + IVec2::ONE).max(IVec2::ZERO);
        let area = area.x as i64 * area.y as i64;
        if area <= 256 {
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    if let Some(e) = self.pages.get_mut(&PageKey::new(level, IVec2::new(x, y))) {
                        e.dirty = true;
                    }
                }
            }
            return;
        }
        for (k, e) in &mut self.pages {
            let p = k.page();
            if k.level as usize == level && p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y
            {
                e.dirty = true;
            }
        }
    }

    /// Bring the cache up to `frame`: drop requests older than `keep` frames or outside the
    /// windows (`addressable`), give every wanted page a physical page (coarse levels first,
    /// evicting the least recently wanted pages that aren't wanted), and return the pages to
    /// render, most urgent first (never rendered before coarse levels first, then the
    /// invalidated ones), at most `budget`.
    pub fn update(
        &mut self,
        frame: u64,
        keep: u64,
        addressable: impl Fn(&PageKey) -> bool,
        budget: usize,
    ) -> Vec<PageKey> {
        let mut stats = CacheStats::default();
        self.wanted
            .retain(|k, f| *f + keep >= frame && addressable(k));
        stats.wanted = self.wanted.len() as u32;
        let mut missing: Vec<PageKey> = Vec::new();
        for (k, f) in &self.wanted {
            match self.pages.get_mut(k) {
                Some(e) => e.used = e.used.max(*f),
                None => missing.push(*k),
            }
        }
        // Coarse levels first: they are the fallback of the fine ones.
        missing.sort_by(|a, b| b.level.cmp(&a.level).then(a.page.cmp(&b.page)));
        let mut victims: Vec<(u64, PageKey)> = Vec::new();
        if missing.len() > self.free.len() {
            victims = self
                .pages
                .iter()
                .filter(|(k, _)| !self.wanted.contains_key(k))
                .map(|(k, e)| (e.used, *k))
                .collect();
            // Oldest last (popped first).
            victims.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        }
        for k in missing {
            let phys = match self.free.pop() {
                Some(p) => p,
                None => match victims.pop() {
                    Some((_, v)) => {
                        stats.evicted += 1;
                        self.pages.remove(&v).map(|e| e.phys).unwrap_or(0)
                    }
                    None => {
                        stats.overflow += 1;
                        continue;
                    }
                },
            };
            stats.allocated += 1;
            self.pages.insert(
                k,
                Entry {
                    phys,
                    used: frame,
                    rendered: false,
                    dirty: false,
                },
            );
        }
        stats.resident = self.pages.len() as u32;
        self.stats = stats;
        let mut todo: Vec<(bool, PageKey)> = self
            .pages
            .iter()
            .filter(|(k, e)| (!e.rendered || e.dirty) && addressable(k))
            .filter(|(k, e)| !e.rendered || self.wanted.contains_key(k))
            .map(|(k, e)| (e.rendered, *k))
            .collect();
        todo.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.level.cmp(&a.1.level))
                .then(a.1.page.cmp(&b.1.page))
        });
        todo.truncate(budget);
        todo.into_iter().map(|(_, k)| k).collect()
    }

    /// The page now holds rendered depth.
    pub fn rendered(&mut self, key: &PageKey) {
        if let Some(e) = self.pages.get_mut(key) {
            e.rendered = true;
            e.dirty = false;
        }
    }

    /// The page holds rendered depth (it may be dirty).
    pub fn is_rendered(&self, key: &PageKey) -> bool {
        self.pages.get(key).is_some_and(|e| e.rendered)
    }

    /// Physical page of a cached page.
    pub fn phys(&self, key: &PageKey) -> Option<u32> {
        self.pages.get(key).map(|e| e.phys)
    }

    /// Rendered pages and their physical pages (what the page table may address).
    pub fn addressable(&self) -> impl Iterator<Item = (&PageKey, u32)> {
        self.pages
            .iter()
            .filter(|(_, e)| e.rendered)
            .map(|(k, e)| (k, e.phys))
    }

    #[cfg(test)]
    pub fn resident(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(level: usize, x: i32, y: i32) -> PageKey {
        PageKey::new(level, IVec2::new(x, y))
    }

    #[test]
    fn wanted_pages_are_allocated_rendered_once_and_then_cached() {
        let mut c = PageCache::new(8);
        for x in 0..4 {
            c.request(key(0, x, 0), 1);
        }
        c.request(key(3, 0, 0), 1);
        let todo = c.update(1, 10, |_| true, 100);
        assert_eq!(todo.len(), 5);
        // Coarse first.
        assert_eq!(todo[0].level, 3);
        for k in &todo {
            c.rendered(k);
        }
        // Still wanted next frame: nothing to render.
        c.request(key(0, 1, 0), 2);
        assert!(c.update(2, 10, |_| true, 100).is_empty());
        assert_eq!(c.stats.resident, 5);
        assert_eq!(c.addressable().count(), 5);
    }

    #[test]
    fn a_moving_caster_dirties_only_its_pages() {
        let mut c = PageCache::new(16);
        for x in 0..4 {
            c.request(key(1, x, 0), 1);
        }
        for k in c.update(1, 10, |_| true, 100) {
            c.rendered(&k);
        }
        c.invalidate(1, IVec2::new(1, 0), IVec2::new(2, 5));
        c.invalidate(0, IVec2::new(-100, -100), IVec2::new(100, 100));
        let todo = c.update(2, 10, |_| true, 100);
        assert_eq!(todo, vec![key(1, 1, 0), key(1, 2, 0)]);
        // Dirty pages stay addressable meanwhile.
        assert_eq!(c.addressable().count(), 4);
    }

    #[test]
    fn eviction_takes_the_least_recently_wanted_and_never_a_wanted_page() {
        let mut c = PageCache::new(3);
        c.request(key(0, 0, 0), 1);
        c.request(key(0, 1, 0), 2);
        c.request(key(0, 2, 0), 3);
        for k in c.update(3, 100, |_| true, 100) {
            c.rendered(&k);
        }
        // Requests expire after `keep` frames: at frame 10 with keep 7 only frame-3 requests
        // remain wanted.
        c.request(key(0, 3, 0), 10);
        let todo = c.update(10, 7, |_| true, 100);
        assert_eq!(todo, vec![key(0, 3, 0)]);
        assert_eq!(c.stats.evicted, 1);
        assert!(c.phys(&key(0, 0, 0)).is_none(), "oldest evicted");
        assert!(c.phys(&key(0, 2, 0)).is_some());
        // Pool full of wanted pages: the rest overflow.
        c.request(key(0, 2, 0), 11);
        c.request(key(0, 3, 0), 11);
        c.request(key(0, 1, 0), 11);
        c.request(key(0, 4, 0), 11);
        c.update(11, 7, |_| true, 100);
        assert_eq!(c.stats.overflow, 1);
    }

    #[test]
    fn pages_outside_the_window_are_not_rendered_but_stay_cached() {
        let mut c = PageCache::new(4);
        c.request(key(0, 0, 0), 1);
        for k in c.update(1, 10, |_| true, 100) {
            c.rendered(&k);
        }
        c.invalidate(0, IVec2::new(-5, -5), IVec2::new(5, 5));
        assert!(c.update(2, 10, |k| k.page.x != 0, 100).is_empty());
        assert_eq!(c.resident(), 1);
        c.clear();
        assert_eq!(c.resident(), 0);
    }

    #[test]
    fn the_budget_defers_the_rest() {
        let mut c = PageCache::new(64);
        for x in 0..40 {
            c.request(key(0, x, 0), 1);
        }
        assert_eq!(c.update(1, 10, |_| true, 16).len(), 16);
        assert_eq!(c.update(2, 10, |_| true, 100).len(), 40);
    }
}
