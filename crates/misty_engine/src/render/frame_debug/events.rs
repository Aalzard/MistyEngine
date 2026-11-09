//! Building Frame Debugger events for mesh draws: names, counts, batch reasons, pipeline
//! state, bound textures and material values. Only runs while a capture is installed.

use std::collections::HashMap;

use super::{EventKind, FrameEvent, Recorder};
use crate::render::batch::Batch;
use crate::render::surface::{PipelineKey, ShadowKey};
use crate::render::{Frame, GpuMesh, MaterialParams, MeshDraw, RenderingMode};
use crate::scene::MeshRef;
use crate::shader_asset::PropertyValue;

/// Display name of a mesh reference ("Cube", "Assets/Models/Tree.glb#0").
pub(in crate::render) fn mesh_name(m: &MeshRef) -> String {
    match m {
        MeshRef::Primitive(p) => p.name().to_owned(),
        MeshRef::Asset(k) if k.starts_with("static-batch-") => {
            format!(
                "Combined Mesh (Static) {}",
                k.rsplit('/').next().unwrap_or("")
            )
        }
        MeshRef::Asset(k) => k.clone(),
    }
}

/// Which list a batch is from (decides the wording of the batch reason).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::render) enum ListKind {
    Opaque,
    Transparent,
    Shadow,
    Prepass,
}

/// How many batches of a list share each mesh and each material state (batch reasons).
pub(in crate::render) struct BatchContext {
    meshes: HashMap<String, u32>,
    materials: HashMap<String, u32>,
    instancing: bool,
    kind: ListKind,
}

fn material_key(m: &MaterialParams) -> String {
    match &m.custom {
        Some(c) => format!("{}|{:?}", c.shader, m.textures()),
        None => format!("{:?}|{:?}", m.pipeline_key(), m.textures()),
    }
}

impl BatchContext {
    pub fn new(frame: &Frame, batches: &[Batch], instancing: bool, kind: ListKind) -> Self {
        let mut meshes = HashMap::new();
        let mut materials = HashMap::new();
        for b in batches {
            let d = &frame.meshes[b.draw];
            *meshes.entry(mesh_name(&d.mesh)).or_insert(0) += 1;
            *materials.entry(material_key(&d.material)).or_insert(0) += 1;
        }
        Self {
            meshes,
            materials,
            instancing,
            kind,
        }
    }

    /// Why `b` is one instanced call, a static batch, or a call of its own.
    pub fn reason(&self, d: &MeshDraw, b: &Batch) -> String {
        if b.count > 1 {
            return format!(
                "GPU instanced: {} objects share mesh, shader variant and textures",
                b.count
            );
        }
        let mesh = mesh_name(&d.mesh);
        if matches!(&d.mesh, MeshRef::Asset(k) if k.starts_with("static-batch-")) {
            return "Static batch: Static objects with this material were combined into one mesh"
                .to_owned();
        }
        if !self.instancing {
            return "Not batched: instancing is turned off".to_owned();
        }
        let shares_mesh = self.meshes.get(&mesh).copied().unwrap_or(0) > 1;
        let shares_material = self
            .materials
            .get(&material_key(&d.material))
            .copied()
            .unwrap_or(0)
            > 1;
        match (self.kind, shares_mesh, shares_material) {
            (ListKind::Transparent, true, _) | (ListKind::Transparent, _, true) => {
                "Not batched: transparent draws keep back-to-front order (neighbours differ)"
                    .to_owned()
            }
            (_, true, _) => {
                "Not batched: different shader variant or textures than other draws of this mesh"
                    .to_owned()
            }
            (_, false, true) => {
                "Not batched: different mesh than other draws with this material".to_owned()
            }
            _ => "Not batched: no other object uses this mesh and material".to_owned(),
        }
    }
}

fn rgba(c: [f32; 4]) -> String {
    format!("({:.2}, {:.2}, {:.2}, {:.2})", c[0], c[1], c[2], c[3])
}

