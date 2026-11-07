//! Shadow stutter harness (ignored; run on purpose in release):
//! `cargo test --release -p misty_engine --lib shadow_stutter_ -- --ignored --nocapture --test-threads=1`
//!
//! Walks and turns the First Person Player's camera through MeadowDemo along a scripted path
//! (60 frames a second, 4 m/s with a stride bob and roll like the player's scripts), renders
//! every frame twice — as the scene is, and with the sun's shadows off (same renderer state
//! otherwise) — and measures how much the image changes from one frame to the next **after
//! reprojecting** the previous frame through the camera motion (the terrain heightfield gives
//! each pixel's world point). The pair gives every 8×8-pixel block a shadow ratio
//! `s = lum(on) / lum(off)`: lit (s ≥ 0.85), penumbra (0.5 ≤ s < 0.85), shadow (s < 0.5).
//! Per class it reports the mean relative change of the image and the share of blocks that
//! "jump" (relative change > 8 %, or shadow ratio change > 0.1) per frame, over the ground
//! between 1.5 m and the shadow distance; frames whose jumps
//! stand far above the run's median are listed with the CPU-side events of that frame
//! (terrain chunk LOD changes, tree LOD switches near the shadows).
//!
//! Env:
//! - `MISTY_STUTTER_PATH` = `walk` (default), `turn` (standing, 40°/s), `still` (fixed camera,
//!   the clock moving), `frozen` (fixed camera, fixed clock).
//! - `MISTY_STUTTER_AT` = `start` (the player's start, default), `oak` (towards the broad
//!   oak), `x,z,yaw` (world metres, degrees).
//! - `MISTY_STUTTER_FRAMES` (default 300), `MISTY_SIZE=WxH` (default 1280x720),
//!   `MISTY_QUALITY` = `high` (default), `medium`, `vsm` (High with virtual shadows), `ultra`.
//! - `MISTY_STUTTER_CLOCK=1`: the wind / cloud / fog clock runs with the frames on every
//!   path (default: only on `still`).
//! - `MISTY_STUTTER_OFF` = comma list of things turned off to attribute the jumps: `trees`
//!   (tree shadows), `grass` (grass shadows), `taa`, `mask` (temporal shadow mask and
//!   denoiser), `gi`, `clouds` (cloud shadows), `contact`, `near` (near grass shadow map),
//!   `blades` (no grass at all: the cleanest view of terrain and tree shadows).
//! - `MISTY_STUTTER_BLOCK` (block side in pixels, default 8), `MISTY_STUTTER_SPEED` (m/s,
//!   default 4 = the player's walk).
//! - `MISTY_STUTTER_DIR`: writes frames (every 60th and the spikes) and block-change heat maps.
//! - `MISTY_STUTTER_CSV`: writes the per-frame numbers.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec2, Vec3};
use rayon::prelude::*;

use crate::camera::Camera;
use crate::quality::{QualityLevel, Tier};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::terrain_grass::gpu_tests::{Shot as GrassShot, meadow_view};
use crate::render::trees::gpu_tests::{Shot as TreeShot, meadow_view as tree_view};
use crate::render::{Renderer, terrain_lod};
use crate::render_scene::GameView;
use crate::scene::Shadows;
use crate::terrain::{TerrainData, TerrainDraw};

fn block() -> u32 {
    env("MISTY_STUTTER_BLOCK")
        .and_then(|s| s.parse().ok())
        .unwrap_or(8)
}
const DT: f32 = 1.0 / 60.0;
fn walk_speed() -> f32 {
    env("MISTY_STUTTER_SPEED")
        .and_then(|s| s.parse().ok())
        .unwrap_or(4.0)
}
/// Stride frequency (Hz) of the bob.
const STRIDE: f32 = 1.9;
/// Blocks whose relative change is above this "jump".
const JUMP: f32 = 0.08;
/// Shadow-ratio change that counts as a jump.
const JUMP_S: f32 = 0.1;
const CLASSES: [&str; 3] = ["shadow", "penumbra", "lit"];

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn quality() -> (String, QualityLevel) {
    let name = env("MISTY_QUALITY").unwrap_or_else(|| "high".into());
    let mut q = match name.as_str() {
        "medium" => QualityLevel::medium(),
        "low" => QualityLevel::low(),
        "ultra" => QualityLevel::ultra(),
        _ => QualityLevel::high(),
    };
    if name == "vsm" {
        q.shadow_mode = crate::quality::ShadowMode::Virtual;
    }
    if let Some(d) = env("MISTY_STUTTER_DISTANCE").and_then(|s| s.parse().ok()) {
        q.shadow_distance = d;
    }
    (name, q)
}

