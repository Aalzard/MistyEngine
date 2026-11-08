//! Mesh and shadow-caster pipelines per material surface (rendering mode, unlit, culling),
//! built the first time a frame needs them.

use std::collections::HashMap;

use super::SHADOW_FORMAT;
use super::pipelines::{self, DEPTH_SRC, Depth, MESH_SRC, Pipelines, Spec};
use super::surface::{PipelineKey, ShadowKey};

#[derive(Default)]
pub struct Variants {
    mesh: HashMap<PipelineKey, wgpu::RenderPipeline>,
    shadow: HashMap<ShadowKey, wgpu::RenderPipeline>,
}

/// The main-pass shader of a variant: the Standard shader, or its copy with the variant's
/// optional features compiled in.
pub(super) fn mesh_source(key: PipelineKey) -> &'static str {
    match key.features {
        0 => MESH_SRC,
        f => super::material_v2::lit_source(f),
    }
}

impl Pipelines {
    /// Group 2 layout of a main-pass variant (feature variants have the bigger one).
    pub(super) fn group2_layout(&self, key: PipelineKey) -> &wgpu::BindGroupLayout {
        match key.features {
            0 => &self.material_layout,
            _ => &self.lit_layout,
        }
    }

    /// Build the main-pass pipeline for `key` if it doesn't exist yet.
    pub fn ensure_mesh(&mut self, device: &wgpu::Device, key: PipelineKey) {
        if self.variants.mesh.contains_key(&key) {
            return;
        }
        let buffers = pipelines::mesh_buffers();
        let layouts = [
            &self.view_layout,
            &self.mesh_layout,
            self.group2_layout(key),
        ];
        let name = if key.features == 0 {
            "mesh"
        } else {
            "mesh lit"
        };
        let pipeline = pipelines::build(
            device,
            Spec {
                vs: "vs_main",
                fs: Some(key.entry()),
                buffers: &buffers,
                cull: key.cull(),
                depth: Depth::main(key.depth_write(), wgpu::CompareFunction::Less),
                blend: key.blend(),
                // Emission is blended like the colour, so glass dims the glow behind it.
                glow: Some(true),
                // Opaque and cutout surfaces write their motion; blended ones leave the
                // motion of what is behind them (they don't write depth either).
                velocity: key.depth_write(),
                ..Spec::new(name, mesh_source(key), &layouts)
            },
        );
        self.variants.mesh.insert(key, pipeline);
    }

    /// Build the shadow-caster pipeline for `key` if it doesn't exist yet.
    pub fn ensure_shadow(&mut self, device: &wgpu::Device, key: ShadowKey) {
        if self.variants.shadow.contains_key(&key) {
            return;
        }
        let buffers = pipelines::mesh_buffers();
        // Cutout casters sample the albedo alpha (group 2).
        let all = [
            &self.shadow_view_layout,
            &self.draws_layout,
            &self.material_layout,
        ];
        let layouts = if key.cutout { &all[..] } else { &all[..2] };
        let pipeline = pipelines::build(
            device,
            Spec {
                vs: if key.cutout { "vs_uv" } else { "vs_main" },
                fs: key.cutout.then_some("fs_shadow_cutout"),
                buffers: &buffers,
                // Light-facing faces, so shadows start right at contact points (no gap).
                // Acne is handled by a small slope bias plus a texel-sized normal offset in the shader.
                cull: (!key.double_sided).then_some(wgpu::Face::Back),
                depth: Some(Depth {
                    format: SHADOW_FORMAT,
                    write: true,
                    compare: wgpu::CompareFunction::LessEqual,
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.0,
                        clamp: 0.0,
                    },
                }),
                color: None,
                samples: 1,
                glow: None,
                ..Spec::new("shadow", DEPTH_SRC, layouts)
            },
        );
        self.variants.shadow.insert(key, pipeline);
    }

    pub fn mesh_variant(&self, key: PipelineKey) -> Option<&wgpu::RenderPipeline> {
        self.variants.mesh.get(&key)
    }

    pub fn shadow_variant(&self, key: ShadowKey) -> Option<&wgpu::RenderPipeline> {
        self.variants.shadow.get(&key)
    }
}
