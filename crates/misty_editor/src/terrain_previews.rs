//! Live previews of the Terrain Inspector's palettes: grass tufts, trees, detail meshes, placed
//! prefabs and texture layer swatches.
//!
//! - **Grass** tufts are built on the CPU from the same recipe the terrain uses: blade heights,
//!   widths, bends, seed heads and the per-blade dry mix and tint are drawn like
//!   `detail_layers::species_blades`; each blade is bent along the grass shader's circular arc
//!   and coloured with its colour model (healthy / dry mix, per-tuft hue and value jitter, dry
//!   tips, root-to-tip gradient, seed heads). The result is an egui mesh, cached by a hash of the
//!   settings, so a slider drag shows the change on the next frame (a tuft costs ~0.1 ms; at
//!   most [`TUFTS_PER_FRAME`] are built per frame, the rest show a placeholder for a frame).
//! - **Trees** use the tree's impostor (baked on the tree store's worker thread from the real
//!   recipe), lit on the CPU into a thumbnail once per built version.
//! - **Detail meshes and prefabs** are rendered offscreen by the engine renderer after the UI
//!   pass, at most [`MESH_THUMBS_PER_FRAME`] per frame, keyed by what they show (mesh, material
//!   and its file version, prefab file version), so a change renders a new thumbnail.
//! - **Texture layers** show their albedo (the Project window's thumbnails) with a swatch of its
//!   mean colour.
//!
//! Counts shown on palette badges (blades, meshes, trees, layer coverage) are cached per
//! terrain data version and refreshed at most every [`COUNTS_EVERY`].

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use egui::{Color32, Pos2, Rect, Shape, Vec2 as EVec2};
use glam::{Vec2, Vec3};
use misty_engine::render::{MeshDraw, RenderTarget};
use misty_engine::terrain::TerrainData;
use misty_engine::terrain::detail_layers::{DetailKind, DetailLayer};
use misty_engine::terrain::details::DetailSettings;
use misty_engine::wgpu;

/// Grass tufts built per frame at most.
pub const TUFTS_PER_FRAME: u32 = 4;
/// Tree thumbnails lit per frame at most.
const TREES_PER_FRAME: u32 = 1;
/// Offscreen mesh / prefab thumbnails rendered per frame at most.
pub const MESH_THUMBS_PER_FRAME: usize = 3;
/// Mesh / prefab thumbnail size in pixels.
pub const MESH_THUMB_SIZE: u32 = 128;
/// Badge counts are recomputed at most this often while the terrain changes.
pub const COUNTS_EVERY: Duration = Duration::from_millis(300);
/// Cached tufts / thumbnails kept at most (the least recently used go first).
const KEEP: usize = 96;

/// What a grass tuft preview shows (sRGB colours, as the settings store them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuftParams {
    pub height: [f32; 2],
    pub width: f32,
    pub bend: f32,
    pub dryness: f32,
    pub tint: f32,
    pub seed_heads: f32,
    pub healthy: [f32; 3],
    pub dry: [f32; 3],
    pub seed: [f32; 3],
    pub tip_dryness: f32,
    pub color_variation: f32,
    /// Blades in the tuft and the radius of the ground they grow on (metres).
    pub blades: u32,
    pub spread: f32,
}

impl TuftParams {
    /// A grass species (its colour is between the terrain grass's healthy and dry colours).
    pub fn species(l: &DetailLayer, s: &DetailSettings) -> Self {
        Self {
            height: l.height,
            width: l.width,
            bend: l.bend,
            dryness: l.dryness,
            tint: l.tint,
            seed_heads: l.seed_heads,
            ..Self::terrain_grass(s)
        }
    }

    /// The terrain's own grass (its dry mix comes from patches; a typical value is shown).
    pub fn terrain_grass(s: &DetailSettings) -> Self {
        Self {
            height: s.height,
            width: s.width,
            bend: s.bend,
            dryness: 0.25,
            tint: 0.0,
            seed_heads: s.seed_heads,
            healthy: s.healthy_color,
            dry: s.dry_color,
            seed: s.seed_color,
            tip_dryness: s.tip_dryness,
            color_variation: s.color_variation,
            blades: 44,
            spread: 0.0,
        }
    }

