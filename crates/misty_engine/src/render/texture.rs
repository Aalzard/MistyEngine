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

/// Loaded textures and one bind group per material texture set.
pub struct TextureCache {
    textures: HashMap<(String, TextureKind), Cached>,
    sets: HashMap<TextureSet, wgpu::BindGroup>,
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
            defaults: None,
            sampler,
            samplers: HashMap::new(),
            last_poll: None,
            warnings: Vec::new(),
            quality: crate::quality::generation(),
        }
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rel: &str,
        kind: TextureKind,
    ) -> Option<wgpu::TextureView> {
        let key = (rel.to_owned(), kind);
        if let Some(c) = self.textures.get(&key) {
            return c.view.clone();
        }
        let imp = sprites::importer(rel);
        let view = match sprites::load_texture(rel) {
            Ok(img) => {
                let srgb = kind == TextureKind::Color && imp.is_srgb();
                Some(upload_with(
                    device,
                    queue,
                    rel,
                    img,
                    srgb,
                    imp.generate_mip_maps,
                ))
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

    /// Loaded textures and their estimated bytes (RGBA8 with mips ≈ 4/3 of the base level).
    pub fn memory(&self) -> (usize, u64) {
        let views = self.textures.values().filter_map(|c| c.view.as_ref());
        views.fold((0, 0), |(n, bytes), v| {
            let size = v.texture().size();
            let base = size.width as u64 * size.height as u64 * 4;
            (n + 1, bytes + base * 4 / 3)
        })
    }

    /// Whether a texture loaded successfully (false for files that failed to load).
    pub fn is_loaded(&self, rel: &str, kind: TextureKind) -> bool {
        self.textures
            .get(&(rel.to_owned(), kind))
            .is_some_and(|c| c.view.is_some())
    }

    /// Drop textures whose files changed (checked at most twice a second); they reload on next use.
    pub fn poll_changes(&mut self) -> bool {
        let now = Instant::now();
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
        {
            return false;
        }
        self.last_poll = Some(now);
        let before = self.textures.len();
        self.textures
            .retain(|(rel, _), c| c.modified == stamps(rel));
        let changed = self.textures.len() != before;
        if changed {
            self.sets.clear();
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
