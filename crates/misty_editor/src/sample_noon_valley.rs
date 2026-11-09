//! NoonValley sample scene in `SampleProject`: a broad meadow in a green valley under a high
//! noon sun. The look comes from the sky rather than the sun: deep blue overhead, hazy towards
//! the horizon, big soft cumulus clouds, layers of distant hills fading into blue haze. The
//! meadow (MeadowDemo's grass, thick and uneven, with patches of cream and yellow flowers and
//! clumps of taller, paler grass) rolls up to a steep hillside on the right covered in packed
//! broadleaf forest, with pale limestone cliffs showing through and heavy trees on the ledges
//! above them; a treeline runs along the meadow's edge ahead, a few lone trees and bushes stand
//! in the open and small rocks lie in the grass.
//!
//! Everything is generated here: the terrain (`Assets/NoonValley/NoonValley.terrain`), the
//! limestone and forest floor textures, the forest tree recipes, the rocks' placement and the
//! scene. A First Person Player stands where the camera is. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_noon_valley`; the other tests check the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::{Quat, Vec2, Vec3};
use misty_engine::image::{self, RgbaImage};
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    CloudPreset, Light, PostProcessVolume, Scene, Shadows, Terrain, TerrainCollider, TonemapMode,
    Transform, WindZone,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::detail_layers::DetailLayer;
use misty_engine::terrain::flowers::FlowerSettings;
use misty_engine::terrain::trees::{TreeInstance, TreePrototype};
use misty_engine::terrain::{MAX_LAYERS, TerrainData, TerrainLayer, details, noise};
use misty_engine::trees::{CrownShape, TreeAsset, TreeWind};

const DIR: &str = "Assets/NoonValley";
const DATA: &str = "Assets/NoonValley/NoonValley.terrain";
const SCENE: &str = "Assets/Scenes/NoonValley.scene";
const LIMESTONE: &str = "Assets/NoonValley/Textures/Limestone.png";
const LIMESTONE_NORMAL: &str = "Assets/NoonValley/Textures/LimestoneNormal.png";
const LIMESTONE_HEIGHT: &str = "Assets/NoonValley/Textures/LimestoneHeight.png";
const FOREST_FLOOR: &str = "Assets/NoonValley/Textures/ForestFloor.png";
const FOREST_FLOOR_NORMAL: &str = "Assets/NoonValley/Textures/ForestFloorNormal.png";
// MeadowDemo's ground (soil and litter under the grass) and hill turf.
const GROUND: &str = "Assets/Textures/Meadow/MeadowThatch.png";
const GROUND_NORMAL: &str = "Assets/Textures/Meadow/MeadowThatchNormal.png";
const GROUND_HEIGHT: &str = "Assets/Textures/Meadow/MeadowThatchHeight.png";
const TURF: &str = "Assets/Textures/Terrain/Meadow.png";

/// 4 km of valley (4 m between height samples, like MeadowDemo).
const SIZE: Vec3 = Vec3::new(4096.0, 700.0, 4096.0);
/// Where the camera stands (terrain-local x, z), looking along +Z: the world origin.
const EYE: (f32, f32) = (1700.0, 1500.0);
const CORNER: Vec3 = Vec3::new(-EYE.0, 0.0, -EYE.1);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Terrain-local point relative to the camera: (right, ahead).
fn rel(x: f32, z: f32) -> Vec2 {
    Vec2::new(x - EYE.0, z - EYE.1)
}

/// Lateral distance past the foot of the forested hillside on the right (negative = in the
/// meadow). The foot runs ahead on the right and swings left in the distance.
fn hill_side(x: f32, z: f32) -> f32 {
    let p = rel(x, z);
    let foot = 150.0 - 0.16 * p.y - 0.00006 * p.y.max(0.0).powi(2)
        + 25.0 * noise::fbm(x, z, 260.0, 2, 41);
    p.x - foot
}

/// Distance to the distant ranges' bands: the ranges come closer behind the camera and to
/// the sides than ahead.
fn far_radius(p: Vec2) -> f32 {
    (p * Vec2::new(0.8, if p.y > 0.0 { 1.0 } else { 1.6 })).length()
}

/// Terrace steps (limestone ledges): `h` metres stepped every `step`, the rise squeezed into
/// the last part of each step (the cliff face), the rest nearly flat (a ledge).
fn terrace(h: f32, step: f32) -> f32 {
    let t = h / step;
    let base = t.floor();
    let f = t - base;
    let rise = smooth(0.62, 0.97, f);
    (base + 0.12 * f + 0.88 * rise) * step
}

