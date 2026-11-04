//! Compact terrain undo: only the heightmap / splat-map / detail-map tiles a stroke touched.
//!
//! A brush stroke copies the whole terrain once when it starts (transient), then on commit the
//! copy is diffed against the edited data in 32×32 tiles. The undo step keeps just the changed
//! tiles plus the small settings (layers, sizes), so a stroke on a 513² terrain costs kilobytes
//! instead of megabytes. Swapping a patch in returns the patch that undoes the swap (for redo).

use misty_engine::terrain::{TerrainData, store};

use crate::terrain_tools::TerrainSnapshot;

/// Tile side in samples.
const TILE: usize = 32;

/// A rectangle of samples (row by row) at (`x`, `y`) in a square map.
struct Tile<T> {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    values: Vec<T>,
}

/// One map's stored values: changed tiles, or the whole array when its size changed.
enum Patch<T> {
    Tiles { width: usize, tiles: Vec<Tile<T>> },
    Whole(Vec<T>),
}

impl<T: Copy + PartialEq> Patch<T> {
    /// `before`'s values wherever it differs from `after` (`width` samples per row).
    fn diff(before: &[T], after: &[T], width: usize) -> Self {
        if before.len() != after.len() || width == 0 || before.len() != width * width {
            return Patch::Whole(before.to_vec());
        }
        let mut tiles = Vec::new();
        for y in (0..width).step_by(TILE) {
            for x in (0..width).step_by(TILE) {
                let (w, h) = (TILE.min(width - x), TILE.min(width - y));
                let rows = (y..y + h).map(|r| r * width + x..r * width + x + w);
                if rows.clone().any(|r| before[r.clone()] != after[r]) {
                    let values = rows.flat_map(|r| before[r].iter().copied()).collect();
                    tiles.push(Tile { x, y, w, h, values });
                }
            }
        }
        Patch::Tiles { width, tiles }
    }

    /// Write the stored values into `data`; returns the values they replaced.
    fn swap(self, data: &mut Vec<T>) -> Self {
        match self {
            Patch::Whole(v) => Patch::Whole(std::mem::replace(data, v)),
            Patch::Tiles { width, tiles } if data.len() == width * width => {
                let tiles = tiles
                    .into_iter()
                    .map(|mut t| {
                        for row in 0..t.h {
                            let at = (t.y + row) * width + t.x;
                            data[at..at + t.w].swap_with_slice(&mut t.values[row * t.w..][..t.w]);
                        }
                        t
                    })
                    .collect();
                Patch::Tiles { width, tiles }
            }
            // The asset changed size outside the history: leave it alone.
            other => other,
        }
    }

    fn bytes(&self) -> usize {
        let n = match self {
            Patch::Whole(v) => v.len(),
            Patch::Tiles { tiles, .. } => tiles.iter().map(|t| t.values.len()).sum(),
        };
        n * size_of::<T>()
    }
}

/// Terrain data an undo step puts back.
pub struct TerrainUndo {
    path: String,
    /// Everything except the three maps (those are left empty here).
    settings: TerrainData,
    heights: Patch<f32>,
    alphamap: Patch<[u8; 4]>,
    detail: Patch<u8>,
}

impl TerrainUndo {
    /// Diff `before` against the data now in the store (the edit's result).
    pub fn new(before: TerrainSnapshot) -> Self {
        let TerrainSnapshot { path, data: mut b } = before;
        let after = store::get(&path).map(|(d, _)| d);
        let (heights, alphamap, detail) = match &after {
            Some(a) => (
                Patch::diff(&b.heights, &a.heights, b.heightmap_resolution),
                Patch::diff(&b.alphamap, &a.alphamap, b.alphamap_resolution),
                Patch::diff(&b.detail_map, &a.detail_map, b.alphamap_resolution),
            ),
            None => (
                Patch::Whole(std::mem::take(&mut b.heights)),
                Patch::Whole(std::mem::take(&mut b.alphamap)),
                Patch::Whole(std::mem::take(&mut b.detail_map)),
            ),
        };
        b.heights = Vec::new();
        b.alphamap = Vec::new();
        b.detail_map = Vec::new();
        Self {
            path,
            settings: b,
            heights,
            alphamap,
            detail,
        }
    }

    /// Put this data back (and save it); returns the step that undoes the restore.
    pub fn restore(self) -> TerrainUndo {
        let Some((current, _)) = store::get(&self.path) else {
            return self;
        };
        let mut data = (*current).clone();
        let heights = self.heights.swap(&mut data.heights);
        let alphamap = self.alphamap.swap(&mut data.alphamap);
        let detail = self.detail.swap(&mut data.detail_map);
        // Swap the settings, keeping the patched maps in the live data.
        let mut settings = self.settings;
        std::mem::swap(&mut settings.heights, &mut data.heights);
        std::mem::swap(&mut settings.alphamap, &mut data.alphamap);
        std::mem::swap(&mut settings.detail_map, &mut data.detail_map);
        std::mem::swap(&mut settings, &mut data);
        store::insert(&self.path, data);
        let _ = store::save(&self.path);
        TerrainUndo {
            path: self.path,
            settings,
            heights,
            alphamap,
            detail,
        }
    }

    /// Approximate memory held by this step.
    pub fn bytes(&self) -> usize {
        self.heights.bytes() + self.alphamap.bytes() + self.detail.bytes() + 4096
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_keeps_only_changed_tiles_and_swaps_back() {
        let width = 100;
        let before = vec![0.0f32; width * width];
        let mut after = before.clone();
        after[5 * width + 5] = 1.0;
        after[98 * width + 98] = 2.0;
        let patch = Patch::diff(&before, &after, width);
        let Patch::Tiles { tiles, .. } = &patch else {
            panic!("tiles expected")
        };
        assert_eq!(tiles.len(), 2);
        // The edge tile is clipped to the map.
        assert!(tiles.iter().any(|t| t.w == 4 && t.h == 4));
        let mut data = after.clone();
        let redo = patch.swap(&mut data);
        assert_eq!(data, before);
        redo.swap(&mut data);
        assert_eq!(data, after);
    }

    #[test]
    fn resized_map_is_stored_whole() {
        let patch = Patch::diff(&[1u8; 4], &[0u8; 9], 2);
        assert!(matches!(patch, Patch::Whole(ref v) if v.len() == 4));
        let mut data = vec![0u8; 9];
        let back = patch.swap(&mut data);
        assert_eq!(data, vec![1u8; 4]);
        assert!(matches!(back, Patch::Whole(ref v) if v.len() == 9));
    }
}
