//! Terrain layer textures as two 2D arrays (albedo, and normal with the height in its alpha;
//! one slice per layer), so a terrain needs only a few texture bindings however many layers
//! it blends, and the shader one fetch for a layer's normal and height. Slices are resized to
//! one square size. Layers without a height map use their albedo's luminance, stretched to
//! 0..1. The mean linear albedo of each layer is kept for Color Harmony.
//!
//! Decoding, resizing and mip-mapping the slices takes a few hundred milliseconds for large
//! source textures, so with streaming on (`render::grass_streaming`: the editor and the
//! player) it runs on a worker thread: a terrain draws with neutral placeholder layers for the
//! first frames of a scene, and keeps its old layers while a changed texture is rebuilt.
//! With streaming off (tests, screenshots) the arrays are built in the frame that needs them.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Instant, SystemTime};

use glam::Vec3;
use image::RgbaImage;
use wgpu::util::DeviceExt;

use super::texture::{load_image, mip_chain};
use crate::assets;
use crate::terrain::{MAX_LAYERS, TerrainLayer, harmony};

/// Largest slice side (4 layers × 2 arrays at 1024² ≈ 43 MB with mips).
const MAX_SIDE: u32 = 1024;

/// Bytes of finished slices written to the GPU per frame while streaming (the slice that
/// crosses it still goes). Writing all of a terrain's arrays at once (~64 MB of staging
/// memory) stalled a frame for 150 ms.
const UPLOAD_BUDGET: usize = 8 << 20;

pub(super) struct LayerSet {
    pub albedo: wgpu::TextureView,
    /// Normal (rgb) and height (a).
    pub normal: wgpu::TextureView,
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

/// Slices built on a worker, ready to upload.
struct Prepared {
    stamps: Vec<Option<SystemTime>>,
    albedo: ArrayData,
    /// Normal (rgb) and height (a).
    normal: ArrayData,
    means: [Option<Vec3>; MAX_LAYERS],
}

#[derive(Default)]
pub(super) struct LayerCache {
    sets: HashMap<Key, Cached>,
    /// Arrays being built on a worker thread.
    pending: HashMap<Key, Mutex<Receiver<Prepared>>>,
    /// Built arrays going up slice by slice (within [`UPLOAD_BUDGET`] per frame).
    uploading: Vec<(Key, Uploading)>,
    /// Drawn until a terrain's own arrays are ready.
    placeholder: Option<LayerSet>,
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
        let streaming = super::grass_streaming();
        let now = Instant::now();
        let poll = self
            .last_poll
            .is_none_or(|t| now.duration_since(t).as_secs_f32() > 0.5);
        if poll {
            self.last_poll = Some(now);
            if streaming {
                // Rebuild changed sets in the background; the old arrays draw meanwhile.
                let busy = |k: &Key| {
                    self.pending.contains_key(k) || self.uploading.iter().any(|u| u.0 == *k)
                };
                let stale: Vec<Key> = (self.sets.iter())
                    .filter(|(k, c)| !busy(k) && c.stamps != stamps(k))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in stale {
                    self.spawn(k);
                }
            } else {
                self.sets.retain(|k, c| c.stamps == stamps(k));
            }
        }
        self.finish_pending(device, queue);
        let busy =
            |k: &Key| self.pending.contains_key(k) || self.uploading.iter().any(|u| u.0 == *k);
        if !self.sets.contains_key(&key) && !busy(&key) {
            if streaming {
                self.spawn(key.clone());
            } else {
                crate::profile_scope!("Terrain.LayerArrays");
                let set = upload(device, queue, prepare(&key));
                self.sets.insert(key.clone(), set);
            }
        }
        if self.sets.contains_key(&key) {
            return &self.sets[&key].set;
        }
        self.placeholder
            .get_or_insert_with(|| placeholder(device, queue))
    }

    /// Start building the arrays of `key` on a worker thread.
    fn spawn(&mut self, key: Key) {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = key.clone();
        let spawned = std::thread::Builder::new()
            .name("misty terrain layers".into())
            .spawn(move || {
                let _ = tx.send(prepare(&job));
            });
        if spawned.is_ok() {
            self.pending.insert(key, Mutex::new(rx));
        }
    }

    /// Upload the arrays the workers finished.
    fn finish_pending(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut done = Vec::new();
        self.pending.retain(
            |k, rx| match rx.get_mut().map_err(|_| ()).map(|r| r.try_recv()) {
                Ok(Ok(p)) => {
                    done.push((k.clone(), p));
                    false
                }
                Ok(Err(TryRecvError::Empty)) => true,
                Ok(Err(TryRecvError::Disconnected)) | Err(()) => false,
            },
        );
        for (k, p) in done {
            self.uploading.push((k, Uploading::new(device, p)));
        }
        if self.uploading.is_empty() {
            return;
        }
        crate::profile_scope!("Terrain.LayerUpload");
        let mut budget = UPLOAD_BUDGET;
        while budget > 0
            && let Some((_, u)) = self.uploading.first_mut()
        {
            budget = budget.saturating_sub(u.write_next(queue));
            if u.done() {
                let (k, u) = self.uploading.remove(0);
                self.sets.insert(k, u.finish());
            }
        }
    }