/// Height in metres at terrain-local (x, z).
fn height_m(x: f32, z: f32) -> f32 {
    let p = rel(x, z);
    // The meadow: gently rolling, undulating ground, rising a little to the left.
    let rolling = noise::fbm(x, z, 220.0, 4, 3);
    let swells = noise::fbm(x, z, 60.0, 3, 8);
    let dips = noise::fbm(x, z, 24.0, 2, 13);
    let mut h = 40.0
        + 7.0 * rolling
        + 2.2 * swells
        + 0.6 * dips
        + 10.0 * smooth(-200.0, -900.0, p.x);
    // The hillside on the right: steep from its foot, rounding off towards a ridge 300 m up.
    let u = hill_side(x, z);
    let soft = 60.0 * (1.0 + (u / 60.0).exp()).ln();
    let ridge = 0.85 + 0.3 * noise::fbm(x, z, 700.0, 2, 17);
    let mut hill = 330.0 * ridge * (1.0 - (-soft / 330.0).exp());
    hill += 40.0 * noise::fbm(x, z, 180.0, 3, 19) * smooth(20.0, 200.0, u);
    // Limestone cliffs: ledges in patches on the slope.
    let cliffs = smooth(0.15, 0.35, noise::fbm(x, z, 340.0, 3, 23))
        * smooth(140.0, 220.0, u)
        * smooth(650.0, 500.0, u)
        * smooth(1100.0, 800.0, p.y);
    if cliffs > 0.0 {
        let step = 26.0 + 10.0 * noise::fbm(x, z, 500.0, 1, 29);
        hill += (terrace(hill, step) - hill) * cliffs;
    }
    h += hill;
    // Distant hills: three rounded, layered ranges ahead (1.5, 2.2 and 2.8 km), each higher
    // than the one before, closer behind; rock shows on the nearest.
    let r = far_radius(p);
    let band = |c: f32, w: f32| (-((r - c) / w).powi(2)).exp();
    let lump = |seed: u32| {
        (0.55 + 0.45 * noise::fbm(x, z, 420.0, 3, seed)).max(0.0)
            + 0.25 * noise::ridged(x, z, 260.0, 4, seed + 7)
    };
    // Behind the camera the ranges are closer, so lower (else too steep for the forest).
    let behind = 0.45 + 0.55 * smooth(-600.0, 300.0, p.y);
    let open_left = 0.35 + 0.65 * smooth(-900.0, -200.0, p.x);
    let near = 110.0 * band(1500.0, 300.0) * lump(31) * open_left;
    let rocky = smooth(0.4, 0.6, noise::fbm(x, z, 300.0, 2, 43)) * smooth(0.0, 300.0, p.y);
    let near = near + (terrace(near, 22.0) - near) * rocky * 0.5;
    h += (near
        + 190.0 * band(2200.0, 380.0) * lump(37)
        + 300.0 * smooth(2350.0, 2950.0, r) * lump(47))
        * behind;
    // Rims at the terrain's edges (nothing to see past them).
    let edge = x.min(z).min(SIZE.x - x).min(SIZE.z - z);
    h += 120.0 * smooth(400.0, 0.0, edge);
    h
}

/// Forest cover 0..1 at terrain-local (x, z): the hillside, the treeline ahead along the
/// meadow's edge, woods on the distant hills; none in the open meadow.
fn forest(x: f32, z: f32) -> f32 {
    let p = rel(x, z);
    let u = hill_side(x, z);
    // Ragged edge along the hill's foot; a few grassy clearings higher up.
    let edge = u + 30.0 * noise::fbm(x, z, 70.0, 3, 51);
    let clearings = smooth(0.35, 0.5, noise::fbm(x, z, 160.0, 3, 53));
    let mut f = smooth(-10.0, 25.0, edge)
        * smooth(800.0, 650.0, u)
        * smooth(-1100.0, -900.0, p.y)
        * smooth(2600.0, 2300.0, p.y)
        * (1.0 - clearings);
    // The treeline ahead: a wood on the meadow's far edge, joining the hill's foot.
    let ahead = smooth(330.0, 380.0, p.y + 50.0 * noise::fbm(x, z, 90.0, 3, 55))
        * smooth(700.0, 640.0, p.y)
        * smooth(-420.0, -320.0, p.x + 60.0 * noise::fbm(x, z, 120.0, 2, 57));
    f = f.max(ahead);
    // Woods on the far hills (their ring starts ~1.3 km out), patchy.
    let r = far_radius(p);
    let woods = smooth(1150.0, 1350.0, r)
        * smooth(3000.0, 2700.0, r)
        * smooth(-0.25, -0.15, noise::fbm(x, z, 300.0, 3, 59));
    f = f.max(woods);
    // Behind the camera, the far treeline 600 m back.
    let behind = smooth(-560.0, -640.0, p.y + 40.0 * noise::fbm(x, z, 80.0, 2, 61))
        * smooth(-1000.0, -900.0, p.y);
    f.max(behind)
}

fn rules(slope: [f32; 2], priority: i32) -> AutoPaintRules {
    AutoPaintRules {
        enabled: true,
        slope,
        spread: 0.3,
        breakup: 0.55,
        breakup_scale: 30.0,
        priority,
        ..Default::default()
    }
}

/// MeadowDemo's grass (same blades, density and wind), tuned to the noon meadow: greener and
/// a little taller, with more lush / dry contrast between patches.
fn grass() -> details::DetailSettings {
    details::DetailSettings {
        enabled: true,
        rules: AutoPaintRules {
            enabled: true,
            slope: [0.0, 26.0],
            spread: 0.25,
            breakup: 0.25,
            breakup_scale: 18.0,
            ..Default::default()
        },
        on_layer: Some(0),
        density: 800.0,
        draw_distance: 160.0,
        height: [0.07, 0.21],
        width: 0.04,
        healthy_color: [0.47, 0.63, 0.19],
        dry_color: [0.66, 0.66, 0.36],
        wind_strength: 0.18,
        wind_speed: 0.9,
        seed_heads: 0.005,
        seed_color: [0.82, 0.76, 0.55],
        clumping: 0.6,
        bend: 0.8,
        translucency: 0.6,
        tip_dryness: 0.4,
        lod_distance: 6.5,
        patch_scale: 30.0,
        lush_color: [0.16, 0.3, 0.07],
        lush_amount: 0.65,
        dry_amount: 0.45,
        color_variation: 0.5,
        height_variation: 0.55,
        density_variation: 0.25,
        ground_tint: 1.0,
        cast_shadows: true,
        wind_direction: 80.0,
        gust_strength: 0.55,
        gust_scale: 22.0,
        turbulence: 0.3,
    }
}

