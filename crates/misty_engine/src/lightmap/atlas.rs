//! Renderers' lightmap rectangles packed into lightmaps.
//!
//! Each renderer gets a square of `side` texels (its uv2 square, sized for the requested texel
//! density). Squares are shelf-packed tallest first with `padding` texels between them; the
//! smallest power-of-two lightmap that holds everything is used, else as many `max_size`
//! lightmaps as needed. Every lightmap of a bake has the same size (one texture array).

/// Where one renderer landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub lightmap: usize,
    /// Texel corner and side of the square.
    pub x: u32,
    pub y: u32,
    pub side: u32,
}

impl Placement {
    /// uv2 × xy + zw = lightmap uv (lightmaps are `size` texels square).
    pub fn scale_offset(&self, size: u32) -> [f32; 4] {
        let s = size as f32;
        let k = self.side as f32 / s;
        [k, k, self.x as f32 / s, self.y as f32 / s]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Atlas {
    /// Side of every lightmap.
    pub size: u32,
    pub count: usize,
    /// Same order as the input sides.
    pub placements: Vec<Placement>,
}

/// Texels per side of a renderer: `resolution` texels per unit over its surface, given its
/// world surface area and the area its triangles cover in uv2 (0–1).
pub fn renderer_side(world_area: f32, uv_area: f32, resolution: f32, max_size: u32) -> u32 {
    let side = resolution * (world_area / uv_area.max(1e-4)).sqrt();
    (side.ceil() as u32).clamp(8, max_size.max(8))
}

/// Pack squares of `sides` texels.
pub fn pack(sides: &[u32], max_size: u32, padding: u32) -> Atlas {
    let max_size = max_size.max(8);
    let sides: Vec<u32> = sides
        .iter()
        .map(|s| (*s).min(max_size - 2 * padding.min(max_size / 4)))
        .collect();
    let mut size = 32u32;
    loop {
        if size >= max_size {
            return place(&sides, max_size, padding);
        }
        let a = place(&sides, size, padding);
        if a.count <= 1 {
            return a;
        }
        size *= 2;
    }
}

/// Shelf placement into as many `size` lightmaps as needed.
fn place(sides: &[u32], size: u32, padding: u32) -> Atlas {
    let mut order: Vec<usize> = (0..sides.len()).collect();
    order.sort_by(|a, b| sides[*b].cmp(&sides[*a]).then(a.cmp(b)));
    let mut placements = vec![
        Placement {
            lightmap: 0,
            x: 0,
            y: 0,
            side: 0,
        };
        sides.len()
    ];
    let (mut map, mut x, mut y, mut row) = (0usize, padding, padding, 0u32);
    for i in order {
        let s = sides[i].min(size - 2 * padding);
        if x + s + padding > size {
            x = padding;
            y += row + padding;
            row = 0;
        }
        if y + s + padding > size {
            map += 1;
            x = padding;
            y = padding;
            row = 0;
        }
        placements[i] = Placement {
            lightmap: map,
            x,
            y,
            side: s,
        };
        x += s + padding;
        row = row.max(s);
    }
    Atlas {
        size,
        count: if sides.is_empty() { 0 } else { map + 1 },
        placements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squares_never_overlap_and_keep_padding() {
        let sides = [40, 33, 8, 8, 64, 17, 90, 12, 12, 12];
        for (max, pad) in [(128, 2), (256, 3), (1024, 2)] {
            let a = pack(&sides, max, pad);
            assert!(a.size <= max && a.size.is_power_of_two());
            for (i, p) in a.placements.iter().enumerate() {
                assert!(p.x >= pad && p.y >= pad && p.x + p.side + pad <= a.size);
                assert!(p.y + p.side + pad <= a.size);
                for q in &a.placements[i + 1..] {
                    if p.lightmap != q.lightmap {
                        continue;
                    }
                    let apart = p.x + p.side + pad <= q.x
                        || q.x + q.side + pad <= p.x
                        || p.y + p.side + pad <= q.y
                        || q.y + q.side + pad <= p.y;
                    assert!(apart, "{p:?} {q:?}");
                }
            }
        }
        // Everything fits one 256 map; with a 128 limit it takes several.
        assert_eq!(pack(&sides, 1024, 2).count, 1);
        assert!(pack(&sides, 128, 2).count > 1);
        let st = Placement {
            lightmap: 0,
            x: 16,
            y: 32,
            side: 64,
        }
        .scale_offset(128);
        assert_eq!(st, [0.5, 0.5, 0.125, 0.25]);
    }

    #[test]
    fn side_follows_texel_density() {
        // A 4 × 4 m surface covering half the uv square at 10 texels / m: 40 / √0.5.
        assert_eq!(renderer_side(16.0, 0.5, 10.0, 1024), 57);
        assert_eq!(renderer_side(0.0001, 1.0, 10.0, 1024), 8);
        assert_eq!(renderer_side(1e6, 1.0, 10.0, 512), 512);
    }
}
