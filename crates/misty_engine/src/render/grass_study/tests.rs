//! The study's measurements (all ignored; run on purpose in release, one at a time):
//! `cargo test --release -p misty_engine --lib study_ -- --ignored --nocapture --test-threads=1`
//!
//! - `study_calc`: blades, triangles and projected sizes per distance band, and the fragment
//!   counters (lanes launched incl. 2×2 quad helpers, fragments kept, visible grass pixels).
//! - `study_times`: GPU ms per pass of every variant per quality level and shot (1920×1080).
//! - `study_shots`: look comparisons (variant | A | difference ×8) and TAA shimmer.
//! - `study_motion`: walking and turning: H, F, B and A1 against A frame by frame; G with
//!   fast clouds against A.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Quat, Vec2, Vec3, Vec4};

use super::field::{Field, FieldSettings, band_stats};
use super::gpu::{Lighting, Path, Study, Variant, View, slot};
use crate::camera::{Camera, Projection};
use crate::render::pass_timer::PassTimer;
use crate::render::terrain_grass::GrassParams;
use crate::render::terrain_grass::gpu_tests::{Shot, meadow_view};
use crate::render_scene::GameView;
use crate::terrain::TerrainData;
use crate::terrain::details::DetailSettings;

const SIZE: (u32, u32) = (1920, 1080);
const GRASS_TIME: f32 = 3.0;

/// Every study shader module parses and validates (both builds), without a GPU.
#[test]
fn study_shaders_validate() {
    use wgpu::naga;
    for count in [false, true] {
        for kind in super::gpu::KINDS {
            let src = super::gpu::module_source(kind, count);
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{kind}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{kind} (count {count}): {e:?}"));
        }
    }
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

fn study_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let wanted = wgpu::Features::SUBGROUP
        | wgpu::Features::SHADER_INT64
        | wgpu::Features::SHADER_INT64_ATOMIC_MIN_MAX;
    let features = crate::render::timestamp_features(&adapter) | (adapter.features() & wanted);
    let info = adapter.get_info();
    eprintln!(
        "adapter: {} ({:?}); subgroups {}, 64-bit atomics {}",
        info.name,
        info.backend,
        features.contains(wgpu::Features::SUBGROUP),
        features.contains(wgpu::Features::SHADER_INT64_ATOMIC_MIN_MAX)
    );
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grass study device"),
        required_features: features,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StudyShot {
    Wide,
    Close,
    Behind,
}

impl StudyShot {
    const ALL: [StudyShot; 3] = [StudyShot::Wide, StudyShot::Close, StudyShot::Behind];

    fn name(self) -> &'static str {
        match self {
            StudyShot::Wide => "wide",
            StudyShot::Close => "close",
            StudyShot::Behind => "behind",
        }
    }
}

