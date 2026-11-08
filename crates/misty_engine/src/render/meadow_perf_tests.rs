//! MeadowDemo performance round 2 (all ignored; run on purpose in release):
//! `cargo test --release -p misty_engine --lib meadow_perf_ -- --ignored --nocapture --test-threads=1`
//!
//! - `meadow_perf_shots`: the look checks. Every quality level (`MISTY_QUALITY=high,medium,low`)
//!   from four views (wide = the player's start view, close = crouching into the grass,
//!   behind = turned round to the trees behind the player, oak = towards the broad oak),
//!   1600×900, 48 frames, wind / cloud / fog clocks fixed, written as `<level>_<shot>.png`
//!   into `MISTY_GFX_DIR`. `MISTY_PERF_OPTS=cover` turns on look options (see [`apply_opts`]).
//! - `meadow_perf_times`: median GPU frame time, CPU time of `Renderer::render` and the GPU
//!   time of every part per level (`MISTY_GFX_SHOT=wide,behind`, `MISTY_SIZE=WxH`).

use std::path::PathBuf;

use crate::quality::QualityLevel;
use crate::render::Renderer;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::terrain_grass::gpu_tests::{Shot as GrassShot, breakdown, meadow_view};
use crate::render::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
use crate::render_scene::GameView;

fn levels() -> Vec<(String, QualityLevel)> {
    fast_off();
    let names = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high,medium,low".into());
    names
        .split(',')
        .filter_map(|n| {
            let q = match n {
                "high" => QualityLevel::high(),
                "medium" => QualityLevel::medium(),
                "low" => QualityLevel::low(),
                "ultra" => QualityLevel::ultra(),
                _ => return None,
            };
            Some((n.to_owned(), apply_opts(q)))
        })
        .collect()
}

/// `MISTY_FAST_OFF=a,b`: turn these shortcuts of round 3 off (render/fast_paths.rs).
fn fast_off() {
    let names = std::env::var("MISTY_FAST_OFF").unwrap_or_default();
    for n in names.split(',').filter(|n| !n.is_empty()) {
        let all = n == "all";
        for f in crate::render::fast_paths::NAMES {
            if all || f == n {
                crate::render::fast_paths::set(f, false);
            }
        }
    }
}

/// The look options named in `MISTY_PERF_OPTS` (comma separated), on top of the level's own
/// settings.
fn apply_opts(mut q: QualityLevel) -> QualityLevel {
    let opts = std::env::var("MISTY_PERF_OPTS").unwrap_or_default();
    for o in opts.split(',').filter(|o| !o.is_empty()) {
        apply_opt(&mut q, o);
    }
    q
}

/// One look option (round 2: `cover`, `msaa2`; round 3: the proposed defaults).
fn apply_opt(q: &mut QualityLevel, o: &str) {
    match o {
        // Quality ▸ Grass Ground Cover.
        "cover" => q.grass_ground_cover = true,
        // Quality ▸ Anti-Aliasing 2×.
        "msaa2" => q.anti_aliasing = crate::quality::AntiAliasing::X2,
        // Quality ▸ Shadows ▸ Virtual.
        "vsm" => q.shadow_mode = crate::quality::ShadowMode::Virtual,
        // Quality ▸ Global Illumination Off (Misty GI's whole cost).
        "gioff" => q.global_illumination = crate::quality::Tier::Off,
        // Quality ▸ Upscaling ▸ Quality.
        "upq" => q.upscaling = crate::quality::Upscaling::Quality,
        // Quality ▸ Virtual Geometry on (MeadowDemo's rocks draw as clusters).
        "vgeo" => q.virtual_geometry = true,
        "default" | "no culling" => {}
        _ => eprintln!("(option {o} unknown: ignored)"),
    }
}