/// Small cream daisies and yellow buttercups in drifts across the meadow.
fn flowers() -> FlowerSettings {
    let kinds = FlowerSettings::wildflowers(crate::terrain_flowers_ui::FLOWER_DIR)
        .into_iter()
        .filter(|k| k.name == "Buttercup")
        .map(|mut k| {
            // Tiny heads: specks of cream and yellow in the turf.
            k.head_size *= 0.55;
            // Small heads close to the turf, as in a grazed meadow.
            k.height = [k.height[0] * 0.85, k.height[1] * 0.85];
            k
        })
        .collect();
    FlowerSettings {
        enabled: true,
        kinds,
        density: 90.0,
        coverage: 0.16,
        patch_scale: 9.0,
        scattered: 0.004,
        edges: 0.3,
        lod_distance: 30.0,
        map: Vec::new(),
    }
}

/// Clumps of taller, paler grass here and there in the turf.
fn tall_grass(d: &TerrainData) -> DetailLayer {
    let n = d.alphamap_resolution;
    let texel = d.size.x / n as f32;
    let map = (0..n * n)
        .map(|t| {
            let (x, z) = (
                (t % n) as f32 * texel + texel * 0.5,
                (t / n) as f32 * texel + texel * 0.5,
            );
            let clump = noise::fbm(x, z, 14.0, 2, 71) * 0.7 + noise::fbm(x, z, 60.0, 2, 73) * 0.3;
            (smooth(0.45, 0.62, clump) * 255.0).round() as u8
        })
        .collect();
    DetailLayer {
        name: "Tall Grass".into(),
        density: 35.0,
        height: [0.32, 0.55],
        width: 0.035,
        bend: 0.55,
        dryness: 0.6,
        tint: 0.3,
        seed_heads: 0.0,
        map,
        ..DetailLayer::grass("Tall Grass")
    }
}

/// Tileable value noise 0..1 with `cells` lattice cells across the tile.
fn tile_noise(u: f32, v: f32, cells: u32, seed: u32) -> f32 {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let c = cells as i32;
    let h = |a: i32, b: i32| noise::hash(a.rem_euclid(c), b.rem_euclid(c), seed);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * sx;
    let bottom = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// Tileable fractal noise 0..1.
fn tile_fbm(u: f32, v: f32, cells: u32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * tile_noise(u, v, cells << o, seed + o);
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// Albedo (sRGB), height and tangent-space normal from linear colour and height fields.
fn maps(n: usize, color: &[[f32; 3]], height: &[f32], strength: f32) -> [RgbaImage; 3] {
    let size = n as u32;
    let albedo = RgbaImage::from_fn(size, size, |x, y| {
        let c = color[y as usize * n + x as usize];
        image::Rgba([srgb(c[0]), srgb(c[1]), srgb(c[2]), 255])
    });
    let at = |x: i32, y: i32| {
        height[y.rem_euclid(n as i32) as usize * n + x.rem_euclid(n as i32) as usize]
    };
    let heights = RgbaImage::from_fn(size, size, |x, y| {
        let h = (at(x as i32, y as i32) * 255.0).round().clamp(0.0, 255.0) as u8;
        image::Rgba([h, h, h, 255])
    });
    let normal = RgbaImage::from_fn(size, size, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let ddx = (at(x + 1, y) - at(x - 1, y)) * strength;
        let ddy = (at(x, y + 1) - at(x, y - 1)) * strength;
        let v = Vec3::new(-ddx, ddy, 1.0).normalize();
        let enc = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        image::Rgba([enc(v.x), enc(v.y), enc(v.z), 255])
    });
    [albedo, heights, normal]
}

/// Side of the generated limestone and forest floor textures in pixels (they tile).
const TEX: usize = 1024;

/// Pale weathered limestone, as a cliff face seen from the side (v = down the face): bedding
/// planes across, joints down, blocky relief, grey water streaks under the ledges, lichen and
/// a little moss. Albedo (sRGB), height and normal.
fn limestone() -> [RgbaImage; 3] {
    let n = TEX;
    let mut color = vec![[0.0f32; 3]; n * n];
    let mut height = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            // Beds: uneven bands across, wavy.
            let wave = tile_fbm(u, v, 3, 2, 101) * 0.08;
            let bed_t = (v + wave) * 7.0;
            let bed = bed_t - bed_t.floor();
            let bed_id = bed_t.floor() as i32;
            let bed_tone = noise::hash(bed_id.rem_euclid(7), 3, 103);
            // Joints: vertical cracks, offset per bed.
            let ju = u * 9.0 + bed_tone * 3.7;
            let joint = (ju - ju.floor() - 0.5).abs();
            let crack = smooth(0.03, 0.0, joint) * (0.6 + 0.4 * tile_noise(u, v, 40, 105));
            let parting = smooth(0.05, 0.0, bed) + smooth(0.95, 1.0, bed) * 0.6;
            let blocks = tile_fbm(u, v, 6, 4, 107);
            let grain = tile_noise(u, v, 180, 109);
            // Water streaks running down from the beds (v grows down the face).
            let streak = (tile_fbm(u, 0.0, 24, 2, 111) - 0.45).max(0.0) * 2.5 * (1.0 - bed * 0.6);
            let lichen = smooth(0.62, 0.72, tile_fbm(u, v, 10, 3, 113));
            let moss = smooth(0.7, 0.8, tile_fbm(u, v, 7, 3, 117)) * smooth(0.6, 0.9, bed);
            let mut c = [0.34, 0.31, 0.26].map(|k| {
                k * (0.82 + 0.25 * bed_tone)
                    * (0.85 + 0.3 * blocks)
                    * (0.92 + 0.12 * grain)
                    * (1.0 - 0.45 * crack)
                    * (1.0 - 0.35 * parting)
                    * (1.0 - 0.4 * streak.min(1.0))
            });
            let lc = [0.33, 0.34, 0.27];
            let mc = [0.12, 0.16, 0.06];
            for i in 0..3 {
                c[i] += (lc[i] - c[i]) * lichen * 0.5;
                c[i] += (mc[i] - c[i]) * moss * 0.7;
            }
            color[y * n + x] = c;
            height[y * n + x] = (0.55 + 0.3 * blocks + 0.05 * grain - 0.35 * crack
                - 0.3 * parting
                + 0.1 * (1.0 - bed))
                .clamp(0.0, 1.0);
        }
    }
    maps(n, &color, &height, 6.0)
}