    /// The same grass as a wider patch (the settings panel's large preview).
    pub fn patch(self) -> Self {
        Self {
            blades: 110,
            spread: 1.0,
            ..self
        }
    }

    pub fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let floats = [
            self.height[0],
            self.height[1],
            self.width,
            self.bend,
            self.dryness,
            self.tint,
            self.seed_heads,
            self.healthy[0],
            self.healthy[1],
            self.healthy[2],
            self.dry[0],
            self.dry[1],
            self.dry[2],
            self.seed[0],
            self.seed[1],
            self.seed[2],
            self.tip_dryness,
            self.color_variation,
            self.spread,
        ];
        for f in floats {
            f.to_bits().hash(&mut h);
        }
        self.blades.hash(&mut h);
        h.finish()
    }
}

/// A built tuft: triangles in metres (x right, y up, the ground at 0) with their colours.
#[derive(Clone, Debug, Default)]
pub struct Tuft {
    pub vertices: Vec<(Vec2, Color32)>,
    pub indices: Vec<u32>,
    /// Height of the view (metres) the tuft is framed in, and its half width.
    pub view_height: f32,
    pub half_width: f32,
}

fn lin(c: f32) -> f32 {
    misty_engine::trees::impostor::lin(c)
}

fn srgb8(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn to_color(c: Vec3) -> Color32 {
    Color32::from_rgb(srgb8(c.x), srgb8(c.y), srgb8(c.z))
}

fn lin3(c: [f32; 3]) -> Vec3 {
    Vec3::new(lin(c[0]), lin(c[1]), lin(c[2]))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// Segments along a blade (the grass shader's near blades use ten vertices: four segments;
/// the preview is seen close up, so it uses a smoother arc).
const SEGMENTS: usize = 7;
/// Camera pitch (looking down a little), radians.
const PITCH: f32 = 0.24;

/// Build a tuft from `p` with the terrain's blade recipe and the grass shader's shape and
/// colour model.
pub fn build_tuft(p: &TuftParams) -> Tuft {
    use misty_engine::terrain::noise::hash;
    let healthy = lin3(p.healthy);
    let dry = lin3(p.dry);
    let seed_col = lin3(p.seed);
    let (h0, h1) = (
        p.height[0].max(0.01),
        p.height[1].max(p.height[0]).max(0.01),
    );
    let spread = if p.spread > 0.0 {
        p.spread
    } else {
        0.04 + h1 * 0.22
    };
    let light = Vec3::new(-0.45, 0.75, -0.5).normalize();
    let (cp, sp) = (PITCH.cos(), PITCH.sin());
    // Screen position and depth of a point (camera looking along +z, pitched down).
    let project = |v: Vec3| (Vec2::new(v.x, v.y * cp + v.z * sp), v.z * cp - v.y * sp);

    struct Strip {
        depth: f32,
        verts: Vec<(Vec2, Color32)>,
        indices: Vec<u32>,
    }
    let mut strips: Vec<Strip> = Vec::new();
    let mut top = h1;
    let n = p.blades.max(1);
    for i in 0..n {
        let r = |salt: u32| hash(i as i32, 17, 0x7f4a_0000u32.wrapping_add(salt * 7919));
        // Roots spread over a disc, denser in the middle.
        let ang = r(1) * std::f32::consts::TAU;
        let rad = spread * r(2).sqrt();
        let root = Vec3::new(ang.cos() * rad, 0.0, ang.sin() * rad);
        // Blade recipe (detail_layers::species_blades, with a mid canopy relief).
        let relief = 0.5;
        let mut tall = h0 + (h1 - h0) * (0.55 * r(3) + 0.45 * relief);
        let seed_head = r(9) < p.seed_heads.clamp(0.0, 1.0);
        let mut width = p.width.max(0.002) * (0.7 + 0.6 * r(5));
        if seed_head {
            tall *= 1.2 + 0.4 * r(10);
            width = width.min(0.02) * 0.3;
        }
        top = top.max(tall);
        let bend =
            p.bend.clamp(0.0, 1.5) * (0.35 + 0.65 * r(12)) * if seed_head { 0.35 } else { 1.0 };
        let dry_mix = (p.dryness + (r(6) - 0.5) * 0.25).clamp(0.0, 1.0);
        let tint = (p.tint + (r(14) - 0.5) * 0.3).clamp(-1.0, 1.0);
        let lean_angle = r(8) * std::f32::consts::TAU;
        let phase = r(7);
        let facing_x = r(4) * std::f32::consts::TAU;

        // Colour (grass.wgsl): healthy / dry mix, per-tuft hue and value, the seed colour.
        let mut base = healthy.lerp(dry, dry_mix);
        let cv = p.color_variation;
        let hue = (fract(phase * 13.7) - 0.5) * 0.3 + tint;
        let value = (fract(phase * 7.31 + 0.37) - 0.5) * 0.2 + tint * 0.5;
        base *= Vec3::new(
            1.0 + 0.22 * hue * cv,
            1.0 + 0.02 * hue * cv,
            1.0 - 0.32 * hue * cv,
        ) * (1.0 + 0.4 * value * cv).max(0.3);
        let seed_color = seed_col * (1.0 + 0.12 * tint);

        // Shape (grass_blade.wgsl): lean out of the tuft along a circular arc.
        let lean = Vec2::new(lean_angle.cos(), lean_angle.sin()) * bend;
        let amount = lean.length();
        let theta = (amount * 2.0).min(1.5);
        let dir = if amount > 1e-5 {
            lean / amount
        } else {
            Vec2::X
        };
        let bend3 = Vec3::new(dir.x, 0.0, dir.y);
        let facing = if seed_head {
            facing_x
        } else {
            lean_angle + std::f32::consts::FRAC_PI_2 + (facing_x - std::f32::consts::PI) * 0.25
        };
        let arc = |t: f32| {
            let a = theta * t;
            let (along, up) = if theta > 1e-3 {
                ((1.0 - a.cos()) / theta, a.sin() / theta)
            } else {
                (0.5 * theta * t * t, t)
            };
            let centre = root + (bend3 * along + Vec3::Y * up) * tall;
            let tangent = (bend3 * a.sin() + Vec3::Y * a.cos()).normalize();
            (centre, tangent)
        };
        let mut strip = Strip {
            depth: project(root).1,
            verts: Vec::with_capacity((SEGMENTS + 1) * 2),
            indices: Vec::with_capacity(SEGMENTS * 6),
        };
        for k in 0..=SEGMENTS {
            let t = k as f32 / SEGMENTS as f32;
            let (centre, tangent) = arc(t);
            let mut across = Vec3::new(facing.cos(), 0.0, facing.sin());
            across =
                (across - tangent * across.dot(tangent) + Vec3::new(1e-4, 0.0, 0.0)).normalize();
            if !seed_head {
                let twist = (fract(phase * 5.3) - 0.5) * 0.9 * t;
                across = across * twist.cos() + tangent.cross(across) * twist.sin();
            }
            // The atlas blade narrows to its tip; seed stems stay thin.
            let profile = if seed_head {
                1.0
            } else {
                (1.0 - t).powf(0.75).max(0.04) * (0.85 + 0.3 * (t * 3.0).min(1.0))
            };
            let half = width * 0.5 * profile;
            // Fragment colour: dry tips and a paler tip; seed stems green below the head.
            let mut color = if seed_head {
                healthy
                    .lerp(dry, 0.5)
                    .lerp(seed_color, smoothstep(0.35, 0.6, t))
            } else {
                let dry_tip = smoothstep(0.45, 1.0, t)
                    * p.tip_dryness
                    * (0.45 + 0.55 * smoothstep(0.2, 0.9, fract(phase * 2.37)));
                base.lerp(dry, dry_tip)
                    * Vec3::new(0.82, 0.95, 0.78).lerp(Vec3::new(1.1, 1.08, 1.0), t)
            };
            // Light: two-sided wrap with translucent backs, sky ambient, darker low in the
            // tuft.
            let normal = across.cross(tangent).normalize();
            let d = normal.dot(light);
            let diffuse = if d >= 0.0 { d } else { -d * 0.55 };
            let ao = 0.42 + 0.58 * t.powf(0.6);
            color *= (0.42 + 0.85 * diffuse) * ao * 1.15;
            let c = to_color(color);
            for side in [-1.0, 1.0] {
                let v = centre + across * side * half;
                strip.verts.push((project(v).0, c));
            }
            if k > 0 {
                let b = (k as u32 - 1) * 2;
                strip
                    .indices
                    .extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
            }
        }
        if seed_head {
            // The panicle: spikelets along the top quarter, alternating sides.
            for s in 0..7 {
                let t = 0.72 + 0.27 * s as f32 / 6.0;
                let (centre, tangent) = arc(t);
                let side = if s % 2 == 0 { 1.0 } else { -1.0 };
                let out = Vec3::new(facing.cos(), 0.0, facing.sin()) * side;
                let len = tall * 0.07 * (1.0 - 0.4 * (s as f32 / 6.0));
                let w = tall * 0.018;
                let tip = centre + (out * 0.6 + tangent).normalize() * len;
                let shade = 0.85 + 0.25 * fract(phase * 3.1 + s as f32 * 0.37);
                let c = to_color(seed_color * shade * 1.1);
                let b = strip.verts.len() as u32;
                let across = out.cross(tangent).normalize_or_zero() * w;
                for q in [
                    centre - across,
                    centre + across,
                    tip + across * 0.4,
                    tip - across * 0.4,
                ] {
                    strip.verts.push((project(q).0, c));
                }
                strip
                    .indices
                    .extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        strips.push(strip);
    }
    // Painter's order: far blades first.
    strips.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    let mut out = Tuft {
        view_height: (top * 1.15).max(0.28),
        half_width: spread + top * 0.45,
        ..Default::default()
    };
    // A soft patch of ground under the tuft.
    let ground = to_color(lin3(p.healthy) * 0.22 + Vec3::new(0.03, 0.025, 0.015));
    let gw = spread + top * 0.25;
    let centre = out.vertices.len() as u32;
    out.vertices.push((Vec2::ZERO, ground));
    const RIM: u32 = 24;
    for k in 0..RIM {
        let a = k as f32 / RIM as f32 * std::f32::consts::TAU;
        let v = Vec3::new(a.cos() * gw, 0.0, a.sin() * gw);
        out.vertices.push((project(v).0, Color32::TRANSPARENT));
        out.indices
            .extend_from_slice(&[centre, centre + 1 + k, centre + 1 + (k + 1) % RIM]);
    }
    for s in strips {
        let b = out.vertices.len() as u32;
        out.vertices.extend(s.verts);
        out.indices.extend(s.indices.into_iter().map(|i| i + b));
    }
    out
}

/// Paint a tuft fitted into `rect` (the ground near the bottom). `zoom` > 1 shows it larger.
pub fn paint_tuft(painter: &egui::Painter, rect: Rect, tuft: &Tuft) {
    let scale = (rect.height() * 0.86 / tuft.view_height)
        .min(rect.width() * 0.5 / tuft.half_width.max(0.05));
    let origin = Pos2::new(rect.center().x, rect.bottom() - rect.height() * 0.1);
    let mesh = egui::Mesh {
        vertices: tuft
            .vertices
            .iter()
            .map(|(p, c)| egui::epaint::Vertex {
                pos: origin + EVec2::new(p.x * scale, -p.y * scale),
                uv: egui::epaint::WHITE_UV,
                color: *c,
            })
            .collect(),
        indices: tuft.indices.clone(),
        ..Default::default()
    };
    painter.with_clip_rect(rect).add(Shape::mesh(mesh));
}

/// A thumbnail rendered offscreen: the draws and their bounds.
pub struct MeshJob {
    pub draws: Vec<MeshDraw>,
    pub min: Vec3,
    pub max: Vec3,
}

impl MeshJob {
    /// The draws with their bounds from the meshes' own bounds (unit boxes when unknown).
    pub fn new(draws: Vec<MeshDraw>) -> Option<Self> {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for d in &draws {
            let (lo, hi) = misty_engine::models::bounds(&d.mesh)
                .unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5)));
            for i in 0..8 {
                let c = Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                let p = d.model.transform_point3(c);
                min = min.min(p);
                max = max.max(p);
            }
        }
        (!draws.is_empty() && min.x <= max.x).then_some(Self { draws, min, max })
    }
}

struct MeshThumb {
    job: Option<MeshJob>,
    texture: Option<(wgpu::Texture, egui::TextureId)>,
    used: u64,
}

/// Badge numbers of one terrain, per data version.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    /// Terrain grass blades at full density, then each detail layer's count.
    pub terrain_grass: usize,
    pub details: Vec<usize>,
    /// Trees placed per prototype.
    pub trees: Vec<usize>,
    /// Share of the terrain each texture layer covers (0..1).
    pub layers: Vec<f32>,
}

