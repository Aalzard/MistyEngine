//! Grid slicing for Sprite Mode ▸ Multiple (the Sprite Editor's Slice ▸ Grid By Cell Size).

use image::RgbaImage;

use super::import::SpriteRect;

/// Grid By Cell Size settings, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSlice {
    pub cell: [u32; 2],
    /// Gap before the first cell (from the top-left corner).
    pub offset: [u32; 2],
    /// Gap between cells.
    pub padding: [u32; 2],
    pub pivot: [f32; 2],
    /// Leave out cells whose pixels are all transparent.
    pub skip_empty: bool,
}

impl Default for GridSlice {
    fn default() -> Self {
        Self {
            cell: [16, 16],
            offset: [0, 0],
            padding: [0, 0],
            pivot: [0.5, 0.5],
            skip_empty: true,
        }
    }
}

/// Cut a `width`×`height` image into cells, numbered left to right, top to bottom
/// (`<base>_0`, `<base>_1`, …). `image` (when given) is used to skip empty cells.
pub fn grid(
    width: u32,
    height: u32,
    s: &GridSlice,
    base: &str,
    image: Option<&RgbaImage>,
) -> Vec<SpriteRect> {
    let [cw, ch] = s.cell.map(|c| c.max(1));
    let mut out = Vec::new();
    let mut top = s.offset[1];
    // Rows from the top of the image; rects count y from the bottom.
    while top + ch <= height {
        let mut left = s.offset[0];
        while left + cw <= width {
            let empty = s.skip_empty && image.is_some_and(|img| is_empty(img, left, top, cw, ch));
            if !empty {
                out.push(SpriteRect {
                    name: format!("{base}_{}", out.len()),
                    rect: [
                        left as f32,
                        (height - top - ch) as f32,
                        cw as f32,
                        ch as f32,
                    ],
                    pivot: s.pivot,
                });
            }
            left += cw + s.padding[0];
        }
        top += ch + s.padding[1];
    }
    out
}

/// All pixels of the cell (top-left at `x`, `y`, image rows) are fully transparent.
fn is_empty(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> bool {
    (y..(y + h).min(img.height()))
        .all(|py| (x..(x + w).min(img.width())).all(|px| img.get_pixel(px, py)[3] == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_cells_count_from_the_top_left() {
        let s = GridSlice {
            cell: [16, 16],
            skip_empty: false,
            ..Default::default()
        };
        let rects = grid(48, 32, &s, "Tiles", None);
        assert_eq!(rects.len(), 6);
        assert_eq!(rects[0].name, "Tiles_0");
        // First cell is the top-left one: y = 32 - 16 from the bottom.
        assert_eq!(rects[0].rect, [0.0, 16.0, 16.0, 16.0]);
        assert_eq!(rects[2].rect, [32.0, 16.0, 16.0, 16.0]);
        assert_eq!(rects[3].rect, [0.0, 0.0, 16.0, 16.0]);
    }

    #[test]
    fn offset_padding_and_empty_cells() {
        let s = GridSlice {
            cell: [4, 4],
            offset: [1, 1],
            padding: [2, 2],
            ..Default::default()
        };
        // 1 + 4 + 2 + 4 = 11 fits twice in 12 pixels.
        assert_eq!(grid(12, 12, &s, "a", None).len(), 4);
        let mut img = RgbaImage::new(8, 4);
        img.get_pixel_mut(5, 1)[3] = 255;
        let s = GridSlice {
            cell: [4, 4],
            ..Default::default()
        };
        let rects = grid(8, 4, &s, "b", Some(&img));
        assert_eq!(rects.len(), 1, "the transparent left cell is skipped");
        assert_eq!(rects[0].rect[0], 4.0);
        assert_eq!(rects[0].name, "b_0");
    }
}