/// Forest floor under the canopy: dark soil, old leaves and moss. Albedo and normal.
fn forest_floor() -> [RgbaImage; 2] {
    let n = TEX;
    let mut color = vec![[0.0f32; 3]; n * n];
    let mut height = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            let big = tile_fbm(u, v, 5, 3, 201);
            let moss = smooth(0.55, 0.7, tile_fbm(u, v, 9, 3, 203));
            let soil = [0.07, 0.055, 0.035].map(|c| c * (0.75 + 0.5 * big));
            let m = [0.06, 0.09, 0.03];
            color[y * n + x] = std::array::from_fn(|i| soil[i] + (m[i] - soil[i]) * moss);
            height[y * n + x] = 0.3 * big + 0.1 * moss;
        }
    }
    // Fallen leaves, later ones on top.
    for k in 0..9000 {
        let r = |salt: u32| noise::hash(k, salt as i32, 0x1eaf);
        let (cx, cy) = (r(1) * n as f32, r(2) * n as f32);
        let (len, wid) = (5.0 + 7.0 * r(3), 2.5 + 3.0 * r(4));
        let a = r(5) * std::f32::consts::TAU;
        let tone = 0.7 + 0.6 * r(6);
        let base = if r(7) < 0.6 {
            [0.2, 0.14, 0.07]
        } else {
            [0.14, 0.13, 0.06]
        };
        let top = 0.4 + 0.6 * k as f32 / 9000.0;
        let reach = len.ceil() as i32 + 1;
        let (ca, sa) = (a.cos(), a.sin());
        for oy in -reach..=reach {
            for ox in -reach..=reach {
                let (lx, ly) = (
                    ox as f32 * ca + oy as f32 * sa,
                    -ox as f32 * sa + oy as f32 * ca,
                );
                let d = (lx / len).powi(2) + (ly / wid).powi(2);
                if d > 1.0 {
                    continue;
                }
                let px = (cx as i32 + ox).rem_euclid(n as i32) as usize;
                let py = (cy as i32 + oy).rem_euclid(n as i32) as usize;
                let i = py * n + px;
                let h = top + 0.05 * (1.0 - d);
                if h > height[i] {
                    height[i] = h;
                    color[i] = base.map(|c| c * tone * (1.0 - 0.3 * d));
                }
            }
        }
    }
    let [albedo, _, normal] = maps(n, &color, &height, 5.0);
    [albedo, normal]
}

fn tree_path(name: &str) -> String {
    format!("{DIR}/Trees/{name}.tree")
}

