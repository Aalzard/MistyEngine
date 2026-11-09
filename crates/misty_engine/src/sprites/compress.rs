//! GPU texture compression: BC1 / BC3 (colour, without or with alpha), BC5 (normal maps), BC7
//! (high quality colour) and BC6H (HDR), encoded on the CPU with `block_compression` and cached
//! in `Library/TextureCache/<key>.mtc`.
//!
//! The cache key hashes the source file's bytes with every setting that changes the result
//! (format choice, size, sRGB, mipmaps, encoder version), so a cache hit skips decoding the PNG
//! as well as encoding. Builds pack the cache entries of their textures next to the images
//! (`build_pipeline`), and players read them through the asset file system; a player never
//! encodes. Without the adapter feature `TEXTURE_COMPRESSION_BC`, or for sizes that are not a
//! multiple of 4, textures upload uncompressed.
//!
//! The editor encodes in the background ([`Mode::Background`]): a texture draws uncompressed
//! until its blocks are ready, then reloads ([`take_finished`]). Tests encode on the spot.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

use block_compression::{BC6HSettings, BC7Settings, CompressionVariant};
use image::RgbaImage;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use super::import::{TextureCompression, TextureImporter, TextureType};
use crate::assets;

/// Folder of the cached blocks, relative to the project.
pub const LIBRARY_DIR: &str = "Library/TextureCache";
/// Bumped when encoding changes: old cache entries are then never read again.
const ENCODER_VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"MISTYTC1";

/// A block-compressed format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BcFormat {
    /// RGB + 1-bit alpha, 8 bytes per 4×4 block.
    Bc1,
    /// RGBA (interpolated alpha), 16 bytes per block.
    Bc3,
    /// Two channels (normal map X and Y; Z is rebuilt in the shader), 16 bytes per block.
    Bc5,
    /// HDR RGB (half floats), 16 bytes per block.
    Bc6h,
    /// High quality RGBA, 16 bytes per block.
    Bc7,
}

impl BcFormat {
    pub const ALL: [Self; 5] = [Self::Bc1, Self::Bc3, Self::Bc5, Self::Bc6h, Self::Bc7];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bc1 => "BC1",
            Self::Bc3 => "BC3",
            Self::Bc5 => "BC5",
            Self::Bc6h => "BC6H",
            Self::Bc7 => "BC7",
        }
    }

    fn id(self) -> u8 {
        match self {
            Self::Bc1 => 1,
            Self::Bc3 => 3,
            Self::Bc5 => 5,
            Self::Bc6h => 6,
            Self::Bc7 => 7,
        }
    }

    fn from_id(id: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// Bytes of one 4×4 block.
    pub fn block_bytes(self) -> usize {
        match self {
            Self::Bc1 => 8,
            _ => 16,
        }
    }

    /// The GPU format (`srgb`: colour decoded from sRGB when sampled).
    pub fn wgpu(self, srgb: bool) -> wgpu::TextureFormat {
        use wgpu::TextureFormat as F;
        match (self, srgb) {
            (Self::Bc1, true) => F::Bc1RgbaUnormSrgb,
            (Self::Bc1, false) => F::Bc1RgbaUnorm,
            (Self::Bc3, true) => F::Bc3RgbaUnormSrgb,
            (Self::Bc3, false) => F::Bc3RgbaUnorm,
            (Self::Bc5, _) => F::Bc5RgUnorm,
            (Self::Bc6h, _) => F::Bc6hRgbUfloat,
            (Self::Bc7, true) => F::Bc7RgbaUnormSrgb,
            (Self::Bc7, false) => F::Bc7RgbaUnorm,
        }
    }

    fn variant(self) -> CompressionVariant {
        match self {
            Self::Bc1 => CompressionVariant::BC1,
            Self::Bc3 => CompressionVariant::BC3,
            Self::Bc5 => CompressionVariant::BC5,
            Self::Bc6h => CompressionVariant::BC6H(BC6HSettings::basic()),
            Self::Bc7 => CompressionVariant::BC7(BC7Settings::alpha_basic()),
        }
    }
}

