//! Headless GPU checks of the grass on a small synthetic meadow: shadows cast and received,
//! colour and height variation, wind waves that travel, the ground hidden between blades and
//! grass shadow casters costing nothing when off. `MISTY_GPU_TEST_DIR=<dir>` saves the images.

use std::path::PathBuf;

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::{QualityLevel, Tier};
use crate::render::environment::Environment;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::post::PostSettings;
use crate::render::{Frame, LightData, MaterialParams, MeshDraw, RenderStats, Renderer};
use crate::scene::{LightType, PrimitiveMesh, Shadows};
use crate::terrain::auto_paint::{self, AutoPaintRules};
use crate::terrain::{TerrainData, TerrainDraw, TerrainLayer, details, store};

const W: u32 = 320;
const H: u32 = 200;

/// A flat 48 m field of grass (store key from `name`) with `edit` applied to its settings.
fn field(name: &str, edit: impl Fn(&mut details::DetailSettings)) -> TerrainDraw {
    let mut d = TerrainData::new(129, Vec3::new(48.0, 10.0, 48.0), 128);
    let n = d.heightmap_resolution;
    for iz in 0..n {
        for ix in 0..n {
            d.set_height(ix, iz, 0.1);
        }
    }
    d.layers = vec![TerrainLayer {
        name: "Ground".into(),
        rules: AutoPaintRules {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }];
    d.details = details::DetailSettings {
        enabled: true,
        density: 300.0,
        height: [0.25, 0.55],
        width: 0.025,
        draw_distance: 60.0,
        lod_distance: 12.0,
        rules: AutoPaintRules {
            enabled: true,
            breakup: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    edit(&mut d.details);
    auto_paint::generate(&mut d);
    details::generate(&mut d);
    let key = std::env::temp_dir()
        .join(format!("misty_grass_{name}_{}.terrain", std::process::id()))
        .to_string_lossy()
        .into_owned();
    store::insert(&key, d);
    let (data, version) = store::get(&key).expect("inserted");
    TerrainDraw {
        key,
        version,
        data,
        position: Vec3::new(-24.0, -1.0, -4.0),
        cast_shadows: true,
    }
}

fn sun(direction: Vec3) -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity: 1.6,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Soft,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }
}

struct Shot<'a> {
    camera: Camera,
    terrains: &'a [TerrainDraw],
    meshes: &'a [MeshDraw],
    light: LightData,
    time: f32,
}

impl<'a> Shot<'a> {
    /// Standing in the field looking along +Z, slightly down; sun from behind-left.
    fn standing(terrains: &'a [TerrainDraw], meshes: &'a [MeshDraw]) -> Self {
        Shot {
            camera: Camera {
                position: Vec3::new(0.0, 0.6, 0.0),
                rotation: Quat::from_rotation_x(14f32.to_radians()),
                projection: Projection::Perspective { fov_y: 55.0 },
                near: 0.05,
                far: 300.0,
            },
            terrains,
            meshes,
            light: sun(Vec3::new(0.3, -0.7, 0.6)),
            time: 3.0,
        }
    }

    /// Pixel showing world point `p`.
    fn pixel(&self, p: Vec3) -> (u32, u32) {
        let ndc = self
            .camera
            .view_projection(W as f32 / H as f32)
            .project_point3(p);
        let x = ((ndc.x * 0.5 + 0.5) * W as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * H as f32) as u32;
        (x.clamp(12, W - 13), y.clamp(8, H - 9))
    }

    fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quality: &QualityLevel,
    ) -> (Vec<u8>, RenderStats) {
        let environment = Environment {
            grass_time: Some(self.time),
            ..Default::default()
        };
        let lights = [self.light];
        let frame = Frame {
            camera: &self.camera,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: self.meshes,
            particles: &[],
            terrains: self.terrains,
            water: &[],
            shaded: true,
            lights: &lights,
        };
        let mut r = Renderer::with_queue(device, queue);
        r.set_quality(
            device,
            &QualityLevel {
                temporal_aa: false,
                ..quality.clone()
            },
        );
        let target = r.create_target(device, W, H);
        // Grass chunks are scattered over the first frames.
        for _ in 0..3 {
            r.render(device, queue, &target, &frame);
        }
        (read_back(device, queue, &target), r.stats())
    }
}

