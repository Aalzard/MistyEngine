//! The pixel art, tiles and level layout shared by the 2D Platformer template and the sample
//! project's Platformer2D and TilemapDemo scenes (`sample_2d`, `sample_tilemap`): everything is
//! drawn here in code at 16 pixels per unit, nothing downloaded, free to reuse.
//!
//! - Sprites: the player (16×16), a coin, the ground sheet (4×4 cells of edges and corners for
//!   the Rule Tile), four frames of water and a props sheet (a stone wall tile and a torch).
//! - Tiles: the ground Rule Tile, the animated water Tile and the stone wall Tile.
//! - The level: ground with a water pit, steps, a column, floating platforms and ruined walls.

use glam::IVec2;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::sprites::slice::{self, GridSlice};
use misty_engine::sprites::{FilterMode, SpriteMode, SpriteRect, TextureImporter};
use misty_engine::tilemap::Tilemap;
use misty_engine::tilemap::edit::{self, Brush};
use misty_engine::tilemap::tile::{ColliderType, TileAsset, TileKind, TilingRule};

pub const PLAYER_PNG: &str = "Assets/Sprites/Player.png";
pub const COIN_PNG: &str = "Assets/Sprites/Coin.png";
pub const GROUND_PNG: &str = "Assets/Sprites/Tilemap/Ground.png";
pub const WATER_PNG: &str = "Assets/Sprites/Tilemap/Water.png";
pub const PROPS_PNG: &str = "Assets/Sprites/Tilemap/Props.png";
pub const GROUND_TILE: &str = "Assets/Tiles/Ground.tile";
pub const WATER_TILE: &str = "Assets/Tiles/Water.tile";
pub const WALL_TILE: &str = "Assets/Tiles/Stone Wall.tile";
/// Pixels per unit of every sprite: one 16×16 tile is one cell (1 m).
pub const PPU: f32 = 16.0;

pub fn rgba(hex: u32, a: u8) -> Rgba<u8> {
    let b = hex.to_be_bytes();
    Rgba([b[1], b[2], b[3], a])
}

/// Deterministic per-pixel noise in 0..n.
fn noise(x: u32, y: u32, seed: u32, n: u32) -> u32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) % n
}

/// An image from rows of characters (`color` maps each one; `None` is transparent).
pub fn pixel_art(rows: &[&str], color: impl Fn(char) -> Option<Rgba<u8>>) -> RgbaImage {
    RgbaImage::from_fn(rows[0].len() as u32, rows.len() as u32, |x, y| {
        let c = rows[y as usize].as_bytes()[x as usize] as char;
        color(c).unwrap_or(Rgba([0, 0, 0, 0]))
    })
}

/// 16×16 hero: dark outline, blue suit, skin face with an eye looking right.
pub fn player_png() -> RgbaImage {
    const ART: [&str; 16] = [
        "................",
        ".....oooooo.....",
        "....osssssso....",
        "....ossssswo....",
        "....osssssko....",
        "....osssssso....",
        ".....oooooo.....",
        "...oobbbbbboo...",
        "..obbbbbbbbbbo..",
        "..obobbbbbbobo..",
        "..ososbbbbsoso..",
        "....obbbbbbo....",
        "....obboobbo....",
        "....obbo.obbo...",
        "...ooooo.ooooo..",
        "................",
    ];
    pixel_art(&ART, |c| match c {
        'o' => Some(rgba(0x1b1b2f, 255)),
        's' => Some(rgba(0xf2c29b, 255)),
        'w' => Some(rgba(0xffffff, 255)),
        'k' => Some(rgba(0x222222, 255)),
        'b' => Some(rgba(0x3b6fd8, 255)),
        _ => None,
    })
}

/// 16×16 coin: gold disc, dark rim, light glint.
pub fn coin_png() -> RgbaImage {
    const ART: [&str; 16] = [
        "................",
        ".....oooooo.....",
        "...ooyyyyyyoo...",
        "..oyyyhhyyyyyo..",
        "..oyyhhyyyyyyo..",
        ".oyyhyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        "..oyyyyyyyyyyo..",
        "..oyyyyyyyyyyo..",
        "...ooyyyyyyoo...",
        ".....oooooo.....",
        "................",
    ];
    pixel_art(&ART, |c| match c {
        'o' => Some(rgba(0x8a5a00, 255)),
        'y' => Some(rgba(0xffc62e, 255)),
        'h' => Some(rgba(0xfff3b0, 255)),
        'd' => Some(rgba(0xd99a00, 255)),
        _ => None,
    })
}