/// Counters for tests and the timing report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewStats {
    pub tufts_requested: u64,
    pub tufts_built: u64,
    pub trees_built: u64,
    pub meshes_requested: u64,
    pub meshes_rendered: u64,
}

#[derive(Default)]
pub struct TerrainPreviews {
    tufts: HashMap<u64, (Arc<Tuft>, u64)>,
    frame: u64,
    tufts_this_frame: u32,
    trees_this_frame: u32,
    trees: HashMap<String, (u64, Option<egui::TextureHandle>, u64)>,
    swatches: HashMap<PathBuf, (Option<SystemTime>, Option<Color32>)>,
    swatches_this_frame: u32,
    meshes: HashMap<u64, MeshThumb>,
    target: Option<RenderTarget>,
    counts: HashMap<String, (u64, Instant, Arc<Counts>)>,
    /// Textures of thumbnails that were evicted, freed on the next render (needs the GPU).
    freed: Vec<egui::TextureId>,
    pub stats: PreviewStats,
}

impl TerrainPreviews {
    fn begin(&mut self, ctx: &egui::Context) {
        let frame = ctx.cumulative_pass_nr();
        if frame != self.frame {
            self.frame = frame;
            self.tufts_this_frame = 0;
            self.trees_this_frame = 0;
            self.swatches_this_frame = 0;
        }
    }

