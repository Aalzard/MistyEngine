//! The textures the renderer makes itself (tree maps and impostors, terrain layer arrays, the
//! grass and flower atlas), block-compressed through `sprites::compress::generated`: uploads,
//! and (ignored, run on purpose in release) MeadowDemo's quality per role, GPU memory and GPU
//! frame time with and without compression:
//!
//! `cargo test --release -p misty_engine --lib made_textures_ -- --ignored --nocapture --test-threads=1`

use std::path::PathBuf;
use std::time::Instant;

use image::RgbaImage;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::headless;
use crate::sprites::compress::generated::{self, Ctx, Made, Role};
use crate::sprites::compress::{BcFormat, Compressed};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// A test image: a gradient with a cutout disc in alpha (when `cutout`).
fn image(w: u32, h: u32, cutout: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (dx, dy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
        let inside = (dx * dx + dy * dy).sqrt() < w as f32 * 0.35;
        image::Rgba([
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            128,
            if !cutout || inside { 255 } else { 0 },
        ])
    })
}

/// Slices of every kind (BC1, BC7 sRGB, BC7 linear, RGBA8) go up as arrays, whole or slice by
/// slice, without validation errors, at the sizes `texture_bytes` expects.
#[test]
fn made_textures_upload_without_validation_errors() {
    let Some((device, queue)) = headless() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let bc = device
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
    let ctx = Ctx {
        root: None,
        ..Ctx::bake(&root(), "Windows")
    };
    let mut cases = vec![(None, true), (None, false)];
    if bc {
        cases.extend([
            (Some(BcFormat::Bc1), true),
            (Some(BcFormat::Bc7), true),
            (Some(BcFormat::Bc7), false),
        ]);
    }
    for (format, srgb) in cases {
        let spec = generated::Spec {
            recipe: "test",
            format,
            srgb,
            coverage: false,
        };
        let images = vec![image(64, 32, true), image(64, 32, false)];
        let layers = generated::make(images, &spec, &ctx, |i| super::texture::mip_chain(i, srgb));
        assert_eq!(layers.layers.len(), 2);
        assert_eq!(layers.layers[0].is_compressed(), format.is_some());
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let (view, bytes) =
            super::texture::upload_made(&device, &queue, "test", &layers.layers, true);
        let t = super::texture::made_texture(&device, "test", &layers.layers[0], 2);
        for (i, m) in layers.layers.iter().enumerate() {
            super::texture::write_made(&queue, &t, i as u32, m);
        }
        assert!(pollster::block_on(scope.pop()).is_none(), "{format:?}");
        let tex = view.texture();
        assert_eq!(tex.mip_level_count(), 7);
        assert_eq!(tex.depth_or_array_layers(), 2);
        assert_eq!(
            super::texture::texture_bytes(tex.format(), 64, 32, 7, 2),
            bytes,
            "{format:?}"
        );
        assert_eq!(bytes, layers.bytes());
    }
}

/// Share of texels passing the 0.5 alpha test.
fn coverage(img: &RgbaImage) -> f32 {
    let n = img.pixels().filter(|p| p[3] as f32 > 127.5).count();
    n as f32 / (img.width() * img.height()) as f32
}

