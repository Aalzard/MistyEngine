//! Grass and small details: placement rules (the same slope / height / spread / breakup rules
//! as layers, plus an optional layer to grow on), a density map filled by Generate, and the
//! deterministic scatter of grass blades per chunk for the renderer.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::auto_paint::{AutoPaintRules, par_rows, rule_mask, surface_at};
use super::brush::Brush;
use super::data::TerrainData;
use super::noise;

/// Most blades per square metre.
pub const MAX_DENSITY: f32 = 1500.0;
/// Coarsest density level of far chunks (each level keeps half the blades of the one before).
pub const MAX_LEVEL: u32 = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailSettings {
    pub enabled: bool,
    /// Where grass grows (slope 0..`rules.slope[1]` keeps it off cliffs).
    pub rules: AutoPaintRules,
    /// Only where this layer is painted (weighted by it); None = any layer.
    pub on_layer: Option<usize>,
    /// Blades per square metre at full density.
    pub density: f32,
    /// Blades fade out (shrink) towards this distance.
    pub draw_distance: f32,
    /// Blade height range in metres (the turf's shortest and tallest stems; tufts stand a
    /// little above the top, dips a little below the bottom).
    pub height: [f32; 2],
    pub width: f32,
    /// sRGB colours of healthy and dry blades (mixed by a noise field).
    pub healthy_color: [f32; 3],
    pub dry_color: [f32; 3],
    pub wind_strength: f32,
    pub wind_speed: f32,
    /// Share of stems that carry a seed head (0..1).
    pub seed_heads: f32,
    /// sRGB colour of the seed heads.
    pub seed_color: [f32; 3],
    /// How strongly blades gather in tufts that lean outwards (0..1).
    pub clumping: f32,
    /// How far blades curve over (0 = straight up, 1 = strongly arched).
    pub bend: f32,
    /// Sunlight shining through backlit blades (0..1).
    pub translucency: f32,
    /// How much blade tips dry out towards the dry colour (0..1).
    pub tip_dryness: f32,
    /// Full density up to this distance; further away fewer, wider blades are drawn.
    pub lod_distance: f32,
    /// Size in metres of the soft colour / height patches across the field.
    pub patch_scale: f32,
    /// sRGB colour of the lush (dark, saturated) patches.
    pub lush_color: [f32; 3],
    /// How strongly lush patches darken and green the grass (0..1).
    pub lush_amount: f32,
    /// How strongly dry patches turn the grass towards the dry colour (0..1).
    pub dry_amount: f32,
    /// Hue and brightness jitter per tuft and blade (0..1).
    pub color_variation: f32,
    /// Taller, fuller tufts in lush patches and shorter, thinner grass elsewhere (0..1).
    pub height_variation: f32,
    /// How much the blade count follows the patches (0..1).
    pub density_variation: f32,
    /// The ground under grass takes the grass's shaded base colour (0..1), so gaps between
    /// blades read as deep grass rather than soil.
    pub ground_tint: f32,
    /// Blades near the camera cast shadows (into the first cascades; Quality decides how far).
    pub cast_shadows: bool,
    /// Compass direction the wind blows towards (degrees from +Z towards +X), used without a
    /// Wind Zone in the scene.
    pub wind_direction: f32,
    /// Strength of the gust waves rolling across the field (0..1.5).
    pub gust_strength: f32,
    /// Distance between gust waves in metres.
    pub gust_scale: f32,
    /// Fast flutter of single blades (0..1).
    pub turbulence: f32,
    /// 0..1: deep shade in the turf under a high sun: the feet of the stems and the
    /// undergrowth go much darker, the sunlit tips stand out (0 = the soft, shallow canopy
    /// shade). Left out of the file at 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub depth_shading: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for DetailSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rules: AutoPaintRules {
                enabled: true,
                slope: [0.0, 28.0],
                spread: 0.2,
                breakup: 0.6,
                breakup_scale: 8.0,
                ..Default::default()
            },
            on_layer: None,
            density: 6.0,
            draw_distance: 80.0,
            height: [0.35, 0.7],
            width: 0.05,
            healthy_color: [0.36, 0.52, 0.18],
            dry_color: [0.66, 0.62, 0.32],
            wind_strength: 0.35,
            wind_speed: 1.2,
            seed_heads: 0.0,
            seed_color: [0.78, 0.69, 0.45],
            clumping: 0.5,
            bend: 0.35,
            translucency: 0.6,
            tip_dryness: 0.35,
            lod_distance: 20.0,
            patch_scale: 14.0,
            lush_color: [0.2, 0.4, 0.1],
            lush_amount: 0.5,
            dry_amount: 0.4,
            color_variation: 0.5,
            height_variation: 0.5,
            density_variation: 0.3,
            ground_tint: 0.9,
            cast_shadows: true,
            wind_direction: 53.0,
            gust_strength: 0.6,
            gust_scale: 30.0,
            turbulence: 0.5,
            depth_shading: 0.0,
        }
    }
}

