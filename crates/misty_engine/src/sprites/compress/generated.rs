//! Block-compressed textures the renderer makes itself from several images: the tree maps
//! with packed channels and the impostors (`render/trees.rs`), the grass and flower atlas
//! (`render/terrain_grass.rs`) and the terrain layer arrays (`render/terrain_layers.rs`).
//!
//! Such a texture has no single source file, so its cache key hashes the packed level-0 pixels
//! with a recipe (how its mips are made), the format and sRGB. A hit skips the mip chain (the
//! coverage-keeping mips are the slow part) as well as the encode. The mips are made as for
//! RGBA8 (coverage-keeping ones included) and each level is encoded as it is.
//!
//! An array is all blocks or all RGBA8 (one texture format). While a slice is encoded in the
//! background ([`Mode::Background`], the editor) the whole array draws uncompressed;
//! [`waiting`] tells its owner when the blocks are in the cache, and the owner builds it again
//! (a cache hit then). Players read the cache only ([`Mode::CacheOnly`]); builds bake the
//! entries ([`Ctx::bake`]).
//!
//! Formats by role ([`Role`]), from the Compression setting of the role's main source image
//! (None keeps it RGBA8):
//!
//! | Role | Normal Quality | High Quality |
//! |---|---|---|
//! | Opaque colour (terrain albedo) | BC1 | BC7 |
//! | Colour + alpha (leaf, grass and flower cutouts, impostor albedo, bark + height) | BC7 | BC7 |
//! | Four data channels (normal x, y + roughness, occlusion, translucency or height) | BC7 | BC7 |
//!
//! BC5 holds two channels and BC4 one, so a normal map with two more maps packed in would need
//! BC5 + BC4 (two textures, 1.5 bytes per texel, new bindings); BC7 keeps the four channels in
//! one texture at 1 byte per texel. BC3's colour endpoints (5:6:5) are too coarse for normals.
//! For cutouts BC7 keeps each mip's alpha-tested coverage as well as BC3 does, with better
//! colour (see the tests).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::*;

/// What a renderer-made texture holds (picks its format).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Opaque colour (alpha unused): BC1, BC7 at High Quality.
    Opaque,
    /// Colour with an alpha that matters (alpha-tested cutouts, height in alpha): BC7.
    ColorAlpha,
    /// Four data channels: BC7.
    Packed,
}

impl Role {
    /// The format of this role at `compression` (None: uncompressed).
    pub fn format(self, compression: TextureCompression) -> Option<BcFormat> {
        match (self, compression) {
            (_, TextureCompression::None) => None,
            (Self::Opaque, TextureCompression::NormalQuality) => Some(BcFormat::Bc1),
            _ => Some(BcFormat::Bc7),
        }
    }
}

/// Renderer-made textures compressed at all (tests compare with them off).
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Keys whose background encode could not be saved (not tried again this run).
static FAILED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Whether any of `keys` is still being encoded in the background.
pub fn waiting(keys: &[String]) -> bool {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    keys.iter().any(|k| pending.contains(k))
}

/// Where and how renderer-made textures get their blocks.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The device samples BC formats.
    pub bc: bool,
    /// Texture import platform (each source's Compression setting is read for it).
    pub platform: &'static str,
    pub mode: Mode,
    /// Project whose `Library/TextureCache` holds the blocks (None: no cache).
    pub root: Option<PathBuf>,
}

impl Ctx {
    /// The running engine's (`bc`: the device has `TEXTURE_COMPRESSION_BC`).
    pub fn runtime(bc: bool) -> Self {
        Self {
            bc,
            platform: super::super::import::current_platform(),
            mode: if enabled() { mode() } else { Mode::Off },
            root: cache_root(),
        }
    }

    /// A build's: encoded on the spot into project `root`'s cache, for `platform`.
    pub fn bake(root: &Path, platform: &'static str) -> Self {
        Self {
            bc: true,
            platform,
            mode: Mode::Blocking,
            root: Some(root.to_path_buf()),
        }
    }