    /// Arrays still being built or uploaded (a scene's first frames).
    pub fn pending(&self) -> usize {
        self.pending.len() + self.uploading.len()
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

/// Decode, fit and mip-map the slices of `key` (any thread).
fn prepare(key: &Key) -> Prepared {
    let stamps = stamps(key);
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
    for (i, [a, n, h]) in images.into_iter().enumerate() {
        means[i] = a.as_ref().map(|a| harmony::mean_linear(a.as_raw()));
        let h = h.or_else(|| a.as_ref().map(luminance_height));
        let solid = |c: [u8; 4]| RgbaImage::from_pixel(side, side, image::Rgba(c));
        albedo.push(a.map(|a| fit(a, side)).unwrap_or_else(|| solid([255; 4])));
        let n = n
            .map(|n| fit(n, side))
            .unwrap_or_else(|| solid([128, 128, 255, 255]));
        let h = h
            .map(|h| fit(h, side))
            .unwrap_or_else(|| solid([128, 128, 128, 255]));
        normal.push(with_height(n, &h));
    }
    Prepared {
        stamps,
        albedo: ArrayData::new(albedo, true),
        normal: ArrayData::new(normal, false),
        means,
    }
}

/// The normal map with the height map's red channel in its alpha (the mip chain filters every
/// channel alike, so each mip's alpha is the height map's own mip).
fn with_height(mut normal: RgbaImage, height: &RgbaImage) -> RgbaImage {
    for (n, h) in normal.pixels_mut().zip(height.pixels()) {
        n[3] = h[0];
    }
    normal
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, p: Prepared) -> Cached {
    let (a, b1) = array(device, queue, "misty terrain albedo", &p.albedo);
    let (n, b2) = array(device, queue, "misty terrain normals", &p.normal);
    Cached {
        stamps: p.stamps,
        set: LayerSet {
            albedo: a,
            normal: n,
            means: p.means,
            bytes: b1 + b2,
        },
    }
}

/// Built arrays whose textures exist and are written one (array, layer) at a time.
struct Uploading {
    stamps: Vec<Option<SystemTime>>,
    means: [Option<Vec3>; MAX_LAYERS],
    /// Albedo, normal + height.
    arrays: [(wgpu::Texture, ArrayData); 2],
    /// Next piece: array * layers + layer.
    next: u32,
}

impl Uploading {
    fn new(device: &wgpu::Device, p: Prepared) -> Self {
        let make = |label: &str, a: ArrayData| (texture(device, label, &a, true), a);
        Self {
            stamps: p.stamps,
            means: p.means,
            arrays: [
                make("misty terrain albedo", p.albedo),
                make("misty terrain normals", p.normal),
            ],
            next: 0,
        }
    }

    fn pieces(&self) -> u32 {
        2 * self.arrays[0].1.layers
    }

    fn done(&self) -> bool {
        self.next >= self.pieces()
    }

    /// Write the next layer of the next array (all its mips); returns the bytes written.
    fn write_next(&mut self, queue: &wgpu::Queue) -> usize {
        if self.done() {
            return 0;
        }
        let layers = self.arrays[0].1.layers;
        let (texture, a) = &self.arrays[(self.next / layers) as usize];
        let layer = self.next % layers;
        self.next += 1;
        let per_layer = a.data.len() / a.layers as usize;
        let mut offset = layer as usize * per_layer;
        for level in 0..a.levels {
            let side = (a.side >> level).max(1);
            let len = (side * side * 4) as usize;
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
                &a.data[offset..offset + len],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 4),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
            offset += len;
        }
        per_layer
    }

    fn finish(self) -> Cached {
        let [a, n] = self
            .arrays
            .map(|(t, d)| (array_view(&t), d.data.len() as u64));
        Cached {
            stamps: self.stamps,
            set: LayerSet {
                albedo: a.0,
                normal: n.0,
                means: self.means,
                bytes: a.1 + n.1,
            },
        }
    }
}

/// Neutral layers (grey, flat, mid height) drawn while a terrain's own are built.
fn placeholder(device: &wgpu::Device, queue: &wgpu::Queue) -> LayerSet {
    let solid = |c: [u8; 4]| vec![RgbaImage::from_pixel(4, 4, image::Rgba(c)); MAX_LAYERS];
    let (albedo, _) = array(
        device,
        queue,
        "misty terrain albedo (loading)",
        &ArrayData::new(solid([128, 128, 128, 255]), true),
    );
    // Flat, mid height (alpha).
    let (normal, _) = array(
        device,
        queue,
        "misty terrain normals (loading)",
        &ArrayData::new(solid([128, 128, 255, 128]), false),
    );
    LayerSet {
        albedo,
        normal,
        means: [None; MAX_LAYERS],
        bytes: 0,
    }
}

/// Square slices with their mips, layer-major, ready for a 2D array texture.
struct ArrayData {
    side: u32,
    layers: u32,
    levels: u32,
    srgb: bool,
    data: Vec<u8>,
}

impl ArrayData {
    fn new(slices: Vec<RgbaImage>, srgb: bool) -> Self {
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
        Self {
            side,
            layers,
            levels,
            srgb,
            data,
        }
    }
}

/// Upload square slices with mips as a 2D array.
fn array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    a: &ArrayData,
) -> (wgpu::TextureView, u64) {
    let texture = device.create_texture_with_data(
        queue,
        &descriptor(label, a, false),
        wgpu::util::TextureDataOrder::LayerMajor,
        &a.data,
    );
    (array_view(&texture), a.data.len() as u64)
}

/// An empty array texture for `a`, written later (`copy_dst`).
fn texture(device: &wgpu::Device, label: &str, a: &ArrayData, copy_dst: bool) -> wgpu::Texture {
    device.create_texture(&descriptor(label, a, copy_dst))
}

fn descriptor<'a>(label: &'a str, a: &ArrayData, copy_dst: bool) -> wgpu::TextureDescriptor<'a> {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING;
    if copy_dst {
        usage |= wgpu::TextureUsages::COPY_DST;
    }
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: a.side,
            height: a.side,
            depth_or_array_layers: a.layers,
        },
        mip_level_count: a.levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if a.srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage,
        view_formats: &[],
    }
}

fn array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}