/// Density 0..1 of the detail rules at one splat texel.
fn texel_density(d: &TerrainData, tx: usize, tz: usize) -> f32 {
    let s = &d.details;
    let n = d.alphamap_resolution;
    let at = Vec2::new(
        (tx as f32 + 0.5) / n as f32 * d.size.x,
        (tz as f32 + 0.5) / n as f32 * d.size.z,
    );
    let surface = surface_at(d, at.x, at.y);
    // Hard limit: breakup may thin grass but never pushes it past the slope limit + spread.
    let limit =
        s.rules.slope[0].max(s.rules.slope[1]) + 0.5 + s.rules.spread.clamp(0.0, 1.0) * 30.0;
    if surface.0 > limit {
        return 0.0;
    }
    let seed = d.rule_seed.wrapping_add(0xde7a);
    let mut m = rule_mask(&s.rules, d, at, surface, seed);
    if let Some(l) = s.on_layer.filter(|l| *l < d.layers.len()) {
        m *= d.layer_weight(tz * n + tx, l) as f32 / 255.0;
    }
    m
}

/// Fill the detail map from the rules (cleared when details are off).
pub fn generate(d: &mut TerrainData) {
    crate::profile_scope!("Terrain.Details");
    if !d.details.enabled {
        d.detail_map.clear();
        return;
    }
    let n = d.alphamap_resolution;
    let mut out = vec![0u8; n * n];
    par_rows(&mut out, n, |tz, row| {
        for (tx, v) in row.iter_mut().enumerate() {
            *v = (texel_density(d, tx, tz) * 255.0).round() as u8;
        }
    });
    d.detail_map = out;
}

/// Move the detail map under the brush towards the rules by `k` (Generate in brush).
pub fn generate_in_brush(d: &mut TerrainData, b: &Brush, k: f32) {
    if !d.details.enabled {
        return;
    }
    let n = d.alphamap_resolution;
    if d.detail_map.len() != n * n {
        d.detail_map = vec![0; n * n];
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let a = k * b.weight((Vec2::new(tx as f32, tz as f32) + 0.5) * texel);
            if a > 0.0 {
                let cur = d.detail_map[tz * n + tx] as f32;
                let target = texel_density(d, tx, tz) * 255.0;
                d.detail_map[tz * n + tx] = (cur + (target - cur) * a).round() as u8;
            }
        }
    }
}

/// Value noise 0..1 at `p` (lattice of 1 m): the same hash and smoothing as `grass_noise` in
/// grass_field.wgsl, so blade heights follow the colour patches the shaders draw.
fn field_noise(p: Vec2) -> f32 {
    // WGSL fract: x - floor(x), never negative.
    let fract = |x: f32| x - x.floor();
    let hash = |q: Vec2| {
        let q = Vec2::new(fract(q.x * 0.1031), fract(q.y * 0.1030));
        let d = q.dot(Vec2::new(q.y, q.x) + 33.33);
        let r = q + d;
        fract((r.x + r.y) * r.x)
    };
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = hash(i);
    let b = hash(i + Vec2::X);
    let c = hash(i + Vec2::Y);
    let d = hash(i + Vec2::ONE);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    top + (bottom - top) * u.y
}

/// Value noise 0..1 with its gradient per metre: `grass_noise_grad` in grass_field.wgsl
/// (the same hash, and the value in the same form as there).
fn field_noise_grad(p: Vec2) -> (f32, Vec2) {
    let fract = |x: f32| x - x.floor();
    let hash = |q: Vec2| {
        let q = Vec2::new(fract(q.x * 0.1031), fract(q.y * 0.1030));
        let d = q.dot(Vec2::new(q.y, q.x) + 33.33);
        let r = q + d;
        fract((r.x + r.y) * r.x)
    };
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let du = 6.0 * f * (Vec2::ONE - f);
    let a = hash(i);
    let b = hash(i + Vec2::X);
    let c = hash(i + Vec2::Y);
    let d = hash(i + Vec2::ONE);
    let (k1, k2, k3) = (b - a, c - a, a - b - c + d);
    let v = a + k1 * u.x + k2 * u.y + k3 * u.x * u.y;
    (v, Vec2::new(du.x * (k1 + k3 * u.y), du.y * (k2 + k3 * u.x)))
}

