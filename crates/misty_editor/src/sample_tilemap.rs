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
use misty_engine::light2d::{Light2D, ShadowCaster2D};
use misty_engine::scene::{
    BoxCollider2D, Camera, ClearFlags, Collider2DCommon, PhysicsMaterial2D, Rigidbody2D, Scene,
    ScriptRef, Scripts, SpriteRenderer, Transform,
};
use misty_engine::sprites::{TextureImporter, builtin};
use misty_engine::tilemap::palette::TilePalette;
use misty_engine::tilemap::tile::TileAsset;
use misty_engine::tilemap::{Grid, Tilemap, TilemapCollider2D, TilemapRenderer};

// The art, tiles and level are shared with the 2D Platformer template.
use crate::templates::platformer_2d::art::{
    GROUND_TILE, PLAYER_PNG, at, ground_map, ground_sprite, tilemap_textures, tiles, torch_sprite,
    wall_map, water_map,
};

const SCENE: &str = "Assets/Scenes/TilemapDemo.scene";
const PALETTE: &str = "Assets/Palettes/Night.palette";
const CONTROLLER: &str = "Assets/Scripts/PlayerController2D.lua";
const FOLLOW: &str = "Assets/Scripts/CameraFollow2D.lua";
const PLAYER_LAYER: u8 = 5;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn palette() -> TilePalette {
    let mut p = TilePalette::default();
    for (path, _) in tiles() {
        p.add(path);
    }
    p
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
            sprite: Some(torch_sprite()),
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
    for (rel, img, imp) in tilemap_textures() {
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
    for (rel, img, imp) in tilemap_textures() {
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
