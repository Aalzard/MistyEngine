//! FoliageLightingDemo sample scene in `SampleProject`: a ~2 × 2 m patch of dense meadow grass
//! (the MeadowDemo grass system: tufts, seed heads, translucent blades) on bare soil, thinning
//! out to sparse tufts at its edges, with a few pebbles, seen from a low macro camera
//! (0.45 m above the ground) against a low golden-hour sun behind the grass, so the blades
//! glow with transmitted light. A second, wider camera (off by default) frames the whole
//! patch. It is the look-development target for "realistic foliage lighting" (see
//! `docs/pending/gfx-foliage.md`); the soil textures are generated tileable PNGs under
//! `Assets/Textures/Foliage`.
//!
//! Regenerate with `cargo test -p misty_editor --release -- --ignored write_foliage_lighting_demo`;
//! `render_foliage_lighting` renders the comparison images into `docs/screenshots/gfx/`
//! (`MISTY_GFX_STEP` names the step) and `foliage_lighting_frame_time` measures the GPU time.

use std::path::PathBuf;

use glam::{Quat, Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::render_scene::GameView;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    Camera, Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene,
    ScreenSpaceQuality, Shadows, Terrain, TerrainCollider, TonemapLook, TonemapMode, Transform,
};
use misty_engine::terrain::{TerrainData, TerrainLayer, details, noise};

use crate::sample_terrain2::{rgb, tile_fbm};

const DATA: &str = "Assets/Terrains/FoliageLightingDemo.terrain";
/// The overlay of taller grass clumps (same ground).
const DATA_TALL: &str = "Assets/Terrains/FoliageLightingDemo Tall Grass.terrain";
const SCENE: &str = "Assets/Scenes/FoliageLightingDemo.scene";
const SOIL: &str = "Assets/Textures/Foliage/Soil.png";
const SOIL_NORMAL: &str = "Assets/Textures/Foliage/SoilNormal.png";
const SOIL_HEIGHT: &str = "Assets/Textures/Foliage/SoilHeight.png";
const PEBBLE: &str = "Assets/Materials/Foliage/Pebble.mat";
/// `LITTER` darkened: the ground under the canopy (the grass casts no shadow-map shadows, so
/// the shade of the blades above is painted into the layer).
const SHADED_LITTER: &str = "Assets/Textures/Foliage/ShadedLitter.png";
// Written by `sample_meadow` / `sample_terrain2`.
const LITTER: &str = "Assets/Textures/Meadow/MeadowGround.png";
const LITTER_NORMAL: &str = "Assets/Textures/Meadow/MeadowGroundNormal.png";
const ROCK: &str = "Assets/Textures/Terrain/Cliff.png";
const ROCK_NORMAL: &str = "Assets/Textures/Terrain/CliffNormal.png";

/// 40 m of ground around the patch (so the low camera sees soil to the haze, not an edge).
const SIZE: Vec3 = Vec3::new(40.0, 4.0, 40.0);
const CORNER: Vec3 = Vec3::new(-20.0, -1.0, -20.0);
/// Side of the generated soil textures (one tile is `SOIL_TILE` metres).
const SOIL_PX: u32 = 1024;
const SOIL_TILE: f32 = 1.2;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- soil textures -------------------------------------------------------------------------

