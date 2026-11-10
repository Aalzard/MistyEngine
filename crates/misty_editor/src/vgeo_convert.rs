//! One-click Misty Virtual Geometry: turn the Virtual Geometry import setting on (or off) for
//! the models behind the selected objects, every object of the open scenes, or every model of
//! the project.
//!
//! - **GameObject ▸ Enable / Disable Virtual Geometry** (Hierarchy context menu, Mesh Filter
//!   Inspector button): the selected objects and their children, right away.
//! - **Tools ▸ Rendering ▸ Convert Scene / Project to Virtual Geometry**: a short dialog (triangle
//!   threshold, "N models, M objects, est. memory", what is skipped and why), then the same.
//!
//! The setting lives in each model's `.meta`; nothing in the scene changes (a Mesh Filter with a
//! virtual mesh draws through the cluster path by itself). Converting re-imports the models; the
//! cluster builds run on the import worker while a progress window counts them. A conversion is
//! one undo step: it puts the import settings back (and the Quality levels it turned on).
//! Ineligible meshes are skipped with a reason: skinned or morphing meshes, alpha-tested,
//! transparent, double-sided, unlit or custom-shader materials, meshes under the triangle
//! threshold, and LOD 1+ of LOD Groups (their LOD 0 takes over).

use std::collections::BTreeMap;

use egui::Ui;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::models::{self, Model, ModelImporter};
use misty_engine::render::{RenderingMode, ShaderKind};
use misty_engine::scene::{LodGroup, MeshFilter, MeshRef, MeshRenderer, SkinnedMeshRenderer};

use crate::state::EditorState;

/// Meshes below this many triangles are skipped by default: there is nothing to cluster, the
/// ordinary path is as fast.
pub const DEFAULT_MIN_TRIANGLES: usize = 5000;

/// GPU bytes of the cluster pool per source vertex and per source triangle (measured on the
/// generated rocks: 32-byte vertices, then every level's local indices, cluster vertex lists
/// and records come to ~18 bytes per source triangle).
const BYTES_PER_VERTEX: u64 = 32;
const BYTES_PER_TRIANGLE: u64 = 18;

/// What a Virtual Geometry command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The selected objects (and their children): on, right away.
    EnableSelected,
    DisableSelected,
    /// Tools ▸ Rendering ▸ Convert … to Virtual Geometry (opens the dialog).
    ConvertDialog(Scope),
}

/// What a conversion dialog covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every object of the open scenes.
    Scene,
    /// Every model of the project.
    Project,
}

impl Scope {
    pub fn title(self) -> &'static str {
        match self {
            Scope::Scene => "Convert Scene to Misty Mesh",
            Scope::Project => "Convert Project to Misty Mesh",
        }
    }
}

/// Why a mesh is not converted.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skip {
    Skinned,
    BlendShapes,
    AlphaTested,
    Transparent,
    DoubleSided,
    Unlit,
    CustomShader,
    /// Fewer triangles than the threshold (the count).
    Small(usize),
    /// LOD 1 or coarser in a LOD Group: LOD 0 stands in for every level once it is virtual.
    LodLevel,
    /// The model could not be imported (the error).
    Missing(String),
}

impl Skip {
    /// Shown in the dialog and the Console.
    pub fn reason(&self, min_triangles: usize) -> String {
        match self {
            Skip::Skinned => "skinned (the hierarchy is built from rest positions)".into(),
            Skip::BlendShapes => "blend shapes (the hierarchy can't morph)".into(),
            Skip::AlphaTested => {
                "alpha-tested material (foliage cards lose coverage when simplified)".into()
            }
            Skip::Transparent => "transparent material (drawn the ordinary way)".into(),
            Skip::DoubleSided => "double-sided material (drawn the ordinary way)".into(),
            Skip::Unlit => "unlit material (drawn the ordinary way)".into(),
            Skip::CustomShader => "custom shader (its vertex stage is its own)".into(),
            Skip::Small(n) => format!(
                "{n} triangles, under the {min_triangles} threshold (the ordinary path is as fast)"
            ),
            Skip::LodLevel => "LOD 1+ of a LOD Group (LOD 0 takes over every level)".into(),
            Skip::Missing(e) => format!("could not be imported: {e}"),
        }
    }
}