/// Material values shown in the details pane.
pub(in crate::render) fn properties(m: &MaterialParams) -> Vec<(String, String)> {
    let mut out = vec![
        ("Color".to_owned(), rgba(m.color)),
        ("Rendering Mode".to_owned(), m.mode.label().to_owned()),
    ];
    if let Some(c) = &m.custom {
        out.push(("Render Queue".to_owned(), c.queue.to_string()));
        for (k, v) in c.values.iter() {
            let value = match v {
                PropertyValue::Float(f) => format!("{f:.3}"),
                PropertyValue::Vector(a) => rgba(*a),
                PropertyValue::Texture(t) => t.clone().unwrap_or_else(|| "None".to_owned()),
            };
            out.push((k.clone(), value));
        }
        return out;
    }
    if !m.unlit {
        out.push(("Metallic".to_owned(), format!("{:.2}", m.metallic)));
        out.push(("Smoothness".to_owned(), format!("{:.2}", m.smoothness)));
    }
    if m.mode == RenderingMode::Cutout {
        out.push(("Alpha Cutoff".to_owned(), format!("{:.2}", m.alpha_cutoff)));
    }
    if m.emission_intensity > 0.0 {
        let e = m.emission;
        out.push((
            "Emission".to_owned(),
            format!(
                "({:.2}, {:.2}, {:.2}) × {:.2}",
                e[0], e[1], e[2], m.emission_intensity
            ),
        ));
    }
    if m.normal_map.is_some() {
        out.push(("Normal Scale".to_owned(), format!("{:.2}", m.normal_scale)));
    }
    out.push((
        "Tiling / Offset".to_owned(),
        format!(
            "({:.2}, {:.2}) / ({:.2}, {:.2})",
            m.tiling[0], m.tiling[1], m.offset[0], m.offset[1]
        ),
    ));
    if m.double_sided {
        out.push(("Double Sided".to_owned(), "On".to_owned()));
    }
    if let Some(l) = &m.lightmap {
        out.push(("Lightmap Index".to_owned(), l.index.to_string()));
    }
    if m.light_probes {
        out.push(("Light Probes".to_owned(), "Blend Probes".to_owned()));
    }
    out
}

/// Bound material textures (slot, path).
pub(in crate::render) fn textures(m: &MaterialParams) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |slot: &str, p: &Option<String>| {
        if let Some(p) = p {
            out.push((slot.to_owned(), p.clone()));
        }
    };
    add("Albedo", &m.albedo_map);
    add("Normal Map", &m.normal_map);
    add("Metallic", &m.metallic_map);
    add("Occlusion", &m.occlusion_map);
    add("Emission", &m.emission_map);
    if let Some(e) = m.extras.as_ref().filter(|_| m.features() != 0) {
        add("Detail Albedo", &e.detail_albedo);
        add("Detail Normal", &e.detail_normal);
        add("Detail Mask", &e.detail_mask);
        add("Height", &e.height_map);
        add("Coat Normal", &e.coat_normal_map);
        add("Thickness", &e.thickness_map);
    }
    if let Some(c) = &m.custom {
        for (k, v) in c.values.iter() {
            if let PropertyValue::Texture(Some(t)) = v {
                out.push((k.clone(), t.clone()));
            }
        }
    }
    out
}

fn blend_label(blend: Option<wgpu::BlendState>) -> String {
    match blend {
        None => "Off".to_owned(),
        Some(b) if b == wgpu::BlendState::ALPHA_BLENDING => "SrcAlpha OneMinusSrcAlpha".to_owned(),
        Some(b) if b == wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING => {
            "One OneMinusSrcAlpha (premultiplied)".to_owned()
        }
        Some(_) => "Additive".to_owned(),
    }
}

/// Main-pass shader description of a built-in variant.
pub(in crate::render) fn shader_label(key: PipelineKey) -> String {
    let family = if key.unlit { "Unlit" } else { "Standard" };
    format!(
        "{family} ({}) · mesh.wgsl vs_main / {}",
        key.mode.label(),
        key.entry()
    )
}

pub(in crate::render) fn shadow_label(key: ShadowKey) -> String {
    match key.cutout {
        true => "Shadow Caster (alpha clip) · depth only".to_owned(),
        false => "Shadow Caster · depth only".to_owned(),
    }
}