/// Peak signal-to-noise ratio (dB) of channels `channels` of `a` against `b`.
fn psnr(a: &RgbaImage, b: &RgbaImage, channels: std::ops::Range<usize>) -> f64 {
    let (mut sum, mut n) = (0.0f64, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in channels.clone() {
            let d = p[c] as f64 - q[c] as f64;
            sum += d * d;
            n += 1;
        }
    }
    let mse = sum / n.max(1) as f64;
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Mean angle (degrees) between the normals of `a` and `b` (x, y in red and green; z rebuilt
/// unless `with_z`, then blue).
fn normal_error(a: &RgbaImage, b: &RgbaImage, with_z: bool) -> f64 {
    let n = |p: &image::Rgba<u8>| {
        let x = p[0] as f64 / 127.5 - 1.0;
        let y = p[1] as f64 / 127.5 - 1.0;
        let z = if with_z {
            p[2] as f64 / 127.5 - 1.0
        } else {
            (1.0 - x * x - y * y).max(0.0).sqrt()
        };
        let l = (x * x + y * y + z * z).sqrt().max(1e-6);
        [x / l, y / l, z / l]
    };
    let (mut sum, mut count) = (0.0, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let (u, v) = (n(p), n(q));
        let d = (u[0] * v[0] + u[1] * v[1] + u[2] * v[2]).clamp(-1.0, 1.0);
        sum += d.acos().to_degrees();
        count += 1;
    }
    sum / count.max(1) as f64
}

/// Coverage-keeping mips stay coverage-keeping in BC7: every level of a cutout keeps its
/// alpha-tested share within a few percent of the uncompressed chain.
#[test]
fn bc7_keeps_cutout_coverage() {
    let chain = super::trees::coverage_mips(image(128, 128, true), 2, 2);
    let c = generated::encode_chain(&chain, BcFormat::Bc7, true, true);
    for (l, src) in chain.iter().enumerate().take(5) {
        let (want, got) = (coverage(src), coverage(&c.decode(l)));
        assert!(
            (want - got).abs() < 0.02,
            "level {l}: coverage {got} instead of {want}"
        );
    }
}

/// One renderer-made texture of MeadowDemo: name, role, whether its normals carry z in blue,
/// whether it is an alpha-tested cutout, and its uncompressed slices.
struct Sample {
    name: String,
    role: Role,
    with_z: bool,
    cutout: bool,
    srgb: bool,
    chains: Vec<Vec<RgbaImage>>,
}

fn rgba_chains(layers: Vec<Made>) -> (Vec<Vec<RgbaImage>>, bool) {
    let mut srgb = false;
    let chains = layers
        .into_iter()
        .filter_map(|m| match m {
            Made::Rgba { chain, srgb: s } => {
                srgb = s;
                Some(chain)
            }
            Made::Blocks(_) => None,
        })
        .collect();
    (chains, srgb)
}

/// MeadowDemo's renderer-made textures, uncompressed (as RGBA8 mip chains).
fn meadow_samples() -> Vec<Sample> {
    crate::assets::set_project_root(&root());
    let off = Ctx::off();
    let mut out = Vec::new();
    let mut add = |name: String, role, with_z, cutout, layers: Vec<Made>| {
        let (chains, srgb) = rgba_chains(layers);
        out.push(Sample {
            name,
            role,
            with_z,
            cutout,
            srgb,
            chains,
        });
    };
    let parts = [
        ("bark", Role::ColorAlpha, false),
        ("bark normal", Role::Packed, false),
        ("leaves", Role::ColorAlpha, true),
        ("leaf normal", Role::Packed, false),
        ("impostor", Role::ColorAlpha, true),
        ("impostor normal", Role::Packed, true),
    ];
    for tree in ["Birch", "Linden", "Oak"] {
        let path = root().join(format!("Assets/Trees/{tree}.tree"));
        let asset = crate::trees::TreeAsset::load(&path).expect("tree");
        let built = std::sync::Arc::new(crate::trees::store::build(asset));
        let t = super::trees::TreeTextures::build(built, &off);
        for (i, layers) in t.textures.into_iter().enumerate() {
            let (part, role, flag) = parts[i];
            let with_z = part == "impostor normal";
            let cutout = role == Role::ColorAlpha && flag;
            add(
                format!("{tree} {part}"),
                role,
                with_z,
                cutout,
                layers.layers,
            );
        }
    }
    let terrain =
        crate::terrain::TerrainData::load(&root().join("Assets/Terrains/MeadowDemo.terrain"))
            .expect("terrain");
    let p = super::terrain_layers::prepare(&super::terrain_layers::key(&terrain.layers), &off);
    add(
        "terrain albedo".into(),
        Role::Opaque,
        false,
        false,
        p.albedo.layers,
    );
    add(
        "terrain normal + height".into(),
        Role::Packed,
        true,
        false,
        p.normal.layers,
    );
    add(
        "grass atlas".into(),
        Role::ColorAlpha,
        false,
        true,
        super::terrain_grass::atlas_layers(&off).layers,
    );
    let flowers = super::terrain_grass::flower_layers(&terrain.flowers.kinds, &off);
    let mut f = flowers.layers.layers.into_iter();
    add(
        "flower albedo".into(),
        Role::ColorAlpha,
        false,
        true,
        f.next().into_iter().collect(),
    );
    add(
        "flower material (sRGB-stored)".into(),
        Role::Packed,
        false,
        false,
        f.collect(),
    );
    out
}

/// Quality of each candidate format per renderer-made texture of MeadowDemo (level 0 and the
/// mips), with encode times: what the role table in `compress::generated` was chosen from.
#[test]
#[ignore = "prints only; decodes and encodes MeadowDemo's tree and terrain textures"]
fn made_textures_quality() {
    let samples = meadow_samples();
    println!(
        "texture | format | colour dB | alpha dB | normal error ° | coverage drift (levels ≥ 64 px / 16–32 px) | encode ms"
    );
    for s in &samples {
        let candidates: &[BcFormat] = match s.role {
            Role::Opaque => &[BcFormat::Bc1, BcFormat::Bc7],
            Role::ColorAlpha => &[BcFormat::Bc3, BcFormat::Bc7],
            Role::Packed => &[BcFormat::Bc3, BcFormat::Bc7],
        };
        for &format in candidates {
            let (mut colour, mut alpha, mut normal) = (0.0, 0.0, 0.0);
            let (mut drift, mut small) = (0.0f32, 0.0f32);
            let started = Instant::now();
            let encoded: Vec<Compressed> = s
                .chains
                .iter()
                .map(|c| generated::encode_chain(c, format, s.srgb, s.cutout))
                .collect();
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            for (chain, c) in s.chains.iter().zip(&encoded) {
                let d0 = c.decode(0);
                if s.role == Role::Packed {
                    normal += normal_error(&chain[0], &d0, s.with_z);
                    colour += psnr(&chain[0], &d0, if s.with_z { 3..4 } else { 2..4 });
                } else {
                    colour += psnr(&chain[0], &d0, 0..3);
                }
                alpha += psnr(&chain[0], &d0, 3..4);
                if s.cutout {
                    for (l, src) in chain.iter().enumerate() {
                        if src.width() < 16 || src.height() < 16 {
                            break;
                        }
                        let d = (coverage(src) - coverage(&c.decode(l))).abs();
                        if src.width().min(src.height()) >= 64 {
                            drift = drift.max(d);
                        } else {
                            small = small.max(d);
                        }
                    }
                }
            }
            let n = s.chains.len().max(1) as f64;
            println!(
                "{} | {} | {:.1} | {:.1} | {} | {} | {ms:.0}",
                s.name,
                format.label(),
                colour / n,
                alpha / n,
                if s.role == Role::Packed {
                    format!("{:.2}", normal / n)
                } else {
                    "-".into()
                },
                if s.cutout {
                    format!("{:.2} % / {:.2} %", drift * 100.0, small * 100.0)
                } else {
                    "-".into()
                },
            );
        }
    }
}

/// The MeadowDemo views measured: (name, view).
fn views() -> Vec<(&'static str, crate::render_scene::GameView)> {
    use super::terrain_grass::gpu_tests::{Shot as GrassShot, meadow_view};
    use super::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
    vec![
        ("wide", meadow_view(GrassShot::WideGolden)),
        ("behind", tree_view(TreeShot::Behind)),
        ("oak", tree_view(TreeShot::Close)),
    ]
}

/// MeadowDemo's renderer-made textures on the GPU, uncompressed and compressed, per category
/// (high quality, the behind view: every tree, the terrain and the flowers are loaded).
#[test]
#[ignore = "prints only; renders MeadowDemo"]
fn made_textures_memory() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (_, view) = views().swap_remove(1);
    let mut rows = Vec::new();
    for on in [false, true] {
        generated::set_enabled(on);
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::high());
        let target = r.create_target(&device, 640, 360);
        let frame = view.frame();
        let started = Instant::now();
        for _ in 0..4 {
            r.render(&device, &queue, &target, &frame);
        }
        let load = started.elapsed().as_secs_f32();
        rows.push((on, r.made_texture_bytes(), r.memory().texture_bytes, load));
    }
    generated::set_enabled(true);
    println!("category | uncompressed MB | compressed MB | saved");
    let (off, on) = (&rows[0], &rows[1]);
    let (mut a, mut b) = (0u64, 0u64);
    for ((name, x), (_, y)) in off.1.iter().zip(on.1.iter()) {
        a += x;
        b += y;
        println!(
            "{name} | {:.1} | {:.1} | {:.0} %",
            *x as f64 / 1e6,
            *y as f64 / 1e6,
            100.0 * (1.0 - *y as f64 / (*x).max(1) as f64)
        );
    }
    println!(
        "total | {:.1} | {:.1} | {:.0} %",
        a as f64 / 1e6,
        b as f64 / 1e6,
        100.0 * (1.0 - b as f64 / a.max(1) as f64)
    );
    println!(
        "all GPU texture memory (Memory module) | {:.1} | {:.1} | first four frames {:.2} s / {:.2} s",
        off.2 as f64 / 1e6,
        on.2 as f64 / 1e6,
        off.3,
        on.3
    );
    assert!(b < a, "compression saved nothing");
}