/// The valley's trees: file, recipe. 0–2 make the forest (tall, packed crowns), 3 is a bush,
/// 4 a low-crowned tree for the forest's edges, 5 a broad lone oak of the meadow.
fn tree_assets() -> Vec<(String, TreeAsset)> {
    let forest_wind = TreeWind {
        trunk_bend: 0.5,
        branch_sway: 0.7,
        leaf_flutter: 0.9,
    };
    vec![
        (
            tree_path("Forest Oak"),
            TreeAsset {
                seed: 21,
                height: 19.0,
                crown_shape: CrownShape::Round,
                crown_width: 0.66,
                crown_base: 0.1,
                trunk_radius: 0.36,
                leader: 0.35,
                branches: 11,
                branch_angle: 52.0,
                twigs: 12,
                leaf_density: 2.1,
                leaf_size: 1.6,
                leaf_color: [0.78, 0.84, 0.72],
                wind: forest_wind.clone(),
                ..TreeAsset::oak()
            },
        ),
        (
            tree_path("Forest Linden"),
            TreeAsset {
                seed: 23,
                height: 21.0,
                crown_width: 0.6,
                crown_base: 0.1,
                leaf_density: 2.1,
                leaf_size: 1.5,
                leaf_color: [0.8, 0.88, 0.74],
                wind: forest_wind.clone(),
                ..TreeAsset::linden()
            },
        ),
        (
            tree_path("Forest Beech"),
            TreeAsset {
                seed: 25,
                height: 17.0,
                crown_shape: CrownShape::Oval,
                crown_width: 0.7,
                crown_base: 0.09,
                trunk_radius: 0.3,
                leader: 0.5,
                branches: 12,
                twigs: 11,
                leaf_density: 2.2,
                leaf_size: 1.5,
                leaf_color: [0.7, 0.8, 0.62],
                wind: forest_wind,
                ..TreeAsset::oak()
            },
        ),
        (
            tree_path("Bush"),
            TreeAsset {
                seed: 27,
                height: 4.5,
                crown_shape: CrownShape::Round,
                crown_width: 1.1,
                crown_base: 0.06,
                trunk_radius: 0.08,
                leader: 0.2,
                branches: 9,
                branch_angle: 65.0,
                twigs: 8,
                leaf_density: 2.0,
                leaf_size: 0.9,
                leaf_color: [0.8, 0.86, 0.72],
                ..TreeAsset::oak()
            },
        ),
        (
            tree_path("Forest Edge"),
            TreeAsset {
                seed: 31,
                height: 13.0,
                crown_shape: CrownShape::Round,
                crown_width: 0.85,
                crown_base: 0.07,
                trunk_radius: 0.28,
                leader: 0.4,
                branches: 12,
                branch_angle: 64.0,
                twigs: 11,
                leaf_density: 2.2,
                leaf_size: 1.4,
                leaf_color: [0.76, 0.84, 0.7],
                ..TreeAsset::oak()
            },
        ),
        (
            tree_path("Meadow Oak"),
            TreeAsset {
                seed: 29,
                height: 15.0,
                crown_shape: CrownShape::Round,
                crown_width: 0.95,
                crown_base: 0.22,
                trunk_radius: 0.45,
                branches: 11,
                branch_angle: 60.0,
                twigs: 13,
                leaf_density: 1.9,
                leaf_size: 1.5,
                ..TreeAsset::oak()
            },
        ),
    ]
}

/// Lone trees and bushes in the meadow: (right, ahead) of the camera in metres, prototype,
/// scale.
const LONE: &[(f32, f32, u16, f32)] = &[
    // The big tree at the right edge of the first view.
    (46.0, 34.0, 5, 1.1),
    // A bush group and a tree mid-left, in the open.
    (-118.0, 300.0, 3, 1.2),
    (-108.0, 306.0, 3, 0.9),
    (-126.0, 296.0, 3, 0.8),
    (-40.0, 360.0, 5, 0.9),
    (-62.0, 340.0, 3, 1.0),
    // Bushes along the treeline.
    (30.0, 330.0, 3, 1.1),
    (90.0, 280.0, 3, 1.0),
    (-200.0, 350.0, 3, 1.2),
    // To the left and behind (seen when turning round).
    (-260.0, 120.0, 5, 1.0),
    (-300.0, 160.0, 3, 1.0),
    (-180.0, -160.0, 5, 1.15),
    (-150.0, -190.0, 3, 1.0),
    (120.0, -220.0, 3, 1.1),
    (-420.0, 520.0, 5, 1.0),
];