/// Height (0..1) and sRGB colour of one soil texel: crumbly humus in lumps, damp darker
/// hollows, small pebbles and bits of dry straw.
fn soil_texel(x: u32, y: u32) -> (f32, [f32; 3]) {
    let n = SOIL_PX;
    let lumps = tile_fbm(x, y, n, 6, 5, 101);
    let crumbs = tile_fbm(x, y, n, 120, 2, 103);
    let grit = noise::hash(x as i32, y as i32, 107);
    // Clods: rounded tops between cracks (cellular noise, ~1.5 cm cells).
    let clod = clods(x, y, n, 80, 109);
    let mut h = 0.2 * lumps + 0.4 * clod + 0.12 * crumbs + 0.05 * grit;
    let damp = 1.0 - (lumps * 1.6 - 0.35).clamp(0.0, 1.0);
    let mut c = [80.0 - 24.0 * damp, 60.0 - 19.0 * damp, 44.0 - 13.0 * damp];
    // Cracks are darker (shade and moisture), clod tops a little dusty.
    let k = (0.55 + 0.55 * clod) * (0.85 + 0.25 * crumbs) + 0.1 * (grit - 0.5);
    c = c.map(|v| v * k);
    // Pebbles: in about a sixth of the 3 cm cells, mostly small (wrapping, so it tiles).
    let cells = 40i32;
    let cell = n as f32 / cells as f32;
    let (gx, gy) = ((x as f32 / cell) as i32, (y as f32 / cell) as i32);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (ix, iy) = ((gx + dx).rem_euclid(cells), (gy + dy).rem_euclid(cells));
            if noise::hash(ix, iy, 111) > 0.12 {
                continue;
            }
            let centre = Vec2::new(
                (gx + dx) as f32 + noise::hash(ix, iy, 113),
                (gy + dy) as f32 + noise::hash(ix, iy, 117),
            ) * cell;
            let r = cell * (0.08 + 0.42 * noise::hash(ix, iy, 119).powi(2));
            let stretch = 0.7 + 0.6 * noise::hash(ix, iy, 123);
            let d = Vec2::new(
                x as f32 + 0.5 - centre.x,
                (y as f32 + 0.5 - centre.y) * stretch,
            );
            // Irregular outline: the radius wobbles with the angle.
            let a = d.y.atan2(d.x);
            let wobble = 1.0
                + 0.18 * (a * 3.0 + 6.0 * noise::hash(ix, iy, 151)).sin()
                + 0.1 * (a * 5.0 + 6.0 * noise::hash(ix, iy, 157)).sin();
            let t = 1.0 - d.length() / (r * wobble);
            if t > 0.0 {
                let dome = (t * (2.0 - t)).sqrt();
                let hp = 0.55 + 0.35 * dome;
                if hp > h {
                    h = hp;
                    let tone = 0.75 + 0.5 * noise::hash(ix, iy, 127);
                    let warm = noise::hash(ix, iy, 131);
                    let stone = [
                        (118.0 + 30.0 * warm) * tone,
                        (106.0 + 14.0 * warm) * tone,
                        (92.0 - 6.0 * warm) * tone,
                    ];
                    // Dusty: part soil colour, darker at the rim.
                    c = [0, 1, 2].map(|i| (stone[i] * 0.5 + c[i] * 0.5) * (0.75 + 0.3 * dome));
                }
            }
        }
    }
    // Straw: a thin dry segment in about half the 12 cm cells.
    let cells = 10i32;
    let cell = n as f32 / cells as f32;
    let (gx, gy) = ((x as f32 / cell) as i32, (y as f32 / cell) as i32);
    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (ix, iy) = ((gx + dx).rem_euclid(cells), (gy + dy).rem_euclid(cells));
            if noise::hash(ix, iy, 149) > 0.45 {
                continue;
            }
            for k in 0..1u32 {
                let r = |salt: u32| noise::hash(ix, iy, 137 + k * 17 + salt);
                let a = Vec2::new((gx + dx) as f32 + r(0), (gy + dy) as f32 + r(1)) * cell;
                let angle = r(2) * std::f32::consts::TAU;
                let len = cell * (0.25 + 0.4 * r(3));
                let b = a + Vec2::from_angle(angle) * len;
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                let dist = (p - (a + ab * t)).length();
                let w = 0.7 + 0.6 * r(4);
                if dist < w {
                    let hs = 0.62 + 0.1 * (1.0 - dist / w);
                    if hs > h {
                        h = hs;
                        let tone = 0.85 + 0.3 * r(5);
                        c = [128.0 * tone, 106.0 * tone, 72.0 * tone];
                    }
                }
            }
        }
    }
    (h.clamp(0.0, 1.0), c)
}

