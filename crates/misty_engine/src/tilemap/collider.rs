//! Tilemap Collider 2D shapes: one box per tile whose Collider Type is not None, or with Used By
//! Composite, full-cell tiles merged into as few boxes as possible (horizontal runs first, then
//! runs of equal width stacked vertically). Also used by Shadow Caster 2D on tilemaps.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use glam::{IVec2, Vec2};

use super::tile::{self, ColliderType};
use super::{Grid, GridLayout, Tilemap};

/// A collider piece in the Tilemap's local space.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Box {
        min: Vec2,
        max: Vec2,
    },
    /// Convex outline (isometric cells).
    Polygon(Vec<Vec2>),
}

impl Shape {
    /// Outline points (counter-clockwise for boxes).
    pub fn points(&self) -> Vec<Vec2> {
        match self {
            Shape::Box { min, max } => {
                vec![*min, Vec2::new(max.x, min.y), *max, Vec2::new(min.x, max.y)]
            }
            Shape::Polygon(p) => p.clone(),
        }
    }
}

/// Where a cell's sprite sits inside it (local bounds), when it covers less than the cell.
fn sprite_rect(map: &Tilemap, grid: &Grid, c: IVec2, sprite: Option<&str>) -> Option<(Vec2, Vec2)> {
    let s = crate::sprites::resolve(sprite?)?;
    let (lo, hi) = s.bounds();
    let anchor = grid.cell_center(c) - grid.cell_size * 0.5 + grid.cell_size * map.tile_anchor;
    Some((anchor + lo, anchor + hi))
}

/// Cells that collide, with their local rectangle when it is not exactly the cell.
fn solid_cells(map: &Tilemap, grid: &Grid) -> Vec<(IVec2, Option<(Vec2, Vec2)>)> {
    let mut out = Vec::new();
    for (&(x, y), cell) in &map.tiles.0 {
        let c = IVec2::new(x, y);
        let Some(asset) = tile::get(&cell.tile) else {
            continue;
        };
        let r = tile::resolve_with(&asset, &cell.tile, map, c, cell.transform, 0.0);
        match r.collider {
            ColliderType::None => {}
            ColliderType::Grid => out.push((c, None)),
            ColliderType::Sprite => {
                let rect = sprite_rect(map, grid, c, r.sprite.as_deref());
                let center = grid.cell_center(c);
                let h = grid.cell_size * 0.5;
                let full = rect.is_none_or(|(lo, hi)| {
                    (lo - (center - h)).abs().max_element() < 1e-3
                        && (hi - (center + h)).abs().max_element() < 1e-3
                });
                if rect.is_some() || full {
                    out.push((c, if full { None } else { rect }));
                }
            }
        }
    }
    out
}

/// Inclusive cell rectangles covering `cells` (greedy: horizontal runs, stacked when equal).
pub fn merge(cells: &[IVec2]) -> Vec<(IVec2, IVec2)> {
    let mut rows: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for c in cells {
        rows.entry(c.y).or_default().push(c.x);
    }
    // Open rectangles keyed by their x span, with the rows they cover so far.
    let mut open: BTreeMap<(i32, i32), (i32, i32)> = BTreeMap::new();
    let mut done = Vec::new();
    let mut last_y = None;
    for (y, mut xs) in rows {
        xs.sort_unstable();
        xs.dedup();
        let mut runs = Vec::new();
        let mut i = 0;
        while i < xs.len() {
            let start = xs[i];
            while i + 1 < xs.len() && xs[i + 1] == xs[i] + 1 {
                i += 1;
            }
            runs.push((start, xs[i]));
            i += 1;
        }
        let contiguous = last_y == Some(y - 1);
        let mut next = BTreeMap::new();
        for run in runs {
            let above = if contiguous { open.remove(&run) } else { None };
            match above {
                Some((y0, _)) => next.insert(run, (y0, y)),
                None => next.insert(run, (y, y)),
            };
        }
        for ((x0, x1), (y0, y1)) in std::mem::replace(&mut open, next) {
            done.push((IVec2::new(x0, y0), IVec2::new(x1, y1)));
        }
        last_y = Some(y);
    }
    for ((x0, x1), (y0, y1)) in open {
        done.push((IVec2::new(x0, y0), IVec2::new(x1, y1)));
    }
    done.sort_by_key(|(lo, _)| (lo.y, lo.x));
    done
}

/// Local-space collider shapes of a Tilemap on `grid`.
pub fn shapes(map: &Tilemap, grid: &Grid, composite: bool) -> Vec<Shape> {
    let cells = solid_cells(map, grid);
    let h = grid.cell_size * 0.5;
    if grid.layout == GridLayout::Isometric {
        return cells
            .iter()
            .map(|(c, _)| {
                let m = grid.cell_center(*c);
                Shape::Polygon(vec![
                    m - Vec2::new(0.0, h.y),
                    m + Vec2::new(h.x, 0.0),
                    m + Vec2::new(0.0, h.y),
                    m - Vec2::new(h.x, 0.0),
                ])
            })
            .collect();
    }
    let mut out = Vec::new();
    let mut full = Vec::new();
    for (c, rect) in &cells {
        match rect {
            Some((min, max)) => out.push(Shape::Box {
                min: *min,
                max: *max,
            }),
            None => full.push(*c),
        }
    }
    // Merging across gaps would close them, so gaps keep one box per cell.
    let merge_ok = composite && grid.cell_gap == Vec2::ZERO;
    let rects = if merge_ok {
        merge(&full)
    } else {
        full.iter().map(|c| (*c, *c)).collect()
    };
    for (lo, hi) in rects {
        out.push(Shape::Box {
            min: grid.cell_center(lo) - h,
            max: grid.cell_center(hi) + h,
        });
    }
    out
}

/// Hash of what the collider depends on (cells, transforms and tile asset versions).
pub fn signature(map: &Tilemap) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    map.tiles.0.hash(&mut h);
    map.tile_anchor.to_array().map(f32::to_bits).hash(&mut h);
    tile::generation().hash(&mut h);
    h.finish()
}

impl Hash for super::TileData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for (k, v) in &self.0 {
            k.hash(state);
            v.hash(state);
        }
    }
}

/// The rapier collider of a Tilemap Collider 2D (boxes, or convex cells, in one compound),
/// scaled by the object's world X/Y scale; None when no tile collides.
pub(crate) fn builder(
    scene: &crate::scene::Scene,
    e: hecs::Entity,
    c: &super::TilemapCollider2D,
    scale: Vec2,
) -> Option<rapier2d::prelude::ColliderBuilder> {
    use rapier2d::prelude as rp;
    let map = scene.world.get::<&Tilemap>(e).ok()?;
    let grid = super::grid_of(scene, e);
    let v = |p: Vec2| rp::Vector::new(p.x * scale.x, p.y * scale.y);
    let parts: Vec<(rp::Pose, rp::SharedShape)> = shapes(&map, &grid, c.used_by_composite)
        .into_iter()
        .filter_map(|s| match s {
            Shape::Box { min, max } => {
                let h = ((max - min) * 0.5 * scale).max(Vec2::splat(1e-3));
                let center = (min + max) * 0.5;
                Some((
                    rp::Pose::new(v(center), 0.0),
                    rp::SharedShape::cuboid(h.x, h.y),
                ))
            }
            Shape::Polygon(p) => {
                let pts: Vec<rp::Vector> = p.into_iter().map(v).collect();
                rp::SharedShape::convex_hull(&pts).map(|s| (rp::Pose::IDENTITY, s))
            }
        })
        .collect();
    (!parts.is_empty()).then(|| rp::ColliderBuilder::compound(parts))
}
