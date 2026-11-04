//! The committed LTC table (`ltc_ggx.bin`, written by [`build`] through the ignored test
//! `write_ltc_table`) and lookups into it.
//!
//! Layout: `SIZE²` cells of the inverse matrix [a, b, d, e], then `SIZE²` cells of
//! [scale, bias, 0, 0], each cell four little-endian f32s, row = view angle, column = roughness.
//! The GPU reads the same bytes as a storage buffer (`area_light.wgsl`).

use std::sync::OnceLock;

use rayon::prelude::*;

use super::ltc_fit::{self, Cell, Ltc, SIZE};

/// File name next to this module.
pub const FILE: &str = "ltc_ggx.bin";
static BYTES: &[u8] = include_bytes!("ltc_ggx.bin");

/// Fit every cell (columns in parallel). Takes a while: run it in release.
pub fn build() -> Vec<Cell> {
    let columns: Vec<Vec<Cell>> = (0..SIZE).into_par_iter().map(ltc_fit::fit_column).collect();
    // Row-major by view angle.
    (0..SIZE * SIZE)
        .map(|k| columns[k % SIZE][k / SIZE])
        .collect()
}

/// The table as the bytes of `ltc_ggx.bin`.
pub fn encode(cells: &[Cell]) -> Vec<u8> {
    let matrices = cells.iter().map(|c| {
        let l = c.ltc;
        [l.a, l.b, l.d, l.e]
    });
    let magnitudes = cells.iter().map(|c| [c.scale, c.bias, 0.0, 0.0]);
    matrices
        .chain(magnitudes)
        .flatten()
        .flat_map(|v| (v as f32).to_le_bytes())
        .collect()
}

/// The committed table as vec4s (what the GPU buffer holds).
pub fn texels() -> &'static [[f32; 4]] {
    static TEXELS: OnceLock<Vec<[f32; 4]>> = OnceLock::new();
    TEXELS.get_or_init(|| {
        let floats: Vec<f32> = BYTES
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        floats
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect()
    })
}

/// Cell `(i, j)` (roughness column, view row) of the committed table.
pub fn cell(i: usize, j: usize) -> Cell {
    let t = texels();
    let m = t[j * SIZE + i];
    let s = t[SIZE * SIZE + j * SIZE + i];
    Cell {
        ltc: Ltc {
            a: m[0] as f64,
            b: m[1] as f64,
            d: m[2] as f64,
            e: m[3] as f64,
        },
        scale: s[0] as f64,
        bias: s[1] as f64,
    }
}

/// Bilinear lookup of `base` (0 = matrices, SIZE² = magnitudes) at perceptual roughness and
/// √(1 - n·v), both 0..1. CPU copy of `ltc_fetch` in area_light.wgsl.
pub fn fetch(base: usize, roughness: f32, t: f32) -> [f32; 4] {
    let n = (SIZE - 1) as f32;
    let x = roughness.clamp(0.0, 1.0) * n;
    let y = t.clamp(0.0, 1.0) * n;
    let (x0, y0) = (x.floor().min(n - 1.0), y.floor().min(n - 1.0));
    let (fx, fy) = (x - x0, y - y0);
    let at = |i: f32, j: f32| texels()[base + j as usize * SIZE + i as usize];
    let lerp = |a: [f32; 4], b: [f32; 4], t: f32| std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
    let top = lerp(at(x0, y0), at(x0 + 1.0, y0), fx);
    let bottom = lerp(at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0), fx);
    lerp(top, bottom, fy)
}