/// Tileable cellular clods 0..1 over a `size` px image of `cells`² cells: 0 in the cracks
/// between cells, rising to rounded tops.
fn clods(x: u32, y: u32, size: u32, cells: i32, seed: u32) -> f32 {
    let cell = size as f32 / cells as f32;
    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / cell;
    let g = p.floor();
    let (mut f1, mut f2) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (g.x as i32 + dx, g.y as i32 + dy);
            let (ix, iy) = (gx.rem_euclid(cells), gy.rem_euclid(cells));
            let c = Vec2::new(
                gx as f32 + 0.15 + 0.7 * noise::hash(ix, iy, seed),
                gy as f32 + 0.15 + 0.7 * noise::hash(ix, iy, seed ^ 0x55),
            );
            let d = c.distance(p);
            if d < f1 {
                (f1, f2) = (d, f1);
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    let edge = ((f2 - f1) / 0.35).clamp(0.0, 1.0);
    (edge * (2.0 - edge)).sqrt()
}

fn soil_maps() -> (RgbaImage, RgbaImage, RgbaImage) {
    let n = SOIL_PX;
    let texels: Vec<(f32, [f32; 3])> = (0..n * n).map(|i| soil_texel(i % n, i / n)).collect();
    let at = |x: i32, y: i32| {
        texels[(y.rem_euclid(n as i32) * n as i32 + x.rem_euclid(n as i32)) as usize]
    };
    let albedo = RgbaImage::from_fn(n, n, |x, y| rgb(at(x as i32, y as i32).1, 1.0));
    let height = RgbaImage::from_fn(n, n, |x, y| {
        let v = (at(x as i32, y as i32).0 * 255.0).round() as u8;
        Rgba([v, v, v, 255])
    });
    let normal = RgbaImage::from_fn(n, n, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let dx = (at(x + 1, y).0 - at(x - 1, y).0) * 3.0;
        let dy = (at(x, y - 1).0 - at(x, y + 1).0) * 3.0;
        let v = Vec3::new(-dx, -dy, 1.0).normalize();
        let enc = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        Rgba([enc(v.x), enc(v.y), enc(v.z), 255])
    });
    (albedo, normal, height)
}

/// The meadow ground photo in the canopy's shade: darker and a little less saturated.
fn shaded_litter(root: &std::path::Path) -> RgbaImage {
    let mut img = misty_engine::image::open(root.join(LITTER))
        .unwrap()
        .to_rgba8();
    for p in img.pixels_mut() {
        let [r, g, b, a] = p.0.map(|c| c as f32);
        let grey = (r + g + b) / 3.0;
        let k = |c: f32| ((c * 0.8 + grey * 0.2) * 0.42).round().clamp(0.0, 255.0) as u8;
        *p = Rgba([k(r), k(g), k(b), a as u8]);
    }
    img
}

fn pebble() -> Material {
    Material {
        color: [1.0, 0.8, 0.6, 1.0],
        smoothness: 0.22,
        albedo_map: Some(ROCK.into()),
        normal_map: None,
        normal_scale: 1.0,
        tiling: [0.6, 0.6],
        ..Default::default()
    }
}

// ---- terrain -------------------------------------------------------------------------------

/// World (x, z) to terrain-local metres.
fn local(x: f32, z: f32) -> (f32, f32) {
    (x - CORNER.x, z - CORNER.z)
}

/// Normalised height at world (x, z): flat-ish soil with a gentle rise under the patch and a
/// slow roll farther out.
fn height(x: f32, z: f32) -> f32 {
    let r = (x * x + z * z).sqrt();
    let mound = 0.035 * (1.0 - (r / 1.8).clamp(0.0, 1.0)).powi(2);
    let bumps = 0.025 * noise::fbm(x, z, 0.9, 3, 5);
    let roll = 0.35 * noise::fbm(x, z, 14.0, 3, 9) * (r / 6.0).clamp(0.0, 1.0);
    ((1.0 + mound + bumps + roll) / SIZE.y).clamp(0.0, 1.0)
}

/// Distance-like coordinate of world (x, z) from the patch centre, with a wobbly outline.
fn patch_r(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z / 0.9);
    p.length() + 0.22 * noise::fbm(x, z, 0.5, 3, 21)
}

/// Grass density 0..1 at world (x, z): the dense patch, sparse tufts around it on bare soil
/// and, past the clearing, a thinner meadow.
pub fn grass_density(x: f32, z: f32) -> f32 {
    let r = patch_r(x, z);
    let dense = 1.0 - smoothstep(0.8, 1.3, r);
    let clumps = smoothstep(0.15, 0.55, noise::fbm(x, z, 0.3, 2, 23));
    let tufts = clumps * (1.0 - smoothstep(1.2, 2.4, r)) * 0.55;
    let meadow = smoothstep(0.0, 0.6, noise::fbm(x, z, 3.0, 3, 27) * 0.5 + 0.5)
        * smoothstep(4.0, 7.0, r + 1.5 * noise::fbm(x, z, 2.0, 2, 29));
    dense.max(tufts).max(meadow * 0.55)
}