/// What a mesh draw binds: view, lighting (or per-draw values for depth-only passes), the
/// material's textures, the mesh buffers and the instances.
pub(in crate::render) fn mesh_bindings(
    d: &MeshDraw,
    mesh: &str,
    m: &GpuMesh,
    instances: u32,
    depth_only: bool,
) -> Vec<(String, String)> {
    let mut out = vec![(
        "Group 0 · View".to_owned(),
        match depth_only {
            true => "view uniform (camera or light matrix)".to_owned(),
            false => "view uniform (camera, jitter, fog, sky), sky cubemaps".to_owned(),
        },
    )];
    out.push(match depth_only {
        true => (
            "Group 1 · Draws".to_owned(),
            "per-draw values (draws[]), draw slots".to_owned(),
        ),
        false => (
            "Group 1 · Lighting".to_owned(),
            "lights, per-draw values (draws[]), draw slots, shadow maps (cascades, local, \
             virtual), lightmaps, screen-space AO / GI texels"
                .to_owned(),
        ),
    });
    let material = match &d.material.custom {
        Some(c) => format!("{} resources", c.shader),
        None => {
            let t = textures(&d.material);
            match t.is_empty() {
                true => "default textures (white albedo, flat normal)".to_owned(),
                false => t
                    .iter()
                    .map(|(slot, path)| {
                        format!("{slot}: {}", path.rsplit('/').next().unwrap_or(path))
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        }
    };
    if !depth_only || d.material.mode == RenderingMode::Cutout {
        out.push(("Group 2 · Material".to_owned(), material));
    }
    out.push((
        "Vertex buffer".to_owned(),
        format!(
            "{mesh}: position, normal, uv, tangent · {} vertices",
            m.vertex_count
        ),
    ));
    out.push((
        "Index buffer".to_owned(),
        format!(
            "{} indices (u32) from {} · base vertex {}",
            m.count, m.first_index, m.base_vertex
        ),
    ));
    out.push((
        "Instances".to_owned(),
        format!("{instances} (per-draw values through the draw slots)"),
    ));
    out
}

/// A mesh batch's event: object / mesh / material names, counts, reason and state.
#[allow(clippy::too_many_arguments)]
pub(in crate::render) fn mesh_event(
    rec: &Recorder,
    frame: &Frame,
    b: &Batch,
    m: &GpuMesh,
    ctx: Option<&BatchContext>,
    shader: String,
    depth: &str,
    blend: Option<wgpu::BlendState>,
    double_sided: bool,
) -> FrameEvent {
    let d = &frame.meshes[b.draw];
    let label = rec.label(b.draw);
    let mesh = mesh_name(&d.mesh);
    let object = label
        .map(|l| l.object.clone())
        .filter(|o| !o.is_empty())
        .unwrap_or_else(|| mesh.clone());
    let name = match b.count {
        1 => format!("Draw Mesh {object}"),
        n => format!("Draw Mesh (instanced ×{n}) {object}"),
    };
    let depth_only = shader.contains("epth") || shader.starts_with("Shadow");
    let bindings = mesh_bindings(d, &mesh, m, b.count, depth_only);
    FrameEvent {
        bindings,
        object: Some(object),
        mesh: Some(mesh),
        material: label.and_then(|l| l.material.clone()),
        submesh: label.map(|l| l.submesh),
        vertices: m.vertex_count,
        indices: m.count,
        instances: b.count,
        batch: ctx.map(|c| c.reason(d, b)),
        shader,
        depth: depth.to_owned(),
        blend: blend_label(blend),
        cull: if double_sided { "Off" } else { "Back" }.to_owned(),
        textures: textures(&d.material),
        properties: properties(&d.material),
        ..FrameEvent::new(EventKind::Draw, name)
    }
}

/// Main-pass event of a batch (built-in or `.shader` material).
pub(in crate::render) fn main_event(
    rec: &Recorder,
    frame: &Frame,
    b: &Batch,
    m: &GpuMesh,
    ctx: Option<&BatchContext>,
    key: PipelineKey,
    prepass: bool,
) -> FrameEvent {
    let d = &frame.meshes[b.draw];
    let shader = match &d.material.custom {
        Some(c) => format!("{} (custom shader, queue {})", c.shader, c.queue),
        None => shader_label(key),
    };
    let depth = match (key.depth_write(), prepass) {
        (true, true) => "Equal, write off (after depth prepass)",
        (true, false) => "LessEqual, write on",
        (false, _) => "LessEqual, write off",
    };
    mesh_event(
        rec,
        frame,
        b,
        m,
        ctx,
        shader,
        depth,
        key.blend(),
        key.double_sided,
    )
}

/// A fullscreen or single-call event (sky, grid, post-processing steps).
pub(in crate::render) fn post_event(name: String, shader: &str) -> FrameEvent {
    FrameEvent {
        shader: shader.to_owned(),
        bindings: vec![
            (
                "Group 0".to_owned(),
                "input image(s) of the step and their sampler".to_owned(),
            ),
            (
                "Vertex buffer".to_owned(),
                "none (fullscreen triangle from the vertex index)".to_owned(),
            ),
        ],
        vertices: 3,
        blend: "Off".to_owned(),
        depth: "Off".to_owned(),
        cull: "Off".to_owned(),
        ..FrameEvent::new(EventKind::Fullscreen, name)
    }
}

/// One mesh draw outside the batched lists (selection mask).
pub(in crate::render) fn simple_draw(
    rec: &Recorder,
    i: usize,
    d: &MeshDraw,
    shader: &str,
) -> FrameEvent {
    let mesh = mesh_name(&d.mesh);
    let object = rec
        .label(i)
        .map(|l| l.object.clone())
        .unwrap_or_else(|| mesh.clone());
    FrameEvent {
        object: Some(object.clone()),
        mesh: Some(mesh),
        shader: shader.to_owned(),
        blend: "Off".to_owned(),
        depth: "Off".to_owned(),
        cull: "Back".to_owned(),
        ..FrameEvent::new(EventKind::Draw, format!("Draw Mesh {object}"))
    }
}

/// A particle system's camera-facing quads (one instanced call).
pub(in crate::render) fn particle_event(
    system: usize,
    particles: u32,
    additive: bool,
    texture: Option<&str>,
) -> FrameEvent {
    FrameEvent {
        object: Some(format!("Particle System {system}")),
        mesh: Some("Quad (billboard)".to_owned()),
        vertices: 4,
        indices: 6,
        instances: particles,
        batch: Some(format!(
            "GPU instanced: {particles} particles of one system in one call"
        )),
        shader: "particles.wgsl".to_owned(),
        blend: match additive {
            true => "One One (additive)",
            false => "SrcAlpha OneMinusSrcAlpha",
        }
        .to_owned(),
        depth: "LessEqual, write off".to_owned(),
        cull: "Off".to_owned(),
        textures: texture
            .map(|t| vec![("Particle Texture".to_owned(), t.to_owned())])
            .unwrap_or_default(),
        ..FrameEvent::new(
            EventKind::Draw,
            format!("Draw Particles ({particles}) System {system}"),
        )
    }
}

impl crate::render::Renderer {
    /// Record a draw event per terrain `visible(i)` keeps; returns which may be drawn.
    pub(in crate::render) fn terrain_events(
        &self,
        frame: &Frame,
        visible: impl Fn(usize) -> bool,
        what: &str,
    ) -> Vec<bool> {
        (0..frame.terrains.len())
            .map(|i| {
                visible(i)
                    && super::record(&self.capture, |_| {
                        let t = &frame.terrains[i];
                        FrameEvent {
                            object: Some(t.key.clone()),
                            mesh: Some(format!("Terrain chunks ({})", t.key)),
                            shader: "terrain.wgsl".to_owned(),
                            depth: "LessEqual, write on".to_owned(),
                            blend: "Off".to_owned(),
                            cull: "Back".to_owned(),
                            ..FrameEvent::new(EventKind::Draw, format!("{what} {}", t.key))
                        }
                    })
            })
            .collect()
    }
}

/// Virtual geometry's culling of one view (compute: instances, group decisions, clusters).
pub(in crate::render) fn vgeo_cull_event(view: usize) -> FrameEvent {
    let name = match view {
        0 => "Dispatch Misty Mesh Culling (camera, first phase)".to_owned(),
        1 => "Dispatch Misty Mesh Culling (camera, second phase, Hi-Z)".to_owned(),
        v => format!("Dispatch Misty Mesh Culling (shadow map {})", v - 2),
    };
    FrameEvent {
        pass: vec!["Misty Mesh".to_owned(), "Culling".to_owned()],
        shader: "vgeo_cull.wgsl cs_instances, cs_prepare, cs_groups, cs_clusters, cs_finish"
            .to_owned(),
        ..FrameEvent::new(EventKind::Compute, name)
    }
}

/// One batch draw of virtual geometry: every visible cluster of one material texture set in
/// one view, drawn indirectly with the count the GPU wrote.
pub(in crate::render) fn vgeo_event(
    rec: &Recorder,
    frame: &Frame,
    d: &crate::render::vgeo::VgeoDraw,
) -> FrameEvent {
    use crate::render::vgeo::VgeoPass;
    let draw = &frame.meshes[d.draw];
    let label = rec.label(d.draw);
    let material = label.and_then(|l| l.material.clone());
    let what = material
        .clone()
        .unwrap_or_else(|| format!("batch {}", d.batch));
    let cutout = if d.cutout { ", Cutout" } else { "" };
    let main = |leaf: &str| vec!["Misty Mesh".to_owned(), leaf.to_owned()];
    let (pass, kind, name, shader, depth) = match d.pass {
        VgeoPass::Lit => (
            main("Clusters"),
            EventKind::DrawIndirect,
            format!("Draw Misty Mesh Clusters ({what}{cutout})"),
            format!(
                "vgeo_mesh.wgsl vs_cluster + mesh.wgsl {}",
                if d.cutout { "fs_cutout" } else { "fs_main" }
            ),
            "Less, write on",
        ),
        VgeoPass::Raster => (
            main("Visibility Raster"),
            EventKind::DrawIndirect,
            format!("Draw Misty Mesh Ids ({what}{cutout})"),
            format!(
                "vgeo_vis.wgsl {}",
                if d.cutout {
                    "fs_cluster_vis_cutout"
                } else {
                    "fs_cluster_vis"
                }
            ),
            "Less, write on",
        ),
        VgeoPass::Resolve => (
            main("Resolve"),
            EventKind::Fullscreen,
            format!("Resolve Misty Mesh ({what})"),
            "vgeo_resolve.wgsl fs_cluster_resolve + mesh.wgsl shade".to_owned(),
            "Off (samples from the visibility ids)",
        ),
        VgeoPass::Shadow => (
            Vec::new(),
            EventKind::DrawIndirect,
            format!("Draw Misty Mesh Casters ({what}{cutout})"),
            format!(
                "vgeo_depth.wgsl {}",
                if d.cutout {
                    "fs_shadow_cutout"
                } else {
                    "depth only"
                }
            ),
            "LessEqual, write on (slope bias)",
        ),
        VgeoPass::Prepass => (
            Vec::new(),
            EventKind::DrawIndirect,
            format!("Draw Misty Mesh Normals ({what}{cutout})"),
            format!(
                "vgeo_prepass.wgsl + ao_prepass.wgsl {}",
                if d.cutout {
                    "fs_normal_cutout"
                } else {
                    "fs_normal"
                }
            ),
            "Less, write on",
        ),
    };
    FrameEvent {
        pass,
        material,
        batch: Some(format!(
            "GPU-driven: the clusters of every object with this material picked for view {} by \
             vgeo_cull.wgsl (one indirect draw, count written by the GPU)",
            d.view
        )),
        instances: 0,
        shader,
        depth: depth.to_owned(),
        blend: "Off".to_owned(),
        cull: if d.pass == VgeoPass::Resolve {
            "Off"
        } else {
            "Back"
        }
        .to_owned(),
        textures: textures(&draw.material),
        properties: properties(&draw.material),
        bindings: vec![
            (
                "Group 0 · View".to_owned(),
                format!("view {} uniform", d.view),
            ),
            (
                "Cluster pool".to_owned(),
                "vertices, triangles and cluster records of every virtual mesh".to_owned(),
            ),
            (
                "Visible list".to_owned(),
                format!("clusters vgeo_cull.wgsl kept for view {}", d.view),
            ),
            (
                "Indirect arguments".to_owned(),
                "one indexed command per visible cluster; the count is read from the GPU \
                 (multi_draw_indexed_indirect_count)"
                    .to_owned(),
            ),
        ],
        ..FrameEvent::new(kind, name)
    }
}

/// The material shown by the Frame Debugger's Emission view: unlit, its colour the emission
/// (black without), its texture the emission map.
pub(crate) fn emission_only(d: &MeshDraw) -> MeshDraw {
    let mut d = d.clone();
    let m = &mut d.material;
    let e = m.emission;
    let k = m.emission_intensity;
    m.color = [e[0] * k, e[1] * k, e[2] * k, m.color[3]];
    m.albedo_map = m.emission_map.take();
    m.normal_map = None;
    m.metallic_map = None;
    m.occlusion_map = None;
    m.unlit = true;
    m.custom = None;
    if m.mode != RenderingMode::Cutout {
        m.mode = RenderingMode::Opaque;
    }
    d
}

/// The terrain grass: blades drawn indirectly when the GPU culled them.
pub(in crate::render) fn grass_event(indirect: bool) -> FrameEvent {
    let mut ev = post_event("Draw Grass".to_owned(), "grass.wgsl");
    if indirect {
        ev.kind = Some(EventKind::DrawIndirect);
        ev.name = "Draw Grass (indirect, GPU-culled chunks)".to_owned();
        ev.vertices = 0;
        ev.batch = Some(
            "GPU-driven: grass_cull.wgsl picked the visible blades of each chunk and wrote \
             one draw_indirect command per chunk"
                .to_owned(),
        );
        ev.bindings = vec![
            ("Group 0 · View".to_owned(), "view uniform".to_owned()),
            (
                "Group 2 · Terrain".to_owned(),
                "grass params, height and colour maps".to_owned(),
            ),
            (
                "Group 3 · Blade page".to_owned(),
                "blade storage of the chunk's page".to_owned(),
            ),
            (
                "Indirect arguments".to_owned(),
                "grass cull args buffer, 16 bytes per chunk (vertex count, instance count \
                 written by the GPU)"
                    .to_owned(),
            ),
        ];
    }
    ev
}

/// Visual Effect outputs (simulated on the GPU, drawn indirectly).
pub(in crate::render) fn effects_event() -> FrameEvent {
    FrameEvent {
        shader: "generated effect shaders (Effect Graph outputs)".to_owned(),
        batch: Some(
            "GPU-driven: each output draws its alive particles with draw_indirect (count \
             written by the simulation)"
                .to_owned(),
        ),
        blend: "per output".to_owned(),
        depth: "LessEqual, write off".to_owned(),
        cull: "Off".to_owned(),
        bindings: vec![
            ("Group 1 · Material".to_owned(), "output texture".to_owned()),
            (
                "Group 2 · Particles".to_owned(),
                "particle buffers (ping-pong) and alive list".to_owned(),
            ),
            (
                "Group 3 · Depth".to_owned(),
                "scene depth (soft particles)".to_owned(),
            ),
            (
                "Indirect arguments".to_owned(),
                "the effect's indirect buffer, 32 bytes per output".to_owned(),
            ),
        ],
        instances: 0,
        ..FrameEvent::new(
            EventKind::DrawIndirect,
            "Draw Visual Effects (indirect)".to_owned(),
        )
    }
}

/// One GPU-culled multi-draw: `batches` share pipeline, material and mesh buffers; their
/// instance counts were written by the culling pass of `phase`.
pub(in crate::render) fn culled_event(
    rec: &Recorder,
    frame: &Frame,
    batches: &[Batch],
    m: &GpuMesh,
    key: PipelineKey,
    phase: usize,
    args_offset: u64,
) -> FrameEvent {
    let first = &batches[0];
    let d = &frame.meshes[first.draw];
    let label = rec.label(first.draw);
    let mesh = mesh_name(&d.mesh);
    let object = label
        .map(|l| l.object.clone())
        .filter(|o| !o.is_empty())
        .unwrap_or_else(|| mesh.clone());
    let candidates: u32 = batches.iter().map(|b| b.count).sum();
    let name = match batches.len() {
        1 => format!("Draw Indexed Indirect {object}"),
        n => format!("Multi Draw Indexed Indirect ×{n} ({object}, …)"),
    };
    let phase_name = match phase {
        0 => "first phase (frustum + last frame's Hi-Z)",
        _ => "second phase (this frame's Hi-Z)",
    };
    let mut bindings = mesh_bindings(d, &mesh, m, candidates, false);
    bindings.push((
        "Indirect arguments".to_owned(),
        format!(
            "GPU culling args buffer @ {args_offset} bytes, {} command(s) of 20 bytes",
            batches.len()
        ),
    ));
    FrameEvent {
        object: Some(object),
        mesh: Some(mesh),
        material: label.and_then(|l| l.material.clone()),
        vertices: m.vertex_count,
        indices: m.count,
        instances: candidates,
        batch: Some(format!(
            "GPU-driven: {} batch(es) of {candidates} candidate objects culled on the GPU in \
             the {phase_name}; visible instance counts stay on the GPU",
            batches.len()
        )),
        shader: shader_label(key),
        depth: match key.depth_write() {
            true => "LessEqual, write on",
            false => "LessEqual, write off",
        }
        .to_owned(),
        blend: blend_label(key.blend()),
        cull: if key.double_sided { "Off" } else { "Back" }.to_owned(),
        textures: textures(&d.material),
        properties: properties(&d.material),
        bindings,
        ..FrameEvent::new(EventKind::DrawIndirect, name)
    }
}