fn view(name: &str) -> Option<GameView> {
    Some(match name {
        "wide" => meadow_view(GrassShot::WideGolden),
        "close" => meadow_view(GrassShot::CloseGolden),
        "behind" => tree_view(TreeShot::Behind),
        "oak" => tree_view(TreeShot::Close),
        // Walked 12 m / 20 m towards the oak (under its crown: the virtual shadow map's near
        // layer takes it).
        "oaknear" | "oakunder" => {
            let mut v = tree_view(TreeShot::Close);
            let f = v.camera.forward();
            let d = if name == "oaknear" { 12.0 } else { 20.0 };
            v.camera.position += glam::Vec3::new(f.x, 0.0, f.z).normalize_or_zero() * d;
            v
        }
        _ => return None,
    })
}

#[test]
#[ignore = "writes look-check renders; run on purpose"]
fn meadow_perf_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_GFX_DIR").expect("MISTY_GFX_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide,close,behind,oak".into());
    let (w, h) = (1600, 900);
    // `MISTY_SHOT_FRAMES`: frames rendered before the read-back (48, as the golden shots).
    let frames: usize = std::env::var("MISTY_SHOT_FRAMES")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(48);
    for (level, quality) in levels() {
        for shot in shots.split(',') {
            let Some(view) = view(shot) else {
                continue;
            };
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            // A view target: the level's Upscaling applies (native when it is Off).
            let target = r.create_view_target(&device, w, h);
            let frame = view.frame();
            for _ in 0..frames {
                r.render(&device, &queue, &target, &frame);
            }
            let img = image::RgbaImage::from_raw(w, h, read_back(&device, &queue, &target))
                .expect("read back");
            let path = dir.join(format!("{level}_{shot}.png"));
            img.save(&path).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }
}

#[test]
#[ignore = "timing, prints only"]
fn meadow_perf_times() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = std::env::var("MISTY_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1920, 1080));
    let scaled = std::env::var("MISTY_SCALED").is_ok();
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
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
            let b = breakdown(&device, &queue, &v, &quality, size);
            b.print(&format!(
                "{shot} {level} {}x{} ({:.0} FPS)",
                size.0,
                size.1,
                1000.0 / b.gpu
            ));
        }
    }
}

/// Where the CPU side of `Renderer::render` goes (round 3): per level and view
/// (`MISTY_GFX_SHOT=wide,behind`), 60 frames to settle, then 120 with the profiler on; the
/// median render CPU time and the mean self time of the costliest profiler scopes. The GPU is
/// waited for after every frame, as in `meadow_perf_times`.
#[test]
#[ignore = "timing, prints only"]
fn meadow_perf_cpu() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide,behind".into());
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            let target = r.create_view_target(&device, 1920, 1080);
            let frame = v.frame();
            for _ in 0..60 {
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
            let profiler = crate::profiler::test_serial();
            crate::profiler::set_enabled(true);
            crate::profiler::clear();
            let first = crate::profiler::frame_index();
            let mut cpu = Vec::new();
            for _ in 0..120 {
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                crate::profiler::end_frame();
                cpu.push(r.stats().cpu_ms);
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
            drop(profiler);
            cpu.sort_by(f32::total_cmp);
            eprintln!(
                "== {shot} {level}: CPU render median {:.2} ms, p90 {:.2} ms",
                cpu[cpu.len() / 2],
                cpu[cpu.len() * 9 / 10]
            );
            scopes.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (name, ms) in scopes.iter().take(14) {
                eprintln!("   {name:<36} {:6.3} ms/frame", ms / counted.max(1) as f32);
            }
        }
    }
}

