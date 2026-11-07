//! Grass visibility buffer (all ignored; run on purpose in release):
//! `cargo test --release -p misty_engine --lib grass_vis_ -- --ignored --nocapture --test-threads=1`

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::headless;
use crate::render::terrain_grass::gpu_tests::{Shot as GrassShot, meadow_view};
use crate::render::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
use crate::render_scene::GameView;

fn view(name: &str) -> Option<GameView> {
    Some(match name {
        "wide" => meadow_view(GrassShot::WideGolden),
        "close" => meadow_view(GrassShot::CloseGolden),
        "behind" => tree_view(TreeShot::Behind),
        "oak" => tree_view(TreeShot::Close),
        _ => return None,
    })
}

/// Grass chunks and blades held per terrain, per level and view.
#[test]
#[ignore = "prints only"]
fn grass_vis_resident() {
    let Some((device, queue)) = headless() else {
        return;
    };
    for shot in ["wide", "behind"] {
        let v = view(shot).expect("view");
        for (name, q) in [
            ("high", QualityLevel::high()),
            ("medium", QualityLevel::medium()),
            ("low", QualityLevel::low()),
        ] {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &q);
            let target = r.create_target(&device, 1920, 1080);
            let frame = v.frame();
            for _ in 0..3 {
                r.render(&device, &queue, &target, &frame);
            }
            for (chunks, blades) in r.terrains.grass_resident() {
                eprintln!(
                    "{shot} {name}: {chunks} chunks, {:.3} M blades ({:.0} MB)",
                    blades as f64 / 1e6,
                    blades as f64 * 72.0 / 1e6
                );
            }
        }
    }
}

fn levels() -> Vec<(String, QualityLevel)> {
    let names = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high,medium,low".into());
    names
        .split(',')
        .filter_map(|n| {
            let mut q = match n {
                "high" => QualityLevel::high(),
                "medium" => QualityLevel::medium(),
                "low" => QualityLevel::low(),
                _ => return None,
            };
            if std::env::var("MISTY_MSAA2").is_ok() {
                q.anti_aliasing = crate::quality::AntiAliasing::X2;
            }
            Some((n.to_owned(), q))
        })
        .collect()
}

/// The look checks of `meadow_perf_shots` (1600×900, 48 frames, clocks fixed) with the
/// visibility buffer on (default) or off (`MISTY_VIS=0`), into `MISTY_GFX_DIR`.
#[test]
#[ignore = "writes look-check renders; run on purpose"]
fn grass_vis_shots() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var("MISTY_GFX_DIR").expect("MISTY_GFX_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let on = std::env::var("MISTY_VIS").map_or(true, |v| v != "0");
    crate::render::set_grass_vis(on);
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide,close,behind,oak".into());
    let (w, h) = (1600, 900);
    for (level, quality) in levels() {
        for shot in shots.split(',') {
            let Some(view) = view(shot) else {
                continue;
            };
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            let target = r.create_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..48 {
                r.render(&device, &queue, &target, &frame);
            }
            let img = image::RgbaImage::from_raw(
                w,
                h,
                crate::render::gpu_tests::read_back(&device, &queue, &target),
            )
            .expect("read back");
            let path = dir.join(format!("{level}_{shot}.png"));
            img.save(&path).unwrap();
            eprintln!(
                "wrote {} (visibility images {:.0} MB)",
                path.display(),
                target.grass_vis.bytes() as f64 / 1e6
            );
        }
    }
    crate::render::set_grass_vis(true);
}

/// Before / after timing in one process: per view and level, the lit grass pass ("lit") and
/// the visibility buffer ("vis"), each at the level's Anti-Aliasing and at 2×, interleaved
/// over `MISTY_AB_ROUNDS` (3) rounds, the quietest round of each kept (other GPU work only
/// ever adds time). `MISTY_SIZE=WxH` (1920×1080), `MISTY_SCALED=1` (the level's Render
/// Scale), `MISTY_GFX_SHOT=wide,behind`, `MISTY_SETUPS=lit,vis,lit2,vis2`.
#[test]
#[ignore = "timing, prints only"]
fn grass_vis_matrix() {
    use crate::render::terrain_grass::gpu_tests::{Breakdown, breakdown};
    let Some((device, queue)) = headless() else {
        return;
    };
    let rounds: usize = std::env::var("MISTY_AB_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    let (w, h) = std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1920, 1080));
    let scaled = std::env::var("MISTY_SCALED").is_ok();
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    let setups = std::env::var("MISTY_SETUPS").unwrap_or_else(|_| "lit,vis,lit2,vis2".into());
    let setups: Vec<&str> = setups.split(',').collect();
    // A first run warms the GPU clocks up.
    if let Some(v) = view("wide") {
        breakdown(&device, &queue, &v, &QualityLevel::low(), (w, h));
    }
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let s = if scaled { quality.render_scale } else { 1.0 };
            let size = ((w as f32 * s) as u32, (h as f32 * s) as u32);
            let setup = |name: &str| {
                let mut q = quality.clone();
                crate::render::set_grass_vis(name.starts_with("vis"));
                if name.ends_with('2') {
                    q.anti_aliasing = crate::quality::AntiAliasing::X2;
                }
                q
            };
            let mut best: Vec<Option<Breakdown>> = setups.iter().map(|_| None).collect();
            for _ in 0..rounds {
                for (i, name) in setups.iter().enumerate() {
                    let b = breakdown(&device, &queue, &v, &setup(name), size);
                    if best[i].as_ref().is_none_or(|x| b.gpu < x.gpu) {
                        best[i] = Some(b);
                    }
                }
            }
            crate::render::set_grass_vis(true);
            eprintln!("== {shot} {level} {}x{}", size.0, size.1);
            for (name, b) in setups.iter().zip(&best) {
                let b = b.as_ref().expect("measured");
                b.print(&format!("{shot} {level} {name}: {:.0} FPS", 1000.0 / b.gpu));
            }
        }
    }
}
