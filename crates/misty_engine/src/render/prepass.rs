//! Optional depth prepass: Opaque draws write depth first (no fragment shader), then the main
//! pass shades them with a less-or-equal test and no depth writes, so every covered pixel is
//! shaded once. It reuses the mesh shader's vertex stage (`@invariant` position), so depths
//! match exactly. Cutout and blended draws are unchanged.

use std::collections::HashMap;

use super::frame_lists::FrameLists;
use super::pipelines::{self, Depth, MESH_SRC, Pipelines, Spec};
use super::surface::PipelineKey;
use super::{Frame, RenderStats, Renderer, RenderingMode, batch};

#[derive(Default)]
pub(super) struct Prepass {
    /// Depth-only pipelines by double-sidedness.
    depth: HashMap<bool, wgpu::RenderPipeline>,
    /// Main-pass variants that test against the prepass depth.
    shade: HashMap<PipelineKey, wgpu::RenderPipeline>,
}

/// Does the prepass draw this variant?
pub(super) fn applies(key: PipelineKey) -> bool {
    key.mode == RenderingMode::Opaque
}

impl Prepass {
    pub fn ensure(&mut self, device: &wgpu::Device, p: &Pipelines, key: PipelineKey) {
        if !applies(key) {
            return;
        }
        let buffers = pipelines::mesh_buffers();
        let layouts = [&p.view_layout, &p.mesh_layout, p.group2_layout(key)];
        self.depth.entry(key.double_sided).or_insert_with(|| {
            pipelines::build(
                device,
                Spec {
                    vs: "vs_main",
                    fs: None,
                    buffers: &buffers,
                    cull: key.cull(),
                    depth: Depth::main(true, wgpu::CompareFunction::Less),
                    color: None,
                    glow: None,
                    samples: super::sample_count(),
                    ..Spec::new("depth prepass", MESH_SRC, &layouts[..2])
                },
            )
        });
        self.shade.entry(key).or_insert_with(|| {
            pipelines::build(
                device,
                Spec {
                    vs: "vs_main",
                    fs: Some(key.entry()),
                    buffers: &buffers,
                    cull: key.cull(),
                    depth: Depth::main(false, wgpu::CompareFunction::LessEqual),
                    blend: key.blend(),
                    glow: Some(true),
                    velocity: true,
                    ..Spec::new(
                        "mesh after prepass",
                        super::variants::mesh_source(key),
                        &layouts,
                    )
                },
            )
        });
    }

    /// Main-pass pipeline of a prepassed draw.
    pub fn shade_variant(&self, key: PipelineKey) -> Option<&wgpu::RenderPipeline> {
        self.shade.get(&key)
    }
}

impl Renderer {
    /// Is the depth prepass on for this frame? (GPU culling replaces it when both are on.)
    pub(super) fn prepass_active(&self, frame: &Frame) -> bool {
        // `.shader` materials may move their vertices, so their depth can't come from a prepass.
        self.options.depth_prepass
            && frame.shaded
            && !self.gpu_culling_active(frame)
            && !super::custom_shader::any_in(frame)
    }

    /// Draw the Opaque batches' depth into the target's depth buffer.
    pub(super) fn depth_prepass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        frame: &Frame,
        lists: &FrameLists,
        stats: &mut RenderStats,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty depth prepass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &self.mesh_bind_group, &[]);
        let mut pipeline_at = 0;
        use super::frame_debug::{self, EventKind, EventTarget, FrameEvent, events};
        let ctx = self.capture.is_some().then(|| {
            let kind = events::ListKind::Prepass;
            events::BatchContext::new(frame, &lists.opaque, self.options.instancing, kind)
        });
        if self.capture.is_some() {
            let size = depth.texture().size();
            let target = || {
                let size = (size.width, size.height);
                frame_debug::target("Depth (MSAA ×4)", size, "Depth32Float")
            };
            frame_debug::pass(&self.capture, &["Depth Prepass"], target, EventTarget::None);
            frame_debug::record(&self.capture, |_| {
                FrameEvent::new(EventKind::Clear, "Clear (depth)")
            });
        }
        for b in &lists.opaque {
            let key = lists.prep.pipeline[b.draw];
            if !applies(key) {
                continue;
            }
            let (Some(pipeline), Some(m)) = (
                self.prepass.depth.get(&key.double_sided),
                self.gpu_mesh(&frame.meshes[b.draw].mesh),
            ) else {
                continue;
            };
            if !frame_debug::record(&self.capture, |r| {
                let shader = "Depth Only · depth.wgsl".to_owned();
                let depth = "Less, write on";
                let ds = key.double_sided;
                events::mesh_event(r, frame, b, m, ctx.as_ref(), shader, depth, None, ds)
            }) {
                continue;
            }
            if batch::addr(pipeline) != pipeline_at {
                pipeline_at = batch::addr(pipeline);
                pass.set_pipeline(pipeline);
                stats.set_pass += 1;
            }
            m.draw(&mut pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
    }
}
