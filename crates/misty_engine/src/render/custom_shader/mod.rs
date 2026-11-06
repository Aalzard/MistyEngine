//! Materials using `.shader` assets: compiling the wrapped WGSL into pipelines (recompiled when
//! the asset changes), per-material uniform + texture bind groups, and the per-frame `globals`
//! group (time, screen size, the opaque scene's colour and depth).
//!
//! A shader that fails to compile is drawn with the magenta error shader and its errors (with
//! lines of the user's file) go to the Console.

mod wrap;

#[cfg(test)]
mod gpu_tests;

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::pipelines::{self, Depth, Pipelines, Spec};
use super::texture::{TextureCache, TextureKind};
use crate::shader_asset::{
    self, Blend, Cull, PropertyKind, PropertyValue, ShaderAsset, ShaderError, ShaderSettings,
    TextureDefault,
};

pub use wrap::{PropsLayout, check, layout, validate, wrap};

/// Any draw of the frame uses a `.shader` material (GPU culling and the depth prepass then step
/// aside: they draw with the built-in variants).
pub(crate) fn any_in(frame: &super::Frame) -> bool {
    frame.meshes.iter().any(|d| d.material.custom.is_some())
}

/// The `.shader` part of a draw's material: asset path, property values and render queue.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMaterial {
    pub shader: String,
    /// Declared properties with the material's values (defaults filled in).
    pub values: Arc<BTreeMap<String, PropertyValue>>,
    pub queue: u32,
}

impl CustomMaterial {
    fn hash_values(&self, h: &mut impl Hasher) {
        for (k, v) in self.values.iter() {
            k.hash(h);
            match v {
                PropertyValue::Float(f) => f.to_bits().hash(h),
                PropertyValue::Vector(a) => a.map(f32::to_bits).hash(h),
                PropertyValue::Texture(t) => t.hash(h),
            }
        }
    }
}

/// Values for a shader's declared properties: the material's where they fit, else the defaults.
pub fn resolve_values(
    asset: &ShaderAsset,
    stored: &BTreeMap<String, PropertyValue>,
) -> BTreeMap<String, PropertyValue> {
    asset
        .properties
        .iter()
        .map(|p| {
            let v = stored
                .get(&p.name)
                .filter(|v| v.fits(p.kind))
                .cloned()
                .unwrap_or_else(|| p.default.clone());
            (p.name.clone(), v)
        })
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsUniform {
    time: [f32; 4],
    screen: [f32; 4],
}

struct Compiled {
    generation: u64,
    asset: Arc<ShaderAsset>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    props: PropsLayout,
}

/// Compiled custom shaders and their material bind groups.
pub struct CustomShaders {
    globals_layout: wgpu::BindGroupLayout,
    globals_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    scene_sampler: wgpu::Sampler,
    /// Group 3 before the scene is copied (opaque queue, or no copy this frame).
    placeholder_group: wgpu::BindGroup,
    /// Group 3 per scene copy id.
    scene_groups: HashMap<u64, wgpu::BindGroup>,
    /// By shader path; a failed compile holds the error shader under the asset's generation.
    shaders: HashMap<String, Compiled>,
    error: Option<Compiled>,
    materials: HashMap<u64, wgpu::BindGroup>,
    defaults: HashMap<TextureDefault, wgpu::TextureView>,
    /// Compile errors for the Console.
    errors: Vec<String>,
    /// Clock value of the previous render (for the frame delta).
    last_time: f64,
}

fn pixel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    data: &[u8],
) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("misty custom shader placeholder"),
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
            data,
        )
        .create_view(&Default::default())
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn texture_type(filterable: bool) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

fn uniform_type() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn blend_state(blend: Blend) -> Option<wgpu::BlendState> {
    match blend {
        Blend::Off => None,
        Blend::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
        Blend::Premultiply => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        Blend::Additive => Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        }),
    }
}

fn cull_face(cull: Cull) -> Option<wgpu::Face> {
    match cull {
        Cull::Back => Some(wgpu::Face::Back),
        Cull::Front => Some(wgpu::Face::Front),
        Cull::Off => None,
    }
}

/// `Shader error in Assets/Shaders/A.shader:12:5: message` (the Console links the location).
pub fn console_line(path: &str, e: &ShaderError) -> String {
    if e.line > 0 {
        format!(
            "Shader error in {path}:{}:{}: {}",
            e.line,
            e.column.max(1),
            e.message
        )
    } else {
        format!("Shader error in {path}: {}", e.message)
    }
}

/// Run `f` inside a validation error scope; `Err` when wgpu rejected what it created.
fn scoped<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> Result<T, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = f();
    match block_on(scope.pop()) {
        Some(e) => Err(e.to_string()),
        None => Ok(value),
    }
}

