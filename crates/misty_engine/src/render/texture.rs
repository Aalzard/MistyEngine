//! Material textures: PNG/JPG decoding, CPU mip generation, upload, and a cache of textures and
//! per-material bind groups keyed by project-relative path.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

use image::RgbaImage;
use wgpu::util::DeviceExt;

use crate::assets;
use crate::sprites::{self, FilterMode, WrapMode};

/// Largest side ever decoded (the largest import Max Size).
const MAX_SIZE: u32 = 8192;

/// Number of mip levels for a full chain down to 1×1.
pub fn mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

fn srgb_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Half-size image by a 2×2 box filter (colour averaged in linear light when `srgb`).
fn downsample(src: &RgbaImage, srgb: bool) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let (w, h) = ((sw / 2).max(1), (sh / 2).max(1));
    let lut = srgb_lut();
    RgbaImage::from_fn(w, h, |x, y| {
        let mut sum = [0.0f32; 4];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = src.get_pixel((x * 2 + dx).min(sw - 1), (y * 2 + dy).min(sh - 1));
            for c in 0..4 {
                sum[c] += if srgb && c < 3 {
                    lut[p[c] as usize]
                } else {
                    p[c] as f32 / 255.0
                };
            }
        }
        image::Rgba(std::array::from_fn(|c| {
            let v = sum[c] * 0.25;
            let v = if srgb && c < 3 { linear_to_srgb(v) } else { v };
            (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        }))
    })
}

/// The image followed by its mips down to 1×1.
pub fn mip_chain(image: RgbaImage, srgb: bool) -> Vec<RgbaImage> {
    let mut chain = vec![image];
    loop {
        let last = chain.last().expect("non-empty");
        if last.width() == 1 && last.height() == 1 {
            break chain;
        }
        let next = downsample(last, srgb);
        chain.push(next);
    }
}

/// Decode a PNG/JPG file as RGBA8 (scaled down if larger than [`MAX_SIZE`]).
pub fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let bytes = crate::vfs::read(path).map_err(|e| err(&e))?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| err(&e))?
        .into_rgba8();
    Ok(fit(img, MAX_SIZE))
}

/// Pixel size of an image file without decoding it.
pub fn image_size(path: &Path) -> Option<[u32; 2]> {
    let bytes = crate::vfs::read(path).ok()?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    Some([w, h])
}

/// Scale `img` down (Lanczos, keeping the aspect ratio) so its larger side is at most `max`.
pub fn fit(img: RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w.max(h) <= max {
        return img;
    }
    let s = max as f64 / w.max(h) as f64;
    image::imageops::resize(
        &img,
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
        image::imageops::FilterType::Lanczos3,
    )
}

/// Upload an image with generated mips.
pub(crate) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: RgbaImage,
    srgb: bool,
) -> wgpu::TextureView {
    upload_with(device, queue, label, image, srgb, true)
}

/// Upload an image, with or without generated mips (import setting Generate Mip Maps).
pub(crate) fn upload_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: RgbaImage,
    srgb: bool,
    mips: bool,
) -> wgpu::TextureView {
    let (w, h) = image.dimensions();
    let chain = if mips {
        mip_chain(image, srgb)
    } else {
        vec![image]
    };
    let data: Vec<u8> = chain
        .iter()
        .flat_map(|m| m.as_raw().iter().copied())
        .collect();
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: chain.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    texture.create_view(&Default::default())
}

/// Upload block-compressed levels (see [`sprites::compress`]).
pub(crate) fn upload_compressed(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    c: &sprites::compress::Compressed,
) -> wgpu::TextureView {
    let data: Vec<u8> = c.levels.concat();
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: c.width,
                height: c.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: c.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: c.wgpu_format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    texture.create_view(&Default::default())
}