/// The canopy relief 0..1 at terrain-local `p` with its slope per metre, as the shaders
/// shade it (`grass_canopy` in grass_field.wgsl; [`canopy`] is the same relief for the blade
/// heights).
pub fn canopy_slope(p: Vec2) -> (f32, Vec2) {
    let (a, ga) = field_noise_grad(p / 4.5 + 7.7);
    let (b, gb) = field_noise_grad(p / 1.9 + 3.1);
    let (c, gc) = field_noise_grad(p / 0.8 + 11.3);
    let v = 0.55 * a + 0.3 * b + 0.15 * c;
    let dv = 0.55 * ga / 4.5 + 0.3 * gb / 1.9 + 0.15 * gc / 0.8;
    let t = ((v - 0.25) / 0.55).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t), dv * 6.0 * t * (1.0 - t) / 0.55)
}

/// Lush and dry patch weights 0..1 at terrain-local `p` (see `grass_patches` in
/// grass_field.wgsl, which colours the blades and the ground under them).
pub fn patches(p: Vec2, scale: f32) -> (f32, f32) {
    let q = p / scale.max(0.5);
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Broad sweeps a few patches across, patches, and smaller mottling inside them. The
    // last octave (0..1 × its weight) is read only where it can still move the smoothstep
    // off 0 or 1: the same values with fewer lookups.
    let lush_ab = 0.3 * field_noise(q * 0.3 + 9.4) + 0.45 * field_noise(q);
    let lush = if lush_ab >= 0.66 {
        1.0
    } else if lush_ab + 0.25 > 0.5 {
        smooth(0.5, 0.66, lush_ab + 0.25 * field_noise(q * 2.3 + 17.1))
    } else {
        0.0
    };
    let dry_ab = 0.3 * field_noise(q * 0.25 + 23.8) + 0.4 * field_noise(q * 0.8 + 41.7);
    let dry = if dry_ab >= 0.68 {
        1.0
    } else if dry_ab + 0.3 > 0.52 {
        smooth(0.52, 0.68, dry_ab + 0.3 * field_noise(q * 1.9 + 5.3))
    } else {
        0.0
    };
    (lush, dry)
}

/// Broad swathes -1..1 at terrain-local `p` for patches of `scale` metres (see
/// `grass_swathe` in grass_field.wgsl, which makes them lighter and more golden or deeper
/// green): bright swathes grow a little shorter, dark ones taller.
pub fn swathe(p: Vec2, scale: f32) -> f32 {
    let q = p / (scale * 1.5).max(1.0);
    let v = 0.65 * field_noise(q + 61.3) + 0.35 * field_noise(q * 2.1 + 83.9);
    ((v - 0.5) * 3.0).clamp(-1.0, 1.0)
}

