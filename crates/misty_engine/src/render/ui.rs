//! UI pass: draws a scene's canvases on top of a render target's final image.

use std::collections::HashMap;

use glam::Vec2;

use super::RenderTarget;
use super::texture::{SamplerKey, upload, upload_with};
use crate::scene::Scene;
use crate::ui::draw::{UiDrawList, UiTexture, UiVertex, build};
use crate::ui::text::{GlyphAtlas, UiFont};
use crate::ui::{UiInteraction, layout};

const SHADER: &str = include_str!("shaders/ui.wgsl");
const ATLAS_SIZE: u32 = 1024;

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    atlas_texture: wgpu::Texture,
    atlas_group: wgpu::BindGroup,
    /// [`GlyphAtlas::version`] last uploaded.
    atlas_uploaded: u64,
    /// Project textures by path (None = failed to load).
    images: HashMap<String, Option<wgpu::BindGroup>>,
    vertices: wgpu::Buffer,
    capacity: usize,
    font: UiFont,
    atlas: GlyphAtlas,
    warnings: Vec<String>,
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty ui layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty ui sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white_view = upload(
            device,
            queue,
            "misty ui white",
            image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
            true,
        );
        let white = group(device, &layout, &sampler, &white_view);
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty glyph atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_group = group(
            device,
            &layout,
            &sampler,
            &atlas_texture.create_view(&Default::default()),
        );
        let capacity = 1024;
        Self {
            pipeline: pipeline(device, &layout),
            layout,
            sampler,
            white,
            atlas_texture,
            atlas_group,
            atlas_uploaded: 0,
            images: HashMap::new(),
            vertices: vertex_buffer(device, capacity),
            capacity,
            font: UiFont::default(),
            atlas: GlyphAtlas::new(ATLAS_SIZE),
            warnings: Vec::new(),
        }
    }

    /// Texture load errors since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Forget loaded UI textures (they reload on next use).
    pub fn reload_textures(&mut self) {
        self.images.clear();
    }

    /// Lay out `scene`'s canvases for the target's size and draw them over its image.
    /// `interaction` = button hover/press state while playing.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        scene: &Scene,
        interaction: Option<&UiInteraction>,
    ) {
        let (w, h) = target.size();
        let l = layout(scene, Vec2::new(w as f32, h as f32));
        if l.nodes.is_empty() {
            return;
        }
        let list = build(&l, scene, interaction, &self.font, &mut self.atlas);
        self.draw(device, queue, target.output_view(), &list);
    }

    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        list: &UiDrawList,
    ) {
        if list.vertices.is_empty() {
            return;
        }
        self.prepare(device, queue, list);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty ui"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty ui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            for b in &list.batches {
                let group = match &b.texture {
                    UiTexture::White => &self.white,
                    UiTexture::Glyphs => &self.atlas_group,
                    UiTexture::Image(path) => match self.images.get(path) {
                        Some(Some(g)) => g,
                        _ => &self.white,
                    },
                };
                pass.set_bind_group(0, group, &[]);
                pass.draw(b.start..b.start + b.count, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Upload vertices, the glyph atlas (when it changed) and any new textures.
    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, list: &UiDrawList) {
        if list.vertices.len() > self.capacity {
            self.capacity = list.vertices.len().next_power_of_two();
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&list.vertices));
        if self.atlas.version != self.atlas_uploaded {
            queue.write_texture(
                self.atlas_texture.as_image_copy(),
                &self.atlas.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS_SIZE),
                    rows_per_image: Some(ATLAS_SIZE),
                },
                self.atlas_texture.size(),
            );
            self.atlas_uploaded = self.atlas.version;
        }
        for b in &list.batches {
            let UiTexture::Image(path) = &b.texture else {
                continue;
            };
            if self.images.contains_key(path) {
                continue;
            }
            let g = match crate::sprites::load_image(path) {
                Ok(img) => {
                    let mips = crate::sprites::importer(path).generate_mip_maps;
                    let view = upload_with(device, queue, path, img, true, mips);
                    // Point filtering or clamping gets its own sampler.
                    let key = SamplerKey::of(Some(path));
                    let custom = (key != SamplerKey::default()).then(|| key.create(device));
                    let sampler = custom.as_ref().unwrap_or(&self.sampler);
                    Some(group(device, &self.layout, sampler, &view))
                }
                Err(e) => {
                    self.warnings.push(format!("Could not load UI texture {e}"));
                    None
                }
            };
            self.images.insert(path.clone(), g);
        }
    }
}

fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty ui group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty ui vertices"),
        size: (capacity * std::mem::size_of::<UiVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pipeline(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty ui"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty ui"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32
    ];
    // Straight alpha for colour; alpha accumulates so the image stays opaque.
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("misty ui"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<UiVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: super::COLOR_FORMAT,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use wgpu::naga;

    #[test]
    fn ui_shader_validates_and_vertex_matches_layout() {
        let module = naga::front::wgsl::parse_str(super::SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("valid ui shader");
        assert_eq!(std::mem::size_of::<super::UiVertex>(), 36);
    }
}
