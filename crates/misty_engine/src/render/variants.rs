//! Mesh and shadow-caster pipelines per material surface (rendering mode, unlit, culling),
//! built the first time a frame needs them: in place, or on the shader queue while the
//! renderer's background loads are on (the editor's views, see
//! [`super::Renderer::set_background_loads`]). Until a queued pipeline is there its draws are
//! left out, or drawn with the plain opaque variant ([`Pipelines::mesh_variant`]).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::SHADOW_FORMAT;
use super::pipelines::{self, DEPTH_SRC, Depth, MESH_SRC, Pipelines, Spec};
use super::surface::{PipelineKey, RenderingMode, ShadowKey};

/// A variant of either kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum VariantKey {
    Mesh(PipelineKey),
    Shadow(ShadowKey),
}

/// Variants compiled on the shader queue, waiting to be picked up.
type Built = Arc<Mutex<Vec<(VariantKey, wgpu::RenderPipeline)>>>;

#[derive(Default)]
pub struct Variants {
    mesh: HashMap<PipelineKey, wgpu::RenderPipeline>,
    shadow: HashMap<ShadowKey, wgpu::RenderPipeline>,
    /// Build missing variants on the shader queue instead of in place.
    pub(super) background: bool,
    /// Variants queued and not picked up yet.
    pending: HashSet<VariantKey>,
    built: Built,
}

/// The plain Standard variant: what an Opaque surface draws with while its own variant
/// compiles in the background.
pub(super) const PLAIN: PipelineKey = PipelineKey {
    unlit: false,
    mode: RenderingMode::Opaque,
    double_sided: false,
    features: 0,
};

/// Surfaces most scenes have, compiled ahead in the background by
/// [`Pipelines::warm_common_variants`].
const COMMON_MESH: [PipelineKey; 4] = [
    PLAIN,
    PipelineKey {
        double_sided: true,
        ..PLAIN
    },
    PipelineKey {
        mode: RenderingMode::Cutout,
        ..PLAIN
    },
    PipelineKey {
        mode: RenderingMode::Cutout,
        double_sided: true,
        ..PLAIN
    },
];
const COMMON_SHADOW: [ShadowKey; 3] = [
    ShadowKey {
        cutout: false,
        double_sided: false,
    },
    ShadowKey {
        cutout: true,
        double_sided: false,
    },
    ShadowKey {
        cutout: true,
        double_sided: true,
    },
];

/// The main-pass shader of a variant: the Standard shader, or its copy with the variant's
/// optional features compiled in.
pub(super) fn mesh_source(key: PipelineKey) -> &'static str {
    match key.features {
        0 => MESH_SRC,
        f => super::material_v2::lit_source(f),
    }
}

/// The main-pass pipeline of `key` (`layouts`: view, mesh, the variant's group 2).
fn mesh_pipeline(
    device: &wgpu::Device,
    layouts: &[&wgpu::BindGroupLayout; 3],
    key: PipelineKey,
    samples: u32,
) -> wgpu::RenderPipeline {
    let buffers = pipelines::mesh_buffers();
    let name = if key.features == 0 {
        "mesh"
    } else {
        "mesh lit"
    };
    pipelines::build(
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
            samples,
            ..Spec::new(name, mesh_source(key), layouts)
        },
    )
}

/// The shadow-caster pipeline of `key` (`all`: shadow view, draws, material layouts).
fn shadow_pipeline(
    device: &wgpu::Device,
    all: &[&wgpu::BindGroupLayout; 3],
    key: ShadowKey,
) -> wgpu::RenderPipeline {
    let buffers = pipelines::mesh_buffers();
    // Cutout casters sample the albedo alpha (group 2).
    let layouts = if key.cutout { &all[..] } else { &all[..2] };
    pipelines::build(
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
    )
}

impl Pipelines {
    /// Group 2 layout of a main-pass variant (feature variants have the bigger one).
    pub(super) fn group2_layout(&self, key: PipelineKey) -> &wgpu::BindGroupLayout {
        match key.features {
            0 => &self.material_layout,
            _ => &self.lit_layout,
        }
    }

