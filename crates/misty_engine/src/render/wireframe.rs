//! The Scene view's Wireframe and Shaded Wireframe draw modes on the GPU.
//!
//! Every part of the scene draws its triangles a second time as edges, with its own vertex
//! stage and the culling and LOD the shaded frame picked: ordinary meshes (each visible draw),
//! terrain chunks (their LOD index ranges), trees (bark, leaf cards, impostor quads), the
//! grass and flowers (a sparse share near the camera, shaders/grass_wire.wgsl) and virtual
//! geometry clusters (the clusters this frame's culling kept, both phases). The edges come
//! from the rasteriser where the device has `POLYGON_MODE_LINE` (native Vulkan, DX12, Metal);
//! else from filled triangles that keep the pixels next to an edge (`SHADER_BARYCENTRICS`);
//! else the editor falls back to its CPU line lists (gizmos.rs, with its edge budget).
//!
//! - **Shaded Wireframe**: the frame renders as usual, then the edges are drawn at the end of
//!   the main pass (before the gizmo lines), depth tested against the scene and pulled a
//!   little towards the camera, so only the visible ones show.
//! - **Wireframe**: the geometry still renders (unlit, no shadows, no water, particles,
//!   transparents or post-processing: its depth hides the edges behind it), then a flat
//!   background is painted over its colour before the grid and the edges. Hidden-line removal
//!   by the real surfaces (alpha-tested leaves and blades included) keeps a dense scene
//!   readable, which see-through edges of a meadow would not be.
//!
//! The edges blend into the HDR image in the main pass like the gizmo lines. Their opacity
//! rides in the view uniform's unused `view_tan.w` and their colour is the pass's blend
//! constant: the shared shader prelude (common.wgsl) stays byte for byte as it was, so no
//! other shader changes (a new prelude makes every pipeline compile afresh: MeadowDemo's
//! first frame went from 0.3 to 2.5 s where the driver's cache had the old ones).

use super::pipelines::{self, Depth, Spec};
use super::{DEPTH_FORMAT, HDR_FORMAT, Renderer, grass_cull};

/// How this device draws edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WirePath {
    /// Polygon mode Line.
    Line,
    /// Filled triangles that keep only the pixels within half a pixel of an edge.
    Barycentric,
}

impl WirePath {
    /// The path `device` supports (`None` = neither feature: CPU line lists).
    /// `MISTY_WIREFRAME=cpu` or `=barycentric` picks a fallback (for checking them).
    pub fn of(device: &wgpu::Device) -> Option<Self> {
        let features = device.features();
        let line = features
            .contains(wgpu::Features::POLYGON_MODE_LINE)
            .then_some(Self::Line);
        let bary = features
            .contains(wgpu::Features::SHADER_BARYCENTRICS)
            .then_some(Self::Barycentric);
        match std::env::var("MISTY_WIREFRAME").as_deref() {
            Ok("cpu") => None,
            Ok("barycentric") => bary,
            _ => line.or(bary),
        }
    }
}

/// Device features the wireframe draw modes use when the adapter has them: polygon mode Line,
/// or barycentrics where it is missing.
pub fn wireframe_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    let has = adapter.features();
    if has.contains(wgpu::Features::POLYGON_MODE_LINE) {
        wgpu::Features::POLYGON_MODE_LINE
    } else {
        has & wgpu::Features::SHADER_BARYCENTRICS
    }
}

/// Edges over (or instead of) one render (see [`Renderer::set_wireframe`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wireframe {
    /// sRGB colour and opacity of the edges.
    pub color: [f32; 4],
    /// Over the shaded scene (Shaded Wireframe); false = on the frame's flat background
    /// (Wireframe).
    pub shaded: bool,
}

const WIRE_SRC: &str = include_str!("shaders/wireframe.wgsl");
const BARY_SRC: &str = include_str!("shaders/wireframe_bary.wgsl");
const GRASS_WIRE_SRC: &str = include_str!("shaders/grass_wire.wgsl");

/// Edges a little in front of the surface they lie on (towards 0: the main pass's depth is
/// standard), so the depth test against the shaded frame keeps them: a pixel's worth of the
/// triangle's slope (an edge pixel's centre is up to half a pixel off the triangle) and a few
/// steps of precision.
const BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: -8,
    slope_scale: -1.5,
    clamp: 0.0,
};

/// The fragment stage writes the opacity `a` in every channel: colour = blend constant × a +
/// scene × (1 − a).
const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Constant,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::OVER,
};

