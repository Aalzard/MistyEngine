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
    FrameEvent {
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