/// Why a material keeps a mesh off the cluster path (`None`: Standard, Opaque, single-sided).
pub fn material_skip(m: &Material) -> Option<Skip> {
    if misty_engine::shader_asset::is_shader_path(&m.shader) {
        return Some(Skip::CustomShader);
    }
    if m.shader_kind() != ShaderKind::Standard {
        return Some(Skip::Unlit);
    }
    match m.rendering_mode {
        RenderingMode::Opaque => {}
        RenderingMode::Cutout => return Some(Skip::AlphaTested),
        RenderingMode::Fade | RenderingMode::Transparent => return Some(Skip::Transparent),
    }
    m.double_sided.then_some(Skip::DoubleSided)
}

/// The facts about one mesh (a glTF mesh's primitives together) that decide eligibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshFacts {
    pub skinned: bool,
    pub blend_shapes: bool,
    pub triangles: usize,
    pub vertices: usize,
}

impl MeshFacts {
    /// Mesh `index` of `model`, with every primitive built together with it.
    pub fn of(model: &Model, index: usize) -> Option<Self> {
        let mesh = model.meshes.get(index)?;
        let first = mesh.primitive.map_or(index, |(f, _)| f);
        let mut facts = MeshFacts {
            skinned: !model.skins.is_empty(),
            ..Default::default()
        };
        for j in models::virtual_build::parts(model, first) {
            let m = &model.meshes[j];
            facts.skinned |= m.skin.is_some();
            facts.blend_shapes |= !m.blend_shapes.is_empty();
            facts.triangles += m.data.indices.len() / 3;
            facts.vertices += m.data.vertices.len();
        }
        Some(facts)
    }

    pub fn skip(&self, min_triangles: usize) -> Option<Skip> {
        if self.skinned {
            Some(Skip::Skinned)
        } else if self.blend_shapes {
            Some(Skip::BlendShapes)
        } else if self.triangles < min_triangles {
            Some(Skip::Small(self.triangles))
        } else {
            None
        }
    }
}

/// A model a conversion turns on.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPlan {
    pub rel: String,
    /// Its eligible meshes together.
    pub triangles: usize,
    pub vertices: usize,
    /// Objects drawing it (scene and selection plans).
    pub objects: usize,
    /// The import setting is already on.
    pub already: bool,
}

impl ModelPlan {
    /// Estimated GPU memory of its cluster builds.
    pub fn bytes(&self) -> u64 {
        self.vertices as u64 * BYTES_PER_VERTEX + self.triangles as u64 * BYTES_PER_TRIANGLE
    }
}

/// What a conversion would do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Models with at least one eligible mesh, by path.
    pub models: Vec<ModelPlan>,
    /// (model path, why, objects) for models left out entirely, by path.
    pub skipped: Vec<(String, Skip, usize)>,
    /// Objects that will draw through clusters.
    pub objects: usize,
    pub min_triangles: usize,
}

impl Plan {
    /// Paths of the models whose setting changes.
    pub fn to_change(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|m| !m.already)
            .map(|m| m.rel.clone())
            .collect()
    }

    /// Estimated GPU memory of every model's clusters.
    pub fn bytes(&self) -> u64 {
        self.models.iter().map(ModelPlan::bytes).sum()
    }

    /// "3 models, 64 objects, est. 8.6 MB" (the dialog's summary).
    pub fn summary(&self, scope: Option<Scope>) -> String {
        let n = self.models.len();
        let mut s = format!("{n} model{}", if n == 1 { "" } else { "s" });
        if scope != Some(Scope::Project) {
            let o = self.objects;
            s += &format!(", {o} object{}", if o == 1 { "" } else { "s" });
        }
        s += &format!(", est. {:.1} MB", self.bytes() as f64 / 1e6);
        let already = self.models.iter().filter(|m| m.already).count();
        if already > 0 {
            s += &format!(" ({already} already on)");
        }
        s
    }
}