/// Every tree (deterministic): the forest on a jittered grid where [`forest`] says so (never
/// on cliff faces), then the lone trees.
fn trees(d: &TerrainData) -> Vec<TreeInstance> {
    let mut out = Vec::new();
    let size = Vec2::new(d.size.x, d.size.z);
    let cell = 8.0;
    let (nx, nz) = ((size.x / cell) as i32, (size.y / cell) as i32);
    for iz in 0..nz {
        for ix in 0..nx {
            let r = |salt: i32| noise::hash(ix * 7 + salt, iz, 0x7ee5);
            let p = Vec2::new(
                (ix as f32 + 0.15 + 0.7 * r(1)) * cell,
                (iz as f32 + 0.15 + 0.7 * r(2)) * cell,
            );
            if p.cmplt(Vec2::splat(12.0)).any() || p.cmpgt(size - 12.0).any() {
                continue;
            }
            let f = forest(p.x, p.y);
            if f <= 0.0 || r(3) > f * 0.97 {
                continue;
            }
            // Far away fewer, bigger trees keep the canopy closed: the share kept falls from
            // all of them within 600 m to a quarter at 2.4 km.
            let dist = rel(p.x, p.y).length();
            let keep = 1.0 - 0.75 * smooth(600.0, 2400.0, dist);
            if r(9) > keep {
                continue;
            }
            let grow = (1.0 / keep).sqrt().min(1.8);
            let slope = misty_engine::terrain::trees::slope_at(d, p.x, p.y);
            if slope > 42.0 {
                continue;
            }
            // The forest's edges: bushes and low crowns down to the ground.
            let kind = if f < 0.92 {
                if r(4) < 0.5 { 3 } else { 4 }
            } else {
                match r(4) {
                    k if k < 0.42 => 0,
                    k if k < 0.74 => 1,
                    k if k < 0.96 => 2,
                    _ => 3,
                }
            };
            // Bigger towards the far woods; edge trees a little lower.
            let edge = smooth(0.0, 0.6, f);
            let height = (0.8 + 0.45 * r(5)) * (0.85 + 0.15 * edge) * grow.powf(0.45);
            // Undergrowth within 700 m: a bush between the trunks of every few trees.
            if dist < 700.0 && r(10) < 0.3 {
                let b = p + Vec2::new(r(11) - 0.5, r(12) - 0.5) * cell * 0.8;
                out.push(TreeInstance {
                    position: b / size,
                    prototype: 3,
                    rotation: r(13) * std::f32::consts::TAU,
                    height_scale: 0.8 + 0.5 * r(14),
                    width_scale: 0.9 + 0.5 * r(15),
                    color: r(16),
                });
            }
            out.push(TreeInstance {
                position: p / size,
                prototype: kind,
                rotation: r(6) * std::f32::consts::TAU,
                height_scale: height,
                width_scale: (0.85 + 0.35 * r(7)) * height.powf(0.4) * grow.powf(0.9),
                color: r(8),
            });
        }
    }
    for (k, &(right, ahead, kind, scale)) in LONE.iter().enumerate() {
        let p = Vec2::new(EYE.0 + right, EYE.1 + ahead);
        let r = |salt: i32| noise::hash(k as i32, salt, 0x10e5);
        out.push(TreeInstance {
            position: p / size,
            prototype: kind,
            rotation: r(1) * std::f32::consts::TAU,
            height_scale: scale,
            width_scale: scale * (0.95 + 0.1 * r(2)),
            color: r(3),
        });
    }
    out
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(1025, SIZE, 512);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            let h = height_m(ix as f32 * s.x, iz as f32 * s.y) / SIZE.y;
            d.set_height(ix, iz, h.clamp(0.0, 1.0));
        }
    }
    d.layers = vec![
        TerrainLayer {
            name: "Meadow Ground".into(),
            diffuse: Some(GROUND.into()),
            normal_map: Some(GROUND_NORMAL.into()),
            height_map: Some(GROUND_HEIGHT.into()),
            normal_scale: 1.0,
            tile_size: [2.2, 2.2],
            smoothness: 0.05,
            rules: rules([0.0, 24.0], 0),
            ..Default::default()
        },
        TerrainLayer {
            name: "Hill Turf".into(),
            diffuse: Some(TURF.into()),
            tile_size: [6.0, 6.0],
            rules: rules([22.0, 46.0], 1),
            ..Default::default()
        },
        TerrainLayer {
            name: "Limestone".into(),
            diffuse: Some(LIMESTONE.into()),
            normal_map: Some(LIMESTONE_NORMAL.into()),
            height_map: Some(LIMESTONE_HEIGHT.into()),
            normal_scale: 1.2,
            tile_size: [24.0, 24.0],
            smoothness: 0.15,
            rules: rules([46.0, 90.0], 2),
            ..Default::default()
        },
        TerrainLayer {
            name: "Forest Floor".into(),
            diffuse: Some(FOREST_FLOOR.into()),
            normal_map: Some(FOREST_FLOOR_NORMAL.into()),
            tile_size: [3.0, 3.0],
            smoothness: 0.1,
            // Under the canopy: little sky light reaches the ground.
            occlusion: 0.3,
            ..Default::default()
        },
    ];
    d.rule_seed = 11;
    d.cliffs.triplanar_angle = 42.0;
    d.details = grass();
    d.flowers = flowers();
    auto_paint::generate(&mut d);
    // Under the forest the ground is leaf litter (and no grass grows there).
    let n = d.alphamap_resolution;
    let texel = SIZE.x / n as f32;
    for t in 0..n * n {
        let (x, z) = (
            (t % n) as f32 * texel + texel * 0.5,
            (t / n) as f32 * texel + texel * 0.5,
        );
        let f = smooth(0.2, 0.7, forest(x, z));
        if f <= 0.0 {
            continue;
        }
        let w = d.layer_weights(t);
        let mut out = [0u8; MAX_LAYERS];
        let soft = |v: u8| (v as f32 * (1.0 - f)).round() as u8;
        // Cliff faces stay rock.
        out[2] = w[2];
        out[0] = soft(w[0]);
        out[1] = soft(w[1]);
        out[3] = 255 - out[0] - out[1] - out[2];
        d.set_layer_weights(t, &out);
    }
    d.detail_layers = vec![tall_grass(&d)];
    details::generate(&mut d);
    d.trees.prototypes = tree_assets()
        .into_iter()
        .map(|(path, _)| TreePrototype { tree: path })
        .collect();
    d.trees.draw_distance = 4000.0;
    // Placed on the heights as stored (16 bits), so the file and the generator agree.
    let stored = TerrainData {
        heights: d
            .heights
            .iter()
            .map(|h| (h * 65535.0).round() / 65535.0)
            .collect(),
        ..d.clone()
    };
    d.trees.instances = trees(&stored);
    d
}

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

