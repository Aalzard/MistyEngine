//! The Preview card: a top-down image of the world that follows every base / erosion / stamp /
//! water / biome edit. While you drag, a coarse pass (129²) runs on a worker thread as often as
//! it can; once you stop for a moment a sharper pass (257²) replaces it. Each pass has its own
//! stage cache, so dragging a stamp only re-runs the stamps and later stages. Passes are
//! cancelled when a newer edit makes them stale. "Live in Scene view" also pushes each result
//! into the terrain (cheap, low resolution; a full Generate replaces it).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::terrain::{TerrainData, store};
use misty_engine::world_gen::classify::WaterMap;
use misty_engine::world_gen::generate::{self, Cache};
use misty_engine::world_gen::preview::{self as image, PreviewImage, PreviewMode};
use misty_engine::world_gen::{self, Biome, Progress, Stages, WorldBuilder};

use crate::state::EditorState;
use crate::terrain_tools::TerrainSnapshot;

/// Heightmap resolution of the pass that runs while editing.
pub const COARSE: usize = 129;
/// Heightmap resolution of the pass after the edits stop.
pub const SHARP: usize = 257;
/// Quiet time before the sharp pass.
const SETTLE: Duration = Duration::from_millis(300);
/// Side of the preview image in pixels.
const IMAGE: usize = 256;

/// Preview card state (kept while the window is closed).
pub struct PreviewState {
    pub mode: PreviewMode,
    /// Signature of the settings the shown image was made from.
    shown: Option<String>,
    /// Signature last asked for and when it changed.
    wanted: Option<String>,
    changed_at: Option<Instant>,
    /// The sharp pass ran for `shown`.
    sharp_done: bool,
    job: Option<PreviewJob>,
    coarse_cache: Arc<Mutex<Cache>>,
    sharp_cache: Arc<Mutex<Cache>>,
    /// Average colour of layer textures (Biomes mode), by path.
    colors: Arc<Mutex<HashMap<String, [f32; 3]>>>,
    /// Last result (re-rendered when the mode changes).
    last: Option<Arc<(TerrainData, WaterMap)>>,
    texture: Option<egui::TextureHandle>,
    texture_mode: Option<PreviewMode>,
    /// "129² · 24 ms" of the shown image.
    pub info: String,
    builder: Option<Entity>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            mode: PreviewMode::Relief,
            shown: None,
            wanted: None,
            changed_at: None,
            sharp_done: false,
            job: None,
            coarse_cache: Default::default(),
            sharp_cache: Default::default(),
            colors: Default::default(),
            last: None,
            texture: None,
            texture_mode: None,
            info: String::new(),
            builder: None,
        }
    }
}

struct PreviewJob {
    handle: JoinHandle<Option<PassResult>>,
    progress: Arc<Progress>,
    signature: String,
    sharp: bool,
    builder: Entity,
    terrain_data: String,
    started: Instant,
}

struct PassResult {
    data: TerrainData,
    wet: WaterMap,
    image: PreviewImage,
}

impl PreviewState {
    /// A pass is running.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// An image is shown (tests).
    #[cfg(test)]
    pub fn has_image(&self) -> bool {
        self.texture.is_some()
    }

    /// Forget results (another builder, or the settings were replaced wholesale).
    fn reset(&mut self) {
        if let Some(j) = &self.job {
            j.progress.cancel();
        }
        self.shown = None;
        self.wanted = None;
        self.sharp_done = false;
        self.last = None;
        self.texture = None;
        self.texture_mode = None;
    }
}

/// The signature of what a generation reads, plus the biome files' modification times (editing
/// a biome asset changes the preview too).
fn signature(state: &EditorState, builder: Entity) -> Option<String> {
    let mut sig = super::job::signature(state, builder)?;
    let root = &state.project.root;
    let scene = &state.scene;
    let mut paths = Vec::new();
    if let Ok(s) = scene.world.get::<&WorldBuilder>(builder) {
        paths.push(s.default_biome.clone());
    }
    for e in world_gen::stamps_of(scene, builder) {
        if let Ok(st) = scene.world.get::<&world_gen::WorldStamp>(e)
            && let world_gen::StampKind::BiomeZone(z) = &st.kind
        {
            paths.push(z.biome.clone());
        }
    }
    for p in paths {
        let t = std::fs::metadata(root.join(&p))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis());
        sig += &format!("|{p}@{t}");
    }
    Some(sig)
}