/// Instances the grass passes draw today against what they need (CPU replay of the chunk
/// prefixes and the blade tests of grass_blade.wgsl), MeadowDemo's wide view.
#[test]
#[ignore = "prints only"]
fn meadow_perf_counts() {
    use crate::render::culling::{Aabb, Frustum};
    use crate::terrain::details;
    use glam::{Vec2, Vec3, Vec4};
    use rayon::prelude::*;
    let shot = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    let v = view(&shot).expect("view");
    let t = &v.terrains[0];
    let d = &t.data;
    let s = &d.details;
    let cam = v.camera.position - t.position;
    let vp = v.camera.view_projection(16.0 / 9.0);
    let frustum = Frustum::from_matrix(&vp);
    let m = vp;
    let planes: Vec<Vec4> = [
        m.row(3) + m.row(0),
        m.row(3) - m.row(0),
        m.row(3) + m.row(1),
        m.row(3) - m.row(1),
        m.row(2),
    ]
    .to_vec();
    let range = s.draw_distance.max(1.0);
    let lod = s.lod_distance.max(1.0);
    let chunk = 16.0f32;
    let per_x = (d.size.x / chunk).ceil() as i64;
    let per_z = (d.size.z / chunk).ceil() as i64;
    let (cx, cz) = ((cam.x / chunk) as i64, (cam.z / chunk) as i64);
    let reach = (range / chunk).ceil() as i64 + 1;
    let pixel = 60f32.to_radians() / 1080.0;
    eprintln!("lod {lod} range {range} density {}", s.density);
    for (level_name, density) in [("high", 1.0f32), ("medium", 0.75), ("low", 0.5)] {
        let q = match level_name {
            "high" => QualityLevel::high(),
            "medium" => QualityLevel::medium(),
            _ => QualityLevel::low(),
        };
        let density = q
            .grass_density()
            .clamp(0.012, 1.0)
            .min(density.max(q.grass_density()));
        let keep_at = |dist: f32| ((lod / dist.max(1e-3)).powi(2)).clamp(0.012, 1.0);
        let mut keys = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                let c = (Vec2::new(x as f32, z as f32) + 0.5) * chunk;
                if c.distance(Vec2::new(cam.x, cam.z)) < range + chunk {
                    keys.push((x, z));
                }
            }
        }
        // [drawn, outside, flat, standing in view] for lit / depth, and the 4 m cell drawing.
        let totals: Vec<[u64; 10]> = keys
            .par_iter()
            .map(|&(x, z)| {
                let lo = Vec2::new(x as f32, z as f32) * chunk;
                let p = Vec2::new(cam.x, cam.z).clamp(lo, lo + chunk);
                let dist = Vec3::new(p.x - cam.x, 0.0, p.y - cam.z).length();
                let mut l = 0;
                while l < details::MAX_LEVEL
                    && details::level_keep(l + 1) >= keep_at(dist) * density
                {
                    l += 1;
                }
                let blades = details::scatter_level(d, lo, lo + chunk, l);
                let mut out = [0u64; 10];
                out[7] = blades.len() as u64;
                if dist < 64.0 {
                    out[8] = blades.len() as u64;
                }
                if blades.is_empty() {
                    return out;
                }
                let top = blades
                    .iter()
                    .map(|b| b.position[1] + b.position[3])
                    .fold(f32::MIN, f32::max);
                let bottom = blades
                    .iter()
                    .map(|b| b.position[1])
                    .fold(f32::MAX, f32::min);
                let world = Aabb::new(
                    Vec3::new(lo.x - 1.0, bottom - 1.0, lo.y - 1.0) + t.position,
                    Vec3::new(lo.x + chunk + 1.0, top + 1.0, lo.y + chunk + 1.0) + t.position,
                );
                let wc = v.camera.position;
                let cdist = wc.clamp(world.min, world.max).distance(wc);
                if !frustum.intersects(&world) {
                    return out;
                }
                let keep = keep_at(cdist) * density / details::level_keep(l);
                let count =
                    ((blades.len() as f32 * keep.min(1.0)).ceil() as usize).min(blades.len());
                // Cells: 4 m, per-cell prefix at the cell's nearest point (depth) and in frustum.
                let mut cell_drawn = [0u64; 16];
                for (i, b) in blades.iter().enumerate() {
                    let root = Vec3::new(b.position[0], b.position[1], b.position[2]);
                    let wroot = root + t.position;
                    let bd = wroot.distance(wc);
                    let key = b.extra[0] - if b.extra[0] >= 2.0 { 2.0 } else { 0.0 };
                    let fade = 1.0 - smooth(range * 0.8, range, bd);
                    let bkeep = keep_at(bd) * density;
                    let flat = fade <= 0.0 || key >= bkeep;
                    let r = b.position[3] * 1.6 + 0.25 + bd * pixel * 2.0;
                    let w4 = wroot.extend(1.0);
                    let outside = planes
                        .iter()
                        .any(|pl| pl.dot(w4) < -r * pl.truncate().length());
                    let cell = ((((root.x - lo.x) / 4.0) as usize).min(3))
                        + 4 * (((root.z - lo.y) / 4.0) as usize).min(3);
                    if i < count {
                        out[0] += 1;
                        if outside {
                            out[1] += 1;
                        } else if flat {
                            out[2] += 1;
                        } else {
                            out[3] += 1;
                        }
                    }
                    // Cell drawing for the depth pass: key below the cell's nearest keep.
                    let clo = lo + Vec2::new((cell % 4) as f32, (cell / 4) as f32) * 4.0;
                    let cp = Vec2::new(cam.x, cam.z).clamp(clo, clo + 4.0);
                    let cdist = Vec3::new(cp.x - cam.x, 0.0, cp.y - cam.z).length();
                    let cell_box = Aabb::new(
                        Vec3::new(clo.x - 1.0, bottom - 1.0, clo.y - 1.0) + t.position,
                        Vec3::new(clo.x + 5.0, top + 1.0, clo.y + 5.0) + t.position,
                    );
                    let cell_in = frustum.intersects(&cell_box);
                    if cell_in && key < keep_at(cdist) * density && i < count {
                        cell_drawn[cell] += 1;
                        out[4] += 1;
                    }
                    if cell_in && i < count {
                        out[5] += 1;
                    }
                    if !outside && i < count {
                        out[6] += 1;
                        // Hidden behind the terrain: the ray to the card's top meets the ground.
                        let tip = root + Vec3::Y * (b.position[3] * 1.2 + 0.3);
                        let dir = tip - cam;
                        let len = dir.length();
                        let mut tt = 0.3;
                        let mut hidden = false;
                        while tt < len - 1.0 {
                            let p = cam + dir * (tt / len);
                            if p.x >= 0.0
                                && p.z >= 0.0
                                && p.x < d.size.x
                                && p.z < d.size.z
                                && p.y < d.sample_height(p.x, p.z) - 0.05
                            {
                                hidden = true;
                                break;
                            }
                            tt += (tt * 0.01).max(0.25);
                        }
                        if hidden {
                            out[9] += 1;
                        }
                    }
                }
                out
            })
            .collect();
        let mut sum = [0u64; 10];
        for t in totals {
            for i in 0..10 {
                sum[i] += t[i];
            }
        }
        let mm = |n: u64| n as f64 / 1e6;
        eprintln!(
            "{level_name}: drawn {:.2} M (outside {:.2}, flat {:.2}, standing in view {:.2}); 4 m cells: depth {:.2} M, lit {:.2} M; per-blade lit {:.2} M; scattered {:.2} M, within 64 m {:.2} M; behind terrain {:.3} M",
            mm(sum[0]),
            mm(sum[1]),
            mm(sum[2]),
            mm(sum[3]),
            mm(sum[4]),
            mm(sum[5]),
            mm(sum[6]),
            mm(sum[7]),
            mm(sum[8]),
            mm(sum[9])
        );
    }
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Blades each grass pass draws after the GPU culling, against the chunk prefixes it tests.
#[test]
#[ignore = "prints only"]
fn meadow_perf_cull_counts() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &quality);
            let target = r.create_target(&device, 1920, 1080);
            let frame = v.frame();
            for _ in 0..3 {
                r.render(&device, &queue, &target, &frame);
            }
            let names = [
                "lit", "depth", "prepass", "casc0", "casc1", "casc2", "casc3", "near0", "near1",
            ];
            for counts in r.terrains.read_grass_cull_counts(&device, &queue) {
                let line: Vec<String> = counts
                    .iter()
                    .zip(names)
                    .filter(|(c, _)| c.1 > 0)
                    .map(|(c, n)| format!("{n} {:.3}/{:.3} M", c.0 as f64 / 1e6, c.1 as f64 / 1e6))
                    .collect();
                eprintln!("{shot} {level}: {}", line.join(", "));
            }
        }
    }
}

