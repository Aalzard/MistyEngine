//! Texture bake: each rock gets its own albedo, normal and mask maps in its octahedral UV
//! layout. Every texel is a point on the dense mesh (rasterised in UV space); its colour,
//! relief, roughness and occlusion come from tileable material sets (`tools/rock_textures.py`)
//! projected triplanar in object space, macro weathering from noise and the mesh's cavities,
//! lichen on exposed faces, moss on top faces and in hollows (blended by height, so it fills
//! crevices first), and a damp, grass-tinted band at the ground line. The normal map is the
//! relief's slope in the mesh's own interpolated tangent frame (exactly what the shader
//! rebuilds on LOD 0).

use std::path::Path;

use glam::{Vec2, Vec3};
use misty_engine::image::{self, RgbImage, RgbaImage};
use misty_engine::render::mesh::MeshData;

use super::shape::{fbm, perlin, ridged};

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A tileable 4-channel map with its box-filtered mip chain.
struct Map {
    levels: Vec<(usize, Vec<[f32; 4]>)>,
}

impl Map {
    fn new(size: usize, texels: Vec<[f32; 4]>) -> Self {
        let mut levels = vec![(size, texels)];
        while levels.last().is_some_and(|(s, _)| *s > 1) {
            let (s, src) = levels.last().unwrap();
            let h = s / 2;
            let next: Vec<[f32; 4]> = (0..h * h)
                .map(|i| {
                    let (x, y) = (i % h * 2, i / h * 2);
                    let mut o = [0.0; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let t = src[(y + dy) * s + x + dx];
                        for c in 0..4 {
                            o[c] += t[c] * 0.25;
                        }
                    }
                    o
                })
                .collect();
            levels.push((h, next));
        }
        Self { levels }
    }

