//! Headless GPU checks of the terrain renderer: chunked drawing with grass, partial uploads
//! matching full uploads, and (ignored) timings of large-terrain editing.

use std::time::Instant;

use glam::{Quat, Vec2, Vec3};

use super::environment::Environment;
use super::gpu_tests::{headless, read_back};
use super::post::PostSettings;
use super::{Frame, LightData, Renderer};
use crate::camera::{Camera, Projection};
use crate::scene::{LightType, Shadows};
use crate::terrain::brush::{self, Brush};
use crate::terrain::{TerrainData, TerrainDraw, TerrainLayer, auto_paint, details, store};

fn hills(res: usize, size: f32) -> TerrainData {
    let mut d = TerrainData::new(res, Vec3::new(size, size * 0.25, size), 256);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            let (x, z) = (ix as f32 * s.x, iz as f32 * s.y);
            let h = crate::terrain::noise::fbm(x, z, size * 0.3, 5, 3) * 0.5 + 0.5;
            d.set_height(ix, iz, h * h);
        }
    }
    let rules = |slope: [f32; 2], priority| crate::terrain::auto_paint::AutoPaintRules {
        enabled: true,
        slope,
        priority,
        ..Default::default()
    };
    d.layers = vec![
        TerrainLayer {
            name: "Grass".into(),
            rules: rules([0.0, 25.0], 0),
            ..Default::default()
        },
        TerrainLayer {
            name: "Rock".into(),
            rules: rules([30.0, 90.0], 1),
            ..Default::default()
        },
    ];
    d.details.enabled = true;
    auto_paint::generate(&mut d);
    details::generate(&mut d);
    d
}

fn path(name: &str) -> String {
    std::env::temp_dir()
        .join(format!(
            "misty_terrain_gpu_{name}_{}.terrain",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn draw(key: &str) -> TerrainDraw {
    let (data, version) = store::get(key).expect("inserted");
    TerrainDraw {
        key: key.to_owned(),
        version,
        data,
        position: Vec3::new(-32.0, -4.0, -10.0),
        cast_shadows: true,
    }
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    key: &str,
) -> Vec<u8> {
    let camera = Camera {
        position: Vec3::new(0.0, 14.0, -20.0),
        rotation: Quat::from_rotation_x(30f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.3,
        far: 200.0,
    };
    let lights = [LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.3, -1.0, 0.4).normalize(),
        color: [1.0; 3],
        intensity: 1.0,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Soft,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
    }];
    let environment = Environment::default();
    let terrains = [draw(key)];
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &[],
        particles: &[],
        terrains: &terrains,
        water: &[],
        shaded: true,
        lights: &lights,
    };
    let target = renderer.create_target(device, 256, 160);
    // Twice: grass chunks are scattered over the first frames.
    renderer.render(device, queue, &target, &frame);
    renderer.render(device, queue, &target, &frame);
    read_back(device, queue, &target)
}

#[test]
fn partial_uploads_match_a_fresh_upload() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let key = path("partial");
    store::insert(&key, hills(129, 64.0));
    let mut edited = Renderer::with_queue(&device, &queue);
    let before = render(&device, &queue, &mut edited, &key);
    // Something other than sky: terrain fills the lower half.
    let row = &before[150 * 256 * 4..151 * 256 * 4];
    assert!(
        row.chunks(4).any(|p| p[1] > p[2]),
        "green-ish terrain drawn"
    );
    // Grass sways with the clock; compare the ground alone.
    store::modify(&key, |d| d.details.enabled = false);
    let no_grass = render(&device, &queue, &mut edited, &key);
    assert_ne!(before, no_grass, "grass was drawn");
    if let Some(p) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        image::RgbaImage::from_raw(256, 160, before.clone())
            .unwrap()
            .save(p)
            .unwrap();
    }

    // A few brush strokes, uploaded as regions.
    for i in 0..5 {
        let b = Brush {
            center: Vec2::new(30.0 + i as f32 * 2.0, 20.0),
            size: 12.0,
            strength: 1.0,
            falloff: 0.5,
        };
        let region = store::get(&key).map(|(d, _)| b.region(&d)).unwrap();
        store::modify_region(&key, region, |d| {
            brush::raise(d, &b, 0.2, false);
            brush::paint(d, &b, 0.2, 1);
        });
        let changed = store::changes_since(&key, store::get(&key).unwrap().1 - 1);
        assert!(changed.is_some_and(|r| !r.is_empty()), "region change");
        render(&device, &queue, &mut edited, &key);
    }
    let partial = render(&device, &queue, &mut edited, &key);
    let mut fresh = Renderer::with_queue(&device, &queue);
    let full = render(&device, &queue, &mut fresh, &key);
    assert_ne!(no_grass, partial, "the strokes show");
    let differing = partial
        .chunks(4)
        .zip(full.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "{differing} pixels differ");
    store::unload(&key);
}

/// Timings of editing a large terrain (`cargo test -p misty_engine --release terrain_timings -- --ignored --nocapture`).
#[test]
#[ignore = "timing run"]
fn terrain_timings() {
    let Some((device, queue)) = headless() else {
        return;
    };
    for res in [2049, 4097] {
        let key = path(&format!("timing{res}"));
        let t = Instant::now();
        let mut d = hills(res, res as f32 - 1.0);
        d.set_alphamap_resolution(1024);
        let built = t.elapsed();
        let t = Instant::now();
        auto_paint::generate(&mut d);
        let generate = t.elapsed();
        let t = Instant::now();
        details::generate(&mut d);
        let details = t.elapsed();
        store::insert(&key, d);
        let mut r = Renderer::with_queue(&device, &queue);
        let t = Instant::now();
        render(&device, &queue, &mut r, &key);
        let first = t.elapsed();
        let mut stroke = std::time::Duration::ZERO;
        let mut frame = std::time::Duration::ZERO;
        let frames = 30;
        for i in 0..frames {
            let b = Brush {
                center: Vec2::new(500.0 + i as f32 * 3.0, 400.0),
                size: 40.0,
                strength: 0.5,
                falloff: 0.7,
            };
            let t = Instant::now();
            let region = store::get(&key).map(|(d, _)| b.region(&d)).unwrap();
            store::modify_region(&key, region, |d| brush::raise(d, &b, 1.0 / 60.0, false));
            stroke += t.elapsed();
            let t = Instant::now();
            render(&device, &queue, &mut r, &key);
            frame += t.elapsed();
        }
        eprintln!(
            "{res}²: build {built:?}, generate splat {generate:?}, details {details:?}, first render {first:?}, \
             stroke step {:?}, render after stroke (2 frames) {:?}",
            stroke / frames,
            frame / frames
        );
        store::unload(&key);
    }
}