/// Density 0..1 of the second grass layer (taller stems): a few clumps in the patch.
pub fn tall_density(x: f32, z: f32) -> f32 {
    let r = patch_r(x, z);
    let clumps = smoothstep(0.1, 0.45, noise::fbm(x, z, 0.35, 2, 31));
    clumps * (1.0 - smoothstep(0.6, 1.2, r))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn grass() -> details::DetailSettings {
    details::DetailSettings {
        enabled: true,
        // The density the demo was made and committed with (11cf089); `MAX_DENSITY` was raised
        // to 1500 later for MeadowDemo, which left this scene as it was.
        density: 600.0,
        draw_distance: 40.0,
        height: [0.1, 0.4],
        width: 0.018,
        healthy_color: [0.28, 0.5, 0.1],
        dry_color: [0.56, 0.56, 0.26],
        wind_strength: 0.12,
        wind_speed: 0.6,
        seed_heads: 0.1,
        seed_color: [0.82, 0.72, 0.5],
        clumping: 0.65,
        bend: 0.45,
        translucency: 1.0,
        tip_dryness: 0.2,
        lod_distance: 12.0,
        ..Default::default()
    }
}

/// The second grass layer: taller, straighter stems with more seed heads.
fn tall_grass() -> details::DetailSettings {
    details::DetailSettings {
        height: [0.25, 0.6],
        width: 0.016,
        seed_heads: 0.25,
        bend: 0.3,
        draw_distance: 20.0,
        ..grass()
    }
}

/// Which grass a terrain carries: the main ground with the patch and meadow, or the
/// overlay of taller clumps (same ground, drawn on top of it with its own scatter seed: the
/// grass system's density is capped per terrain).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Main,
    Tall,
}

fn terrain_data_of(layer: Layer) -> TerrainData {
    let mut d = TerrainData::new(257, SIZE, 512);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            let (x, z) = (ix as f32 * s.x + CORNER.x, iz as f32 * s.y + CORNER.z);
            d.set_height(ix, iz, height(x, z));
        }
    }
    d.layers = vec![
        TerrainLayer {
            name: "Soil".into(),
            diffuse: Some(SOIL.into()),
            normal_map: Some(SOIL_NORMAL.into()),
            height_map: Some(SOIL_HEIGHT.into()),
            normal_scale: 1.0,
            tile_size: [SOIL_TILE, SOIL_TILE],
            smoothness: 0.0,
            ..Default::default()
        },
        TerrainLayer {
            name: "Grass Litter".into(),
            diffuse: Some(SHADED_LITTER.into()),
            normal_map: Some(LITTER_NORMAL.into()),
            normal_scale: 0.5,
            tile_size: [1.6, 1.6],
            smoothness: 0.0,
            ..Default::default()
        },
    ];
    let (seed, settings, density): (u32, _, fn(f32, f32) -> f32) = match layer {
        Layer::Main => (11, grass(), grass_density),
        Layer::Tall => (29, tall_grass(), tall_density),
    };
    d.rule_seed = seed;
    d.details = settings;
    // Hand-made maps (no rules): litter under the grass, the grass density from
    // `grass_density` / `tall_density`.
    let n = d.alphamap_resolution;
    let texel = Vec2::new(SIZE.x, SIZE.z) / n as f32;
    d.detail_map = vec![0; n * n];
    for tz in 0..n {
        for tx in 0..n {
            let x = (tx as f32 + 0.5) * texel.x + CORNER.x;
            let z = (tz as f32 + 0.5) * texel.y + CORNER.z;
            d.detail_map[tz * n + tx] = (density(x, z) * 255.0).round() as u8;
            let litter = (smoothstep(0.05, 0.5, grass_density(x, z)) * 255.0).round() as u8;
            d.alphamap[tz * n + tx] = [255 - litter, litter, 0, 0];
        }
    }
    d
}

fn terrain_data() -> TerrainData {
    terrain_data_of(Layer::Main)
}

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    let (lx, lz) = local(x, z);
    CORNER.y + d.sample_height(lx, lz)
}

// ---- scene ---------------------------------------------------------------------------------

/// Lighting set-ups of the comparison renders (the scene file is `Golden`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// Low warm sun behind the grass, every effect on.
    Golden,
    /// High sun from the side.
    Midday,
    /// The golden-hour sun with the screen-space effects, fog and bloom off (ACES).
    EffectsOff,
}

/// The sun as (elevation, azimuth of the light's travel direction) in degrees, colour and
/// intensity.
fn sun(look: Look) -> ([f32; 2], [f32; 3], f32) {
    match look {
        // From ahead and a little right of the camera, 12° up: the patch is backlit.
        Look::Golden | Look::EffectsOff => ([12.0, 195.0], [1.0, 0.74, 0.48], 3.2),
        Look::Midday => ([58.0, 262.0], [1.0, 0.97, 0.92], 3.2),
    }
}