/// Worn spots 0..1 at terrain-local `p`: bare dirt patches a metre or two across (see
/// `grass_worn` in grass_field.wgsl, which shows the soil there).
pub fn worn(p: Vec2) -> f32 {
    let a = 0.7 * field_noise(p / 2.6 + 91.7);
    // (The fine octave adds at most 0.3: below the edge without it, the spot stays 0.)
    if a + 0.3 <= 0.78 {
        return 0.0;
    }
    let v = a + 0.3 * field_noise(p / 0.9 + 17.3);
    let t = ((v - 0.78) / 0.1).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Relief of the canopy top 0..1 at terrain-local `p`: tufts and dips a few metres across
/// (the same noise as `grass_canopy` in grass_field.wgsl, which tilts the shading of the
/// canopy to follow it). Blade heights follow it, so the turf rolls instead of standing at
/// one height.
pub fn canopy(p: Vec2) -> f32 {
    let v = 0.55 * field_noise(p / 4.5 + 7.7)
        + 0.3 * field_noise(p / 1.9 + 3.1)
        + 0.15 * field_noise(p / 0.8 + 11.3);
    // Sharpened: distinct crests and hollows rather than a gentle swell.
    let t = ((v - 0.25) / 0.55).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One grass blade for the renderer (terrain-local position).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub struct Blade {
    /// xyz = root position, w = height.
    pub position: [f32; 4],
    /// x = facing angle (radians), y = width, z = dry mix 0..1 (flowers: 2 + their card,
    /// see `flowers::card_of`), w = random phase 0..1.
    pub shape: [f32; 4],
    /// x = drop-out key 0..1 (see [`level_keep`]) + 2 for a seed head stem, y = bend,
    /// z = lean angle (radians), w = tuft tint -1..1.
    pub extra: [f32; 4],
    /// The field at the root, worked out once here instead of in every vertex of every
    /// frame: x = lush and y = dry patch weight ([`patches`]), z = swathe ([`swathe`]),
    /// w = canopy relief ([`canopy_slope`]).
    pub field: [f32; 4],
    /// The canopy relief's slope per metre at the root (xz).
    pub slope: [f32; 2],
}

impl Blade {
    /// The stem carries a seed head (flowers are stems too).
    pub fn seed_head(&self) -> bool {
        self.extra[0] >= 2.0
    }

    /// One of a wildflower's cards (terrain/flowers.rs).
    pub fn flower(&self) -> bool {
        self.shape[2] >= 1.5
    }

    /// Drop-out key: the blade is drawn while the share kept at its distance is above it.
    pub fn key(&self) -> f32 {
        self.extra[0] % 2.0
    }
}

/// Share of blades kept at density `level`: a blade stays when its drop-out key is below
/// `0.5^level` (the renderer also draws prefixes of key-sorted chunks for finer thinning).
pub fn level_keep(level: u32) -> f32 {
    0.5f32.powi(level.min(MAX_LEVEL) as i32)
}

/// Tuft (clump) nearest to `p`: its centre and a random id 0..1 (jittered grid of `cell` m).
fn tuft(p: Vec2, cell: f32, seed: u32) -> (Vec2, f32) {
    let g = (p / cell).floor();
    let mut best = (f32::MAX, Vec2::ZERO, 0.0);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (ix, iz) = (g.x as i32 + dx, g.y as i32 + dz);
            let jitter = Vec2::new(
                noise::hash(ix, iz, seed),
                noise::hash(ix, iz, seed ^ 0x9e37),
            );
            let c = (Vec2::new(ix as f32, iz as f32) + jitter) * cell;
            let d = c.distance_squared(p);
            if d < best.0 {
                best = (d, c, noise::hash(ix, iz, seed ^ 0x3c6e));
            }
        }
    }
    (best.1, best.2)
}

/// Blades rooted in the local rectangle `[lo, hi)` (metres), deterministic per seed.
pub fn scatter(d: &TerrainData, lo: Vec2, hi: Vec2) -> Vec<Blade> {
    scatter_level(d, lo, hi, 0)
}