/// An empty texture for renderer-made slices (`layers` of the size and format of `first`),
/// written by [`write_made`].
pub(crate) fn made_texture(
    device: &wgpu::Device,
    label: &str,
    first: &sprites::compress::generated::Made,
    layers: u32,
) -> wgpu::Texture {
    let (width, height) = first.size();
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: first.level_count(),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: first.format(),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Write slice `layer` of `texture` (every level of `m`, blocks or RGBA8); returns the bytes.
pub(crate) fn write_made(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    layer: u32,
    m: &sprites::compress::generated::Made,
) -> usize {
    let format = m.format();
    let (bw, bh) = format.block_dimensions();
    let block = format.block_copy_size(None).unwrap_or(4);
    let (width, height) = m.size();
    let mut bytes = 0;
    for level in 0..m.level_count() {
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        // Whole blocks (the physical size of a level smaller than a block).
        let (cols, rows) = (w.div_ceil(bw), h.div_ceil(bh));
        let data = m.level(level as usize);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cols * block),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width: cols * bw,
                height: rows * bh,
                depth_or_array_layers: 1,
            },
        );
        bytes += data.len();
    }
    bytes
}

/// Upload renderer-made slices (all of one size and format) as a texture (`array`: viewed as a
/// 2D array); returns the view and the GPU bytes.
pub(crate) fn upload_made(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    layers: &[sprites::compress::generated::Made],
    array: bool,
) -> (wgpu::TextureView, u64) {
    let texture = made_texture(device, label, &layers[0], layers.len() as u32);
    let mut bytes = 0;
    for (i, m) in layers.iter().enumerate() {
        bytes += write_made(queue, &texture, i as u32, m) as u64;
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(if array {
            wgpu::TextureViewDimension::D2Array
        } else {
            wgpu::TextureViewDimension::D2
        }),
        ..Default::default()
    });
    (view, bytes)
}

/// Sampler state from a texture's import settings (Filter Mode, Wrap Mode, Aniso Level) and
/// the active quality level's Anisotropic Textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerKey {
    pub filter: FilterMode,
    pub wrap: WrapMode,
    /// Anisotropy clamp, 1 = off.
    pub anisotropy: u16,
}

impl Default for SamplerKey {
    fn default() -> Self {
        Self {
            filter: FilterMode::default(),
            wrap: WrapMode::default(),
            anisotropy: 1,
        }
    }
}

impl SamplerKey {
    /// The sampler of a texture by project path (trilinear + repeat without import settings).
    pub fn of(rel: Option<&str>) -> Self {
        let imp = rel.map(sprites::importer).unwrap_or_default();
        Self {
            filter: imp.filter_mode,
            wrap: imp.wrap_mode,
            anisotropy: crate::quality::active().anisotropy(imp.aniso_level),
        }
    }

    pub(crate) fn create(self, device: &wgpu::Device) -> wgpu::Sampler {
        let address = match self.wrap {
            WrapMode::Repeat => wgpu::AddressMode::Repeat,
            WrapMode::Clamp => wgpu::AddressMode::ClampToEdge,
            WrapMode::Mirror => wgpu::AddressMode::MirrorRepeat,
        };
        let (filter, mip) = match self.filter {
            FilterMode::Nearest => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest),
            FilterMode::Bilinear => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest),
            FilterMode::Trilinear => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear),
        };
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty material sampler"),
            address_mode_u: address,
            address_mode_v: address,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: mip,
            // Anisotropy needs linear filtering in every direction (trilinear only).
            anisotropy_clamp: if self.filter == FilterMode::Trilinear {
                self.anisotropy.clamp(1, 16)
            } else {
                1
            },
            ..Default::default()
        })
    }
}

/// Colour maps are sRGB; normal maps hold vectors and stay linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureKind {
    Color,
    Normal,
    /// Other data maps (metallic/smoothness, occlusion): linear, white when unset.
    Linear,
}

/// The textures of one material (group 2 of the mesh pipeline); `None` = default texture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextureSet {
    pub albedo: Option<String>,
    pub normal: Option<String>,
    pub metallic: Option<String>,
    pub occlusion: Option<String>,
    pub emission: Option<String>,
}

