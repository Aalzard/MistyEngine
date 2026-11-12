//! 2D tilemaps: a **Grid** object lays out cells; child **Tilemap** objects store one tile asset
//! per cell, drawn by a **Tilemap Renderer** (chunked, batched quads) and optionally collided by a
//! **Tilemap Collider 2D** (adjacent cells merged into fewer boxes).
//!
//! Tiles are `.tile` assets (see [`tile`]): a plain sprite tile, a Rule Tile whose sprite depends
//! on its eight neighbours, or an Animated Tile cycling through frames. Tile Palettes (`.palette`,
//! see [`palette`]) are the painting swatches of the editor's Tile Palette window; [`edit`] holds
//! the brush operations (paint, box fill, erase, flood fill, move).
//!
//! Cells are integer (x, y) coordinates in the Tilemap's XY plane. Cell (0, 0) spans
//! [0, cell size) from the Grid's origin, so its centre is at half a cell.

pub mod chunks;
pub mod collider;
pub mod edit;
pub mod palette;
pub mod tile;

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{IVec2, Mat4, Vec2, Vec3};
use hecs::Entity;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::scene::{Scene, SceneComponent};

pub use tile::{ColliderType, TileAsset, TileKind};

/// How cells are arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridLayout {
    #[default]
    Rectangle,
    /// Diamond cells: x runs right-up, y runs left-up (cell size = the diamond's width/height).
    Isometric,
}

impl GridLayout {
    pub const ALL: [Self; 2] = [Self::Rectangle, Self::Isometric];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Isometric => "Isometric",
        }
    }
}

/// Grid: the cell layout its child Tilemaps use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub cell_size: Vec2,
    /// Space between cells (tiles keep the cell size, so gaps show through).
    pub cell_gap: Vec2,
    pub layout: GridLayout,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            cell_size: Vec2::ONE,
            cell_gap: Vec2::ZERO,
            layout: GridLayout::Rectangle,
        }
    }
}

impl Grid {
    /// Distance from one cell's origin to the next.
    fn pitch(&self) -> Vec2 {
        (self.cell_size + self.cell_gap).max(Vec2::splat(1e-4))
    }

    /// Local position of a cell's centre (in the Tilemap's plane).
    pub fn cell_center(&self, c: IVec2) -> Vec2 {
        let p = self.pitch();
        match self.layout {
            GridLayout::Rectangle => (c.as_vec2() + 0.5) * p - self.cell_gap * 0.5,
            GridLayout::Isometric => {
                let half = p * 0.5;
                Vec2::new(
                    (c.x - c.y) as f32 * half.x,
                    (c.x + c.y) as f32 * half.y + half.y,
                )
            }
        }
    }

    /// Largest cell coordinate a position maps to.
    pub const CELL_LIMIT: i32 = 1 << 20;

    /// The cell containing a local position (within ±[`Self::CELL_LIMIT`] on each axis: a point
    /// far outside a tiny-cell grid would otherwise saturate to ±i32::MAX).
    pub fn local_to_cell(&self, p: Vec2) -> IVec2 {
        let pitch = self.pitch();
        let c = match self.layout {
            GridLayout::Rectangle => (p / pitch).floor().as_ivec2(),
            GridLayout::Isometric => {
                // Undo the diamond transform relative to cell (0,0)'s bottom corner.
                let half = pitch * 0.5;
                let (u, v) = (p.x / half.x, p.y / half.y);
                IVec2::new(
                    ((u + v) * 0.5).floor() as i32,
                    ((v - u) * 0.5).floor() as i32,
                )
            }
        };
        c.clamp(
            IVec2::splat(-Self::CELL_LIMIT),
            IVec2::splat(Self::CELL_LIMIT),
        )
    }
}

/// One painted cell: its tile asset and its rotation / flips.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileCell {
    /// Project path of the `.tile` asset.
    pub tile: Arc<str>,
    pub transform: TileTransform,
}

/// Rotation in quarter turns counter-clockwise (applied after the flips) and flips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TileTransform(pub u8);

impl TileTransform {
    pub const FLIP_X: u8 = 4;
    pub const FLIP_Y: u8 = 8;

    pub fn rotation(self) -> u8 {
        self.0 & 3
    }

    pub fn flip_x(self) -> bool {
        self.0 & Self::FLIP_X != 0
    }

    pub fn flip_y(self) -> bool {
        self.0 & Self::FLIP_Y != 0
    }

    /// A quarter turn counter-clockwise (`[`) or clockwise (`]`, `ccw = false`).
    pub fn rotated(self, ccw: bool) -> Self {
        let r = if ccw {
            (self.rotation() + 1) & 3
        } else {
            (self.rotation() + 3) & 3
        };
        Self((self.0 & !3) | r)
    }

    pub fn flipped_x(self) -> Self {
        Self(self.0 ^ Self::FLIP_X)
    }