/// Which sides of a ground cell are open (drawn with grass or an outline).
#[derive(Clone, Copy)]
struct Edges {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

const fn e(top: bool, bottom: bool, left: bool, right: bool) -> Edges {
    Edges {
        top,
        bottom,
        left,
        right,
    }
}

/// The 4×4 ground sheet, row by row: top edges, sides, bottom edges, one-tall platforms.
const GROUND_CELLS: [Edges; 16] = [
    e(true, false, true, false),
    e(true, false, false, false),
    e(true, false, false, true),
    e(true, false, true, true),
    e(false, false, true, false),
    e(false, false, false, false),
    e(false, false, false, true),
    e(false, false, true, true),
    e(false, true, true, false),
    e(false, true, false, false),
    e(false, true, false, true),
    e(false, true, true, true),
    e(true, true, true, false),
    e(true, true, false, false),
    e(true, true, false, true),
    e(true, true, true, true),
];

/// 64×64 ground sheet: dirt with grass along open tops and dark outlines on open sides.
pub fn ground_png() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        let (cell, lx, ly) = ((y / 16) * 4 + x / 16, x % 16, y % 16);
        let ed = GROUND_CELLS[cell as usize];
        let outline = rgba(0x1f1610, 255);
        // Grass along an open top, hanging slightly over the sides.
        if ed.top && ly < 4 {
            if ly == 3 && noise(x, y, 1, 3) == 0 {
                return rgba(0x2e6b2c, 255);
            }
            return match ly {
                0 => rgba(0x7fd36a, 255),
                1 => rgba(0x58b04c, 255),
                _ => rgba(0x3f8f3a, 255),
            };
        }
        if (ed.left && lx < 1) || (ed.right && lx > 14) || (ed.bottom && ly > 14) {
            return outline;
        }
        if (ed.left && lx < 2) || (ed.right && lx > 13) || (ed.bottom && ly > 13) {
            return rgba(0x3b281c, 255);
        }
        match noise(x, y, 7, 17) {
            0 => rgba(0x8a6a4a, 255),
            1 | 2 => rgba(0x4a3424, 255),
            _ => rgba(0x654733, 255),
        }
    })
}

/// Four 16×16 water frames: a wave crest sliding right over deep blue.
pub fn water_png() -> RgbaImage {
    RgbaImage::from_fn(64, 16, |x, y| {
        let (frame, lx) = (x / 16, x % 16);
        let phase = (lx as f32 + frame as f32 * 4.0) / 16.0 * std::f32::consts::TAU;
        let crest = 2.0 + phase.sin() * 1.5;
        let y = y as f32;
        if y < crest - 0.5 {
            Rgba([0, 0, 0, 0])
        } else if y < crest + 1.0 {
            rgba(0xa8e4ff, 230)
        } else if y < 9.0 {
            rgba(0x2f7cc4, 210)
        } else {
            rgba(0x1f5592, 225)
        }
    })
}

/// 32×16 props: a stone wall tile and a torch.
pub fn props_png() -> RgbaImage {
    const TORCH: [&str; 16] = [
        "................",
        "................",
        ".......y........",
        "......yoy.......",
        "......yyoy......",
        ".....yoyyo......",
        ".....oyyyo......",
        "......ooo.......",
        "......bmb.......",
        ".......b........",
        ".......b........",
        ".......b........",
        ".......b........",
        ".......b........",
        "......ddd.......",
        "................",
    ];
    RgbaImage::from_fn(32, 16, |x, y| {
        if x < 16 {
            // Bricks 8×4 with offset rows and dark mortar.
            let row = y / 4;
            let off = if row % 2 == 0 { 0 } else { 4 };
            let mortar = y % 4 == 3 || (x + off) % 8 == 7;
            if mortar {
                rgba(0x23263a, 255)
            } else if noise(x, y, 3, 9) == 0 {
                rgba(0x4c5270, 255)
            } else {
                rgba(0x3a3f58, 255)
            }
        } else {
            let c = TORCH[y as usize].as_bytes()[(x - 16) as usize] as char;
            match c {
                'y' => rgba(0xffe27a, 255),
                'o' => rgba(0xff8a2a, 255),
                'b' => rgba(0x6b4423, 255),
                'm' => rgba(0x9a9aa8, 255),
                'd' => rgba(0x3b2a1c, 255),
                _ => Rgba([0, 0, 0, 0]),
            }
        }
    })
}

/// Pixel-art sprite import: Nearest Neighbor filter, [`PPU`] pixels per unit; one sprite, or
/// `sprites` cut from the sheet.
pub fn importer(sprites: Vec<SpriteRect>) -> TextureImporter {
    TextureImporter {
        pixels_per_unit: PPU,
        filter_mode: FilterMode::Nearest,
        sprite_mode: if sprites.is_empty() {
            SpriteMode::Single
        } else {
            SpriteMode::Multiple
        },
        sprites,
        ..TextureImporter::sprite()
    }
}

/// [`importer`] for a `w`×`h` sheet sliced into 16×16 sprites named `name_0`, `name_1`…
pub fn sheet_importer(w: u32, h: u32, name: &str) -> TextureImporter {
    let grid = GridSlice {
        cell: [16, 16],
        skip_empty: false,
        ..Default::default()
    };
    importer(slice::grid(w, h, &grid, name, None))
}

/// The tilemap sprite sheets with their import settings.
pub fn tilemap_textures() -> Vec<(&'static str, RgbaImage, TextureImporter)> {
    vec![
        (GROUND_PNG, ground_png(), sheet_importer(64, 64, "Ground")),
        (WATER_PNG, water_png(), sheet_importer(64, 16, "Water")),
        (PROPS_PNG, props_png(), sheet_importer(32, 16, "Props")),
    ]
}

