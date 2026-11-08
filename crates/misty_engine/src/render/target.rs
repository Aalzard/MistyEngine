//! Offscreen render target: HDR MSAA colour + depth, a bloom mip chain, and the final
//! 8-bit image shown in the editor.
//!
//! An upscaled target (Quality ▸ Upscaling, see [`super::taa`]) renders the scene at a lower
//! internal resolution ([`RenderTarget::size`]): MSAA colour, depth, motion vectors, glow,
//! the resolved HDR image and every screen-space buffer are that size. The temporal upscaler
//! writes a sharp image at the output resolution ([`RenderTarget::output_size`]) into a
//! second HDR image; bloom, tonemapping, FXAA / sharpening and the canvas UI run at the
//! output resolution from there.

use super::pipelines::VELOCITY_FORMAT;
use super::{COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, MASK_FORMAT};

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
    /// Unique per target (per-view state such as fog history is keyed by it).
    pub(crate) id: u64,
    /// Internal (render) resolution: the scene passes draw at this size.
    width: u32,
    height: u32,
    /// Output resolution: post-processing and the final image.
    out_width: u32,
    out_height: u32,
    /// Share of the output resolution rendered (1 = native, no upscaling).
    scale: f32,
    /// Output-resolution HDR image the temporal upscaler writes (None = native target: the
    /// resolved HDR image is the post input).
    upscaled: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// MSAA samples of the main pass targets (the renderer's when made).
    samples: u32,
    msaa_view: wgpu::TextureView,
    /// Emission only (bloom source), MSAA + resolved.
    glow_msaa_view: wgpu::TextureView,
    glow_view: wgpu::TextureView,
    /// Motion vectors of the main pass (MSAA, read per sample by TAA).
    velocity_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    hdr_view: wgpu::TextureView,
    /// The resolved HDR image (before bloom and tonemapping).
    hdr: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// The final image (copy source for thumbnails).
    output: wgpu::Texture,
    display_view: wgpu::TextureView,
    /// Composite result when a finish pass (FXAA, sharpen) runs before the output.
    pre_finish_view: wgpu::TextureView,
    pub(crate) pre_finish_group: wgpu::BindGroup,
    mask_view: wgpu::TextureView,
    /// Outline pass input (selection mask).
    pub(crate) mask_bind_group: wgpu::BindGroup,
    /// Samples the resolved glow image (bloom prefilter input).
    pub(crate) hdr_group: wgpu::BindGroup,
    pub(crate) bloom: Vec<BloomLevel>,
    /// HDR image + bloom level 0 + exposure, for the final composite.
    pub(crate) composite_group: wgpu::BindGroup,
    /// Auto Exposure's multiplier for the composite (1 while it is off, see `postfx`).
    pub(crate) exposure: wgpu::Buffer,
    /// The composite's sampler (groups made later, see [`RenderTarget::composite_group_for`]).
    sampler: wgpu::Sampler,
    /// Screen-space effects' textures and AO history (created when first needed).
    pub(crate) screen_space: super::screen_space::TargetSlot,
    /// SSGI / SSR images, lit colour history and texels (created when first needed).
    pub(crate) screen_trace: super::screen_trace::TargetSlot,
    /// Misty GI clipmaps and probes (created when first needed).
    pub(crate) misty_gi: super::misty_gi::TargetSlot,
    /// Temporal shadow mask buffers and history (created when first needed).
    pub(crate) shadow_mask: super::shadow_denoise::TargetSlot,
    /// GPU culling's Hi-Z pyramid and last frame's visibility (made on first use).
    pub(in crate::render) culling: std::sync::Mutex<Option<super::gpu_culling::TargetState>>,
    /// Grass visibility buffer and resolve images (made on first use, grass_vis.rs).
    pub(crate) grass_vis: super::grass_vis::TargetSlot,
}

