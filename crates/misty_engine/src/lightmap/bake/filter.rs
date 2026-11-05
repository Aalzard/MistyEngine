//! Lightmap filters: an edge-aware À-trous denoise of the indirect light and seam dilation.

use glam::Vec3;

use super::raster::TexelMap;
use crate::lightmap::LightmapData;

/// B3-spline taps of the À-trous wavelet.
const KERNEL: [f32; 5] = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];

/// The indirect light of `map` after `passes` À-trous passes (steps 1, 2, 4, …). Neighbours
/// count less the further apart they are in the world and the more their normals differ, so
/// charts and renderers that touch in the atlas never blur into each other.
pub fn denoise(map: &TexelMap, passes: u32) -> Vec<Vec3> {
    let size = map.size as i32;
    let mut cur: Vec<Vec3> = map.texels.iter().map(|t| t.indirect).collect();
    for pass in 0..passes {
        let step = 1i32 << pass;
        let mut next = cur.clone();
        for y in 0..size {
            for x in 0..size {
                let i = (y * size + x) as usize;
                let t = &map.texels[i];
                if !t.valid {
                    continue;
                }
                // Positions further apart than ~2 texels (× step) stop counting.
                let reach = (t.size * 2.0 * step as f32).max(1e-4);
                let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
                for (ky, wy) in KERNEL.iter().enumerate() {
                    let sy = y + (ky as i32 - 2) * step;
                    if !(0..size).contains(&sy) {
                        continue;
                    }
                    for (kx, wx) in KERNEL.iter().enumerate() {
                        let sx = x + (kx as i32 - 2) * step;
                        if !(0..size).contains(&sx) {
                            continue;
                        }
                        let j = (sy * size + sx) as usize;
                        let o = &map.texels[j];
                        if !o.valid {
                            continue;
                        }
                        let dn = t.normal.dot(o.normal).max(0.0).powi(16);
                        let dp = (o.position - t.position).length() / reach;
                        let w = wx * wy * dn * (-dp * dp).exp();
                        sum += cur[j] * w;
                        weight += w;
                    }
                }
                if weight > 0.0 {
                    next[i] = sum / weight;
                }
            }
        }
        cur = next;
    }
    cur
}

/// Grow the texels marked in `valid` outward by `steps` texels: each empty texel next to filled
/// ones takes their average. Hides chart seams under bilinear filtering.
pub fn dilate(map: &mut LightmapData, valid: &mut [bool], steps: u32) {
    let (w, h) = (map.width as i32, map.height as i32);
    for _ in 0..steps {
        let mut grown = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if valid[i] {
                    continue;
                }
                let (mut sum, mut n) = (Vec3::ZERO, 0);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (sx, sy) = (x + dx, y + dy);
                        if (dx, dy) == (0, 0) || !(0..w).contains(&sx) || !(0..h).contains(&sy) {
                            continue;
                        }
                        let j = (sy * w + sx) as usize;
                        if valid[j] {
                            sum += map.texels[j];
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    grown.push((i, sum / n as f32));
                }
            }
        }
        if grown.is_empty() {
            break;
        }
        for (i, c) in grown {
            map.texels[i] = c;
            valid[i] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dilation_fills_the_ring_around_a_chart() {
        let mut m = LightmapData::new(8, 8);
        let mut valid = vec![false; 64];
        for y in 3..5 {
            for x in 3..5 {
                m.texels[y * 8 + x] = Vec3::ONE;
                valid[y * 8 + x] = true;
            }
        }
        dilate(&mut m, &mut valid, 2);
        // Two rings filled with the chart's colour, the corners of the map still empty.
        assert_eq!(m.get(1, 1), Vec3::ONE);
        assert_eq!(m.get(5, 2), Vec3::ONE);
        assert_eq!(m.get(0, 0), Vec3::ZERO);
        assert!(!valid[0]);
    }
}
