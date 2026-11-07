//! Wildflowers (Grass & Details ▸ Flowers): up to four flower kinds (poppies, daisies, ...)
//! growing in patches in the grass. Like the reference engine's detail prototypes, each kind
//! names its textures (a per-kind atlas: the flower head seen from above, and the plant from
//! the side) and its size; where they grow comes from soft low-frequency patches (one noise
//! field per kind, so colours gather in their own drifts), a few lone flowers between them,
//! more at the meadow's edges (near trees, where the grass thins out) and fewer on worn
//! paths, times the grass density and an optional painted mask.
//!
//! Flowers are not a renderer of their own: [`scatter`] adds them to the grass chunks as
//! blades of a special kind (terrain/details.rs `Blade`, see [`FLOWER_KEY`]), so they are
//! streamed, culled, drawn into the grass visibility buffer, shaded, shadowed and swayed by
//! the same wind as the grass. Each flower is three cards: two crossed plant cards (stem,
//! leaves and the head from the side) and a head card lying nearly flat at the top of the
//! stem (grass_blade.wgsl).

use glam::Vec2;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::TerrainData;
use super::details::{self, Blade};
use super::noise;

/// Most flower kinds a terrain holds (the columns of the renderer's flower atlas).
pub const MAX_KINDS: usize = 4;
/// Most flowers per square metre in the middle of a patch.
pub const MAX_DENSITY: f32 = 60.0;
/// Flower blades store their drop-out key scaled into `[0, FLOWER_KEY)`: they sort before
/// (nearly) every grass blade of a chunk, so every LOD level and drawn prefix keeps them; the
/// shaders thin flowers with their own, gentler curve (`grass_flower_keep`).
pub const FLOWER_KEY: f32 = 0.004;
/// Cards per flower: two crossed plant cards and the head.
pub const CARDS: u32 = 3;
/// Height of the head on the plant card (share of the plant's height): where the textures
/// draw the side view's head and where the head card sits.
pub const HEAD_AT: f32 = 0.92;

/// One kind of flower the terrain grows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowerKind {
    pub name: String,
    /// Project-relative albedo atlas (RGBA, alpha = cut-out): 1:4, the head seen from above in
    /// the top square, the plant from the side below it, its stem entering at the bottom
    /// centre and its head at [`HEAD_AT`] of the plant's height.
    pub texture: String,
    /// Normal map (OpenGL), roughness and translucency of the same layout (optional).
    pub normal: String,
    pub roughness: String,
    pub translucency: String,
    /// How common this kind is (relative to the others; 0 = none).
    pub amount: f32,
    /// Plant height range (m).
    pub height: [f32; 2],
    /// Diameter of the flower head (m) on a plant of average height.
    pub head_size: f32,
}

impl Default for FlowerKind {
    fn default() -> Self {
        Self {
            name: "Flower".into(),
            texture: String::new(),
            normal: String::new(),
            roughness: String::new(),
            translucency: String::new(),
            amount: 1.0,
            height: [0.25, 0.4],
            head_size: 0.04,
        }
    }
}

impl FlowerKind {
    /// Every texture the kind names (empty slots left out).
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        [
            &self.texture,
            &self.normal,
            &self.roughness,
            &self.translucency,
        ]
        .into_iter()
        .map(String::as_str)
        .filter(|t| !t.is_empty())
    }
}

/// The terrain's flowers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowerSettings {
    pub enabled: bool,
    pub kinds: Vec<FlowerKind>,
    /// Flowers per square metre in the middle of a patch.
    pub density: f32,
    /// Share of the grass covered by flower patches (0..1).
    pub coverage: f32,
    /// Size of the patches (m).
    pub patch_scale: f32,
    /// Lone flowers between the patches (share of the patch density, 0..1).
    pub scattered: f32,
    /// More flowers at the meadow's edges: around trees and where the grass thins out (0..1).
    pub edges: f32,
    /// All flowers drawn up to this distance (m); further away fewer, bigger ones.
    pub lod_distance: f32,
    /// Painted flower mask per splat texel (0..255 of the rules' density); empty = unpainted
    /// (the rules alone).
    #[serde(
        serialize_with = "save_map",
        deserialize_with = "load_map",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub map: Vec<u8>,
}

