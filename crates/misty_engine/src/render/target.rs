//! Offscreen render target: MSAA colour + depth, resolved into a sampleable texture.

use super::{COLOR_FORMAT, DEPTH_FORMAT, MASK_FORMAT, SAMPLE_COUNT};

/// Non-sRGB view of the resolved image: samplers return the stored (gamma-encoded) bytes,
/// which is what the egui compositor expects.
const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct RenderTarget {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    display_view: wgpu::TextureView,
    mask_view: wgpu::TextureView,
    /// Outline pass input (selection mask).
    pub(crate) mask_bind_group: wgpu::BindGroup,
}

impl RenderTarget {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        mask_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let make = |label, format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats,
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());

        let msaa = make("misty msaa colour", COLOR_FORMAT, SAMPLE_COUNT, attach, &[]);
        let depth = make("misty depth", DEPTH_FORMAT, SAMPLE_COUNT, attach, &[]);
        let resolve = make(
            "misty resolve",
            COLOR_FORMAT,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING,
            &[DISPLAY_FORMAT],
        );
        let mask = make(
            "misty selection mask",
            MASK_FORMAT,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING,
            &[],
        );
        let mask_view = view(&mask);
        let mask_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty mask bind group"),
            layout: mask_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            }],
        });
        let display_view = resolve.create_view(&wgpu::TextureViewDescriptor {
            label: Some("misty display view"),
            format: Some(DISPLAY_FORMAT),
            ..Default::default()
        });
        Self {
            width,
            height,
            msaa_view: view(&msaa),
            depth_view: view(&depth),
            resolve_view: view(&resolve),
            display_view,
            mask_view,
            mask_bind_group,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    /// The final image for display in the editor (gamma-encoded bytes).
    pub fn view(&self) -> &wgpu::TextureView {
        &self.display_view
    }

    pub(crate) fn resolve_view(&self) -> &wgpu::TextureView {
        &self.resolve_view
    }

    pub(crate) fn mask_view(&self) -> &wgpu::TextureView {
        &self.mask_view
    }

    pub(crate) fn msaa_view(&self) -> &wgpu::TextureView {
        &self.msaa_view
    }

    pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }
}
