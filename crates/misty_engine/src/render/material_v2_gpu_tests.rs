//! GPU checks of the Standard Lit v2 features (render/material_v2.rs): each one changes the
//! picture where it should, the feature path with neutral values draws what the plain Standard
//! shader draws, and (ignored, on purpose) what each feature costs on the GPU.

use std::path::PathBuf;

use glam::{Mat4, Quat, Vec3};
use image::{Rgba, RgbaImage};

use super::gpu_tests::{draw, headless, read_back};
use super::*;
use crate::camera::{Camera, Projection};
use crate::material_features::{
    Anisotropy, AnisotropyDirection, ClearCoat, DetailMaps, Features, Parallax, Translucency,
};
use crate::scene::PrimitiveMesh;

const W: u32 = 320;
const H: u32 = 200;

/// Test textures written once per process (absolute paths: tests move the project root).
fn texture_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("misty_lit_v2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = |name: &str, img: RgbaImage| img.save(dir.join(name)).unwrap();
        // Mid grey (no change as a detail albedo) and a flat normal.
        save(
            "grey.png",
            RgbaImage::from_pixel(8, 8, Rgba([128, 128, 128, 255])),
        );
        save(
            "flat.png",
            RgbaImage::from_pixel(8, 8, Rgba([128, 128, 255, 255])),
        );
        // A checker detail and bumpy detail normals.
        save(
            "checker.png",
            RgbaImage::from_fn(64, 64, |x, y| {
                let v = if (x / 8 + y / 8) % 2 == 0 { 230 } else { 30 };
                Rgba([v, v, v, 255])
            }),
        );
        save(
            "bumps.png",
            RgbaImage::from_fn(64, 64, |x, _| {
                let s = ((x as f32 / 64.0) * std::f32::consts::TAU * 4.0).sin();
                Rgba([(128.0 + 100.0 * s) as u8, 128, 230, 255])
            }),
        );
        // Bricks: mortar low, bricks high.
        save(
            "height.png",
            RgbaImage::from_fn(128, 128, |x, y| {
                let row = y / 32;
                let xo = (x + row * 32) % 64;
                let mortar = y % 32 < 4 || xo < 4;
                let v = if mortar { 0 } else { 255 };
                Rgba([v, v, v, 255])
            }),
        );
        dir
    })
    .clone()
}

fn tex(name: &str) -> Option<String> {
    Some(
        texture_dir()
            .join(name)
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

fn base_material() -> MaterialParams {
    MaterialParams {
        color: [0.75, 0.3, 0.2, 1.0],
        smoothness: 0.6,
        ..Default::default()
    }
}

/// A Standard material with `features` (through the asset path, as `.mat` files get them).
fn with_features(features: Features) -> MaterialParams {
    MaterialParams {
        extras: features.extras().map(std::sync::Arc::new),
        ..base_material()
    }
}

/// The base material with a checker albedo (parallax only moves the texture).
fn textured(m: MaterialParams) -> MaterialParams {
    MaterialParams {
        albedo_map: tex("checker.png"),
        tiling: [2.0, 2.0],
        ..m
    }
}

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 1.6, -4.2),
        rotation: Quat::from_rotation_x(14f32.to_radians()),
        projection: Projection::Perspective { fov_y: 45.0 },
        near: 0.1,
        far: 50.0,
    }
}

/// A sphere and a tilted plane behind it, lit by the sun from the front left and a point
/// light behind (translucency shows against it).
fn meshes(m: &MaterialParams) -> Vec<MeshDraw> {
    vec![
        draw(
            PrimitiveMesh::Sphere,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(1.6),
                Quat::IDENTITY,
                Vec3::new(-0.9, 0.8, 0.0),
            ),
            m,
        ),
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.25, 1.0, 0.25),
                Quat::from_rotation_x(-60f32.to_radians()),
                Vec3::new(1.1, 0.9, 0.6),
            ),
            m,
        ),
    ]
}

fn lights() -> Vec<LightData> {
    let light = |kind, position: Vec3, direction: Vec3, color, intensity| LightData {
        kind,
        position,
        direction: direction.normalize(),
        color,
        intensity,
        range: 8.0,
        spot_angle: 50.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    };
    vec![
        light(
            LightType::Directional,
            Vec3::ZERO,
            Vec3::new(0.5, -0.6, 0.6),
            [1.0, 0.97, 0.92],
            1.3,
        ),
        light(
            LightType::Point,
            Vec3::new(-0.9, 1.0, 1.6),
            Vec3::Z,
            [1.0, 0.8, 0.6],
            3.0,
        ),
    ]
}

fn render_meshes(device: &wgpu::Device, queue: &wgpu::Queue, meshes: &[MeshDraw]) -> Vec<u8> {
    let cam = camera();
    let lights = lights();
    let environment = Environment::default();
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let mut renderer = Renderer::with_queue(device, queue);
    let target = renderer.create_target(device, W, H);
    renderer.render(device, queue, &target, &frame);
    read_back(device, queue, &target)
}

