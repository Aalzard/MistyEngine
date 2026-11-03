//! Terrain layer textures as three 2D arrays (albedo, normal, height; one slice per layer),
//! so a terrain needs only a few texture bindings however many layers it blends. Slices are
//! resized to one square size. Layers without a height map use their albedo's luminance,
//! stretched to 0..1. The mean linear albedo of each layer is kept for Color Harmony.

use std::collections::HashMap;
use std::time::{Instant, SystemTime};

use glam::Vec3;
use image::RgbaImage;
use wgpu::util::DeviceExt;

use super::texture::{load_image, mip_chain};
use crate::assets;
use crate::terrain::{MAX_LAYERS, TerrainLayer, harmony};

/// Largest slice side (4 layers × 3 arrays at 1024² ≈ 64 MB with mips).
const MAX_SIDE: u32 = 1024;

pub(super) struct LayerSet {
    pub albedo: wgpu::TextureView,
    pub normal: wgpu::TextureView,
    pub height: wgpu::TextureView,
    /// Mean linear albedo per layer (None = no texture).
    pub means: [Option<Vec3>; MAX_LAYERS],
    pub bytes: u64,
}

/// Texture paths of each layer slot (albedo, normal, height).
type Key = Vec<[Option<String>; 3]>;

struct Cached {
    stamps: Vec<Option<SystemTime>>,
    set: LayerSet,
}

#[derive(Default)]
pub(super) struct LayerCache {
    sets: HashMap<Key, Cached>,
    last_poll: Option<Instant>,
}

fn key(layers: &[TerrainLayer]) -> Key {
    (0..MAX_LAYERS)
        .map(|i| {
            let l = layers.get(i);
            [
                l.and_then(|l| l.diffuse.clone()),
                l.and_then(|l| l.normal_map.clone()),
                l.and_then(|l| l.height_map.clone()),
            ]
        })
        .collect()
}

fn stamps(key: &Key) -> Vec<Option<SystemTime>> {
    key.iter()
        .flatten()
        .map(|p| {
            p.as_ref().and_then(|p| {
                std::fs::metadata(assets::absolute(p))
                    .and_then(|m| m.modified())
                    .ok()
            })
        })
        .collect()
}

impl LayerCache {
    /// The arrays for these layers (built on first use, rebuilt when a file changes).
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: &[TerrainLayer],
    ) -> &LayerSet {
        let key = key(layers);
        let now = Instant::now();
        let poll = self
            .last_poll
            .is_none_or(|t| now.duration_since(t).as_secs_f32() > 0.5);
        if poll {
            self.last_poll = Some(now);
            self.sets.retain(|k, c| c.stamps == stamps(k));
        }
        if !self.sets.contains_key(&key) {
            crate::profile_scope!("Terrain.LayerArrays");
            let cached = Cached {
                stamps: stamps(&key),
                set: build(device, queue, &key),
            };
            self.sets.insert(key.clone(), cached);
        }
        &self.sets[&key].set
    }

    pub fn bytes(&self) -> u64 {
        self.sets.values().map(|c| c.set.bytes).sum()
    }
}

fn load(rel: &Option<String>) -> Option<RgbaImage> {
    rel.as_ref()
        .and_then(|r| load_image(&assets::absolute(r)).ok())
}

fn fit(img: RgbaImage, side: u32) -> RgbaImage {
    if img.width() == side && img.height() == side {
        img
    } else {
        image::imageops::resize(&img, side, side, image::imageops::FilterType::Triangle)
    }
}

/// Greyscale height from the albedo's luminance, stretched to the full range.
fn luminance_height(albedo: &RgbaImage) -> RgbaImage {
    let lum = |p: &image::Rgba<u8>| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
    let (lo, hi) = albedo.pixels().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(lum(p)), hi.max(lum(p)))
    });
    let scale = 255.0 / (hi - lo).max(1.0);
    RgbaImage::from_fn(albedo.width(), albedo.height(), |x, y| {
        let v = ((lum(albedo.get_pixel(x, y)) - lo) * scale).round() as u8;
        image::Rgba([v, v, v, 255])
    })
}

fn build(device: &wgpu::Device, queue: &wgpu::Queue, key: &Key) -> LayerSet {
    let images: Vec<[Option<RgbaImage>; 3]> = key
        .iter()
        .map(|[a, n, h]| [load(a), load(n), load(h)])
        .collect();
    let side = images
        .iter()
        .flatten()
        .flatten()
        .map(|i| i.width().max(i.height()))
        .max()
        .unwrap_or(4)
        .next_power_of_two()
        .clamp(4, MAX_SIDE);
    let mut means = [None; MAX_LAYERS];
    let mut albedo = Vec::new();
    let mut normal = Vec::new();
    let mut height = Vec::new();
    for (i, [a, n, h]) in images.into_iter().enumerate() {
        means[i] = a.as_ref().map(|a| harmony::mean_linear(a.as_raw()));
        let h = h.or_else(|| a.as_ref().map(luminance_height));
        let solid = |c: [u8; 4]| RgbaImage::from_pixel(side, side, image::Rgba(c));
        albedo.push(a.map(|a| fit(a, side)).unwrap_or_else(|| solid([255; 4])));
        normal.push(
            n.map(|n| fit(n, side))
                .unwrap_or_else(|| solid([128, 128, 255, 255])),
        );
        height.push(
            h.map(|h| fit(h, side))
                .unwrap_or_else(|| solid([128, 128, 128, 255])),
        );
    }
    let (a, b1) = array(device, queue, "misty terrain albedo", albedo, true);
    let (n, b2) = array(device, queue, "misty terrain normals", normal, false);
    let (h, b3) = array(device, queue, "misty terrain heights", height, false);
    LayerSet {
        albedo: a,
        normal: n,
        height: h,
        means,
        bytes: b1 + b2 + b3,
    }
}

/// Upload square slices with mips as a 2D array.
fn array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    slices: Vec<RgbaImage>,
    srgb: bool,
) -> (wgpu::TextureView, u64) {
    let side = slices[0].width();
    let layers = slices.len() as u32;
    let mut levels = 0;
    let mut data = Vec::new();
    for s in slices {
        let chain = mip_chain(s, srgb);
        levels = chain.len() as u32;
        for m in chain {
            data.extend_from_slice(m.as_raw());
        }
    }
    let bytes = data.len() as u64;
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (view, bytes)
}