fn luma(rgba: &[u8], x: u32, y: u32) -> f32 {
    let i = ((y * W + x) * 4) as usize;
    0.2126 * rgba[i] as f32 + 0.7152 * rgba[i + 1] as f32 + 0.0722 * rgba[i + 2] as f32
}

/// Mean brightness of the rectangle `[x0, x1) × [y0, y1)`.
fn mean(rgba: &[u8], (x0, y0, x1, y1): (u32, u32, u32, u32)) -> f32 {
    let mut sum = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += luma(rgba, x, y);
        }
    }
    sum / ((x1 - x0) * (y1 - y0)) as f32
}

fn save(name: &str, rgba: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
        let path = PathBuf::from(dir).join(format!("grass_{name}.png"));
        image::RgbaImage::from_raw(W, H, rgba.to_vec())
            .unwrap()
            .save(path)
            .unwrap();
    }
}

fn unload(terrains: &[TerrainDraw]) {
    for t in terrains {
        store::unload(&t.key);
    }
}

fn cube(at: Vec3, size: f32) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::from_scale_rotation_translation(Vec3::splat(size), Quat::IDENTITY, at),
        material: MaterialParams::default(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

fn no_grass_shadows(q: &QualityLevel) -> QualityLevel {
    QualityLevel {
        grass_shadows: Tier::Off,
        ..q.clone()
    }
}

/// Grass casts shadows (it darkens the field, the ground under it most), a cube standing in
/// the grass shades the blades next to it, and with grass shadows off no casters are drawn.
#[test]
fn grass_casts_and_receives_shadows() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let terrains = [field("shadows", |_| {})];
    let high = QualityLevel::high();
    let off = no_grass_shadows(&high);
    let shot = Shot::standing(&terrains, &[]);
    let (lit, with) = shot.render(&device, &queue, &high);
    let (unshaded, without) = shot.render(&device, &queue, &off);
    save("shadows_on", &lit);
    save("shadows_off", &unshaded);
    let field_rect = (0, H / 2, W, H);
    let (a, b) = (mean(&lit, field_rect), mean(&unshaded, field_rect));
    eprintln!("field brightness: grass shadows {a:.1}, none {b:.1}");
    assert!(a < b * 0.96, "grass shadows darken the field: {a} vs {b}");
    assert!(
        with.triangles > without.triangles,
        "casters drawn only with grass shadows on"
    );

    // A cube 4 m ahead with a low sun from the right: its shadow falls on the grass to its
    // left. Compare that grass with the same grass without the cube.
    let at = Vec3::new(0.4, 0.4, 4.0);
    let meshes = [cube(at, 1.2)];
    let mut shot = Shot::standing(&terrains, &meshes);
    shot.light = sun(Vec3::new(-0.9, -0.45, 0.1));
    let (img, _) = shot.render(&device, &queue, &off);
    shot.meshes = &[];
    let (bare, _) = shot.render(&device, &queue, &off);
    save("cube", &img);
    save("cube_bare", &bare);
    let (x, y) = shot.pixel(at + Vec3::new(-1.7, -0.3, 0.0));
    let patch = (x - 12, y - 8, x + 12, y + 8);
    let (shaded, sunny) = (mean(&img, patch), mean(&bare, patch));
    eprintln!("grass in the cube's shadow: {shaded:.1}, without the cube {sunny:.1}");
    assert!(
        shaded < sunny * 0.85,
        "the cube shades the grass: {shaded} vs {sunny}"
    );
    unload(&terrains);
}

/// Colour of `rgba` averaged over blocks of `size` pixels in the lower part of the image (the
/// grass), and how far the blocks spread around the mean of their row (RGB distance, 0..255;
/// rows are compared within themselves, so the change with distance does not count).
fn block_spread(rgba: &[u8], size: u32) -> f32 {
    let (mut sum, mut count) = (0.0, 0);
    let mut y = H / 2;
    while y + size <= H {
        let mut blocks = Vec::new();
        let mut x = 0;
        while x + size <= W {
            let mut c = Vec3::ZERO;
            for py in y..y + size {
                for px in x..x + size {
                    let i = ((py * W + px) * 4) as usize;
                    c += Vec3::new(rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32);
                }
            }
            blocks.push(c / (size * size) as f32);
            x += size;
        }
        let m = blocks.iter().copied().sum::<Vec3>() / blocks.len() as f32;
        sum += blocks.iter().map(|b| b.distance_squared(m)).sum::<f32>();
        count += blocks.len();
        y += size;
    }
    (sum / count as f32).sqrt()
}

