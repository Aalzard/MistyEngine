//! Procedural TilemapDemo scene in `SampleProject`: a small night-time platform level painted on
//! a Tilemap with a Rule Tile (ground edges, corners, one-tile platforms and columns picked
//! automatically from the neighbours), an animated water Tile in a pit, a stone wall Tilemap
//! behind, a Tilemap Collider 2D (cells merged into a few boxes) so the Platformer2D player
//! (`PlayerController2D.lua`) runs on it, torches with shadowed Point Light 2D lights and a dim
//! blue Global Light 2D. The tiles are also gathered in a Tile Palette.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_tilemap_demo`; the other tests
//! check the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::{IVec2, Vec2, Vec3};
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::light2d::{Light2D, ShadowCaster2D};
use misty_engine::scene::{
    BoxCollider2D, Camera, ClearFlags, Collider2DCommon, PhysicsMaterial2D, Rigidbody2D, Scene,
    ScriptRef, Scripts, SpriteRenderer, Transform,
};
use misty_engine::sprites::slice::{self, GridSlice};
use misty_engine::sprites::{FilterMode, SpriteMode, TextureImporter, builtin};
use misty_engine::tilemap::edit::{self, Brush};
use misty_engine::tilemap::palette::TilePalette;
use misty_engine::tilemap::tile::{ColliderType, TileAsset, TileKind, TilingRule};
use misty_engine::tilemap::{Grid, Tilemap, TilemapCollider2D, TilemapRenderer};

const SCENE: &str = "Assets/Scenes/TilemapDemo.scene";
const GROUND_PNG: &str = "Assets/Sprites/Tilemap/Ground.png";
const WATER_PNG: &str = "Assets/Sprites/Tilemap/Water.png";
const PROPS_PNG: &str = "Assets/Sprites/Tilemap/Props.png";
const GROUND_TILE: &str = "Assets/Tiles/Ground.tile";
const WATER_TILE: &str = "Assets/Tiles/Water.tile";
const WALL_TILE: &str = "Assets/Tiles/Stone Wall.tile";
const PALETTE: &str = "Assets/Palettes/Night.palette";
/// Made by the Platformer2D sample (sample_2d.rs).
const PLAYER_PNG: &str = "Assets/Sprites/Player.png";
const CONTROLLER: &str = "Assets/Scripts/PlayerController2D.lua";
const FOLLOW: &str = "Assets/Scripts/CameraFollow2D.lua";
const PPU: f32 = 16.0;
const PLAYER_LAYER: u8 = 5;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn rgba(hex: u32, a: u8) -> Rgba<u8> {
    let b = hex.to_be_bytes();
    Rgba([b[1], b[2], b[3], a])
}

/// Deterministic per-pixel noise in 0..n.
fn noise(x: u32, y: u32, seed: u32, n: u32) -> u32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) % n
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