impl Default for FlowerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kinds: Vec::new(),
            density: 8.0,
            coverage: 0.08,
            patch_scale: 14.0,
            scattered: 0.003,
            edges: 0.5,
            lod_distance: 25.0,
            map: Vec::new(),
        }
    }
}

fn save_map<S: Serializer>(map: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&super::file::encode(map))
}

fn load_map<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(d)?;
    super::file::decode(&text).map_err(serde::de::Error::custom)
}

impl FlowerSettings {
    /// Nothing set (left out of the terrain file).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The four sample wildflowers of `SampleProject/Assets/Flowers` (`dir` = that folder,
    /// project-relative): purple knapweed, red poppies, white oxeye daisies, yellow buttercups.
    pub fn wildflowers(dir: &str) -> Vec<FlowerKind> {
        let kind = |name: &str, file: &str, amount: f32, height: [f32; 2], head: f32| {
            let path = |suffix: &str| format!("{dir}/{file}{suffix}.png");
            FlowerKind {
                name: name.into(),
                texture: path(""),
                normal: path("Normal"),
                roughness: path("Roughness"),
                translucency: path("Translucency"),
                amount,
                height,
                head_size: head,
            }
        };
        vec![
            kind("Knapweed", "Knapweed", 0.8, [0.32, 0.5], 0.055),
            kind("Poppy", "Poppy", 0.9, [0.3, 0.48], 0.095),
            kind("Oxeye Daisy", "Daisy", 1.0, [0.28, 0.46], 0.072),
            kind("Buttercup", "Buttercup", 1.2, [0.22, 0.38], 0.042),
        ]
    }

    pub(super) fn sanitize(&mut self, alphamap_resolution: usize) {
        self.kinds.truncate(MAX_KINDS);
        let a = alphamap_resolution;
        if !self.map.is_empty() && self.map.len() != a * a {
            self.map.clear();
        }
    }
}

/// Quantiles of the patch noise (`noise::fbm` with 3 octaves; its distribution does not depend
/// on the scale or seed): `QUANTILES[i]` is the value that `i / 256` of the field lies below.
fn quantile(share: f32) -> f32 {
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut v: Vec<f32> = (0..160 * 160)
            .map(|i| {
                let (x, z) = ((i % 160) as f32 * 0.37, (i / 160) as f32 * 0.37);
                noise::fbm(x, z, 1.0, 3, 0x51f1 + (i % 7) as u32)
            })
            .collect();
        v.sort_by(f32::total_cmp);
        (0..=256)
            .map(|i| v[(i * (v.len() - 1)) / 256])
            .collect::<Vec<_>>()
    });
    let x = share.clamp(0.0, 1.0) * 256.0;
    let i = (x.floor() as usize).min(255);
    let f = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Patch weight 0..1 of kind `k` at terrain-local `p` (`threshold` from [`quantile`]).
fn patch(s: &FlowerSettings, k: usize, p: Vec2, threshold: f32, seed: u32) -> f32 {
    let scale = s.patch_scale.max(1.0);
    let seed = seed ^ (0x7a31 + k as u32 * 0x1f3d);
    let n = noise::fbm(p.x, p.y, scale, 3, seed);
    let core = smooth(threshold, threshold + 0.14, n);
    if core <= 0.0 {
        return 0.0;
    }
    // Drifts inside a patch: denser clumps and thinner gaps a few metres across.
    let clumps = noise::fbm(p.x, p.y, scale * 0.3, 2, seed ^ 0x55aa);
    core * (0.3 + 0.7 * smooth(-0.45, 0.35, clumps))
}