    /// The tuft for `p`: cached by its settings, built now when the frame's budget allows
    /// (else None for a frame, with a repaint asked for).
    pub fn tuft(&mut self, ctx: &egui::Context, p: &TuftParams) -> Option<Arc<Tuft>> {
        self.begin(ctx);
        self.stats.tufts_requested += 1;
        let key = p.key();
        let frame = self.frame;
        if let Some((t, used)) = self.tufts.get_mut(&key) {
            *used = frame;
            return Some(t.clone());
        }
        if self.tufts_this_frame >= TUFTS_PER_FRAME {
            ctx.request_repaint();
            return None;
        }
        self.tufts_this_frame += 1;
        self.stats.tufts_built += 1;
        let t = Arc::new(build_tuft(p));
        if self.tufts.len() >= KEEP
            && let Some(old) = self
                .tufts
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k)
        {
            self.tufts.remove(&old);
        }
        self.tufts.insert(key, (t.clone(), frame));
        Some(t)
    }

    /// Is the tuft for `p` cached?
    #[cfg(test)]
    pub fn has_tuft(&self, p: &TuftParams) -> bool {
        self.tufts.contains_key(&p.key())
    }

    /// Thumbnail of the `.tree` asset at `rel` (None while the tree store builds it).
    pub fn tree(&mut self, ctx: &egui::Context, rel: &str) -> Option<egui::TextureId> {
        self.begin(ctx);
        let Some(built) = misty_engine::trees::store::get(rel) else {
            ctx.request_repaint_after(Duration::from_millis(150));
            return None;
        };
        let frame = self.frame;
        if let Some((version, tex, used)) = self.trees.get_mut(rel)
            && *version == built.version
        {
            *used = frame;
            return tex.as_ref().map(|t| t.id());
        }
        if self.trees_this_frame >= TREES_PER_FRAME {
            ctx.request_repaint();
            // The older version meanwhile.
            return self
                .trees
                .get(rel)
                .and_then(|(_, t, _)| t.as_ref().map(|t| t.id()));
        }
        self.trees_this_frame += 1;
        self.stats.trees_built += 1;
        let tex = tree_image(&built.model.impostor).map(|img| {
            ctx.load_texture(
                format!("tree thumb {rel}"),
                img,
                egui::TextureOptions::LINEAR,
            )
        });
        let id = tex.as_ref().map(|t| t.id());
        self.trees
            .insert(rel.to_owned(), (built.version, tex, frame));
        id
    }

    /// Mean colour of the image at `abs` (texture layer swatches).
    pub fn swatch(&mut self, ctx: &egui::Context, abs: &Path) -> Option<Color32> {
        self.begin(ctx);
        let modified = std::fs::metadata(abs).and_then(|m| m.modified()).ok();
        if let Some((m, c)) = self.swatches.get(abs)
            && *m == modified
        {
            return *c;
        }
        if self.swatches_this_frame >= 2 {
            ctx.request_repaint();
            return None;
        }
        self.swatches_this_frame += 1;
        let c = misty_engine::image::open(abs).ok().map(|img| {
            let small = img.thumbnail(8, 8).into_rgb8();
            let n = (small.width() * small.height()).max(1) as f32;
            let mut sum = Vec3::ZERO;
            for p in small.pixels() {
                sum += Vec3::new(
                    lin(p[0] as f32 / 255.0),
                    lin(p[1] as f32 / 255.0),
                    lin(p[2] as f32 / 255.0),
                );
            }
            to_color(sum / n)
        });
        self.swatches.insert(abs.to_path_buf(), (modified, c));
        c
    }

    /// Offscreen thumbnail `key` (made from `job` the first time it is asked for). None until
    /// rendered after the UI pass.
    pub fn mesh_thumb(
        &mut self,
        key: u64,
        job: impl FnOnce() -> Option<MeshJob>,
    ) -> Option<egui::TextureId> {
        let frame = self.frame;
        if let Some(t) = self.meshes.get_mut(&key) {
            t.used = frame;
            return t.texture.as_ref().map(|(_, id)| *id);
        }
        self.stats.meshes_requested += 1;
        let job = job();
        if self.meshes.len() >= KEEP
            && let Some(old) = self
                .meshes
                .iter()
                .min_by_key(|(_, t)| t.used)
                .map(|(k, _)| *k)
            && let Some(t) = self.meshes.remove(&old)
            && let Some((_, id)) = t.texture
        {
            self.freed.push(id);
        }
        self.meshes.insert(
            key,
            MeshThumb {
                job,
                texture: None,
                used: frame,
            },
        );
        None
    }

    /// Was thumbnail `key` asked for already?
    pub fn has_mesh_thumb(&self, key: u64) -> bool {
        self.meshes.contains_key(&key)
    }

    /// Some offscreen thumbnail still waits to be rendered.
    pub fn pending(&self) -> bool {
        self.meshes
            .values()
            .any(|t| t.texture.is_none() && t.job.is_some())
    }

    /// Badge numbers of the terrain data at `path` (recomputed when it changed, at most every
    /// [`COUNTS_EVERY`]).
    pub fn counts(&mut self, path: &str, d: &TerrainData, version: u64) -> Arc<Counts> {
        if let Some((v, at, c)) = self.counts.get(path)
            && (*v == version || at.elapsed() < COUNTS_EVERY)
        {
            return c.clone();
        }
        let c = Arc::new(count(d));
        self.counts
            .insert(path.to_owned(), (version, Instant::now(), c.clone()));
        c
    }
}