/// GPU frame time with the renderer-made textures uncompressed and compressed, alternating
/// (`MISTY_ROUNDS`, 3) per view (`MISTY_GFX_SHOT`, all) and level (`MISTY_QUALITY`, high),
/// 1920×1080: medians.
#[test]
#[ignore = "timing, prints only"]
fn made_textures_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let rounds: usize = std::env::var("MISTY_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    let levels = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high".into());
    let size = (1920, 1080);
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let views = views();
    // A first run warms the GPU clocks up.
    super::terrain_grass::gpu_tests::breakdown(
        &device,
        &queue,
        &views[0].1,
        &QualityLevel::low(),
        size,
    );
    for level in levels.split(',') {
        let quality = match level {
            "ultra" => QualityLevel::ultra(),
            "medium" => QualityLevel::medium(),
            "low" => QualityLevel::low(),
            _ => QualityLevel::high(),
        };
        let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
        for (name, view) in &views {
            if !shots.is_empty() && !shots.split(',').any(|s| s == *name) {
                continue;
            }
            let (mut off, mut on) = (Vec::new(), Vec::new());
            for _ in 0..rounds {
                for compressed in [false, true] {
                    generated::set_enabled(compressed);
                    let b = super::terrain_grass::gpu_tests::breakdown(
                        &device, &queue, view, &quality, size,
                    );
                    if compressed {
                        on.push(b.gpu);
                    } else {
                        off.push(b.gpu);
                    }
                }
            }
            generated::set_enabled(true);
            let (a, b) = (median(off.clone()), median(on.clone()));
            println!(
                "{name} {level}: uncompressed {a:.2} ms, compressed {b:.2} ms ({:+.2} ms) runs {off:.2?} / {on:.2?}",
                b - a
            );
        }
    }
}