/// `base` (a pipeline's shader source) with the wireframe fragment stages.
pub(super) fn source(base: &str, path: WirePath) -> String {
    match path {
        WirePath::Line => format!("{base}\n{WIRE_SRC}"),
        WirePath::Barycentric => format!("{base}\n{WIRE_SRC}\n{BARY_SRC}"),
    }
}

/// The wireframe pipeline of a shaded one: its vertex stage, buffers, layouts and culling;
/// edges blended into the main pass's colour, depth tested (not written) against the scene.
/// `source` = [`source`] of the shaded pipeline's source.
pub(super) fn spec<'a>(
    shaded: Spec<'a>,
    name: &'a str,
    source: &'a str,
    path: WirePath,
) -> Spec<'a> {
    Spec {
        name,
        source,
        fs: Some(match path {
            WirePath::Line => "fs_wire",
            WirePath::Barycentric => "fs_wire_bary",
        }),
        polygon: match path {
            WirePath::Line => wgpu::PolygonMode::Line,
            WirePath::Barycentric => wgpu::PolygonMode::Fill,
        },
        depth: Some(Depth {
            format: DEPTH_FORMAT,
            write: false,
            compare: wgpu::CompareFunction::LessEqual,
            bias: BIAS,
        }),
        color: Some(HDR_FORMAT),
        blend: Some(BLEND),
        samples: super::sample_count(),
        glow: Some(false),
        alpha_to_coverage: false,
        velocity: false,
        color_writes: wgpu::ColorWrites::ALL,
        ..shaded
    }
}

/// The renderer's own wireframe pipelines (trees and virtual geometry keep theirs).
pub(super) struct WirePipelines {
    path: WirePath,
    samples: u32,
    /// Ordinary meshes (the depth shader's vertex stage, both faces).
    mesh: wgpu::RenderPipeline,
    terrain: wgpu::RenderPipeline,
    grass: wgpu::RenderPipeline,
    /// The Wireframe mode's flat background over the scene's colour.
    background: wgpu::RenderPipeline,
}

impl WirePipelines {
    fn new(device: &wgpu::Device, p: &pipelines::Pipelines, path: WirePath) -> Self {
        let mesh_buffers = pipelines::mesh_buffers();
        let mesh_src = source(pipelines::DEPTH_SRC, path);
        let mesh = pipelines::build(
            device,
            spec(
                Spec {
                    vs: "vs_main",
                    buffers: &mesh_buffers,
                    ..Spec::new("mesh", "", &[&p.view_layout, &p.draws_layout])
                },
                "mesh wireframe",
                &mesh_src,
                path,
            ),
        );
        let terrain_src = source(pipelines::TERRAIN_SRC, path);
        let terrain_layouts = [
            &p.view_layout,
            &p.mesh_layout,
            &p.empty_layout,
            &p.terrain_layout,
        ];
        let terrain = pipelines::build(
            device,
            spec(
                Spec {
                    vs: "vs_terrain",
                    cull: Some(wgpu::Face::Back),
                    ..Spec::new("terrain", "", &terrain_layouts)
                },
                "terrain wireframe",
                &terrain_src,
                path,
            ),
        );
        let grass_src = source(&format!("{}\n{GRASS_WIRE_SRC}", pipelines::GRASS_SRC), path);
        let grass_layouts = [
            &p.view_layout,
            &p.mesh_layout,
            &p.grass_layout,
            &p.grass_blades_layout,
        ];
        let grass = pipelines::build(
            device,
            spec(
                Spec {
                    vs: "vs_grass_wire",
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Spec::new("grass", "", &grass_layouts)
                },
                "grass wireframe",
                &grass_src,
                path,
            ),
        );
        let background = pipelines::build(
            device,
            Spec {
                fs: Some("fs_wire_background"),
                glow: Some(true),
                ..Spec::new("wireframe background", WIRE_SRC, &[&p.view_layout])
            },
        );
        Self {
            path,
            samples: super::sample_count(),
            mesh,
            terrain,
            grass,
            background,
        }
    }
}

impl Renderer {
    /// Draw the next render's triangle edges on the GPU (the Scene view's Wireframe and
    /// Shaded Wireframe draw modes; that render only, like the virtual geometry debug view).
    /// False = this device can't (no polygon mode Line, no barycentrics): draw CPU lines.
    pub fn set_wireframe(&mut self, wire: Wireframe) -> bool {
        self.wire_next = self.wire_path.is_some().then_some(wire);
        self.wire_next.is_some()
    }

    /// The wireframe draw modes draw on the GPU on this device ([`Renderer::set_wireframe`]).
    pub fn gpu_wireframe(&self) -> bool {
        self.wire_path.is_some()
    }