impl TextureSet {
    /// Albedo and normal only (particles, terrains).
    pub fn pair(albedo: Option<&str>, normal: Option<&str>) -> Self {
        Self {
            albedo: albedo.map(str::to_owned),
            normal: normal.map(str::to_owned),
            ..Default::default()
        }
    }
}

/// 1×1 fallbacks: white sRGB, flat normal, white linear.
struct Defaults {
    white: wgpu::TextureView,
    flat: wgpu::TextureView,
    white_linear: wgpu::TextureView,
}

struct Cached {
    /// Modification times of the image and its `.meta` import settings.
    modified: (Option<SystemTime>, Option<SystemTime>),
    /// None when the file could not be read (the default texture is used).
    view: Option<wgpu::TextureView>,
}

/// One loaded texture (see [`TextureCache::entries`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TextureEntry {
    pub path: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub mips: u32,
    /// Material texture sets using it.
    pub refs: u32,
}

impl TextureEntry {
    /// GPU bytes of the whole mip chain (block-compressed formats by their block size).
    pub fn bytes(&self) -> u64 {
        texture_bytes(self.format, self.width, self.height, self.mips, 1)
    }
}

/// Bytes of a `width`×`height`×`layers` texture with `mips` levels in `format`.
pub fn texture_bytes(
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    mips: u32,
    layers: u32,
) -> u64 {
    let (bw, bh) = format.block_dimensions();
    let block = format
        .block_copy_size(None)
        .or_else(|| format.block_copy_size(Some(wgpu::TextureAspect::DepthOnly)))
        .unwrap_or(4) as u64;
    (0..mips.max(1))
        .map(|m| {
            let (w, h) = ((width >> m).max(1), (height >> m).max(1));
            w.div_ceil(bw) as u64 * h.div_ceil(bh) as u64 * block
        })
        .sum::<u64>()
        * layers.max(1) as u64
}

/// Loaded textures and one bind group per material texture set.
pub struct TextureCache {
    textures: HashMap<(String, TextureKind), Cached>,
    sets: HashMap<TextureSet, wgpu::BindGroup>,
    /// Group 2 of the Standard feature variants (render/material_v2.rs), by textures and values.
    lit_sets: HashMap<super::material_v2::LitSet, wgpu::BindGroup>,
    /// Created on first use.
    defaults: Option<Defaults>,
    /// Bilinear repeat (textures without import settings).
    sampler: wgpu::Sampler,
    /// Samplers for other import settings, created on first use.
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    last_poll: Option<Instant>,
    warnings: Vec<String>,
    /// [`crate::quality::generation`] the textures were loaded for.
    quality: u64,
    /// Video frames and other textures published at runtime (`video://…`).
    runtime: super::runtime_tex::RuntimeTextures,
}