fn off(what: &str) -> bool {
    env("MISTY_STUTTER_OFF").is_some_and(|s| s.split(',').any(|o| o == what))
}

/// The view the walk starts from.
fn start_view() -> GameView {
    match env("MISTY_STUTTER_AT").as_deref() {
        Some("oak") => tree_view(TreeShot::Close),
        Some(at) if at.contains(',') => {
            let v: Vec<f32> = at
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            let mut view = meadow_view(GrassShot::WideGolden);
            if v.len() == 3 {
                let t = &view.terrains[0];
                let ground = t
                    .data
                    .sample_height(v[0] - t.position.x, v[1] - t.position.z);
                view.camera.position = Vec3::new(v[0], ground + t.position.y + 1.7, v[1]);
                view.camera.rotation = Quat::from_rotation_y(v[2].to_radians());
            }
            view
        }
        _ => meadow_view(GrassShot::WideGolden),
    }
}

/// Terrain draws with `edit` applied to their data (own store key).
fn edited(view: &GameView, tag: &str, edit: impl Fn(&mut TerrainData)) -> Vec<TerrainDraw> {
    view.terrains
        .iter()
        .map(|t| {
            let mut d = (*t.data).clone();
            edit(&mut d);
            TerrainDraw {
                key: format!("{}#{tag}", t.key),
                data: Arc::new(d),
                ..t.clone()
            }
        })
        .collect()
}

/// World height of the terrain under `p` (first terrain).
fn ground(view: &GameView, p: Vec3) -> f32 {
    let t = &view.terrains[0];
    t.data.sample_height(p.x - t.position.x, p.z - t.position.z) + t.position.y
}

struct Path {
    kind: String,
    start: Vec3,
    eye: f32,
    yaw: f32,
    pitch: f32,
}

impl Path {
    fn new(view: &GameView) -> Self {
        let kind = env("MISTY_STUTTER_PATH").unwrap_or_else(|| "walk".into());
        let start = view.camera.position;
        let f = view.camera.forward();
        Self {
            kind,
            start,
            eye: start.y - ground(view, start),
            yaw: f.x.atan2(f.z),
            pitch: (-f.y).asin(),
        }
    }

    /// The camera at frame `k`.
    fn camera(&self, view: &GameView, k: usize) -> Camera {
        let t = k as f32 * DT;
        let mut cam = view.camera;
        let (mut yaw, mut pitch, mut roll) = (self.yaw, self.pitch, 0.0f32);
        let mut pos = self.start;
        match self.kind.as_str() {
            "walk" => {
                // Mouse look drifts ±12° over 6 s while walking forward along the start yaw.
                yaw += (12.0f32).to_radians() * (t * std::f32::consts::TAU / 6.0).sin();
                let dir = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
                pos += dir * walk_speed() * t;
                let phase = t * STRIDE * std::f32::consts::TAU;
                pos.y = ground(view, pos) + self.eye + 0.035 * phase.sin();
                roll = 0.8f32.to_radians() * (phase * 0.5).sin();
                pitch += 1.0f32.to_radians() * phase.cos();
            }
            "turn" => yaw += 40.0f32.to_radians() * t,
            _ => {}
        }
        cam.position = pos;
        cam.rotation =
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(roll);
        cam
    }
}

/// Linear luminance of an sRGB8 pixel.
fn luminance(p: &[u8]) -> f32 {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2])
}

/// Linear luminance of every pixel.
fn lum_image(rgba: &[u8]) -> Vec<f32> {
    rgba.par_chunks(4).map(luminance).collect()
}