/// A/B timing in one process, the two setups alternating (other GPU work on the machine then
/// weighs on both alike): `MISTY_AB=cull` (GPU grass culling off / on), `cover` (Quality ▸
/// Grass Ground Cover off / on), `msaa` (the level's Anti-Aliasing / 2×) or `fast:<name>` (a
/// shortcut of round 3 off / on, render/fast_paths.rs). Median per part over `MISTY_AB_ROUNDS`
/// (3) rounds.
#[test]
#[ignore = "timing, prints only"]
fn meadow_perf_ab() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let what = std::env::var("MISTY_AB").unwrap_or_else(|_| "cull".into());
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
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let setup = |on: bool| {
                let mut q = quality.clone();
                match what.as_str() {
                    "cover" => q.grass_ground_cover = on,
                    "msaa" if on => q.anti_aliasing = crate::quality::AntiAliasing::X2,
                    "msaa" => {}
                    // A shortcut of performance round 3 (render/fast_paths.rs) off / on.
                    f if f.starts_with("fast:") => {
                        assert!(crate::render::fast_paths::set(&f[5..], on), "{f}?");
                    }
                    _ => crate::render::terrain_grass::set_grass_gpu_cull(on),
                }
                q
            };
            breakdown(&device, &queue, &v, &setup(false), (w, h));
            let mut runs: [Vec<super::terrain_grass::gpu_tests::Breakdown>; 2] = Default::default();
            for _ in 0..rounds {
                for on in [false, true] {
                    let q = setup(on);
                    runs[on as usize].push(breakdown(&device, &queue, &v, &q, (w, h)));
                }
            }
            crate::render::terrain_grass::set_grass_gpu_cull(true);
            if let Some(f) = what.strip_prefix("fast:") {
                crate::render::fast_paths::set(f, true);
            }
            let median = |mut x: Vec<f32>| {
                x.sort_by(f32::total_cmp);
                x[x.len() / 2]
            };
            let mut labels: Vec<&'static str> = Vec::new();
            for b in runs.iter().flatten() {
                for (l, _) in &b.parts {
                    if !labels.contains(l) {
                        labels.push(l);
                    }
                }
            }
            eprintln!("== {shot} {level} {w}x{h}: {what} off | on");
            let gpu = |i: usize| median(runs[i].iter().map(|b| b.gpu).collect());
            eprintln!(
                "   {:<32} {:6.2} | {:6.2} ms  ({:.0} | {:.0} FPS)",
                "GPU frame",
                gpu(0),
                gpu(1),
                1000.0 / gpu(0),
                1000.0 / gpu(1)
            );
            let cpu = |i: usize| median(runs[i].iter().map(|b| b.cpu).collect());
            eprintln!("   {:<32} {:6.2} | {:6.2} ms", "CPU render", cpu(0), cpu(1));
            for l in labels {
                let part = |i: usize| median(runs[i].iter().map(|b| b.part(l)).collect());
                let (a, b) = (part(0), part(1));
                if a.max(b) >= 0.05 {
                    eprintln!("   {l:<32} {a:6.2} | {b:6.2} ms");
                }
            }
        }
    }
}