/// Per model: eligible meshes (index → facts), eligible objects, first skip reason and the
/// objects it applies to.
#[derive(Default)]
struct Tally {
    meshes: BTreeMap<usize, MeshFacts>,
    objects: usize,
    skip: Option<Skip>,
    skipped_objects: usize,
}

impl Tally {
    fn skip(&mut self, why: Skip) {
        self.skipped_objects += 1;
        // The most telling reason: anything before "small" or "LOD level".
        if self.skip.as_ref().is_none_or(|s| why < *s) {
            self.skip = Some(why);
        }
    }
}

fn finish(s: &EditorState, tallies: BTreeMap<String, Tally>, min_triangles: usize) -> Plan {
    let mut plan = Plan {
        min_triangles,
        ..Default::default()
    };
    for (rel, t) in tallies {
        if t.meshes.is_empty() {
            let why = t.skip.unwrap_or(Skip::Small(0));
            plan.skipped.push((rel, why, t.skipped_objects));
            continue;
        }
        let already = ModelImporter::load(&s.assets.absolute(&rel)).virtual_geometry;
        plan.objects += t.objects;
        plan.models.push(ModelPlan {
            triangles: t.meshes.values().map(|f| f.triangles).sum(),
            vertices: t.meshes.values().map(|f| f.vertices).sum(),
            objects: t.objects,
            already,
            rel,
        });
    }
    plan
}

/// Is `r` a mesh of an imported model file (the Mesh Filter Inspector's button)?
pub fn is_model_mesh(r: &MeshRef) -> bool {
    model_mesh(r).is_some()
}

/// The model file and mesh index of an imported (not generated, not posed) mesh.
fn model_mesh(r: &MeshRef) -> Option<(&str, usize)> {
    match r {
        MeshRef::Asset(key)
            if misty_engine::skinning::is_pose_key(key)
                || key.starts_with(models::runtime::PREFIX) =>
        {
            None
        }
        _ => r.split_asset(),
    }
}

/// The plan for `objects` (each with its children), e.g. the selection or every object of the
/// open scenes.
pub fn plan_objects(s: &EditorState, objects: &[Entity], min_triangles: usize) -> Plan {
    let scene = &s.scene;
    let all: Vec<Entity> = scene.walk().into_iter().map(|(e, _)| e).collect();
    let wanted: Vec<Entity> = all
        .iter()
        .copied()
        .filter(|&e| objects.iter().any(|&o| o == e || scene.is_ancestor(o, e)))
        .collect();
    // Renderers in LOD 1+ of a LOD Group.
    let mut lod_rest = std::collections::HashSet::new();
    for &e in &all {
        if let Ok(g) = scene.world.get::<&LodGroup>(e) {
            lod_rest.extend(
                g.lods
                    .iter()
                    .skip(1)
                    .flat_map(|l| l.renderers.iter().copied()),
            );
        }
    }
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for e in wanted {
        if let Ok(r) = scene.world.get::<&SkinnedMeshRenderer>(e)
            && let Some((path, _)) = r.mesh.as_ref().and_then(model_mesh)
        {
            tallies
                .entry(path.to_owned())
                .or_default()
                .skip(Skip::Skinned);
        }
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let Some((path, index)) = model_mesh(&mesh) else {
            continue;
        };
        if crate::mesh_builder::has_editable(s, e) {
            continue;
        }
        let tally = tallies.entry(path.to_owned()).or_default();
        if lod_rest.contains(&e) {
            tally.skip(Skip::LodLevel);
            continue;
        }
        let model = match models::model(path) {
            Ok(m) => m,
            Err(err) => {
                tally.skip(Skip::Missing(err));
                continue;
            }
        };
        let Some(facts) = MeshFacts::of(&model, index) else {
            tally.skip(Skip::Missing(format!("no mesh #{index}")));
            continue;
        };
        // Every material the renderer draws the mesh with (None = Default-Material: Standard,
        // Opaque).
        let materials: Vec<Option<String>> = scene
            .world
            .get::<&MeshRenderer>(e)
            .map(|r| {
                std::iter::once(r.material.clone())
                    .chain(r.extra_materials.iter().cloned())
                    .collect()
            })
            .unwrap_or_else(|_| vec![None]);
        let material = materials.iter().find_map(|m| {
            m.as_ref()
                .and_then(|rel| s.assets.materials.get(rel))
                .and_then(material_skip)
        });
        match facts.skip(min_triangles).or(material) {
            Some(why) => tally.skip(why),
            None => {
                let first = model.meshes[index].primitive.map_or(index, |(f, _)| f);
                tally.meshes.insert(first, facts);
                tally.objects += 1;
            }
        }
    }
    finish(s, tallies, min_triangles)
}

