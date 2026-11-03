//! Walkable cells → polygons: greedy rectangles of connected cells that lie on one plane
//! (within a small tolerance), then portal links between rectangles that touch.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use super::heightfield::{Cell, CompactField, DIRS, NONE};
use super::navmesh::{NavLink, NavPoly};

/// How far a cell may sit off its rectangle's plane.
const PLANE_TOLERANCE: f32 = 0.08;

/// A rectangle of cells with its plane `y = y0 + g · (p - p0)` (world x/z).
struct Rect {
    x0: u32,
    z0: u32,
    x1: u32,
    z1: u32,
    p0: Vec2,
    y0: f32,
    gradient: Vec2,
}

impl Rect {
    fn height(&self, p: Vec2) -> f32 {
        self.y0 + self.gradient.dot(p - self.p0)
    }
}

/// Rectangle grown from `seed` with one plane guess: a row along direction `along` (2 = +x,
/// 1 = +z), then more rows across it. Returns it and its cells.
fn grow(
    f: &CompactField,
    owner: &[u32],
    seed: u32,
    gradient: Vec2,
    along: usize,
) -> (Rect, Vec<u32>) {
    let across = 3 - along;
    let c0 = f.cells[seed as usize];
    let p0 = f.center(&c0);
    let fits = |i: u32| {
        if i == NONE || owner[i as usize] != NONE {
            return false;
        }
        let c = &f.cells[i as usize];
        let pred = c0.y + gradient.dot(f.center(c) - p0);
        (c.y - pred).abs() <= PLANE_TOLERANCE
    };
    let mut row = vec![seed];
    loop {
        let next = f.cells[*row.last().expect("row") as usize].links[along];
        if !fits(next) {
            break;
        }
        row.push(next);
    }
    let mut cells = row.clone();
    // Further rows while every cell has a fitting, connected neighbour.
    loop {
        let next: Vec<u32> = row
            .iter()
            .map(|&i| f.cells[i as usize].links[across])
            .collect();
        let ok = next.iter().all(|&i| fits(i))
            && next
                .windows(2)
                .all(|w| f.cells[w[0] as usize].links[along] == w[1]);
        if !ok {
            break;
        }
        cells.extend_from_slice(&next);
        row = next;
    }
    let (x1, z1) = cells.iter().fold((c0.x, c0.z), |(x, z), &i| {
        let c = &f.cells[i as usize];
        (x.max(c.x), z.max(c.z))
    });
    let rect = Rect {
        x0: c0.x,
        z0: c0.z,
        x1: x1 + 1,
        z1: z1 + 1,
        p0,
        y0: c0.y,
        gradient,
    };
    (rect, cells)
}

/// Slope guess from the seed's +x / +z neighbours.
fn neighbour_gradient(f: &CompactField, c: &Cell) -> Vec2 {
    let slope = |d: usize| {
        let n = c.links[d];
        if n == NONE {
            0.0
        } else {
            (f.cells[n as usize].y - c.y) / f.cell
        }
    };
    Vec2::new(slope(2), slope(1))
}

/// Polygons for the walkable cells (scan order, so the result is deterministic).
pub(super) fn build(f: &CompactField) -> Vec<NavPoly> {
    let mut owner = vec![NONE; f.cells.len()];
    let mut rects: Vec<(Rect, Vec<u32>)> = Vec::new();
    // Cells sorted by row, then column.
    let mut order: Vec<u32> = (0..f.cells.len() as u32).collect();
    order.sort_by_key(|&i| {
        let c = &f.cells[i as usize];
        (c.z, c.x)
    });
    for seed in order {
        if owner[seed as usize] != NONE {
            continue;
        }
        let c = f.cells[seed as usize];
        // Try the neighbours' slope and flat, rows along x and along z; keep the biggest
        // (the first on ties, so results are deterministic).
        let g = neighbour_gradient(f, &c);
        let (rect, cells) = [(g, 2), (g, 1), (Vec2::ZERO, 2), (Vec2::ZERO, 1)]
            .into_iter()
            .map(|(g, along)| grow(f, &owner, seed, g, along))
            .reduce(|best, r| if r.1.len() > best.1.len() { r } else { best })
            .expect("candidates");
        let id = rects.len() as u32;
        for &i in &cells {
            owner[i as usize] = id;
        }
        rects.push((rect, cells));
    }
    let rects = merge(f, &mut owner, rects);
    let links = portals(f, &owner, &rects);
    rects
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let corner = |x: u32, z: u32| {
                let p = f.origin + Vec2::new(x as f32, z as f32) * f.cell;
                Vec3::new(p.x, r.height(p), p.y)
            };
            NavPoly {
                vertices: vec![
                    corner(r.x0, r.z0),
                    corner(r.x0, r.z1),
                    corner(r.x1, r.z1),
                    corner(r.x1, r.z0),
                ],
                links: links.get(&(i as u32)).cloned().unwrap_or_default(),
            }
        })
        .collect()
}