/// Keep the preview up to date: pick up a finished pass, start the next one. Call every frame
/// the window is drawn.
pub fn tick(state: &mut EditorState, builder: Entity, ctx: &egui::Context) {
    poll(state, ctx);
    let p = &mut state.world_builder.preview;
    if p.builder != Some(builder) {
        p.reset();
        p.builder = Some(builder);
    }
    let Some(sig) = signature(state, builder) else {
        return;
    };
    let p = &mut state.world_builder.preview;
    if p.wanted.as_ref() != Some(&sig) {
        p.wanted = Some(sig.clone());
        p.changed_at = Some(Instant::now());
        // A sharp pass for older settings is wasted work now.
        if let Some(j) = &p.job
            && j.sharp
        {
            j.progress.cancel();
        }
    }
    if p.job.is_some() {
        ctx.request_repaint_after(Duration::from_millis(16));
        return;
    }
    let stale = p.shown.as_ref() != Some(&sig);
    let settled = p.changed_at.is_none_or(|t| t.elapsed() >= SETTLE);
    if stale {
        start(state, builder, sig, false);
    } else if !p.sharp_done && settled {
        start(state, builder, sig, true);
    } else if !p.sharp_done {
        ctx.request_repaint_after(SETTLE);
    }
}

/// Start a coarse or sharp pass for `sig`.
fn start(state: &mut EditorState, builder: Entity, sig: String, sharp: bool) {
    let root = state.project.root.clone();
    let load = |p: &str| Biome::load(&root.join(p));
    let Ok((mut req, _warnings)) =
        world_gen::request(&state.scene, builder, Stages::All, true, None, &load)
    else {
        // No terrain yet: nothing to preview, and nothing to retry until something changes.
        let p = &mut state.world_builder.preview;
        p.shown = Some(sig);
        p.sharp_done = true;
        return;
    };
    let res = req.settings.heightmap_resolution;
    req.preview_resolution = if sharp { SHARP } else { COARSE };
    // Small terrains: the coarse pass already has every sample.
    let sharp_needed = res > COARSE;
    let p = &mut state.world_builder.preview;
    let cache = if sharp {
        p.sharp_cache.clone()
    } else {
        p.coarse_cache.clone()
    };
    let colors = p.colors.clone();
    let mode = p.mode;
    let progress = Arc::new(Progress::default());
    let shared = progress.clone();
    let terrain_data = req.settings.terrain_data.clone();
    let handle = std::thread::spawn(move || {
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        let out = generate::run_cached(&req, &shared, &mut cache)?;
        let image = render(&out.data, &out.wet, mode, &colors, &root);
        Some(PassResult {
            data: out.data,
            wet: out.wet,
            image,
        })
    });
    p.job = Some(PreviewJob {
        handle,
        progress,
        signature: sig,
        sharp: sharp || !sharp_needed,
        builder,
        terrain_data,
        started: Instant::now(),
    });
}

/// Render an image, filling the layer colour cache from the project's textures.
fn render(
    data: &TerrainData,
    wet: &WaterMap,
    mode: PreviewMode,
    colors: &Mutex<HashMap<String, [f32; 3]>>,
    root: &std::path::Path,
) -> PreviewImage {
    let layer_colors: Vec<[f32; 3]> = data
        .layers
        .iter()
        .map(|l| {
            let key = l.diffuse.clone().unwrap_or_default();
            if let Some(c) = colors.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
                return *c;
            }
            let c = image::layer_color(l, &|p| {
                misty_engine::image::open(root.join(p))
                    .ok()
                    .map(|i| i.thumbnail(32, 32).into_rgba8())
            });
            colors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key, c);
            c
        })
        .collect();
    image::render(data, Some(wet), mode, IMAGE, &|i| {
        layer_colors.get(i).copied().unwrap_or([0.5; 3])
    })
}

/// Pick up a finished pass: show it, and push it into the terrain when Live in Scene view is on.
fn poll(state: &mut EditorState, ctx: &egui::Context) {
    let p = &mut state.world_builder.preview;
    if !p.job.as_ref().is_some_and(|j| j.handle.is_finished()) {
        return;
    }
    let job = p.job.take().expect("checked");
    let ms = job.started.elapsed().as_secs_f32() * 1000.0;
    let Ok(Some(result)) = job.handle.join() else {
        return;
    };
    let res = result.data.heightmap_resolution;
    p.info = format!(
        "{res}² · {ms:.0} ms{}",
        if job.sharp { "" } else { " · refining…" }
    );
    p.shown = Some(job.signature.clone());
    p.sharp_done = job.sharp;
    set_texture(p, ctx, &result.image);
    p.texture_mode = Some(p.mode);
    let live = state
        .scene
        .world
        .get::<&WorldBuilder>(job.builder)
        .is_ok_and(|s| s.live_preview);
    let data = result.data;
    state.world_builder.preview.last = Some(Arc::new((data.clone(), result.wet)));
    if live && !state.world_builder.busy() && !state.play.in_play_mode() {
        push_to_scene(state, &job.terrain_data, data);
    }
}