/// Badge numbers of `d`.
pub fn count(d: &TerrainData) -> Counts {
    let n = d.alphamap_resolution;
    let texel = (d.size.x / n as f32) * (d.size.z / n as f32);
    let terrain_grass = if d.detail_map.len() == n * n {
        let per = d.details.density * texel;
        (d.detail_map.iter().map(|v| *v as f32 / 255.0).sum::<f32>() * per) as usize
    } else {
        0
    };
    let details = (d.detail_layers.iter().enumerate())
        .map(|(i, l)| match l.kind {
            DetailKind::Mesh => misty_engine::terrain::detail_layers::mesh_instances(d, i).len(),
            DetailKind::Grass => {
                let per = l.density * texel;
                (l.map.iter().map(|v| *v as f32 / 255.0).sum::<f32>() * per) as usize
            }
        })
        .collect();
    let mut trees = vec![0; d.trees.prototypes.len()];
    for t in &d.trees.instances {
        if let Some(c) = trees.get_mut(t.prototype as usize) {
            *c += 1;
        }
    }
    // Layer coverage from a sparse grid of texels (about 16k samples).
    let step = (n / 128).max(1);
    let mut sums = vec![0u64; d.layers.len()];
    let mut samples = 0u64;
    for z in (0..n).step_by(step) {
        for x in (0..n).step_by(step) {
            let w = d.layer_weights(z * n + x);
            for (s, v) in sums.iter_mut().zip(w.iter()) {
                *s += *v as u64;
            }
            samples += 1;
        }
    }
    let layers = sums
        .iter()
        .map(|s| *s as f32 / (samples.max(1) * 255) as f32)
        .collect();
    Counts {
        terrain_grass,
        details,
        trees,
        layers,
    }
}