/// `view` turned towards pixel (`x`, `y`) of its 800×450 picture and zoomed to `fov_y` degrees
/// (a telephoto close-up from the same spot).
fn aimed(
    mut view: crate::render_scene::GameView,
    x: f32,
    y: f32,
    fov_y: f32,
) -> crate::render_scene::GameView {
    use crate::camera::Projection;
    let Projection::Perspective { fov_y: base } = view.camera.projection else {
        return view;
    };
    let t = (base.to_radians() * 0.5).tan();
    let (nx, ny) = (x / 400.0 - 1.0, 1.0 - y / 225.0);
    let d = (view.camera.rotation * glam::Vec3::new(nx * t * 16.0 / 9.0, ny * t, 1.0)).normalize();
    view.camera.rotation =
        glam::Quat::from_euler(glam::EulerRot::YXZ, d.x.atan2(d.z), -d.y.asin(), 0.0);
    view.camera.projection = Projection::Perspective { fov_y };
    view
}

/// Close-ups of bark, leaves, flowers and terrain with the renderer-made textures uncompressed
/// and compressed (1600×900, 48 frames, high): `texcomp_<view>_{off,on}.png` in
/// `MISTY_GFX_DIR`, telephoto views from MeadowDemo's look-check spots.
#[test]
#[ignore = "writes look-check renders; run on purpose"]
fn made_textures_closeups() {
    use super::terrain_grass::gpu_tests::{Shot as GrassShot, meadow_view};
    use super::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_GFX_DIR").expect("MISTY_GFX_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    // 20 m towards the broad oak (the `oakunder` look check).
    let oak = || {
        let mut v = tree_view(TreeShot::Close);
        let f = v.camera.forward();
        v.camera.position += glam::Vec3::new(f.x, 0.0, f.z).normalize_or_zero() * 20.0;
        v
    };
    let views: Vec<(&str, crate::render_scene::GameView)> = vec![
        ("bark", aimed(oak(), 518.0, 335.0, 5.0)),
        ("leaves", aimed(oak(), 560.0, 120.0, 8.0)),
        (
            "flowers",
            aimed(tree_view(TreeShot::Behind), 165.0, 266.0, 5.0),
        ),
        (
            "terrain_cliff",
            aimed(tree_view(TreeShot::Behind), 700.0, 175.0, 6.0),
        ),
        (
            "terrain_ground",
            aimed(tree_view(TreeShot::Behind), 420.0, 292.0, 5.0),
        ),
        ("meadow", meadow_view(GrassShot::CloseGolden)),
    ];
    let filter = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    let (w, h) = (1600, 900);
    for (name, view) in &views {
        if !filter.is_empty() && !filter.split(',').any(|f| f == *name) {
            continue;
        }
        for on in [false, true] {
            generated::set_enabled(on);
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::high());
            let target = r.create_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..48 {
                r.render(&device, &queue, &target, &frame);
            }
            let rgba = super::gpu_tests::read_back(&device, &queue, &target);
            let img = RgbaImage::from_raw(w, h, rgba).expect("read back");
            let path = dir.join(format!(
                "texcomp_{name}_{}.png",
                if on { "on" } else { "off" }
            ));
            img.save(&path).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
    generated::set_enabled(true);
}

/// BC7 encoder settings compared on MeadowDemo's renderer-made textures (quality and time).
#[test]
#[ignore = "prints only"]
fn made_textures_bc7_settings() {
    use block_compression::{BC7Settings, CompressionVariant};
    let samples = meadow_samples();
    let settings: [(&str, BC7Settings); 4] = [
        ("ultrafast", BC7Settings::alpha_ultrafast()),
        ("very fast", BC7Settings::alpha_very_fast()),
        ("fast", BC7Settings::alpha_fast()),
        ("basic", BC7Settings::alpha_basic()),
    ];
    let encode = |img: &RgbaImage, s: BC7Settings| -> RgbaImage {
        let (w, h) = img.dimensions();
        if w % 4 != 0 || h % 4 != 0 {
            return img.clone();
        }
        let v = CompressionVariant::BC7(s);
        let mut blocks = vec![0u8; v.blocks_byte_size(w, h)];
        block_compression::encode::compress_rgba8(v, img.as_raw(), &mut blocks, w, h, w * 4);
        let mut out = vec![0u8; (w * h * 4) as usize];
        block_compression::decode::decompress_blocks_as_rgba8(v, w, h, &blocks, &mut out);
        RgbaImage::from_raw(w, h, out).unwrap()
    };
    for s in samples
        .iter()
        .filter(|s| !s.name.contains("Linden") && !s.name.contains("Birch"))
    {
        for (name, set) in settings {
            let started = Instant::now();
            let (mut q, mut drift) = (0.0, 0.0f32);
            for chain in &s.chains {
                let d0 = encode(&chain[0], set);
                q += if s.role == Role::Packed {
                    normal_error(&chain[0], &d0, s.with_z)
                } else {
                    psnr(&chain[0], &d0, 0..3)
                };
                if s.cutout {
                    for src in chain.iter().skip(1).take_while(|m| m.width() >= 16) {
                        drift = drift.max((coverage(src) - coverage(&encode(src, set))).abs());
                    }
                }
            }
            println!(
                "{} | {name} | {:.2} {} | drift {:.2} % | {:.0} ms",
                s.name,
                q / s.chains.len() as f64,
                if s.role == Role::Packed { "°" } else { "dB" },
                drift * 100.0,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}

/// A build bakes the blocks of the textures the renderer makes from the packed tree and
/// terrain assets into the project's cache, and a player (cache only) uploads exactly those.
#[test]
fn builds_bake_tree_and_terrain_textures() {
    crate::assets::set_project_root(&root());
    let cache = std::env::temp_dir().join(format!("misty_made_bake_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let files = [
        "Assets/Trees/Birch.tree".to_owned(),
        "Assets/Terrains/MeadowDemo.terrain".to_owned(),
        "Assets/Scenes/MeadowDemo.scene".to_owned(),
    ];
    let (entries, warnings) = super::bake_made_textures(&cache, &files, "Windows");
    assert!(warnings.is_empty(), "{warnings:?}");
    // Birch: bark, bark normal, leaves, leaf normal, impostor, impostor normal; the terrain:
    // four albedo slices (the normal + height array stays RGBA8: its meadow thatch slice is
    // noise to BC7), the grass atlas and two flower layers.
    assert_eq!(entries.len(), 6 + 4 + 1 + 2, "{entries:?}");
    for e in &entries {
        assert!(cache.join(e).is_file(), "{e}");
    }
    // What a player (cache only) builds from the same images is those blocks.
    let player = Ctx {
        mode: crate::sprites::compress::Mode::CacheOnly,
        ..Ctx::bake(&cache, "Windows")
    };
    let terrain = crate::terrain::TerrainData::load(&root().join(&files[1])).expect("terrain");
    let p = super::terrain_layers::prepare(&super::terrain_layers::key(&terrain.layers), &player);
    for (layers, format) in [
        (&p.albedo, wgpu::TextureFormat::Bc1RgbaUnormSrgb),
        (&p.normal, wgpu::TextureFormat::Rgba8Unorm),
    ] {
        assert!(layers.layers.iter().all(|m| m.format() == format));
    }
    let flowers = super::terrain_grass::flower_layers(&terrain.flowers.kinds, &player);
    assert!(
        flowers
            .layers
            .layers
            .iter()
            .all(|m| m.format() == wgpu::TextureFormat::Bc7RgbaUnormSrgb)
    );
    let _ = std::fs::remove_dir_all(&cache);
}