    fn bilinear(&self, level: usize, u: f32, v: f32) -> [f32; 4] {
        let (s, t) = &self.levels[level.min(self.levels.len() - 1)];
        let n = *s as i64;
        let (x, y) = (u * *s as f32 - 0.5, v * *s as f32 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |a: i64, b: i64| t[(b.rem_euclid(n) * n + a.rem_euclid(n)) as usize];
        let (a, b) = (x0 as i64, y0 as i64);
        let (p, q, r, w) = (at(a, b), at(a + 1, b), at(a, b + 1), at(a + 1, b + 1));
        std::array::from_fn(|c| {
            let top = p[c] + (q[c] - p[c]) * fx;
            let bottom = r[c] + (w[c] - r[c]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// Trilinear sample at (u, v) in tiles, `lod` in mip levels.
    fn sample(&self, u: f32, v: f32, lod: f32) -> [f32; 4] {
        let lod = lod.clamp(0.0, (self.levels.len() - 1) as f32);
        let l0 = lod.floor() as usize;
        let f = lod - l0 as f32;
        let a = self.bilinear(l0, u, v);
        if f < 1e-3 {
            return a;
        }
        let b = self.bilinear(l0 + 1, u, v);
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * f)
    }
}

/// One tileable material: (albedo linear rgb, height) and (roughness, AO, alpha, 0).
pub struct Set {
    size: usize,
    /// Metres covered by one tile.
    tile: f32,
    colour: Map,
    surface: Map,
}

impl Set {
    /// `<dir>/<name>_{Albedo,Height,Roughness,AO}.png`.
    pub fn load(dir: &Path, name: &str, tile: f32) -> Result<Self, String> {
        let open = |map: &str| {
            let path = dir.join(format!("{name}_{map}.png"));
            image::open(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        let albedo = open("Albedo")?.to_rgba8();
        let height = open("Height")?.to_luma16();
        let rough = open("Roughness")?.to_luma8();
        let ao = open("AO")?.to_luma8();
        let size = albedo.width() as usize;
        let colour = albedo
            .pixels()
            .zip(height.pixels())
            .map(|(a, h)| {
                [
                    srgb_to_linear(a[0] as f32 / 255.0),
                    srgb_to_linear(a[1] as f32 / 255.0),
                    srgb_to_linear(a[2] as f32 / 255.0),
                    h[0] as f32 / 65535.0,
                ]
            })
            .collect();
        let surface = rough
            .pixels()
            .zip(ao.pixels())
            .zip(albedo.pixels())
            .map(|((r, o), a)| {
                [
                    r[0] as f32 / 255.0,
                    o[0] as f32 / 255.0,
                    a[3] as f32 / 255.0,
                    0.0,
                ]
            })
            .collect();
        Ok(Self {
            size,
            tile,
            colour: Map::new(size, colour),
            surface: Map::new(size, surface),
        })
    }

    /// Triplanar sample at object point `p` with blend weights `w`; `texel` = metres per
    /// output texel (picks the mip level).
    fn triplanar(&self, p: Vec3, w: Vec3, texel: f32, salt: f32) -> ([f32; 4], [f32; 4]) {
        let lod = (texel / (self.tile / self.size as f32)).max(1.0).log2();
        let q = p / self.tile;
        // Image rows run down the sides (image up = world up), each plane shifted.
        let planes = [
            (w.x, Vec2::new(q.z + 0.37 * salt, -q.y)),
            (w.y, Vec2::new(q.x + 0.61, q.z + 0.13 * salt)),
            (w.z, Vec2::new(q.x + 0.29 * salt, -q.y + 0.47)),
        ];
        let mut c = [0.0; 4];
        let mut s = [0.0; 4];
        for (weight, uv) in planes {
            if weight < 1e-3 {
                continue;
            }
            let a = self.colour.sample(uv.x, uv.y, lod);
            let b = self.surface.sample(uv.x, uv.y, lod);
            for k in 0..4 {
                c[k] += a[k] * weight;
                s[k] += b[k] * weight;
            }
        }
        (c, s)
    }
}

/// Every tileable set the bake uses.
pub struct Sets {
    pub granite: Set,
    pub sandstone: Set,
    pub cobble: Set,
    pub moss: Set,
    pub lichen: Set,
}

impl Sets {
    pub fn load(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            granite: Set::load(dir, "Granite", 1.0)?,
            sandstone: Set::load(dir, "Sandstone", 1.5)?,
            cobble: Set::load(dir, "Cobble", 0.5)?,
            moss: Set::load(dir, "Moss", 0.5)?,
            lichen: Set::load(dir, "Lichen", 0.6)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stone {
    Granite,
    Sandstone,
    Cobble,
}

/// How a rock looks.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub stone: Stone,
    /// Depth of the stone's height map (metres).
    pub relief: f32,
    /// Multiplier on the stone's albedo.
    pub tint: Vec3,
    /// Amounts 0..1.
    pub moss: f32,
    pub lichen: f32,
    pub stain: f32,
    /// Height of the damp, grass-tinted band at the ground line (metres).
    pub band: f32,
    pub seed: u32,
}

struct Texel {
    albedo: Vec3,
    height: f32,
    rough: f32,
    ao: f32,
}

/// The material at object point `p` with normal `n` and cavities `cav`.
fn surface(sets: &Sets, look: &Look, p: Vec3, n: Vec3, cav: [f32; 2], texel: f32) -> Texel {
    let s = look.seed;
    let a = n.abs();
    let a = a * a * a * a;
    let w = a / (a.x + a.y + a.z).max(1e-6);
    let stone = match look.stone {
        Stone::Granite => &sets.granite,
        Stone::Sandstone => &sets.sandstone,
        Stone::Cobble => &sets.cobble,
    };
    let (rc, rs) = stone.triplanar(p, w, texel, 1.0);
    let mut albedo = Vec3::new(rc[0], rc[1], rc[2]) * look.tint;
    let rock_h = rc[3];
    let mut rough = rs[0];
    let mut ao = rs[1].powf(0.7);
    let (fine, broad) = (cav[0], cav[1]);
    // Broad weathering blotches, warmer / greyer patches.
    let m1 = fbm(p * 0.35, 3, s + 1);
    let m2 = fbm(p * 1.6, 3, s + 2);
    albedo *= 1.0 + 0.2 * m1 + 0.08 * m2;
    let warm = smoothstep(-0.15, 0.4, fbm(p * 0.55 + Vec3::splat(4.0), 2, s + 3));
    albedo = albedo.lerp(albedo * Vec3::new(1.06, 1.0, 0.86), warm * 0.6);
    // Dirt in hollows, bleached ridges.
    let dirt = smoothstep(0.002, 0.025, fine) * 0.3 + smoothstep(0.01, 0.14, broad) * 0.25;
    albedo *= 1.0 - dirt;
    albedo *= 1.0 + 0.14 * smoothstep(0.004, 0.03, -fine);
    // Iron and water stains running down the sides.
    if look.stain > 0.0 {
        let streak = fbm(Vec3::new(p.x * 2.2, p.y * 0.22, p.z * 2.2), 3, s + 4);
        let st = smoothstep(0.12, 0.42, streak) * look.stain * smoothstep(0.9, 0.2, n.y);
        albedo = albedo.lerp(albedo * Vec3::new(0.86, 0.72, 0.55), st * 0.55);
        let dark = smoothstep(
            0.2,
            0.5,
            fbm(Vec3::new(p.x * 4.0, p.y * 0.3, p.z * 4.0), 2, s + 9),
        );
        albedo *= 1.0 - dark * 0.18 * look.stain * smoothstep(0.7, 0.0, n.y);
    }
    // Weathered pits and grain bumps between the mesh's resolution and the texel's.
    let bumps = 0.9 * (ridged(p * 26.0, 3, s + 14) - 0.5) + 0.5 * fbm(p * 70.0, 2, s + 15);
    let mut height = rock_h * look.relief + bumps * look.relief * 0.8;
    // Lichen: crusts on exposed faces, fewer in hollows and underneath.
    if look.lichen > 0.0 {
        let (lc, ls) = sets.lichen.triplanar(p, w, texel, 2.0);
        let patch = smoothstep(-0.15, 0.3, fbm(p * 0.8, 3, s + 5) + look.lichen - 0.55);
        let exposed = smoothstep(-0.4, 0.15, n.y) * (1.0 - smoothstep(0.0, 0.03, fine));
        let cover = (ls[2] * patch * exposed * (look.lichen * 1.6).min(1.0)).clamp(0.0, 1.0);
        albedo = albedo.lerp(Vec3::new(lc[0], lc[1], lc[2]), cover);
        height += cover * (0.0006 + 0.0012 * lc[3]);
        rough += (ls[0] - rough) * cover;
        ao = ao.min(1.0 - 0.15 * cover * (1.0 - ls[1]));
    }
    // Moss on top faces and in hollows that hold water, filling crevices first.
    if look.moss > 0.0 {
        let top = smoothstep(0.3, 0.85, n.y + 0.18 * fbm(p * 1.8, 2, s + 6));
        let wet = smoothstep(0.0, 0.06, broad) * 0.6 + smoothstep(0.0, 0.015, fine) * 0.4;
        let patch = fbm(p * 0.6, 3, s + 7) * 0.5 + 0.5;
        let want = look.moss * (top * (0.4 + 1.1 * patch) + wet * 0.45 * top.max(0.25));
        let (mc, ms) = sets.moss.triplanar(p, w, texel, 3.0);
        let mh = mc[3];
        let m = smoothstep(0.5, 0.66, want + 0.3 * (mh - 0.5) - 0.25 * (rock_h - 0.5));
        let tone = 0.9 + 0.25 * perlin(p * 2.3, s + 8);
        albedo = albedo.lerp(Vec3::new(mc[0], mc[1], mc[2]) * tone, m);
        height = height.max(rock_h * look.relief) + m * (0.003 + 0.011 * mh);
        rough += (ms[0] - rough) * m;
        ao += (ms[1].powf(0.8) - ao) * m;
    }
    // The ground line: damp, darker, grass-tinted; soil below it.
    let band = look.band * (1.0 + 0.5 * fbm(p * 1.4, 2, s + 10));
    let g = 1.0 - smoothstep(-0.1 * look.band, band, p.y);
    albedo = albedo.lerp(
        albedo * Vec3::new(0.55, 0.62, 0.42) + Vec3::new(0.012, 0.02, 0.004),
        g * 0.75,
    );
    albedo = albedo.lerp(Vec3::new(0.085, 0.105, 0.03), g * g * 0.3);
    rough *= 1.0 - 0.12 * g;
    let soil = 1.0 - smoothstep(-0.6 * look.band, -0.12 * look.band, p.y);
    albedo = albedo.lerp(Vec3::new(0.055, 0.042, 0.028), soil);
    ao *= (1.0 - 0.45 * smoothstep(0.002, 0.03, fine))
        * (1.0 - 0.3 * smoothstep(0.02, 0.16, broad))
        * (1.0 - 0.3 * g);
    Texel {
        albedo,
        height,
        rough: rough.clamp(0.05, 1.0),
        ao: ao.clamp(0.0, 1.0),
    }
}

/// The baked maps of one rock.
pub struct Baked {
    /// Albedo divided by `tint` (so the material colour carries the rock's overall colour,
    /// which is what Misty GI bounces).
    pub albedo: RgbImage,
    pub normal: RgbImage,
    /// R = metallic (0), G = occlusion, B = 0, A = smoothness.
    pub mask: RgbaImage,
    /// Linear material colour.
    pub tint: Vec3,
}

/// Per texel: (triangle, barycentrics) of the dense mesh, or none.
type Coverage = Vec<Option<(u32, [f32; 3])>>;

fn rasterise(data: &MeshData, n: usize) -> (Coverage, Vec<f32>) {
    let mut cover: Coverage = vec![None; n * n];
    // Metres per texel of each triangle.
    let mut texel = Vec::with_capacity(data.indices.len() / 3);
    for (ti, t) in data.indices.chunks(3).enumerate() {
        let v = |k: usize| data.vertices[t[k] as usize];
        let uv = |k: usize| Vec2::from(v(k).uv) * n as f32;
        let (a, b, c) = (uv(0), uv(1), uv(2));
        let p = |k: usize| Vec3::from(v(k).position);
        let area3 = (p(1) - p(0)).cross(p(2) - p(0)).length() * 0.5;
        let det = (b - a).perp_dot(c - a);
        let area2 = det.abs() * 0.5;
        texel.push(if area2 > 1e-9 {
            (area3 / area2).sqrt()
        } else {
            0.0
        });
        if det.abs() < 1e-12 {
            continue;
        }
        let lo = a.min(b).min(c).floor().max(Vec2::ZERO);
        let hi = a.max(b).max(c).ceil().min(Vec2::splat(n as f32));
        for y in lo.y as usize..hi.y as usize {
            for x in lo.x as usize..hi.x as usize {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w1 = (q - a).perp_dot(c - a) / det;
                let w2 = (b - a).perp_dot(q - a) / det;
                let w0 = 1.0 - w1 - w2;
                if w0 >= -1e-4 && w1 >= -1e-4 && w2 >= -1e-4 {
                    cover[y * n + x] = Some((ti as u32, [w0, w1, w2]));
                }
            }
        }
    }
    (cover, texel)
}

/// Fill empty texels from their filled neighbours, `passes` texels out (mip gutters).
fn dilate<T: Copy + Send + Sync>(
    texels: &mut [Option<T>],
    n: usize,
    passes: usize,
    mix: impl Fn(&[T]) -> T,
) {
    for _ in 0..passes {
        let src = texels.to_vec();
        for y in 0..n {
            for x in 0..n {
                if src[y * n + x].is_some() {
                    continue;
                }
                let mut found = Vec::with_capacity(4);
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (a, b) = (x as i64 + dx, y as i64 + dy);
                    if a >= 0
                        && b >= 0
                        && (a as usize) < n
                        && (b as usize) < n
                        && let Some(t) = src[b as usize * n + a as usize]
                    {
                        found.push(t);
                    }
                }
                if !found.is_empty() {
                    texels[y * n + x] = Some(mix(&found));
                }
            }
        }
    }
}

/// Bake the maps of `data` (with per-vertex `cavity`) at `size` texels (normal and mask
/// at `size / detail_div`).
pub fn bake(
    sets: &Sets,
    look: &Look,
    data: &MeshData,
    cavity: &[[f32; 2]],
    size: usize,
    detail_div: usize,
) -> Baked {
    let n = size;
    let (cover, texel_m) = rasterise(data, n);
    // Shade rows in parallel.
    let threads = std::thread::available_parallelism().map_or(4, |t| t.get());
    let rows_per = n.div_ceil(threads);
    // Per texel: albedo (linear), tangent-space normal, (ao, smoothness).
    let mut out: Vec<Option<(Vec3, Vec3, Vec2)>> = vec![None; n * n];
    std::thread::scope(|scope| {
        for (chunk, slot) in out.chunks_mut(rows_per * n).enumerate() {
            let cover = &cover;
            let texel_m = &texel_m;
            scope.spawn(move || {
                for (i, o) in slot.iter_mut().enumerate() {
                    let idx = chunk * rows_per * n + i;
                    let Some((tri, w)) = cover[idx] else {
                        continue;
                    };
                    let t = &data.indices[tri as usize * 3..tri as usize * 3 + 3];
                    let vs = [0, 1, 2].map(|k| data.vertices[t[k] as usize]);
                    let mix3 = |f: &dyn Fn(usize) -> Vec3| f(0) * w[0] + f(1) * w[1] + f(2) * w[2];
                    let p = mix3(&|k| Vec3::from(vs[k].position));
                    let nrm = mix3(&|k| Vec3::from(vs[k].normal)).normalize_or(Vec3::Y);
                    let tan =
                        mix3(&|k| Vec3::new(vs[k].tangent[0], vs[k].tangent[1], vs[k].tangent[2]));
                    let sign = vs[0].tangent[3];
                    let tan = (tan - nrm * nrm.dot(tan)).normalize_or(Vec3::X);
                    let bit = nrm.cross(tan) * sign;
                    let cav = [0, 1].map(|c| {
                        cavity[t[0] as usize][c] * w[0]
                            + cavity[t[1] as usize][c] * w[1]
                            + cavity[t[2] as usize][c] * w[2]
                    });
                    let tm = texel_m[tri as usize].max(1e-4);
                    let centre = surface(sets, look, p, nrm, cav, tm);
                    let e = tm;
                    let h = |d: Vec3| surface(sets, look, p + d, nrm, cav, tm).height;
                    let dt = (h(tan * e) - h(-tan * e)) / (2.0 * e);
                    let db = (h(bit * e) - h(-bit * e)) / (2.0 * e);
                    let normal = Vec3::new(-dt, -db, 1.0).normalize();
                    *o = Some((
                        centre.albedo,
                        normal,
                        Vec2::new(centre.ao, 1.0 - centre.rough),
                    ));
                }
            });
        }
    });
    dilate(&mut out, n, 12, |f| {
        let k = f.len() as f32;
        let (a, b, c) = f.iter().fold((Vec3::ZERO, Vec3::ZERO, Vec2::ZERO), |s, t| {
            (s.0 + t.0, s.1 + t.1, s.2 + t.2)
        });
        (a / k, b.normalize_or(Vec3::Z), c / k)
    });
    // Material colour: a bright percentile of the albedo (the texture keeps the rest).
    let mut lum: Vec<Vec3> = out.iter().flatten().map(|t| t.0).collect();
    let tint = if lum.is_empty() {
        Vec3::ONE
    } else {
        let pick = |c: usize, l: &mut Vec<Vec3>| {
            l.sort_by(|a, b| a[c].total_cmp(&b[c]));
            l[(l.len() - 1) * 995 / 1000][c]
        };
        Vec3::new(pick(0, &mut lum), pick(1, &mut lum), pick(2, &mut lum)).max(Vec3::splat(0.02))
    };
    let filled = |i: usize| out[i].unwrap_or((Vec3::splat(0.2), Vec3::Z, Vec2::new(1.0, 0.2)));
    let albedo = RgbImage::from_fn(n as u32, n as u32, |x, y| {
        let a = filled(y as usize * n + x as usize).0 / tint;
        let c = |v: f32| (linear_to_srgb(v) * 255.0).round() as u8;
        image::Rgb([c(a.x), c(a.y), c(a.z)])
    });
    let d = detail_div.max(1);
    let m = n / d;
    let avg = |x: u32, y: u32| {
        let (mut nn, mut s) = (Vec3::ZERO, Vec2::ZERO);
        for dy in 0..d {
            for dx in 0..d {
                let t = filled((y as usize * d + dy) * n + x as usize * d + dx);
                nn += t.1;
                s += t.2;
            }
        }
        (nn.normalize_or(Vec3::Z), s / (d * d) as f32)
    };
    let enc = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
    let normal = RgbImage::from_fn(m as u32, m as u32, |x, y| {
        let (nn, _) = avg(x, y);
        image::Rgb([enc(nn.x), enc(nn.y), enc(nn.z)])
    });
    let unit = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let mask = RgbaImage::from_fn(m as u32, m as u32, |x, y| {
        let (_, s) = avg(x, y);
        image::Rgba([0, unit(s.x), 0, unit(s.y)])
    });
    Baked {
        albedo,
        normal,
        mask,
        tint,
    }
}
