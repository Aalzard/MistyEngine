//! The 2D Platformer template, built from the sample project's Platformer2D and TilemapDemo
//! scenes (the same art, tiles and level: [`art`]), generated here so every install can make it:
//!
//! - A daytime tilemap level on a Grid: the ground painted with a Rule Tile (grass tops, edges
//!   and corners picked from the neighbours) with a Tilemap Collider 2D and a Shadow Caster 2D,
//!   animated water in a pit and ruined stone walls behind.
//! - The Platformer2D player: a Rigidbody 2D with a Box Collider 2D, moved by
//!   `PlayerController2D.lua` (A/D or the arrow keys run; Space, W or Up jumps).
//! - Coins to collect (triggers) counted by a UI Text, an orthographic camera following the
//!   player inside the level (`CameraFollow2D.lua`), a sun and hills behind.
//! - 2D lights: a Global Light 2D (daylight) and torches with shadowed Point Light 2D lights; a
//!   post-processing volume with bloom, so the sun, the flames and the coins glow.
//! - A Tile Palette with the three tiles, ready to paint with (Window ▸ 2D ▸ Tile Palette), and
//!   project settings for 2D: Background / Default / Foreground Sorting Layers. The Scene view
//!   starts in 2D mode.

pub mod art;

use std::path::Path;

use glam::{Vec2, Vec3};
use misty_engine::asset_db::{Guid, GuidDatabase, meta, refs};
use misty_engine::hecs::Entity;
use misty_engine::light2d::{Light2D, ShadowCaster2D};
use misty_engine::scene::{
    BoxCollider2D, Camera, Canvas, CanvasScaler, CircleCollider2D, ClearFlags, Collider2DCommon,
    PhysicsMaterial2D, PostProcessVolume, RectTransform, Rigidbody2D, Scene, ScriptRef, Scripts,
    SpriteRenderer, TextAnchor, Transform, UiText,
};
use misty_engine::settings::ProjectSettings;
use misty_engine::sprites::builtin;
use misty_engine::tilemap::palette::TilePalette;
use misty_engine::tilemap::{Grid, Tilemap, TilemapCollider2D, TilemapRenderer};
use misty_engine::ui::presets;

use super::io_error;
use super::scenes::VOLUME;
use crate::scene_files::SavedView2D;

pub const CONTROLLER: &str = "Assets/Scripts/PlayerController2D.lua";
pub const FOLLOW: &str = "Assets/Scripts/CameraFollow2D.lua";
pub const PALETTE: &str = "Assets/Palettes/Level.palette";

/// The scripts, as shipped in `templates/platformer_2d/`.
pub const SCRIPTS: [(&str, &str); 2] = [
    (
        CONTROLLER,
        include_str!("platformer_2d/PlayerController2D.lua"),
    ),
    (FOLLOW, include_str!("platformer_2d/CameraFollow2D.lua")),
];

/// The project's Sorting Layers: sky and walls behind, the level, water in front.
pub const SORTING_LAYERS: [&str; 3] = ["Background", "Default", "Foreground"];
const BACKGROUND: &str = "Background";
const FOREGROUND: &str = "Foreground";

/// Where the player starts (feet on the ground at y = 0).
pub const PLAYER_START: Vec2 = Vec2::new(-10.0, 0.5);
/// Built-in layer 5, "Player".
const PLAYER_LAYER: u8 = 5;
/// Built-in layer 2: the player's ground rays (Default only) never hit coins.
const IGNORE_RAYCAST_LAYER: u8 = 2;
/// Half the camera's view height (world units).
pub const ORTHO_SIZE: f32 = 6.0;
/// Where the camera starts: the follow script's place for the player at a 16:9 aspect (the
/// view's left edge on the level's left edge, its bottom on the level's bottom).
pub const CAMERA_START: Vec2 = Vec2::new(-14.0 + ORTHO_SIZE * 16.0 / 9.0, -4.0 + ORTHO_SIZE);