/// The final table: per view and level, GPU culling off (≈ `meadow-trees-1`), the default
/// (culling on), Grass Ground Cover, 2× MSAA and both; the setups interleaved over
/// `MISTY_AB_ROUNDS` (3) rounds and each one's quietest round kept (other GPU work on the
/// machine only ever adds time). `MISTY_SIZE`, `MISTY_SCALED` as for `meadow_perf_times`;
/// `MISTY_SETUPS` picks other setups (options of `apply_opt` joined by `+`).
#[test]
#[ignore = "timing, prints only"]
fn meadow_perf_matrix() {
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
    // `MISTY_SETUPS`: setups as options joined by `+` (`apply_opt`), comma separated.
    let setups: Vec<String> = std::env::var("MISTY_SETUPS")
        .unwrap_or_else(|_| "no culling,default,cover,msaa2,cover+msaa2".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let s = if scaled { quality.render_scale } else { 1.0 };
            let size = ((w as f32 * s) as u32, (h as f32 * s) as u32);
            let setup = |name: &str| {
                let mut q = quality.clone();
                crate::render::terrain_grass::set_grass_gpu_cull(name != "no culling");
                for o in name.split('+') {
                    apply_opt(&mut q, o);
                }
                q
            };
            let mut best: Vec<Option<super::terrain_grass::gpu_tests::Breakdown>> =
                (0..setups.len()).map(|_| None).collect();
            for _ in 0..rounds {
                for (i, name) in setups.iter().enumerate() {
                    let b = breakdown(&device, &queue, &v, &setup(name), size);
                    if best[i].as_ref().is_none_or(|x| b.gpu < x.gpu) {
                        best[i] = Some(b);
                    }
                }
            }
            crate::render::terrain_grass::set_grass_gpu_cull(true);
            eprintln!("== {shot} {level} {}x{}", size.0, size.1);
            for (name, b) in setups.iter().zip(&best) {
                let b = b.as_ref().expect("measured");
                b.print(&format!("{shot} {level} {name}: {:.0} FPS", 1000.0 / b.gpu));
            }
        }
    }
}