    /// Never compressed.
    pub fn off() -> Self {
        Self {
            bc: false,
            platform: super::super::import::current_platform(),
            mode: Mode::Off,
            root: None,
        }
    }

    /// The format of `role` for a texture whose main source image is `rel` (its Compression
    /// setting on this platform; an empty path or a made-up image: Normal Quality).
    pub fn format(&self, role: Role, rel: &str) -> Option<BcFormat> {
        if !self.bc || self.mode == Mode::Off {
            return None;
        }
        let compression = if rel.is_empty() {
            TextureCompression::NormalQuality
        } else {
            super::super::importer(rel).compression_for(self.platform)
        };
        role.format(compression)
    }

    /// The format of `role` for an array whose slices come from `rels`: None when any slice
    /// asks for no compression, High Quality when any asks for it.
    pub fn format_of_all<'a>(
        &self,
        role: Role,
        rels: impl IntoIterator<Item = &'a str>,
    ) -> Option<BcFormat> {
        if !self.bc || self.mode == Mode::Off {
            return None;
        }
        let mut compression = TextureCompression::NormalQuality;
        for rel in rels.into_iter().filter(|r| !r.is_empty()) {
            match super::super::importer(rel).compression_for(self.platform) {
                TextureCompression::None => return None,
                TextureCompression::HighQuality => compression = TextureCompression::HighQuality,
                TextureCompression::NormalQuality => {}
            }
        }
        role.format(compression)
    }
}

/// One texture (or array slice) ready to upload: blocks, or RGBA8 levels.
#[derive(Clone, Debug)]
pub enum Made {
    Blocks(Compressed),
    Rgba { chain: Vec<RgbaImage>, srgb: bool },
}

impl Made {
    pub fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Blocks(c) => c.wgpu_format(),
            Self::Rgba { srgb: true, .. } => wgpu::TextureFormat::Rgba8UnormSrgb,
            Self::Rgba { .. } => wgpu::TextureFormat::Rgba8Unorm,
        }
    }

    /// Size of level 0.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Blocks(c) => (c.width, c.height),
            Self::Rgba { chain, .. } => chain[0].dimensions(),
        }
    }

    pub fn level_count(&self) -> u32 {
        match self {
            Self::Blocks(c) => c.levels.len() as u32,
            Self::Rgba { chain, .. } => chain.len() as u32,
        }
    }

    /// The bytes of level `l` (blocks row by row, or RGBA8 texels).
    pub fn level(&self, l: usize) -> &[u8] {
        match self {
            Self::Blocks(c) => &c.levels[l],
            Self::Rgba { chain, .. } => chain[l].as_raw(),
        }
    }

    /// GPU bytes of all levels.
    pub fn bytes(&self) -> u64 {
        (0..self.level_count() as usize)
            .map(|l| self.level(l).len() as u64)
            .sum()
    }

    pub fn is_compressed(&self) -> bool {
        matches!(self, Self::Blocks(_))
    }

    /// RGBA8 levels (blocks decoded: an array whose other slices are uncompressed).
    pub fn into_rgba(self) -> Self {
        match self {
            Self::Blocks(c) => Self::Rgba {
                chain: (0..c.levels.len()).map(|l| c.decode(l)).collect(),
                srgb: c.srgb,
            },
            rgba => rgba,
        }
    }
}

/// The slices of a texture or array, all blocks or all RGBA8.
#[derive(Clone, Debug, Default)]
pub struct Layers {
    pub layers: Vec<Made>,
    /// Cache keys of the slices.
    pub keys: Vec<String>,
    /// Keys being encoded in the background (the slices are RGBA8 until then).
    pub waiting: Vec<String>,
}

impl Layers {
    pub fn bytes(&self) -> u64 {
        self.layers.iter().map(Made::bytes).sum()
    }

    /// Project-relative cache entries of the slices (what a build packs).
    pub fn entries(&self) -> Vec<String> {
        if self.layers.iter().all(Made::is_compressed) {
            self.keys.iter().map(|k| cache_rel(k)).collect()
        } else {
            Vec::new()
        }
    }
}

