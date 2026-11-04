//! Final-image pass after the composite: Quality ▸ FXAA and the Sharpen upscale filter
//! (`shaders/finish.wgsl`). When neither is on, the composite writes the output directly.

use super::COLOR_FORMAT;
use super::pipelines::{Spec, build};
use crate::quality::{QualityLevel, UpscaleFilter};

pub(super) const SRC: &str = include_str!("shaders/finish.wgsl");

pub struct FinishPipelines {
    fxaa: wgpu::RenderPipeline,
    sharpen: wgpu::RenderPipeline,
    both: wgpu::RenderPipeline,
}

impl FinishPipelines {
    /// `layouts` = [view, post (texture + sampler)].
    pub(super) fn new(device: &wgpu::Device, layouts: &[&wgpu::BindGroupLayout]) -> Self {
        let make = |name, fs| {
            build(
                device,
                Spec {
                    fs: Some(fs),
                    depth: None,
                    samples: 1,
                    color: Some(COLOR_FORMAT),
                    glow: None,
                    ..Spec::new(name, SRC, layouts)
                },
            )
        };
        Self {
            fxaa: make("finish fxaa", "fs_fxaa"),
            sharpen: make("finish sharpen", "fs_sharpen"),
            both: make("finish fxaa sharpen", "fs_both"),
        }
    }

    /// The pass this level needs, or None when the composite can write the output itself.
    pub(super) fn pick(&self, q: &QualityLevel) -> Option<&wgpu::RenderPipeline> {
        match (q.fxaa, q.upscale_filter == UpscaleFilter::Sharpen) {
            (true, true) => Some(&self.both),
            (true, false) => Some(&self.fxaa),
            (false, true) => Some(&self.sharpen),
            (false, false) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::quality::{QualityLevel, UpscaleFilter};
    use crate::render::RenderOptions;
    use crate::render::gpu_tests::{headless, render_with_quality};

    /// Pixels that differ by more than `tol` in any channel.
    fn changed(a: &[u8], b: &[u8], tol: u8) -> usize {
        a.chunks(4)
            .zip(b.chunks(4))
            .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > tol))
            .count()
    }

    #[test]
    fn fxaa_and_sharpen_touch_edges_but_keep_the_image() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let render = |fxaa, upscale_filter| {
            let q = QualityLevel {
                fxaa,
                upscale_filter,
                ..QualityLevel::high()
            };
            render_with_quality(&device, &queue, RenderOptions::default(), Some(&q)).0
        };
        let plain = render(false, UpscaleFilter::Bilinear);
        let total = plain.len() / 4;
        for (fxaa, filter) in [
            (true, UpscaleFilter::Bilinear),
            (false, UpscaleFilter::Sharpen),
            (true, UpscaleFilter::Sharpen),
        ] {
            let image = render(fxaa, filter);
            let n = changed(&plain, &image, 2);
            assert!(n > 0, "{fxaa} {filter:?} changed nothing");
            // Flat areas stay as they are: under a quarter of the pixels move noticeably.
            assert!(changed(&plain, &image, 40) < total / 4, "{fxaa} {filter:?}");
        }
        // Nearest is only a display sampler: the rendered image is the same.
        assert!(render(false, UpscaleFilter::Nearest) == plain);
    }
}
