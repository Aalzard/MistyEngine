//! The colour-blind filter (Project Settings ▸ Player ▸ Accessibility, Lua `Accessibility`):
//! one colour matrix over the final image, after the UI, so menus and HUDs are filtered too. The
//! host calls [`ColorFilter::apply`] after drawing the canvases; with the filter off it does
//! nothing at all.

use crate::accessibility::{self, Mat3};

use super::target::RenderTarget;

const SHADER: &str = r#"
struct Filter { r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32> };
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> rows: Filter;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(pos.xy), 0);
    let rgb = vec3<f32>(dot(rows.r0.xyz, c.rgb), dot(rows.r1.xyz, c.rgb), dot(rows.r2.xyz, c.rgb));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}
"#;

/// GPU resources of the filter pass (made on first use).
#[derive(Default)]
pub struct ColorFilter {
    gpu: Option<Gpu>,
    /// Copy of the final image the pass reads, and its size.
    scratch: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
}

struct Gpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

/// The matrix as three padded rows for the shader.
pub fn uniform_bytes(m: &Mat3) -> [f32; 12] {
    let mut out = [0.0; 12];
    for (i, row) in m.iter().enumerate() {
        out[i * 4..i * 4 + 3].copy_from_slice(row);
    }
    out
}

impl ColorFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter `target`'s final image with the accessibility settings in effect. False (and no
    /// GPU work) when the filter is off.
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
    ) -> bool {
        match accessibility::current().filter_matrix() {
            Some(m) => {
                self.apply_matrix(device, queue, target, &m);
                true
            }
            None => false,
        }
    }

    /// Filter `target`'s final image with `m`.
    pub fn apply_matrix(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        m: &Mat3,
    ) {
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(device));
        queue.write_buffer(&gpu.uniform, 0, bytemuck::cast_slice(&uniform_bytes(m)));
        let size = target.output_size();
        if self.scratch.as_ref().is_none_or(|s| s.2 != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("misty colour filter source"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::COLOR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty colour filter"),
                layout: &gpu.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: gpu.uniform.as_entire_binding(),
                    },
                ],
            });
            self.scratch = Some((tex, group, size));
        }
        let Some((tex, group, _)) = &self.scratch else {
            return;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("misty colour filter"),
        });
        target.copy_output(&mut encoder, tex);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty colour filter"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.output_view(),
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
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

impl Gpu {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty colour filter"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty colour filter"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty colour filter"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("misty colour filter"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(super::COLOR_FORMAT.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty colour filter"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            uniform,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::{ColorBlindMode, filter_matrix};

    #[test]
    fn uniform_rows_are_padded() {
        let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        assert_eq!(
            uniform_bytes(&m),
            [1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0, 0.0, 7.0, 8.0, 9.0, 0.0]
        );
    }

    /// The filter on a real render: the image changes and the shader's result matches the CPU
    /// matrix.
    #[test]
    fn filter_pass_recolours_the_final_image() {
        use crate::render::gpu_tests::{headless, read_back, test_lights, test_meshes};
        use crate::render::{Environment, Frame, PostSettings, Renderer};
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let camera = crate::camera::Camera {
            position: glam::Vec3::new(0.0, 7.0, -12.0),
            rotation: glam::Quat::from_rotation_x(28f32.to_radians()),
            projection: crate::camera::Projection::Perspective { fov_y: 50.0 },
            near: 0.3,
            far: 60.0,
        };
        let (meshes, lights) = (test_meshes(), test_lights());
        let environment = Environment::default();
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        let mut renderer = Renderer::with_queue(&device, &queue);
        let target = renderer.create_target(&device, 96, 64);
        renderer.render(&device, &queue, &target, &frame);
        let before = read_back(&device, &queue, &target);

        let mut filter = ColorFilter::new();
        let m = filter_matrix(ColorBlindMode::Protanopia, 1.0, true).unwrap();
        filter.apply_matrix(&device, &queue, &target, &m);
        let after = read_back(&device, &queue, &target);
        assert_eq!(before.len(), after.len());
        // Every pixel against the CPU matrix (sRGB bytes to linear and back).
        let to_lin = |b: u8| {
            let c = b as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let to_srgb = |l: f32| {
            let l = l.clamp(0.0, 1.0);
            let c = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            (c * 255.0).round()
        };
        let mut differing = 0;
        for (p, q) in before.chunks(4).zip(after.chunks(4)) {
            let want = crate::accessibility::apply(&m, [to_lin(p[0]), to_lin(p[1]), to_lin(p[2])]);
            for (w, got) in want.iter().zip(q) {
                assert!((to_srgb(*w) - *got as f32).abs() <= 3.0, "{p:?} -> {q:?}");
            }
            differing += usize::from(p[..3] != q[..3]);
        }
        assert!(differing > 0, "the filter changed the image");
    }
}
