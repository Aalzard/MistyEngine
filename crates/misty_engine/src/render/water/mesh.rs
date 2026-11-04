//! Water grids. The ocean is a set of nested square rings (a clipmap) centred on the camera:
//! level 0 has the finest cells and each ring doubles the cell size, out to a horizon skirt.
//! Vertices near a ring's outer edge morph onto the next ring's coarser grid, so rings meet
//! without cracks. Lakes and rivers use one uniform unit-square grid scaled per surface.

/// Vertex: local x, local z, cell size, ring half-extent (0 = no morphing).
pub type WaterVertex = [f32; 4];

/// Ocean levels, finest cell size (m) and cells from the centre to a ring's edge.
pub const LEVELS: u32 = 9;
pub const BASE_CELL: f32 = 0.5;
pub const HALF_CELLS: i32 = 32;
/// The horizon skirt reaches this far (m).
pub const HORIZON: f32 = 60_000.0;
/// Cells per side of the lake / river grid.
pub const BOUNDED_CELLS: u32 = 160;

pub struct GridData {
    pub vertices: Vec<WaterVertex>,
    pub indices: Vec<u32>,
}

impl GridData {
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        // a b / c d, clockwise seen from above (front face = Cw with +Y up, +Z forward).
        self.indices.extend_from_slice(&[a, b, c, b, d, c]);
    }
}

/// Outer half-extent of ocean level `level` in metres.
pub fn ring_extent(level: u32) -> f32 {
    BASE_CELL * (1u32 << level) as f32 * HALF_CELLS as f32
}

/// The camera-centred ocean clipmap.
pub fn ocean() -> GridData {
    let mut g = GridData {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    let n = HALF_CELLS;
    let side = (2 * n + 1) as u32;
    for level in 0..LEVELS {
        let cell = BASE_CELL * (1u32 << level) as f32;
        let extent = ring_extent(level);
        let first = g.vertices.len() as u32;
        for z in -n..=n {
            for x in -n..=n {
                g.vertices
                    .push([x as f32 * cell, z as f32 * cell, cell, extent]);
            }
        }
        let at = |x: i32, z: i32| first + ((z + n) as u32) * side + (x + n) as u32;
        let hole = if level == 0 { 0 } else { n / 2 };
        for z in -n..n {
            for x in -n..n {
                // Rings leave out the area the finer level covers.
                if level > 0 && x >= -hole && x < hole && z >= -hole && z < hole {
                    continue;
                }
                g.quad(at(x, z + 1), at(x + 1, z + 1), at(x, z), at(x + 1, z));
            }
        }
    }
    // Horizon skirt: from the last ring out to the horizon, 4 segments per side.
    let inner = ring_extent(LEVELS - 1);
    let steps = 4i32;
    let mut ring = |r: f32| {
        let start = g.vertices.len() as u32;
        for i in 0..steps * 4 {
            // Walk the square's perimeter clockwise from (-r, -r).
            let (side, t) = (i / steps, (i % steps) as f32 / steps as f32);
            let s = -r + 2.0 * r * t;
            let (x, z) = match side {
                0 => (s, -r),
                1 => (r, s),
                2 => (-s, r),
                _ => (-r, -s),
            };
            g.vertices.push([x, z, r / steps as f32, 0.0]);
        }
        start
    };
    let a = ring(inner);
    let b = ring(HORIZON);
    let count = (steps * 4) as u32;
    for i in 0..count {
        let j = (i + 1) % count;
        g.indices
            .extend_from_slice(&[a + i, a + j, b + i, a + j, b + j, b + i]);
    }
    g
}

/// The unit square grid (-0.5..0.5) of lakes and rivers.
pub fn bounded() -> GridData {
    let n = BOUNDED_CELLS;
    let mut g = GridData {
        vertices: Vec::with_capacity(((n + 1) * (n + 1)) as usize),
        indices: Vec::new(),
    };
    let cell = 1.0 / n as f32;
    for z in 0..=n {
        for x in 0..=n {
            g.vertices
                .push([x as f32 * cell - 0.5, z as f32 * cell - 0.5, cell, 0.0]);
        }
    }
    let at = |x: u32, z: u32| z * (n + 1) + x;
    for z in 0..n {
        for x in 0..n {
            g.quad(at(x, z + 1), at(x + 1, z + 1), at(x, z), at(x + 1, z));
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipmap_covers_to_the_horizon_without_overlap() {
        let g = ocean();
        // Level 0: 64² quads; each ring 64² - 32²; the skirt 16 quads.
        let quads = 64 * 64 + (LEVELS as usize - 1) * (64 * 64 - 32 * 32) + 16;
        assert_eq!(g.indices.len(), quads * 6);
        assert!(g.indices.iter().all(|&i| (i as usize) < g.vertices.len()));
        // Total area (each triangle once) matches the skirt's outer square.
        let area: f64 = g
            .indices
            .chunks(3)
            .map(|t| {
                let p = |i: u32| {
                    let v = g.vertices[i as usize];
                    (v[0] as f64, v[1] as f64)
                };
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() * 0.5
            })
            .sum();
        let full = (2.0 * HORIZON as f64).powi(2);
        assert!((area - full).abs() / full < 1e-6, "{area} vs {full}");
        // Triangles face up (clockwise from above, like the other meshes).
        let t = &g.indices[0..3];
        let p = |i: u32| g.vertices[i as usize];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        assert!(cross < 0.0, "clockwise seen from +Y");
        assert_eq!(
            bounded().indices.len(),
            (BOUNDED_CELLS * BOUNDED_CELLS * 6) as usize
        );
    }
}