/// World point the camera ray through block `i` meets the terrain at (None = sky / too far).
fn block_world(view: &GameView, inv: &Mat4, eye: Vec3, bw: u32, bh: u32, i: u32) -> Option<Vec3> {
    let (bx, by) = (i % bw, i / bw);
    let ndc = Vec2::new(
        (bx as f32 + 0.5) / bw as f32 * 2.0 - 1.0,
        1.0 - (by as f32 + 0.5) / bh as f32 * 2.0,
    );
    let far = inv.project_point3(ndc.extend(1.0));
    let dir = (far - eye).normalize();
    let t = &view.terrains[0];
    let d = &t.data;
    let above = |p: Vec3| {
        let l = p - t.position;
        l.y - d.sample_height(l.x, l.z)
    };
    let (mut a, mut dist) = (0.0f32, 0.05f32);
    while dist < 600.0 {
        let p = eye + dir * dist;
        let l = p - t.position;
        if l.x < 0.0 || l.z < 0.0 || l.x >= d.size.x || l.z >= d.size.z {
            return None;
        }
        if above(p) <= 0.0 {
            // Bisect between the last point above and this one.
            let (mut lo, mut hi) = (a, dist);
            for _ in 0..12 {
                let m = 0.5 * (lo + hi);
                if above(eye + dir * m) > 0.0 {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            return Some(eye + dir * hi);
        }
        a = dist;
        dist += (dist * 0.006).max(0.03);
    }
    None
}

/// `img` (w × h, pixel centres at integer + 0.5) at `(x, y)`; None outside.
fn bilinear(img: &[f32], w: u32, h: u32, x: f32, y: f32) -> Option<f32> {
    let (x, y) = (x - 0.5, y - 0.5);
    if x < 0.0 || y < 0.0 || x > (w - 1) as f32 || y > (h - 1) as f32 {
        return None;
    }
    let (x0, y0) = (x as u32, y as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let g = |x: u32, y: u32| img[(y * w + x) as usize];
    let top = g(x0, y0) * (1.0 - fx) + g(x1, y0) * fx;
    let bottom = g(x0, y1) * (1.0 - fx) + g(x1, y1) * fx;
    Some(top * (1.0 - fy) + bottom * fy)
}

/// One rendered frame: full-resolution luminance with and without the sun's shadows, the
/// terrain point under each block centre and the camera.
struct Shot {
    on: Vec<f32>,
    off: Vec<f32>,
    world: Vec<Option<Vec3>>,
    view_proj: Mat4,
}

#[derive(Default, Clone, Copy)]
struct ClassStats {
    blocks: u32,
    change: f64,
    jumps: u32,
    s_change: f64,
    s_jumps: u32,
}

/// Per-frame stats per class, and each block's relative change (heat map; -1 = no data).
/// Every pixel of a block is reprojected into the previous frame with the motion of the
/// block's terrain point and sampled there (bilinear); the block's sums of this frame and of
/// the reprojected previous frame are compared, so resampling noise of fine texture averages
/// out and only coherent changes remain.
fn compare(prev: &Shot, now: &Shot, w: u32, h: u32) -> ([ClassStats; 3], Vec<f32>) {
    let block = block();
    let (bw, bh) = (w / block, h / block);
    let rows: Vec<(usize, usize, f32, f32)> = (0..bw * bh)
        .into_par_iter()
        .filter_map(|i| {
            let p = now.world[i as usize]?;
            let (bx, by) = (i % bw, i / bw);
            let c = prev.view_proj.project_point3(p);
            let px = (c.x * 0.5 + 0.5) * w as f32;
            let py = (0.5 - c.y * 0.5) * h as f32;
            let (cx, cy) = (
                (bx as f32 + 0.5) * block as f32,
                (by as f32 + 0.5) * block as f32,
            );
            let (dx, dy) = (px - cx, py - cy);
            let (mut a, mut a0, mut b, mut b0) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for y in 0..block {
                for x in 0..block {
                    let (ix, iy) = (bx * block + x, by * block + y);
                    let k = (iy * w + ix) as usize;
                    let (sx, sy) = (ix as f32 + 0.5 + dx, iy as f32 + 0.5 + dy);
                    a += now.on[k];
                    b += now.off[k];
                    a0 += bilinear(&prev.on, w, h, sx, sy)?;
                    b0 += bilinear(&prev.off, w, h, sx, sy)?;
                }
            }
            let (s, s0) = ((a / b.max(1e-4)).min(1.5), (a0 / b0.max(1e-4)).min(1.5));
            let class = if s < 0.5 {
                0
            } else if s < 0.85 {
                1
            } else {
                2
            };
            let n = (block * block) as f32;
            let rel = (a - a0).abs() / a.max(a0).max(0.02 * n);
            Some((i as usize, class, rel, (s - s0).abs()))
        })
        .collect();
    let mut stats = [ClassStats::default(); 3];
    let mut heat = vec![-1.0; (bw * bh) as usize];
    for (i, class, rel, ds) in rows {
        let c = &mut stats[class];
        c.blocks += 1;
        c.change += rel as f64;
        c.s_change += ds as f64;
        c.jumps += (rel > JUMP) as u32;
        c.s_jumps += (ds > JUMP_S) as u32;
        heat[i] = rel;
    }
    (stats, heat)
}

/// Chunk LODs of every terrain chunk for a camera at `eye` (as `TerrainCache::select`).
fn chunk_lods(t: &TerrainDraw, eye: Vec3) -> Vec<u8> {
    let d = &t.data;
    let res = d.heightmap_resolution;
    let cells = terrain_lod::chunk_cells(res);
    let per = (res - 1) / cells;
    let s = d.spacing();
    let chunk_m = cells as f32 * s.x.max(s.y);
    let range = |c: usize| {
        let (cx, cz) = (c % per, c / per);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for z in cz * cells..=(cz + 1) * cells {
            for x in cx * cells..=(cx + 1) * cells {
                let h = d.height(x, z);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        (lo, hi)
    };
    let ranges: Vec<(f32, f32)> = (0..per * per).map(range).collect();
    terrain_lod::select(per, chunk_m, terrain_lod::max_lod(cells), |c| {
        let (cx, cz) = (c % per, c / per);
        let (lo, hi) = ranges[c];
        let min = Vec3::new(
            cx as f32 * cells as f32 * s.x,
            lo * d.size.y,
            cz as f32 * cells as f32 * s.y,
        ) + t.position;
        let max = min + Vec3::new(cells as f32 * s.x, (hi - lo) * d.size.y, cells as f32 * s.y);
        eye.clamp(min, max).distance(eye)
    })
}

fn heat_image(heat: &[f32], bw: u32, bh: u32) -> image::RgbImage {
    image::RgbImage::from_fn(bw, bh, |x, y| {
        let v = heat[(y * bw + x) as usize];
        if v < 0.0 {
            return image::Rgb([40, 40, 60]);
        }
        let t = (v / (2.0 * JUMP)).clamp(0.0, 1.0);
        image::Rgb([
            (255.0 * t) as u8,
            (255.0 * (1.0 - (t - 0.5).abs() * 2.0)) as u8,
            30,
        ])
    })
}

/// The start view with the `MISTY_STUTTER_OFF` edits (`shadowed` = false: the sun's
/// shadows off too).
fn scene_view(shadowed: bool) -> GameView {
    let mut view = start_view();
    if off("trees") || off("grass") || off("blades") {
        let (trees, grass, blades) = (off("trees"), off("grass"), off("blades"));
        view.terrains = edited(&view, "stutter", |d| {
            if trees {
                d.trees.cast_shadows = false;
            }
            if grass {
                d.details.cast_shadows = false;
            }
            if blades {
                d.details.enabled = false;
            }
        });
    }
    if off("clouds") {
        view.post.clouds.shadows = false;
    }
    for l in &mut view.lights {
        if off("contact") {
            l.contact_shadows = false;
        }
        if !shadowed && l.kind == crate::scene::LightType::Directional {
            l.shadows = Shadows::None;
        }
    }
    view
}

#[test]
#[ignore = "shadow stability measurements; run on purpose"]
fn shadow_stutter_walk() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = env("MISTY_SIZE")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((1280, 720));
    let frames: usize = env("MISTY_STUTTER_FRAMES")
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let (level, mut q) = quality();
    if off("taa") {
        q.temporal_aa = false;
    }
    if off("mask") {
        q.shadow_temporal = false;
        q.shadow_denoise = false;
    }
    if off("gi") {
        q.global_illumination = Tier::Off;
    }
    if off("near") {
        q.grass_shadow_map = Tier::Off;
    }
    let mut view = scene_view(true);
    // The reference: the same view with the sun's shadows off.
    let mut unshadowed = scene_view(false);
    let path = Path::new(&view);
    let clock = path.kind == "still" || env("MISTY_STUTTER_CLOCK").is_some();
    let dir = env("MISTY_STUTTER_DIR").map(PathBuf::from);
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let tag = env("MISTY_STUTTER_TAG").unwrap_or_else(|| level.clone());

    let mut on = Renderer::with_queue(&device, &queue);
    let mut off_r = Renderer::with_queue(&device, &queue);
    on.set_quality(&device, &q);
    off_r.set_quality(&device, &q);
    let target_on = on.create_target(&device, w, h);
    let target_off = off_r.create_target(&device, w, h);
    let (bw, bh) = (w / block(), h / block());
    let reach = q.shadow_distance + 5.0;
    let aspect = w as f32 / h as f32;

    // Trees near enough to cast into the cascades (terrain-local trunks, world feet).
    let trunks: Vec<(Vec3, f32)> = crate::terrain::trees::trunks(&view.terrains[0].data)
        .into_iter()
        .map(|t| (t.foot + view.terrains[0].position, t.height))
        .collect();
    let tree_lod_d = view.terrains[0].data.trees.lod_distance * q.lod_bias.max(0.1);

    // Warm-up: converge the temporal passes at the first pose.
    let warm = 30;
    let mut prev: Option<Shot> = None;
    let mut prev_lods: Option<Vec<u8>> = None;
    let mut prev_tree: Option<Vec<bool>> = None;
    let mut per_frame: Vec<([ClassStats; 3], String)> = Vec::new();
    let mut csv = String::from(
        "frame,shadow_blocks,shadow_change,shadow_jumps,shadow_sjumps,penumbra_blocks,penumbra_change,penumbra_jumps,penumbra_sjumps,lit_blocks,lit_change,lit_jumps,lit_sjumps,chunk_lod_changes,tree_lod_switches\n",
    );
    let mut heats: Vec<(usize, Vec<f32>, Vec<u8>)> = Vec::new();
    for k in 0..warm + frames {
        let i = k.saturating_sub(warm);
        let cam = path.camera(&view, i);
        let time = if clock { 3.0 + i as f32 * DT } else { 3.0 };
        view.camera = cam;
        view.environment.grass_time = Some(time);
        unshadowed.camera = cam;
        unshadowed.environment.grass_time = Some(time);
        on.render(&device, &queue, &target_on, &view.frame());
        off_r.render(&device, &queue, &target_off, &unshadowed.frame());
        if k < warm {
            continue;
        }
        let a = read_back(&device, &queue, &target_on);
        let b = read_back(&device, &queue, &target_off);
        let view_proj = cam.view_projection(aspect);
        let inv = view_proj.inverse();
        let world: Vec<Option<Vec3>> = (0..bw * bh)
            .into_par_iter()
            .map(|b| block_world(&view, &inv, cam.position, bw, bh, b))
            // Only the ground the sun's shadows reach (and not the blades at the feet).
            .map(|p| p.filter(|p| (1.5..reach).contains(&p.distance(cam.position))))
            .collect();
        let now = Shot {
            on: lum_image(&a),
            off: lum_image(&b),
            world,
            view_proj,
        };
        // CPU events: terrain chunk LOD changes, trees switching LOD within the shadow reach.
        let lods = chunk_lods(&view.terrains[0], cam.position);
        let chunk_changes = prev_lods
            .as_ref()
            .map_or(0, |p| p.iter().zip(&lods).filter(|(a, b)| a != b).count());
        let tree_lods: Vec<bool> = trunks
            .iter()
            .map(|(foot, h)| (*foot + Vec3::Y * *h).distance(cam.position) > tree_lod_d)
            .collect();
        let tree_switches = prev_tree.as_ref().map_or(0, |p| {
            p.iter()
                .zip(&tree_lods)
                .zip(&trunks)
                .filter(|((a, b), (foot, _))| a != b && foot.distance(cam.position) < 150.0)
                .count()
        });
        if let Some(p) = &prev {
            let (stats, heat) = compare(p, &now, w, h);
            let mut line = format!("{i}");
            for c in &stats {
                let n = c.blocks.max(1) as f64;
                line += &format!(
                    ",{},{:.5},{:.5},{:.5}",
                    c.blocks,
                    c.change / n,
                    c.jumps as f64 / n,
                    c.s_jumps as f64 / n
                );
            }
            line += &format!(",{chunk_changes},{tree_switches}\n");
            csv += &line;
            let events = format!("chunks {chunk_changes} trees {tree_switches}");
            per_frame.push((stats, events));
            // Keep memory bounded: only the frames that may be written keep their image.
            let jumpy = stats
                .iter()
                .any(|c| c.jumps as f64 > 0.01 * c.blocks.max(1) as f64);
            let keep = dir.is_some() && (i % 60 == 0 || jumpy);
            heats.push((i, heat, if keep { a.clone() } else { Vec::new() }));
        }
        prev = Some(now);
        prev_lods = Some(lods);
        prev_tree = Some(tree_lods);
    }

    // Summary per class: mean change, mean jump share, 99th percentile and worst jump share.
    eprintln!(
        "== shadow stutter: {tag} path {} at {} ({frames} frames, {w}x{h}, clock {}) ==",
        path.kind,
        env("MISTY_STUTTER_AT").unwrap_or_else(|| "start".into()),
        if clock { "running" } else { "fixed" }
    );
    let mut spikes: Vec<usize> = Vec::new();
    for (ci, name) in CLASSES.iter().enumerate() {
        let shares: Vec<f64> = per_frame
            .iter()
            .map(|(s, _)| s[ci].jumps as f64 / s[ci].blocks.max(1) as f64)
            .collect();
        let s_shares: Vec<f64> = per_frame
            .iter()
            .map(|(s, _)| s[ci].s_jumps as f64 / s[ci].blocks.max(1) as f64)
            .collect();
        let change: f64 = per_frame
            .iter()
            .map(|(s, _)| s[ci].change / s[ci].blocks.max(1) as f64)
            .sum::<f64>()
            / per_frame.len().max(1) as f64;
        let blocks: f64 = per_frame
            .iter()
            .map(|(s, _)| s[ci].blocks as f64)
            .sum::<f64>()
            / per_frame.len().max(1) as f64;
        let mut sorted = shares.clone();
        sorted.sort_by(f64::total_cmp);
        let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
        let mean = shares.iter().sum::<f64>() / shares.len().max(1) as f64;
        let s_mean = s_shares.iter().sum::<f64>() / s_shares.len().max(1) as f64;
        eprintln!(
            "{name:>9}: {blocks:>7.0} blocks  change {:.4}  jumps mean {:.5} median {:.5} p99 {:.5} max {:.5}  ratio-jumps mean {:.5}",
            change,
            mean,
            pct(0.5),
            pct(0.99),
            pct(1.0),
            s_mean
        );
        // Spikes: frames with jumps above 4× the median and above 0.5 % of the class.
        let median = pct(0.5);
        for (f, v) in shares.iter().enumerate() {
            if *v > (4.0 * median).max(0.005) && !spikes.contains(&f) {
                spikes.push(f);
            }
        }
    }
    spikes.sort();
    eprintln!("spike frames ({}):", spikes.len());
    for &f in spikes.iter().take(40) {
        let (s, events) = &per_frame[f];
        let share = |c: usize| s[c].jumps as f64 / s[c].blocks.max(1) as f64;
        eprintln!(
            "  frame {:>4}: shadow {:.4} penumbra {:.4} lit {:.4}  [{events}]",
            heats[f].0,
            share(0),
            share(1),
            share(2)
        );
    }
    let events: usize = per_frame
        .iter()
        .filter(|(_, e)| !e.starts_with("chunks 0 trees 0"))
        .count();
    eprintln!("frames with CPU events: {events}");
    if let Some(p) = env("MISTY_STUTTER_CSV") {
        std::fs::write(&p, csv).unwrap();
        eprintln!("wrote {p}");
    }
    if let Some(d) = &dir {
        for (f, (i, heat, rgba)) in heats.iter().enumerate() {
            let spike = spikes.contains(&f);
            if !(spike || i % 60 == 0) {
                continue;
            }
            heat_image(heat, bw, bh)
                .save(d.join(format!("{tag}_heat_{i:04}.png")))
                .unwrap();
            if !rgba.is_empty() {
                image::RgbaImage::from_raw(w, h, rgba.clone())
                    .unwrap()
                    .save(d.join(format!("{tag}_frame_{i:04}.png")))
                    .unwrap();
            }
        }
        eprintln!("wrote frames into {}", d.display());
    }
}

/// Depths of layer `layer` of a depth texture (the top-left `side`² texels).
fn read_depth(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    layer: u32,
    side: u32,
) -> Vec<f32> {
    let row = (side * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty stutter depth readback"),
        size: (row * side) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::DepthOnly,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(side),
            },
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped");
    let mut out = Vec::with_capacity((side * side) as usize);
    for y in 0..side as usize {
        let r = &data[y * row as usize..y * row as usize + side as usize * 4];
        out.extend(r.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
    }
    out
}

/// One shadow map layer of one frame: its view-projection, side and depths.
struct MapShot {
    view_proj: Mat4,
    side: u32,
    depth: Vec<f32>,
}

/// Distance along the light (metres) of a stored depth.
fn light_depth(m: &Mat4, d: f32) -> f32 {
    let row = m.row(2);
    (d - row.w) / row.truncate().length()
}

/// Texels both frames cover (within `used` metres of `at` across the ground), texels of `now` whose caster depth differs from the same world
/// texel of `prev` by more than 3 cm (or that gained / lost a caster short of the far plane),
/// and the change mask
/// (true = changed) of `now`.
fn map_changes(prev: &MapShot, now: &MapShot, at: Vec3, used: f32) -> (u32, u32, Vec<bool>) {
    let inv = now.view_proj.inverse();
    const MAP_CHANGE: f32 = 0.03;
    const FAR: f32 = 0.99;
    let side = now.side as i64;
    let texel = |m: &Mat4, s: u32| {
        let c = m.project_point3(at);
        Vec2::new((c.x * 0.5 + 0.5) * s as f32, (0.5 - c.y * 0.5) * s as f32)
    };
    let off = texel(&now.view_proj, now.side) - texel(&prev.view_proj, prev.side);
    let (ox, oy) = (off.x.round() as i64, off.y.round() as i64);
    let rows: Vec<(u32, u32, Vec<bool>)> = (0..side)
        .into_par_iter()
        .map(|y| {
            let (mut both, mut changed) = (0u32, 0u32);
            let mut mask = vec![false; side as usize];
            let py = y - oy;
            if !(0..side).contains(&py) {
                return (0, 0, mask);
            }
            for x in 0..side {
                let px = x - ox;
                if !(0..side).contains(&px) {
                    continue;
                }
                let a = now.depth[(y * side + x) as usize];
                let b = prev.depth[(py * side + px) as usize];
                let (ea, eb) = (a >= 0.99999, b >= 0.99999);
                // Casters crossing the far plane (which follows the camera) only matter to
                // receivers outside the cascade's sphere: left out.
                if ea && eb || ea && b > FAR || eb && a > FAR {
                    continue;
                }
                // Only the part of the map the lit shaders read (`used` metres around the
                // camera, across the ground).
                if used < f32::MAX {
                    let d = if ea { b } else { a };
                    let ndc = Vec3::new(
                        (x as f32 + 0.5) / side as f32 * 2.0 - 1.0,
                        1.0 - (y as f32 + 0.5) / side as f32 * 2.0,
                        d,
                    );
                    let p = inv.project_point3(ndc);
                    if Vec2::new(p.x - at.x, p.z - at.z).length() > used {
                        continue;
                    }
                }
                both += 1;
                let differs = ea != eb
                    || (light_depth(&now.view_proj, a) - light_depth(&prev.view_proj, b)).abs()
                        > MAP_CHANGE;
                if differs {
                    changed += 1;
                    mask[x as usize] = true;
                }
            }
            (both, changed, mask)
        })
        .collect();
    let both = rows.iter().map(|r| r.0).sum();
    let changed = rows.iter().map(|r| r.1).sum();
    let mask = rows.into_iter().flat_map(|r| r.2).collect();
    (both, changed, mask)
}

/// The change mask shrunk to `out`² (red where any texel of the cell changed) over the layer's
/// own depths in grey.
fn mask_image(shot: &MapShot, mask: &[bool], out: u32) -> image::RgbImage {
    let k = (shot.side / out).max(1);
    let out = shot.side / k;
    image::RgbImage::from_fn(out, out, |x, y| {
        let mut any = false;
        let mut d = 0.0;
        for j in 0..k {
            for i in 0..k {
                let t = ((y * k + j) * shot.side + x * k + i) as usize;
                any |= mask[t];
                d += shot.depth[t];
            }
        }
        if any {
            image::Rgb([255, 40, 40])
        } else {
            let g = (255.0 * (1.0 - d / (k * k) as f32).clamp(0.0, 1.0).powf(0.3)) as u8;
            image::Rgb([g, g, g])
        }
    })
}

/// One frame of [`shadow_stutter_maps`]: frame, (changed, covered) texels per map, terrain
/// chunk LOD changes.
type MapRow = (usize, Vec<(u32, u32)>, usize);

const MAP_NAMES: [&str; 6] = ["c0", "c1", "c2", "c3", "near0", "near1"];

/// Camera-independence of the shadow maps' content: walks the path, reads every sun cascade
/// and both near grass shadow map slices back each frame and counts the texels whose caster
/// changed against the same world texel of the frame before (the maps are snapped to a texel
/// grid fixed in the world, so a static scene must give identical depths wherever both frames
/// cover it). Any change is a caster that popped because the camera moved: terrain chunk
/// detail, tree LOD, grass caster selection, culling. `MISTY_STUTTER_DIR` writes the change
/// masks of the frames that changed most.
#[test]
#[ignore = "shadow stability measurements; run on purpose"]
fn shadow_stutter_maps() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let frames: usize = env("MISTY_STUTTER_FRAMES")
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let (level, q) = quality();
    let mut view = scene_view(true);
    let path = Path::new(&view);
    let tag = env("MISTY_STUTTER_TAG").unwrap_or_else(|| level.clone());
    let dir = env("MISTY_STUTTER_DIR").map(PathBuf::from);
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &q);
    let target = r.create_target(&device, 640, 360);
    let mut prev: Vec<Option<MapShot>> = Vec::new();
    let mut prev_lods: Option<Vec<u8>> = None;
    // Per frame: (changed, covered) per map, chunk LOD changes.
    let mut rows: Vec<MapRow> = Vec::new();
    // The masks of the frames that changed most: (share, frame, map, image).
    let mut worst: Vec<(f64, usize, usize, image::RgbImage)> = Vec::new();
    let mut csv = String::from("frame");
    for n in MAP_NAMES {
        csv += &format!(",{n}_changed,{n}_covered");
    }
    csv += ",chunk_lod_changes\n";
    for k in 0..frames + 2 {
        let cam = path.camera(&view, k);
        view.camera = cam;
        view.environment.grass_time = Some(3.0);
        r.render(&device, &queue, &target, &view.frame());
        let (cascades, near) = r.last_shadow_views.clone();
        let cascade_tex = r.shadows.cascade_array_view.texture().clone();
        let near_tex = r.shadows.grass_near.view.texture().clone();
        let mut shots: Vec<Option<MapShot>> = Vec::new();
        for c in 0..4 {
            shots.push(cascades.get(c).map(|cascade| MapShot {
                view_proj: cascade.view_proj,
                side: r.shadows.directional_size,
                depth: read_depth(
                    &device,
                    &queue,
                    &cascade_tex,
                    c as u32,
                    r.shadows.directional_size,
                ),
            }));
        }
        for s in 0..2 {
            let side = r.shadows.grass_near.slice_size(s);
            shots.push(near.map(|m| MapShot {
                view_proj: m[s],
                side,
                depth: read_depth(&device, &queue, &near_tex, s as u32, side),
            }));
        }
        let lods = chunk_lods(&view.terrains[0], cam.position);
        let chunk_changes = prev_lods
            .as_ref()
            .map_or(0, |p| p.iter().zip(&lods).filter(|(a, b)| a != b).count());
        prev_lods = Some(lods);
        if k >= 2 {
            let f = k - 2;
            let mut per = Vec::new();
            let mut line = format!("{f}");
            for (i, s) in shots.iter().enumerate() {
                let (covered, changed, mask) = match (prev.get(i).and_then(|p| p.as_ref()), s) {
                    (Some(p), Some(s)) if p.side == s.side => {
                        // The near slices are read out to 95 % of the inner half side and to
                        // the outer one's edge.
                        let used = match i {
                            4 => 0.95 * crate::render::grass_near_shadow::INNER_RADIUS,
                            5 => crate::render::grass_near_shadow::RADIUS,
                            _ => f32::MAX,
                        };
                        map_changes(p, s, cam.position, used)
                    }
                    _ => (0, 0, Vec::new()),
                };
                line += &format!(",{changed},{covered}");
                per.push((changed, covered));
                if changed > 0
                    && let Some(d) = &dir
                    && let Some(s) = s
                {
                    let share = changed as f64 / covered.max(1) as f64;
                    // Every map's mask at frame 60 (and the worst frames below).
                    if f == 60 {
                        let name = format!("{tag}_mask60_{}.png", MAP_NAMES[i]);
                        mask_image(s, &mask, 512).save(d.join(name)).unwrap();
                    }
                    if worst.len() < 8 || share > worst[worst.len() - 1].0 {
                        worst.push((share, f, i, mask_image(s, &mask, 512)));
                        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
                        worst.truncate(8);
                    }
                }
            }
            line += &format!(",{chunk_changes}\n");
            csv += &line;
            rows.push((f, per, chunk_changes));
        }
        prev = shots;
    }
    eprintln!(
        "== shadow map stability: {tag} path {} at {} ({frames} frames) ==",
        path.kind,
        env("MISTY_STUTTER_AT").unwrap_or_else(|| "start".into()),
    );
    for (i, n) in MAP_NAMES.iter().enumerate() {
        let share = |r: &MapRow| r.1[i].0 as f64 / r.1[i].1.max(1) as f64;
        let frames_changed = rows.iter().filter(|r| r.1[i].0 > 0).count();
        let mean = rows.iter().map(share).sum::<f64>() / rows.len().max(1) as f64;
        let max = rows.iter().map(share).fold(0.0, f64::max);
        let texels: u64 = rows.iter().map(|r| r.1[i].0 as u64).sum();
        eprintln!(
            "{n:>6}: frames with changed casters {frames_changed:>4} / {}  mean share {mean:.6}  max share {max:.6}  changed texels {texels}",
            rows.len(),
        );
    }
    let chunk_frames = rows.iter().filter(|r| r.2 > 0).count();
    eprintln!("frames with terrain chunk LOD changes: {chunk_frames}");
    if let Some(p) = env("MISTY_STUTTER_CSV") {
        std::fs::write(&p, csv).unwrap();
    }
    if let Some(d) = &dir {
        for (share, f, map, img) in &worst {
            let name = format!("{tag}_mask_{f:04}_{}_{share:.4}.png", MAP_NAMES[*map]);
            img.save(d.join(name)).unwrap();
        }
    }
}