/// (pixels differing by more than 8 in some channel, mean absolute channel difference).
fn difference(a: &[u8], b: &[u8]) -> (usize, f32) {
    let changed = a
        .chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    let sum: u64 = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    (changed, sum as f32 / a.len() as f32)
}

fn save_png(name: &str, pixels: &[u8]) {
    // `MISTY_GPU_TEST_PNG=<dir>` saves the renders for a look.
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        let img = RgbaImage::from_raw(W, H, pixels.to_vec()).expect("size");
        img.save(PathBuf::from(dir).join(format!("lit_v2_{name}.png")))
            .expect("png");
    }
}

/// The features one at a time (name, features, smallest share of the picture that changes).
fn feature_cases() -> Vec<(&'static str, Features, f32)> {
    let f = Features::new;
    vec![
        (
            "detail",
            Features {
                detail: DetailMaps {
                    enabled: true,
                    albedo_map: tex("checker.png"),
                    normal_map: tex("bumps.png"),
                    ..Default::default()
                },
                ..f()
            },
            0.05,
        ),
        (
            "parallax",
            Features {
                parallax: Parallax {
                    enabled: true,
                    height_map: tex("height.png"),
                    scale: 0.08,
                    steps: 32,
                },
                ..f()
            },
            0.004,
        ),
        (
            "clear_coat",
            Features {
                clear_coat: ClearCoat {
                    enabled: true,
                    ..Default::default()
                },
                ..f()
            },
            0.02,
        ),
        (
            "translucency",
            Features {
                translucency: Translucency {
                    enabled: true,
                    strength: 1.0,
                    wrap: 0.8,
                    ..Default::default()
                },
                ..f()
            },
            0.02,
        ),
        (
            "anisotropy",
            Features {
                anisotropy: Anisotropy {
                    enabled: true,
                    strength: 0.9,
                    direction: AnisotropyDirection::Vertical,
                    rotation: 0.0,
                },
                ..f()
            },
            0.005,
        ),
        (
            "specular",
            Features {
                specular: 0.0,
                ..f()
            },
            0.005,
        ),
    ]
}

#[test]
fn each_feature_changes_the_picture() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let plain = render_meshes(&device, &queue, &meshes(&textured(base_material())));
    save_png("plain", &plain);
    let total = (W * H) as f32;
    for (name, features, least) in feature_cases() {
        let m = textured(with_features(features));
        assert_ne!(m.features(), 0, "{name}");
        let img = render_meshes(&device, &queue, &meshes(&m));
        save_png(name, &img);
        let (changed, mean) = difference(&plain, &img);
        eprintln!("{name}: {changed} pixels changed, mean {mean:.2}");
        assert!(
            changed as f32 > least * total,
            "{name}: only {changed} pixels changed"
        );
        // Nothing blown out to NaN black or white on the sphere's centre.
        let centre = ((H / 2 + 10) * W + W / 2 - 50) as usize * 4;
        assert!(
            img[centre..centre + 3].iter().any(|c| *c > 4),
            "{name}: black centre"
        );
    }
}

/// The feature path's base shading is the Standard shader's: a detail with a mid-grey albedo
/// and a flat normal draws the plain picture (the explicit-gradient sampling may round a few
/// texels differently).
#[test]
fn neutral_detail_draws_the_plain_material() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let plain = render_meshes(&device, &queue, &meshes(&base_material()));
    let neutral = with_features(Features {
        detail: DetailMaps {
            enabled: true,
            albedo_map: tex("grey.png"),
            normal_map: tex("flat.png"),
            normal_scale: 0.0,
            ..Default::default()
        },
        ..Features::new()
    });
    assert_eq!(neutral.features(), material_v2::feature::DETAIL);
    let img = render_meshes(&device, &queue, &meshes(&neutral));
    let (changed, mean) = difference(&plain, &img);
    eprintln!("neutral detail: {changed} pixels changed, mean {mean:.3}");
    assert!(mean < 0.6, "mean difference {mean}");
    assert!(
        (changed as f32) < 0.003 * (W * H) as f32,
        "{changed} pixels changed"
    );
}

/// Plain Standard materials still draw with the plain variants (no feature pipelines built,
/// the same image whether or not a feature material is drawn elsewhere in the frame).
#[test]
fn plain_materials_are_untouched_by_feature_materials() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let plain_meshes = meshes(&base_material());
    let alone = render_meshes(&device, &queue, &plain_meshes);
    // A clear-coated sphere far behind the camera: culled, but prepared and batched.
    let mut with_coat = plain_meshes.clone();
    let coat = with_features(Features {
        clear_coat: ClearCoat {
            enabled: true,
            ..Default::default()
        },
        ..Features::new()
    });
    with_coat.push(draw(
        PrimitiveMesh::Sphere,
        Mat4::from_translation(Vec3::new(0.0, 0.0, -30.0)),
        &coat,
    ));
    let both = render_meshes(&device, &queue, &with_coat);
    assert_eq!(difference(&alone, &both).0, 0);
    assert_eq!(base_material().pipeline_key().features, 0);
    assert!(base_material().lit_set().is_none());
}