fn camera(s: &mut Scene, name: &str, eye: Vec3, euler: Vec3, fov: f32, enabled: bool) {
    let e = match s.find_by_name(name) {
        Some(e) => e,
        None => {
            let e = s.spawn(name, None);
            s.add(e, Camera::default());
            e
        }
    };
    s.set_transform(e, Transform::from_position(eye).with_euler(euler));
    let mut c = s.world.get::<&mut Camera>(e).unwrap();
    c.fov = fov;
    c.near = 0.02;
    c.far = 600.0;
    c.enabled = enabled;
}

fn pebbles(s: &mut Scene, d: &TerrainData) {
    let parent = s.spawn("Pebbles", None);
    // (x, z, size x, y, z, yaw): partly sunk into the soil.
    let stones = [
        (0.38, -0.92, 0.11, 0.055, 0.08, 25.0),
        (0.52, -0.82, 0.05, 0.03, 0.045, -40.0),
        (-0.55, -1.05, 0.07, 0.04, 0.06, 70.0),
        (-1.15, 0.2, 0.14, 0.07, 0.1, 10.0),
        (1.2, 0.55, 0.09, 0.05, 0.07, -15.0),
    ];
    for (i, (x, z, sx, sy, sz, yaw)) in stones.into_iter().enumerate() {
        let e = s.spawn(&format!("Pebble {}", i + 1), Some(parent));
        let y = ground(d, x, z) + sy * 0.2;
        s.set_transform(
            e,
            Transform {
                position: Vec3::new(x, y, z),
                rotation: Quat::from_rotation_y((yaw as f32).to_radians())
                    * Quat::from_rotation_z(0.15 * (i as f32 - 2.0)),
                scale: Vec3::new(sx, sy, sz),
                ..Default::default()
            },
        );
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Sphere));
        s.add(
            e,
            MeshRenderer {
                material: Some(PEBBLE.into()),
                ..Default::default()
            },
        );
    }
}

fn volume(look: Look) -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Agx;
    v.tonemapping.look = TonemapLook::Punchy;
    v.bloom.active = true;
    v.bloom.intensity = 0.25;
    v.bloom.threshold = 1.0;
    v.bloom.scatter = 0.65;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = -0.4;
    // Warmer white balance: the low sky reads less violet.
    v.color_adjustments.temperature = 10.0;
    v.vignette.active = true;
    v.vignette.intensity = 0.18;
    v.vignette.smoothness = 0.35;
    let ao = &mut v.ambient_occlusion;
    ao.active = true;
    ao.intensity = 1.6;
    ao.radius = 0.3;
    ao.direct_lighting_strength = 0.25;
    ao.step_count = 8;
    ao.full_resolution = true;
    ao.temporal = true;
    let cs = &mut v.contact_shadows;
    cs.active = true;
    cs.length = 0.1;
    cs.distance_fade = 12.0;
    cs.opacity = 0.9;
    cs.sample_count = 16;
    let gi = &mut v.screen_space_gi;
    gi.active = true;
    gi.intensity = 1.0;
    gi.radius = 0.8;
    gi.thickness = 0.08;
    gi.quality = ScreenSpaceQuality::Medium;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 800.0;
    f.attenuation_distance = 1200.0;
    f.base_height = 0.0;
    f.maximum_height = 6.0;
    f.anisotropy = 0.55;
    f.albedo = [1.0, 0.95, 0.88];
    f.volumetric_distance = 40.0;
    if look == Look::EffectsOff {
        v.tonemapping.mode = TonemapMode::Aces;
        v.bloom.active = false;
        v.vignette.active = false;
        v.ambient_occlusion.active = false;
        v.contact_shadows.active = false;
        v.screen_space_gi.active = false;
        v.fog.active = false;
    }
    v
}

