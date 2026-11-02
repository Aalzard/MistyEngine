//! Shader modules and render pipelines.

use super::{COLOR_FORMAT, DEPTH_FORMAT, LineVertex, SAMPLE_COUNT};

const COMMON: &str = include_str!("shaders/common.wgsl");

pub struct Pipelines {
    pub view_layout: wgpu::BindGroupLayout,
    pub sky: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub lines: wgpu::RenderPipeline,
}

/// Depth behaviour per pipeline.
struct DepthMode {
    write: bool,
    compare: wgpu::CompareFunction,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty view layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty pipeline layout"),
            bind_group_layouts: &[Some(&view_layout)],
            immediate_size: 0,
        });

        let sky = build(
            device,
            &layout,
            "sky",
            include_str!("shaders/sky.wgsl"),
            "vs_fullscreen",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            DepthMode {
                write: false,
                compare: wgpu::CompareFunction::Always,
            },
            None,
        );
        let grid = build(
            device,
            &layout,
            "grid",
            include_str!("shaders/grid.wgsl"),
            "vs_fullscreen",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            DepthMode {
                write: false,
                compare: wgpu::CompareFunction::LessEqual,
            },
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        };
        let lines = build(
            device,
            &layout,
            "lines",
            include_str!("shaders/lines.wgsl"),
            "vs_main",
            &[line_layout],
            wgpu::PrimitiveTopology::LineList,
            DepthMode {
                write: true,
                compare: wgpu::CompareFunction::LessEqual,
            },
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        Self {
            view_layout,
            sky,
            grid,
            lines,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    name: &str,
    source: &str,
    vs_entry: &str,
    buffers: &[wgpu::VertexBufferLayout],
    topology: wgpu::PrimitiveTopology,
    depth: DepthMode,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(format!("{COMMON}\n{source}").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(name),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vs_entry),
            compilation_options: Default::default(),
            buffers: &buffers.iter().cloned().map(Some).collect::<Vec<_>>(),
        },
        primitive: wgpu::PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth.write),
            depth_compare: Some(depth.compare),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: SAMPLE_COUNT,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::COMMON;
    use wgpu::naga;

    /// Every shader must parse and validate (catches WGSL errors without a GPU).
    #[test]
    fn shaders_validate() {
        for (name, src) in [
            ("sky", include_str!("shaders/sky.wgsl")),
            ("grid", include_str!("shaders/grid.wgsl")),
            ("lines", include_str!("shaders/lines.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(&format!("{COMMON}\n{src}"))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