impl TextureCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = SamplerKey {
            anisotropy: 8,
            ..Default::default()
        }
        .create(device);
        Self {
            textures: HashMap::new(),
            sets: HashMap::new(),
            lit_sets: HashMap::new(),
            defaults: None,
            sampler,
            samplers: HashMap::new(),
            last_poll: None,
            warnings: Vec::new(),
            quality: crate::quality::generation(),
            runtime: Default::default(),
        }
    }

    /// Upload runtime textures (video frames) published since the last frame.
    pub fn sync_runtime(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.runtime.sync(device, queue) {
            self.sets.clear();
            self.lit_sets.clear();
        }
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rel: &str,
        kind: TextureKind,
    ) -> Option<wgpu::TextureView> {
        if crate::video::textures::is_runtime(rel) {
            return Some(self.runtime.view(device, queue, rel));
        }
        let key = (rel.to_owned(), kind);
        if let Some(c) = self.textures.get(&key) {
            return c.view.clone();
        }
        let imp = sprites::importer(rel);
        let srgb = kind == TextureKind::Color && imp.is_srgb();
        // Block-compressed (cached in Library/TextureCache) when the device supports BC.
        let bc = device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let usage = match kind {
            TextureKind::Color => sprites::compress::Use::Color,
            TextureKind::Normal => sprites::compress::Use::Normal,
            TextureKind::Linear => sprites::compress::Use::Linear,
        };
        BC.store(bc, std::sync::atomic::Ordering::Relaxed);
        let upload = match take_ahead(rel, kind) {
            Some(up) => Ok(up),
            None => sprites::compress::texture_upload(rel, usage, bc),
        };
        let view = match upload {
            Ok(sprites::compress::Upload::Rgba(img)) => Some(upload_with(
                device,
                queue,
                rel,
                img,
                srgb,
                imp.generate_mip_maps,
            )),
            Ok(sprites::compress::Upload::Compressed(c)) => {
                Some(upload_compressed(device, queue, rel, &c))
            }
            Err(e) => {
                self.warnings.push(format!("Could not load texture {e}"));
                None
            }
        };
        self.textures.insert(
            key,
            Cached {
                modified: stamps(rel),
                view: view.clone(),
            },
        );
        view
    }

    /// A texture by project path (`None` when it can't be read), loaded on first use.
    pub(crate) fn texture_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rel: &str,
        kind: TextureKind,
    ) -> Option<wgpu::TextureView> {
        self.texture(device, queue, rel, kind)
    }

    /// Make sure the bind group for this texture pair exists.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        albedo: Option<&str>,
        normal: Option<&str>,
    ) {
        self.prepare_set(device, queue, layout, &TextureSet::pair(albedo, normal));
    }

    /// Make sure the bind group for a material's textures exists.
    pub fn prepare_set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        set: &TextureSet,
    ) {
        // A new quality level can change texture sizes and anisotropy: reload everything.
        let quality = crate::quality::generation();
        if quality != self.quality {
            self.quality = quality;
            self.textures.clear();
            self.sets.clear();
            self.lit_sets.clear();
            self.samplers.clear();
        }
        if self.sets.contains_key(set) {
            return;
        }
        let (albedo, normal) =
            self.views(device, queue, set.albedo.as_deref(), set.normal.as_deref());
        let d = self.defaults(device, queue);
        let (white, white_linear) = (d.white.clone(), d.white_linear.clone());
        let mut load = |rel: &Option<String>, kind, fallback: &wgpu::TextureView| {
            rel.as_deref()
                .and_then(|r| self.texture(device, queue, r, kind))
                .unwrap_or_else(|| fallback.clone())
        };
        let metallic = load(&set.metallic, TextureKind::Linear, &white_linear);
        let occlusion = load(&set.occlusion, TextureKind::Linear, &white_linear);
        let emission = load(&set.emission, TextureKind::Color, &white);
        // The albedo texture's import settings pick the sampler of the whole set.
        let key = SamplerKey::of(set.albedo.as_deref());
        let sampler = self
            .samplers
            .entry(key)
            .or_insert_with(|| key.create(device));
        let view = |binding, v| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(v),
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty material textures"),
            layout,
            entries: &[
                view(0, &albedo),
                view(1, &normal),
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                view(3, &metallic),
                view(4, &occlusion),
                view(5, &emission),
            ],
        });
        self.sets.insert(set.clone(), group);
    }

    /// The 1×1 fallback textures (created on first use).
    fn defaults(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> &Defaults {
        self.defaults.get_or_insert_with(|| {
            let pixel = |label, rgba: [u8; 4], format| {
                device
                    .create_texture_with_data(
                        queue,
                        &wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: 1,
                                height: 1,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::util::TextureDataOrder::LayerMajor,
                        &rgba,
                    )
                    .create_view(&Default::default())
            };
            Defaults {
                white: pixel("misty white", [255; 4], wgpu::TextureFormat::Rgba8UnormSrgb),
                flat: pixel(
                    "misty flat normal",
                    [128, 128, 255, 255],
                    wgpu::TextureFormat::Rgba8Unorm,
                ),
                white_linear: pixel(
                    "misty white linear",
                    [255; 4],
                    wgpu::TextureFormat::Rgba8Unorm,
                ),
            }
        })
    }

    /// Albedo and normal views for a pair (1×1 white / flat normal when unset or unreadable).
    pub(crate) fn views(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        albedo: Option<&str>,
        normal: Option<&str>,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        let d = self.defaults(device, queue);
        let (white, flat) = (d.white.clone(), d.flat.clone());
        let albedo_view = albedo
            .and_then(|a| self.texture(device, queue, a, TextureKind::Color))
            .unwrap_or(white);
        let normal_view = normal
            .and_then(|n| self.texture(device, queue, n, TextureKind::Normal))
            .unwrap_or(flat);
        (albedo_view, normal_view)
    }

    /// Repeating, mipmapped sampler used by materials.
    pub(crate) fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Bind group prepared for this pair.
    pub fn get(&self, albedo: Option<&str>, normal: Option<&str>) -> Option<&wgpu::BindGroup> {
        self.sets.get(&TextureSet::pair(albedo, normal))
    }

    /// Bind group prepared for this material texture set.
    pub fn get_set(&self, set: &TextureSet) -> Option<&wgpu::BindGroup> {
        self.sets.get(set)
    }

    /// Make sure group 2 of a feature variant exists: the Standard textures (as
    /// [`Self::prepare_set`]), the feature values and the feature maps.
    pub fn prepare_lit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        set: &super::material_v2::LitSet,
    ) {
        // (A new quality level clears these in `prepare_set`, which the renderer calls first.)
        if self.lit_sets.contains_key(set) {
            return;
        }
        let b = &set.base;
        let (albedo, normal) = self.views(device, queue, b.albedo.as_deref(), b.normal.as_deref());
        let d = self.defaults(device, queue);
        let (white, white_linear, flat) = (d.white.clone(), d.white_linear.clone(), d.flat.clone());
        let mut load = |rel: &Option<String>, kind, fallback: &wgpu::TextureView| {
            rel.as_deref()
                .and_then(|r| self.texture(device, queue, r, kind))
                .unwrap_or_else(|| fallback.clone())
        };
        let metallic = load(&b.metallic, TextureKind::Linear, &white_linear);
        let occlusion = load(&b.occlusion, TextureKind::Linear, &white_linear);
        let emission = load(&b.emission, TextureKind::Color, &white);
        let [d_albedo, d_normal, d_mask, height, coat_normal, thickness] = &set.maps;
        let feature_views = [
            load(d_albedo, TextureKind::Color, &white),
            load(d_normal, TextureKind::Normal, &flat),
            load(d_mask, TextureKind::Linear, &white_linear),
            load(height, TextureKind::Linear, &white_linear),
            load(coat_normal, TextureKind::Normal, &flat),
            load(thickness, TextureKind::Linear, &white_linear),
        ];
        let key = SamplerKey::of(b.albedo.as_deref());
        let sampler = self
            .samplers
            .entry(key)
            .or_insert_with(|| key.create(device));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty material features"),
            contents: bytemuck::cast_slice(&set.values),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = |binding, v| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(v),
        };
        let mut entries = vec![
            view(0, &albedo),
            view(1, &normal),
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            view(3, &metallic),
            view(4, &occlusion),
            view(5, &emission),
            wgpu::BindGroupEntry {
                binding: 6,
                resource: uniform.as_entire_binding(),
            },
        ];
        for (i, v) in feature_views.iter().enumerate() {
            entries.push(view(7 + i as u32, v));
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty material lit"),
            layout,
            entries: &entries,
        });
        self.lit_sets.insert(set.clone(), group);
    }

    /// Group 2 prepared for a feature variant's set.
    pub fn get_lit(&self, set: &super::material_v2::LitSet) -> Option<&wgpu::BindGroup> {
        self.lit_sets.get(set)
    }

    /// Loaded textures and their GPU bytes (by format: block-compressed ones by block size).
    pub fn memory(&self) -> (usize, u64) {
        let views = self.textures.values().filter_map(|c| c.view.as_ref());
        views.fold((0, 0), |(n, bytes), v| {
            let t = v.texture();
            let b = texture_bytes(t.format(), t.width(), t.height(), t.mip_level_count(), 1);
            (n + 1, bytes + b)
        })
    }

    /// Loaded textures for the Memory Profiler: (path, kind label, size, format, mip count,
    /// material texture sets that use it).
    pub fn entries(&self) -> Vec<TextureEntry> {
        let mut out: Vec<TextureEntry> = self
            .textures
            .iter()
            .filter_map(|((rel, kind), c)| {
                let t = c.view.as_ref()?.texture();
                let refs = self
                    .sets
                    .keys()
                    .filter(|s| {
                        [&s.albedo, &s.normal, &s.metallic, &s.occlusion, &s.emission]
                            .into_iter()
                            .any(|p| p.as_deref() == Some(rel.as_str()))
                    })
                    .count() as u32
                    + self
                        .lit_sets
                        .keys()
                        .filter(|s| s.maps.iter().any(|p| p.as_deref() == Some(rel.as_str())))
                        .count() as u32;
                Some(TextureEntry {
                    path: rel.clone(),
                    kind: format!("{kind:?}"),
                    width: t.width(),
                    height: t.height(),
                    format: t.format(),
                    mips: t.mip_level_count(),
                    refs,
                })
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(&b.kind)));
        out
    }

    /// Whether a texture loaded successfully (false for files that failed to load).
    pub fn is_loaded(&self, rel: &str, kind: TextureKind) -> bool {
        self.textures
            .get(&(rel.to_owned(), kind))
            .is_some_and(|c| c.view.is_some())
    }

    /// Drop textures whose files changed (checked at most twice a second); they reload on next
    /// use. Returns the changed paths (each once, whatever kinds of it were loaded).
    pub fn poll_changes(&mut self) -> Vec<String> {
        let now = Instant::now();
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
        {
            return Vec::new();
        }
        self.last_poll = Some(now);
        let mut changed = Vec::new();
        // Textures whose compressed blocks were made in the background reload with them.
        let compressed = sprites::compress::take_finished();
        let before = self.textures.len();
        self.textures
            .retain(|(rel, _), _| !compressed.contains(rel));
        if self.textures.len() != before {
            self.sets.clear();
            self.lit_sets.clear();
        }
        self.textures.retain(|(rel, _), c| {
            let same = c.modified == stamps(rel);
            if !same && !changed.contains(rel) {
                changed.push(rel.clone());
            }
            same
        });
        if !changed.is_empty() {
            self.sets.clear();
            self.lit_sets.clear();
        }
        changed
    }

    /// Load errors since the last call (for the Console).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    crate::vfs::modified(path)
}