/// Colour patches, tuft jitter and height variation make the field visibly uneven (blocks of
/// grass seen from above differ) where uniform settings leave it the same everywhere.
#[test]
fn grass_varies_across_the_field() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let varied = [field("varied", |s| {
        s.patch_scale = 4.0;
        s.lush_amount = 0.8;
        s.dry_amount = 0.6;
        s.color_variation = 0.8;
        s.height_variation = 0.8;
    })];
    let uniform = [field("uniform", |s| {
        s.patch_scale = 4.0;
        s.lush_amount = 0.0;
        s.dry_amount = 0.0;
        s.color_variation = 0.0;
        s.height_variation = 0.0;
        s.density_variation = 0.0;
    })];
    // Low: no AO or grass shadows, so the spread is the grass's own colour and shape.
    let quality = QualityLevel::low();
    let mut spreads = Vec::new();
    for (name, t) in [("varied", &varied), ("uniform", &uniform)] {
        let mut shot = Shot::standing(t, &[]);
        // From 6 m up, looking down over the field.
        shot.camera.position = Vec3::new(0.0, 6.0, -2.0);
        shot.camera.rotation = Quat::from_rotation_x(40f32.to_radians());
        let (img, _) = shot.render(&device, &queue, &quality);
        save(name, &img);
        spreads.push(block_spread(&img, 40));
    }
    eprintln!(
        "block colour spread: varied {:.2}, uniform {:.2}",
        spreads[0], spreads[1]
    );
    assert!(
        spreads[0] > spreads[1] * 1.3 && spreads[0] > 10.0,
        "{spreads:?}"
    );
    // Heights follow the patches: the mean blade height of 1 m cells spreads wider.
    let heights = |t: &TerrainDraw| {
        let blades = details::scatter(&t.data, glam::Vec2::splat(8.0), glam::Vec2::splat(32.0));
        let mut cells = vec![(0.0f32, 0u32); 24 * 24];
        for b in blades.iter().filter(|b| !b.seed_head()) {
            let (x, z) = (
                (b.position[0] - 8.0) as usize,
                (b.position[2] - 8.0) as usize,
            );
            let c = &mut cells[z.min(23) * 24 + x.min(23)];
            *c = (c.0 + b.position[3], c.1 + 1);
        }
        let means: Vec<f32> = cells
            .iter()
            .filter(|c| c.1 > 0)
            .map(|c| c.0 / c.1 as f32)
            .collect();
        let m = means.iter().sum::<f32>() / means.len() as f32;
        (means.iter().map(|h| (h - m) * (h - m)).sum::<f32>() / means.len() as f32).sqrt()
    };
    let (hv, hu) = (heights(&varied[0]), heights(&uniform[0]));
    eprintln!("1 m cell height spread: varied {hv:.3} m, uniform {hu:.3} m");
    assert!(hv > hu * 2.0, "{hv} vs {hu}");
    unload(&varied);
    unload(&uniform);
}

/// Column brightness profile of the grass band between rows `y0..y1`.
fn columns(rgba: &[u8], y0: u32, y1: u32) -> Vec<f32> {
    (0..W).map(|x| mean(rgba, (x, y0, x + 1, y1))).collect()
}