/// Rocks: a few small ones in the meadow, boulders along the hill's foot and big layered
/// outcrops on the slope (world space).
fn rocks(d: &TerrainData) -> Vec<crate::sample_rocks::Placed> {
    use crate::sample_rocks::{OUTCROP, Placed, ROUND, SMALL as ROCK_SMALL, SPLIT};
    let trees: Vec<Vec2> = d
        .trees
        .instances
        .iter()
        .map(|t| t.position * Vec2::new(SIZE.x, SIZE.z))
        .collect();
    let mut out = Vec::new();
    let mut taken: Vec<Vec2> = Vec::new();
    // (right, ahead, spread, count, kinds, scale)
    let groups: &[(f32, f32, f32, usize, &[usize], [f32; 2])] = &[
        (-150.0, 140.0, 40.0, 4, &[ROCK_SMALL, ROUND], [0.7, 1.1]),
        (-230.0, 220.0, 50.0, 5, &[ROCK_SMALL, SPLIT, ROCK_SMALL], [0.6, 1.0]),
        (-60.0, 70.0, 20.0, 2, &[ROCK_SMALL], [0.6, 0.9]),
        (20.0, -60.0, 25.0, 2, &[ROCK_SMALL, ROUND], [0.6, 0.9]),
        (110.0, 120.0, 30.0, 3, &[SPLIT, ROCK_SMALL], [0.9, 1.3]),
        (150.0, 220.0, 50.0, 4, &[OUTCROP, SPLIT], [1.2, 2.0]),
        (200.0, 40.0, 60.0, 4, &[OUTCROP, ROUND], [1.5, 2.5]),
        (300.0, 300.0, 120.0, 8, &[OUTCROP], [2.5, 4.0]),
        (330.0, 600.0, 150.0, 8, &[OUTCROP], [3.0, 5.0]),
    ];
    let mut n = 0u32;
    for &(right, ahead, spread, count, kinds, scale) in groups {
        let centre = Vec2::new(EYE.0 + right, EYE.1 + ahead);
        let mut placed = 0;
        for attempt in 0..count * 30 {
            if placed == count {
                break;
            }
            n += 1;
            let r = |salt: i32| noise::hash(n as i32, salt, 0x40c5);
            let a = r(1) * std::f32::consts::TAU;
            let p = centre + Vec2::new(a.cos(), a.sin()) * spread * r(2).sqrt();
            if rel(p.x, p.y).length() < 12.0 {
                continue;
            }
            let kind = kinds[(placed + attempt) % kinds.len()];
            let s = scale[0] + (scale[1] - scale[0]) * r(3);
            let fp = 2.0 * s;
            if trees.iter().any(|t| t.distance(p) < 1.5 + fp)
                || taken.iter().any(|q| q.distance(p) < 2.0 * fp)
            {
                continue;
            }
            let normal = crate::sample_rocks::terrain_normal(d, p.x, p.y);
            let up = Vec3::Y.lerp(normal, 0.6).normalize();
            let rotation =
                Quat::from_rotation_arc(Vec3::Y, up) * Quat::from_rotation_y(r(4) * 6.283);
            // Sunk into the slope so no edge floats.
            let h = d.sample_height(p.x, p.y);
            let slope = Vec2::new(normal.x, normal.z).length() / normal.y.max(0.2);
            let sink = slope * fp * 0.8 + 0.1 * fp;
            out.push(Placed {
                rock: kind,
                position: CORNER + Vec3::new(p.x, h - sink, p.y),
                rotation,
                scale: s,
            });
            taken.push(p);
            placed += 1;
        }
    }
    out
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.2;
    v.bloom.threshold = 1.2;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.1;
    v.color_adjustments.contrast = 12.0;
    v.color_adjustments.saturation = -2.0;
    v.ambient_occlusion.active = true;
    v.ambient_occlusion.intensity = 1.4;
    v.ambient_occlusion.radius = 2.5;
    let cs = &mut v.contact_shadows;
    cs.active = true;
    cs.length = 0.12;
    cs.distance_fade = 16.0;
    cs.opacity = 0.3;
    cs.sample_count = 6;
    // Fair-weather cumulus: big soft heaps with small ones scattered between.
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Sparse.apply(c);
    c.coverage = 0.3;
    c.density = 0.6;
    c.shape = 0.65;
    c.altitude = 1100.0;
    c.thickness = 1300.0;
    c.wind_direction = 100.0;
    c.wind_speed = 6.0;
    c.silver_lining = 0.5;
    c.cumulus = 1.0;
    c.shadow_opacity = 0.7;
    // Aerial perspective: blue-grey haze that layers the distant hills.
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 6000.0;
    f.attenuation_distance = 1700.0;
    f.base_height = 60.0;
    f.maximum_height = 500.0;
    f.albedo = [0.62, 0.8, 1.0];
    f.anisotropy = 0.2;
    f.sky_haze = 0.9;
    f.volumetric_distance = 300.0;
    v
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("NoonValley");
    let eye_y = ground(d, 0.0, 0.0) + 1.7;
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, eye_y, 0.0))
                .with_euler(Vec3::new(-1.0, 0.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 6000.0;
        c.near = 0.05;
        c.fov = 60.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Noon: the sun high overhead, a little behind the camera and to the left.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 200.0, 0.0))
                .with_euler(Vec3::new(72.0, 25.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.97, 0.92];
        l.intensity = 3.2;
        l.shadows = Shadows::Soft;
        l.contact_shadows = true;
    }
    let rs = &mut s.render_settings;
    rs.skybox.sun_disk = SunDisk::HighQuality;
    rs.skybox.sun_size = 0.04;
    rs.skybox.atmosphere_thickness = 1.0;
    // A deep blue overhead, paler and hazier towards the horizon.
    rs.skybox.sky_tint = [0.38, 0.41, 0.45];
    rs.realtime_gi = true;
    rs.environment_lighting.intensity = 0.75;

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
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    let wind = s.spawn("Wind Zone", None);
    s.set_transform(
        wind,
        Transform::from_position(Vec3::new(0.0, eye_y + 8.0, 0.0))
            .with_euler(Vec3::new(0.0, 75.0, 0.0)),
    );
    s.add(wind, WindZone::default());
    // The First Person Player stands at the camera; its camera renders (the Main Camera is
    // kept, switched off).
    let prefab =
        misty_engine::scene::prefab::Prefab::load(&project().join(crate::fps_player::PREFAB))
            .expect("First Person Player prefab");
    let player = s.instantiate_prefab(&prefab, Some(crate::fps_player::PREFAB), None);
    s.set_transform(
        player,
        Transform::from_position(Vec3::new(0.0, ground(d, 0.0, 0.0), 0.0)),
    );
    if let Some(cam) = s.find_by_name("Camera") {
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 6000.0;
        c.fov = 60.0;
    }
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.game_object_mut(cam).active = false;
    }
    crate::sample_rocks::add_to_scene(&mut s, &rocks(d));
    s
}