/// The coins, along the way: three on the ground, the others on platforms and over the pit.
pub const COINS: [Vec2; 10] = [
    Vec2::new(-6.0, 0.5),
    Vec2::new(-5.0, 0.5),
    Vec2::new(-4.0, 0.5),
    Vec2::new(-2.5, 3.6),
    Vec2::new(5.0, 6.6),
    Vec2::new(9.0, 1.2),
    Vec2::new(13.0, 4.6),
    Vec2::new(17.0, 1.6),
    Vec2::new(20.5, 6.6),
    Vec2::new(29.5, 4.6),
];

/// Torches: on the ground and on two platforms.
const TORCHES: [Vec2; 5] = [
    Vec2::new(-8.0, 0.5),
    Vec2::new(4.5, 6.5),
    Vec2::new(13.0, 0.5),
    Vec2::new(25.5, 0.5),
    Vec2::new(30.0, 4.5),
];

/// Write the template into `root` (a project folder with its settings). Returns the 2D Scene
/// view to start with.
pub(super) fn create(root: &Path) -> Result<SavedView2D, String> {
    for dir in ["Assets/Sprites/Tilemap", "Assets/Tiles", "Assets/Palettes"] {
        let path = root.join(dir);
        std::fs::create_dir_all(&path).map_err(io_error("Could not create the folder", &path))?;
    }
    let mut textures = vec![
        (
            art::PLAYER_PNG,
            art::player_png(),
            art::importer(Vec::new()),
        ),
        (art::COIN_PNG, art::coin_png(), art::importer(Vec::new())),
    ];
    textures.extend(art::tilemap_textures());
    for (rel, image, importer) in textures {
        let path = root.join(rel);
        image
            .save(&path)
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        meta::write_guid(&path, Guid::new())?;
        importer.save(&path)?;
    }
    // Tiles reference the sprites, the palette the tiles: written in that order so each file's
    // GUID table finds what it uses.
    for (rel, tile) in art::tiles() {
        write_json(root, rel, &tile)?;
    }
    let mut palette = TilePalette::default();
    for (rel, _) in art::tiles() {
        palette.add(rel);
    }
    write_json(root, PALETTE, &palette)?;
    for (rel, source) in SCRIPTS {
        let path = root.join(rel);
        std::fs::write(&path, source).map_err(io_error("Could not write", &path))?;
        meta::write_guid(&path, Guid::new())?;
    }

    let mut settings = ProjectSettings::load(root)?;
    settings.sorting_layers = SORTING_LAYERS.map(String::from).to_vec();
    settings.save(root)?;
    super::save_scene(root, super::SCENE, &scene())?;
    Ok(SavedView2D {
        center: CAMERA_START.to_array(),
        size: ORTHO_SIZE + 0.5,
    })
}

/// Save `value` as JSON at `root/rel` with the GUID table of the new project's meta files,
/// and give it a meta file of its own.
fn write_json(root: &Path, rel: &str, value: &impl serde::Serialize) -> Result<(), String> {
    let path = root.join(rel);
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let text = refs::embed(&GuidDatabase::read(root), &text);
    std::fs::write(&path, text).map_err(io_error("Could not write", &path))?;
    meta::write_guid(&path, Guid::new())
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_position(Vec3::new(x, y, z))
}

/// A built-in shape sprite, tinted, on the Background layer.
fn backdrop(s: &mut Scene, name: &str, parent: Option<Entity>, t: Transform, color: [f32; 4]) {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(
        e,
        SpriteRenderer {
            sprite: Some(builtin::path("Circle")),
            color,
            sorting_layer: BACKGROUND.into(),
            order_in_layer: -20,
            ..Default::default()
        },
    );
}

fn scaled(x: f32, y: f32, z: f32, sx: f32, sy: f32) -> Transform {
    let mut t = at(x, y, z);
    t.scale = Vec3::new(sx, sy, 1.0);
    t
}

fn tilemap(
    s: &mut Scene,
    grid: Entity,
    name: &str,
    map: Tilemap,
    layer: &str,
    order: i32,
) -> Entity {
    let e = s.spawn(name, Some(grid));
    s.add(e, map);
    s.add(
        e,
        TilemapRenderer {
            sorting_layer: layer.into(),
            order_in_layer: order,
            ..Default::default()
        },
    );
    s.game_object_mut(e).is_static = true;
    e
}

