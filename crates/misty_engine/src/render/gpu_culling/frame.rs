//! The renderer side of GPU culling: which opaque draws are candidates, and the indirect draws
//! of each phase.

use glam::Mat4;

use super::super::batch::{Batch, Merge, addr};
use super::super::frame_lists::FrameLists;
use super::super::{Frame, RenderStats, RenderTarget, Renderer};
use super::{CulledBatches, TargetState};

impl Renderer {
    /// Upload this frame's culling inputs (making the target's Hi-Z state on first use).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::render) fn upload_culling(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        lists: &FrameLists,
        culled: &CulledBatches,
        view_proj: &Mat4,
    ) {
        let size = target.size();
        let mut state = target.culling.lock().unwrap_or_else(|e| e.into_inner());
        if !state.as_ref().is_some_and(|s| s.fits(size)) {
            *state = Some(TargetState::new(device, size));
        }
        let ranges: Vec<(u32, u32, i32)> = culled
            .batches
            .iter()
            .map(|b| {
                self.gpu_mesh(&frame.meshes[b.draw].mesh)
                    .map_or((0, 0, 0), |m| (m.count, m.first_index, m.base_vertex))
            })
            .collect();
        let (Some(culler), Some(state)) = (self.culler.as_mut(), state.as_mut()) else {
            return;
        };
        let bounds = &lists.prep.bounds;
        culler.upload(
            device, queue, state, culled, bounds, &ranges, view_proj, size, true,
        );
    }

    /// Is GPU culling on for this frame?
    pub(in crate::render) fn gpu_culling_active(&self, frame: &Frame) -> bool {
        // Off while AO / contact shadows run: their prepass draws the CPU-culled batches.
        // Dynamic occlusion (baked occlusion data in use) turns the Hi-Z pass on for the view.
        let dynamic = frame
            .environment
            .occlusion
            .as_ref()
            .is_some_and(|o| o.dynamic);
        (self.options.gpu_culling || dynamic && self.options.culling)
            && self.culler.is_some()
            && frame.shaded
            // The Frame Debugger lists CPU-side draws.
            && self.capture.is_none()
            && !self.screen_space_wanted(frame)
            && !super::super::custom_shader::any_in(frame)
    }

    /// Group every loaded opaque draw into batches for the GPU to cull (no CPU frustum test),
    /// ordered by state so neighbours can share a multi-draw. `base` = first free slot.
    pub(in crate::render) fn gpu_batches(
        &mut self,
        frame: &Frame,
        lists: &FrameLists,
        base: u32,
    ) -> CulledBatches {
        let prep = &lists.prep;
        let merge = match self.options.instancing {
            true => Merge::Anywhere,
            false => Merge::None,
        };
        let mut candidates = Vec::new();
        let mut batcher = std::mem::take(&mut self.batcher);
        let mut batches = batcher.build(
            lists.opaque_order.iter().copied(),
            // Draws the baked occlusion data hides are not candidates.
            |i| prep.main_keys[i].filter(|_| !lists.occluded.get(i).copied().unwrap_or(false)),
            merge,
            &mut candidates,
        );
        self.batcher = batcher;
        // Same shader, textures and vertex buffer next to each other (stable: keeps the
        // front-to-back order of first appearance within a state).
        batches.sort_by_key(|b| {
            let key = prep.main_keys[b.draw].unwrap_or_default();
            let buffer = self
                .gpu_mesh(&frame.meshes[b.draw].mesh)
                .map_or(0, |m| m.buffer_id);
            (key[0], key[1], buffer)
        });
        // Re-pack the candidates in the new batch order.
        let mut packed = Vec::with_capacity(candidates.len());
        for b in &mut batches {
            let run = &candidates[b.first as usize..(b.first + b.count) as usize];
            b.first = packed.len() as u32;
            packed.extend_from_slice(run);
        }
        let n = packed.len() as u32;
        CulledBatches {
            batches,
            candidates: packed,
            out_a: base,
            out_b: base + n,
        }
    }

    /// Issue one phase's indirect draws: runs of batches with the same state are one
    /// `multi_draw_indexed_indirect`.
    pub(in crate::render) fn draw_culled(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &Frame,
        lists: &FrameLists,
        culled: &CulledBatches,
        phase: usize,
        stats: &mut RenderStats,
    ) {
        let Some(culler) = self.culler.as_ref() else {
            return;
        };
        let args = &culler.args.buffer;
        let state = |b: &Batch| {
            let d = &frame.meshes[b.draw];
            let pipeline = self.pipelines.mesh_variant(lists.prep.pipeline[b.draw])?;
            let group = self.textures.get_set(&d.material.textures())?;
            let mesh = self.gpu_mesh(&d.mesh)?;
            Some((pipeline, group, mesh))
        };
        let (mut pipeline_at, mut group_at) = (0, 0);
        let mut i = 0;
        while i < culled.batches.len() {
            let Some((pipeline, group, mesh)) = state(&culled.batches[i]) else {
                i += 1;
                continue;
            };
            // Extend the run while the next batch binds the same things.
            let mut run = 1;
            while let Some((p, g, m)) = culled.batches.get(i + run).and_then(state) {
                if addr(p) != addr(pipeline) || addr(g) != addr(group) {
                    break;
                }
                if m.buffer_id != mesh.buffer_id {
                    break;
                }
                run += 1;
            }
            if addr(pipeline) != pipeline_at {
                pipeline_at = addr(pipeline);
                pass.set_pipeline(pipeline);
                stats.set_pass += 1;
            }
            if addr(group) != group_at {
                group_at = addr(group);
                pass.set_bind_group(2, group, &[]);
                stats.texture_binds += 1;
            }
            mesh.bind(pass);
            let offset = culled.args_offset(phase, i);
            if run > 1 {
                pass.multi_draw_indexed_indirect(args, offset, run as u32);
            } else {
                pass.draw_indexed_indirect(args, offset);
            }
            stats.batches += 1;
            // Instance counts stay on the GPU: report the frustum-visible upper bound once.
            if phase == 0 {
                for b in &culled.batches[i..i + run] {
                    let Some(m) = self.gpu_mesh(&frame.meshes[b.draw].mesh) else {
                        continue;
                    };
                    let members =
                        &culled.candidates[b.first as usize..(b.first + b.count) as usize];
                    let n = members
                        .iter()
                        .filter(|d| lists.visible[**d as usize])
                        .count() as u64;
                    stats.saved_by_batching += (n as u32).saturating_sub(1);
                    stats.triangles += n * (m.count / 3) as u64;
                    stats.vertices += n * m.vertex_count as u64;
                }
            }
            i += run;
        }
    }
}