/// A compressed texture: blocks of every mip level, largest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Compressed {
    pub format: BcFormat,
    pub srgb: bool,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
}

impl Compressed {
    pub fn wgpu_format(&self) -> wgpu::TextureFormat {
        self.format.wgpu(self.srgb)
    }

    /// GPU bytes of all levels.
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }

    /// The `.mtc` file: magic, format, sRGB, size, level count, then each level's length and
    /// blocks (little endian).
    pub fn to_file(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes() + 32 + self.levels.len() * 4);
        out.extend_from_slice(MAGIC);
        out.push(self.format.id());
        out.push(u8::from(self.srgb));
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&(self.levels.len() as u32).to_le_bytes());
        for l in &self.levels {
            out.extend_from_slice(&(l.len() as u32).to_le_bytes());
            out.extend_from_slice(l);
        }
        out
    }

    pub fn from_file(bytes: &[u8]) -> Option<Self> {
        let mut cur = bytes.strip_prefix(MAGIC.as_slice())?;
        let mut take = |n: usize| -> Option<&[u8]> {
            if cur.len() < n {
                return None;
            }
            let (a, b) = cur.split_at(n);
            cur = b;
            Some(a)
        };
        let format = BcFormat::from_id(take(1)?[0])?;
        let srgb = take(1)?[0] != 0;
        let u32_of = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap_or([0; 4]));
        let width = u32_of(take(4)?);
        let height = u32_of(take(4)?);
        let count = u32_of(take(4)?) as usize;
        if count > 16 {
            return None;
        }
        let mut levels = Vec::with_capacity(count);
        for l in 0..count {
            let len = u32_of(take(4)?) as usize;
            let (w, h) = level_size(width, height, l as u32);
            if len != w.div_ceil(4) as usize * h.div_ceil(4) as usize * format.block_bytes() {
                return None;
            }
            levels.push(take(len)?.to_vec());
        }
        Some(Self {
            format,
            srgb,
            width,
            height,
            levels,
        })
    }

    /// Level `l` decoded back to RGBA8 (tests and previews; BC6H clamps to 0..1).
    pub fn decode(&self, l: usize) -> RgbaImage {
        let (w, h) = level_size(self.width, self.height, l as u32);
        let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
        let mut rgba = vec![0u8; pw as usize * ph as usize * 4];
        block_compression::decode::decompress_blocks_as_rgba8(
            self.format.variant(),
            pw,
            ph,
            &self.levels[l],
            &mut rgba,
        );
        let full = RgbaImage::from_raw(pw, ph, rgba).unwrap_or_default();
        image::imageops::crop_imm(&full, 0, 0, w, h).to_image()
    }
}

/// Size of mip level `l` of a `width`×`height` texture.
pub fn level_size(width: u32, height: u32, l: u32) -> (u32, u32) {
    ((width >> l).max(1), (height >> l).max(1))
}

/// Whether any pixel is not fully opaque.
pub fn has_alpha(img: &RgbaImage) -> bool {
    img.pixels().any(|p| p[3] < 255)
}

/// How a material samples a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Use {
    /// Albedo, emission: sRGB when the import settings say so.
    Color,
    /// A normal map slot: linear, BC5 (whatever the Texture Type).
    Normal,
    /// Data maps (metallic, occlusion): linear.
    Linear,
}

impl Use {
    /// Sampled as sRGB colour.
    pub fn srgb(self, imp: &TextureImporter) -> bool {
        self == Self::Color && imp.is_srgb()
    }

    /// Holds normals (BC5).
    pub fn normals(self, imp: &TextureImporter) -> bool {
        self == Self::Normal || imp.texture_type == TextureType::NormalMap
    }