/// Like [`scatter`] but only the blades kept at density `level` (see [`level_keep`]),
/// sorted by drop-out key so any prefix of the result is an even thinning.
pub fn scatter_level(d: &TerrainData, lo: Vec2, hi: Vec2, level: u32) -> Vec<Blade> {
    let s = &d.details;
    let n = d.alphamap_resolution;
    if !super::detail_layers::draws_grass(d) {
        return Vec::new();
    }
    let keep = level_keep(level);
    // Grass species painted on the terrain (terrain/detail_layers.rs).
    let mut out: Vec<(f32, Blade)> = super::detail_layers::species_blades(d, lo, hi, keep);
    if d.detail_map.len() != n * n {
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        return out.into_iter().map(|(_, b)| b).collect();
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let per_texel = s.density.clamp(0.0, MAX_DENSITY) * texel.x * texel.y;
    let (t0, t1) = (
        (lo / texel).floor().max(Vec2::ZERO),
        (hi / texel).ceil().min(Vec2::splat(n as f32)),
    );
    let seed = d.rule_seed;
    let holes = d.has_holes();
    let clumping = s.clumping.clamp(0.0, 1.0);
    let hv = s.height_variation.clamp(0.0, 1.0);
    let dv = s.density_variation.clamp(0.0, 1.0);
    // Tufts a little closer together than the tallest blades.
    let cell = (s.height[1].max(0.05) * 0.8).max(0.25);
    for tz in t0.y as usize..t1.y as usize {
        for tx in t0.x as usize..t1.x as usize {
            let density = d.detail_map[tz * n + tx];
            if density == 0 {
                continue;
            }
            // Expected count; the fraction becomes one more blade with that probability.
            // Fuller grass in lush patches and in the taller areas, thinner elsewhere.
            let centre = (Vec2::new(tx as f32, tz as f32) + 0.5) * texel;
            let (lush, dry) = patches(centre, s.patch_scale);
            let area =
                noise::fbm(centre.x, centre.y, s.patch_scale * 0.7, 2, seed ^ 0x91) * 0.5 + 0.5;
            // Worn spots keep a third of the blades.
            let fuller = (1.0 + dv * (0.8 * lush + 0.8 * area - 0.3 * dry - 0.65))
                * (1.0 - 0.65 * worn(centre));
            let want = per_texel * density as f32 / 255.0 * fuller.max(0.15);
            let (ix, iz) = (tx as i32, tz as i32);
            let count =
                want.floor() as u32 + u32::from(noise::hash(ix, iz, seed ^ 0x51) < want.fract());
            for k in 0..count {
                let r = |salt: u32| noise::hash(ix, iz, seed.wrapping_add(k * 97 + salt));
                let key = r(11);
                if key >= keep {
                    continue;
                }
                let root = Vec2::new(tx as f32 + r(1), tz as f32 + r(2)) * texel;
                if root.x < lo.x || root.y < lo.y || root.x >= hi.x || root.y >= hi.y {
                    continue;
                }
                // Tufts: blades are pulled towards their tuft's centre and lean away from it
                // (a root pulled out of the rectangle stays put, so chunks still add up).
                let (centre, id) = tuft(root, cell, seed ^ 0x7f4a);
                // Undergrowth stays spread out and fills the ground between the tufts.
                let under = r(17) < 0.68;
                let pull = if under { 0.08 } else { 0.45 };
                let pulled = centre + (root - centre) * (1.0 - pull * clumping);
                let inside = pulled.cmpge(lo).all() && pulled.cmplt(hi).all();
                let p = if inside { pulled } else { root };
                // Nothing grows over a hole.
                if holes && d.hole_at(p.x, p.y) {
                    continue;
                }
                let away = p - centre;
                let lean = if away.length_squared() > 1e-6 {
                    away.y.atan2(away.x)
                } else {
                    r(8) * std::f32::consts::TAU
                };
                let spread = (away.length() / (cell * 0.5)).min(1.0);
                // The canopy top rolls: tufts and dips a few metres across (shared with the
                // shaders, which shade the canopy as that rolling surface), broad areas of
                // taller and shorter grass, every tuft and blade a little different and a
                // few stems standing above the rest.
                let relief = canopy(p);
                let field = noise::fbm(p.x, p.y, 28.0, 2, seed ^ 0x5d) * 0.5 + 0.5;
                // Every blade its own height too, so the top line breaks up; the relief and
                // the tufts' sizes fade out with the height variation (none: one even height).
                let rv = (hv / 0.6).min(1.0);
                // Tufts differ clearly: some short, some big and tall.
                let tuft_scale =
                    1.0 + rv * ((id - 0.5) * 0.9 * clumping + if id > 0.88 { 0.2 } else { 0.0 });
                let mix = (0.4 * r(3) + rv * (0.42 * relief + 0.18 * field) + (1.0 - rv) * 0.3)
                    .clamp(0.0, 1.0);
                let outlier = if r(15) < 0.05 { 1.3 + 0.3 * r(16) } else { 1.0 };
                let mut tall =
                    (s.height[0] + (s.height[1] - s.height[0]) * mix) * tuft_scale * outlier;
                // Areas of taller, fuller tufts (lush patches) and shorter grass (dry ones).
                let (lush, dry_patch) = patches(p, s.patch_scale);
                let area = noise::fbm(p.x, p.y, s.patch_scale * 0.7, 2, seed ^ 0x91) * 0.5 + 0.5;
                let sw = swathe(p, s.patch_scale);
                tall *= (1.0
                    + hv * (0.5 * (lush - 0.35) - 0.35 * dry_patch + 0.6 * (area - 0.5)
                        - 0.25 * sw))
                    .max(0.6);
                // Most blades are undergrowth a little below the tallest ones: they fill the
                // turf between the stems.
                if under {
                    tall *= 0.6 + 0.3 * r(18);
                }
                let dry = (noise::fbm(p.x, p.y, 9.0, 3, seed ^ 0x77) * 0.5 + 0.5).clamp(0.0, 1.0);
                let seed_head = r(9) < s.seed_heads.clamp(0.0, 1.0) * (0.4 + 1.2 * relief);
                // Undergrowth is broader, low and arches over: it carpets the base of the
                // canopy so no ground shows between the thin stems above it.
                let mut width = s.width * (0.7 + 0.6 * r(5)) * if under { 1.5 } else { 1.0 };
                if seed_head {
                    tall *= 1.2 + 0.4 * r(10);
                    width = s.width.min(0.02) * 0.3;
                }
                // Every blade leans its own way (random lean on top of the tuft's).
                let bend = s.bend.clamp(0.0, 1.5)
                    * (0.35 + 0.65 * r(12))
                    * (1.0 + clumping * spread)
                    * if seed_head {
                        0.35
                    } else if under {
                        1.35
                    } else {
                        1.0
                    };
                let tint = (id - 0.5) * 2.0 * clumping + (r(14) - 0.5) * 0.2;
                let (shade_relief, slope) = canopy_slope(p);
                out.push((
                    key,
                    Blade {
                        position: [p.x, d.sample_height(p.x, p.y), p.y, tall],
                        shape: [
                            r(4) * std::f32::consts::TAU,
                            width,
                            // Mostly green: dry blades gather in a few spots (the dry
                            // patches of the field do the rest).
                            ((dry - 0.45) * 1.6 + 0.15 * r(6)).clamp(0.0, 1.0),
                            r(7),
                        ],
                        extra: [
                            key + if seed_head { 2.0 } else { 0.0 },
                            bend,
                            lean + (r(13) - 0.5) * 1.2,
                            tint.clamp(-1.0, 1.0),
                        ],
                        field: [lush, dry_patch, sw, shade_relief],
                        slope: slope.into(),
                    },
                ));
            }
        }
    }
    // Wildflowers ride along as blades of their own kind (their keys sort them first).
    out.extend(
        super::flowers::scatter(d, lo, hi)
            .into_iter()
            .map(|b| (b.key(), b)),
    );
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, b)| b).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// Flat for x < 64 m, a 60° cliff after.
    fn cliff() -> TerrainData {
        let mut d = TerrainData::new(129, Vec3::new(128.0, 120.0, 128.0), 128);
        for iz in 0..129 {
            for ix in 0..129 {
                let rise = (ix as f32 - 64.0).max(0.0) * 3f32.sqrt();
                d.set_height(ix, iz, rise / 120.0);
            }
        }
        d.details.enabled = true;
        d.details.rules.breakup = 0.0;
        d
    }

    #[test]
    fn grass_grows_on_flat_ground_only() {
        let mut d = cliff();
        generate(&mut d);
        let n = d.alphamap_resolution;
        for tz in (0..n).step_by(7) {
            assert_eq!(d.detail_map[tz * n + 20], 255, "dense on the flat");
            assert_eq!(d.detail_map[tz * n + 100], 0, "none on the cliff");
        }
        let blades = scatter(&d, Vec2::ZERO, Vec2::splat(128.0));
        assert!(blades.len() > 10_000, "{}", blades.len());
        for b in &blades {
            // The cliff starts at x = 64 m (texels there straddle its foot).
            assert!(
                b.position[0] < 65.0,
                "blade on the cliff at x = {}",
                b.position[0]
            );
            let ground = d.sample_height(b.position[0], b.position[2]);
            assert!(
                (b.position[1] - ground).abs() < 1e-4,
                "rooted on the ground"
            );
        }
        // Deterministic, and chunks add up to the whole.
        assert_eq!(blades, scatter(&d, Vec2::ZERO, Vec2::splat(128.0)));
        let halves = scatter(&d, Vec2::ZERO, Vec2::new(64.0, 128.0)).len()
            + scatter(&d, Vec2::new(64.0, 0.0), Vec2::splat(128.0)).len();
        assert_eq!(halves, blades.len());
    }

    #[test]
    fn grass_only_on_its_layer() {
        let mut d = cliff();
        d.layers = vec![Default::default(), Default::default()];
        let n = d.alphamap_resolution;
        for tz in 0..n {
            for tx in 0..n / 4 {
                d.alphamap[tz * n + tx] = [0, 255, 0, 0];
            }
        }
        d.details.on_layer = Some(0);
        generate(&mut d);
        assert_eq!(d.detail_map[5 * n + 10], 0, "layer 1 here");
        assert_eq!(d.detail_map[5 * n + 40], 255);
    }

    #[test]
    fn disabled_details_clear_the_map() {
        let mut d = cliff();
        generate(&mut d);
        d.details.enabled = false;
        generate(&mut d);
        assert!(d.detail_map.is_empty());
        assert!(scatter(&d, Vec2::ZERO, Vec2::splat(128.0)).is_empty());
    }
}