impl RenderTarget {
    /// A target whose final image is `out` pixels, rendering the scene at `scale` of that
    /// (1 = native).
    pub(crate) fn new(
        device: &wgpu::Device,
        out: (u32, u32),
        scale: f32,
        layouts: &TargetLayouts,
    ) -> Self {
        let (out_width, out_height) = (out.0.max(1), out.1.max(1));
        let scale = if scale.is_finite() {
            scale.clamp(0.25, 1.0)
        } else {
            1.0
        };
        let (width, height) = render_size((out_width, out_height), scale);
        let native = (width, height) == (out_width, out_height);
        let scale = if native { 1.0 } else { scale };
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
        let out = (out_width, out_height);

        let msaa = make(
            "misty msaa hdr",
            full,
            HDR_FORMAT,
            super::sample_count(),
            attach,
            &[],
        );
        // Sampled by the sun shafts, GPU culling's Hi-Z pyramid and the scene copy (water).
        let depth = make(
            "misty depth",
            full,
            DEPTH_FORMAT,
            super::sample_count(),
            sampled,
            &[],
        );
        // Copy source for reflection probe captures.
        let hdr = make(
            "misty hdr",
            full,
            HDR_FORMAT,
            1,
            // COPY_DST: temporal anti-aliasing writes its result back.
            sampled | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            &[],
        );
        let glow_msaa = make(
            "misty msaa glow",
            full,
            HDR_FORMAT,
            super::sample_count(),
            attach,
            &[],
        );
        let glow = make("misty glow", full, HDR_FORMAT, 1, sampled, &[]);
        // Sampled by TAA (sample 0, like the depth).
        let velocity = make(
            "misty velocity",
            full,
            VELOCITY_FORMAT,
            super::sample_count(),
            sampled,
            &[],
        );
        let output = make(
            "misty output",
            out,
            COLOR_FORMAT,
            1,
            sampled | wgpu::TextureUsages::COPY_SRC,
            &[DISPLAY_FORMAT],
        );
        let pre_finish = make("misty pre-finish", out, COLOR_FORMAT, 1, sampled, &[]);
        // The upscaler's output-resolution HDR image (post-processing input).
        let upscaled = (!native).then(|| {
            let t = make("misty upscaled hdr", out, HDR_FORMAT, 1, sampled, &[]);
            let v = view(&t);
            (t, v)
        });
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

        // Bloom chain (output resolution): stop before levels get tiny.
        let levels = MAX_BLOOM_LEVELS
            .min((out_width.min(out_height) as f32).log2().floor().max(2.0) as u32 - 1);
        let bloom: Vec<BloomLevel> = (0..levels)
            .map(|i| {
                let size = (
                    (out_width >> (i + 1)).max(1),
                    (out_height >> (i + 1)).max(1),
                );
                let tex = make("misty bloom", size, HDR_FORMAT, 1, sampled, &[]);
                let v = view(&tex);
                let group = sample_group("misty bloom group", layouts.post, &v);
                BloomLevel { view: v, group }
            })
            .collect();

        let exposure = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("misty exposure"),
                contents: bytemuck::bytes_of(&super::postfx::EXPOSURE_OFF),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            },
        );
        let composite_group = composite_group(
            device,
            layouts.composite,
            layouts.sampler,
            upscaled.as_ref().map_or(&hdr_view, |u| &u.1),
            &bloom[0].view,
            &exposure,
        );

        let pre_finish_view = view(&pre_finish);
        let pre_finish_group = sample_group("misty finish group", layouts.post, &pre_finish_view);
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
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            samples: super::sample_count(),
            width,
            height,
            out_width,
            out_height,
            scale,
            upscaled,
            msaa_view: view(&msaa),
            glow_msaa_view: view(&glow_msaa),
            glow_view,
            velocity_view: view(&velocity),
            depth_view: view(&depth),
            hdr_view,
            hdr,
            output_view: view(&output),
            output,
            display_view,
            pre_finish_view,
            pre_finish_group,
            mask_view,
            mask_bind_group,
            hdr_group,
            bloom,
            composite_group,
            exposure,
            sampler: layouts.sampler.clone(),
            screen_space: Default::default(),
            screen_trace: Default::default(),
            misty_gi: Default::default(),
            shadow_mask: Default::default(),
            culling: Default::default(),
            grass_vis: Default::default(),
        }
    }

    /// A composite group reading `hdr` (an output-resolution image after the resolved one:
    /// depth of field, motion blur) instead of the resolved image.
    pub(crate) fn composite_group_for(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        hdr: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        composite_group(
            device,
            layout,
            &self.sampler,
            hdr,
            &self.bloom[0].view,
            &self.exposure,
        )
    }

    /// Internal resolution the scene renders at (the output size unless upscaled).
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Resolution of the final image.
    pub fn output_size(&self) -> (u32, u32) {
        (self.out_width, self.out_height)
    }

    /// Share of the output resolution the scene renders at (1 = native).
    pub fn render_scale(&self) -> f32 {
        self.scale
    }

    /// The scene renders below the output resolution and is upscaled temporally.
    pub fn upscaled(&self) -> bool {
        self.upscaled.is_some()
    }

    /// MSAA samples of the main pass targets (see `Renderer::target_fits`).
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Mark this view secondary for Misty GI (the editor's Scene view): near a primary view
    /// (the Game view) it gathers from that view's clipmaps instead of updating its own, so
    /// the editor doesn't pay for the world-space passes twice.
    pub fn set_misty_gi_secondary(&self, on: bool) {
        self.misty_gi.set_secondary(on);
    }

    /// Aspect ratio of the final image (the camera's).
    pub fn aspect(&self) -> f32 {
        self.out_width as f32 / self.out_height as f32
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

    /// Linear HDR scene colour of the last render (reflection probe captures).
    pub(crate) fn hdr_texture(&self) -> &wgpu::Texture {
        &self.hdr
    }

    /// MSAA resolve destination of the main pass.
    pub(crate) fn hdr_view(&self) -> &wgpu::TextureView {
        &self.hdr_view
    }

    /// HDR image post-processing reads (output resolution): the upscaler's result, or the
    /// resolved HDR image of a native target.
    pub(crate) fn post_hdr_view(&self) -> &wgpu::TextureView {
        self.upscaled.as_ref().map_or(&self.hdr_view, |u| &u.1)
    }

    /// Final 8-bit sRGB image (composite pass output).
    pub(crate) fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }

    /// Composite output when a finish pass follows.
    pub(crate) fn pre_finish_view(&self) -> &wgpu::TextureView {
        &self.pre_finish_view
    }

    pub(crate) fn glow_msaa_view(&self) -> &wgpu::TextureView {
        &self.glow_msaa_view
    }

    pub(crate) fn glow_view(&self) -> &wgpu::TextureView {
        &self.glow_view
    }

    /// Motion vectors of the main pass (MSAA).
    pub(crate) fn velocity_view(&self) -> &wgpu::TextureView {
        &self.velocity_view
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

/// The composite's inputs: HDR image, sampler, bloom level 0 and the exposure buffer.
fn composite_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    hdr: &wgpu::TextureView,
    bloom: &wgpu::TextureView,
    exposure: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty composite group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(hdr),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(bloom),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: exposure.as_entire_binding(),
            },
        ],
    })
}

/// Internal resolution for an `out` image rendered at `scale` (each side rounded, at least 1).
pub fn render_size(out: (u32, u32), scale: f32) -> (u32, u32) {
    let side = |v: u32| ((v as f32 * scale).round() as u32).clamp(1, v.max(1));
    (side(out.0), side(out.1))
}

#[cfg(test)]
mod tests {
    use super::render_size;

    #[test]
    fn render_size_rounds_each_side() {
        assert_eq!(render_size((1920, 1080), 1.0), (1920, 1080));
        assert_eq!(render_size((1920, 1080), 0.5), (960, 540));
        assert_eq!(render_size((1920, 1080), 2.0 / 3.0), (1280, 720));
        assert_eq!(render_size((2560, 1440), 0.58), (1485, 835));
        assert_eq!(render_size((3, 1), 0.25), (1, 1));
    }
}