/// Meadow edge 0..1 at `p`: around tree trunks (a ring from ~2 to ~16 m) and where the grass
/// thins out around the point.
fn edge(d: &TerrainData, p: Vec2, trees: &[(Vec2, f32)], grass: f32) -> f32 {
    let mut near = 0.0f32;
    for &(t, size) in trees {
        let dist = t.distance(p);
        let reach = 6.0 + 10.0 * size;
        near = near.max(smooth(reach, reach * 0.4, dist) * smooth(1.2, 3.0, dist));
    }
    let n = d.alphamap_resolution;
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let at = |q: Vec2| {
        let t = (q / texel).floor();
        if t.x < 0.0 || t.y < 0.0 || t.x >= n as f32 || t.y >= n as f32 {
            return 0.0;
        }
        d.detail_map[t.y as usize * n + t.x as usize] as f32 / 255.0
    };
    let r = 10.0;
    let around = (at(p + Vec2::new(r, 0.0))
        + at(p - Vec2::new(r, 0.0))
        + at(p + Vec2::new(0.0, r))
        + at(p - Vec2::new(0.0, r)))
        * 0.25;
    near.max(smooth(0.15, 0.6, grass - around))
}

/// The flowers rooted in the local rectangle `[lo, hi)` (metres), as blades of the grass
/// chunk there (three per flower), deterministic per seed. Empty while flowers or grass are
/// off, or no kind has a texture.
pub fn scatter(d: &TerrainData, lo: Vec2, hi: Vec2) -> Vec<Blade> {
    let s = &d.flowers;
    let n = d.alphamap_resolution;
    if !s.enabled || !d.details.enabled || d.detail_map.len() != n * n {
        return Vec::new();
    }
    let kinds: Vec<(usize, &FlowerKind)> = s
        .kinds
        .iter()
        .enumerate()
        .take(MAX_KINDS)
        .filter(|(_, k)| k.amount > 0.0 && !k.texture.is_empty())
        .collect();
    let total: f32 = kinds.iter().map(|(_, k)| k.amount).sum();
    if kinds.is_empty() || s.density <= 0.0 || total <= 0.0 {
        return Vec::new();
    }
    let weights: Vec<f32> = kinds.iter().map(|(_, k)| k.amount / total).collect();
    // Each kind covers its share of the patch area.
    let thresholds: Vec<f32> = weights
        .iter()
        .map(|w| quantile(1.0 - (s.coverage.clamp(0.0, 1.0) * w)))
        .collect();
    let seed = d.rule_seed.wrapping_add(0xf10e);
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let (t0, t1) = (
        (lo / texel).floor().max(Vec2::ZERO),
        (hi / texel).ceil().min(Vec2::splat(n as f32)),
    );
    // Trees near the chunk (meadow edges).
    let size = Vec2::new(d.size.x, d.size.z);
    let trees: Vec<(Vec2, f32)> = d
        .trees
        .instances
        .iter()
        .map(|t| (t.position * size, t.height_scale.max(0.2)))
        .filter(|(p, _)| p.cmpge(lo - 20.0).all() && p.cmplt(hi + 20.0).all())
        .collect();
    let edges = s.edges.clamp(0.0, 1.0);
    let scattered = s.scattered.clamp(0.0, 1.0);
    let density = s.density.clamp(0.0, MAX_DENSITY);
    let bound = density * texel.x * texel.y * (1.0 + 2.0 * edges);
    let mut out = Vec::new();
    for tz in t0.y as usize..t1.y as usize {
        for tx in t0.x as usize..t1.x as usize {
            let grass = d.detail_map[tz * n + tx] as f32 / 255.0;
            let painted = s.map.get(tz * n + tx).map_or(1.0, |m| *m as f32 / 255.0);
            let local = grass * painted;
            if local <= 0.0 {
                continue;
            }
            let want = bound * local;
            let (ix, iz) = (tx as i32, tz as i32);
            let count =
                want.floor() as u32 + u32::from(noise::hash(ix, iz, seed ^ 0x3b) < want.fract());
            for c in 0..count {
                let r = |salt: u32| noise::hash(ix, iz, seed.wrapping_add(c * 131 + salt));
                let p = Vec2::new(tx as f32 + r(1), tz as f32 + r(2)) * texel;
                if p.x < lo.x || p.y < lo.y || p.x >= hi.x || p.y >= hi.y {
                    continue;
                }
                let masks: Vec<f32> = kinds
                    .iter()
                    .zip(&thresholds)
                    .map(|((k, _), &t)| patch(s, *k, p, t, seed))
                    .collect();
                // Full density in the middle of any kind's patch (the weights set how much
                // ground each kind covers, not how dense its patches are).
                let in_patch = masks.iter().cloned().fold(0.0, f32::max);
                let e = if edges > 0.0 {
                    edge(d, p, &trees, grass)
                } else {
                    0.0
                };
                // Worn paths and bare spots keep few flowers.
                let accept = (in_patch + scattered).min(1.0) * (1.0 + 2.0 * edges * e)
                    / (1.0 + 2.0 * edges)
                    * (1.0 - 0.9 * details::worn(p));
                if r(3) >= accept {
                    continue;
                }
                // The kind: by its patch here, plus its share of the lone flowers.
                let pick: Vec<f32> = masks
                    .iter()
                    .zip(&weights)
                    .map(|(m, w)| w * (m + scattered))
                    .collect();
                let sum: f32 = pick.iter().sum();
                let mut x = r(4) * sum;
                let mut which = pick.len() - 1;
                for (i, v) in pick.iter().enumerate() {
                    if x < *v {
                        which = i;
                        break;
                    }
                    x -= v;
                }
                let (slot, kind) = kinds[which];
                out.extend(flower(d, kind, slot, p, &r));
            }
        }
    }
    out
}

