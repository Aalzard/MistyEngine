//! Golden shots: the key look shots rendered and compared with the references in
//! `docs/golden/` (ignored; run on purpose, in release, before flipping a default):
//!
//! `cargo test --release -p misty_engine --lib golden_shots -- --ignored --nocapture --test-threads=1`
//!
//! - Shots: MeadowDemo wide, close, behind and oak (the `meadow_perf_shots` views) at every
//!   quality level (low, medium, high, ultra), and CyclesRoomDemo as saved at every level.
//!   Rendered at 1600×900 like the look checks (48 frames, wind clock fixed), then box-filtered
//!   to 320×180 (5 × 5 pixels each), which is what is stored and compared.
//! - A shot drifts when its mean channel difference is over [`MEAN_LIMIT`] or more than
//!   [`CHANGED_LIMIT`] of its pixels differ by over [`PIXEL_DIFF`] in some channel. Every
//!   drifting shot is listed and the test fails; the render, the reference and an amplified
//!   difference image go into `target/golden/` to look at.
//! - `MISTY_GOLDEN=update` writes the references instead (after a look change was checked and
//!   meant). `MISTY_GOLDEN_SHOTS=meadow_high,cycles` renders only the shots whose names
//!   contain one of the words.
//! - The references are for the GPU and driver named in `docs/golden/adapter.txt`; another
//!   adapter prints a warning (its rounding differs; the tolerance may not cover it).

use std::path::PathBuf;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::{headless, instance, read_back};
use crate::render::terrain_grass::gpu_tests::{Shot as GrassShot, meadow_view};
use crate::render::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
use crate::render_scene::GameView;

/// Render size (the look checks' size).
const RENDER: (u32, u32) = (1600, 900);
/// Each stored pixel is the average of this many render pixels across and down.
const SCALE: u32 = 5;
/// Frames rendered before the read-back (temporal effects settle).
const FRAMES: usize = 48;
/// Largest mean absolute channel difference (0..255) of a shot that has not drifted. Repeat
/// runs on the reference machine are identical (mean 0.00); neighbouring quality levels of
/// MeadowDemo differ by 2–5. The margin is for drivers that round differently.
pub(crate) const MEAN_LIMIT: f32 = 0.5;
/// A pixel has changed when one of its channels differs by more than this (0..255).
pub(crate) const PIXEL_DIFF: u8 = 16;
/// Largest share of changed pixels of a shot that has not drifted (a small object appearing
/// or vanishing is more).
pub(crate) const CHANGED_LIMIT: f32 = 0.0025;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/golden")
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/golden")
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// CyclesRoomDemo as saved.
fn cycles_view() -> GameView {
    crate::assets::set_project_root(&root());
    let path = root().join("Assets/Scenes/CyclesRoomDemo.scene");
    let (scene, _) = crate::scene::Scene::load(&path).expect("CyclesRoomDemo");
    let materials = crate::assets::AssetCache::open(&root());
    GameView::new(&scene, &materials).expect("camera")
}

fn levels() -> [(&'static str, QualityLevel); 4] {
    [
        ("low", QualityLevel::low()),
        ("medium", QualityLevel::medium()),
        ("high", QualityLevel::high()),
        ("ultra", QualityLevel::ultra()),
    ]
}

/// Builds the view of a shot.
type ViewFn = fn() -> GameView;

/// Every golden shot: (name, view, quality level).
fn shots() -> Vec<(String, ViewFn, QualityLevel)> {
    let views: [(&str, ViewFn); 5] = [
        ("meadow_{level}_wide", || meadow_view(GrassShot::WideGolden)),
        ("meadow_{level}_close", || {
            meadow_view(GrassShot::CloseGolden)
        }),
        ("meadow_{level}_behind", || tree_view(TreeShot::Behind)),
        ("meadow_{level}_oak", || tree_view(TreeShot::Close)),
        ("cycles_{level}", cycles_view),
    ];
    let mut out = Vec::new();
    for (pattern, view) in views {
        for (level, quality) in levels() {
            out.push((pattern.replace("{level}", level), view, quality));
        }
    }
    out
}

/// `rgba` (`w` × `h`) averaged over `SCALE` × `SCALE` blocks.
pub(crate) fn downscale(rgba: &[u8], w: u32, h: u32) -> image::RgbaImage {
    let (sw, sh) = (w / SCALE, h / SCALE);
    image::RgbaImage::from_fn(sw, sh, |x, y| {
        let mut sum = [0u32; 4];
        for dy in 0..SCALE {
            for dx in 0..SCALE {
                let i = (((y * SCALE + dy) * w + x * SCALE + dx) * 4) as usize;
                for (c, s) in sum.iter_mut().enumerate() {
                    *s += u32::from(rgba[i + c]);
                }
            }
        }
        let n = SCALE * SCALE;
        image::Rgba(sum.map(|s| ((s + n / 2) / n) as u8))
    })
}

/// How far `a` is from `b` (same size): mean absolute RGB difference and share of pixels with
/// a channel more than [`PIXEL_DIFF`] apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Drift {
    pub mean: f32,
    pub changed: f32,
}

impl Drift {
    pub fn within_tolerance(&self) -> bool {
        self.mean <= MEAN_LIMIT && self.changed <= CHANGED_LIMIT
    }
}

pub(crate) fn drift(a: &image::RgbaImage, b: &image::RgbaImage) -> Drift {
    assert_eq!(a.dimensions(), b.dimensions());
    let (mut total, mut changed) = (0u64, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = [0, 1, 2].map(|c| p[c].abs_diff(q[c]));
        total += d.iter().map(|&v| u64::from(v)).sum::<u64>();
        changed += usize::from(d.iter().any(|&v| v > PIXEL_DIFF));
    }
    let n = (a.width() * a.height()) as f32;
    Drift {
        mean: total as f32 / (n * 3.0),
        changed: changed as f32 / n,
    }
}

