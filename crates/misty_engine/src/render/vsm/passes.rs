//! The renderer's side of the virtual shadow map: what changed among the casters, the frame's
//! plan (pages, rectangles, the views the frame lists cull for), and the scratch / copy passes.

use glam::Mat4;

use super::{VsmInput, caster_signature};
use crate::render::culling::Frustum;
use crate::render::draw_prep::DrawPrep;
use crate::render::frame_lists::FrameLists;
use crate::render::shadows::{ShadowPlan, VIEW_STRIDE};
use crate::render::{Casters, Frame, RenderStats, RenderTarget, Renderer};

impl Renderer {
    /// Plan the sun's virtual shadow map for this frame (when the plan gives it one): caster
    /// changes, page cache, rectangles. Their views join the plan after the local layers.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::render) fn prepare_vsm(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        target: &RenderTarget,
        plan: &mut ShadowPlan,
        prep: &DrawPrep,
        casters: &[(glam::Vec3, glam::Vec3)],
    ) {
        let (Some(dir), true) = (plan.vsm, self.vsm.gpu.is_some()) else {
            self.vsm.frame = None;
            return;
        };
        // Casters whose shadow may have changed: moved, appeared, vanished, or posed.
        let now = frame
            .meshes
            .iter()
            .enumerate()
            .filter(|(i, d)| d.cast_shadows && prep.shadow[*i].is_some())
            .filter_map(|(i, d)| {
                let b = prep.bounds[i]?;
                let key =
                    prep.shadow[i].map_or(0, |k| k.cutout as u64 | (k.double_sided as u64) << 1);
                let print = prep.motion.get(i).map_or(0, |m| m.0);
                let posed = matches!(&d.mesh, crate::scene::MeshRef::Asset(k)
                    if crate::skinning::is_pose_key(k));
                Some((caster_signature(print, &d.model, key), b, posed))
            });
        let mut changed = self.vsm.caster_changes(now);
        let states = self.terrains.chunk_states(frame.terrains);
        let terrains = &self.terrains;
        let (chunks, reset) = self
            .vsm
            .terrain_changes(states, |i, c| terrains.chunk_bounds(i, c));
        changed.extend(chunks);
        let settings = crate::quality::ShadowSettings {
            map_size: self.shadows.directional_size,
            ..self.quality.shadow_settings()
        };
        let sun = frame
            .lights
            .iter()
            .zip(&plan.light_layers)
            .find(|(_, l)| **l == Some(-1))
            .map(|(l, _)| l);
        let soft = settings.soft != crate::quality::Tier::Off;
        let sun_size = match sun {
            Some(l) if soft => crate::render::soft_shadows::source_size(l),
            _ => 0.0,
        };
        // Virtual geometry draws into its first few shadow views only.
        let vgeo = self.vgeo.on;
        let max_rects = if vgeo {
            crate::render::vgeo::MAX_SHADOW_VIEWS.saturating_sub(plan.vsm_first_slot())
        } else {
            super::MAX_RECTS
        };
        let input = VsmInput {
            camera: frame.camera,
            sun_dir: dir,
            sun_size,
            target_height: target.size().1,
            settings: &settings,
            casters,
            changed: &changed,
            reset,
            frame: self.vsm_frame,
            // One render only (the editor sets it per Scene view render).
            debug: std::mem::take(&mut self.vsm.debug),
            max_rects,
            // (Last frame's crowns: the trees are prepared after the frame lists.)
            sway: self.trees.sway_bounds(),
        };
        self.vsm.prepare(device, queue, &input);
        plan.vsm_views = self
            .vsm
            .frame
            .iter()
            .flat_map(|f| f.rects.iter().map(|r| r.view_proj))
            .collect::<Vec<Mat4>>();
    }

    /// Draw this frame's page rectangles into the scratch atlas and copy their pages into the
    /// pool, one scratch fill at a time.
    pub(in crate::render) fn vsm_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        plan: &ShadowPlan,
        lists: &FrameLists,
        stats: &mut RenderStats,
    ) {
        let (Some(f), Some(gpu)) = (&self.vsm.frame, &self.vsm.gpu) else {
            return;
        };
        let first = plan.vsm_first_slot();
        let page = super::clipmap::PAGE as f32;
        for (round, copies) in f.rounds.iter().enumerate() {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("misty vsm pages"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &gpu.scratch_view,
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
                pass.set_bind_group(1, &self.draws_bind_group, &[]);
                for (j, r) in f.rects.iter().enumerate() {
                    if r.round != round {
                        continue;
                    }
                    let Some(list) = lists.shadows.get(first + j) else {
                        continue;
                    };
                    let (x, y) = (r.at.x as f32 * page, r.at.y as f32 * page);
                    let (w, h) = (r.rect.size.x as f32 * page, r.rect.size.y as f32 * page);
                    pass.set_viewport(x, y, w, h, 0.0, 1.0);
                    pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
                    let offset = (j as u64 * VIEW_STRIDE) as u32;
                    pass.set_bind_group(0, &gpu.view_group, &[offset]);
                    let frustum = Frustum::from_matrix(&r.view_proj);
                    let casters = Casters {
                        grass: false,
                        trees: Some(plan.cascades.len() + j),
                        chunks: Some(&frustum),
                    };
                    self.draw_casters(&mut pass, frame, first + j, lists, list, casters, stats);
                }
            }
            self.vsm.encode_copy(encoder, copies.clone());
        }
    }
}
