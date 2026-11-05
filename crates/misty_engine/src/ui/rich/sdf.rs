//! Signed distance field glyph atlas: each glyph is rendered once at [`BASE`] pixels per em
//! (supersampled, then an exact Euclidean distance transform) and stored as distances, so one
//! atlas entry draws crisply at any size and gives outlines, soft shadows and bold for free.

use std::collections::HashMap;

use glam::Vec2;

use super::fonts::{FontId, LoadedFont};

/// Em size glyphs are stored at.
pub const BASE: f32 = 42.0;
/// Distance (pixels at [`BASE`]) from the outline to the ends of the 0–1 range.
pub const SPREAD: f32 = 8.0;
/// Supersampling of the coverage the distances are measured on.
const SS: u32 = 4;

/// Where a glyph sits in the atlas; offsets and sizes are pixels at [`BASE`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfGlyph {
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    /// Top-left of the (padded) bitmap relative to the pen position on the baseline.
    pub offset: Vec2,
    pub size: Vec2,
}

/// "Far away" for the distance transform (larger than any squared distance in a glyph).
const FAR: f64 = 1e12;

/// Squared distance transform of one row or column (Felzenszwalb & Huttenlocher).
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let fq = f[q] + (q * q) as f64;
        let mut s;
        loop {
            let p = v[k];
            s = (fq - (f[p] + (p * p) as f64)) / (2.0 * (q - p) as f64);
            if s <= z[k] {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let d = q as f64 - v[k] as f64;
        *o = d * d + f[v[k]];
    }
}

/// Squared distance from every cell to the nearest cell where `grid` is 0 (others: [`FAR`]).
fn edt_2d(grid: &mut [f64], w: usize, h: usize) {
    let n = w.max(h);
    let (mut f, mut out, mut v, mut z) = (
        vec![0.0; n],
        vec![0.0; n],
        vec![0usize; n],
        vec![0.0; n + 1],
    );
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..y * w + w]);
        edt_1d(&f[..w], &mut out[..w], &mut v, &mut z);
        grid[y * w..y * w + w].copy_from_slice(&out[..w]);
    }
}

/// The distance field of one glyph: (width, height, offset at BASE, 0–255 values where 128 is
/// the outline and larger is inside). `None` for blank glyphs.
pub fn glyph_field(font: &LoadedFont, glyph: u16) -> Option<(u32, u32, Vec2, Vec<u8>)> {
    let (cw, ch, off, cov) = font.coverage(glyph, BASE * SS as f32)?;
    let pad = (SPREAD as u32 + 1) * SS;
    let (w, h) = ((cw + 2 * pad) as usize, (ch + 2 * pad) as usize);
    let inside = |x: usize, y: usize| -> bool {
        let (cx, cy) = (x as i64 - pad as i64, y as i64 - pad as i64);
        if cx < 0 || cy < 0 || cx >= cw as i64 || cy >= ch as i64 {
            return false;
        }
        cov[(cy as usize) * cw as usize + cx as usize] >= 0.5
    };
    let mut to_in = vec![0.0f64; w * h];
    let mut to_out = vec![0.0f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = inside(x, y);
            to_in[y * w + x] = if i { 0.0 } else { FAR };
            to_out[y * w + x] = if i { FAR } else { 0.0 };
        }
    }
    edt_2d(&mut to_in, w, h);
    edt_2d(&mut to_out, w, h);
    let (lw, lh) = (w.div_ceil(SS as usize), h.div_ceil(SS as usize));
    let mut data = vec![0u8; lw * lh];
    for ly in 0..lh {
        for lx in 0..lw {
            // Average the signed distance over the cell (smoother than its centre alone).
            let mut sum = 0.0f64;
            let mut n = 0.0f64;
            for sy in 0..SS as usize {
                for sx in 0..SS as usize {
                    let (x, y) = (lx * SS as usize + sx, ly * SS as usize + sy);
                    if x >= w || y >= h {
                        continue;
                    }
                    let i = y * w + x;
                    // Pixel centres sit half a pixel from the edge between them.
                    let d = if to_in[i] > 0.0 {
                        to_in[i].sqrt() - 0.5
                    } else {
                        -(to_out[i].sqrt() - 0.5)
                    };
                    sum += d;
                    n += 1.0;
                }
            }
            let d = (sum / n.max(1.0) / SS as f64) as f32;
            let v = (0.5 - d / (2.0 * SPREAD)).clamp(0.0, 1.0);
            data[ly * lw + lx] = (v * 255.0).round() as u8;
        }
    }
    let offset = (Vec2::from(off) - Vec2::splat(pad as f32)) / SS as f32;
    Some((lw as u32, lh as u32, offset, data))
}