/// The demo scene lit for `look` (only `Golden` is saved).
pub fn scene(d: &TerrainData, look: Look) -> Scene {
    let mut s = Scene::default_3d("FoliageLightingDemo");
    // Macro: 0.45 m above the soil just outside the patch, looking across it towards the sun.
    let eye = Vec3::new(0.12, 0.0, -1.5);
    let eye = eye + Vec3::Y * (ground(d, eye.x, eye.z) + 0.45);
    camera(
        &mut s,
        "Main Camera",
        eye,
        Vec3::new(7.0, -3.0, 0.0),
        40.0,
        true,
    );
    // Wide: the whole patch from above and to the left (enable it instead to look).
    let wide = Vec3::new(-2.3, 0.0, -2.9);
    let wide = wide + Vec3::Y * (ground(d, wide.x, wide.z) + 1.6);
    camera(
        &mut s,
        "Wide Camera",
        wide,
        Vec3::new(24.0, 38.0, 0.0),
        42.0,
        false,
    );
    let ([elevation, azimuth], color, intensity) = sun(look);
    if let Some(e) = s.find_by_name("Directional Light") {
        s.set_transform(
            e,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(elevation, azimuth, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(e).unwrap();
        l.color = color;
        l.intensity = intensity;
        l.shadows = match look {
            Look::EffectsOff => Shadows::Hard,
            _ => Shadows::Soft,
        };
        l.contact_shadows = true;
    }
    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.04;
    rs.skybox.atmosphere_thickness = 1.0;
    // Light from below is the sunlit soil's bounce (warm), not the default grey ground.
    rs.skybox.ground = [0.42, 0.31, 0.21];
    // The procedural sky dims its ambient with the low sun; lift it so faces turned away
    // from the sun (pebbles, the backs of blades) keep some sky and bounce light.
    rs.environment_lighting.intensity = 1.3;
    let terrain = s.spawn("Terrain", None);
    s.set_transform(terrain, Transform::from_position(CORNER));
    s.add(
        terrain,
        Terrain {
            terrain_data: DATA.into(),
            ..Default::default()
        },
    );
    s.add(terrain, TerrainCollider::default());
    // Same ground (hidden under the main terrain's identical surface), only for the taller
    // grass clumps.
    let tall = s.spawn("Tall Grass", None);
    s.set_transform(tall, Transform::from_position(CORNER));
    s.add(
        tall,
        Terrain {
            terrain_data: DATA_TALL.into(),
            cast_shadows: false,
            ..Default::default()
        },
    );
    pebbles(&mut s, d);
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume(look));
    s
}

fn written_assets() -> Vec<&'static str> {
    vec![
        "Assets/Textures/Foliage",
        SOIL,
        SOIL_NORMAL,
        SOIL_HEIGHT,
        SHADED_LITTER,
        "Assets/Materials/Foliage",
        PEBBLE,
        DATA,
        DATA_TALL,
        SCENE,
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose (use --release) to regenerate"]
fn write_foliage_lighting_demo() {
    let root = project();
    std::fs::create_dir_all(root.join(SOIL).parent().unwrap()).unwrap();
    let (albedo, normal, height) = soil_maps();
    albedo.save(root.join(SOIL)).unwrap();
    normal.save(root.join(SOIL_NORMAL)).unwrap();
    height.save(root.join(SOIL_HEIGHT)).unwrap();
    shaded_litter(&root).save(root.join(SHADED_LITTER)).unwrap();
    std::fs::create_dir_all(root.join(PEBBLE).parent().unwrap()).unwrap();
    pebble().save(&root.join(PEBBLE)).unwrap();
    let d = terrain_data();
    d.save(&root.join(DATA)).unwrap();
    terrain_data_of(Layer::Tall)
        .save(&root.join(DATA_TALL))
        .unwrap();
    scene(&d, Look::Golden).save(&root.join(SCENE)).unwrap();
    for rel in written_assets() {
        let path = root.join(rel);
        if misty_engine::asset_db::meta::read_guid(&path).is_none() {
            misty_engine::asset_db::meta::write_guid(&path, misty_engine::asset_db::Guid::new())
                .unwrap();
        }
    }
}

#[test]
fn foliage_lighting_demo_matches_its_generator() {
    let root = project();
    for rel in [
        SOIL,
        SOIL_NORMAL,
        SOIL_HEIGHT,
        LITTER,
        LITTER_NORMAL,
        ROCK,
        ROCK_NORMAL,
    ] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    for rel in written_assets() {
        assert!(
            misty_engine::asset_db::meta::read_guid(&root.join(rel)).is_some(),
            "{rel}: no .meta"
        );
    }
    assert_eq!(Material::load(&root.join(PEBBLE)).unwrap(), pebble());
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.size, generated.size);
    assert_eq!(on_disk.detail_map, generated.detail_map);
    assert_eq!(on_disk.alphamap, generated.alphamap);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let tall = TerrainData::load(&root.join(DATA_TALL)).unwrap();
    let tall_generated = terrain_data_of(Layer::Tall);
    assert_eq!(tall.details, tall_generated.details);
    assert_eq!(tall.detail_map, tall_generated.detail_map);
    // The overlay's ground is exactly the main one (hidden under it).
    assert_eq!(tall.heights, on_disk.heights);
    assert_eq!(tall.alphamap, on_disk.alphamap);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == scene(&generated, Look::Golden).to_json(),
        "regenerate with write_foliage_lighting_demo"
    );
    let (loaded, report) = Scene::from_json("FoliageLightingDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The macro camera renders; the wide one is there to switch to.
    let (cam, _) = misty_engine::render_scene::game_camera(&loaded).unwrap();
    assert!(cam.position.y - ground(&generated, cam.position.x, cam.position.z) < 0.8);
    assert!(loaded.find_by_name("Wide Camera").is_some());
    // A dense 2 × 2 m patch that thins out to bare soil.
    let centre = Vec2::new(-CORNER.x, -CORNER.z);
    let dense = details::scatter(&generated, centre - 0.5, centre + 0.5);
    assert!(dense.len() > 400, "{}", dense.len());
    let edge = details::scatter(
        &generated,
        centre + Vec2::new(1.6, -0.5),
        centre + Vec2::new(2.6, 0.5),
    );
    assert!(
        edge.len() < dense.len() / 3,
        "{} vs {}",
        edge.len(),
        dense.len()
    );
    assert!(dense.iter().any(|b| b.extra[0] > 0.5), "seed heads");
}

// ---- comparison renders --------------------------------------------------------------------

pub const WIDTH: u32 = 1600;
pub const HEIGHT: u32 = 900;

type Gpu = (misty_engine::wgpu::Device, misty_engine::wgpu::Queue);

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// Views of the demo through `SampleProject`'s assets (the terrain from the generator).
struct Demo {
    assets: misty_engine::assets::AssetCache,
    data: TerrainData,
}

impl Demo {
    fn open() -> Self {
        let root = project();
        misty_engine::assets::set_project_root(&root);
        let data = terrain_data();
        // `MISTY_GFX_CALM=1`: no wind (to tell flicker from blade motion).
        let calm = std::env::var("MISTY_GFX_CALM").is_ok_and(|v| v == "1");
        for (path, layer) in [(DATA, Layer::Main), (DATA_TALL, Layer::Tall)] {
            let mut d = terrain_data_of(layer);
            if calm {
                d.details.wind_strength = 0.0;
            }
            misty_engine::terrain::store::insert(path, d);
        }
        Self {
            assets: misty_engine::assets::AssetCache::open(&root),
            data,
        }
    }

    fn view(&self, look: Look, wide: bool) -> GameView {
        let s = scene(&self.data, look);
        if wide {
            for (name, on) in [("Main Camera", false), ("Wide Camera", true)] {
                let e = s.find_by_name(name).unwrap();
                s.world.get::<&mut Camera>(e).unwrap().enabled = on;
            }
        }
        GameView::new(&s, &self.assets).unwrap()
    }
}

/// The quality level of a look: High, or for the baseline High without soft shadows, their
/// temporal denoise and TAA.
fn quality(look: Look) -> misty_engine::quality::QualityLevel {
    use misty_engine::quality::{QualityLevel, Tier};
    let mut q = QualityLevel::high();
    if let Ok(name) = std::env::var("MISTY_GFX_QUALITY") {
        q = match name.as_str() {
            "medium" => QualityLevel::medium(),
            "ultra" => QualityLevel::ultra(),
            _ => q,
        };
    }
    if look == Look::EffectsOff {
        q.soft_shadows = Tier::Off;
        q.shadow_temporal = false;
        q.shadow_denoise = false;
        q.temporal_aa = false;
    }
    q
}

fn render_rgba(gpu: &Gpu, view: &GameView, look: Look, frames: u32) -> Vec<u8> {
    let (device, queue) = gpu;
    let mut renderer = misty_engine::render::Renderer::new(device);
    renderer.set_quality(device, &quality(look));
    let target = renderer.create_target(device, WIDTH, HEIGHT);
    let mut frame = view.frame();
    // `MISTY_GFX_OFF=fog,ssgi,ao,contact,bloom`: turn effects off to isolate one (debugging).
    let off = std::env::var("MISTY_GFX_OFF").unwrap_or_default();
    let off = |name: &str| off.split(',').any(|o| o.trim() == name);
    frame.post.fog.volume.active &= !off("fog");
    frame.post.ssgi.enabled &= !off("ssgi");
    frame.post.ao.enabled &= !off("ao");
    frame.post.contact_shadows.enabled &= !off("contact");
    if off("bloom") {
        frame.post.bloom_intensity = 0.0;
    }
    // `MISTY_GFX_SSGI=4`: override the SSGI intensity (debugging).
    if let Some(k) = std::env::var("MISTY_GFX_SSGI")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        frame.post.ssgi.intensity = k;
    }
    for _ in 0..frames.max(1) {
        renderer.render(device, queue, &target, &frame);
    }
    let image = crate::project_thumbnail::read_pixels(device, queue, &target);
    // Frame-to-frame stability: one more frame, how many pixels jump by more than 8 levels
    // (TAA jitter / noise flicker; the slow wind moves blades by far less in one frame).
    renderer.render(device, queue, &target, &frame);
    let next = crate::project_thumbnail::read_pixels(device, queue, &target);
    let jumps = image
        .chunks(4)
        .zip(next.chunks(4))
        .filter(|(a, b)| (0..3).any(|i| (a[i] as i32 - b[i] as i32).abs() > 8))
        .count();
    eprintln!(
        "  flicker: {:.2} % of pixels change by > 8 levels in the next frame",
        jumps as f64 * 100.0 / (image.len() / 4) as f64
    );
    image
}