    /// Build the main-pass pipeline for `key` if it doesn't exist yet (queue it while the
    /// background loads are on).
    pub fn ensure_mesh(&mut self, device: &wgpu::Device, key: PipelineKey) {
        if self.variants.mesh.contains_key(&key) {
            return;
        }
        let samples = super::sample_count();
        if self.variants.background {
            let layouts = [
                self.view_layout.clone(),
                self.mesh_layout.clone(),
                self.group2_layout(key).clone(),
            ];
            self.variants
                .queue(device, VariantKey::Mesh(key), move |device| {
                    let [a, b, c] = &layouts;
                    mesh_pipeline(device, &[a, b, c], key, samples)
                });
            return;
        }
        let layouts = [
            &self.view_layout,
            &self.mesh_layout,
            self.group2_layout(key),
        ];
        let pipeline = mesh_pipeline(device, &layouts, key, samples);
        self.variants.mesh.insert(key, pipeline);
    }

    /// Build the shadow-caster pipeline for `key` if it doesn't exist yet (queue it while the
    /// background loads are on).
    pub fn ensure_shadow(&mut self, device: &wgpu::Device, key: ShadowKey) {
        if self.variants.shadow.contains_key(&key) {
            return;
        }
        if self.variants.background {
            let layouts = [
                self.shadow_view_layout.clone(),
                self.draws_layout.clone(),
                self.material_layout.clone(),
            ];
            self.variants
                .queue(device, VariantKey::Shadow(key), move |device| {
                    let [a, b, c] = &layouts;
                    shadow_pipeline(device, &[a, b, c], key)
                });
            return;
        }
        let all = [
            &self.shadow_view_layout,
            &self.draws_layout,
            &self.material_layout,
        ];
        let pipeline = shadow_pipeline(device, &all, key);
        self.variants.shadow.insert(key, pipeline);
    }

    /// Queue the variants most scenes use (plain, double-sided and cutout surfaces and their
    /// shadow casters), so a scene's first frames rarely wait for one. Nothing unless the
    /// background loads are on.
    pub fn warm_common_variants(&mut self, device: &wgpu::Device) {
        if !self.variants.background {
            return;
        }
        for key in COMMON_MESH {
            self.ensure_mesh(device, key);
        }
        for key in COMMON_SHADOW {
            self.ensure_shadow(device, key);
        }
    }

    /// Take the variants the shader queue finished. Returns how many.
    pub fn take_built_variants(&mut self) -> usize {
        let built = std::mem::take(
            &mut *self
                .variants
                .built
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        let n = built.len();
        for (key, pipeline) in built {
            self.variants.pending.remove(&key);
            match key {
                VariantKey::Mesh(k) => {
                    self.variants.mesh.entry(k).or_insert(pipeline);
                }
                VariantKey::Shadow(k) => {
                    self.variants.shadow.entry(k).or_insert(pipeline);
                }
            }
        }
        n
    }

    /// Variants still compiling in the background.
    pub fn variants_pending(&self) -> usize {
        self.variants.pending.len()
    }

    /// The main-pass pipeline of `key`. While an Opaque surface's own variant (unlit,
    /// double-sided) compiles in the background, the plain Standard one stands in for it;
    /// other surfaces are left out until theirs is there.
    pub fn mesh_variant(&self, key: PipelineKey) -> Option<&wgpu::RenderPipeline> {
        self.variants.mesh.get(&key).or_else(|| {
            let stand_in = key.mode == RenderingMode::Opaque
                && key.features == 0
                && self.variants.pending.contains(&VariantKey::Mesh(key));
            stand_in.then(|| self.variants.mesh.get(&PLAIN)).flatten()
        })
    }

    pub fn shadow_variant(&self, key: ShadowKey) -> Option<&wgpu::RenderPipeline> {
        self.variants.shadow.get(&key)
    }
}

impl Variants {
    /// Build a variant on the shader queue (once: nothing when it is already queued).
    fn queue(
        &mut self,
        device: &wgpu::Device,
        key: VariantKey,
        build: impl FnOnce(&wgpu::Device) -> wgpu::RenderPipeline + Send + 'static,
    ) {
        if !self.pending.insert(key) {
            return;
        }
        let device = device.clone();
        let built = self.built.clone();
        crate::load_queue::shaders().push(format!("{key:?}"), move |ticket| {
            let pipeline = build(&device);
            ticket.publish(|| {
                built
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((key, pipeline))
            });
            Ok(())
        });
    }
}
