use std::path::PathBuf;

use glam::{Mat4, Vec2, Vec3};
use image::{Rgba, RgbaImage};

use super::*;
use crate::render::surface::draw_order;
use crate::scene::{Scene, SpriteRenderer, Transform};

/// A fresh folder with a `w`×`h` PNG; returns the image path as a string.
fn texture(test: &str, w: u32, h: u32) -> String {
    let dir = std::env::temp_dir().join(format!("misty_sprites_{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Sheet.png");
    RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
        .save(&path)
        .unwrap();
    path.to_string_lossy().replace('\\', "/")
}

fn image_path(rel: &str) -> PathBuf {
    PathBuf::from(rel)
}

#[test]
fn import_settings_round_trip_and_default_without_meta() {
    let tex = texture("round_trip", 8, 8);
    let path = image_path(&tex);
    assert!(TextureImporter::try_load(&path).is_err(), "no .meta yet");
    assert_eq!(TextureImporter::load(&path), TextureImporter::default());
    assert_eq!(importer(&tex).texture_type, TextureType::Default);

    let s = TextureImporter {
        sprite_mode: SpriteMode::Multiple,
        pixels_per_unit: 16.0,
        filter_mode: FilterMode::Nearest,
        sprites: vec![SpriteRect {
            name: "A".into(),
            rect: [0.0, 0.0, 4.0, 4.0],
            pivot: [0.5, 0.0],
        }],
        ..TextureImporter::sprite()
    };
    s.save(&path).unwrap();
    assert!(TextureImporter::meta_path(&path).is_file());
    assert_eq!(TextureImporter::load(&path), s);
    invalidate(&tex);
    assert_eq!(importer(&tex), s);

    // Bad values are repaired, missing fields use defaults.
    std::fs::write(
        TextureImporter::meta_path(&path),
        r#"{ "texture_type": "Sprite", "pixels_per_unit": -3 }"#,
    )
    .unwrap();
    let fixed = TextureImporter::load(&path);
    assert_eq!(fixed.pixels_per_unit, 100.0);
    assert!(fixed.is_sprite() && fixed.generate_mip_maps);
}

/// `.meta` files written before Max Size / Aniso / sRGB / Trilinear existed.
#[test]
fn old_meta_files_load_and_point_is_nearest_neighbor() {
    let old = r#"{
  "texture_type": "Sprite",
  "sprite_mode": "Single",
  "pixels_per_unit": 16.0,
  "pivot": [0.5, 0.5],
  "filter_mode": "Point",
  "wrap_mode": "Clamp",
  "generate_mip_maps": false,
  "sprites": []
}"#;
    let s: TextureImporter = serde_json::from_str(old).unwrap();
    assert_eq!(s.filter_mode, FilterMode::Nearest);
    assert_eq!(s.max_size, 2048);
    assert_eq!(s.aniso_level, 1);
    assert!(s.is_srgb());
    let bilinear: TextureImporter =
        serde_json::from_str(r#"{ "filter_mode": "Bilinear" }"#).unwrap();
    assert_eq!(bilinear.filter_mode, FilterMode::Bilinear);
    // New files say "Nearest"; no file at all is trilinear with mips.
    let text = serde_json::to_string(&s).unwrap();
    assert!(text.contains("\"Nearest\""));
    assert_eq!(
        TextureImporter::default().filter_mode,
        FilterMode::Trilinear
    );
}

#[test]
fn texture_types_set_mips_and_color_space() {
    let mut s = TextureImporter::default();
    s.set_type(TextureType::NormalMap);
    assert!(!s.is_srgb() && s.generate_mip_maps);
    s.set_type(TextureType::Sprite);
    assert!(!s.generate_mip_maps && s.wrap_mode == WrapMode::Clamp);
    s.set_type(TextureType::Default);
    assert!(s.generate_mip_maps);
    let mut odd = TextureImporter {
        max_size: 300,
        aniso_level: 99,
        ..Default::default()
    };
    odd.sanitize();
    assert_eq!((odd.max_size, odd.aniso_level), (512, 16));
}

#[test]
fn pixels_per_unit_sets_the_world_size() {
    let tex = texture("ppu", 64, 32);
    let path = image_path(&tex);
    let sprite = |ppu: f32, pivot: [f32; 2]| {
        TextureImporter {
            pixels_per_unit: ppu,
            pivot,
            ..TextureImporter::sprite()
        }
        .save(&path)
        .unwrap();
        invalidate(&tex);
        resolve(&tex).unwrap()
    };
    assert_eq!(sprite(32.0, [0.5, 0.5]).size(), Vec2::new(2.0, 1.0));
    let s = sprite(64.0, [0.5, 0.0]);
    assert_eq!(s.size(), Vec2::new(1.0, 0.5));
    // Bottom pivot: the sprite stands on the object's origin.
    assert_eq!(s.bounds(), (Vec2::new(-0.5, 0.0), Vec2::new(0.5, 0.5)));
    assert_eq!(s.texture_size, [64, 32]);
}

#[test]
fn multiple_mode_resolves_named_rects() {
    let tex = texture("multiple", 32, 16);
    let path = image_path(&tex);
    let grid = slice::GridSlice {
        cell: [16, 16],
        skip_empty: false,
        ..Default::default()
    };
    TextureImporter {
        sprite_mode: SpriteMode::Multiple,
        pixels_per_unit: 16.0,
        sprites: slice::grid(32, 16, &grid, "Sheet", None),
        ..TextureImporter::sprite()
    }
    .save(&path)
    .unwrap();
    invalidate(&tex);
    assert_eq!(
        references(&tex),
        vec![format!("{tex}#Sheet_0"), format!("{tex}#Sheet_1")]
    );
    let second = resolve(&format!("{tex}#Sheet_1")).unwrap();
    assert_eq!(second.rect, [16.0, 0.0, 16.0, 16.0]);
    assert_eq!(second.name, "Sheet_1");
    assert_eq!(second.size(), Vec2::ONE);
    assert!(resolve(&format!("{tex}#Missing")).is_none());
    assert_eq!(
        resolve(&tex).unwrap().name,
        "Sheet_0",
        "no name = first sprite"
    );
    assert_eq!(display_name(Some(&format!("{tex}#Sheet_1"))), "Sheet_1");
    assert_eq!(display_name(None), "None (Sprite)");
}

#[test]
fn uv_transform_maps_the_rect_and_flips() {
    let s = Sprite {
        texture: "t.png".into(),
        name: "t".into(),
        // Right half, bottom row of a 4×2 grid of 16 px cells.
        rect: [32.0, 0.0, 32.0, 16.0],
        pivot: Vec2::splat(0.5),
        pixels_per_unit: 16.0,
        texture_size: [64, 32],
    };
    let at = |(t, o): ([f32; 2], [f32; 2]), uv: Vec2| Vec2::from(t) * uv + Vec2::from(o);
    // Quad uv (0,0) = top-left of the sprite; image rows count down: bottom row starts at v=0.5.
    let plain = s.uv_transform(false, false);
    assert_eq!(at(plain, Vec2::ZERO), Vec2::new(0.5, 0.5));
    assert_eq!(at(plain, Vec2::ONE), Vec2::new(1.0, 1.0));
    let flipped = s.uv_transform(true, true);
    assert_eq!(at(flipped, Vec2::ZERO), Vec2::new(1.0, 1.0));
    assert_eq!(at(flipped, Vec2::ONE), Vec2::new(0.5, 0.5));
    assert_eq!(s.uv_rect(), (Vec2::new(0.5, 0.0), Vec2::new(1.0, 0.5)));
}

#[test]
fn sprite_renderers_draw_sorted_quads() {
    let square = builtin::path("Square");
    let mut scene = Scene::new("t");
    let add = |scene: &mut Scene, name: &str, z: f32, layer: &str, order: i32| {
        let e = scene.spawn(name, None);
        scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 0.0, z)));
        scene.add(
            e,
            SpriteRenderer {
                sprite: Some(square.clone()),
                color: [1.0, 0.0, 0.0, 1.0],
                flip_x: true,
                sorting_layer: layer.into(),
                order_in_layer: order,
                ..Default::default()
            },
        );
        e
    };
    let front = add(&mut scene, "Front", 0.0, "Default", 5);
    add(&mut scene, "Back", -1.0, "Default", 0);
    let hidden = add(&mut scene, "Hidden", 0.0, "Default", 0);
    scene
        .world
        .get::<&mut SpriteRenderer>(hidden)
        .unwrap()
        .enabled = false;
    let mut draws = Vec::new();
    render::draws(&scene, &|e| e == front, &mut draws);
    assert_eq!(draws.len(), 2, "disabled renderers are skipped");
    let d = &draws[0];
    assert!(d.outline && !draws[1].outline);
    assert!(d.material.unlit && d.material.double_sided && d.material.mode.is_blended());
    assert_eq!(d.material.color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(d.material.tiling[0], -1.0, "flip X mirrors the uvs");
    assert_eq!(d.model, Mat4::IDENTITY, "1×1 unit, centred pivot");
    assert!(!d.cast_shadows && !d.receive_shadows);
    assert_eq!(d.sorting.order, 5);
    // Back is nearer the camera (looking +Z from -10), but Front has the higher order.
    let order = draw_order(&draws, |p| p.z + 10.0);
    assert_eq!(order.transparent, vec![1, 0]);
    assert!(local_bounds(&scene, front).is_some());
    assert_eq!(
        world_corners(&scene, front).unwrap()[2],
        Vec3::new(0.5, 0.5, 0.0)
    );
}

#[test]
fn missing_sprites_are_not_drawn() {
    let mut scene = Scene::new("t");
    let e = scene.spawn("S", None);
    scene.add(
        e,
        SpriteRenderer {
            sprite: Some("Assets/Missing.png".into()),
            ..Default::default()
        },
    );
    let none = scene.spawn("None", None);
    scene.add(none, SpriteRenderer::default());
    let mut draws = Vec::new();
    render::draws(&scene, &|_| false, &mut draws);
    assert!(draws.is_empty());
}