pub fn ground_sprite(i: usize) -> String {
    format!("{GROUND_PNG}#Ground_{i}")
}

/// The torch sprite of the props sheet.
pub fn torch_sprite() -> String {
    format!("{PROPS_PNG}#Props_1")
}

/// The ground Rule Tile: most specific neighbourhoods first, plain fill as the default.
pub fn ground_tile() -> TileAsset {
    let rule = |rows: [&str; 3], i: usize| TilingRule {
        collider: ColliderType::Grid,
        ..TilingRule::pattern(rows, &ground_sprite(i))
    };
    TileAsset {
        kind: TileKind::RuleTile,
        sprite: Some(ground_sprite(5)),
        collider: ColliderType::Grid,
        rules: vec![
            // One cell tall: platforms.
            rule(["?.?", ".x.", "?.?"], 15),
            rule(["?.?", ".x#", "?.?"], 12),
            rule(["?.?", "#x.", "?.?"], 14),
            rule(["?.?", "#x#", "?.?"], 13),
            // Open top: grass, corners and column tops.
            rule(["?.?", ".x.", "?#?"], 3),
            rule(["?.?", ".x#", "???"], 0),
            rule(["?.?", "#x.", "???"], 2),
            rule(["?.?", "?x?", "???"], 1),
            // Open bottom.
            rule(["?#?", ".x.", "?.?"], 11),
            rule(["???", ".x#", "?.?"], 8),
            rule(["???", "#x.", "?.?"], 10),
            rule(["???", "?x?", "?.?"], 9),
            // Sides.
            rule(["???", ".x.", "???"], 7),
            rule(["???", ".x?", "???"], 4),
            rule(["???", "?x.", "???"], 6),
        ],
        ..Default::default()
    }
}

pub fn water_tile() -> TileAsset {
    TileAsset {
        kind: TileKind::AnimatedTile,
        frames: (0..4).map(|i| format!("{WATER_PNG}#Water_{i}")).collect(),
        speed: 5.0,
        collider: ColliderType::None,
        ..Default::default()
    }
}

pub fn wall_tile() -> TileAsset {
    TileAsset {
        collider: ColliderType::None,
        color: [0.85, 0.85, 0.95, 1.0],
        ..TileAsset::sprite(&format!("{PROPS_PNG}#Props_0"))
    }
}

pub fn tiles() -> Vec<(&'static str, TileAsset)> {
    vec![
        (GROUND_TILE, ground_tile()),
        (WATER_TILE, water_tile()),
        (WALL_TILE, wall_tile()),
    ]
}

pub fn at(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

/// The level layout (cells): ground with a water pit, steps, a column and floating platforms.
/// The ground's top is at y = 0 (the pit floor two cells lower), walls at both ends.
pub fn ground_map() -> Tilemap {
    let g = Brush::single(GROUND_TILE, Default::default());
    let mut m = Tilemap::default();
    // Left wall and the ground up to the pit.
    edit::box_fill(&mut m, at(-14, -4), at(-13, 7), &g);
    edit::box_fill(&mut m, at(-12, -4), at(6, -1), &g);
    // Pit floor two cells lower.
    edit::box_fill(&mut m, at(7, -4), at(10, -3), &g);
    // Ground after the pit, a step and a column.
    edit::box_fill(&mut m, at(11, -4), at(34, -1), &g);
    edit::box_fill(&mut m, at(15, 0), at(18, 0), &g);
    edit::box_fill(&mut m, at(23, 0), at(23, 2), &g);
    // Right wall.
    edit::box_fill(&mut m, at(35, -4), at(36, 7), &g);
    // Floating platforms.
    edit::box_fill(&mut m, at(-4, 2), at(-1, 2), &g);
    edit::box_fill(&mut m, at(3, 4), at(6, 5), &g);
    edit::box_fill(&mut m, at(12, 3), at(13, 3), &g);
    edit::box_fill(&mut m, at(27, 3), at(31, 3), &g);
    edit::paint(&mut m, at(20, 5), &g);
    m
}

/// Water filling the pit.
pub fn water_map() -> Tilemap {
    let w = Brush::single(WATER_TILE, Default::default());
    let mut m = Tilemap::default();
    edit::box_fill(&mut m, at(7, -2), at(10, -2), &w);
    m
}

/// Two ruined walls behind the level, with gaps.
pub fn wall_map() -> Tilemap {
    let w = Brush::single(WALL_TILE, Default::default());
    let mut m = Tilemap::default();
    edit::box_fill(&mut m, at(-9, 0), at(2, 6), &w);
    edit::box_erase(&mut m, at(-5, 4), at(-3, 6));
    edit::box_erase(&mut m, at(0, 5), at(2, 6));
    edit::box_fill(&mut m, at(14, 1), at(26, 7), &w);
    edit::box_erase(&mut m, at(17, 5), at(19, 7));
    edit::box_erase(&mut m, at(24, 6), at(26, 7));
    m
}