/// How a renderer-made texture is built: `recipe` names how its mips are made (part of the
/// key: a different mip filter is a different texture), `format` is None to stay RGBA8.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    pub recipe: &'a str,
    pub format: Option<BcFormat>,
    pub srgb: bool,
    /// Alpha-tested (alpha to coverage): the blocks keep the edges' mean alpha (see
    /// [`encode_chain`]).
    pub coverage: bool,
}

/// Cache key of a renderer-made slice.
pub fn key(image: &RgbaImage, spec: &Spec, format: BcFormat) -> String {
    let mut h = Sha256::new();
    h.update(ENCODER_VERSION.to_le_bytes());
    h.update(b"generated|");
    h.update(ENCODER.as_bytes());
    h.update(spec.recipe.as_bytes());
    h.update([format.id(), u8::from(spec.srgb), u8::from(spec.coverage)]);
    h.update(image.width().to_le_bytes());
    h.update(image.height().to_le_bytes());
    h.update(image.as_raw());
    h.finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The encoder settings of renderer-made textures (in their cache keys). BC7 uses the ISPC
/// kernels' "alpha very fast" settings: within 0.1–1 dB (and 0.3°) of "alpha basic" on
/// MeadowDemo's tree, grass and terrain textures at a third to a sixth of the time (about
/// 40 s of CPU for the scene with basic; `made_textures_bc7_settings`). Cutout edges are
/// unbiased and the blocks checked against [`MIN_PSNR`].
const ENCODER: &str = "bc7 alpha very fast|unbiased edges|30 dB";

fn variant(format: BcFormat) -> CompressionVariant {
    match format {
        BcFormat::Bc7 => CompressionVariant::BC7(BC7Settings::alpha_very_fast()),
        f => f.variant(),
    }
}

/// Blocks of every level of a mip chain made beforehand (`srgb`: sampled as sRGB).
///
/// `coverage`: an alpha-tested cutout. BC7 rounds the alpha of the partly transparent edge
/// texels up by about 0.6/255 on average (MeadowDemo's grass atlas, every encoder setting),
/// which alpha to coverage turns into slightly denser, brighter grass; BC3's alpha block does
/// not. A cutout level whose edges come out more than 0.2/255 off is encoded again with that
/// bias taken out of its edge texels (dithered), and the closer of the two is kept.
pub fn encode_chain(
    chain: &[RgbaImage],
    format: BcFormat,
    srgb: bool,
    coverage: bool,
) -> Compressed {
    let (width, height) = chain[0].dimensions();
    let level = |l: &RgbaImage| {
        let blocks = encode_level_as(l, format, variant(format));
        if coverage && format == BcFormat::Bc7 {
            unbiased_edges(l, format, blocks)
        } else {
            blocks
        }
    };
    Compressed {
        format,
        srgb: srgb && format != BcFormat::Bc5 && format != BcFormat::Bc6h,
        width,
        height,
        levels: chain.iter().map(level).collect(),
    }
}

/// Mean alpha error (in 1/255) of `blocks` over the partly transparent texels of `img`.
fn edge_alpha_bias(img: &RgbaImage, format: BcFormat, blocks: &[u8]) -> Option<f64> {
    let (w, h) = img.dimensions();
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    let mut rgba = vec![0u8; pw as usize * ph as usize * 4];
    block_compression::decode::decompress_blocks_as_rgba8(
        variant(format),
        pw,
        ph,
        blocks,
        &mut rgba,
    );
    let (mut sum, mut n) = (0i64, 0i64);
    for (i, p) in img.pixels().enumerate() {
        if (1..=254).contains(&p[3]) {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let got = rgba[((y * pw + x) * 4 + 3) as usize];
            sum += i64::from(got) - i64::from(p[3]);
            n += 1;
        }
    }
    (n > 0).then(|| sum as f64 / n as f64)
}

/// `blocks` of cutout level `img`, or its blocks encoded again with the edges' alpha bias
/// taken out (see [`encode_chain`]).
fn unbiased_edges(img: &RgbaImage, format: BcFormat, blocks: Vec<u8>) -> Vec<u8> {
    let Some(bias) = edge_alpha_bias(img, format, &blocks) else {
        return blocks;
    };
    if bias.abs() < 0.2 {
        return blocks;
    }
    let mut adjusted = img.clone();
    for (i, p) in adjusted.pixels_mut().enumerate() {
        if (1..=254).contains(&p[3]) {
            // A fixed dither in 0..1 per texel spreads the fractional shift.
            let d = f64::from((i as u32).wrapping_mul(2_654_435_761) >> 8) / f64::from(1u32 << 24);
            p[3] = (f64::from(p[3]) - bias + d - 0.5).round().clamp(1.0, 254.0) as u8;
        }
    }
    let again = encode_level_as(&adjusted, format, variant(format));
    match edge_alpha_bias(img, format, &again) {
        Some(b) if b.abs() < bias.abs() => again,
        _ => blocks,
    }
}

/// Worst channel's PSNR (dB, level 0) below which a slice's blocks are not used: its array
/// stays RGBA8. Noise-like data does not survive 4×4 blocks: MeadowThatch's straw normal +
/// height map comes out of BC7 with a 15° mean normal error and its height at 22 dB, which
/// shows in the meadow's ground (and BC5 + BC4 would still be 6°). Every other MeadowDemo
/// texture keeps 38 dB or more in every channel.
const MIN_PSNR: f64 = 30.0;

/// Project-relative path of the marker of a slice kept uncompressed (see [`MIN_PSNR`]).
fn raw_rel(key: &str) -> String {
    format!("{LIBRARY_DIR}/{key}.raw")
}

/// Whether the blocks `c` hold `img` closely enough to be used ([`MIN_PSNR`]; BC1 is for
/// opaque colour, its alpha is not looked at).
fn close_enough(img: &RgbaImage, c: &Compressed) -> bool {
    let decoded = c.decode(0);
    let channels = if c.format == BcFormat::Bc1 { 3 } else { 4 };
    let mut sum = [0u64; 4];
    for (p, q) in img.pixels().zip(decoded.pixels()) {
        for (ch, s) in sum.iter_mut().enumerate().take(channels) {
            let d = u64::from(p[ch].abs_diff(q[ch]));
            *s += d * d;
        }
    }
    let n = (img.width() * img.height()).max(1) as f64;
    sum[..channels].iter().all(|&s| {
        let mse = s as f64 / n;
        mse == 0.0 || 10.0 * (255.0f64 * 255.0 / mse).log10() >= MIN_PSNR
    })
}

/// Blocks of a slice's chain, or None when they would not hold it ([`MIN_PSNR`]).
fn encode_slice(chain: &[RgbaImage], format: BcFormat, spec: &Spec) -> Option<Compressed> {
    let c = encode_chain(chain, format, spec.srgb, spec.coverage);
    close_enough(&chain[0], &c).then_some(c)
}

/// Remember under project `root` that slice `key` stays uncompressed.
fn store_raw(root: &Path, key: &str) -> Option<()> {
    let path = root.join(raw_rel(key));
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(path, b"").ok()
}

/// The slices `images` (one size) as blocks or RGBA8, `mips` making each one's chain. Cache
/// hits upload as they are; misses are encoded now ([`Mode::Blocking`]) or on a worker while
/// the slices go up uncompressed ([`Mode::Background`]: see [`Layers::waiting`]). An array
/// with a slice the blocks would not hold ([`MIN_PSNR`]) stays RGBA8.
pub fn make(
    images: Vec<RgbaImage>,
    spec: &Spec,
    ctx: &Ctx,
    mips: impl Fn(RgbaImage) -> Vec<RgbaImage>,
) -> Layers {
    let rgba = |images: Vec<RgbaImage>| -> Vec<Made> {
        images
            .into_iter()
            .map(|i| Made::Rgba {
                chain: mips(i),
                srgb: spec.srgb,
            })
            .collect()
    };
    let size = images.first().map(RgbaImage::dimensions);
    let blockable = size.is_some_and(|(w, h)| w.is_multiple_of(4) && h.is_multiple_of(4))
        && images.iter().all(|i| Some(i.dimensions()) == size);
    let format = match spec.format {
        Some(f) if blockable && ctx.bc && ctx.mode != Mode::Off => f,
        _ => {
            return Layers {
                layers: rgba(images),
                ..Default::default()
            };
        }
    };
    let keys: Vec<String> = images.iter().map(|i| key(i, spec, format)).collect();
    let uncompressed = |images: Vec<RgbaImage>, keys: Vec<String>| Layers {
        layers: rgba(images),
        keys,
        waiting: Vec::new(),
    };
    let Some(root) = ctx.root.as_ref() else {
        // No cache: encoded on the spot when blocking (tests without a project), else RGBA8.
        if ctx.mode != Mode::Blocking {
            return uncompressed(images, keys);
        }
        let blocks: Option<Vec<Made>> = images
            .iter()
            .map(|img| encode_slice(&mips(img.clone()), format, spec).map(Made::Blocks))
            .collect();
        return match blocks {
            Some(layers) => Layers {
                layers,
                keys,
                waiting: Vec::new(),
            },
            None => uncompressed(images, keys),
        };
    };
    if keys
        .iter()
        .any(|k| crate::vfs::read(root.join(raw_rel(k))).is_ok())
    {
        return uncompressed(images, keys);
    }
    let (w, h) = size.unwrap_or_default();
    let hit = |k: &String| {
        let c = cached(root, k)?;
        (c.format == format && (c.width, c.height) == (w, h)).then_some(c)
    };
    let hits: Vec<Option<Compressed>> = keys.iter().map(hit).collect();
    if hits.iter().all(Option::is_some) {
        return Layers {
            layers: hits.into_iter().flatten().map(Made::Blocks).collect(),
            keys,
            waiting: Vec::new(),
        };
    }
    match ctx.mode {
        Mode::Blocking => {
            let mut layers = Vec::with_capacity(images.len());
            for ((img, k), hit) in images.iter().zip(&keys).zip(hits) {
                let c = match hit {
                    Some(c) => c,
                    None => match encode_slice(&mips(img.clone()), format, spec) {
                        Some(c) => {
                            store(root, k, &c);
                            c
                        }
                        None => {
                            store_raw(root, k);
                            return uncompressed(images, keys);
                        }
                    },
                };
                layers.push(Made::Blocks(c));
            }
            Layers {
                layers,
                keys,
                waiting: Vec::new(),
            }
        }
        Mode::Background => {
            let layers = rgba(images);
            let mut waiting = Vec::new();
            for ((made, k), hit) in layers.iter().zip(&keys).zip(&hits) {
                let Made::Rgba { chain, .. } = made else {
                    continue;
                };
                if hit.is_some() || FAILED.lock().unwrap_or_else(|e| e.into_inner()).contains(k) {
                    continue;
                }
                waiting.push(k.clone());
                if !PENDING
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(k.clone())
                {
                    continue; // Already being encoded (another owner of the same slice).
                }
                let (chain, key, root) = (chain.clone(), k.clone(), root.clone());
                let (srgb, coverage) = (spec.srgb, spec.coverage);
                crate::background::pool().spawn(move || {
                    let spec = Spec {
                        recipe: "",
                        format: Some(format),
                        srgb,
                        coverage,
                    };
                    let saved = match encode_slice(&chain, format, &spec) {
                        Some(c) => store(&root, &key, &c).map(|_| ()),
                        None => store_raw(&root, &key),
                    };
                    if saved.is_none() {
                        FAILED
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key.clone());
                    }
                    PENDING
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&key);
                });
            }
            Layers {
                layers,
                keys,
                waiting,
            }
        }
        Mode::CacheOnly | Mode::Off => uncompressed(images, keys),
    }
}