    /// The use a texture gets when nothing says otherwise (its Texture Type's).
    pub fn of(imp: &TextureImporter) -> Self {
        match imp.texture_type {
            TextureType::NormalMap => Self::Normal,
            _ if imp.is_srgb() => Self::Color,
            _ => Self::Linear,
        }
    }
}

/// The format a texture with these settings gets on `platform` when used as `usage` (None:
/// uncompressed).
pub fn choose(
    imp: &TextureImporter,
    platform: &str,
    img: &RgbaImage,
    usage: Use,
) -> Option<BcFormat> {
    let (w, h) = img.dimensions();
    if !w.is_multiple_of(4) || !h.is_multiple_of(4) {
        return None;
    }
    match imp.compression_for(platform) {
        TextureCompression::None => None,
        _ if usage.normals(imp) => Some(BcFormat::Bc5),
        TextureCompression::NormalQuality if has_alpha(img) => Some(BcFormat::Bc3),
        TextureCompression::NormalQuality => Some(BcFormat::Bc1),
        TextureCompression::HighQuality => Some(BcFormat::Bc7),
    }
}

/// Pad an image to whole 4×4 blocks by repeating its last row and column.
fn padded(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    if (pw, ph) == (w, h) {
        return img.clone();
    }
    RgbaImage::from_fn(pw, ph, |x, y| *img.get_pixel(x.min(w - 1), y.min(h - 1)))
}

/// Blocks of one image (any size; padded to whole blocks). Rows of blocks encode in parallel.
pub fn encode_level(img: &RgbaImage, format: BcFormat) -> Vec<u8> {
    encode_level_as(img, format, format.variant())
}

/// [`encode_level`] with the encoder settings of `variant` (a variant of `format`).
fn encode_level_as(img: &RgbaImage, format: BcFormat, variant: CompressionVariant) -> Vec<u8> {
    let p = padded(img);
    let (w, h) = p.dimensions();
    let row_bytes = w as usize / 4 * format.block_bytes();
    let mut out = vec![0u8; row_bytes * h as usize / 4];
    let src = p.as_raw();
    let stride = w as usize * 4;
    out.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(by, blocks)| {
            let rows = &src[by * 4 * stride..(by * 4 + 4) * stride];
            block_compression::encode::compress_rgba8(variant, rows, blocks, w, 4, stride as u32);
        });
    out
}

/// Encode an image and its mips (`srgb`: mips averaged in linear light).
pub fn encode(img: RgbaImage, format: BcFormat, srgb: bool, mips: bool) -> Compressed {
    let (width, height) = img.dimensions();
    let chain = if mips {
        crate::render::texture::mip_chain(img, srgb)
    } else {
        vec![img]
    };
    Compressed {
        format,
        srgb: srgb && format != BcFormat::Bc5 && format != BcFormat::Bc6h,
        width,
        height,
        levels: chain.iter().map(|l| encode_level(l, format)).collect(),
    }
}