fn save_png(path: &std::path::Path, rgba: Vec<u8>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    RgbaImage::from_raw(WIDTH, HEIGHT, rgba)
        .unwrap()
        .save(path)
        .unwrap();
}

/// Mean sRGB brightness of the top and bottom halves, printed for the step log.
fn summary(label: &str, rgba: &[u8]) {
    let half = (WIDTH * HEIGHT / 2) as usize;
    let mean = |px: &[u8]| {
        let sum: f64 = px
            .chunks(4)
            .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
            .sum();
        sum / (px.len() / 4) as f64
    };
    eprintln!(
        "{label}: top {:.0}, bottom {:.0}",
        mean(&rgba[..half * 4]),
        mean(&rgba[half * 4..])
    );
}

/// Renders `docs/screenshots/gfx/foliage_<MISTY_GFX_STEP>_<look>.png` at 1600 × 900: the
/// backlit golden hour (main), the side-lit midday, the effects-off baseline and the wide
/// golden-hour framing. `MISTY_GFX_LOOKS=golden,wide` renders only some of them.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn render_foliage_lighting() {
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let only = std::env::var("MISTY_GFX_LOOKS").unwrap_or_default();
    let demo = Demo::open();
    let shots = [
        ("golden", Look::Golden, false),
        ("midday", Look::Midday, false),
        ("off", Look::EffectsOff, false),
        ("wide", Look::Golden, true),
    ];
    for (name, look, wide) in shots {
        if !only.is_empty() && !only.split(',').any(|o| o.trim() == name) {
            continue;
        }
        let view = demo.view(look, wide);
        let rgba = render_rgba(&gpu, &view, look, 32);
        summary(name, &rgba);
        save_png(
            &screenshots().join(format!("foliage_{step}_{name}.png")),
            rgba,
        );
    }
}

