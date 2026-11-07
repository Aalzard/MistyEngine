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
            // Quality ▸ Upscaling (the scene renders at the internal size).
            match std::env::var("MISTY_UPSCALING").as_deref() {
                Ok("performance") => q.upscaling = crate::quality::Upscaling::Performance,
                Ok("quality") => q.upscaling = crate::quality::Upscaling::Quality,
                Ok("off") => q.upscaling = crate::quality::Upscaling::Off,
                _ => {}
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
            // (At the level's Upscaling share, as the editor and player render.)
            let target = r.create_view_target(&device, w, h);
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

/// Flying over MeadowDemo like the Scene view's fly camera (accelerated: `MISTY_FLY_SPEED`
/// m/s, 20 by default, turning 30°/s, 60 FPS steps) with grass streaming on: per frame the
/// CPU time of `Renderer::render`, the grass upload time and its longest piece, and the
/// profiler scopes' self time averaged over the flight (`MISTY_SIZE`, 1125×558 = the docked
/// view, `MISTY_QUALITY`, `MISTY_FRAMES` 600).
#[test]
#[ignore = "timing, prints only"]
fn grass_vis_flight() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let (w, h) = std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1125, 558));
    let speed: f32 = std::env::var("MISTY_FLY_SPEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20.0);
    let frames: usize = std::env::var("MISTY_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let view = view("wide").expect("view");
    for (level, quality) in levels() {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality);
        let target = r.create_view_target(&device, w, h);
        // Settle the start view first (as the editor has it before flying).
        for _ in 0..3 {
            r.render(&device, &queue, &target, &view.frame());
        }
        crate::render::set_grass_streaming(true);
        crate::profiler::set_enabled(true);
        crate::profiler::clear();
        let mut cam = view.camera;
        let dt = 1.0 / 60.0;
        let mut cpu = Vec::new();
        let mut upload = Vec::new();
        let mut piece: f32 = 0.0;
        let mut over = 0;
        let first = crate::profiler::frame_index();
        for _ in 0..frames {
            cam.rotation = glam::Quat::from_rotation_y((30.0f32 * dt).to_radians()) * cam.rotation;
            let mut f = cam.forward();
            f.y = 0.0;
            cam.position += f.normalize_or_zero() * speed * dt;
            let mut frame = view.frame();
            frame.camera = &cam;
            r.render(&device, &queue, &target, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            crate::profiler::end_frame();
            let s = r.stats();
            cpu.push(s.cpu_ms);
            upload.push(s.grass_upload_ms);
            piece = piece.max(s.grass_upload_piece_ms);
            over += usize::from(s.grass_upload_ms > 2.5);
        }
        let mut scopes: Vec<(String, f32)> = Vec::new();
        let mut counted = 0;
        crate::profiler::with_frames(|all| {
            for f in all.iter().filter(|f| f.index >= first) {
                counted += 1;
                let tree = crate::profiler::HierarchyTree::build(f);
                for n in tree.nodes.iter().skip(1) {
                    match scopes.iter_mut().find(|(name, _)| *name == n.name) {
                        Some(s) => s.1 += n.self_ms,
                        None => scopes.push((n.name.clone(), n.self_ms)),
                    }
                }
            }
        });
        crate::profiler::set_enabled(false);
        crate::render::set_grass_streaming(false);
        let stat = |mut x: Vec<f32>| {
            x.sort_by(f32::total_cmp);
            let at = |q: f32| x[((x.len() - 1) as f32 * q) as usize];
            (at(0.5), at(0.9), at(0.99), x[x.len() - 1])
        };
        let (c50, c90, c99, cmax) = stat(cpu.clone());
        let (u50, u90, u99, umax) = stat(upload.clone());
        eprintln!(
            "== flight {level} {w}x{h} at {speed} m/s, {frames} frames: CPU render median {c50:.2} / p90 {c90:.2} / p99 {c99:.2} / max {cmax:.2} ms; grass upload median {u50:.2} / p90 {u90:.2} / p99 {u99:.2} / max {umax:.2} ms, longest piece {piece:.2} ms, frames over 2.5 ms: {over}"
        );
        scopes.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (name, ms) in scopes.iter().take(18) {
            eprintln!("   {name:<36} {:6.3} ms/frame", ms / counted.max(1) as f32);
        }
    }
}

/// Shimmer and motion, the lit grass pass against the visibility buffer, both rendered in
/// lockstep (1600×900, per level): `still` = camera still, wind blowing, per-pixel temporal
/// standard deviation of luma over the last 16 of 64 frames (lower half of the frame: the
/// grass); `walk` = 1.4 m/s forward while turning 25°/s for 90 frames with the wind, the mean
/// frame-to-frame luma change (lower half) of each and their mean difference per frame.
/// `MISTY_GFX_DIR` also gets the last walk frame of both and their difference × 8.
#[test]
#[ignore = "renders and prints; run on purpose"]
fn grass_vis_motion() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let (w, h) = (1600u32, 900u32);
    let dir = std::env::var("MISTY_GFX_DIR")
        .ok()
        .map(std::path::PathBuf::from);
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    let luma = |px: &[u8]| -> Vec<f32> {
        px.chunks(4)
            .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
            .collect()
    };
    for shot in shots.split(',') {
        let Some(mut view) = view(shot) else {
            continue;
        };
        let base_cam = view.camera;
        for (level, quality) in levels() {
            let mut make = |on: bool| {
                crate::render::set_grass_vis(on);
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(&device, &quality);
                let t = r.create_view_target(&device, w, h);
                (r, t, on)
            };
            let mut runs = [make(false), make(true)];
            let lower = (h / 2 * w) as usize..(h * w) as usize;
            for mode in ["still", "walk"] {
                let frames = if mode == "still" { 64 } else { 90 };
                let mut lumas: [Vec<Vec<f32>>; 2] = Default::default();
                let mut last: [Vec<u8>; 2] = Default::default();
                for i in 0..frames {
                    let t = i as f32 / 60.0;
                    view.environment.grass_time = Some(3.0 + t);
                    let mut cam = base_cam;
                    if mode == "walk" {
                        cam.rotation = glam::Quat::from_rotation_y((25.0 * t).to_radians())
                            * base_cam.rotation;
                        let mut f = cam.forward();
                        f.y = 0.0;
                        cam.position += f.normalize_or_zero() * 1.4 * t;
                    }
                    for (k, (r, target, on)) in runs.iter_mut().enumerate() {
                        crate::render::set_grass_vis(*on);
                        let mut frame = view.frame();
                        frame.camera = &cam;
                        r.render(&device, &queue, target, &frame);
                        let px = crate::render::gpu_tests::read_back(&device, &queue, target);
                        lumas[k].push(luma(&px)[lower.clone()].to_vec());
                        last[k] = px;
                    }
                }
                crate::render::set_grass_vis(true);
                let n = lower.len();
                if mode == "still" {
                    let std = |l: &[Vec<f32>]| {
                        let tail = &l[l.len() - 16..];
                        let mut sum = 0.0f64;
                        for p in 0..n {
                            let m = tail.iter().map(|f| f[p] as f64).sum::<f64>() / 16.0;
                            let v =
                                tail.iter().map(|f| (f[p] as f64 - m).powi(2)).sum::<f64>() / 16.0;
                            sum += v.sqrt();
                        }
                        sum / n as f64
                    };
                    eprintln!(
                        "{shot} {level} still (wind): temporal std of luma, lit {:.3} | vis {:.3} levels",
                        std(&lumas[0]),
                        std(&lumas[1])
                    );
                } else {
                    let change = |l: &[Vec<f32>]| {
                        let mut sum = 0.0f64;
                        for f in 1..l.len() {
                            sum += (0..n)
                                .map(|p| (l[f][p] - l[f - 1][p]).abs() as f64)
                                .sum::<f64>()
                                / n as f64;
                        }
                        sum / (l.len() - 1) as f64
                    };
                    let mut diff = 0.0f64;
                    let mut worst = 0.0f64;
                    for f in 0..lumas[0].len() {
                        let d = (0..n)
                            .map(|p| (lumas[0][f][p] - lumas[1][f][p]).abs() as f64)
                            .sum::<f64>()
                            / n as f64;
                        diff += d;
                        worst = worst.max(d);
                    }
                    eprintln!(
                        "{shot} {level} walk: frame-to-frame change lit {:.3} | vis {:.3}; lit vs vis per frame mean {:.3}, worst frame {:.3} levels",
                        change(&lumas[0]),
                        change(&lumas[1]),
                        diff / lumas[0].len() as f64,
                        worst
                    );
                    if let Some(dir) = &dir {
                        std::fs::create_dir_all(dir).unwrap();
                        let (ow, oh) = runs[0].1.output_size();
                        let mut out = image::RgbaImage::new(ow * 3, oh);
                        for y in 0..oh {
                            for x in 0..ow {
                                let i = ((y * ow + x) * 4) as usize;
                                let a = &last[0][i..i + 4];
                                let b = &last[1][i..i + 4];
                                out.put_pixel(x, y, image::Rgba([a[0], a[1], a[2], 255]));
                                out.put_pixel(x + ow, y, image::Rgba([b[0], b[1], b[2], 255]));
                                let d = |c: usize| {
                                    ((a[c] as i32 - b[c] as i32).unsigned_abs() * 8).min(255) as u8
                                };
                                out.put_pixel(x + 2 * ow, y, image::Rgba([d(0), d(1), d(2), 255]));
                            }
                        }
                        let path = dir.join(format!("grassvis_motion_{level}_{shot}.png"));
                        out.save(&path).unwrap();
                        eprintln!("wrote {}", path.display());
                    }
                }
            }
        }
    }
}