/// The plan for every model of the project (by the models' own materials).
pub fn plan_project(s: &EditorState, min_triangles: usize) -> Plan {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for rel in s.assets.models.keys() {
        let tally = tallies.entry(rel.clone()).or_default();
        let model = match models::model(rel) {
            Ok(m) => m,
            Err(err) => {
                tally.skip(Skip::Missing(err));
                continue;
            }
        };
        for (i, mesh) in model.meshes.iter().enumerate() {
            // Later primitives of a glTF mesh are counted with their first one.
            if mesh.primitive.is_some_and(|(f, _)| f != i) {
                continue;
            }
            let Some(facts) = MeshFacts::of(&model, i) else {
                continue;
            };
            let material = models::virtual_build::parts(&model, i)
                .iter()
                .filter_map(|&j| model.meshes[j].material)
                .filter_map(|m| model.materials.get(m))
                .find_map(|m| material_skip(&m.material));
            match facts.skip(min_triangles).or(material) {
                Some(why) => tally.skip(why),
                None => {
                    tally.meshes.insert(i, facts);
                }
            }
        }
        if model.meshes.is_empty() {
            tally.skip(Skip::Small(0));
        }
    }
    finish(s, tallies, min_triangles)
}

/// Quality levels to turn Virtual Geometry on in: High and Ultra (by name; without levels of
/// those names, the default level and the ones after it) that have it off.
pub fn quality_levels_to_enable(q: &misty_engine::quality::QualitySettings) -> Vec<usize> {
    let named: Vec<usize> = (q.levels.iter().enumerate())
        .filter(|(_, l)| l.name == "High" || l.name == "Ultra")
        .map(|(i, _)| i)
        .collect();
    let levels = match named.is_empty() {
        true => (q.default_level..q.levels.len()).collect(),
        false => named,
    };
    levels
        .into_iter()
        .filter(|&i| !q.levels[i].virtual_geometry)
        .collect()
}

/// Turn the setting `on` for `models`, as one undo step named `label` (it also puts back the
/// Quality levels `quality` turns on). Re-imports them; turning on starts the progress window.
/// Returns how many models changed.
pub fn apply(
    s: &mut EditorState,
    label: &str,
    models_to_change: &[String],
    on: bool,
    quality: &[usize],
) -> usize {
    let mut before = Vec::new();
    for rel in models_to_change {
        let abs = s.assets.absolute(rel);
        let settings = ModelImporter::load(&abs);
        if settings.virtual_geometry != on {
            before.push((rel.clone(), settings.virtual_geometry));
        }
    }
    let quality: Vec<usize> = quality
        .iter()
        .copied()
        .filter(|&i| {
            s.project_settings
                .quality
                .levels
                .get(i)
                .is_some_and(|l| !l.virtual_geometry)
        })
        .collect();
    if before.is_empty() && quality.is_empty() {
        return 0;
    }
    let undo = crate::undo::ImportUndo {
        models: before.clone(),
        quality: (!quality.is_empty()).then(|| s.project_settings.quality.clone()),
    };
    s.undo.push_imports(label, &s.scene, &s.selection, undo);
    let changed: Vec<String> = before.into_iter().map(|(rel, _)| rel).collect();
    let failed = set_models(s, &changed, on);
    for (rel, e) in &failed {
        s.console
            .warn(format!("Could not save the import settings of {rel}: {e}"));
    }
    if !quality.is_empty() {
        let names: Vec<String> = quality
            .iter()
            .map(|&i| s.project_settings.quality.levels[i].name.clone())
            .collect();
        for &i in &quality {
            s.project_settings.quality.levels[i].virtual_geometry = true;
        }
        crate::project_settings::changed(s);
        s.console.info(format!(
            "Quality ▸ Misty Mesh turned on for {}.",
            names.join(" and ")
        ));
    }
    if on && !changed.is_empty() {
        s.vgeo.progress = Some(Progress::new(changed.clone()));
    }
    changed.len() - failed.len()
}