/// Modification times of a texture and its import settings (built-ins never change).
fn stamps(rel: &str) -> (Option<SystemTime>, Option<SystemTime>) {
    if sprites::builtin::is_builtin(rel) {
        return (None, None);
    }
    let abs = assets::absolute(rel);
    (
        mtime(&abs),
        mtime(&sprites::TextureImporter::meta_path(&abs)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_reaches_one_pixel() {
        assert_eq!(mip_count(1, 1), 1);
        assert_eq!(mip_count(256, 256), 9);
        assert_eq!(mip_count(256, 64), 9);
        assert_eq!(mip_count(300, 7), 9);
        let img = RgbaImage::from_pixel(64, 16, image::Rgba([255, 0, 0, 255]));
        let chain = mip_chain(img, true);
        assert_eq!(chain.len() as u32, mip_count(64, 16));
        let sizes: Vec<_> = chain.iter().map(|m| m.dimensions()).collect();
        assert_eq!(sizes[1], (32, 8));
        assert_eq!(*sizes.last().unwrap(), (1, 1));
        assert_eq!(chain.last().unwrap().get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn max_size_downscales_keeping_the_aspect_ratio() {
        let img = RgbaImage::from_pixel(512, 128, image::Rgba([10, 20, 30, 255]));
        let small = fit(img.clone(), 128);
        assert_eq!(small.dimensions(), (128, 32));
        assert_eq!(small.get_pixel(5, 5).0, [10, 20, 30, 255]);
        assert_eq!(fit(img, 1024).dimensions(), (512, 128), "never upscaled");
        // Texture Quality halves on top of the import Max Size.
        let half = crate::quality::QualityLevel {
            texture_quality: crate::quality::TextureQuality::Half,
            ..Default::default()
        };
        assert_eq!(half.texture_size_limit(256), 128);
    }

    #[test]
    fn filter_modes_and_defaults() {
        assert_eq!(SamplerKey::default().anisotropy, 1);
        assert_eq!(SamplerKey::of(None).filter, FilterMode::Trilinear);
        assert_eq!(SamplerKey::of(None).wrap, WrapMode::Repeat);
    }

    #[test]
    fn srgb_mips_average_in_linear_light() {
        // Black/white checker: linear average is 0.5, which is ~188 in sRGB (not 128).
        let img = RgbaImage::from_fn(2, 2, |x, y| {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            image::Rgba([v, v, v, 255])
        });
        let srgb = mip_chain(img.clone(), true);
        let px = srgb[1].get_pixel(0, 0).0;
        assert!((186..=190).contains(&px[0]), "{px:?}");
        assert_eq!(mip_chain(img, false)[1].get_pixel(0, 0).0[0], 128);
    }
}

// ---- reading ahead (World Partition cells) ------------------------------------------------

/// Textures decoded (or read from the compressed cache) ahead on a background thread, waiting
/// for the renderer's first use; and the ones some renderer already loaded.
#[derive(Default)]
struct Ahead {
    ready: HashMap<(String, TextureKind), sprites::compress::Upload>,
    loaded: std::collections::HashSet<(String, TextureKind)>,
}

fn ahead() -> std::sync::MutexGuard<'static, Ahead> {
    static AHEAD: OnceLock<std::sync::Mutex<Ahead>> = OnceLock::new();
    AHEAD
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Whether the GPU takes block-compressed textures (set by the first renderer).
static BC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Most textures waiting at once (beyond that, nothing more is read ahead).
const AHEAD_MAX: usize = 48;

/// Read a material texture ahead (on the calling thread, a background one): its first draw
/// then only uploads it instead of decoding a JPEG / PNG on the render thread.
pub fn prefetch(rel: &str, kind: TextureKind) {
    let key = (rel.to_owned(), kind);
    {
        let a = ahead();
        if a.loaded.contains(&key) || a.ready.contains_key(&key) || a.ready.len() >= AHEAD_MAX {
            return;
        }
    }
    let usage = match kind {
        TextureKind::Color => sprites::compress::Use::Color,
        TextureKind::Normal => sprites::compress::Use::Normal,
        TextureKind::Linear => sprites::compress::Use::Linear,
    };
    let bc = BC.load(std::sync::atomic::Ordering::Relaxed);
    // In the background mode the encode is handed to a worker as usual; otherwise only cached
    // blocks or the decoded image (a blocking encode would hold this worker for seconds).
    let up = if sprites::compress::mode() == sprites::compress::Mode::Background {
        sprites::compress::texture_upload(rel, usage, bc)
    } else {
        sprites::compress::texture_upload_cached(rel, usage, bc)
    };
    if let Ok(up) = up {
        ahead().ready.insert(key, up);
    }
}

/// The textures of a material asset, read ahead.
pub fn prefetch_material(rel: &str) {
    let Ok(m) = crate::assets::Material::load(&assets::absolute(rel)) else {
        return;
    };
    for (path, kind) in [
        (&m.albedo_map, TextureKind::Color),
        (&m.normal_map, TextureKind::Normal),
        (&m.metallic_map, TextureKind::Linear),
        (&m.occlusion_map, TextureKind::Linear),
        (&m.emission_map, TextureKind::Color),
    ] {
        if let Some(p) = path {
            prefetch(p, kind);
        }
    }
}

/// A texture read ahead for this use, if any (the renderer marks it loaded either way).
fn take_ahead(rel: &str, kind: TextureKind) -> Option<sprites::compress::Upload> {
    let key = (rel.to_owned(), kind);
    let mut a = ahead();
    let up = a.ready.remove(&key);
    a.loaded.insert(key);
    up
}
