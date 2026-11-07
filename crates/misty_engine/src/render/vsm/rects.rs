//! Which pages render together: the pages to render this frame are grouped per level into
//! rectangles of neighbouring pages (greedy, at most `MAX_RECT` pages a side), each drawn
//! once with its own view-projection into a scratch depth atlas, then copied page by page into
//! the physical pool. Drawing a rectangle instead of each page culls and submits every caster
//! once per neighbourhood instead of once per page.

use std::collections::HashSet;

use glam::IVec2;

use super::cache::PageKey;

/// Most pages on a rectangle side (the scratch atlas side in pages).
pub const MAX_RECT: i32 = 16;

/// Neighbouring pages of one level drawn with one view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRect {
    /// The pages' set (see `PageKey::set`).
    pub set: u8,
    pub level: usize,
    /// First global page.
    pub lo: IVec2,
    /// Pages across and down.
    pub size: IVec2,
}

impl PageRect {
    /// Exclusive end page.
    pub fn hi(&self) -> IVec2 {
        self.lo + self.size
    }
}

/// Group `pages` into rectangles per level (rows first, then grown downwards while the whole
/// row below is to render too).
pub fn group(pages: &[PageKey]) -> Vec<PageRect> {
    let mut levels: Vec<(u8, usize)> = pages.iter().map(|k| (k.set, k.level as usize)).collect();
    levels.sort_unstable();
    levels.dedup();
    let mut out = Vec::new();
    for (set, level) in levels {
        let mut todo: HashSet<(i32, i32)> = pages
            .iter()
            .filter(|k| k.set == set && k.level as usize == level)
            .map(|k| (k.page.x, k.page.y))
            .collect();
        let mut order: Vec<(i32, i32)> = todo.iter().copied().collect();
        order.sort_by_key(|&(x, y)| (y, x));
        for (x, y) in order {
            if !todo.contains(&(x, y)) {
                continue;
            }
            let mut w = 1;
            while w < MAX_RECT && todo.contains(&(x + w, y)) {
                w += 1;
            }
            let mut h = 1;
            while h < MAX_RECT && (0..w).all(|i| todo.contains(&(x + i, y + h))) {
                h += 1;
            }
            for j in 0..h {
                for i in 0..w {
                    todo.remove(&(x + i, y + j));
                }
            }
            out.push(PageRect {
                set,
                level,
                lo: IVec2::new(x, y),
                size: IVec2::new(w, h),
            });
        }
    }
    out
}

/// Where each rectangle lands in the scratch atlas (`side` pages a side): the round it is drawn
/// in and its first page there. Shelf packing, tallest rectangles first.
pub fn pack(rects: &[PageRect], side: i32) -> Vec<(usize, IVec2)> {
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by_key(|&i| (-rects[i].size.y, -rects[i].size.x, i));
    let mut out = vec![(0, IVec2::ZERO); rects.len()];
    let (mut round, mut x, mut y, mut shelf) = (0usize, 0, 0, 0);
    for i in order {
        let s = rects[i].size.min(IVec2::splat(side));
        if x + s.x > side {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        if y + s.y > side {
            round += 1;
            x = 0;
            y = 0;
            shelf = 0;
        }
        out[i] = (round, IVec2::new(x, y));
        x += s.x;
        shelf = shelf.max(s.y);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(level: usize, pages: &[(i32, i32)]) -> Vec<PageKey> {
        pages
            .iter()
            .map(|&(x, y)| PageKey::new(level, IVec2::new(x, y)))
            .collect()
    }

    #[test]
    fn neighbours_merge_and_every_page_is_covered_once() {
        let mut pages = Vec::new();
        for y in 0..3 {
            for x in 0..5 {
                pages.push((x, y));
            }
        }
        pages.push((10, 10));
        pages.push((6, 0));
        let mut all = keys(2, &pages);
        all.extend(keys(5, &[(0, 0), (1, 0)]));
        let rects = group(&all);
        let covered: usize = rects.iter().map(|r| (r.size.x * r.size.y) as usize).sum();
        assert_eq!(covered, all.len());
        // The 5 × 3 block is one rectangle (plus the lone pages and the level 5 pair).
        assert!(rects.contains(&PageRect {
            set: 0,
            level: 2,
            lo: IVec2::ZERO,
            size: IVec2::new(5, 3)
        }));
        assert_eq!(rects.len(), 4, "{rects:?}");
        for k in &all {
            let p = k.page();
            let n = rects
                .iter()
                .filter(|r| {
                    r.level == k.level as usize && p.cmpge(r.lo).all() && p.cmplt(r.hi()).all()
                })
                .count();
            assert_eq!(n, 1);
        }
    }

    #[test]
    fn rectangles_are_capped() {
        let pages: Vec<(i32, i32)> = (0..40).map(|x| (x, 0)).collect();
        let rects = group(&keys(0, &pages));
        assert_eq!(rects.len(), 3);
        assert!(rects.iter().all(|r| r.size.x <= MAX_RECT));
    }

    #[test]
    fn packing_never_overlaps_within_a_round() {
        let rects: Vec<PageRect> = (0..30)
            .map(|i| PageRect {
                set: 0,
                level: 0,
                lo: IVec2::new(i * 20, 0),
                size: IVec2::new(1 + i % 7, 1 + (i * 3) % 5),
            })
            .collect();
        let placed = pack(&rects, 16);
        for (i, a) in rects.iter().enumerate() {
            let (ra, pa) = placed[i];
            assert!(pa.x + a.size.x <= 16 && pa.y + a.size.y <= 16);
            for (j, b) in rects.iter().enumerate().skip(i + 1) {
                let (rb, pb) = placed[j];
                if ra != rb {
                    continue;
                }
                let apart = pa.x + a.size.x <= pb.x
                    || pb.x + b.size.x <= pa.x
                    || pa.y + a.size.y <= pb.y
                    || pb.y + b.size.y <= pa.y;
                assert!(apart, "{i} {j}");
            }
        }
    }
}