fn camera(s: &mut Scene) -> Entity {
    let cam = s.spawn("Main Camera", None);
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.set_transform(cam, at(CAMERA_START.x, CAMERA_START.y, -10.0));
    s.add(
        cam,
        Camera {
            clear_flags: ClearFlags::SolidColor,
            background: [0.42, 0.72, 0.95, 1.0],
            orthographic: true,
            ortho_size: ORTHO_SIZE,
            ..Default::default()
        },
    );
    s.add(cam, script(FOLLOW));
    cam
}

/// The sky: a glowing sun that travels with the camera (it is far away), clouds and two rows
/// of hills along the level.
fn sky(s: &mut Scene, cam: Entity) {
    backdrop(
        s,
        "Sun",
        Some(cam),
        scaled(6.5, 3.4, 15.0, 2.2, 2.2),
        [2.6, 2.3, 1.4, 1.0],
    );
    let clouds = s.spawn("Clouds", None);
    for (i, (x, y)) in [(-9.0, 7.2), (4.0, 8.4), (16.0, 7.6), (28.0, 8.2)]
        .into_iter()
        .enumerate()
    {
        let cloud = s.spawn(&format!("Cloud {}", i + 1), Some(clouds));
        s.set_transform(cloud, at(x, y, 12.0));
        for (j, (dx, dy, r)) in [(-1.0, 0.0, 1.3), (0.2, 0.35, 1.7), (1.4, 0.0, 1.2)]
            .into_iter()
            .enumerate()
        {
            backdrop(
                s,
                &format!("Puff {}", j + 1),
                Some(cloud),
                scaled(dx, dy, 0.0, r, r * 0.8),
                [1.0, 1.0, 1.0, 0.95],
            );
        }
    }
    let hills = s.spawn("Hills", None);
    for i in 0..5 {
        let x = -16.0 + i as f32 * 13.0;
        let far = s.spawn(&format!("Far Hill {}", i + 1), Some(hills));
        s.set_transform(far, scaled(x, -3.0, 10.0, 16.0, 11.0));
        let near = s.spawn(&format!("Hill {}", i + 1), Some(hills));
        s.set_transform(near, scaled(x + 6.5, -4.5, 9.0, 13.0, 9.0));
        for (e, color, order) in [
            (far, [0.6, 0.82, 0.55, 1.0], -16),
            (near, [0.48, 0.72, 0.42, 1.0], -15),
        ] {
            s.add(
                e,
                SpriteRenderer {
                    sprite: Some(builtin::path("Circle")),
                    color,
                    sorting_layer: BACKGROUND.into(),
                    order_in_layer: order,
                    ..Default::default()
                },
            );
        }
    }
}

fn torch(s: &mut Scene, parent: Entity, i: usize, p: Vec2) {
    let t = s.spawn(&format!("Torch {i}"), Some(parent));
    s.set_transform(t, at(p.x, p.y, 0.0));
    s.add(
        t,
        SpriteRenderer {
            sprite: Some(art::torch_sprite()),
            order_in_layer: 2,
            ..Default::default()
        },
    );
    // The light sits in the flame.
    let l = s.spawn("Light", Some(t));
    s.set_transform(l, at(0.0, 0.2, 0.0));
    s.add(
        l,
        Light2D {
            color: [1.0, 0.62, 0.3],
            intensity: 0.9,
            inner_radius: 0.4,
            outer_radius: 5.0,
            falloff_strength: 0.5,
            shadows_enabled: true,
            shadow_strength: 0.8,
            shadow_softness: 0.35,
            ..Default::default()
        },
    );
}

fn coin(s: &mut Scene, parent: Entity, i: usize, p: Vec2) {
    let c = s.spawn(&format!("Coin {i}"), Some(parent));
    s.set_transform(c, at(p.x, p.y, 0.0));
    s.add(
        c,
        SpriteRenderer {
            sprite: Some(art::COIN_PNG.into()),
            // Brighter than white: the bloom makes the coins shine.
            color: [1.35, 1.25, 1.0, 1.0],
            order_in_layer: 1,
            ..Default::default()
        },
    );
    s.add(
        c,
        CircleCollider2D {
            common: Collider2DCommon {
                is_trigger: true,
                ..Default::default()
            },
            radius: 0.35,
        },
    );
    s.game_object_mut(c).layer = IGNORE_RAYCAST_LAYER;
}

