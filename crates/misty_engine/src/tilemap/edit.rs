//! Brush operations of the Tile Palette tools: paint, box fill, erase, flood fill, pick and move.
//! Each returns how many cells changed, so callers only record undo steps for real edits.

use std::collections::{HashSet, VecDeque};

use glam::IVec2;

use super::{TileTransform, Tilemap};

/// Largest region a flood fill touches.
pub const FLOOD_LIMIT: usize = 1 << 16;

/// Most cells a box fill or pick covers (a drag across a huge area does nothing instead of
/// looping over billions of cells).
pub const BOX_LIMIT: i64 = 1 << 22;

fn box_too_big(lo: IVec2, hi: IVec2) -> bool {
    let d = hi.as_i64vec2() - lo.as_i64vec2() + glam::I64Vec2::ONE;
    d.x.saturating_mul(d.y) > BOX_LIMIT
}

/// One cell of a brush pattern (`tile` None = erase there).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrushCell {
    pub offset: IVec2,
    pub tile: Option<String>,
    pub transform: TileTransform,
}

/// The tiles a paint stroke stamps: one tile, or a block picked from a palette or tilemap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Brush {
    pub cells: Vec<BrushCell>,
}

impl Brush {
    pub fn single(tile: &str, transform: TileTransform) -> Self {
        Self {
            cells: vec![BrushCell {
                offset: IVec2::ZERO,
                tile: Some(tile.to_owned()),
                transform,
            }],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Size in cells of the pattern's bounding box.
    pub fn size(&self) -> IVec2 {
        let (lo, hi) = self.extent();
        hi - lo + IVec2::ONE
    }

    fn extent(&self) -> (IVec2, IVec2) {
        let mut lo = IVec2::splat(i32::MAX);
        let mut hi = IVec2::splat(i32::MIN);
        for c in &self.cells {
            lo = lo.min(c.offset);
            hi = hi.max(c.offset);
        }
        if self.cells.is_empty() {
            (IVec2::ZERO, IVec2::ZERO)
        } else {
            (lo, hi)
        }
    }

    /// The first tile of the pattern (shown as the active tile).
    pub fn first_tile(&self) -> Option<&str> {
        self.cells.iter().find_map(|c| c.tile.as_deref())
    }

    /// Turn the brush a quarter turn (`[` = counter-clockwise, `]` = clockwise): every tile
    /// rotates, and a multi-cell pattern turns about its first cell.
    pub fn rotated(&self, ccw: bool) -> Self {
        let turn = |o: IVec2| {
            if ccw {
                IVec2::new(-o.y, o.x)
            } else {
                IVec2::new(o.y, -o.x)
            }
        };
        self.mapped(turn, |t| t.rotated(ccw))
    }

    /// Mirror the brush left-right (Shift+[) or top-bottom (Shift+]).
    pub fn flipped(&self, x: bool) -> Self {
        let mirror = |o: IVec2| {
            if x {
                IVec2::new(-o.x, o.y)
            } else {
                IVec2::new(o.x, -o.y)
            }
        };
        self.mapped(mirror, |t| if x { t.flipped_x() } else { t.flipped_y() })
    }

    fn mapped(
        &self,
        pos: impl Fn(IVec2) -> IVec2,
        t: impl Fn(TileTransform) -> TileTransform,
    ) -> Self {
        let mut cells: Vec<BrushCell> = self
            .cells
            .iter()
            .map(|c| BrushCell {
                offset: pos(c.offset),
                tile: c.tile.clone(),
                transform: t(c.transform),
            })
            .collect();
        // Keep offsets starting at the pattern's lower-left corner.
        let lo = cells
            .iter()
            .fold(IVec2::splat(i32::MAX), |m, c| m.min(c.offset));
        if !cells.is_empty() {
            for c in &mut cells {
                c.offset -= lo;
            }
        }
        Self { cells }
    }
}

/// Stamp `brush` with its lower-left cell at `at`.
pub fn paint(map: &mut Tilemap, at: IVec2, brush: &Brush) -> usize {
    brush
        .cells
        .iter()
        .filter(|c| map.set(at + c.offset, c.tile.as_deref(), c.transform))
        .count()
}

/// Stamp `brush` along a line of cells from `a` to `b` (a dragged stroke).
pub fn paint_line(map: &mut Tilemap, a: IVec2, b: IVec2, brush: &Brush) -> usize {
    line(a, b).into_iter().map(|c| paint(map, c, brush)).sum()
}

/// Cells of a line (Bresenham), both ends included.
pub fn line(a: IVec2, b: IVec2) -> Vec<IVec2> {
    // Ends far apart (cells from a huge drag): no line rather than overflowing `2 * err`.
    let span = (b.as_i64vec2() - a.as_i64vec2()).abs();
    if span.max_element() > i64::from(super::Grid::CELL_LIMIT) * 2 {
        return vec![a];
    }
    let d = (b - a).abs();
    let s = (b - a).signum();
    let mut err = d.x - d.y;
    let mut p = a;
    let mut out = vec![p];
    while p != b {
        let e2 = 2 * err;
        if e2 > -d.y {
            err -= d.y;
            p.x += s.x;
        }
        if e2 < d.x {
            err += d.x;
            p.y += s.y;
        }
        out.push(p);
    }
    out
}

/// Inclusive rectangle between two corners, as (min, max).
pub fn rect(a: IVec2, b: IVec2) -> (IVec2, IVec2) {
    (a.min(b), a.max(b))
}

/// Fill the rectangle between `a` and `b` with the brush tiled across it.
pub fn box_fill(map: &mut Tilemap, a: IVec2, b: IVec2, brush: &Brush) -> usize {
    let (lo, hi) = rect(a, b);
    if box_too_big(lo, hi) {
        return 0;
    }
    let size = brush.size().max(IVec2::ONE);
    let mut n = 0;
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let rel = IVec2::new(x, y) - lo;
            let o = IVec2::new(rel.x.rem_euclid(size.x), rel.y.rem_euclid(size.y));
            if let Some(c) = brush.cells.iter().find(|c| c.offset == o) {
                n += map.set(IVec2::new(x, y), c.tile.as_deref(), c.transform) as usize;
            }
        }
    }
    n
}

