//! Baked occlusion data: the view cell grid, the occludees' boxes and one visibility bit set per
//! distinct cell result, saved in a small binary file (little endian):
//!
//! ```text
//! "MOCC" version:u32
//! smallest_occluder smallest_hole backface_threshold : f32
//! origin:3×f32 cell_size:f32 dims:3×u32
//! occludees:u32 then min:3×f32 max:3×f32 each
//! sets:u32 words:u32 then sets × words u64
//! cells: dims.x·dims.y·dims.z u32 (set index, OUTSIDE or INVALID), x fastest, then z, then y
//! ```

use std::path::Path;

use glam::{UVec3, Vec3};

use crate::render::culling::Aabb;

const MAGIC: &[u8; 4] = b"MOCC";
const VERSION: u32 = 1;

/// Cell not inside any view volume: no culling there.
pub const OUTSIDE: u32 = u32::MAX;
/// Every sample point of the cell is inside geometry: no culling there either.
pub const INVALID: u32 = u32::MAX - 1;

/// Where a point falls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellLookup {
    /// Outside the grid or outside every view volume.
    Outside,
    /// In a cell whose sample points were all inside geometry.
    Invalid { cell: usize },
    /// In a baked cell; `set` indexes the visibility sets.
    Cell { cell: usize, set: u32 },
}

impl CellLookup {
    pub fn cell(&self) -> Option<usize> {
        match *self {
            CellLookup::Outside => None,
            CellLookup::Invalid { cell } | CellLookup::Cell { cell, .. } => Some(cell),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OcclusionData {
    /// Bake parameters (smallest occluder, smallest hole, backface threshold).
    pub params: [f32; 3],
    pub origin: Vec3,
    pub cell_size: f32,
    /// Cells along x, y, z.
    pub dims: UVec3,
    /// World boxes of the Occludee Static renderers, in Hierarchy order.
    pub occludees: Vec<Aabb>,
    /// u64 words per set.
    pub words: u32,
    /// Every distinct set, `words` each.
    pub sets: Vec<u64>,
    /// Per cell: set index, [`OUTSIDE`] or [`INVALID`].
    pub cells: Vec<u32>,
}

/// Words a set of `n` bits needs.
pub fn words_for(n: usize) -> u32 {
    n.div_ceil(64) as u32
}

impl OcclusionData {
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn set_count(&self) -> usize {
        match self.words {
            0 => usize::from(!self.cells.is_empty()),
            w => self.sets.len() / w as usize,
        }
    }

    /// Index of the cell at grid coordinates `c`.
    pub fn index(&self, c: UVec3) -> usize {
        (c.x + self.dims.x * (c.z + self.dims.z * c.y)) as usize
    }

    /// Grid coordinates of cell `i`.
    pub fn coords(&self, i: usize) -> UVec3 {
        let i = i as u32;
        let (nx, nz) = (self.dims.x.max(1), self.dims.z.max(1));
        UVec3::new(i % nx, i / (nx * nz), (i / nx) % nz)
    }

    /// World box of cell `i`.
    pub fn cell_bounds(&self, i: usize) -> Aabb {
        let min = self.origin + self.coords(i).as_vec3() * self.cell_size;
        Aabb::new(min, min + Vec3::splat(self.cell_size))
    }

    /// The cell containing `p`.
    pub fn lookup(&self, p: Vec3) -> CellLookup {
        if self.cell_size <= 0.0 || !p.is_finite() {
            return CellLookup::Outside;
        }
        let g = ((p - self.origin) / self.cell_size).floor();
        if g.min_element() < 0.0 {
            return CellLookup::Outside;
        }
        let c = g.as_uvec3();
        if c.x >= self.dims.x || c.y >= self.dims.y || c.z >= self.dims.z {
            return CellLookup::Outside;
        }
        let cell = self.index(c);
        match self.cells.get(cell).copied() {
            None | Some(OUTSIDE) => CellLookup::Outside,
            Some(INVALID) => CellLookup::Invalid { cell },
            Some(set) => CellLookup::Cell { cell, set },
        }
    }

    /// Whether occludee `i` is potentially visible in set `set`.
    pub fn visible(&self, set: u32, i: usize) -> bool {
        let w = self.words as usize;
        let at = set as usize * w + i / 64;
        self.sets
            .get(at)
            .is_some_and(|word| word >> (i % 64) & 1 == 1)
    }

    /// Occludees potentially visible in set `set`.
    pub fn visible_count(&self, set: u32) -> usize {
        let w = self.words as usize;
        let start = set as usize * w;
        self.sets
            .get(start..start + w)
            .map_or(0, |s| s.iter().map(|x| x.count_ones() as usize).sum())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + self.sets.len() * 8 + self.cells.len() * 4);
        b.extend_from_slice(MAGIC);
        let u32s = |b: &mut Vec<u8>, v: &[u32]| v.iter().for_each(|x| b.extend(x.to_le_bytes()));
        let f32s = |b: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| b.extend(x.to_le_bytes()));
        u32s(&mut b, &[VERSION]);
        f32s(&mut b, &self.params);
        f32s(&mut b, &self.origin.to_array());
        f32s(&mut b, &[self.cell_size]);
        u32s(&mut b, &self.dims.to_array());
        u32s(&mut b, &[self.occludees.len() as u32]);
        for o in &self.occludees {
            f32s(&mut b, &o.min.to_array());
            f32s(&mut b, &o.max.to_array());
        }
        u32s(&mut b, &[self.set_count() as u32, self.words]);
        for w in &self.sets {
            b.extend(w.to_le_bytes());
        }
        u32s(&mut b, &self.cells);
        b
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader { b: bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return Err("not an occlusion data file".into());
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(format!("unsupported occlusion data version {version}"));
        }
        let params = [r.f32()?, r.f32()?, r.f32()?];
        let origin = r.vec3()?;
        let cell_size = r.f32()?;
        let dims = UVec3::new(r.u32()?, r.u32()?, r.u32()?);
        let n = r.u32()? as usize;
        let mut occludees = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            occludees.push(Aabb::new(r.vec3()?, r.vec3()?));
        }
        let (sets, words) = (r.u32()? as usize, r.u32()?);
        if words != words_for(n) {
            return Err("occlusion data: set size doesn't match the occludees".into());
        }
        let total = sets
            .checked_mul(words as usize)
            .ok_or("occlusion data: too many sets")?;
        let mut set_words = Vec::with_capacity(total.min(1 << 24));
        for _ in 0..total {
            set_words.push(u64::from_le_bytes(r.take(8)?.try_into().expect("8 bytes")));
        }
        let cell_count = (dims.x as usize)
            .checked_mul(dims.y as usize)
            .and_then(|v| v.checked_mul(dims.z as usize))
            .ok_or("occlusion data: grid too large")?;
        let mut cells = Vec::with_capacity(cell_count.min(1 << 24));
        for _ in 0..cell_count {
            let c = r.u32()?;
            if c != OUTSIDE && c != INVALID && c as usize >= sets.max(1) {
                return Err("occlusion data: cell names a missing set".into());
            }
            cells.push(c);
        }
        if r.at != bytes.len() {
            return Err("occlusion data: trailing bytes".into());
        }
        Ok(Self {
            params,
            origin,
            cell_size,
            dims,
            occludees,
            words,
            sets: set_words,
            cells,
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_bytes(&bytes)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at + n;
        let s = self
            .b
            .get(self.at..end)
            .ok_or("occlusion data: file is truncated")?;
        self.at = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }

    fn vec3(&mut self) -> Result<Vec3, String> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
}