/// GPU time of the main (golden-hour) shot at 1920 × 1080 on the High and Medium levels.
#[test]
#[ignore = "benchmark; run on purpose with --release"]
fn foliage_lighting_frame_time() {
    use misty_engine::quality::QualityLevel;
    let gpu = crate::sample_render::headless_timed().expect("a GPU adapter");
    let (device, queue) = (&gpu.0, &gpu.1);
    let demo = Demo::open();
    let view = demo.view(Look::Golden, false);
    for (name, q) in [
        ("High", QualityLevel::high()),
        ("Medium", QualityLevel::medium()),
    ] {
        let mut r = misty_engine::render::Renderer::with_queue(device, queue);
        r.set_quality(device, &q);
        let target = r.create_target(device, 1920, 1080);
        let frame = view.frame();
        let (mut times, frames) = (Vec::new(), 60);
        for i in 0..frames + 15 {
            r.render(device, queue, &target, &frame);
            let _ = device.poll(misty_engine::wgpu::PollType::wait_indefinitely());
            if i >= 15
                && let Some(ms) = r.gpu_ms()
            {
                times.push(ms);
            }
        }
        if times.is_empty() {
            eprintln!("{name}: no GPU timestamps on this adapter");
            continue;
        }
        times.sort_by(f32::total_cmp);
        let mean = times.iter().sum::<f32>() / times.len() as f32;
        eprintln!(
            "{name}: GPU mean {mean:.2} ms, median {:.2} ms ({} frames)",
            times[times.len() / 2],
            times.len()
        );
    }
}