/// [`make`] for one texture.
pub fn make_one(
    image: RgbaImage,
    spec: &Spec,
    ctx: &Ctx,
    mips: impl Fn(RgbaImage) -> Vec<RgbaImage>,
) -> Layers {
    make(vec![image], spec, ctx, mips)
}

/// Bring `layers` to one kind: when some are blocks and some RGBA8 (slices of one array made
/// separately), the blocks are decoded.
pub fn same_kind(layers: Vec<Made>) -> Vec<Made> {
    if layers.iter().all(Made::is_compressed) || !layers.iter().any(Made::is_compressed) {
        return layers;
    }
    layers.into_iter().map(Made::into_rgba).collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("misty_made_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn ctx(root: &Path, mode: Mode) -> Ctx {
        Ctx {
            mode,
            ..Ctx::bake(root, "Windows")
        }
    }

    fn cutout(seed: u8) -> RgbaImage {
        RgbaImage::from_fn(32, 32, |x, y| {
            let a = if (x + y) % 7 < 4 { 255 } else { 0 };
            image::Rgba([x as u8 * 8, y as u8 * 8, seed, a])
        })
    }

    const SPEC: Spec = Spec {
        recipe: "test",
        format: Some(BcFormat::Bc7),
        srgb: true,
        coverage: true,
    };

    #[test]
    fn formats_by_role_and_compression() {
        use TextureCompression::{HighQuality, None as Off, NormalQuality};
        assert_eq!(Role::Opaque.format(NormalQuality), Some(BcFormat::Bc1));
        assert_eq!(Role::Opaque.format(HighQuality), Some(BcFormat::Bc7));
        assert_eq!(Role::ColorAlpha.format(NormalQuality), Some(BcFormat::Bc7));
        assert_eq!(Role::Packed.format(NormalQuality), Some(BcFormat::Bc7));
        for role in [Role::Opaque, Role::ColorAlpha, Role::Packed] {
            assert_eq!(role.format(Off), None);
        }
        // Made-up images (no source file) take Normal Quality; no BC on the device, none.
        let root = project("formats");
        assert_eq!(
            ctx(&root, Mode::Blocking).format(Role::Opaque, ""),
            Some(BcFormat::Bc1)
        );
        let no_bc = Ctx {
            bc: false,
            ..ctx(&root, Mode::Blocking)
        };
        assert_eq!(no_bc.format(Role::Packed, ""), None);
        assert_eq!(no_bc.format_of_all(Role::Packed, [""]), None);
    }

    #[test]
    fn key_follows_pixels_recipe_and_format() {
        let k = key(&cutout(1), &SPEC, BcFormat::Bc7);
        assert_eq!(k, key(&cutout(1), &SPEC, BcFormat::Bc7));
        assert_ne!(k, key(&cutout(2), &SPEC, BcFormat::Bc7));
        assert_ne!(k, key(&cutout(1), &SPEC, BcFormat::Bc3));
        let other = Spec {
            recipe: "other mips",
            ..SPEC
        };
        assert_ne!(k, key(&cutout(1), &other, BcFormat::Bc7));
        let linear = Spec {
            srgb: false,
            ..SPEC
        };
        assert_ne!(k, key(&cutout(1), &linear, BcFormat::Bc7));
    }

    /// Encoded once into the cache; read back later without making the mips again (players
    /// read the cache only).
    #[test]
    fn blocking_encodes_once_then_hits() {
        let root = project("blocking");
        let calls = Cell::new(0);
        let mips = |i: RgbaImage| {
            calls.set(calls.get() + 1);
            crate::render::texture::mip_chain(i, true)
        };
        let first = make(
            vec![cutout(1), cutout(2)],
            &SPEC,
            &ctx(&root, Mode::Blocking),
            mips,
        );
        assert!(first.layers.iter().all(Made::is_compressed));
        assert_eq!(calls.get(), 2);
        assert_eq!(first.entries().len(), 2);
        for e in first.entries() {
            assert!(root.join(e).is_file());
        }
        let Made::Blocks(c) = &first.layers[0] else {
            unreachable!()
        };
        assert_eq!((c.format, c.srgb, c.levels.len()), (BcFormat::Bc7, true, 6));
        let again = make(
            vec![cutout(1), cutout(2)],
            &SPEC,
            &ctx(&root, Mode::CacheOnly),
            mips,
        );
        assert_eq!(calls.get(), 2, "a hit makes no mips");
        assert!(again.layers.iter().all(Made::is_compressed));
        assert_eq!(again.bytes(), first.bytes());
        // A slice missing from the cache: a player draws the array uncompressed.
        let missing = make(
            vec![cutout(1), cutout(3)],
            &SPEC,
            &ctx(&root, Mode::CacheOnly),
            mips,
        );
        assert!(!missing.layers.iter().any(Made::is_compressed));
        assert!(missing.waiting.is_empty() && missing.entries().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The editor: uncompressed at once, blocks encoded on a worker, then a hit.
    #[test]
    fn background_draws_uncompressed_until_the_blocks_are_ready() {
        let root = project("background");
        let mips = |i| crate::render::texture::mip_chain(i, true);
        let c = ctx(&root, Mode::Background);
        let first = make(vec![cutout(4)], &SPEC, &c, mips);
        assert!(!first.layers[0].is_compressed());
        assert_eq!(first.waiting, first.keys);
        let started = std::time::Instant::now();
        while waiting(&first.waiting) {
            assert!(started.elapsed().as_secs() < 60, "encode never finished");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let later = make(vec![cutout(4)], &SPEC, &c, mips);
        assert!(later.layers[0].is_compressed());
        assert!(later.waiting.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Noise does not survive 4×4 blocks: the array stays RGBA8, and the cache remembers it
    /// (no encode on the next load, nothing for a build to pack).
    #[test]
    fn data_the_blocks_cannot_hold_stays_uncompressed() {
        let root = project("noise");
        let mut seed = 0x1234_5678u32;
        let noise = RgbaImage::from_fn(32, 32, |_, _| {
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 24) as u8
            };
            image::Rgba([next(), next(), next(), next()])
        });
        let calls = Cell::new(0);
        let mips = |i: RgbaImage| {
            calls.set(calls.get() + 1);
            crate::render::texture::mip_chain(i, false)
        };
        let packed = Spec {
            coverage: false,
            srgb: false,
            ..SPEC
        };
        let first = make(
            vec![cutout(1), noise.clone()],
            &packed,
            &ctx(&root, Mode::Blocking),
            mips,
        );
        assert!(!first.layers.iter().any(Made::is_compressed));
        assert!(first.entries().is_empty() && first.waiting.is_empty());
        assert!(root.join(raw_rel(&first.keys[1])).is_file());
        let before = calls.get();
        let again = make(
            vec![cutout(1), noise],
            &packed,
            &ctx(&root, Mode::Background),
            mips,
        );
        assert!(!again.layers.iter().any(Made::is_compressed));
        assert!(again.waiting.is_empty(), "nothing is encoded again");
        assert_eq!(calls.get(), before + 2, "only the RGBA8 chains");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sizes_off_the_block_grid_and_mixed_kinds_stay_uncompressed() {
        let root = project("odd");
        let mips = |i| crate::render::texture::mip_chain(i, true);
        let odd = RgbaImage::new(30, 32);
        let l = make(vec![odd], &SPEC, &ctx(&root, Mode::Blocking), mips);
        assert!(!l.layers[0].is_compressed() && l.keys.is_empty());
        let mixed = make(
            vec![RgbaImage::new(32, 32), RgbaImage::new(16, 16)],
            &SPEC,
            &ctx(&root, Mode::Blocking),
            mips,
        );
        assert!(!mixed.layers.iter().any(Made::is_compressed));
        // Blocks next to RGBA8 slices are decoded (one array, one format).
        let blocks = make_one(cutout(5), &SPEC, &ctx(&root, Mode::Blocking), mips);
        let rgba = make_one(cutout(5), &SPEC, &Ctx::off(), mips);
        let both = same_kind(vec![blocks.layers[0].clone(), rgba.layers[0].clone()]);
        assert!(!both.iter().any(Made::is_compressed));
        assert_eq!(both[0].format(), both[1].format());
        assert_eq!(both[0].level_count(), both[1].level_count());
        let _ = std::fs::remove_dir_all(&root);
    }
}