/// How the view changes during a VSM measurement (60 frames a second).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Motion {
    Still,
    /// 4 m/s forward, turning 15°/s.
    Walk,
    /// Turning 90°/s on the spot (page requests arrive a frame late).
    Turn,
    /// Still camera, the sun turning 0.5°/s (a fast time of day).
    Sun,
}

/// A VSM measurement: the GPU breakdown and the virtual shadow map's mean counts per timed
/// frame.
struct VsmRun {
    b: crate::render::terrain_grass::gpu_tests::Breakdown,
    rendered: f32,
    resident: f32,
    late: f32,
    swaying: f32,
    catching_up: f32,
}

/// GPU frame median and parts of `view` at `size` with `quality` under `motion`.
fn vsm_breakdown(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    size: (u32, u32),
    motion: Motion,
) -> VsmRun {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_view_target(device, size.0, size.1);
    let camera_at = |i: usize| {
        let mut c = view.camera;
        let t = i as f32 / 60.0;
        let (speed, turn) = match motion {
            Motion::Walk => (4.0, 15.0),
            Motion::Turn => (0.0, 90.0),
            _ => (0.0, 0.0),
        };
        let f = c.forward();
        let flat = glam::Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
        c.position += flat * speed * t;
        c.rotation = glam::Quat::from_rotation_y((turn * t).to_radians()) * c.rotation;
        c
    };
    let lights_at = |i: usize| {
        let mut lights = view.lights.clone();
        if motion == Motion::Sun {
            let a = (0.5 * i as f32 / 60.0).to_radians();
            for l in &mut lights {
                if l.kind == crate::scene::LightType::Directional {
                    l.direction = glam::Quat::from_rotation_y(a) * l.direction;
                }
            }
        }
        lights
    };
    let (mut rendered, mut resident, mut late, mut swaying, mut catching_up) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut totals, mut cpu, mut timed) = (vec![], vec![], 0.0f32);
    for i in 0..120 {
        let (cam, lights) = (camera_at(i), lights_at(i));
        let frame = crate::render::Frame {
            camera: &cam,
            lights: &lights,
            ..view.frame()
        };
        r.render(device, queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if i >= 60 {
            if let Some(ms) = r.gpu_ms() {
                totals.push(ms);
            }
            cpu.push(r.stats().cpu_ms);
            if let Some(s) = r.vsm_stats() {
                rendered += s.rendered as f32;
                resident += s.resident as f32;
                late += s.late as f32;
                swaying += s.near_trees as f32;
                catching_up += s.catching_up as f32;
            }
            timed += 1.0;
        }
    }
    let stats = r.stats();
    let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
    if r.set_pass_timing(device, queue, true) {
        for i in 120..180 {
            let (cam, lights) = (camera_at(i), lights_at(i));
            let frame = crate::render::Frame {
                camera: &cam,
                lights: &lights,
                ..view.frame()
            };
            r.render(device, queue, &target, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            let mut seen: Vec<(&'static str, f32)> = Vec::new();
            for &(label, ms) in r.pass_times() {
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
        }
    }
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    let n = timed.max(1.0);
    VsmRun {
        b: crate::render::terrain_grass::gpu_tests::Breakdown {
            gpu: median(totals),
            cpu: median(cpu),
            parts: parts.into_iter().map(|(l, v)| (l, median(v))).collect(),
            stats,
        },
        rendered: rendered / n,
        resident: resident / n,
        late: late / n,
        swaying: swaying / n,
        catching_up: catching_up / n,
    }
}

/// A VSM setup: name, mode, motion, guard band.
type VsmSetup = (&'static str, crate::quality::ShadowMode, Motion, bool);

/// The VSM setups measured.
fn vsm_setups() -> Vec<VsmSetup> {
    use crate::quality::ShadowMode::{Cascaded, Virtual};
    let mut v = vec![
        ("cascaded still", Cascaded, Motion::Still, true),
        ("virtual still", Virtual, Motion::Still, true),
        ("cascaded walk", Cascaded, Motion::Walk, true),
        ("virtual walk", Virtual, Motion::Walk, true),
        ("cascaded sun", Cascaded, Motion::Sun, true),
        ("virtual sun", Virtual, Motion::Sun, true),
    ];
    if std::env::var_os("MISTY_VSM_TURN").is_some() {
        v.push(("cascaded turn", Cascaded, Motion::Turn, true));
        v.push(("virtual turn", Virtual, Motion::Turn, true));
        v.push(("virtual turn, no guard band", Virtual, Motion::Turn, false));
    }
    v
}

fn vsm_line(name: &str, r: &VsmRun) -> String {
    format!(
        "{name}: {:.0} FPS, pages rendered/frame {:.1} (catching up {:.1}), near-layer trees {:.1}, resident {:.0}, late {:.1}",
        1000.0 / r.b.gpu,
        r.rendered,
        r.catching_up,
        r.swaying,
        r.resident,
        r.late
    )
}

/// Every setup of [`vsm_setups`] for `v` at `quality`, alternating over `rounds` rounds (the
/// quietest of each kept).
fn vsm_rounds(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    v: &GameView,
    quality: &QualityLevel,
    size: (u32, u32),
    rounds: usize,
) -> Vec<(VsmSetup, VsmRun)> {
    let setups = vsm_setups();
    let mut best: Vec<Option<VsmRun>> = (0..setups.len()).map(|_| None).collect();
    for _ in 0..rounds {
        for (i, (_, mode, motion, guard)) in setups.iter().enumerate() {
            super::vsm::set_guard_band(*guard);
            let q = QualityLevel {
                shadow_mode: *mode,
                ..quality.clone()
            };
            let b = vsm_breakdown(device, queue, v, &q, size, *motion);
            if best[i].as_ref().is_none_or(|x| b.b.gpu < x.b.gpu) {
                best[i] = Some(b);
            }
        }
    }
    super::vsm::set_guard_band(true);
    setups
        .into_iter()
        .zip(best)
        .map(|(s, b)| (s, b.expect("measured")))
        .collect()
}

/// Cascaded against virtual sun shadows per level (`MISTY_QUALITY`) and view
/// (`MISTY_GFX_SHOT`): camera still, walking, the sun turning (and with `MISTY_VSM_TURN` a
/// fast turn, with and without the guard band), alternating over `MISTY_AB_ROUNDS` rounds:
/// GPU frame, parts and the virtual map's page counts.
#[test]
#[ignore = "timing, prints only"]
fn meadow_perf_vsm() {
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
    let shots = std::env::var("MISTY_GFX_SHOT").unwrap_or_else(|_| "wide".into());
    // Warm the GPU clocks up.
    if let Some(v) = view("wide") {
        breakdown(&device, &queue, &v, &QualityLevel::low(), (w, h));
    }
    for shot in shots.split(',') {
        let Some(v) = view(shot) else {
            continue;
        };
        for (level, quality) in levels() {
            let runs = vsm_rounds(&device, &queue, &v, &quality, (w, h), rounds);
            eprintln!("== {shot} {level} {w}x{h}");
            for ((name, ..), b) in &runs {
                b.b.print(&vsm_line(&format!("{shot} {level} {name}"), b));
            }
        }
    }
}

/// Cascaded against virtual sun shadows in other sample scenes (`MISTY_VSM_SCENES`, default the
/// static-heavy StressStatic and ShadowsDemo) per level (`MISTY_QUALITY`), the setups of
/// [`meadow_perf_vsm`].
#[test]
#[ignore = "timing, prints only"]
fn scene_perf_vsm() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let materials = crate::assets::AssetCache::open(&root);
    let scenes =
        std::env::var("MISTY_VSM_SCENES").unwrap_or_else(|_| "StressStatic,ShadowsDemo".into());
    let rounds: usize = std::env::var("MISTY_AB_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(2);
    for name in scenes.split(',') {
        let path = root.join(format!("Assets/Scenes/{name}.scene"));
        let Ok((scene, _)) = crate::scene::Scene::load(&path) else {
            eprintln!("{name}: missing, skipped");
            continue;
        };
        let Some(v) = GameView::new(&scene, &materials) else {
            eprintln!("{name}: no camera, skipped");
            continue;
        };
        for (level, quality) in levels() {
            let runs = vsm_rounds(&device, &queue, &v, &quality, (1920, 1080), rounds);
            for ((setup, ..), b) in &runs {
                let shadow = b.b.part("Shadow maps (cascades, local)")
                    + b.b.part("Virtual shadow map pages")
                    + b.b.part("Virtual shadow map marking");
                eprintln!(
                    "{name} {level} {}, GPU {:.2} ms, shadow passes {shadow:.2} ms",
                    vsm_line(setup, b),
                    b.b.gpu
                );
            }
        }
    }
}

/// Cascaded and virtual renders of sample scenes (`MISTY_VSM_SCENES`) per level, 1600×900,
/// written as `vsm_<scene>_<level>_<mode>.png` into `MISTY_GFX_DIR` (look checks).
#[test]
#[ignore = "writes look-check renders; run on purpose"]
fn scene_shots_vsm() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let dir = PathBuf::from(std::env::var("MISTY_GFX_DIR").expect("MISTY_GFX_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let materials = crate::assets::AssetCache::open(&root);
    let scenes =
        std::env::var("MISTY_VSM_SCENES").unwrap_or_else(|_| "ShadowsDemo,StressStatic".into());
    let (w, h) = (1600, 900);
    for name in scenes.split(',') {
        let path = root.join(format!("Assets/Scenes/{name}.scene"));
        let Ok((scene, _)) = crate::scene::Scene::load(&path) else {
            continue;
        };
        let Some(v) = GameView::new(&scene, &materials) else {
            continue;
        };
        for (level, quality) in levels() {
            for mode in [
                crate::quality::ShadowMode::Cascaded,
                crate::quality::ShadowMode::Virtual,
            ] {
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(
                    &device,
                    &QualityLevel {
                        shadow_mode: mode,
                        ..quality.clone()
                    },
                );
                let target = r.create_target(&device, w, h);
                for _ in 0..32 {
                    r.render(&device, &queue, &target, &v.frame());
                    let _ = device.poll(wgpu::PollType::wait_indefinitely());
                }
                let img = image::RgbaImage::from_raw(w, h, read_back(&device, &queue, &target))
                    .expect("read back");
                let mode = format!("{mode:?}").to_lowercase();
                let path = dir.join(format!("vsm_{name}_{level}_{mode}.png"));
                img.save(&path).unwrap();
                eprintln!("wrote {}", path.display());
            }
        }
    }
}
