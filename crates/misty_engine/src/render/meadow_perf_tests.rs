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

/// The look options of round 2 named in `MISTY_PERF_OPTS` (comma separated), on top of the
/// level's own settings.
fn apply_opts(mut q: QualityLevel) -> QualityLevel {
    let opts = std::env::var("MISTY_PERF_OPTS").unwrap_or_default();
    for o in opts.split(',').filter(|o| !o.is_empty()) {
        match o {
            // Quality ▸ Grass Ground Cover.
            "cover" => q.grass_ground_cover = true,
            // Quality ▸ Anti-Aliasing 2×.
            "msaa2" => q.anti_aliasing = crate::quality::AntiAliasing::X2,
            _ => eprintln!("(option {o} unknown: ignored)"),
        }
    }
    q
}

fn view(name: &str) -> Option<GameView> {
    Some(match name {
        "wide" => meadow_view(GrassShot::WideGolden),
        "close" => meadow_view(GrassShot::CloseGolden),
        "behind" => tree_view(TreeShot::Behind),
        "oak" => tree_view(TreeShot::Close),
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
/// Grass Ground Cover off / on) or `msaa` (the level's Anti-Aliasing / 2×). Median per part over `MISTY_AB_ROUNDS` (3) rounds.
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
/// machine only ever adds time). `MISTY_SIZE`, `MISTY_SCALED` as for `meadow_perf_times`.
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
    let setups = ["no culling", "default", "cover", "msaa2", "cover+msaa2"];
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
                q.grass_ground_cover = name.contains("cover");
                if name.contains("msaa2") {
                    q.anti_aliasing = crate::quality::AntiAliasing::X2;
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
