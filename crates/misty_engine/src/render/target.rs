//! Offscreen render target: HDR MSAA colour + depth, a bloom mip chain, and the final
//! 8-bit image shown in the editor.

use super::{COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, MASK_FORMAT, SAMPLE_COUNT};

/// Non-sRGB view of the final image: samplers return the stored (gamma-encoded) bytes,
/// which is what the egui compositor expects.
const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Bloom levels at 1/2, 1/4, … resolution.
const MAX_BLOOM_LEVELS: u32 = 6;

/// Layouts and sampler a target needs to build its bind groups.
pub(crate) struct TargetLayouts<'a> {
    pub mask: &'a wgpu::BindGroupLayout,
    pub post: &'a wgpu::BindGroupLayout,
    pub composite: &'a wgpu::BindGroupLayout,
    pub sampler: &'a wgpu::Sampler,
}

pub(crate) struct BloomLevel {
    pub view: wgpu::TextureView,
    /// Samples this level (input of the next pass).
    pub group: wgpu::BindGroup,
}

pub struct RenderTarget {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    /// Emission only (bloom source), MSAA + resolved.
    glow_msaa_view: wgpu::TextureView,
    glow_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    hdr_view: wgpu::TextureView,
    output_view: wgpu::TextureView,
    /// The final image (copy source for thumbnails).
    output: wgpu::Texture,
    display_view: wgpu::TextureView,
    mask_view: wgpu::TextureView,
    /// Outline pass input (selection mask).
    pub(crate) mask_bind_group: wgpu::BindGroup,
    /// Samples the resolved glow image (bloom prefilter input).
    pub(crate) hdr_group: wgpu::BindGroup,
    pub(crate) bloom: Vec<BloomLevel>,
    /// HDR image + bloom level 0, for the final composite.
    pub(crate) composite_group: wgpu::BindGroup,
    /// Screen-space effects' textures and AO history (created when first needed).
    pub(crate) screen_space: super::screen_space::TargetSlot,
}

impl RenderTarget {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        layouts: &TargetLayouts,
    ) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let make = |label,
                    size: (u32, u32),
                    format,
                    samples,
                    usage,
                    view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
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
        let sampled = attach | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let full = (width, height);

        let msaa = make(
            "misty msaa hdr",
            full,
            HDR_FORMAT,
            SAMPLE_COUNT,
            attach,
            &[],
        );
        let depth = make("misty depth", full, DEPTH_FORMAT, SAMPLE_COUNT, attach, &[]);
        let hdr = make("misty hdr", full, HDR_FORMAT, 1, sampled, &[]);
        let glow_msaa = make(
            "misty msaa glow",
            full,
            HDR_FORMAT,
            SAMPLE_COUNT,
            attach,
            &[],
        );
        let glow = make("misty glow", full, HDR_FORMAT, 1, sampled, &[]);
        let output = make(
            "misty output",
            full,
            COLOR_FORMAT,
            1,
            sampled | wgpu::TextureUsages::COPY_SRC,
            &[DISPLAY_FORMAT],
        );
        let mask = make("misty selection mask", full, MASK_FORMAT, 1, sampled, &[]);

        let sample_group = |label, layout: &wgpu::BindGroupLayout, tex: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(tex),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(layouts.sampler),
                    },
                ],
            })
        };

        let hdr_view = view(&hdr);
        let glow_view = view(&glow);
        let hdr_group = sample_group("misty glow group", layouts.post, &glow_view);

        // Bloom chain: stop before levels get tiny.
        let levels =
            MAX_BLOOM_LEVELS.min((width.min(height) as f32).log2().floor().max(2.0) as u32 - 1);
        let bloom: Vec<BloomLevel> = (0..levels)
            .map(|i| {
                let size = ((width >> (i + 1)).max(1), (height >> (i + 1)).max(1));
                let tex = make("misty bloom", size, HDR_FORMAT, 1, sampled, &[]);
                let v = view(&tex);
                let group = sample_group("misty bloom group", layouts.post, &v);
                BloomLevel { view: v, group }
            })
            .collect();

        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty composite group"),
            layout: layouts.composite,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(layouts.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&bloom[0].view),
                },
            ],
        });

        let mask_view = view(&mask);
        let mask_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty mask bind group"),
            layout: layouts.mask,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            }],
        });
        let display_view = output.create_view(&wgpu::TextureViewDescriptor {
            label: Some("misty display view"),
            format: Some(DISPLAY_FORMAT),
            ..Default::default()
        });
        Self {
            width,
            height,
            msaa_view: view(&msaa),
            glow_msaa_view: view(&glow_msaa),
            glow_view,
            depth_view: view(&depth),
            hdr_view,
            output_view: view(&output),
            output,
            display_view,
            mask_view,
            mask_bind_group,
            hdr_group,
            bloom,
            composite_group,
            screen_space: Default::default(),
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

    /// Copy the final image into `dst` (same size, `Rgba8Unorm` or `Rgba8UnormSrgb`).
    pub fn copy_output(&self, encoder: &mut wgpu::CommandEncoder, dst: &wgpu::Texture) {
        encoder.copy_texture_to_texture(
            self.output.as_image_copy(),
            dst.as_image_copy(),
            self.output.size(),
        );
    }

    /// MSAA resolve destination of the main pass.
    pub(crate) fn hdr_view(&self) -> &wgpu::TextureView {
        &self.hdr_view
    }

    /// Final 8-bit sRGB image (composite pass output).
    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }

    pub(crate) fn glow_msaa_view(&self) -> &wgpu::TextureView {
        &self.glow_msaa_view
    }

    pub(crate) fn glow_view(&self) -> &wgpu::TextureView {
        &self.glow_view
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