/// Write the setting into each model's `.meta` and drop the cached models (the next use
/// imports them again). Returns the models that could not be saved.
pub fn set_models(s: &EditorState, rels: &[String], on: bool) -> Vec<(String, String)> {
    let mut failed = Vec::new();
    for rel in rels {
        let abs = s.assets.absolute(rel);
        let mut settings = ModelImporter::load(&abs);
        settings.virtual_geometry = on;
        if let Err(e) = settings.save(&abs) {
            failed.push((rel.clone(), e));
        }
    }
    models::forget(rels);
    failed
}

/// "Boulder.glb" from "Assets/Rocks/Boulder.glb".
fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Run a command from a menu, the Inspector or the search palette.
pub fn run(s: &mut EditorState, cmd: Command) {
    match cmd {
        Command::ConvertDialog(scope) => {
            s.vgeo.dialog = Some(Dialog::new(scope, s.vgeo.min_triangles));
        }
        Command::EnableSelected => {
            let targets = s.selection.list().to_vec();
            let plan = plan_objects(s, &targets, s.vgeo.min_triangles);
            for (rel, why, _) in &plan.skipped {
                let reason = why.reason(plan.min_triangles);
                s.console
                    .info(format!("Misty Mesh: skipped {}: {reason}.", file_name(rel)));
            }
            let change = plan.to_change();
            if plan.models.is_empty() {
                s.console
                    .info("Misty Mesh: nothing in the selection can use it.");
                return;
            }
            let quality = quality_levels_to_enable(&s.project_settings.quality);
            let label = format!("Enable Misty Mesh on {}", s.selection_label());
            let n = apply(s, &label, &change, true, &quality);
            let already = plan.models.len() - change.len();
            s.console.info(format!(
                "Misty Mesh on for {n} model{}{}.",
                if n == 1 { "" } else { "s" },
                if already > 0 {
                    format!(" ({already} already on)")
                } else {
                    String::new()
                }
            ));
        }
        Command::DisableSelected => {
            let targets = s.selection.list().to_vec();
            let rels = selected_models(s, &targets, true);
            if rels.is_empty() {
                s.console
                    .info("Misty Mesh: no model in the selection uses it.");
                return;
            }
            let label = format!("Disable Misty Mesh on {}", s.selection_label());
            let n = apply(s, &label, &rels, false, &[]);
            s.console.info(format!(
                "Misty Mesh off for {n} model{}.",
                if n == 1 { "" } else { "s" }
            ));
        }
    }
}

/// Models drawn by `objects` (and their children) whose import setting is `on` (or off).
pub fn selected_models(s: &EditorState, objects: &[Entity], on: bool) -> Vec<String> {
    let scene = &s.scene;
    let mut out: Vec<String> = Vec::new();
    for (e, _) in scene.walk() {
        if !objects.iter().any(|&o| o == e || scene.is_ancestor(o, e)) {
            continue;
        }
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let Some((path, _)) = model_mesh(&mesh) else {
            continue;
        };
        if out.iter().any(|p| p == path) {
            continue;
        }
        let set = ModelImporter::load(&s.assets.absolute(path)).virtual_geometry;
        if set == on {
            out.push(path.to_owned());
        }
    }
    out
}