    /// Draw edges with `path` (tests: the barycentric fallback on a device that has both).
    #[cfg(test)]
    pub(super) fn force_wire_path(&mut self, path: Option<WirePath>) {
        self.wire_path = path;
    }

    /// Start a render: this render's wireframe request (`Some` and not shaded = the Wireframe
    /// mode, see [`Renderer::wire_only`]).
    pub(super) fn begin_wireframe(&mut self) {
        self.wire = self.wire_next.take().filter(|_| self.wire_path.is_some());
        self.wire_only = self.wire.is_some_and(|w| !w.shaded);
    }

    /// The edges' linear colour (the blend constant of their draws).
    fn wire_color(w: &Wireframe) -> wgpu::Color {
        let [r, g, b] = super::srgb_to_linear([w.color[0], w.color[1], w.color[2]]);
        let c = |v: f32| v as f64;
        wgpu::Color {
            r: c(r),
            g: c(g),
            b: c(b),
            a: c(w.color[3]),
        }
    }

    /// Build this render's wireframe pipelines (once per path and MSAA samples).
    pub(super) fn prepare_wireframe(&mut self, device: &wgpu::Device) {
        let (Some(_), Some(path)) = (self.wire, self.wire_path) else {
            return;
        };
        let samples = super::sample_count();
        if !(self.wire_pipelines.as_ref()).is_some_and(|w| w.path == path && w.samples == samples) {
            crate::profile_scope!("Render.WireframePipelines");
            self.wire_pipelines = Some(WirePipelines::new(device, &self.pipelines, path));
        }
        self.trees.ensure_wire(device, &self.pipelines, path);
        if self.vgeo.active() {
            self.vgeo.ensure_wire(device, &self.pipelines, path);
        }
    }

    /// The Wireframe mode's flat background over the scene's colour (start of the grid's
    /// part of the main pass).
    pub(super) fn draw_wire_background(&self, pass: &mut wgpu::RenderPass) {
        if let (true, Some(w)) = (self.wire_only, &self.wire_pipelines) {
            pass.set_pipeline(&w.background);
            pass.draw(0..3, 0..1);
            self.mark_pass(pass, "Main: wireframe background");
        }
    }

    /// This render's edges (main pass, after transparents and before the gizmo lines; group 0
    /// bound). Not counted in the frame's draw stats.
    pub(super) fn draw_wireframe(
        &self,
        pass: &mut wgpu::RenderPass,
        frame: &super::Frame,
        lists: &super::frame_lists::FrameLists,
    ) {
        let (Some(wire), Some(w)) = (self.wire, &self.wire_pipelines) else {
            return;
        };
        pass.set_blend_constant(Self::wire_color(&wire));
        let mut stats = super::RenderStats::default();
        // Ordinary meshes: each draw in view (and shown by its LOD); virtual meshes drawn as
        // clusters have theirs below.
        let clusters = self.vgeo.active();
        pass.set_pipeline(&w.mesh);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        for (i, d) in frame.meshes.iter().enumerate() {
            let as_clusters =
                clusters && (lists.prep.virtual_mesh.get(i)).is_some_and(|v| v.is_some());
            if lists.visible.get(i).copied().unwrap_or(false) && !as_clusters {
                self.draw_mesh(pass, i, d);
            }
        }
        if !frame.terrains.is_empty() {
            pass.set_bind_group(1, self.lit_group(), &[]);
            // Terrain chunks at their camera LOD.
            pass.set_pipeline(&w.terrain);
            pass.set_bind_group(2, &self.empty_group, &[]);
            let first = frame.meshes.len();
            let visible = |i: usize| lists.terrains.get(i).copied().unwrap_or(false);
            (self.terrains).draw(pass, frame.terrains, first, true, visible, &mut stats);
            // Trees at their LOD (meshes, impostor quads far away).
            self.trees.draw_wire(pass, &mut stats);
            // The camera's standing blades (the grass depth lane), thinned by the vertex stage.
            if self.terrains.has_grass() {
                pass.set_pipeline(&w.grass);
                let lane = grass_cull::DEPTH;
                (self.terrains).draw_grass(pass, frame.terrains, lane, &mut stats);
            }
        }
        if clusters {
            pass.set_bind_group(1, self.lit_group(), &[]);
            let textures = &self.textures;
            let material = |draw: usize| textures.get_set(&frame.meshes[draw].material.textures());
            self.vgeo.draw_wire(pass, &material);
        }
        self.mark_pass(pass, "Main: wireframe");
    }
}