struct Scene {
    camera: Camera,
    terrain: Arc<TerrainData>,
    origin: Vec3,
    details: DetailSettings,
    wind: Option<crate::render::environment::Wind>,
    lighting: Lighting,
    game: GameView,
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn scene(shot: StudyShot) -> Scene {
    let game = meadow_view(match shot {
        StudyShot::Close => Shot::CloseGolden,
        _ => Shot::WideGolden,
    });
    let mut camera = game.camera.clone();
    if shot == StudyShot::Behind {
        camera.rotation = Quat::from_rotation_y(std::f32::consts::PI) * camera.rotation;
    }
    let t = game.terrains.first().expect("MeadowDemo terrain");
    let sun = game
        .lights
        .iter()
        .find(|l| l.kind == crate::scene::LightType::Directional)
        .expect("sun");
    let color = Vec3::new(lin(sun.color[0]), lin(sun.color[1]), lin(sun.color[2]));
    let lighting = Lighting {
        sun_dir: sun.direction.normalize(),
        sun: color * sun.intensity * 1.6,
        shadows: true,
        sky: Vec3::new(0.32, 0.42, 0.6),
        equator: Vec3::new(0.5, 0.47, 0.4),
        ground: Vec3::new(0.2, 0.17, 0.12),
        ambient: 1.0,
        fog: Vec4::new(0.62, 0.62, 0.6, 0.0012),
    };
    Scene {
        camera,
        terrain: Arc::clone(&t.data),
        origin: t.position,
        details: t.data.details.clone(),
        wind: game.environment.wind.clone(),
        lighting,
        game,
    }
}

#[derive(Clone, Copy)]
struct Level {
    name: &'static str,
    density: f32,
    shadows: bool,
}

const LEVELS: [Level; 3] = [
    Level {
        name: "high",
        density: 1.0,
        shadows: true,
    },
    Level {
        name: "medium",
        density: 0.75,
        shadows: true,
    },
    Level {
        name: "low",
        density: 0.5,
        shadows: false,
    },
];

fn variants() -> Vec<Variant> {
    let a = Variant::forward("A", "baseline: depth prepass + lit pass, 4x MSAA + A2C");
    vec![
        a.clone(),
        Variant {
            name: "A1",
            label: "A single-sample, stochastic coverage",
            path: Path::Forward { samples: 1 },
            ..a.clone()
        },
        Variant {
            name: "B",
            label: "visibility buffer + compute shading",
            path: Path::Vis {
                swr: false,
                quarter: false,
            },
            ..a.clone()
        },
        Variant {
            name: "B4",
            label: "visibility buffer at 4x MSAA (sample-mask coverage), each blade of a pixel shaded once",
            path: Path::VisMs { samples: 4 },
            ..a.clone()
        },
        Variant {
            name: "B2",
            label: "visibility buffer at 2x MSAA (sample-mask coverage)",
            path: Path::VisMs { samples: 2 },
            ..a.clone()
        },
        Variant {
            name: "C",
            label: "B + compute raster for blades < 1.5 px",
            path: Path::Vis {
                swr: true,
                quarter: false,
            },
            ..a.clone()
        },
        Variant {
            name: "D",
            label: "A with 3x fewer, 3x wider clump cards past ~10-20 m",
            clump: 3.0,
            ..a.clone()
        },
        Variant {
            name: "E",
            label: "A with blades to 40 m, parallax grass layer beyond",
            far_field: Some(40.0),
            ..a.clone()
        },
        Variant {
            name: "F",
            label: "B shaded at quarter rate (rotating 2x2 anchor)",
            path: Path::Vis {
                swr: false,
                quarter: true,
            },
            ..a.clone()
        },
        Variant {
            name: "G",
            label: "A with the world-space sun visibility cache",
            cache: true,
            ..a.clone()
        },
        Variant {
            name: "H",
            label: "A, half the blades past 12 m per frame",
            split: Some((12.0, 1.0)),
            ..a.clone()
        },
        Variant {
            name: "H2",
            label: "H with the split blades 1.4x wider",
            split: Some((12.0, 1.4)),
            ..a.clone()
        },
        Variant {
            name: "BE",
            label: "B + E (vis buffer, blades to 40 m, parallax beyond)",
            path: Path::Vis {
                swr: false,
                quarter: false,
            },
            far_field: Some(40.0),
            ..a.clone()
        },
        Variant {
            name: "B4H",
            label: "B4 + H (half the blades past 12 m per frame, 1.4x wider)",
            path: Path::VisMs { samples: 4 },
            split: Some((12.0, 1.4)),
            ..a.clone()
        },
        Variant {
            name: "BEG",
            label: "B + E + G",
            path: Path::Vis {
                swr: false,
                quarter: false,
            },
            far_field: Some(40.0),
            cache: true,
            ..a.clone()
        },
        Variant {
            name: "AE",
            label: "A + E at 60 m",
            far_field: Some(60.0),
            ..a.clone()
        },
        Variant {
            name: "AEG",
            label: "A + E(40 m) + G",
            far_field: Some(40.0),
            cache: true,
            ..a
        },
    ]
}

/// Variants picked by `MISTY_STUDY=A,B,...` (all when unset).
fn picked() -> Vec<Variant> {
    let only = std::env::var("MISTY_STUDY").unwrap_or_default();
    variants()
        .into_iter()
        .filter(|v| only.is_empty() || only.split(',').any(|n| n == v.name))
        .collect()
}

fn pixel_angle(cam: &Camera, h: u32) -> f32 {
    match cam.projection {
        Projection::Perspective { fov_y } => 2.0 * (fov_y.to_radians() * 0.5).tan() / h as f32,
        Projection::Orthographic { .. } => 0.0,
    }
}

fn view_of(cam: &Camera, size: (u32, u32)) -> View {
    View {
        eye: cam.position,
        view_proj: cam.view_projection(size.0 as f32 / size.1 as f32),
        pixel: pixel_angle(cam, size.1),
    }
}

fn details_for(sc: &Scene, v: &Variant) -> DetailSettings {
    let mut d = sc.details.clone();
    if let Some(far) = v.far_field {
        d.draw_distance = far;
    }
    d
}

fn grass_params(sc: &Scene, v: &Variant, view: &View, density: f32, caster: bool) -> GrassParams {
    GrassParams::new(
        &details_for(sc, v),
        sc.origin,
        GRASS_TIME,
        0,
        view.eye,
        sc.wind.as_ref(),
        caster.then_some((24.0, 1.0)),
        [view.pixel, if caster { 0.0 } else { 1.0 }, 0.0, 0.0],
        density,
    )
}

fn field_settings(sc: &Scene, v: &Variant, density: f32) -> FieldSettings {
    FieldSettings {
        density,
        clump: v.clump,
        draw_distance: details_for(sc, v).draw_distance,
    }
}

fn field(sc: &Scene, v: &Variant, view: &View, density: f32) -> Field {
    Field::build(
        &sc.terrain,
        sc.origin,
        view.eye,
        &view.view_proj,
        field_settings(sc, v, density),
    )
}

fn halton(mut i: u32, base: u32) -> f32 {
    let mut f = 1.0;
    let mut r = 0.0;
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

fn jitter(i: u32) -> Vec2 {
    let k = i % 8 + 1;
    Vec2::new(halton(k, 2) - 0.5, halton(k, 3) - 0.5)
}

/// A study for `sc` at `level` with the baseline field's shadows baked and `v`'s field set.
fn prepare(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    st: &mut Study,
    sc: &Scene,
    v: &Variant,
    view: &View,
    level: Level,
) -> Field {
    let base = Variant::forward("A", "");
    st.lighting.shadows = level.shadows;
    let f0 = field(sc, &base, view, level.density);
    st.set_field(device, &f0);
    st.bake_shadows(device, queue, view.eye, &grass_params(sc, &base, view, level.density, true));
    let f = field(sc, v, view, level.density);
    st.set_field(device, &f);
    f
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
}

/// GPU ms per pass of `v` (30 frames to settle, the median of 60 timed).
#[allow(clippy::too_many_arguments)]
fn time_variant(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    st: &mut Study,
    sc: &Scene,
    v: &Variant,
    view: &View,
    density: f32,
) -> Vec<(&'static str, f32)> {
    let gp = grass_params(sc, v, view, density, false);
    for i in 0..30 {
        st.frame(device, queue, v, view, jitter(i), i, SIZE, &gp, 0.0, None);
        if i == 0 && v.cache {
            st.fill_cache(device, queue, v);
        }
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }
    let Some(mut timer) = PassTimer::new(device, queue) else {
        return Vec::new();
    };
    let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
    for i in 30..90 {
        st.frame(device, queue, v, view, jitter(i), i, SIZE, &gp, 0.0, Some(&timer));
        timer.read(device);
        for &(label, ms) in timer.last() {
            match parts.iter_mut().find(|(l, _)| *l == label) {
                Some(p) => p.1.push(ms),
                None => parts.push((label, vec![ms])),
            }
        }
    }
    parts.into_iter().map(|(l, v)| (l, median(v))).collect()
}

fn grass_ms(parts: &[(&'static str, f32)]) -> f32 {
    parts
        .iter()
        .filter(|(l, _)| l.starts_with("grass") || l.starts_with("G "))
        .map(|p| p.1)
        .sum()
}

#[test]
#[ignore = "timing, prints only"]
fn study_times() {
    let Some((device, queue)) = study_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let shots: Vec<StudyShot> = match std::env::var("MISTY_GFX_SHOT").as_deref() {
        Ok("all") => StudyShot::ALL.to_vec(),
        Ok("close") => vec![StudyShot::Close],
        Ok("behind") => vec![StudyShot::Behind],
        _ => vec![StudyShot::Wide],
    };
    let levels = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high,medium,low".into());
    for shot in shots {
        let sc = scene(shot);
        let view = view_of(&sc.camera, SIZE);
        let mut st = Study::new(&device, &queue, Arc::clone(&sc.terrain), sc.origin, sc.lighting, false);
        st.build_ground(&device, view.eye);
        // Warm the GPU clocks up.
        let base = Variant::forward("A", "");
        prepare(&device, &queue, &mut st, &sc, &base, &view, LEVELS[0]);
        time_variant(&device, &queue, &mut st, &sc, &base, &view, 1.0);
        let rounds: u32 = std::env::var("MISTY_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(3);
        for level in LEVELS.iter().filter(|l| levels.contains(l.name)) {
            // Interleaved rounds; each variant keeps its fastest round (other work on the
            // GPU only ever adds time).
            let vs = picked();
            let mut best: Vec<Option<(f32, Vec<(&'static str, f32)>, usize, u64)>> = vec![None; vs.len()];
            for _ in 0..rounds {
                for (k, v) in vs.iter().enumerate() {
                    let f = prepare(&device, &queue, &mut st, &sc, v, &view, *level);
                    let parts = time_variant(&device, &queue, &mut st, &sc, v, &view, level.density);
                    let total: f32 = parts.iter().map(|p| p.1).sum();
                    if best[k].as_ref().is_none_or(|b| total < b.0) {
                        best[k] = Some((total, parts, f.drawn.len(), f.triangles()));
                    }
                }
            }
            let mut base_ms = None;
            for (v, b) in vs.iter().zip(best) {
                let Some((total, parts, blades, tris)) = b else {
                    continue;
                };
                if level.name == "high" {
                    eprintln!("-- {}: {}", v.name, v.label);
                }
                let g = grass_ms(&parts);
                let ground = parts.iter().find(|p| p.0 == "ground").map_or(0.0, |p| p.1);
                let b = *base_ms.get_or_insert(g + ground);
                let list: Vec<String> = parts.iter().map(|(l, m)| format!("{l} {m:.2}")).collect();
                eprintln!(
                    "{} {} {:<4} grass {g:6.2} ms  ground {ground:5.2}  grass+ground {:6.2} ({:+.2} vs A)  frame {total:6.2}  {:.2} M blades {:.1} M tris | {}",
                    shot.name(),
                    level.name,
                    v.name,
                    g + ground,
                    g + ground - b,
                    blades as f64 / 1e6,
                    tris as f64 / 1e6,
                    list.join(", ")
                );
            }
        }
    }
}

/// Mean |a - b| (max channel) in levels, share of pixels off by > 8 and > 24 levels.
fn diff_stats(a: &[u8], b: &[u8]) -> (f32, f32, f32) {
    let n = a.len() / 4;
    let (mut sum, mut over8, mut over24) = (0u64, 0usize, 0usize);
    for i in 0..n {
        let d = (0..3)
            .map(|c| (a[i * 4 + c] as i32 - b[i * 4 + c] as i32).abs())
            .max()
            .unwrap();
        sum += d as u64;
        over8 += usize::from(d > 8);
        over24 += usize::from(d > 24);
    }
    (
        sum as f32 / n as f32,
        over8 as f32 / n as f32 * 100.0,
        over24 as f32 / n as f32 * 100.0,
    )
}

fn luma(p: &[u8]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

/// Mean luma of rows `from..to` (shares of the height).
fn band_luma(img: &[u8], (w, h): (u32, u32), from: f32, to: f32) -> f32 {
    let (y0, y1) = ((h as f32 * from) as u32, (h as f32 * to) as u32);
    let mut sum = 0.0;
    for y in y0..y1 {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            sum += luma(&img[i..i + 4]);
        }
    }
    sum / ((y1 - y0) * w) as f32
}

/// Render `frames` frames of `v` (TAA converges) and keep the last `keep` images.
#[allow(clippy::too_many_arguments)]
fn render_images(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    st: &mut Study,
    sc: &Scene,
    v: &Variant,
    cams: &dyn Fn(u32) -> Camera,
    density: f32,
    frames: u32,
    keep: u32,
    cloud: &dyn Fn(u32) -> f32,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for i in 0..frames {
        let view = view_of(&cams(i), SIZE);
        let gp = grass_params(sc, v, &view, density, false);
        st.frame(device, queue, v, &view, jitter(i), i, SIZE, &gp, cloud(i), None);
        if i == 0 && v.cache {
            st.fill_cache(device, queue, v);
        }
        if i + keep >= frames {
            out.push(st.read_image(device, queue));
        } else {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
    }
    out
}

/// Mean temporal standard deviation of luma (levels) over the lower half of the frame.
fn shimmer(images: &[Vec<u8>], (w, h): (u32, u32)) -> f32 {
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in h / 2..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let l: Vec<f32> = images.iter().map(|img| luma(&img[i..i + 4])).collect();
            let m = l.iter().sum::<f32>() / l.len() as f32;
            let var = l.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / l.len() as f32;
            sum += var.sqrt() as f64;
            n += 1;
        }
    }
    (sum / n as f64) as f32
}

fn to_image(rgba: &[u8], (w, h): (u32, u32)) -> image::RgbaImage {
    image::RgbaImage::from_raw(w, h, rgba.to_vec()).expect("image")
}

fn diff_image(a: &[u8], b: &[u8], (w, h): (u32, u32)) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(w, h);
    for (i, p) in out.pixels_mut().enumerate() {
        for c in 0..3 {
            let d = (a[i * 4 + c] as i32 - b[i * 4 + c] as i32).unsigned_abs() * 8;
            p[c] = d.min(255) as u8;
        }
        p[3] = 255;
    }
    out
}

/// variant | A | difference ×8: a 640×360 full-resolution crop of each, then the three
/// whole frames at a third of the size.
fn write_strip(file: &str, v: &[u8], a: &[u8]) {
    let size = SIZE;
    let (cw, ch) = (640u32, 360u32);
    let (cx, cy) = (size.0 / 2 - cw / 2, (size.1 as f32 * 0.6) as u32 - ch / 2);
    let full = [to_image(v, size), to_image(a, size), diff_image(v, a, size)];
    let mut out = image::RgbaImage::new(cw * 3 + 16, ch + 8 + size.1 / 3);
    for (k, img) in full.iter().enumerate() {
        let crop = image::imageops::crop_imm(img, cx, cy, cw, ch).to_image();
        image::imageops::replace(&mut out, &crop, (k as u32 * (cw + 8)) as i64, 0);
        let small = image::imageops::resize(img, size.0 / 3, size.1 / 3, image::imageops::FilterType::Triangle);
        image::imageops::replace(&mut out, &small, (k as u32 * (cw + 8)) as i64, (ch + 8) as i64);
    }
    let path = screenshots().join(file);
    out.save(&path).expect("save");
    eprintln!("wrote {}", path.display());
}

#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn study_shots() {
    let Some((device, queue)) = study_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let level = LEVELS[0];
    let mut finals: Vec<(StudyShot, &'static str, Vec<u8>)> = Vec::new();
    for shot in StudyShot::ALL {
        let sc = scene(shot);
        let view = view_of(&sc.camera, SIZE);
        let mut st = Study::new(&device, &queue, Arc::clone(&sc.terrain), sc.origin, sc.lighting, false);
        st.build_ground(&device, view.eye);
        let cam = sc.camera.clone();
        let cams = move |_: u32| cam.clone();
        let mut base: Option<Vec<u8>> = None;
        for v in picked() {
            prepare(&device, &queue, &mut st, &sc, &v, &view, level);
            let imgs = render_images(&device, &queue, &mut st, &sc, &v, &cams, level.density, 64, 16, &|_| 0.0);
            let last = imgs.last().expect("image").clone();
            let sh = shimmer(&imgs, SIZE);
            let a = base.get_or_insert_with(|| last.clone()).clone();
            let (mean, o8, o24) = diff_stats(&last, &a);
            let bands: Vec<String> = [(0.45, 0.55), (0.55, 0.65), (0.65, 0.8), (0.8, 1.0)]
                .iter()
                .map(|&(f, t)| format!("{:.1}", band_luma(&last, SIZE, f, t) - band_luma(&a, SIZE, f, t)))
                .collect();
            eprintln!(
                "{} {:<4} vs A: mean {mean:.2}, >8 {o8:.2}%, >24 {o24:.3}% | luma delta rows 45-55/55-65/65-80/80-100%: {} | shimmer (temporal std, lower half) {sh:.2} levels",
                shot.name(),
                v.name,
                bands.join(" / ")
            );
            if v.name != "A" {
                write_strip(&format!("study_{}_{}.png", v.name, shot.name()), &last, &a);
            } else {
                let path = screenshots().join(format!("study_A_{}.png", shot.name()));
                to_image(&last, SIZE).save(&path).expect("save");
            }
            finals.push((shot, v.name, last));
        }
    }
    // Overview per shot: every variant at a quarter size, in the order of `variants()`.
    for shot in StudyShot::ALL {
        let imgs: Vec<&(StudyShot, &str, Vec<u8>)> = finals.iter().filter(|f| f.0 == shot).collect();
        let (w, h) = (SIZE.0 / 4, SIZE.1 / 4);
        let cols = 4u32;
        let rows = (imgs.len() as u32).div_ceil(cols);
        let mut out = image::RgbaImage::new(cols * (w + 4), rows * (h + 4));
        for (k, f) in imgs.iter().enumerate() {
            let small = image::imageops::resize(&to_image(&f.2, SIZE), w, h, image::imageops::FilterType::Triangle);
            let (x, y) = (k as u32 % cols, k as u32 / cols);
            image::imageops::replace(&mut out, &small, (x * (w + 4)) as i64, (y * (h + 4)) as i64);
        }
        let path = screenshots().join(format!("study_overview_{}.png", shot.name()));
        out.save(&path).expect("save");
        eprintln!("wrote {} ({})", path.display(), imgs.iter().map(|f| f.1).collect::<Vec<_>>().join(", "));
    }
    // Next to the reference photo: the reference | A | the recommended combination (wide).
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../MistyDesign/grass_reference.png");
    let pick = std::env::var("MISTY_STUDY_PICK").unwrap_or_else(|_| "BE".into());
    if let (Ok(r), Some(a), Some(p)) = (
        image::open(&reference),
        finals.iter().find(|f| f.0 == StudyShot::Wide && f.1 == "A"),
        finals.iter().find(|f| f.0 == StudyShot::Wide && f.1 == pick),
    ) {
        let h = 540;
        let r = r.to_rgba8();
        let rw = r.width() * h / r.height();
        let r = image::imageops::resize(&r, rw, h, image::imageops::FilterType::Triangle);
        let a = image::imageops::resize(&to_image(&a.2, SIZE), 960, h, image::imageops::FilterType::Triangle);
        let p = image::imageops::resize(&to_image(&p.2, SIZE), 960, h, image::imageops::FilterType::Triangle);
        let mut out = image::RgbaImage::new(rw + 8 + 960 + 8 + 960, h);
        image::imageops::replace(&mut out, &r, 0, 0);
        image::imageops::replace(&mut out, &a, (rw + 8) as i64, 0);
        image::imageops::replace(&mut out, &p, (rw + 16 + 960) as i64, 0);
        let path = screenshots().join("study_vs_reference.png");
        out.save(&path).expect("save");
        eprintln!("wrote {}", path.display());
    }
}

/// The live renderer's own frame of the wide golden shot (High, 1920×1080) next to the
/// study's A: how close the study's stand-in lighting is to the engine's.
#[test]
#[ignore = "writes a comparison render into docs/screenshots/gfx; run on purpose"]
fn study_live_vs_study() {
    let Some((device, queue)) = study_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    crate::render::set_grass_streaming(false);
    for shot in [StudyShot::Wide, StudyShot::Close] {
        let sc = scene(shot);
        let mut r = crate::render::Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &crate::quality::QualityLevel::high());
        let target = r.create_target(&device, SIZE.0, SIZE.1);
        let game = sc.game.frame();
        for _ in 0..48 {
            r.render(&device, &queue, &target, &game);
        }
        let live = crate::render::gpu_tests::read_back(&device, &queue, &target);
        let view = view_of(&sc.camera, SIZE);
        let mut st = Study::new(&device, &queue, Arc::clone(&sc.terrain), sc.origin, sc.lighting, false);
        st.build_ground(&device, view.eye);
        let a = Variant::forward("A", "");
        prepare(&device, &queue, &mut st, &sc, &a, &view, LEVELS[0]);
        let cam = sc.camera.clone();
        let imgs = render_images(&device, &queue, &mut st, &sc, &a, &move |_| cam.clone(), 1.0, 48, 1, &|_| 0.0);
        let study = &imgs[0];
        eprintln!(
            "{}: mean luma rows 50-100%: live {:.1}, study {:.1}",
            shot.name(),
            band_luma(&live, SIZE, 0.5, 1.0),
            band_luma(study, SIZE, 0.5, 1.0)
        );
        let (w, h) = (SIZE.0 / 2, SIZE.1 / 2);
        let mut out = image::RgbaImage::new(w * 2 + 8, h);
        let l = image::imageops::resize(&to_image(&live, SIZE), w, h, image::imageops::FilterType::Triangle);
        let s = image::imageops::resize(&to_image(study, SIZE), w, h, image::imageops::FilterType::Triangle);
        image::imageops::replace(&mut out, &l, 0, 0);
        image::imageops::replace(&mut out, &s, (w + 8) as i64, 0);
        let path = screenshots().join(format!("study_live_vs_study_{}.png", shot.name()));
        out.save(&path).expect("save");
        eprintln!("wrote {}", path.display());
    }
}

#[test]
#[ignore = "counting, prints only"]
fn study_calc() {
    let Some((device, queue)) = study_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let quads = device.features().contains(wgpu::Features::SUBGROUP);
    for shot in StudyShot::ALL {
        let sc = scene(shot);
        let view = view_of(&sc.camera, SIZE);
        eprintln!(
            "== {}: eye {:?}, pixel angle {:.3} mrad, {:.1} px per cm at 10 m",
            shot.name(),
            view.eye,
            view.pixel * 1000.0,
            0.01 / (10.0 * view.pixel)
        );
        let a = Variant::forward("A", "");
        let f = field(&sc, &a, &view, 1.0);
        let bands = band_stats(&f, &sc.terrain, sc.origin, view.eye, view.pixel, field_settings(&sc, &a, 1.0));
        eprintln!("   band (m)   blades  standing  triangles  vertices  width px  height px  <1px  <2px");
        for b in &bands {
            eprintln!(
                "   {:>4}-{:<4} {:>8} {:>8} {:>10} {:>9} {:>8.2} {:>9.1} {:>5.0}% {:>4.0}%",
                b.lo,
                b.hi.min(160.0),
                b.blades,
                b.standing,
                b.triangles,
                b.vertices,
                b.width_px,
                b.height_px,
                b.under_1px as f64 / b.standing.max(1) as f64 * 100.0,
                b.under_2px as f64 / b.standing.max(1) as f64 * 100.0
            );
        }
        eprintln!(
            "   total {:.2} M blades drawn, {:.1} M triangles per pass ({} chunks)",
            f.drawn.len() as f64 / 1e6,
            f.triangles() as f64 / 1e6,
            f.draws.len()
        );
        if !quads {
            eprintln!("   (no subgroup ops: no fragment counters)");
            continue;
        }
        let mut st = Study::new(&device, &queue, Arc::clone(&sc.terrain), sc.origin, sc.lighting, true);
        st.build_ground(&device, view.eye);
        let px = (SIZE.0 * SIZE.1) as f64;
        for v in picked().into_iter().filter(|v| matches!(v.name, "A" | "A1" | "B" | "B4" | "B2" | "C" | "D" | "E" | "H")) {
            prepare(&device, &queue, &mut st, &sc, &v, &view, LEVELS[0]);
            let gp = grass_params(&sc, &v, &view, 1.0, false);
            // A few frames first (pipelines, TAA), then one counted.
            for i in 0..4 {
                st.frame(&device, &queue, &v, &view, jitter(i), i, SIZE, &gp, 0.0, None);
            }
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            st.reset_counters(&queue);
            st.frame(&device, &queue, &v, &view, jitter(4), 4, SIZE, &gp, 0.0, None);
            let c = st.read_counters(&device, &queue);
            let m = |x: u32| x as f64 / 1e6;
            match v.path {
                Path::Forward { .. } => eprintln!(
                    "   {:<3} depth: {:.2} M frags in {:.2} M quads ({:.2} M lanes), {:.2} M kept | lit: {:.2} M frags in {:.2} M quads ({:.2} M lanes, {:.1} lanes per screen pixel), {:.2} M kept, {:.2} M samples covered",
                    v.name,
                    m(c[slot::DEPTH_LANES]),
                    m(c[slot::DEPTH_QUADS12]) / 12.0,
                    m(c[slot::DEPTH_QUADS12]) / 3.0,
                    m(c[slot::DEPTH_KEPT]),
                    m(c[slot::LIT_LANES]),
                    m(c[slot::LIT_QUADS12]) / 12.0,
                    m(c[slot::LIT_QUADS12]) / 3.0,
                    c[slot::LIT_QUADS12] as f64 / 3.0 / px,
                    m(c[slot::LIT_KEPT]),
                    m(c[slot::LIT_SAMPLES]),
                ),
                Path::Vis { .. } | Path::VisMs { .. } => eprintln!(
                    "   {:<3} vis raster: {:.2} M frags in {:.2} M quads ({:.2} M lanes), {:.2} M kept | shaded {:.2} M pixels ({:.1} % of the screen) | swr: {:.2} M blades, {:.2} M pixels",
                    v.name,
                    m(c[slot::VIS_LANES]),
                    m(c[slot::VIS_QUADS12]) / 12.0,
                    m(c[slot::VIS_QUADS12]) / 3.0,
                    m(c[slot::VIS_KEPT]),
                    m(c[slot::RESOLVED]),
                    c[slot::RESOLVED] as f64 / px * 100.0,
                    m(c[slot::SWR_BLADES]),
                    m(c[slot::SWR_PIXELS]),
                ),
            }
        }
    }
}

/// Walk (1.4 m/s) and turn (25°/s) at 60 FPS for 1.5 s from the wide shot; each variant
/// against A frame by frame, plus each one's frame-to-frame change. G: fast clouds.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn study_motion() {
    let Some((device, queue)) = study_device() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let sc = scene(StudyShot::Wide);
    let start = sc.camera.clone();
    let path = move |i: u32| {
        let t = i as f32 / 60.0;
        let mut c = start.clone();
        let fwd = c.forward();
        let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
        c.position += flat * 1.4 * t;
        c.rotation = Quat::from_rotation_y((25.0 * t).to_radians()) * c.rotation;
        c
    };
    let view = view_of(&sc.camera, SIZE);
    let mut st = Study::new(&device, &queue, Arc::clone(&sc.terrain), sc.origin, sc.lighting, false);
    st.build_ground(&device, view.eye);
    let frames = 90;
    let mut base: Option<Vec<Vec<u8>>> = None;
    let names = std::env::var("MISTY_STUDY").unwrap_or_else(|_| "A,A1,B,F,H,H2,G,E".into());
    for v in variants().into_iter().filter(|v| names.split(',').any(|n| n == v.name)) {
        // The field of the start position (the walk is short: the chunks' prefixes would
        // hardly change).
        prepare(&device, &queue, &mut st, &sc, &v, &view, LEVELS[0]);
        let clouds = |i: u32| if v.cache { i as f32 * 4.0 / 60.0 } else { 0.0 };
        let imgs = render_images(&device, &queue, &mut st, &sc, &v, &path, 1.0, frames, 60, &clouds);
        let a = base.get_or_insert_with(|| imgs.clone()).clone();
        let mut d = Vec::new();
        let mut step = Vec::new();
        for k in 0..imgs.len() {
            d.push(diff_stats(&imgs[k], &a[k]));
            if k > 0 {
                step.push(diff_stats(&imgs[k], &imgs[k - 1]).0);
            }
        }
        let mean = |f: &dyn Fn(&(f32, f32, f32)) -> f32| d.iter().map(f).sum::<f32>() / d.len() as f32;
        eprintln!(
            "motion {:<4} vs A: mean {:.2}, >8 {:.2}%, >24 {:.3}% | frame-to-frame change {:.2}",
            v.name,
            mean(&|x| x.0),
            mean(&|x| x.1),
            mean(&|x| x.2),
            step.iter().sum::<f32>() / step.len().max(1) as f32
        );
        if v.name != "A" {
            let k = imgs.len() - 1;
            write_strip(&format!("study_motion_{}.png", v.name), &imgs[k], &a[k]);
        }
    }
    // G with clouds moving 4x faster than in the other runs vs A with the same clouds.
    if names.contains("Gc") {
        let mut runs = Vec::new();
        for v in variants().into_iter().filter(|v| v.name == "A" || v.name == "G") {
            prepare(&device, &queue, &mut st, &sc, &v, &view, LEVELS[0]);
            let cam = sc.camera.clone();
            let imgs = render_images(&device, &queue, &mut st, &sc, &v, &move |_| cam.clone(), 1.0, 90, 30, &|i| i as f32 * 8.0 / 60.0);
            runs.push(imgs);
        }
        let d: Vec<f32> = (0..runs[0].len()).map(|k| diff_stats(&runs[1][k], &runs[0][k]).0).collect();
        eprintln!("G vs A with clouds moving (8x speed): mean {:.2}", d.iter().sum::<f32>() / d.len() as f32);
    }
}