/// Erase the rectangle between `a` and `b`.
pub fn box_erase(map: &mut Tilemap, a: IVec2, b: IVec2) -> usize {
    let (lo, hi) = rect(a, b);
    let cells: Vec<IVec2> = map.cells_in(lo, hi).map(|(c, _)| c).collect();
    cells
        .into_iter()
        .filter(|c| map.set(*c, None, TileTransform::default()))
        .count()
}

/// Erase `brush`-sized blocks at each cell (the eraser uses the brush's footprint).
pub fn erase(map: &mut Tilemap, at: IVec2, size: IVec2) -> usize {
    box_erase(map, at, at + size.max(IVec2::ONE) - IVec2::ONE)
}

/// Replace the 4-connected region of cells holding the same tile as `start` (or empty) with
/// `tile` (None = erase). Empty regions are bounded by the painted area (plus `start`).
pub fn flood_fill(
    map: &mut Tilemap,
    start: IVec2,
    tile: Option<&str>,
    transform: TileTransform,
) -> usize {
    let target = map.tile_at(start).map(str::to_owned);
    if target.as_deref() == tile && map.get(start).is_none_or(|c| c.transform == transform) {
        return 0;
    }
    let (lo, hi) = match map.bounds() {
        Some((lo, hi)) => (lo.min(start), hi.max(start)),
        None => (start, start),
    };
    let mut seen: HashSet<IVec2> = HashSet::new();
    let mut queue = VecDeque::from([start]);
    let mut region = Vec::new();
    while let Some(c) = queue.pop_front() {
        if region.len() >= FLOOD_LIMIT
            || c.cmplt(lo).any()
            || c.cmpgt(hi).any()
            || !seen.insert(c)
            || map.tile_at(c) != target.as_deref()
        {
            continue;
        }
        region.push(c);
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            queue.push_back(c + d);
        }
    }
    region
        .into_iter()
        .filter(|c| map.set(*c, tile, transform))
        .count()
}

/// The tiles of a rectangle as a brush (Pick tool; empty cells erase when stamped).
pub fn pick(map: &Tilemap, a: IVec2, b: IVec2) -> Brush {
    let (lo, hi) = rect(a, b);
    if box_too_big(lo, hi) {
        return Brush { cells: Vec::new() };
    }
    let mut cells = Vec::new();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let c = IVec2::new(x, y);
            let cell = map.get(c);
            cells.push(BrushCell {
                offset: c - lo,
                tile: cell.map(|t| t.tile.to_string()),
                transform: cell.map(|t| t.transform).unwrap_or_default(),
            });
        }
    }
    Brush { cells }
}

/// Move the tiles of a rectangle by `delta` (Select / Move tool); returns cells changed.
pub fn move_cells(map: &mut Tilemap, a: IVec2, b: IVec2, delta: IVec2) -> usize {
    if delta == IVec2::ZERO {
        return 0;
    }
    let (lo, hi) = rect(a, b);
    let picked: Vec<(IVec2, super::TileCell)> =
        map.cells_in(lo, hi).map(|(c, t)| (c, t.clone())).collect();
    let mut n = box_erase(map, lo, hi);
    for (c, t) in picked {
        n += map.set(c + delta, Some(&t.tile), t.transform) as usize;
    }
    n
}
