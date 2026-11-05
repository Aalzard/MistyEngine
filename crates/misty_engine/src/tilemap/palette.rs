//! Tile Palettes (`.palette`, JSON): a grid of tile assets the Tile Palette window paints from.

use std::path::Path;

use glam::IVec2;
use serde::{Deserialize, Serialize};

/// File extension of palette assets.
pub const EXTENSION: &str = "palette";

/// Columns used when tiles are added without a position.
pub const COLUMNS: i32 = 8;

pub fn is_palette(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// One swatch: a tile at a palette cell (y grows downwards, like reading order).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteEntry {
    pub x: i32,
    pub y: i32,
    pub tile: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TilePalette {
    pub tiles: Vec<PaletteEntry>,
}

impl TilePalette {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&crate::asset_db::resolve_json(&text)).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, crate::asset_db::embed_json(&text)).map_err(|e| e.to_string())
    }

    pub fn get(&self, c: IVec2) -> Option<&str> {
        self.tiles
            .iter()
            .find(|t| t.x == c.x && t.y == c.y)
            .map(|t| t.tile.as_str())
    }

    /// Put a tile at a cell (None = clear it).
    pub fn set(&mut self, c: IVec2, tile: Option<&str>) {
        self.tiles.retain(|t| t.x != c.x || t.y != c.y);
        if let Some(tile) = tile {
            self.tiles.push(PaletteEntry {
                x: c.x,
                y: c.y,
                tile: tile.to_owned(),
            });
            self.tiles.sort_by_key(|t| (t.y, t.x));
        }
    }

    /// The first free cell in reading order (rows of [`COLUMNS`]).
    pub fn next_free(&self) -> IVec2 {
        (0..)
            .map(|i| IVec2::new(i % COLUMNS, i / COLUMNS))
            .find(|c| self.get(*c).is_none())
            .unwrap_or_default()
    }

    /// Add a tile at the next free cell (no-op when it is already in the palette); returns its
    /// cell.
    pub fn add(&mut self, tile: &str) -> IVec2 {
        if let Some(t) = self.tiles.iter().find(|t| t.tile == tile) {
            return IVec2::new(t.x, t.y);
        }
        let c = self.next_free();
        self.set(c, Some(tile));
        c
    }

    /// Cells used: (columns, rows) of the bounding box from the origin.
    pub fn extent(&self) -> IVec2 {
        self.tiles
            .iter()
            .fold(IVec2::ZERO, |m, t| m.max(IVec2::new(t.x + 1, t.y + 1)))
    }
}