/// The difference of `a` and `b`, amplified 4× (black = same).
fn difference(a: &image::RgbaImage, b: &image::RgbaImage) -> image::RgbaImage {
    image::RgbaImage::from_fn(a.width(), a.height(), |x, y| {
        let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
        let d = [0, 1, 2].map(|c| p[c].abs_diff(q[c]).saturating_mul(4));
        image::Rgba([d[0], d[1], d[2], 255])
    })
}

/// The adapter `headless` picks (a device does not name its adapter).
fn adapter_line() -> String {
    let options = wgpu::RequestAdapterOptions::default();
    pollster::block_on(instance().request_adapter(&options))
        .map(|a| {
            let i = a.get_info();
            format!(
                "{} ({:?}, driver {} {})",
                i.name, i.backend, i.driver, i.driver_info
            )
        })
        .unwrap_or_default()
}

#[test]
#[ignore = "golden shots: renders and compares against docs/golden; run on purpose in release"]
fn golden_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let update = std::env::var("MISTY_GOLDEN").is_ok_and(|v| v == "update");
    let filter: Vec<String> = std::env::var("MISTY_GOLDEN_SHOTS")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let (golden, out) = (golden_dir(), out_dir());
    std::fs::create_dir_all(&golden).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let adapter = adapter_line();
    let adapter_file = golden.join("adapter.txt");
    if update {
        std::fs::write(&adapter_file, format!("{adapter}\n")).unwrap();
    } else if let Ok(saved) = std::fs::read_to_string(&adapter_file)
        && saved.trim() != adapter
    {
        eprintln!(
            "warning: references made on {}, this is {adapter}",
            saved.trim()
        );
    }
    let (w, h) = RENDER;
    let mut drifted = Vec::new();
    let mut missing = Vec::new();
    let mut compared = 0;
    for (name, view, quality) in shots() {
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let view = view();
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality);
        let target = r.create_target(&device, w, h);
        let frame = view.frame();
        for _ in 0..FRAMES {
            r.render(&device, &queue, &target, &frame);
        }
        let shot = downscale(&read_back(&device, &queue, &target), w, h);
        let reference = golden.join(format!("{name}.png"));
        if update {
            shot.save(&reference).unwrap();
            eprintln!("wrote {}", reference.display());
            continue;
        }
        let Ok(expected) = image::open(&reference).map(|i| i.to_rgba8()) else {
            missing.push(name);
            continue;
        };
        compared += 1;
        let d = drift(&shot, &expected);
        let verdict = if d.within_tolerance() { "ok" } else { "DRIFT" };
        eprintln!(
            "{name:24} mean {:.2} changed {:.2} % {verdict}",
            d.mean,
            d.changed * 100.0
        );
        if !d.within_tolerance() {
            shot.save(out.join(format!("{name}_now.png"))).unwrap();
            expected.save(out.join(format!("{name}_ref.png"))).unwrap();
            difference(&shot, &expected)
                .save(out.join(format!("{name}_diff.png")))
                .unwrap();
            drifted.push(format!(
                "{name}: mean {:.2} (limit {MEAN_LIMIT}), changed {:.2} % (limit {:.1} %)",
                d.mean,
                d.changed * 100.0,
                CHANGED_LIMIT * 100.0
            ));
        }
    }
    if update {
        return;
    }
    assert!(
        missing.is_empty(),
        "no reference for {missing:?}: make them with MISTY_GOLDEN=update"
    );
    assert!(compared > 0, "no shot matched MISTY_GOLDEN_SHOTS");
    assert!(
        drifted.is_empty(),
        "{} of {compared} shots drifted from docs/golden (images in {}):\n{}",
        drifted.len(),
        out.display(),
        drifted.join("\n")
    );
}

/// The comparison itself (no GPU): a copy matches, small noise passes, a changed region or a
/// colour shift fails.
#[test]
fn golden_drift_tolerates_noise_and_catches_changes() {
    let base = image::RgbaImage::from_fn(320, 180, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
    });
    assert_eq!(
        drift(&base, &base),
        Drift {
            mean: 0.0,
            changed: 0.0
        }
    );
    // ±1 on a quarter of the pixels (GPU rounding, a different atomics order): within.
    let mut noisy = base.clone();
    for (i, p) in noisy.pixels_mut().enumerate().filter(|(i, _)| i % 4 == 0) {
        let d = if i % 8 == 0 { 1 } else { -1i16 };
        for c in 0..3 {
            p[c] = (i16::from(p[c]) + d).clamp(0, 255) as u8;
        }
    }
    assert!(drift(&noisy, &base).within_tolerance());
    // A 40 × 40 block changed (an object missing, a broken pass): over the share of changed
    // pixels.
    let mut block = base.clone();
    for y in 0..40 {
        for x in 0..40 {
            block.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        }
    }
    assert!(!drift(&block, &base).within_tolerance());
    // Everything 4 levels brighter (an exposure or tone-mapping default flipped): over the mean.
    let mut brighter = base.clone();
    for p in brighter.pixels_mut() {
        for c in 0..3 {
            p[c] = p[c].saturating_add(4);
        }
    }
    assert!(!drift(&brighter, &base).within_tolerance());
    // The downscale averages blocks.
    let full = [10u8, 20, 30, 255].repeat((SCALE * SCALE) as usize);
    let small = downscale(&full, SCALE, SCALE);
    assert_eq!(small.get_pixel(0, 0).0, [10, 20, 30, 255]);
}