/// Wind: the image changes over time, and the change moves downwind: the brightness change
/// between the first two frames reappears shifted to the right between the next two.
#[test]
fn wind_waves_travel_across_the_field() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Wind towards +X (screen right), short gust waves, no flutter.
    let windy = [field("windy", |s| {
        s.wind_direction = 90.0;
        s.wind_strength = 0.5;
        s.gust_strength = 1.2;
        s.gust_scale = 6.0;
        s.turbulence = 0.0;
    })];
    let quality = no_grass_shadows(&QualityLevel::medium());
    let render = |time: f32| {
        let mut shot = Shot::standing(&windy, &[]);
        shot.time = time;
        shot.render(&device, &queue, &quality).0
    };
    let frames = [render(2.0), render(2.15), render(2.3)];
    save("wind_a", &frames[0]);
    save("wind_b", &frames[1]);
    let changed = frames[0]
        .chunks(4)
        .zip(frames[1].chunks(4))
        .filter(|(p, q)| p != q)
        .count();
    assert!(
        changed > (W * H / 20) as usize,
        "wind moves the grass: {changed} pixels"
    );
    // Column profiles of a band 7–12 m away (waves about 100 px long, moving about 17 px
    // per step), smoothed to the waves' scale.
    let profile = |img: &[u8]| -> Vec<f32> {
        let c = columns(img, 58, 72);
        (0..W as usize)
            .map(|x| {
                let (lo, hi) = (x.saturating_sub(6), (x + 7).min(W as usize));
                c[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            })
            .collect()
    };
    let p: Vec<Vec<f32>> = frames.iter().map(|f| profile(f)).collect();
    let d1: Vec<f32> = p[1].iter().zip(&p[0]).map(|(b, a)| b - a).collect();
    let d2: Vec<f32> = p[2].iter().zip(&p[1]).map(|(b, a)| b - a).collect();
    let score = |shift: i32| {
        let mut s = 0.0;
        for x in 50..(W as i32 - 50) {
            s += d1[x as usize] * d2[(x + shift) as usize];
        }
        s
    };
    let best = (-40..=40)
        .max_by(|p, q| score(*p).total_cmp(&score(*q)))
        .unwrap();
    eprintln!(
        "wind change shift: {best} px (score {:.1} vs {:.1} unshifted)",
        score(best),
        score(0)
    );
    assert!(
        best > 3,
        "gusts travel downwind (to the right): shift {best}"
    );
    unload(&windy);
}

/// Few ground pixels show between the blades in the standing shot (pixels where the image
/// equals one of the same field without blades).
#[test]
fn grass_hides_the_ground() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let quality = no_grass_shadows(&QualityLevel::medium());
    let grass = [field("cover", |_| {})];
    let bare = [field("bare", |s| s.density = 0.0)];
    let (a, _) = Shot::standing(&grass, &[]).render(&device, &queue, &quality);
    let (b, _) = Shot::standing(&bare, &[]).render(&device, &queue, &quality);
    let (c, _) = Shot::standing(&[], &[]).render(&device, &queue, &quality);
    let diff = |p: &[u8], q: &[u8]| {
        (0..3)
            .map(|i| (p[i] as i32 - q[i] as i32).abs())
            .max()
            .unwrap()
    };
    let (mut ground, mut seen) = (0, 0);
    for ((p, q), r) in a.chunks(4).zip(b.chunks(4)).zip(c.chunks(4)) {
        if diff(q, r) > 10 {
            ground += 1;
            if diff(p, q) <= 4 {
                seen += 1;
            }
        }
    }
    let share = seen as f32 / ground.max(1) as f32;
    eprintln!(
        "ground visible: {:.1}% of {ground} ground pixels",
        share * 100.0
    );
    assert!(
        ground > (W * H / 3) as usize,
        "the field fills the lower image"
    );
    assert!(
        share < 0.2,
        "ground visible between blades: {:.1}%",
        share * 100.0
    );
    unload(&grass);
    unload(&bare);
}

/// With grass shadows off (Low quality, or the layer's Cast Shadows off) no grass is drawn
/// into shadow maps, and the camera pass draws the same grass either way.
#[test]
fn grass_shadows_off_cost_nothing() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let grass = [field("cost", |_| {})];
    let none = [field("cost_none", |s| s.cast_shadows = false)];
    let low = QualityLevel::low();
    assert_eq!(low.grass_shadows, Tier::Off, "Low keeps grass shadows off");
    let high = QualityLevel::high();
    let shot = Shot::standing(&grass, &[]);
    let (_, off) = shot.render(&device, &queue, &no_grass_shadows(&high));
    let (_, on) = shot.render(&device, &queue, &high);
    let (_, layer_off) = Shot::standing(&none, &[]).render(&device, &queue, &high);
    eprintln!(
        "triangles: casters off {}, on {}, layer off {}",
        off.triangles, on.triangles, layer_off.triangles
    );
    assert!(on.triangles > off.triangles);
    assert_eq!(
        off.triangles, layer_off.triangles,
        "the layer switch turns casters off too"
    );
    unload(&grass);
    unload(&none);
}
