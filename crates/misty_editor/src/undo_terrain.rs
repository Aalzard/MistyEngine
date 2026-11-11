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
        // The asset may have changed size outside the history (reloaded from disk after a
        // Split into tiles, a checkout…): a tile patch is then left alone above, while the
        // settings (the resolutions, the other maps) still swap. Make the lengths agree
        // again so nothing indexes past a map.
        data.sanitize();
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

    /// Approximate memory held by this step (the settings keep the other maps whole).
    pub fn bytes(&self) -> usize {
        let s = &self.settings;
        let whole = s.alphamap_extra.len() * 4
            + s.holes.len()
            + s.flowers.map.len()
            + s.border.len() * 4
            + s.detail_layers.iter().map(|l| l.map.len()).sum::<usize>()
            + s.trees.instances.len() * size_of::<misty_engine::terrain::trees::TreeInstance>();
        self.heights.bytes() + self.alphamap.bytes() + self.detail.bytes() + whole + 4096
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

    /// The asset was reloaded at another resolution after the step was taken: the restore
    /// keeps every map the length its resolution says (nothing indexes past the end later).
    #[test]
    fn restore_after_an_outside_resize_keeps_the_maps_consistent() {
        use glam::Vec3;
        let dir = std::env::temp_dir().join(format!("misty_undo_resize_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("R.terrain").to_string_lossy().into_owned();
        let before = TerrainData::new(129, Vec3::new(100.0, 20.0, 100.0), 64);
        store::insert(&path, before.clone());
        store::modify(&path, |d| d.heights[64 * 129 + 64] = 0.5);
        let step = TerrainUndo::new(TerrainSnapshot {
            path: path.clone(),
            data: before,
        });
        // Replaced on disk and reloaded at 65 samples with a 32 splat map.
        store::insert(
            &path,
            TerrainData::new(65, Vec3::new(100.0, 20.0, 100.0), 32),
        );
        let redo = step.restore();
        let d = store::get(&path).unwrap().0;
        let r = d.heightmap_resolution;
        assert_eq!(d.heights.len(), r * r);
        let a = d.alphamap_resolution;
        assert_eq!(d.alphamap.len(), a * a);
        // Sampling and the mesh use the maps through the resolution: no panic.
        let _ = d.sample_height(50.0, 50.0);
        let _ = d.weights_at(50.0, 50.0);
        let _ = misty_engine::terrain::mesh::build(&d);
        redo.restore();
        let d = store::get(&path).unwrap().0;
        assert_eq!(
            d.heights.len(),
            d.heightmap_resolution * d.heightmap_resolution
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Paint Details, holes and flower masks live in the settings copy; the step's size
    /// counts them, so the undo memory cap sees them.
    #[test]
    fn bytes_count_the_maps_kept_whole() {
        use glam::Vec3;
        let dir = std::env::temp_dir().join(format!("misty_undo_bytes_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("B.terrain").to_string_lossy().into_owned();
        let mut before = TerrainData::new(33, Vec3::new(100.0, 20.0, 100.0), 256);
        before
            .detail_layers
            .push(misty_engine::terrain::detail_layers::DetailLayer::grass(
                "Tall",
            ));
        before.detail_layers[0].map = vec![0; 256 * 256];
        store::insert(&path, before.clone());
        store::modify(&path, |d| d.detail_layers[0].map[0] = 9);
        let step = TerrainUndo::new(TerrainSnapshot {
            path: path.clone(),
            data: before,
        });
        assert!(step.bytes() >= 256 * 256, "{} bytes", step.bytes());
        let _ = std::fs::remove_dir_all(dir);
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