/// The impostor's front view lit like the Scene (sun from the upper left, sky light), box
/// filtered to half size.
fn tree_image(imp: &misty_engine::trees::impostor::Impostor) -> Option<egui::ColorImage> {
    use misty_engine::trees::impostor::CELL;
    if imp.albedo.width() < CELL || imp.albedo.height() < CELL {
        return None;
    }
    let light = Vec3::new(-0.5, 0.65, 0.6).normalize();
    let half = CELL / 2;
    let mut pixels = Vec::with_capacity((half * half) as usize);
    for y in 0..half {
        for x in 0..half {
            let mut rgb = Vec3::ZERO;
            let mut alpha = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                let a = imp.albedo.get_pixel(sx, sy);
                let nm = imp.normal.get_pixel(sx, sy);
                let cov = a[3] as f32 / 255.0;
                if cov <= 0.0 {
                    continue;
                }
                let n = Vec3::new(
                    nm[0] as f32 / 127.5 - 1.0,
                    nm[1] as f32 / 127.5 - 1.0,
                    nm[2] as f32 / 127.5 - 1.0,
                )
                .normalize_or_zero();
                let ao = nm[3] as f32 / 255.0;
                let albedo = Vec3::new(
                    lin(a[0] as f32 / 255.0),
                    lin(a[1] as f32 / 255.0),
                    lin(a[2] as f32 / 255.0),
                );
                let lit = albedo * (0.35 + 0.95 * n.dot(light).max(0.0)) * (0.45 + 0.55 * ao);
                rgb += lit * cov;
                alpha += cov;
            }
            let c = if alpha > 0.0 { rgb / alpha } else { Vec3::ZERO };
            let a = (alpha / 4.0 * 255.0).round() as u8;
            pixels.push(Color32::from_rgba_unmultiplied(
                srgb8(c.x),
                srgb8(c.y),
                srgb8(c.z),
                a,
            ));
        }
    }
    Some(egui::ColorImage::new(
        [half as usize, half as usize],
        pixels,
    ))
}