/// Which of Enable / Disable apply to the selection (the context menu shows only those).
pub fn selection_state(s: &EditorState) -> (bool, bool) {
    let targets = s.selection.list();
    if targets.is_empty() {
        return (false, false);
    }
    let scene = &s.scene;
    let (mut off, mut on) = (false, false);
    for (e, _) in scene.walk() {
        if !targets.iter().any(|&o| o == e || scene.is_ancestor(o, e)) {
            continue;
        }
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        if model_mesh(&mesh).is_none() {
            continue;
        }
        // The in-memory import (the selected objects are drawn: their models are loaded).
        if models::is_virtual(&mesh) {
            on = true;
        } else {
            off = true;
        }
        if on && off {
            break;
        }
    }
    (off, on)
}

// ---- windows ------------------------------------------------------------------------------

/// The conversion dialog and the build progress.
pub struct VgeoTools {
    pub dialog: Option<Dialog>,
    pub progress: Option<Progress>,
    /// The triangle threshold (the dialog edits it; Enable on the selection uses it too).
    pub min_triangles: usize,
}

impl Default for VgeoTools {
    fn default() -> Self {
        Self {
            dialog: None,
            progress: None,
            min_triangles: DEFAULT_MIN_TRIANGLES,
        }
    }
}

pub struct Dialog {
    pub scope: Scope,
    pub min_triangles: usize,
    /// Turn Quality ▸ Virtual Geometry on for High and Ultra.
    pub quality: bool,
    /// Made for `min_triangles`; remade when it changes.
    plan: Option<Plan>,
}

impl Dialog {
    fn new(scope: Scope, min_triangles: usize) -> Self {
        Self {
            scope,
            min_triangles,
            quality: true,
            plan: None,
        }
    }
}

/// Models being imported and built after a conversion.
pub struct Progress {
    pub models: Vec<String>,
    /// Models imported so far (one per frame, so the window keeps drawing).
    pub imported: usize,
}

impl Progress {
    fn new(models: Vec<String>) -> Self {
        Self {
            models,
            imported: 0,
        }
    }

    /// Import the next model; (fraction done, what is happening, finished).
    pub fn step(&mut self) -> (f32, String, bool) {
        if let Some(rel) = self.models.get(self.imported) {
            // Importing starts its cluster build on the worker.
            let _ = models::model(rel);
            self.imported += 1;
            let n = self.models.len();
            let text = format!("Importing {} ({} of {n})…", file_name(rel), self.imported);
            return (0.2 * self.imported as f32 / n as f32, text, false);
        }
        let (mut built, mut meshes) = (0, 0);
        for rel in &self.models {
            if let Ok(m) = models::model(rel) {
                let s = m.virtual_builds.summary();
                built += s.built;
                meshes += s.meshes;
            }
        }
        let text = format!("Building clusters: {built} of {meshes} meshes…");
        let fraction = 0.2 + 0.8 * built as f32 / meshes.max(1) as f32;
        (fraction, text, built >= meshes)
    }
}

/// Draw the dialog and the progress window (every frame).
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    if let Some(mut p) = s.vgeo.progress.take() {
        let (fraction, text, done) = p.step();
        if done {
            let n = p.models.len();
            s.console.info(format!(
                "Misty Mesh: {n} model{} built.",
                if n == 1 { "" } else { "s" }
            ));
        } else {
            egui::Window::new("Misty Mesh")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -36.0])
                .show(ctx, |ui| {
                    ui.add(egui::ProgressBar::new(fraction).desired_width(260.0));
                    ui.weak(text);
                });
            s.vgeo.progress = Some(p);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    let Some(scope) = s.vgeo.dialog.as_ref().map(|d| d.scope) else {
        return;
    };
    let mut open = true;
    egui::Window::new(scope.title())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| dialog_contents(ui, s));
    if !open {
        s.vgeo.dialog = None;
    }
}