    pub fn flipped_y(self) -> Self {
        Self(self.0 ^ Self::FLIP_Y)
    }

    /// Combine with another transform applied afterwards (rule outputs on top of the cell's).
    pub fn then(self, other: Self) -> Self {
        // Apply both to a probe vector pair and read the result back.
        let m = other.matrix() * self.matrix();
        Self::from_matrix(m)
    }

    /// 2×2 matrix (columns) of the transform: flips first, then the rotation.
    pub fn matrix(self) -> glam::Mat2 {
        let fx = if self.flip_x() { -1.0 } else { 1.0 };
        let fy = if self.flip_y() { -1.0 } else { 1.0 };
        let flip = glam::Mat2::from_cols(Vec2::new(fx, 0.0), Vec2::new(0.0, fy));
        let rot = glam::Mat2::from_angle(self.rotation() as f32 * std::f32::consts::FRAC_PI_2);
        rot * flip
    }

    fn from_matrix(m: glam::Mat2) -> Self {
        (0..16u8)
            .map(Self)
            .find(|t| {
                (t.matrix() - m)
                    .abs()
                    .to_cols_array()
                    .iter()
                    .all(|v| *v < 1e-3)
            })
            .unwrap_or_default()
    }
}

/// Every painted cell of a Tilemap, ordered by (x, y).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileData(pub BTreeMap<(i32, i32), TileCell>);

/// Serialised form: the distinct tile paths, then `[x, y, tile index, transform]` per cell.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct TileDataFile {
    palette: Vec<String>,
    cells: Vec<[i32; 4]>,
}

impl Serialize for TileData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut file = TileDataFile::default();
        let mut index: std::collections::HashMap<&str, i32> = Default::default();
        for (&(x, y), c) in &self.0 {
            let i = *index.entry(&c.tile).or_insert_with(|| {
                file.palette.push(c.tile.to_string());
                file.palette.len() as i32 - 1
            });
            file.cells.push([x, y, i, c.transform.0 as i32]);
        }
        file.serialize(s)
    }
}

impl<'de> Deserialize<'de> for TileData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let file = TileDataFile::deserialize(d)?;
        let palette: Vec<Arc<str>> = file.palette.iter().map(|p| Arc::from(p.as_str())).collect();
        let mut map = BTreeMap::new();
        for [x, y, i, t] in file.cells {
            if let Some(tile) = palette.get(i.max(0) as usize) {
                map.insert(
                    (x, y),
                    TileCell {
                        tile: tile.clone(),
                        transform: TileTransform((t & 15) as u8),
                    },
                );
            }
        }
        Ok(Self(map))
    }
}

/// Tilemap: the tiles painted on a Grid's cells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tilemap {
    pub tiles: TileData,
    /// sRGB tint + alpha of every tile.
    pub color: [f32; 4],
    /// Where in a cell a tile's sprite pivot sits (0.5, 0.5 = the centre).
    pub tile_anchor: Vec2,
    /// Speed multiplier of Animated Tiles.
    pub animation_frame_rate: f32,
}

impl Default for Tilemap {
    fn default() -> Self {
        Self {
            tiles: TileData::default(),
            color: [1.0; 4],
            tile_anchor: Vec2::splat(0.5),
            animation_frame_rate: 1.0,
        }
    }
}

impl Tilemap {
    pub fn get(&self, c: IVec2) -> Option<&TileCell> {
        self.tiles.0.get(&(c.x, c.y))
    }

    /// Tile path at a cell.
    pub fn tile_at(&self, c: IVec2) -> Option<&str> {
        self.get(c).map(|t| &*t.tile)
    }

    /// Put `tile` (None = erase) at a cell; true when the cell changed.
    pub fn set(&mut self, c: IVec2, tile: Option<&str>, transform: TileTransform) -> bool {
        let key = (c.x, c.y);
        match tile {
            Some(t) => {
                let cell = TileCell {
                    tile: Arc::from(t),
                    transform,
                };
                self.tiles.0.insert(key, cell.clone()) != Some(cell)
            }
            None => self.tiles.0.remove(&key).is_some(),
        }
    }

    pub fn clear(&mut self) {
        self.tiles.0.clear();
    }

    pub fn len(&self) -> usize {
        self.tiles.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.0.is_empty()
    }

    /// Smallest and largest painted cell (None when empty).
    pub fn bounds(&self) -> Option<(IVec2, IVec2)> {
        let mut it = self.tiles.0.keys();
        let first = it.next()?;
        let mut lo = IVec2::new(first.0, first.1);
        let mut hi = lo;
        for &(x, y) in it {
            lo = lo.min(IVec2::new(x, y));
            hi = hi.max(IVec2::new(x, y));
        }
        Some((lo, hi))
    }

