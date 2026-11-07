//! UI pass: draws a scene's canvases on top of a render target's final image.

use std::collections::HashMap;

use glam::Vec2;

use super::RenderTarget;
use super::runtime_tex::RuntimeTextures;
use super::texture::{SamplerKey, upload, upload_with};
use crate::scene::Scene;
use crate::ui::draw::{UiDrawList, UiTexture, UiVertex, build_with};
use crate::ui::rich::sdf::SdfAtlas;
use crate::ui::text::{GlyphAtlas, UiFont};
use crate::ui::{UiInteraction, layout};
use crate::video::{VideoPlayer, VideoRenderMode};

const SHADER: &str = include_str!("shaders/ui.wgsl");
const ATLAS_SIZE: u32 = 1024;
const SDF_ATLAS_SIZE: u32 = 1024;

fn r8_atlas(device: &wgpu::Device, label: &str, size: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_atlas(queue: &wgpu::Queue, texture: &wgpu::Texture, pixels: &[u8], size: u32) {
    queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size),
            rows_per_image: Some(size),
        },
        texture.size(),
    );
}

/// Video Players drawing on a camera plane: (runtime texture, aspect ratio mode, alpha).
fn camera_plane_videos(
    scene: &Scene,
    far: bool,
) -> Vec<(String, crate::video::VideoAspectRatio, f32)> {
    let want = if far {
        VideoRenderMode::CameraFarPlane
    } else {
        VideoRenderMode::CameraNearPlane
    };
    let mut out: Vec<_> = scene
        .world
        .query::<(hecs::Entity, &VideoPlayer)>()
        .iter()
        .filter(|(e, v)| v.enabled && v.render_mode == want && scene.active_in_hierarchy(*e))
        .filter_map(|(_, v)| {
            Some((
                v.output_texture()?,
                v.aspect_ratio,
                v.target_camera_alpha.clamp(0.0, 1.0),
            ))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Full-screen video quads for one camera plane (aspect-fitted, centred).
pub fn camera_plane_list(scene: &Scene, screen: Vec2, far: bool) -> UiDrawList {
    let mut list = UiDrawList::default();
    for (name, aspect, alpha) in camera_plane_videos(scene, far) {
        let Some(f) = crate::video::textures::get(&name) else {
            continue;
        };
        let size = aspect.fit(Vec2::new(f.width as f32, f.height as f32), screen);
        let min = (screen - size) * 0.5;
        list.image(
            screen,
            min,
            min + size,
            UiTexture::Image(name),
            [1.0, 1.0, 1.0, alpha],
        );
    }
    list
}

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    /// Camera Far Plane video: draws only where the depth buffer is empty.
    far_pipeline: wgpu::RenderPipeline,
    depth_layout: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    atlas_texture: wgpu::Texture,
    atlas_group: wgpu::BindGroup,
    /// [`GlyphAtlas::version`] last uploaded.
    atlas_uploaded: u64,
    /// Project textures by path (None = failed to load).
    images: HashMap<String, Option<wgpu::BindGroup>>,
    sdf_texture: wgpu::Texture,
    sdf_group: wgpu::BindGroup,
    sdf_uploaded: u64,
    sdf: SdfAtlas,
    /// Video frames (runtime textures) and their bind groups.
    runtime: RuntimeTextures,
    runtime_groups: HashMap<String, wgpu::BindGroup>,
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
        let atlas_texture = r8_atlas(device, "misty glyph atlas", ATLAS_SIZE);
        let sdf_texture = r8_atlas(device, "misty sdf glyph atlas", SDF_ATLAS_SIZE);
        let sdf_group = group(
            device,
            &layout,
            &sampler,
            &sdf_texture.create_view(&Default::default()),
        );
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty ui depth layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let atlas_group = group(
            device,
            &layout,
            &sampler,
            &atlas_texture.create_view(&Default::default()),
        );
        let capacity = 1024;
        Self {
            pipeline: pipeline(device, &[&layout], "fs"),
            far_pipeline: pipeline(device, &[&layout, &depth_layout], "fs_far"),
            depth_layout,
            layout,
            sampler,
            white,
            atlas_texture,
            atlas_group,
            atlas_uploaded: 0,
            images: HashMap::new(),
            sdf_texture,
            sdf_group,
            sdf_uploaded: 0,
            sdf: SdfAtlas::new(SDF_ATLAS_SIZE),
            runtime: RuntimeTextures::default(),
            runtime_groups: HashMap::new(),
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
        crate::profile_scope!("UI.Render", crate::profiler::Category::Ui);
        // The canvas covers the final image (output resolution, also when upscaled).
        let (w, h) = target.output_size();
        let screen = Vec2::new(w as f32, h as f32);
        // Video Players on camera planes: behind the scene (sky only), then in front of it.
        let far = camera_plane_list(scene, screen, true);
        if !far.vertices.is_empty() {
            let depth = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty ui depth"),
                layout: &self.depth_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(target.depth_view()),
                }],
            });
            self.draw(device, queue, target.output_view(), &far, Some(&depth));
        }
        let near = camera_plane_list(scene, screen, false);
        self.draw(device, queue, target.output_view(), &near, None);
        let l = {
            crate::profile_scope!("UI.Layout");
            layout(scene, screen)
        };
        if l.nodes.is_empty() {
            return;
        }
        let list = build_with(
            &l,
            scene,
            interaction,
            &self.font,
            &mut self.atlas,
            &mut self.sdf,
        );
        self.draw(device, queue, target.output_view(), &list, None);
    }

    /// Draw `list` over `view`; with `depth` (Camera Far Plane) only where the scene is empty.
    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        list: &UiDrawList,
        depth: Option<&wgpu::BindGroup>,
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
            match depth {
                Some(d) => {
                    pass.set_pipeline(&self.far_pipeline);
                    pass.set_bind_group(1, d, &[]);
                }
                None => pass.set_pipeline(&self.pipeline),
            }
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            for b in &list.batches {
                let group = match &b.texture {
                    UiTexture::White => &self.white,
                    UiTexture::Glyphs => &self.atlas_group,
                    UiTexture::SdfGlyphs => &self.sdf_group,
                    UiTexture::Image(path) => match self.runtime_groups.get(path) {
                        Some(g) => g,
                        None => match self.images.get(path) {
                            Some(Some(g)) => g,
                            _ => &self.white,
                        },
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
            write_atlas(queue, &self.atlas_texture, &self.atlas.pixels, ATLAS_SIZE);
            self.atlas_uploaded = self.atlas.version;
        }
        if self.sdf.version != self.sdf_uploaded {
            write_atlas(queue, &self.sdf_texture, &self.sdf.pixels, SDF_ATLAS_SIZE);
            self.sdf_uploaded = self.sdf.version;
        }
        if self.runtime.sync(device, queue) {
            self.runtime_groups.clear();
        }
        for b in &list.batches {
            let UiTexture::Image(path) = &b.texture else {
                continue;
            };
            if crate::video::textures::is_runtime(path) {
                if !self.runtime_groups.contains_key(path) {
                    let view = self.runtime.view(device, queue, path);
                    let g = group(device, &self.layout, &self.sampler, &view);
                    self.runtime_groups.insert(path.clone(), g);
                }
                continue;
            }
            if self.images.contains_key(path) {
                continue;
            }
            let g = match crate::sprites::load_texture(path) {
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

fn pipeline(
    device: &wgpu::Device,
    layouts: &[&wgpu::BindGroupLayout],
    fragment: &str,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty ui"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty ui"),
        bind_group_layouts: &layouts.iter().map(|l| Some(*l)).collect::<Vec<_>>(),
        immediate_size: 0,
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32, 4 => Float32x2
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
    crate::render::pipelines::note_pipeline();
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
            entry_point: Some(fragment),
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
        assert_eq!(std::mem::size_of::<super::UiVertex>(), 44);
    }
}