/// The conversion dialog's body (kittest draws it on its own).
pub fn dialog_contents(ui: &mut Ui, s: &mut EditorState) {
    let Some(mut d) = s.vgeo.dialog.take() else {
        return;
    };
    ui.weak(
        "Dense opaque meshes draw as clusters with per-cluster level of detail. Nothing in \
         the scene changes; one undo step puts it back.",
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Min. triangles").on_hover_text(
            "Meshes with fewer triangles keep the ordinary path (nothing to cluster)",
        );
        ui.add(
            egui::DragValue::new(&mut d.min_triangles)
                .speed(100.0)
                .range(0..=10_000_000),
        );
    });
    if d.plan
        .as_ref()
        .is_none_or(|p| p.min_triangles != d.min_triangles)
    {
        let plan = match d.scope {
            Scope::Scene => {
                let roots = s.scene.roots();
                plan_objects(s, roots, d.min_triangles)
            }
            Scope::Project => plan_project(s, d.min_triangles),
        };
        d.plan = Some(plan);
    }
    let plan = d.plan.clone().unwrap_or_default();
    ui.add_space(2.0);
    ui.strong(plan.summary(Some(d.scope)));
    let levels = quality_levels_to_enable(&s.project_settings.quality);
    if !levels.is_empty() {
        let names: Vec<&str> = levels
            .iter()
            .map(|&i| s.project_settings.quality.levels[i].name.as_str())
            .collect();
        ui.checkbox(
            &mut d.quality,
            format!("Turn on Quality ▸ Misty Mesh for {}", names.join(" and ")),
        );
    }
    if !plan.skipped.is_empty() {
        egui::CollapsingHeader::new(format!("Skipped ({})", plan.skipped.len()))
            .id_salt("vgeo_skipped")
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (rel, why, objects) in &plan.skipped {
                            let what = match (d.scope, *objects) {
                                (Scope::Project, _) | (_, 0) => String::new(),
                                (_, 1) => " (1 object)".into(),
                                (_, n) => format!(" ({n} objects)"),
                            };
                            ui.label(format!("{}{what}", file_name(rel)))
                                .on_hover_text(rel);
                            ui.weak(format!("    {}", why.reason(plan.min_triangles)));
                        }
                    });
            });
    }
    ui.add_space(6.0);
    let change = plan.to_change();
    let mut close = false;
    // Nothing left to turn on: say so instead of leaving a silently greyed-out button.
    let nothing_to_do = change.is_empty() && levels.is_empty();
    if nothing_to_do {
        let what = match d.scope {
            Scope::Scene => "this scene",
            Scope::Project => "the project",
        };
        let line = if plan.models.is_empty() {
            format!("Nothing in {what} is dense enough to convert.")
        } else {
            format!("Everything eligible in {what} already uses Misty Mesh.")
        };
        ui.colored_label(ui.visuals().widgets.active.fg_stroke.color, line);
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        let convert =
            !change.is_empty() || (d.quality && !levels.is_empty() && !plan.models.is_empty());
        if ui
            .add_enabled(convert, egui::Button::new("Convert"))
            .on_disabled_hover_text(if nothing_to_do {
                "Nothing to convert: every eligible model already has Misty Mesh on, \
                and Quality ▸ Misty Mesh is on."
            } else {
                "Tick the Quality box above, or lower Min. triangles."
            })
            .clicked()
        {
            s.vgeo.min_triangles = d.min_triangles;
            let quality = if d.quality {
                levels.clone()
            } else {
                Vec::new()
            };
            let n = apply(s, d.scope.title(), &change, true, &quality);
            s.console.info(format!(
                "Misty Mesh on for {n} model{}.",
                if n == 1 { "" } else { "s" }
            ));
            close = true;
        }
        if ui.button("Cancel").clicked() {
            close = true;
        }
    });
    if !close {
        s.vgeo.dialog = Some(d);
    }
}

#[cfg(test)]
mod tests;