/// The three blades of one flower of `kind` (atlas column `slot`) at `p`.
fn flower(
    d: &TerrainData,
    kind: &FlowerKind,
    slot: usize,
    p: Vec2,
    r: &dyn Fn(u32) -> f32,
) -> [Blade; CARDS as usize] {
    let [h0, h1] = kind.height;
    let (h0, h1) = (h0.max(0.02), h1.max(h0.max(0.02)));
    let mean = 0.5 * (h0 + h1);
    let tall = h0 + (h1 - h0) * r(5);
    let head = kind.head_size.max(0.005) * (tall / mean).sqrt() * (0.85 + 0.3 * r(6));
    let key = r(7) * FLOWER_KEY;
    let phase = r(8);
    // Leaning a little (each its own way), heads tilted.
    let bend = 0.05 + 0.2 * r(9);
    let lean = r(10) * std::f32::consts::TAU;
    let tint = (r(11) - 0.5) * 2.0;
    let facing = r(12) * std::f32::consts::TAU;
    let (lush, dry) = details::patches(p, d.details.patch_scale);
    let sw = details::swathe(p, d.details.patch_scale);
    let (relief, slope) = details::canopy_slope(p);
    let y = d.sample_height(p.x, p.y);
    let blade = |part: u32, width: f32, angle: f32| Blade {
        position: [p.x, y, p.y, tall],
        shape: [
            angle,
            width,
            2.0 + (slot as u32 * CARDS + part) as f32,
            phase,
        ],
        extra: [2.0 + key, bend, lean, tint],
        field: [lush, dry, sw, relief],
        slope: slope.into(),
    };
    // The plant region of the atlas is a third as wide as it is tall.
    let card = tall / 3.0;
    [
        blade(0, card, facing),
        blade(1, head, r(13) * std::f32::consts::TAU),
        blade(2, card, facing + std::f32::consts::FRAC_PI_2),
    ]
}