/// Join neighbouring rectangles that share a whole edge and lie on the same plane (the greedy
/// pass leaves strips behind); `owner` is renumbered to the result.
fn merge(f: &CompactField, owner: &mut [u32], mut rects: Vec<(Rect, Vec<u32>)>) -> Vec<Rect> {
    let mut alive = vec![true; rects.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for a in 0..rects.len() {
            if !alive[a] {
                continue;
            }
            for dir in [2usize, 1] {
                let Some(b) = mergeable(f, owner, &rects, a, dir) else {
                    continue;
                };
                let (rb, cells) = std::mem::replace(&mut rects[b], (empty_rect(), Vec::new()));
                for &i in &cells {
                    owner[i as usize] = a as u32;
                }
                let ra = &mut rects[a];
                ra.0.x1 = ra.0.x1.max(rb.x1);
                ra.0.z1 = ra.0.z1.max(rb.z1);
                ra.1.extend(cells);
                alive[b] = false;
                changed = true;
            }
        }
    }
    let mut remap = vec![NONE; rects.len()];
    let mut out = Vec::new();
    for (i, (r, _)) in rects.into_iter().enumerate() {
        if alive[i] {
            remap[i] = out.len() as u32;
            out.push(r);
        }
    }
    for o in owner.iter_mut() {
        if *o != NONE {
            *o = remap[*o as usize];
        }
    }
    out
}

fn empty_rect() -> Rect {
    Rect {
        x0: 0,
        z0: 0,
        x1: 0,
        z1: 0,
        p0: Vec2::ZERO,
        y0: 0.0,
        gradient: Vec2::ZERO,
    }
}

/// The rectangle beyond `a`'s +x (dir 2) or +z (dir 1) edge if the two can become one.
fn mergeable(
    f: &CompactField,
    owner: &[u32],
    rects: &[(Rect, Vec<u32>)],
    a: usize,
    dir: usize,
) -> Option<usize> {
    let (ra, cells) = &rects[a];
    let edge = cells.iter().filter(|&&i| {
        let c = &f.cells[i as usize];
        if dir == 2 {
            c.x + 1 == ra.x1
        } else {
            c.z + 1 == ra.z1
        }
    });
    let mut b = None;
    for &i in edge {
        let n = f.cells[i as usize].links[dir];
        if n == NONE {
            return None;
        }
        let o = owner[n as usize];
        if o == a as u32 || b.is_some_and(|b| b != o) {
            return None;
        }
        b = Some(o);
    }
    let b = b? as usize;
    let rb = &rects[b].0;
    let lined_up = if dir == 2 {
        rb.x0 == ra.x1 && rb.z0 == ra.z0 && rb.z1 == ra.z1
    } else {
        rb.z0 == ra.z1 && rb.x0 == ra.x0 && rb.x1 == ra.x1
    };
    let same_plane = (ra.gradient - rb.gradient).length() < 1e-4
        && (ra.height(rb.p0) - rb.y0).abs() < PLANE_TOLERANCE * 0.25;
    (lined_up && same_plane).then_some(b)
}

/// Shared edge segments between neighbouring rectangles.
fn portals(f: &CompactField, owner: &[u32], rects: &[Rect]) -> HashMap<u32, Vec<NavLink>> {
    // (from, to, direction) → covered range along the edge, in cell units.
    let mut spans: HashMap<(u32, u32, usize), (u32, u32)> = HashMap::new();
    for (i, c) in f.cells.iter().enumerate() {
        let a = owner[i];
        for (d, (&n, dir)) in c.links.iter().zip(DIRS).enumerate() {
            if n == NONE || owner[n as usize] == a {
                continue;
            }
            let along = if dir.0 != 0 { c.z } else { c.x };
            let e = spans
                .entry((a, owner[n as usize], d))
                .or_insert((along, along + 1));
            e.0 = e.0.min(along);
            e.1 = e.1.max(along + 1);
        }
    }
    let mut keys: Vec<_> = spans.keys().copied().collect();
    keys.sort();
    let mut out: HashMap<u32, Vec<NavLink>> = HashMap::new();
    for key in keys {
        let (from, to, d) = key;
        let (lo, hi) = spans[&key];
        let r = &rects[from as usize];
        let point = |x: u32, z: u32| {
            let p = f.origin + Vec2::new(x as f32, z as f32) * f.cell;
            Vec3::new(p.x, r.height(p), p.y)
        };
        let (a, b) = match d {
            0 => (point(r.x0, lo), point(r.x0, hi)),
            2 => (point(r.x1, lo), point(r.x1, hi)),
            1 => (point(lo, r.z1), point(hi, r.z1)),
            _ => (point(lo, r.z0), point(hi, r.z0)),
        };
        out.entry(from).or_default().push(NavLink { to, a, b });
    }
    out
}