    /// Painted cells inside an inclusive cell rectangle.
    pub fn cells_in(&self, lo: IVec2, hi: IVec2) -> impl Iterator<Item = (IVec2, &TileCell)> {
        self.tiles
            .0
            .range((lo.x, i32::MIN)..=(hi.x, i32::MAX))
            .filter(move |((_, y), _)| *y >= lo.y && *y <= hi.y)
            .map(|(&(x, y), c)| (IVec2::new(x, y), c))
    }
}

/// How a Tilemap Renderer sorts its tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TilemapRenderMode {
    /// Tiles merged into one mesh per 16×16-cell chunk and texture.
    #[default]
    Chunk,
    /// One draw per tile (each sorted on its own).
    Individual,
}

/// Tilemap Renderer: draws the Tilemap on the same object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TilemapRenderer {
    pub enabled: bool,
    pub mode: TilemapRenderMode,
    pub sorting_layer: String,
    pub order_in_layer: i32,
}

impl Default for TilemapRenderer {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: TilemapRenderMode::Chunk,
            sorting_layer: crate::sprites::sorting::DEFAULT.to_owned(),
            order_in_layer: 0,
        }
    }
}

/// Tilemap Collider 2D: a collider per tile whose Collider Type is not None, or (Used By
/// Composite) the cells merged into as few boxes as possible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TilemapCollider2D {
    pub common: crate::scene::Collider2DCommon,
    /// Merge adjacent cells into larger boxes (like a Composite Collider 2D).
    pub used_by_composite: bool,
}

impl Default for TilemapCollider2D {
    fn default() -> Self {
        Self {
            common: Default::default(),
            used_by_composite: true,
        }
    }
}

impl SceneComponent for Grid {
    const NAME: &'static str = "Grid";
    const CATEGORY: &'static str = "Layout";
}
impl SceneComponent for Tilemap {
    const NAME: &'static str = "Tilemap";
    const CATEGORY: &'static str = "Tilemap";
}
impl SceneComponent for TilemapRenderer {
    const NAME: &'static str = "Tilemap Renderer";
    const CATEGORY: &'static str = "Tilemap";
}
impl SceneComponent for TilemapCollider2D {
    const NAME: &'static str = "Tilemap Collider 2D";
    const CATEGORY: &'static str = "Physics 2D";
}

/// The Grid a Tilemap uses: on itself or the closest parent (default cells without one).
pub fn grid_of(scene: &Scene, e: Entity) -> Grid {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if let Ok(g) = scene.world.get::<&Grid>(c) {
            return (*g).clone();
        }
        cur = scene.parent(c);
    }
    Grid::default()
}

/// World position → cell of Tilemap `e` (projected onto its plane).
pub fn world_to_cell(scene: &Scene, e: Entity, world: Vec3) -> IVec2 {
    let local = scene.world_matrix(e).inverse().transform_point3(world);
    grid_of(scene, e).local_to_cell(local.truncate())
}

/// Cell centre of Tilemap `e` in world space.
pub fn cell_to_world(scene: &Scene, e: Entity, c: IVec2) -> Vec3 {
    let local = grid_of(scene, e).cell_center(c);
    scene.world_matrix(e).transform_point3(local.extend(0.0))
}

/// Where a ray hits Tilemap `e`'s plane (None when parallel or behind).
pub fn ray_to_plane(scene: &Scene, e: Entity, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let m: Mat4 = scene.world_matrix(e);
    let p = m.transform_point3(Vec3::ZERO);
    let n = m.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
    let denom = n.dot(dir);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = n.dot(p - origin) / denom;
    (t >= 0.0).then(|| origin + dir * t)
}

/// World-space corners of a cell (bottom-left, bottom-right, top-right, top-left; for gizmos).
pub fn cell_corners(scene: &Scene, e: Entity, c: IVec2) -> [Vec3; 4] {
    let g = grid_of(scene, e);
    let m = scene.world_matrix(e);
    let center = g.cell_center(c);
    let h = g.cell_size * 0.5;
    let local = match g.layout {
        GridLayout::Rectangle => [
            center - h,
            center + Vec2::new(h.x, -h.y),
            center + h,
            center + Vec2::new(-h.x, h.y),
        ],
        GridLayout::Isometric => [
            center - Vec2::new(0.0, h.y),
            center + Vec2::new(h.x, 0.0),
            center + Vec2::new(0.0, h.y),
            center - Vec2::new(h.x, 0.0),
        ],
    };
    local.map(|p| m.transform_point3(p.extend(0.0)))
}

/// Active objects with an enabled Tilemap Renderer and a Tilemap.
pub fn rendered(scene: &Scene) -> Vec<Entity> {
    scene
        .active_with::<TilemapRenderer>()
        .into_iter()
        .filter(|e| {
            scene.world.get::<&Tilemap>(*e).is_ok()
                && scene
                    .world
                    .get::<&TilemapRenderer>(*e)
                    .is_ok_and(|r| r.enabled)
        })
        .collect()
}

#[cfg(test)]
mod tests;