/// Render the offscreen thumbnails asked for while drawing (after the UI pass).
pub fn render(gpu: &mut crate::viewport::Gpu, p: &mut TerrainPreviews) {
    for id in p.freed.drain(..) {
        gpu.state.renderer.write().free_texture(&id);
    }
    let waiting: Vec<u64> = p
        .meshes
        .iter()
        .filter(|(_, t)| t.texture.is_none() && t.job.is_some())
        .map(|(k, _)| *k)
        .take(MESH_THUMBS_PER_FRAME)
        .collect();
    if waiting.is_empty() {
        return;
    }
    let target = p.target.get_or_insert_with(|| {
        gpu.renderer
            .create_target(&gpu.state.device, MESH_THUMB_SIZE, MESH_THUMB_SIZE)
    });
    for key in waiting {
        let Some(t) = p.meshes.get_mut(&key) else {
            continue;
        };
        let Some(job) = t.job.take() else {
            continue;
        };
        draw_job(gpu, target, &job);
        let device = &gpu.state.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty terrain palette thumbnail"),
            size: wgpu::Extent3d {
                width: MESH_THUMB_SIZE,
                height: MESH_THUMB_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = gpu.state.renderer.write().register_native_texture(
            device,
            &view,
            wgpu::FilterMode::Linear,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        target.copy_output(&mut encoder, &texture);
        gpu.state.queue.submit([encoder.finish()]);
        t.texture = Some((texture, id));
        p.stats.meshes_rendered += 1;
    }
}

/// The job's meshes seen from the front, a little from above and the side, framed to fit.
fn draw_job(gpu: &mut crate::viewport::Gpu, target: &RenderTarget, job: &MeshJob) {
    use misty_engine::camera::{Camera, Projection};
    use misty_engine::glam::{EulerRot, Quat};
    use misty_engine::render::{Environment, Frame, LightData};
    use misty_engine::scene::{LightType, Shadows};
    let centre = (job.min + job.max) * 0.5;
    let radius = ((job.max - job.min).length() * 0.5).max(0.05);
    let fov: f32 = 30.0;
    let rotation = Quat::from_euler(EulerRot::YXZ, -0.6, 0.42, 0.0);
    let forward = rotation * Vec3::Z;
    let dist = radius / (fov.to_radians() * 0.5).tan() * 1.08;
    let camera = Camera {
        position: centre - forward * dist,
        rotation,
        projection: Projection::Perspective { fov_y: fov },
        near: (dist - radius * 2.0).max(0.01),
        far: (dist + radius * 2.0).max(100.0),
    };
    let light = |direction: Vec3, intensity| LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    };
    let lights = vec![
        light(Vec3::new(0.5, -0.7, 0.6), 1.1),
        light(Vec3::new(-0.8, 0.2, 0.3), 0.35),
    ];
    let environment = Environment::new(Default::default(), &lights);
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: false,
        background: [0.17, 0.18, 0.19],
        environment: &environment,
        post: Default::default(),
        lines: &[],
        overlay: &[],
        meshes: &job.draws,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let rs = &gpu.state;
    gpu.renderer.render(&rs.device, &rs.queue, target, &frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> TuftParams {
        TuftParams::species(&DetailLayer::grass("G"), &DetailSettings::default())
    }

    #[test]
    fn tufts_follow_the_settings() {
        let p = params();
        let t = build_tuft(&p);
        assert!(t.indices.len() > 36 * SEGMENTS * 6, "{}", t.indices.len());
        let top = |t: &Tuft| t.vertices.iter().map(|(v, _)| v.y).fold(0.0, f32::max);
        // Taller grass reaches higher.
        let tall = build_tuft(&TuftParams {
            height: [1.0, 1.4],
            ..p
        });
        assert!(top(&tall) > top(&t) * 1.8, "{} vs {}", top(&tall), top(&t));
        // Drier grass is yellower (more red).
        let red = |t: &Tuft| t.vertices.iter().map(|(_, c)| c.r() as u32).sum::<u32>();
        let dry = build_tuft(&TuftParams { dryness: 1.0, ..p });
        assert!(red(&dry) > red(&t));
        // Seed heads add panicles.
        let seeds = build_tuft(&TuftParams {
            seed_heads: 1.0,
            ..p
        });
        assert!(seeds.indices.len() > t.indices.len());
        // Keys differ with any setting.
        assert_ne!(p.key(), TuftParams { bend: 0.9, ..p }.key());
        assert_eq!(p.key(), params().key());
    }

    #[test]
    fn tufts_are_cached_and_built_within_the_frame_budget() {
        let ctx = egui::Context::default();
        let mut previews = TerrainPreviews::default();
        let p = params();
        assert!(previews.tuft(&ctx, &p).is_some(), "built at once");
        assert_eq!(previews.stats.tufts_built, 1);
        assert!(previews.tuft(&ctx, &p).is_some());
        assert_eq!(previews.stats.tufts_built, 1, "cached");
        // More new tufts than the budget in one frame: the rest wait for the next frame.
        let mut got = 0;
        for i in 0..TUFTS_PER_FRAME + 3 {
            let q = TuftParams {
                width: 0.01 + i as f32 * 0.003,
                ..p
            };
            got += usize::from(previews.tuft(&ctx, &q).is_some());
        }
        assert_eq!(got as u32, TUFTS_PER_FRAME - 1);
    }

    #[test]
    fn counts_cover_layers_trees_and_details() {
        let mut d = TerrainData::new(33, Vec3::splat(32.0), 32);
        d.layers = vec![Default::default(), Default::default()];
        d.fit_splat_maps();
        let mut l = DetailLayer::grass("A");
        l.map = vec![255; 32 * 32];
        l.density = 10.0;
        d.detail_layers.push(l);
        let c = count(&d);
        assert_eq!(c.details.len(), 1);
        assert!((c.details[0] as f32 - 32.0 * 32.0 * 10.0).abs() < 10.0);
        assert_eq!(c.layers.len(), 2);
        assert!(c.layers[0] > 0.99, "{:?}", c.layers);
    }
}
