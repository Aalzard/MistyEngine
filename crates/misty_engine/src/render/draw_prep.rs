//! Per-draw values of one render, computed once and in parallel: the LOD level state, world
//! bounds, the shader variants actually used (a cross-fading LOD level draws with the cutout
//! variant so plain opaque shaders never contain a discard), batch keys and `draws[]` entries.

use rayon::prelude::*;

use super::batch::{BatchKey, addr};
use super::culling::Aabb;
use super::lod::{LodState, screen_height};
use super::surface::{PipelineKey, ShadowKey};
use super::{DrawUniform, Frame, MeshDraw, Renderer, RenderingMode};

/// Below this many draws the work stays on the calling thread.
pub(super) const PARALLEL_MIN: usize = 512;

pub(super) struct DrawPrep {
    /// World box (`None` = mesh not loaded or hidden by its LOD Group).
    pub bounds: Vec<Option<Aabb>>,
    /// Main pass shader variant and shadow caster variant (`None` = casts no shadow).
    pub pipeline: Vec<PipelineKey>,
    pub shadow: Vec<Option<ShadowKey>>,
    pub main_keys: Vec<Option<BatchKey>>,
    pub shadow_keys: Vec<Option<BatchKey>>,
    pub uniforms: Vec<DrawUniform>,
    /// Per draw: mesh fingerprint and model matrix, kept for next frame's motion vectors.
    pub motion: Vec<(u64, glam::Mat4)>,
    /// Per draw: drawn as virtual geometry (pool mesh index) instead of by the CPU lists.
    /// Per draw: (pool mesh, material slot) when it draws through virtual geometry clusters.
    pub virtual_mesh: Vec<Option<(u32, u32)>>,
}

/// LOD state of every draw for `frame`'s camera.
pub(super) fn lod_states(frame: &Frame, bias: f32) -> Vec<LodState> {
    frame
        .meshes
        .iter()
        .map(|d| match d.lod {
            None => LodState::Visible,
            Some(l) => l.state(screen_height(frame.camera, l.center, l.size, bias)),
        })
        .collect()
}

/// The main pass variant of a draw: cross-fading opaque levels clip like Cutout.
pub(super) fn pipeline_key(d: &MeshDraw, lod: LodState) -> PipelineKey {
    let mut key = d.material.pipeline_key();
    if matches!(lod, LodState::Fading(_)) && key.mode == RenderingMode::Opaque && !key.unlit {
        key.mode = RenderingMode::Cutout;
    }
    key
}

/// The shadow caster variant of a draw (`None` = no shadow).
pub(super) fn shadow_key(d: &MeshDraw, lod: LodState) -> Option<ShadowKey> {
    let mut key = d.material.shadow_key().filter(|_| d.cast_shadows)?;
    if matches!(lod, LodState::Fading(_)) {
        key.cutout = true;
    }
    Some(key)
}

/// Map `f` over `0..n`, in parallel for large frames.
pub(super) fn map_draws<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    if n >= PARALLEL_MIN {
        (0..n).into_par_iter().map(f).collect()
    } else {
        (0..n).map(f).collect()
    }
}

impl Renderer {
    /// `prev_draws` = last frame's draws of this target (object motion vectors).
    pub(super) fn draw_prep(
        &self,
        frame: &Frame,
        lod: Vec<LodState>,
        prev_draws: &[(u64, glam::Mat4)],
    ) -> DrawPrep {
        crate::profile_scope!("Render.DrawPrep");
        let n = frame.meshes.len();
        let shaded = frame.shaded;
        let per_draw = map_draws(n, |i| {
            let d = &frame.meshes[i];
            let state = lod[i];
            let print = super::motion::fingerprint(d);
            let prev_model = super::motion::prev_model(prev_draws, i, d, print);
            let mesh = self.gpu_mesh(&d.mesh);
            let bounds = match (mesh, state) {
                (_, LodState::Hidden) | (None, _) => None,
                (Some(m), _) => Some(m.bounds.transformed(&d.model)),
            };
            let pipeline = pipeline_key(d, state);
            let shadow = shadow_key(d, state);
            // Virtual geometry takes lit Opaque and Cutout single-sided surfaces of virtual
            // meshes (not while a LOD Group level fades: the dither needs the cutout shader).
            let virtual_mesh = self.vgeo.mesh_index(&d.mesh).filter(|_| {
                bounds.is_some()
                    && d.material.custom.is_none()
                    && state == LodState::Visible
                    && !pipeline.unlit
                    && matches!(pipeline.mode, RenderingMode::Opaque | RenderingMode::Cutout)
                    && !pipeline.double_sided
            });
            let group = || self.textures.get_set(&d.material.textures());
            let main = (|| {
                bounds?;
                // `.shader` materials bring their own pipeline and group 2.
                let (p, g) = match &d.material.custom {
                    Some(_) => self.material_resources(&d.material)?,
                    None => (self.pipelines.mesh_variant(pipeline)?, group()?),
                };
                Some([addr(p), addr(g), addr(mesh?)])
            })()
            .filter(|_| shaded && virtual_mesh.is_none());
            let caster = (|| {
                let key = shadow?;
                bounds?;
                let p = self.pipelines.shadow_variant(key)?;
                let g = if key.cutout { addr(group()?) } else { 0 };
                Some([addr(p), addr(mesh?), g])
            })()
            .filter(|_| virtual_mesh.is_none());
            let fade = match state {
                LodState::Fading(f) => f,
                _ => 0.0,
            };
            let mut u = super::draw_uniform(d, fade, prev_model);
            if fade != 0.0 && d.material.mode == RenderingMode::Opaque {
                // Opaque levels drawn with the cutout variant: only the dither clips.
                u.surface[1] = f32::MIN;
            }
            let m = (print, d.model);
            (bounds, pipeline, shadow, main, caster, u, m, virtual_mesh)
        });
        let mut p = DrawPrep {
            bounds: Vec::with_capacity(n),
            pipeline: Vec::with_capacity(n),
            shadow: Vec::with_capacity(n),
            main_keys: Vec::with_capacity(n),
            shadow_keys: Vec::with_capacity(n),
            uniforms: Vec::with_capacity(n),
            motion: Vec::with_capacity(n),
            virtual_mesh: Vec::with_capacity(n),
        };
        for (b, pk, sk, mk, ck, u, m, v) in per_draw {
            p.bounds.push(b);
            p.pipeline.push(pk);
            p.shadow.push(sk);
            p.main_keys.push(mk);
            p.shadow_keys.push(ck);
            p.uniforms.push(u);
            p.motion.push(m);
            p.virtual_mesh.push(v);
        }
        p
    }
}