/// GPU cost of each feature: a wall of spheres covering a 1600×900 view, all in one material,
/// main pass time with the plain Standard shader and with each feature (median of 60 frames).
///
/// `cargo test --release -p misty_engine --lib material_feature_costs -- --ignored --nocapture`
#[test]
#[ignore = "GPU timing; run on purpose"]
fn material_feature_costs() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let cam = Camera {
        position: Vec3::new(0.0, 0.0, -9.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 60.0,
    };
    let lights = lights();
    let environment = Environment::default();
    let grid = |m: &MaterialParams| {
        let mut out = Vec::new();
        for y in -4..=4 {
            for x in -8..=8 {
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::splat(1.15),
                    Quat::from_rotation_y(x as f32 * 0.3),
                    Vec3::new(x as f32 * 1.05, y as f32 * 1.05, 0.0),
                );
                out.push(draw(PrimitiveMesh::Sphere, model, m));
            }
        }
        out.push(draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale_rotation_translation(
                Vec3::new(3.0, 1.0, 2.0),
                Quat::from_rotation_x(-90f32.to_radians()),
                Vec3::new(0.0, 0.0, 1.0),
            ),
            m,
        ));
        out
    };
    let mut cases: Vec<(&str, MaterialParams)> = vec![("plain Standard", base_material())];
    for (name, f, _) in feature_cases() {
        cases.push((name, with_features(f)));
    }
    let mut all = Features::new();
    for (_, f, _) in feature_cases() {
        all.detail = if f.detail.enabled {
            f.detail
        } else {
            all.detail
        };
        all.parallax = if f.parallax.enabled {
            f.parallax
        } else {
            all.parallax
        };
        all.clear_coat = if f.clear_coat.enabled {
            f.clear_coat
        } else {
            all.clear_coat
        };
        all.translucency = if f.translucency.enabled {
            f.translucency
        } else {
            all.translucency
        };
        all.anisotropy = if f.anisotropy.enabled {
            f.anisotropy
        } else {
            all.anisotropy
        };
        all.specular = all.specular.min(f.specular.max(0.5));
    }
    cases.push(("all six", with_features(all)));
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1600, 900);
    if !renderer.set_pass_timing(&device, &queue, true) {
        eprintln!("no timestamp queries: skipped");
        return;
    }
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    eprintln!("material feature costs, 1600x900, 154 spheres + a wall filling the view:");
    // Three interleaved rounds (each case's median of 60 frames per round); the smallest of
    // a case's round medians is kept, which drops most of the noise of a shared machine.
    type Parts = Vec<(&'static str, f32)>;
    let mut best: Vec<(f32, Parts)> = vec![(f32::MAX, Vec::new()); cases.len()];
    for _round in 0..3 {
        for (c, (_, m)) in cases.iter().enumerate() {
            let meshes = grid(m);
            let frame = Frame {
                camera: &cam,
                grid: None,
                skybox: true,
                background: [0.1; 3],
                environment: &environment,
                post: PostSettings::default(),
                lines: &[],
                overlay: &[],
                meshes: &meshes,
                particles: &[],
                terrains: &[],
                shaded: true,
                lights: &lights,
                water: &[],
            };
            let mut totals = Vec::new();
            let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
            for i in 0..80 {
                renderer.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i < 20 {
                    continue;
                }
                let mut all = 0.0;
                let mut seen: Vec<(&'static str, f32)> = Vec::new();
                for &(label, ms) in renderer.pass_times() {
                    all += ms;
                    match seen.iter_mut().find(|(l, _)| *l == label) {
                        Some(s) => s.1 += ms,
                        None => seen.push((label, ms)),
                    }
                }
                for (label, ms) in seen {
                    match parts.iter_mut().find(|(l, _)| *l == label) {
                        Some(p) => p.1.push(ms),
                        None => parts.push((label, vec![ms])),
                    }
                }
                totals.push(all);
            }
            let total = median(totals);
            if total < best[c].0 {
                best[c] = (
                    total,
                    parts.into_iter().map(|(l, v)| (l, median(v))).collect(),
                );
            }
        }
    }
    let (base, base_parts) = best[0].clone();
    for (label, ms) in &base_parts {
        eprintln!("    plain: {label:<40} {ms:6.3} ms");
    }
    for ((name, _), (total, parts)) in cases.iter().zip(&best) {
        // The passes that changed most (the lit draws).
        let mut deltas: Vec<(&str, f32)> = parts
            .iter()
            .map(|(l, ms)| {
                let b = base_parts
                    .iter()
                    .find(|(bl, _)| bl == l)
                    .map_or(0.0, |p| p.1);
                (*l, ms - b)
            })
            .filter(|(_, d)| d.abs() >= 0.005)
            .collect();
        deltas.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        let detail: Vec<String> = deltas
            .iter()
            .take(3)
            .map(|(l, d)| format!("{l} {d:+.3}"))
            .collect();
        eprintln!(
            "  {name:<16} frame {total:6.3} ms ({:+6.3} ms)  {}",
            total - base,
            detail.join(", ")
        );
    }
}