/// Native error scopes resolve immediately; poll once with a no-op waker.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

impl CustomShaders {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty custom globals"),
            entries: &[
                entry(0, uniform_type()),
                entry(1, texture_type(true)),
                entry(2, texture_type(false)),
                entry(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty custom globals"),
            size: std::mem::size_of::<GlobalsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty custom sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty scene sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = pixel(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            &[0, 0, 0, 255],
        );
        let far = pixel(
            device,
            queue,
            wgpu::TextureFormat::R32Float,
            &1.0f32.to_le_bytes(),
        );
        let mut s = Self {
            placeholder_group: Self::make_globals(
                device,
                &globals_layout,
                &globals_buffer,
                &scene_sampler,
                &black,
                &far,
            ),
            globals_layout,
            globals_buffer,
            sampler,
            scene_sampler,
            scene_groups: HashMap::new(),
            shaders: HashMap::new(),
            error: None,
            materials: HashMap::new(),
            defaults: HashMap::new(),
            errors: Vec::new(),
            last_time: crate::clock::now(),
        };
        for d in [
            TextureDefault::White,
            TextureDefault::Black,
            TextureDefault::Grey,
            TextureDefault::Normal,
        ] {
            let format = if d == TextureDefault::Normal {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                wgpu::TextureFormat::Rgba8UnormSrgb
            };
            s.defaults
                .insert(d, pixel(device, queue, format, &d.rgba()));
        }
        s
    }

    fn make_globals(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty custom globals"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Time and screen size of this render.
    pub fn write_globals(&mut self, queue: &wgpu::Queue, size: (u32, u32)) {
        let t = crate::clock::now();
        let dt = (t - self.last_time).clamp(0.0, 1.0) as f32;
        self.last_time = t;
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let u = GlobalsUniform {
            time: [t as f32, t.sin() as f32, t.cos() as f32, dt],
            screen: [w, h, 1.0 / w, 1.0 / h],
        };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&u));
    }

    /// Group 3 with the opaque scene copy (`id` names the copy; built once per copy).
    pub fn scene_group(
        &mut self,
        device: &wgpu::Device,
        id: u64,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) -> &wgpu::BindGroup {
        if self.scene_groups.len() > 8 && !self.scene_groups.contains_key(&id) {
            self.scene_groups.clear();
        }
        let (layout, buffer, sampler) = (
            &self.globals_layout,
            &self.globals_buffer,
            &self.scene_sampler,
        );
        self.scene_groups
            .entry(id)
            .or_insert_with(|| Self::make_globals(device, layout, buffer, sampler, color, depth))
    }

    pub fn scene_group_for(&self, id: Option<u64>) -> &wgpu::BindGroup {
        id.and_then(|id| self.scene_groups.get(&id))
            .unwrap_or(&self.placeholder_group)
    }

    /// Compile errors since the last call, as Console lines.
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Re-read changed shader files (they recompile on next use).
    pub fn poll_changes(&mut self) {
        shader_asset::poll_changes();
    }

    /// Material bind groups hold texture views: rebuild them after textures reload.
    pub fn textures_changed(&mut self) {
        self.materials.clear();
    }