/// BC6H blocks of an HDR image (RGBA half floats, row-major; alpha ignored), padded to whole
/// blocks; for HDR sources such as sky or emissive maps.
pub fn encode_hdr(width: u32, height: u32, rgba: &[half::f16]) -> Compressed {
    let (pw, ph) = (width.div_ceil(4) * 4, height.div_ceil(4) * 4);
    let mut src = vec![half::f16::ZERO; pw as usize * ph as usize * 4];
    for y in 0..ph {
        for x in 0..pw {
            let s = ((y.min(height - 1) * width + x.min(width - 1)) * 4) as usize;
            let d = ((y * pw + x) * 4) as usize;
            src[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    let variant = BcFormat::Bc6h.variant();
    let mut out = vec![0u8; variant.blocks_byte_size(pw, ph)];
    block_compression::encode::compress_rgba16(variant, &src, &mut out, pw, ph, pw * 4);
    Compressed {
        format: BcFormat::Bc6h,
        srgb: false,
        width,
        height,
        levels: vec![out],
    }
}

/// BC6H blocks decoded back to RGBA float (tests).
pub fn decode_hdr(c: &Compressed) -> Vec<f32> {
    let (bw, bh) = (c.width.div_ceil(4) as usize, c.height.div_ceil(4) as usize);
    let pw = bw * 4;
    let mut out = vec![1f32; pw * bh * 4 * 4];
    // Block by block: RGB floats, 12 per row of a block.
    for (i, block) in c.levels[0].as_chunks::<16>().0.iter().enumerate() {
        let (bx, by) = (i % bw, i / bw);
        let mut rgb = [0f32; 48];
        block_compression::decode::decode_block_bc6h_float(block, &mut rgb, 12, false);
        for y in 0..4 {
            for x in 0..4 {
                let d = ((by * 4 + y) * pw + bx * 4 + x) * 4;
                out[d..d + 3].copy_from_slice(&rgb[y * 12 + x * 3..y * 12 + x * 3 + 3]);
            }
        }
    }
    out
}

// ---- cache ---------------------------------------------------------------------------------

/// How textures get their compressed blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Never compressed.
    Off = 0,
    /// Read from the cache only (players: builds ship the blocks).
    CacheOnly = 1,
    /// Encoded on the spot when missing (tests, builds).
    Blocking = 2,
    /// Encoded on a worker when missing; uncompressed until then (the editor).
    Background = 3,
}

static MODE: AtomicU8 = AtomicU8::new(Mode::Blocking as u8);

pub fn set_mode(mode: Mode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        0 => Mode::Off,
        1 => Mode::CacheOnly,
        3 => Mode::Background,
        _ => Mode::Blocking,
    }
}

/// Cache keys being encoded on workers, and the textures whose blocks became ready.
static PENDING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
static FINISHED: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);