/// Single-channel distance atlas filled shelf by shelf.
pub struct SdfAtlas {
    pub size: u32,
    pub pixels: Vec<u8>,
    /// Bumped whenever `pixels` changes.
    pub version: u64,
    entries: HashMap<(FontId, u16), Option<SdfGlyph>>,
    cursor: (u32, u32),
    shelf_h: u32,
}

/// No room left; clear the atlas and try again.
#[derive(Debug, PartialEq, Eq)]
pub struct SdfFull;

impl SdfAtlas {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            pixels: vec![0; (size * size) as usize],
            version: 1,
            entries: HashMap::new(),
            cursor: (1, 1),
            shelf_h: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self {
            version: self.version + 1,
            ..Self::new(self.size)
        };
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for a glyph, generating its field on first use (`Ok(None)` = blank glyph).
    pub fn glyph(&mut self, font: &LoadedFont, glyph: u16) -> Result<Option<SdfGlyph>, SdfFull> {
        if let Some(e) = self.entries.get(&(font.id, glyph)) {
            return Ok(*e);
        }
        let entry = match glyph_field(font, glyph) {
            None => None,
            Some((w, h, offset, data)) => Some(self.insert(w, h, offset, &data)?),
        };
        self.entries.insert((font.id, glyph), entry);
        Ok(entry)
    }

    fn insert(&mut self, w: u32, h: u32, offset: Vec2, data: &[u8]) -> Result<SdfGlyph, SdfFull> {
        const PAD: u32 = 1;
        if w + 2 * PAD > self.size {
            return Err(SdfFull);
        }
        if self.cursor.0 + w + PAD > self.size {
            self.cursor = (PAD, self.cursor.1 + self.shelf_h + PAD);
            self.shelf_h = 0;
        }
        if self.cursor.1 + h + PAD > self.size {
            return Err(SdfFull);
        }
        let (x0, y0) = self.cursor;
        for y in 0..h {
            let row = ((y0 + y) * self.size + x0) as usize;
            let src = (y * w) as usize;
            self.pixels[row..row + w as usize].copy_from_slice(&data[src..src + w as usize]);
        }
        self.cursor.0 += w + PAD;
        self.shelf_h = self.shelf_h.max(h);
        self.version += 1;
        let s = self.size as f32;
        Ok(SdfGlyph {
            uv_min: Vec2::new(x0 as f32 / s, y0 as f32 / s),
            uv_max: Vec2::new((x0 + w) as f32 / s, (y0 + h) as f32 / s),
            offset,
            size: Vec2::new(w as f32, h as f32),
        })
    }
}

/// Distance threshold (0–1 value of the drawn edge) after growing the glyph by `grow` pixels
/// on screen at em size `px` (negative shrinks).
pub fn threshold(grow: f32, px: f32) -> f32 {
    let base_px = grow * BASE / px.max(0.01);
    (0.5 - base_px / (2.0 * SPREAD)).clamp(0.02, 0.98)
}

/// Extra edge width (0–1 units) for a blur of `px_blur` screen pixels at em size `px`.
pub fn softness(px_blur: f32, px: f32) -> f32 {
    (px_blur * BASE / px.max(0.01) / (2.0 * SPREAD)).clamp(0.0, 0.5)
}