    fn build(
        &self,
        device: &wgpu::Device,
        p: &Pipelines,
        asset: Arc<ShaderAsset>,
        generation: u64,
    ) -> Result<Compiled, ShaderError> {
        let wrapped = wrap(&asset);
        validate(&wrapped)?;
        let mut entries = vec![
            entry(wrap::UNIFORM_BINDING, uniform_type()),
            entry(
                wrap::SAMPLER_BINDING,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ];
        for (_, binding, _) in &wrapped.props.textures {
            entries.push(entry(*binding, texture_type(true)));
        }
        let settings: ShaderSettings = asset.settings;
        scoped(device, || {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("misty custom material"),
                entries: &entries,
            });
            let layouts = [
                &p.view_layout,
                &p.mesh_layout,
                &layout,
                &self.globals_layout,
            ];
            let buffers = pipelines::mesh_buffers();
            let pipeline = pipelines::build(
                device,
                Spec {
                    vs: "vs_custom",
                    fs: Some("fs_custom"),
                    buffers: &buffers,
                    cull: cull_face(settings.cull),
                    depth: Depth::main(settings.zwrite, wgpu::CompareFunction::Less),
                    blend: blend_state(settings.blend),
                    glow: Some(true),
                    velocity: settings.zwrite,
                    ..Spec::new("custom shader", &wrapped.body, &layouts)
                },
            );
            Compiled {
                generation,
                asset: asset.clone(),
                layout,
                pipeline,
                props: wrapped.props.clone(),
            }
        })
        .map_err(|message| ShaderError {
            line: 0,
            column: 0,
            message,
        })
    }

    fn error_shader(&mut self, device: &wgpu::Device, p: &Pipelines) -> &Compiled {
        if self.error.is_none() {
            let asset = Arc::new(
                shader_asset::parse(shader_asset::templates::ERROR).expect("error shader parses"),
            );
            let compiled = self
                .build(device, p, asset, 0)
                .expect("error shader compiles");
            self.error = Some(compiled);
        }
        self.error.as_ref().expect("just built")
    }

    /// Compile the material's shader if needed and build its bind group.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        p: &Pipelines,
        textures: &mut TextureCache,
        m: &CustomMaterial,
    ) {
        let loaded = shader_asset::load(&m.shader);
        let generation = shader_asset::generation(&m.shader);
        if self
            .shaders
            .get(&m.shader)
            .is_none_or(|c| c.generation != generation)
        {
            let built = loaded.and_then(|asset| self.build(device, p, asset, generation));
            let compiled = match built {
                Ok(c) => c,
                Err(e) => {
                    self.errors.push(console_line(&m.shader, &e));
                    let fallback = self.error_shader(device, p);
                    Compiled {
                        generation,
                        asset: fallback.asset.clone(),
                        layout: fallback.layout.clone(),
                        pipeline: fallback.pipeline.clone(),
                        props: fallback.props.clone(),
                    }
                }
            };
            self.shaders.insert(m.shader.clone(), compiled);
        }
        let key = self.group_key(m);
        if self.materials.contains_key(&key) {
            return;
        }
        if self.materials.len() > 512 {
            self.materials.clear();
        }
        let group = self.material_group(device, queue, textures, m);
        if let Some(group) = group {
            self.materials.insert(key, group);
        }
    }

    fn group_key(&self, m: &CustomMaterial) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        m.shader.hash(&mut h);
        self.shaders
            .get(&m.shader)
            .map(|c| c.generation)
            .hash(&mut h);
        m.hash_values(&mut h);
        h.finish()
    }

    fn material_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: &mut TextureCache,
        m: &CustomMaterial,
    ) -> Option<wgpu::BindGroup> {
        let c = self.shaders.get(&m.shader)?;
        let mut bytes = vec![0u8; c.props.size];
        for f in &c.props.fields {
            let value = m.values.get(&f.name);
            let words: Vec<f32> = match f.kind {
                PropertyKind::Color => {
                    let v = value.and_then(PropertyValue::as_vector).unwrap_or([1.0; 4]);
                    let l = super::srgb_to_linear([v[0], v[1], v[2]]);
                    vec![l[0], l[1], l[2], v[3]]
                }
                PropertyKind::Vector => value
                    .and_then(PropertyValue::as_vector)
                    .unwrap_or_default()
                    .to_vec(),
                _ => vec![value.and_then(PropertyValue::as_float).unwrap_or(0.0)],
            };
            let data: &[u8] = bytemuck::cast_slice(&words);
            bytes[f.offset..f.offset + data.len()].copy_from_slice(data);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty custom material"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let views: Vec<(u32, wgpu::TextureView)> = c
            .props
            .textures
            .iter()
            .map(|(name, binding, default)| {
                let kind = if *default == TextureDefault::Normal {
                    TextureKind::Normal
                } else {
                    TextureKind::Color
                };
                let rel = m.values.get(name).and_then(PropertyValue::as_texture);
                let view = rel
                    .and_then(|r| textures.texture_view(device, queue, r, kind))
                    .unwrap_or_else(|| self.defaults[default].clone());
                (*binding, view)
            })
            .collect();
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: wrap::UNIFORM_BINDING,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: wrap::SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ];
        for (binding, view) in &views {
            entries.push(wgpu::BindGroupEntry {
                binding: *binding,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty custom material"),
            layout: &c.layout,
            entries: &entries,
        }))
    }

    /// Pipeline and material group prepared for a draw.
    pub fn get(&self, m: &CustomMaterial) -> Option<(&wgpu::RenderPipeline, &wgpu::BindGroup)> {
        let c = self.shaders.get(&m.shader)?;
        let group = self.materials.get(&self.group_key(m))?;
        Some((&c.pipeline, group))
    }

    /// Does the material's shader sample the opaque scene (needs the scene copied first)?
    pub fn uses_scene(&self, m: &CustomMaterial) -> bool {
        self.shaders
            .get(&m.shader)
            .is_some_and(|c| c.asset.uses_scene)
    }
}