/// Textures whose compressed blocks became ready since the last call (they reload).
pub fn take_finished() -> Vec<String> {
    std::mem::take(&mut *FINISHED.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Whether textures are still being encoded in the background.
pub fn busy() -> bool {
    !PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// Cache key of a texture: its file's bytes and what changes the blocks.
pub fn cache_key(
    source: &[u8],
    imp: &TextureImporter,
    platform: &str,
    size: [u32; 2],
    usage: Use,
) -> String {
    let mut h = Sha256::new();
    h.update(ENCODER_VERSION.to_le_bytes());
    h.update(source);
    let settings = format!(
        "{:?}|{}|{}|{}x{}|{}",
        imp.compression_for(platform),
        usage.normals(imp),
        usage.srgb(imp),
        size[0],
        size[1],
        imp.generate_mip_maps
    );
    h.update(settings.as_bytes());
    h.finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Project-relative path of a cache entry.
pub fn cache_rel(key: &str) -> String {
    format!("{LIBRARY_DIR}/{key}.mtc")
}

/// The project's cache folder (none without a project).
fn cache_root() -> Option<PathBuf> {
    let root = assets::absolute("");
    (!root.as_os_str().is_empty()).then_some(root)
}

fn read_cache(key: &str) -> Option<Compressed> {
    cached(&cache_root()?, key)
}

fn write_cache(key: &str, c: &Compressed) -> Option<PathBuf> {
    store(&cache_root()?, key, c)
}

/// The cached blocks under project `root` (read through the asset file system, so a build's
/// pak serves them).
pub fn cached(root: &Path, key: &str) -> Option<Compressed> {
    let bytes = crate::vfs::read(root.join(cache_rel(key))).ok()?;
    Compressed::from_file(&bytes)
}

/// Save blocks in the cache under project `root`; returns the file.
pub fn store(root: &Path, key: &str, c: &Compressed) -> Option<PathBuf> {
    let path = root.join(cache_rel(key));
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Write then rename, so a reader never sees half a file (one temporary file per write:
    // two threads may store the same entry).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, c.to_file()).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

/// What a texture uploads.
pub enum Upload {
    Rgba(RgbaImage),
    Compressed(Compressed),
}

/// The size a texture decodes to: its file's size fitted to Max Size and the quality level.
fn target_size(rel: &str, imp: &TextureImporter, platform: &str) -> Option<[u32; 2]> {
    let [w, h] = crate::render::texture::image_size(&assets::absolute(rel))?;
    let limit = crate::quality::active().texture_size_limit(imp.max_size_for(platform));
    Some(fit_size(w, h, limit))
}

/// [`crate::render::texture::fit`]'s result size.
pub fn fit_size(w: u32, h: u32, max: u32) -> [u32; 2] {
    if w.max(h) <= max {
        return [w, h];
    }
    let s = max as f64 / w.max(h) as f64;
    [
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    ]
}

/// What the GPU gets for texture `rel` used as `usage`: cached blocks when the device supports
/// BC (`bc`), the settings ask for compression and the cache has them (encoding them now, in
/// the background or not at all, by [`mode`]); else the decoded image.
/// [`texture_upload`] that never encodes: the cached blocks when there are some, else the
/// decoded image (reading textures ahead on a background thread, where an encode would hold a
/// worker for seconds).
pub fn texture_upload_cached(rel: &str, usage: Use, bc: bool) -> Result<Upload, String> {
    if !bc || mode() == Mode::Off || super::builtin::is_builtin(rel) {
        return super::load_texture(rel).map(Upload::Rgba);
    }
    let imp = super::importer(rel);
    let platform = super::import::current_platform();
    if imp.compression_for(platform) != TextureCompression::None
        && let Some(size) = target_size(rel, &imp, platform)
        && size[0].is_multiple_of(4)
        && size[1].is_multiple_of(4)
    {
        let source = crate::vfs::read(assets::absolute(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let key = cache_key(&source, &imp, platform, size, usage);
        if let Some(c) = read_cache(&key) {
            return Ok(Upload::Compressed(c));
        }
    }
    super::load_texture(rel).map(Upload::Rgba)
}

pub fn texture_upload(rel: &str, usage: Use, bc: bool) -> Result<Upload, String> {
    let m = mode();
    if !bc || m == Mode::Off || super::builtin::is_builtin(rel) {
        return super::load_texture(rel).map(Upload::Rgba);
    }
    let imp = super::importer(rel);
    let platform = super::import::current_platform();
    if imp.compression_for(platform) == TextureCompression::None {
        return super::load_texture(rel).map(Upload::Rgba);
    }
    let Some(size) = target_size(rel, &imp, platform) else {
        return super::load_texture(rel).map(Upload::Rgba);
    };
    if !size[0].is_multiple_of(4) || !size[1].is_multiple_of(4) {
        return super::load_texture(rel).map(Upload::Rgba);
    }
    let source = crate::vfs::read(assets::absolute(rel)).map_err(|e| format!("{rel}: {e}"))?;
    let key = cache_key(&source, &imp, platform, size, usage);
    if let Some(c) = read_cache(&key) {
        return Ok(Upload::Compressed(c));
    }
    let img = super::load_texture(rel)?;
    let srgb = usage.srgb(&imp);
    let mips = imp.generate_mip_maps;
    match m {
        Mode::Blocking => match choose(&imp, platform, &img, usage) {
            Some(format) => {
                let c = encode(img, format, srgb, mips);
                write_cache(&key, &c);
                Ok(Upload::Compressed(c))
            }
            None => Ok(Upload::Rgba(img)),
        },
        Mode::Background => {
            let pending = || PENDING.lock().unwrap_or_else(|e| e.into_inner());
            let fresh = pending().insert(key.clone());
            match choose(&imp, platform, &img, usage) {
                Some(format) if fresh => {
                    let (rel, copy) = (rel.to_owned(), img.clone());
                    rayon::spawn(move || {
                        let c = encode(copy, format, srgb, mips);
                        write_cache(&key, &c);
                        PENDING
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .remove(&key);
                        FINISHED.lock().unwrap_or_else(|e| e.into_inner()).push(rel);
                    });
                }
                _ if fresh => {
                    pending().remove(&key);
                }
                _ => {}
            }
            Ok(Upload::Rgba(img))
        }
        Mode::CacheOnly | Mode::Off => Ok(Upload::Rgba(img)),
    }
}

/// What [`bake`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Baked {
    /// The texture's cache entries (project-relative), empty when it stays uncompressed.
    pub entries: Vec<String>,
    /// Entries encoded now (the rest were already cached).
    pub encoded: usize,
}

/// Make sure the cache of project `root` holds the blocks of texture `rel` for `platform`, for
/// each of `uses` (how materials sample it; empty = its Texture Type's use), at every size
/// the project's quality levels load it at. Builds pack them; a texture sampled some other
/// way falls back to uncompressed in players.
pub fn bake(
    root: &Path,
    rel: &str,
    platform: &str,
    quality: &crate::quality::QualitySettings,
    uses: &[Use],
) -> Result<Baked, String> {
    let imp = TextureImporter::load(&root.join(rel));
    bake_with(root, rel, platform, quality, uses, &imp)
}

/// [`bake`] with import settings `imp` instead of the texture's `.meta` (a Build Profile's
/// texture overrides; the build packs the matching `.meta`).
pub fn bake_with(
    root: &Path,
    rel: &str,
    platform: &str,
    quality: &crate::quality::QualitySettings,
    uses: &[Use],
    imp: &TextureImporter,
) -> Result<Baked, String> {
    let mut out = Baked::default();
    if super::builtin::is_builtin(rel) {
        return Ok(out);
    }
    let abs = root.join(rel);
    let imp = imp.clone();
    if imp.compression_for(platform) == TextureCompression::None {
        return Ok(out);
    }
    let Some([w, h]) = crate::render::texture::image_size(&abs) else {
        return Ok(out);
    };
    let source = crate::vfs::read(&abs).map_err(|e| format!("{rel}: {e}"))?;
    let mut sizes: Vec<[u32; 2]> = quality
        .levels
        .iter()
        .map(|q| fit_size(w, h, q.texture_size_limit(imp.max_size_for(platform))))
        .collect();
    sizes.sort();
    sizes.dedup();
    let mut uses = if uses.is_empty() {
        vec![Use::of(&imp)]
    } else {
        uses.to_vec()
    };
    // Uses that give the same blocks share an entry.
    let mut seen = HashSet::new();
    uses.retain(|u| seen.insert((u.srgb(&imp), u.normals(&imp))));
    let mut decoded: Option<RgbaImage> = None;
    for usage in uses {
        for size in &sizes {
            if !size[0].is_multiple_of(4) || !size[1].is_multiple_of(4) {
                continue;
            }
            let key = cache_key(&source, &imp, platform, *size, usage);
            if cached(root, &key).is_none() {
                let full = match &decoded {
                    Some(d) => d.clone(),
                    None => {
                        let d = crate::render::texture::load_image(&abs)?;
                        decoded = Some(d.clone());
                        d
                    }
                };
                let img = crate::render::texture::fit(full, size[0].max(size[1]));
                let Some(format) = choose(&imp, platform, &img, usage) else {
                    continue;
                };
                let c = encode(img, format, usage.srgb(&imp), imp.generate_mip_maps);
                store(root, &key, &c)
                    .ok_or_else(|| format!("{rel}: cannot write the texture cache"))?;
                out.encoded += 1;
            }
            let entry = cache_rel(&key);
            if !out.entries.contains(&entry) {
                out.entries.push(entry);
            }
        }
    }
    Ok(out)
}

pub mod generated;

#[cfg(test)]
mod meadow;
#[cfg(test)]
mod tests;