/// Paint the flower mask under the brush towards full (`erase`: towards none) by `k`.
pub fn paint(d: &mut TerrainData, b: &super::brush::Brush, k: f32, erase: bool) {
    let n = d.alphamap_resolution;
    if d.flowers.map.len() != n * n {
        d.flowers.map = vec![255; n * n];
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    let target = if erase { 0.0 } else { 255.0 };
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let a = (k * b.weight((Vec2::new(tx as f32, tz as f32) + 0.5) * texel)).min(1.0);
            if a > 0.0 {
                let v = &mut d.flowers.map[tz * n + tx];
                let cur = *v as f32;
                // At least one step, so a slow brush still gets there.
                let step = (target - cur) * a;
                let step = if step.abs() < 1.0 && cur != target {
                    step.signum()
                } else {
                    step
                };
                *v = (cur + step).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Which flower card a blade is: `(kind slot, part)` with part 0 / 2 = the crossed plant
/// cards, 1 = the head; None for grass.
pub fn card_of(b: &Blade) -> Option<(u32, u32)> {
    (b.shape[2] >= 1.5).then(|| {
        let i = (b.shape[2] - 2.0).round().max(0.0) as u32;
        (i / CARDS, i % CARDS)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn meadow() -> TerrainData {
        let mut d = TerrainData::new(129, Vec3::new(256.0, 40.0, 256.0), 128);
        d.details.enabled = true;
        d.details.rules.breakup = 0.0;
        details::generate(&mut d);
        d.flowers.enabled = true;
        d.flowers.kinds = FlowerSettings::wildflowers("Assets/Flowers");
        d
    }

    fn count_flowers(blades: &[Blade]) -> usize {
        blades
            .iter()
            .filter(|b| card_of(b).is_some_and(|(_, part)| part == 1))
            .count()
    }

    #[test]
    fn flowers_grow_in_patches_of_every_kind() {
        let d = meadow();
        let all = scatter(&d, Vec2::ZERO, Vec2::splat(256.0));
        let flowers = count_flowers(&all);
        assert_eq!(all.len(), flowers * CARDS as usize);
        // About coverage × density per m² over the field (patch edges and clumps thin it).
        let area = 256.0 * 256.0;
        let per_m2 = flowers as f32 / area;
        let s = &d.flowers;
        assert!(
            per_m2 > s.density * s.coverage * 0.15 && per_m2 < s.density * s.coverage * 1.5,
            "{per_m2} flowers per m²"
        );
        // Every kind grows, each mostly in its own patches.
        let mut by_kind = [0usize; MAX_KINDS];
        for b in &all {
            let (k, part) = card_of(b).unwrap();
            if part == 1 {
                by_kind[k as usize] += 1;
            }
        }
        assert!(by_kind.iter().all(|&n| n > 50), "{by_kind:?}");
        // Patches: most 8 m cells hold no flower, a few hold many.
        let mut cells = vec![0u32; 32 * 32];
        for b in all.iter().filter(|b| card_of(b).unwrap().1 == 1) {
            let (x, z) = (
                (b.position[0] / 8.0) as usize,
                (b.position[2] / 8.0) as usize,
            );
            cells[z.min(31) * 32 + x.min(31)] += 1;
        }
        let sparse = cells.iter().filter(|&&c| c <= 2).count();
        let busy = cells.iter().filter(|&&c| c >= 20).count();
        assert!(
            sparse > cells.len() / 2,
            "{sparse} cells with a lone flower or none"
        );
        assert!(busy > 5, "{busy} busy cells");
        // The cards of a flower share their root, height, phase and lean (one stem sways).
        for f in all.chunks(CARDS as usize) {
            assert!(f.iter().all(|b| b.position == f[0].position));
            assert!(
                f.iter()
                    .all(|b| b.extra == f[0].extra && b.shape[3] == f[0].shape[3])
            );
            assert!(f.iter().all(|b| b.seed_head() && b.key() < FLOWER_KEY));
        }
    }

    #[test]
    fn flowers_are_deterministic_and_chunks_add_up() {
        let d = meadow();
        let whole = scatter(&d, Vec2::ZERO, Vec2::splat(128.0));
        assert_eq!(whole, scatter(&d, Vec2::ZERO, Vec2::splat(128.0)));
        let parts = scatter(&d, Vec2::ZERO, Vec2::new(64.0, 128.0)).len()
            + scatter(&d, Vec2::new(64.0, 0.0), Vec2::splat(128.0)).len();
        assert_eq!(parts, whole.len());
    }

    #[test]
    fn no_flowers_without_grass_kinds_or_paint() {
        let mut d = meadow();
        let mut off = d.clone();
        off.flowers.enabled = false;
        assert!(scatter(&off, Vec2::ZERO, Vec2::splat(256.0)).is_empty());
        let mut no_kinds = d.clone();
        no_kinds.flowers.kinds.clear();
        assert!(scatter(&no_kinds, Vec2::ZERO, Vec2::splat(256.0)).is_empty());
        let n = d.alphamap_resolution;
        // Painted out on the left half.
        d.flowers.map = (0..n * n)
            .map(|i| if i % n < n / 2 { 0 } else { 255 })
            .collect();
        let blades = scatter(&d, Vec2::ZERO, Vec2::splat(256.0));
        assert!(!blades.is_empty());
        assert!(blades.iter().all(|b| b.position[0] >= 127.0));
    }

    #[test]
    fn more_flowers_near_trees() {
        let mut d = meadow();
        d.flowers.scattered = 0.05;
        d.flowers.coverage = 0.0;
        let base = count_flowers(&scatter(&d, Vec2::splat(100.0), Vec2::splat(140.0)));
        d.trees.instances.push(crate::terrain::trees::TreeInstance {
            position: Vec2::splat(120.0 / 256.0),
            ..Default::default()
        });
        let near = count_flowers(&scatter(&d, Vec2::splat(100.0), Vec2::splat(140.0)));
        assert!(near as f32 > base as f32 * 1.2, "{base} → {near}");
    }

    #[test]
    fn the_brush_paints_and_thins_the_flower_mask() {
        let mut d = meadow();
        let b = crate::terrain::brush::Brush {
            center: Vec2::splat(128.0),
            size: 40.0,
            strength: 1.0,
            falloff: 0.0,
        };
        // Thinning out starts from an unpainted (full) mask.
        paint(&mut d, &b, 1.0, true);
        let n = d.alphamap_resolution;
        let at = |d: &TerrainData, x: f32, z: f32| {
            let t = (Vec2::new(x, z) / (256.0 / n as f32)).floor();
            d.flowers.map[t.y as usize * n + t.x as usize]
        };
        assert_eq!(at(&d, 128.0, 128.0), 0, "erased under the brush");
        assert_eq!(at(&d, 20.0, 20.0), 255, "untouched elsewhere");
        let inside = scatter(&d, Vec2::splat(120.0), Vec2::splat(136.0));
        assert!(inside.is_empty(), "no flowers where painted out");
        for _ in 0..4 {
            paint(&mut d, &b, 1.0, false);
        }
        assert_eq!(at(&d, 128.0, 128.0), 255, "painted back");
    }

    #[test]
    fn flower_settings_round_trip_through_the_terrain_file() {
        let mut d = meadow();
        let n = d.alphamap_resolution;
        d.flowers.map = (0..n * n).map(|i| (i % 251) as u8).collect();
        let back = TerrainData::from_file_str(&d.to_file_string()).unwrap();
        assert_eq!(back.flowers, d.flowers);
        // Terrains without flowers leave them out of the file.
        let plain = TerrainData::default();
        assert!(!plain.to_file_string().contains("\"flowers\""));
    }
}
