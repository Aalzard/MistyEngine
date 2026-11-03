//! Material textures: PNG/JPG decoding, CPU mip generation, upload, and a cache of textures and
//! per-material bind groups keyed by project-relative path.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

use image::RgbaImage;
use wgpu::util::DeviceExt;

use crate::assets;

/// Largest side kept on upload (bigger images are scaled down).
const MAX_SIZE: u32 = 4096;

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
    let img = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    if w.max(h) <= MAX_SIZE {
        return Ok(img);
    }
    let s = MAX_SIZE as f32 / w.max(h) as f32;
    Ok(image::imageops::resize(
        &img,
        ((w as f32 * s) as u32).max(1),
        ((h as f32 * s) as u32).max(1),
        image::imageops::FilterType::Triangle,
    ))
}

/// Upload an image with generated mips.
pub(crate) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    image: RgbaImage,
    srgb: bool,
) -> wgpu::TextureView {
    let (w, h) = image.dimensions();
    let chain = mip_chain(image, srgb);
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

/// Colour maps are sRGB; normal maps hold vectors and stay linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureKind {
    Color,
    Normal,
}

struct Cached {
    modified: Option<SystemTime>,
    /// None when the file could not be read (the default texture is used).
    view: Option<wgpu::TextureView>,
}

type SetKey = (Option<String>, Option<String>);

/// Loaded textures and one bind group per (albedo, normal) pair.
pub struct TextureCache {
    textures: HashMap<(String, TextureKind), Cached>,
    sets: HashMap<SetKey, wgpu::BindGroup>,
    /// 1×1 white (albedo) and flat normal, created on first use.
    defaults: Option<(wgpu::TextureView, wgpu::TextureView)>,
    sampler: wgpu::Sampler,
    last_poll: Option<Instant>,
    warnings: Vec<String>,
}

impl TextureCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self {
            textures: HashMap::new(),
            sets: HashMap::new(),
            defaults: None,
            sampler,
            last_poll: None,
            warnings: Vec::new(),
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
        let abs = assets::absolute(rel);
        let view = match load_image(&abs) {
            Ok(img) => Some(upload(device, queue, rel, img, kind == TextureKind::Color)),
            Err(e) => {
                self.warnings.push(format!("Could not load texture {e}"));
                None
            }
        };
        self.textures.insert(
            key,
            Cached {
                modified: mtime(&abs),
                view: view.clone(),
            },
        );
        view
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
        let key = (albedo.map(str::to_owned), normal.map(str::to_owned));
        if self.sets.contains_key(&key) {
            return;
        }
        let (white, flat) = self
            .defaults
            .get_or_insert_with(|| {
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
                (
                    pixel("misty white", [255; 4], wgpu::TextureFormat::Rgba8UnormSrgb),
                    pixel(
                        "misty flat normal",
                        [128, 128, 255, 255],
                        wgpu::TextureFormat::Rgba8Unorm,
                    ),
                )
            })
            .clone();
        let albedo_view = albedo
            .and_then(|a| self.texture(device, queue, a, TextureKind::Color))
            .unwrap_or(white);
        let normal_view = normal
            .and_then(|n| self.texture(device, queue, n, TextureKind::Normal))
            .unwrap_or(flat);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty material textures"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&albedo_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.sets.insert(key, group);
    }

    /// Bind group prepared for this pair.
    pub fn get(&self, albedo: Option<&str>, normal: Option<&str>) -> Option<&wgpu::BindGroup> {
        self.sets
            .get(&(albedo.map(str::to_owned), normal.map(str::to_owned)))
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
            .retain(|(rel, _), c| c.modified == mtime(&assets::absolute(rel)));
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
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
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