fn ground_png() -> RgbaImage {
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
fn water_png() -> RgbaImage {
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
fn props_png() -> RgbaImage {
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

fn importer(w: u32, h: u32, name: &str) -> TextureImporter {
    let grid = GridSlice {
        cell: [16, 16],
        skip_empty: false,
        ..Default::default()
    };
    TextureImporter {
        pixels_per_unit: PPU,
        filter_mode: FilterMode::Nearest,
        sprite_mode: SpriteMode::Multiple,
        sprites: slice::grid(w, h, &grid, name, None),
        ..TextureImporter::sprite()
    }
}

fn textures() -> Vec<(&'static str, RgbaImage, TextureImporter)> {
    vec![
        (GROUND_PNG, ground_png(), importer(64, 64, "Ground")),
        (WATER_PNG, water_png(), importer(64, 16, "Water")),
        (PROPS_PNG, props_png(), importer(32, 16, "Props")),
    ]
}

fn ground_sprite(i: usize) -> String {
    format!("{GROUND_PNG}#Ground_{i}")
}

/// The ground Rule Tile: most specific neighbourhoods first, plain fill as the default.
fn ground_tile() -> TileAsset {
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

fn water_tile() -> TileAsset {
    TileAsset {
        kind: TileKind::AnimatedTile,
        frames: (0..4).map(|i| format!("{WATER_PNG}#Water_{i}")).collect(),
        speed: 5.0,
        collider: ColliderType::None,
        ..Default::default()
    }
}

fn wall_tile() -> TileAsset {
    TileAsset {
        collider: ColliderType::None,
        color: [0.85, 0.85, 0.95, 1.0],
        ..TileAsset::sprite(&format!("{PROPS_PNG}#Props_0"))
    }
}

fn tiles() -> Vec<(&'static str, TileAsset)> {
    vec![
        (GROUND_TILE, ground_tile()),
        (WATER_TILE, water_tile()),
        (WALL_TILE, wall_tile()),
    ]
}

fn palette() -> TilePalette {
    let mut p = TilePalette::default();
    for (path, _) in tiles() {
        p.add(path);
    }
    p
}

fn at(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

/// The level layout (cells): ground with a water pit, steps, a column and floating platforms.
fn ground_map() -> Tilemap {
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

fn water_map() -> Tilemap {
    let w = Brush::single(WATER_TILE, Default::default());
    let mut m = Tilemap::default();
    edit::box_fill(&mut m, at(7, -2), at(10, -2), &w);
    m
}

fn wall_map() -> Tilemap {
    let w = Brush::single(WALL_TILE, Default::default());
    let mut m = Tilemap::default();
    // Two ruined walls behind the level, with gaps.
    edit::box_fill(&mut m, at(-9, 0), at(2, 6), &w);
    edit::box_erase(&mut m, at(-5, 4), at(-3, 6));
    edit::box_erase(&mut m, at(0, 5), at(2, 6));
    edit::box_fill(&mut m, at(14, 1), at(26, 7), &w);
    edit::box_erase(&mut m, at(17, 5), at(19, 7));
    edit::box_erase(&mut m, at(24, 6), at(26, 7));
    m
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn tilemap(
    s: &mut Scene,
    grid: misty_engine::hecs::Entity,
    name: &str,
    map: Tilemap,
    order: i32,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, Some(grid));
    s.add(e, map);
    s.add(
        e,
        TilemapRenderer {
            order_in_layer: order,
            ..Default::default()
        },
    );
    s.game_object_mut(e).is_static = true;
    e
}

fn torch(s: &mut Scene, i: usize, at: Vec2) {
    let t = s.spawn(&format!("Torch {i}"), None);
    s.set_transform(t, Transform::from_position(at.extend(0.0)));
    s.add(
        t,
        SpriteRenderer {
            sprite: Some(format!("{PROPS_PNG}#Props_1")),
            order_in_layer: 2,
            ..Default::default()
        },
    );
    // The light sits in the flame.
    let l = s.spawn("Light", Some(t));
    s.set_transform(l, Transform::from_position(Vec3::new(0.0, 0.2, 0.0)));
    s.add(
        l,
        Light2D {
            color: [1.0, 0.62, 0.3],
            intensity: 1.6,
            inner_radius: 0.6,
            outer_radius: 7.0,
            falloff_strength: 0.45,
            shadows_enabled: true,
            shadow_strength: 0.9,
            shadow_softness: 0.35,
            ..Default::default()
        },
    );
}

fn demo_scene() -> Scene {
    let mut s = Scene::new("TilemapDemo");

    let cam = s.spawn("Main Camera", None);
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.set_transform(cam, Transform::from_position(Vec3::new(-4.0, 2.0, -10.0)));
    s.add(
        cam,
        Camera {
            clear_flags: ClearFlags::SolidColor,
            background: [0.03, 0.04, 0.09, 1.0],
            orthographic: true,
            ortho_size: 6.0,
            ..Default::default()
        },
    );
    s.add(cam, script(FOLLOW));

    // Moonlight: a dim blue global light (without it, unlit areas are black).
    let global = s.spawn("Global Light 2D", None);
    s.add(global, Light2D::global([0.45, 0.52, 0.85], 0.3));

    let moon = s.spawn("Moon", None);
    let mut t = Transform::from_position(Vec3::new(14.0, 9.0, 5.0));
    t.scale = Vec3::new(3.0, 3.0, 1.0);
    s.set_transform(moon, t);
    s.add(
        moon,
        SpriteRenderer {
            sprite: Some(builtin::path("Circle")),
            color: [3.0, 3.0, 2.8, 1.0],
            order_in_layer: -20,
            ..Default::default()
        },
    );

    let grid = s.spawn("Grid", None);
    s.add(grid, Grid::default());
    tilemap(&mut s, grid, "Stone Wall", wall_map(), -10);
    let ground = tilemap(&mut s, grid, "Ground", ground_map(), 0);
    s.add(ground, TilemapCollider2D::default());
    s.add(ground, ShadowCaster2D::default());
    let water = tilemap(&mut s, grid, "Water", water_map(), 6);
    s.world.get::<&mut Tilemap>(water).unwrap().color = [1.0, 1.0, 1.0, 0.9];

    for (i, p) in [
        Vec2::new(-8.0, 0.5),
        Vec2::new(4.5, 6.5),
        Vec2::new(13.0, 0.5),
        Vec2::new(25.5, 0.5),
        Vec2::new(30.0, 4.5),
    ]
    .into_iter()
    .enumerate()
    {
        torch(&mut s, i + 1, p);
    }

    let player = s.spawn("Player", None);
    s.game_object_mut(player).layer = PLAYER_LAYER;
    s.set_transform(player, Transform::from_position(Vec3::new(-10.0, 0.5, 0.0)));
    s.add(
        player,
        SpriteRenderer {
            sprite: Some(PLAYER_PNG.into()),
            order_in_layer: 5,
            ..Default::default()
        },
    );
    s.add(
        player,
        Rigidbody2D {
            gravity_scale: 3.0,
            freeze_rotation: true,
            ..Default::default()
        },
    );
    s.add(
        player,
        BoxCollider2D {
            common: Collider2DCommon {
                material: PhysicsMaterial2D {
                    friction: 0.0,
                    bounciness: 0.0,
                },
                ..Default::default()
            },
            size: Vec2::new(0.7, 0.95),
            ..Default::default()
        },
    );
    s.add(player, script(CONTROLLER));
    s.add(player, ShadowCaster2D::default());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_tilemap_demo() {
    let root = project();
    for dir in ["Assets/Sprites/Tilemap", "Assets/Tiles", "Assets/Palettes"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for (rel, img, imp) in textures() {
        let path = root.join(rel);
        img.save(&path).unwrap();
        imp.save(&path).unwrap();
    }
    for (rel, t) in tiles() {
        t.save(&root.join(rel)).unwrap();
    }
    palette().save(&root.join(PALETTE)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn tilemap_demo_matches_its_generator() {
    let root = project();
    for (rel, img, imp) in textures() {
        let path = root.join(rel);
        let on_disk = misty_engine::image::open(&path).unwrap().into_rgba8();
        crate::test_util::assert_image_close(&on_disk, &img, rel);
        crate::test_util::assert_close(
            &TextureImporter::try_load(&path).unwrap(),
            &imp,
            format!("{rel}.meta"),
        );
    }
    for (rel, t) in tiles() {
        crate::test_util::assert_close(&TileAsset::load(&root.join(rel)).unwrap(), &t, rel);
    }
    crate::test_util::assert_close(
        &TilePalette::load(&root.join(PALETTE)).unwrap(),
        &palette(),
        PALETTE,
    );
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene().to_json(), SCENE);
}

/// Rule tiles picked the edges: grass on top, corners at the ends, platforms one tall.
#[test]
fn tilemap_demo_rule_tiles_pick_edges() {
    use misty_engine::tilemap::tile;
    let root = project();
    tile::register(
        GROUND_TILE,
        TileAsset::load(&root.join(GROUND_TILE)).unwrap(),
    );
    let m = ground_map();
    let sprite = |c: IVec2| tile::resolve(&m, c, 0.0).and_then(|r| r.sprite);
    assert_eq!(sprite(at(0, -1)), Some(ground_sprite(1)), "grass top");
    assert_eq!(sprite(at(0, -2)), Some(ground_sprite(5)), "fill below");
    assert_eq!(
        sprite(at(6, -1)),
        Some(ground_sprite(2)),
        "corner before the pit"
    );
    assert_eq!(sprite(at(6, -2)), Some(ground_sprite(6)), "pit wall");
    assert_eq!(
        sprite(at(-4, 2)),
        Some(ground_sprite(12)),
        "platform left end"
    );
    assert_eq!(
        sprite(at(-2, 2)),
        Some(ground_sprite(13)),
        "platform middle"
    );
    assert_eq!(sprite(at(20, 5)), Some(ground_sprite(15)), "single block");
    assert_eq!(sprite(at(23, 2)), Some(ground_sprite(3)), "column top");
    assert_eq!(
        sprite(at(3, 4)),
        Some(ground_sprite(8)),
        "bottom-left of a thick platform"
    );
    // The collider merges ~200 cells into a handful of boxes.
    let shapes = misty_engine::tilemap::collider::shapes(&m, &Grid::default(), true);
    assert!(
        m.len() > 150 && shapes.len() < 20,
        "{} cells → {} boxes",
        m.len(),
        shapes.len()
    );
}

/// Playing: the player lands on the tile collider, runs right along the ground, jumps and
/// lands again; the torches' lights cast shadows.
#[test]
fn tilemap_demo_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let lights = misty_engine::light2d::collect(&scene);
    assert_eq!(lights.items.len(), 6, "global + 5 torches");
    assert_eq!(lights.shadows.len(), 5 * misty_engine::light2d::SHADOW_BINS);
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let player = scene.find_by_name("Player").unwrap();
    let mut time = Time::default();
    let mut run = |scene: &mut Scene, seconds: f32, keys: &[&str], press: &[&str]| {
        for i in 0..(seconds * 60.0) as usize {
            let input = Input {
                held: keys.iter().map(|k| k.to_string()).collect(),
                pressed: if i == 0 {
                    press.iter().map(|k| k.to_string()).collect()
                } else {
                    Default::default()
                },
                ..Default::default()
            };
            time.advance(1.0 / 60.0);
            rt.update(scene, &time, input);
            let errors: Vec<_> = rt
                .take_logs()
                .into_iter()
                .filter(|(k, _)| *k != LogKind::Info)
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
        }
    };
    run(&mut scene, 0.5, &[], &[]);
    let start = scene.world_position(player);
    assert!(
        (start.y - 0.475).abs() < 0.05,
        "standing on the tiles: {start}"
    );
    run(&mut scene, 0.6, &["d"], &[]);
    let p = scene.world_position(player);
    assert!(p.x > start.x + 2.0, "ran right: {p}");
    assert!((p.y - 0.475).abs() < 0.05, "still on the ground: {p}");
    run(&mut scene, 0.3, &[], &["space"]);
    assert!(scene.world_position(player).y > 2.0, "jumped");
    run(&mut scene, 1.5, &[], &[]);
    let down = scene.world_position(player);
    assert!((down.y - 0.475).abs() < 0.05, "landed: {down}");
}