/// Meta files for the sample's folder and everything under it, and for the scene, where
/// missing.
fn write_metas(root: &std::path::Path) {
    use misty_engine::asset_db::{Guid, meta};
    let dir = root.join(DIR);
    let mut paths = vec![dir.clone(), root.join(SCENE)];
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if meta::is_meta(&p) {
                continue;
            }
            if p.is_dir() {
                stack.push(p.clone());
            }
            paths.push(p);
        }
    }
    for p in paths {
        if p.exists() && meta::read_guid(&p).is_none() {
            meta::write_guid(&p, Guid::new()).unwrap();
        }
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_noon_valley() {
    let root = project();
    std::fs::create_dir_all(root.join(DIR).join("Textures")).unwrap();
    let [albedo, height, normal] = limestone();
    albedo.save(root.join(LIMESTONE)).unwrap();
    height.save(root.join(LIMESTONE_HEIGHT)).unwrap();
    normal.save(root.join(LIMESTONE_NORMAL)).unwrap();
    let [albedo, normal] = forest_floor();
    albedo.save(root.join(FOREST_FLOOR)).unwrap();
    normal.save(root.join(FOREST_FLOOR_NORMAL)).unwrap();
    for (rel, tree) in tree_assets() {
        tree.save(&root.join(rel)).unwrap();
    }
    let d = terrain_data();
    eprintln!("{} trees", d.trees.instances.len());
    d.save(&root.join(DATA)).unwrap();
    // The new files get their meta files (GUIDs) before the scene refers to them; nothing
    // else in the project is touched.
    write_metas(&root);
    misty_engine::asset_db::install(misty_engine::asset_db::GuidDatabase::read(&root));
    demo_scene(&d).save(&root.join(SCENE)).unwrap();
    write_metas(&root);
}

#[test]
fn noon_valley_matches_its_generator() {
    let root = project();
    for rel in [
        LIMESTONE,
        LIMESTONE_NORMAL,
        LIMESTONE_HEIGHT,
        FOREST_FLOOR,
        FOREST_FLOOR_NORMAL,
        GROUND,
        GROUND_NORMAL,
        GROUND_HEIGHT,
        TURF,
    ] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    for (rel, tree) in tree_assets() {
        assert_eq!(TreeAsset::load(&root.join(&rel)).unwrap(), tree, "{rel}");
    }
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.flowers, generated.flowers);
    assert_eq!(on_disk.detail_layers, generated.detail_layers);
    assert_eq!(on_disk.trees.prototypes, generated.trees.prototypes);
    assert_eq!(
        on_disk.trees.instances.len(),
        generated.trees.instances.len()
    );
    for (i, (a, b)) in on_disk
        .trees
        .instances
        .iter()
        .zip(&generated.trees.instances)
        .enumerate()
    {
        assert!(
            a.position.distance(b.position) < 1e-6 && a.prototype == b.prototype,
            "tree {i}: {a:?} vs {b:?}"
        );
    }
    assert_eq!(on_disk.size, generated.size);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene(&generated).to_json(), SCENE);
    // A dense forest on the hillside, the meadow ahead open.
    let n = generated.trees.instances.len();
    assert!(n > 10_000, "{n} trees");
    for t in &generated.trees.instances {
        let p = t.position * Vec2::new(SIZE.x, SIZE.z);
        assert!(rel(p.x, p.y).length() > 30.0, "tree at {p}");
    }
    // Thick grass round the camera, flowers in the meadow.
    let lo = Vec2::new(EYE.0 - 8.0, EYE.1);
    let blades = details::scatter(&generated, lo, lo + 16.0);
    assert!(blades.len() > 40_000, "{}", blades.len());
    let mut flowers = 0;
    for x in 0..4 {
        for z in 0..4 {
            let lo = Vec2::new(EYE.0 - 128.0 + x as f32 * 64.0, EYE.1 + z as f32 * 64.0);
            flowers += misty_engine::terrain::flowers::scatter(&generated, lo, lo + 64.0).len();
        }
    }
    assert!(flowers > 1000, "{flowers} flower cards");
}

/// Playing works (the terrain and rock colliders build).
#[test]
fn noon_valley_plays() {
    let root = project();
    let data = TerrainData::load(&root.join(DATA)).unwrap();
    let scene = demo_scene(&data);
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    for f in scene
        .world
        .query::<&mut misty_engine::scene::MeshFilter>()
        .iter()
    {
        if let Some(misty_engine::scene::MeshRef::Asset(key)) = &mut f.mesh
            && key.starts_with("Assets/")
        {
            let abs = root.join(key.as_str()).to_string_lossy().into_owned();
            *key = abs;
        }
    }
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
}