fn player(s: &mut Scene) {
    let player = s.spawn("Player", None);
    s.game_object_mut(player).layer = PLAYER_LAYER;
    s.game_object_mut(player).tag = "Player".into();
    s.set_transform(player, at(PLAYER_START.x, PLAYER_START.y, 0.0));
    s.add(
        player,
        SpriteRenderer {
            sprite: Some(art::PLAYER_PNG.into()),
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
            // No friction, so the player slides along walls instead of sticking to them.
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
}

fn text(s: &mut Scene, canvas: Entity, name: &str, value: &str, size: f32, rt: RectTransform) {
    let e = presets::spawn(s, name, Some(canvas), rt);
    s.add(
        e,
        UiText {
            text: value.into(),
            font_size: size,
            color: [0.08, 0.1, 0.2, 1.0],
            alignment: TextAnchor::UpperLeft,
            wrap: false,
            ..Default::default()
        },
    );
}

/// The coin counter top left and the controls bottom left.
fn hud(s: &mut Scene) {
    let canvas = presets::spawn(s, "Canvas", None, RectTransform::stretched());
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            reference_resolution: Vec2::new(1600.0, 900.0),
            match_width_or_height: 0.5,
            ..Default::default()
        },
    );
    let corner = |anchor: Vec2, pos: Vec2, size: Vec2| RectTransform {
        anchor_min: anchor,
        anchor_max: anchor,
        pivot: anchor,
        anchored_position: pos,
        size_delta: size,
    };
    text(
        s,
        canvas,
        "Score Text",
        "Coins: 0",
        40.0,
        corner(
            Vec2::new(0.0, 1.0),
            Vec2::new(28.0, -20.0),
            Vec2::new(400.0, 56.0),
        ),
    );
    text(
        s,
        canvas,
        "Controls Text",
        "A/D or arrows: run    Space: jump",
        24.0,
        corner(Vec2::ZERO, Vec2::new(28.0, 20.0), Vec2::new(700.0, 36.0)),
    );
}

/// Bloom for the glow of the sun, the flames and the coins; a light vignette.
pub fn glow_volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.bloom.active = true;
    v.bloom.intensity = 0.6;
    v.bloom.threshold = 1.0;
    v.vignette.active = true;
    v.vignette.intensity = 0.15;
    v
}

/// The template's scene (see the module docs).
pub fn scene() -> Scene {
    let mut s = Scene::new("Main");
    let cam = camera(&mut s);
    let global = s.spawn("Global Light 2D", None);
    s.add(global, Light2D::global([1.0, 0.97, 0.92], 1.0));
    sky(&mut s, cam);

    let grid = s.spawn("Grid", None);
    s.add(grid, Grid::default());
    tilemap(&mut s, grid, "Stone Wall", art::wall_map(), BACKGROUND, -10);
    let ground = tilemap(&mut s, grid, "Ground", art::ground_map(), "Default", 0);
    s.add(ground, TilemapCollider2D::default());
    s.add(ground, ShadowCaster2D::default());
    let water = tilemap(&mut s, grid, "Water", art::water_map(), FOREGROUND, 6);
    s.world.get::<&mut Tilemap>(water).unwrap().color = [1.0, 1.0, 1.0, 0.9];

    let torches = s.spawn("Torches", None);
    for (i, p) in TORCHES.into_iter().enumerate() {
        torch(&mut s, torches, i + 1, p);
    }
    let coins = s.spawn("Coins", None);
    for (i, p) in COINS.into_iter().enumerate() {
        coin(&mut s, coins, i + 1, p);
    }
    player(&mut s);
    hud(&mut s);
    let volume = s.spawn(VOLUME, None);
    s.add(volume, glow_volume());
    s
}