/// Live in Scene view: show a preview result on the terrain (Revert / undo go back to the data
/// from before the first preview).
fn push_to_scene(state: &mut EditorState, path: &str, data: TerrainData) {
    if path.is_empty() {
        return;
    }
    if state.world_builder.preview_base.is_none()
        && let Some((d, _)) = store::get(path)
    {
        state.world_builder.preview_base = Some(TerrainSnapshot {
            path: path.to_owned(),
            data: (*d).clone(),
        });
    }
    store::insert(path, data);
}

fn set_texture(p: &mut PreviewState, ctx: &egui::Context, img: &PreviewImage) {
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [img.size, img.size],
        &img.rgba.iter().flatten().copied().collect::<Vec<u8>>(),
    );
    match &mut p.texture {
        Some(t) => t.set(color, egui::TextureOptions::LINEAR),
        None => {
            p.texture =
                Some(ctx.load_texture("world_builder_preview", color, egui::TextureOptions::LINEAR))
        }
    }
}

/// The Preview card body: the image, the mode switch and Live in Scene view.
pub fn show(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    // Re-render the last result when the mode changed.
    {
        let p = &mut state.world_builder.preview;
        if p.texture_mode != Some(p.mode)
            && let Some(last) = p.last.clone()
        {
            let img = render(&last.0, &last.1, p.mode, &p.colors, &state.project.root);
            set_texture(p, ui.ctx(), &img);
            p.texture_mode = Some(p.mode);
        }
    }
    let side = ui.available_width().clamp(120.0, 320.0);
    let p = &state.world_builder.preview;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    match &p.texture {
        Some(t) => {
            painter.image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if p.busy() {
                    "Generating preview…"
                } else {
                    "No preview"
                },
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
    if !p.info.is_empty() || p.busy() {
        let text = if p.busy() && p.info.is_empty() {
            "…".to_owned()
        } else {
            p.info.clone()
        };
        let pos = rect.left_bottom() + egui::vec2(6.0, -6.0);
        let galley = painter.layout_no_wrap(
            text,
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(235),
        );
        let bg = egui::Rect::from_min_size(
            pos - egui::vec2(3.0, galley.size().y + 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, 3.0, egui::Color32::from_black_alpha(140));
        painter.galley(
            pos - egui::vec2(0.0, galley.size().y),
            galley,
            egui::Color32::WHITE,
        );
    }
    ui.horizontal(|ui| {
        let p = &mut state.world_builder.preview;
        for m in PreviewMode::ALL {
            if ui.selectable_label(p.mode == m, m.label()).clicked() {
                p.mode = m;
            }
        }
        ui.separator();
        let mut live = state
            .scene
            .world
            .get::<&WorldBuilder>(builder)
            .is_ok_and(|s| s.live_preview);
        if ui
            .checkbox(&mut live, "Live in Scene view")
            .on_hover_text(
                "Also show the low-resolution preview on the terrain while you edit. \
                 Generate replaces it with the full result.",
            )
            .changed()
        {
            super::with_settings(state, builder, |s| {
                s.live_preview = live;
                true
            });
            if !live {
                super::job::revert_preview(state);
            } else if let Some(last) = state.world_builder.preview.last.clone()
                && let Ok(path) = state
                    .scene
                    .world
                    .get::<&WorldBuilder>(builder)
                    .map(|s| s.terrain_data.clone())
            {
                push_to_scene(state, &path, last.0.clone());
            }
        }
    });
    if state.world_builder.preview_base.is_some() && !state.world_builder.busy() {
        ui.horizontal(|ui| {
            ui.label(RichText::new("The terrain shows a preview.").weak().small());
            if ui.small_button("Revert").clicked() {
                super::job::revert_preview(state);
            }
        });
    }
}

/// Wait for the running pass (tests).
#[cfg(test)]
pub fn finish(state: &mut EditorState, ctx: &egui::Context) {
    while state.world_builder.preview.job.is_some() {
        if state
            .world_builder
            .preview
            .job
            .as_ref()
            .is_some_and(|j| j.handle.is_finished())
        {
            poll(state, ctx);
        } else {
            std::thread::yield_now();
        }
    }
}

/// Whether the shown image comes from the sharp pass (tests).
#[cfg(test)]
pub fn sharp(state: &EditorState) -> bool {
    state.world_builder.preview.sharp_done
}

/// The last preview result (tests).
#[cfg(test)]
pub fn last_data(state: &EditorState) -> Option<TerrainData> {
    state
        .world_builder
        .preview
        .last
        .as_ref()
        .map(|l| l.0.clone())
}
