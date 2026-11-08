//! Prefab Mode: edit a prefab asset on its own, like a small scene.
//!
//! Opening a prefab (double-click in the Project window, the ▸ arrow on an instance row in the
//! Hierarchy, Open in the Inspector) puts its objects in a scene of their own in place of the open
//! scenes; the Hierarchy, Inspector and Scene view then edit the prefab. The scenes, their undo
//! history, selection and Scene view camera are kept aside and come back on exit. Opening a
//! prefab from inside Prefab Mode goes one level deeper; the breadcrumb bar ("Scenes ▸ Cart ▸
//! Wheel") goes back any number of levels.
//!
//! **Save** writes the prefab and every instance (and every prefab that uses it) follows; with
//! **Auto Save** on, each change is saved once the mouse is released. Leaving with unsaved
//! changes asks Save / Discard / Cancel in the bar. Opened from an instance, the prefab is
//! shown **in context**: the scene around the instance is drawn dimmed behind it.

use std::collections::HashSet;
use std::time::Instant;

use misty_engine::hecs::Entity;
use misty_engine::render::MeshDraw;
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{Scene, SceneId};

use crate::actions::Action;
use crate::scene_camera::SceneCamera;
use crate::selection::Selection;
use crate::state::EditorState;
use crate::undo::UndoStack;

/// One open level of Prefab Mode.
pub struct PrefabStage {
    /// The prefab asset (project-relative).
    pub path: String,
    /// The prefab's root in the stage scene.
    pub root: Entity,
    pub auto_save: bool,
    /// Draw the scene underneath, dimmed, around the prefab.
    pub in_context: bool,
    /// The instance (in the scene underneath) it was opened from.
    pub context: Option<Entity>,
    /// Waiting for Save / Discard / Cancel before leaving this many levels.
    pub confirm: Option<usize>,
    /// When the stage last became changed (Auto Save waits for the edit to settle).
    changed_at: Option<Instant>,
    /// The context scene's draws (dimmed, moved so the instance lines up with the prefab), and
    /// the prefab root's placement they were made for.
    context_draws: Option<(glam::Mat4, std::sync::Arc<Vec<MeshDraw>>)>,
    stash: Stash,
}

/// What a stage replaced while it is open.
struct Stash {
    scene: Scene,
    undo: UndoStack,
    selection: Selection,
    scene_dirty: bool,
    expanded: HashSet<Entity>,
    collapsed: HashSet<SceneId>,
    inspector_targets: Vec<Entity>,
    inspector_asset: Option<String>,
    camera: SceneCamera,
    saved: std::collections::HashMap<SceneId, u64>,
    dirty: Vec<SceneId>,
}

/// Prefab Mode is open.
pub fn active(s: &EditorState) -> bool {
    !s.prefab_stages.is_empty()
}

/// The prefab being edited, if any.
pub fn open_path(s: &EditorState) -> Option<String> {
    s.prefab_stages.last().map(|st| st.path.clone())
}

/// Scenes kept aside by Prefab Mode (they follow prefab changes too).
pub fn stashed_scenes(s: &mut EditorState) -> impl Iterator<Item = &mut Scene> {
    s.prefab_stages.iter_mut().map(|st| &mut st.stash.scene)
}

pub fn apply(s: &mut EditorState, action: Action) {
    match action {
        Action::OpenPrefab { path, from } => {
            open(s, &path, from);
        }
        Action::ClosePrefab { levels } => request_close(s, levels),
        Action::SavePrefabStage => {
            save(s);
        }
        Action::PrefabModeAnswer(save_first) => answer(s, Some(save_first)),
        _ => {}
    }
}

/// Open the prefab at `rel` in Prefab Mode (`from`: the instance it was opened from).
pub fn open(s: &mut EditorState, rel: &str, from: Option<Entity>) -> bool {
    if s.play.in_play_mode() {
        s.console.warn("Prefab Mode isn't available in Play mode.");
        return false;
    }
    if crate::prefabs::read_only(rel) {
        s.console.warn(format!(
            "{} is a model file: make a Prefab Variant of it to edit it.",
            crate::prefabs::display_name(rel)
        ));
        return false;
    }
    if s.prefab_stages.iter().any(|st| st.path == rel) {
        s.console.warn(format!(
            "{} is already open in Prefab Mode.",
            crate::prefabs::display_name(rel)
        ));
        return false;
    }
    let prefab = match s.assets.prefab(rel) {
        Ok(p) => (*p).clone().with_ids(),
        Err(err) => {
            s.console.warn(format!("Could not open prefab: {err}"));
            return false;
        }
    };
    crate::panels::animation::preview::stop_preview(s);
    crate::panels::timeline::preview::stop_preview(s);
    let context = from.filter(|e| s.scene.contains(*e));
    let mut scene = primed_scene(&crate::prefabs::display_name(rel), prefab.outline().len());
    let root = prefab.spawn_into(&mut scene);
    let stash = Stash {
        scene: std::mem::replace(&mut s.scene, scene),
        undo: std::mem::take(&mut s.undo),
        selection: std::mem::take(&mut s.selection),
        scene_dirty: std::mem::replace(&mut s.scene_dirty, false),
        expanded: std::mem::take(&mut s.hierarchy.expanded),
        collapsed: std::mem::take(&mut s.hierarchy.collapsed_scenes),
        inspector_targets: std::mem::take(&mut s.inspector_targets),
        inspector_asset: s.inspector_asset.take(),
        camera: std::mem::take(&mut s.scene_view.camera),
        saved: std::mem::take(&mut s.files.saved),
        dirty: std::mem::take(&mut s.files.dirty),
    };
    s.scene_view.camera.settings = stash.camera.settings;
    s.hierarchy.renaming = None;
    s.hierarchy.expanded.insert(root);
    s.prefab_stages.push(PrefabStage {
        path: rel.to_owned(),
        root,
        auto_save: false,
        in_context: context.is_some(),
        context,
        confirm: None,
        changed_at: None,
        context_draws: None,
        stash,
    });
    // Frame the whole prefab (wider in context, so the scene around it shows).
    s.selection.set_many(s.scene.subtree(root));
    if let Some((center, radius)) = crate::commands::selection_bounds(s) {
        let radius = if context.is_some() {
            radius.max(1.0) * 3.0
        } else {
            radius.max(0.25)
        };
        s.scene_view.camera.frame(center, radius);
    }
    s.select(root);
    s.console.info(format!("Opened {rel} in Prefab Mode"));
    true
}

/// A new scene whose objects get ids no scene underneath uses: panels that remember an object
/// of the scenes (the Animation window's, a Timeline's director…) never land on a stage object.
fn primed_scene(name: &str, objects: usize) -> Scene {
    const GENERATION: u64 = 0x5EED_0000;
    let mut scene = Scene::new(name);
    let reserved: Vec<Entity> = (0..objects as u64 + 1024)
        .filter_map(|i| Entity::from_bits(GENERATION << 32 | i))
        .collect();
    for e in &reserved {
        scene.world.spawn_at(*e, ());
    }
    for e in reserved {
        let _ = scene.world.despawn(e);
    }
    scene
}

/// The prefab as it stands in the stage (what Save writes).
fn current(s: &EditorState, st: &PrefabStage) -> Prefab {
    Prefab::from_scene_keep_base(&s.scene, st.root).with_ids()
}

/// Save the deepest open prefab. Returns false when it couldn't be written.
pub fn save(s: &mut EditorState) -> bool {
    let Some(st) = s.prefab_stages.last() else {
        return false;
    };
    let (rel, root) = (st.path.clone(), st.root);
    if !s.scene.contains(root) {
        s.console
            .warn("The prefab's root object was deleted: undo, then save.");
        return false;
    }
    let new = current(s, st);
    let old = match s.assets.prefab(&rel) {
        Ok(p) => (*p).clone(),
        Err(_) => new.clone(),
    };
    match crate::prefabs::save_changed(s, &rel, &old, &new) {
        Ok(_) => {
            // New objects keep the ids they were saved with, even after a rename.
            s.scene.store_prefab_ids(root);
            s.scene_dirty = false;
            if let Some(st) = s.prefab_stages.last_mut() {
                st.changed_at = None;
                st.context_draws = None;
            }
            s.console.info(format!("Saved {rel}"));
            true
        }
        Err(err) => {
            s.console.warn(format!("Could not save prefab: {err}"));
            false
        }
    }
}

/// Back arrow / breadcrumb: leave `levels` levels, asking first about unsaved changes.
pub fn request_close(s: &mut EditorState, levels: usize) {
    for left in (1..=levels.min(s.prefab_stages.len())).rev() {
        let Some(st) = s.prefab_stages.last() else {
            return;
        };
        if s.scene_dirty {
            if st.auto_save {
                if !save(s) {
                    return;
                }
            } else {
                if let Some(st) = s.prefab_stages.last_mut() {
                    st.confirm = Some(left);
                }
                return;
            }
        }
        close(s);
    }
}

/// The bar's answer to "Save changes?": `Some(true)` Save, `Some(false)` Discard, `None`
/// Cancel.
pub fn answer(s: &mut EditorState, save_first: Option<bool>) {
    let Some(levels) = s.prefab_stages.last_mut().and_then(|st| st.confirm.take()) else {
        return;
    };
    match save_first {
        None => return,
        Some(true) => {
            if !save(s) {
                return;
            }
        }
        Some(false) => s.scene_dirty = false,
    }
    close(s);
    if levels > 1 {
        request_close(s, levels - 1);
    }
}

/// Leave the deepest level without saving, putting back what it replaced.
pub fn close(s: &mut EditorState) {
    let Some(st) = s.prefab_stages.pop() else {
        return;
    };
    let path = st.path.clone();
    let x = st.stash;
    s.scene = x.scene;
    s.undo = x.undo;
    s.selection = x.selection;
    s.scene_dirty = x.scene_dirty;
    s.hierarchy.expanded = x.expanded;
    s.hierarchy.collapsed_scenes = x.collapsed;
    s.hierarchy.renaming = None;
    s.inspector_targets = x.inspector_targets;
    s.inspector_asset = x.inspector_asset;
    s.scene_view.camera = x.camera;
    s.files.saved = x.saved;
    s.files.dirty = x.dirty;
    s.selection.retain_existing(&s.scene);
    // Prefab saves may have changed instances underneath.
    s.scene_draws = Default::default();
    s.console.info(format!("Closed Prefab Mode ({path})"));
}

/// Leave Prefab Mode completely before something replaces the scenes (open a scene, Play, quit).
/// Unsaved changes are saved with Auto Save on; otherwise the bar asks and this returns false.
pub fn leave_for_scene_change(s: &mut EditorState) -> bool {
    if !active(s) {
        return true;
    }
    request_close(s, usize::MAX);
    if active(s) {
        s.console
            .warn("Save or discard the prefab's changes first (Prefab Mode bar).");
        return false;
    }
    true
}

/// Once a frame: Auto Save when an edit has settled.
pub fn update(s: &mut EditorState, ctx: &egui::Context) {
    let dirty = s.scene_dirty;
    let Some(st) = s.prefab_stages.last_mut() else {
        return;
    };
    if !dirty {
        st.changed_at = None;
        return;
    }
    let since = *st.changed_at.get_or_insert_with(Instant::now);
    let pointer_down = ctx.input(|i| i.pointer.any_down());
    if st.auto_save && !pointer_down && since.elapsed().as_secs_f32() > 0.4 {
        save(s);
    }
}

/// The scene underneath, dimmed, for in-context editing: its draws with the instance the
/// prefab was opened from left out, moved so the instance stands where the prefab root is.
pub fn context_draws(s: &mut EditorState) -> Option<std::sync::Arc<Vec<MeshDraw>>> {
    let root = s.prefab_stages.last()?.root;
    let root_world = s.scene.contains(root).then(|| s.scene.world_matrix(root))?;
    let assets = &s.assets;
    let st = s.prefab_stages.last_mut()?;
    if !st.in_context {
        return None;
    }
    if let Some((at, d)) = &st.context_draws
        && *at == root_world
    {
        return Some(d.clone());
    }
    // A copy of the scene without the instance (the stage shows it).
    let mut scene = st.stash.scene.clone();
    let offset = match st.context.filter(|c| scene.contains(*c)) {
        Some(c) => {
            let m = root_world * scene.world_matrix(c).inverse();
            scene.despawn(c);
            m
        }
        None => glam::Mat4::IDENTITY,
    };
    let mut cache = misty_engine::draw_cache::DrawCache::default();
    let all = cache.update(&scene, assets, u32::MAX, |_| false);
    let draws: Vec<MeshDraw> = all
        .iter()
        .map(|d| {
            let mut d = d.clone();
            d.model = offset * d.model;
            dim(&mut d.material);
            d.outline = false;
            d.cast_shadows = false;
            d
        })
        .collect();
    let draws = std::sync::Arc::new(draws);
    st.context_draws = Some((root_world, draws.clone()));
    Some(draws)
}

/// In context: the lights and environment of the scene underneath, moved like its draws.
pub fn context_lighting(
    s: &EditorState,
) -> Option<(
    Vec<misty_engine::render::LightData>,
    misty_engine::render::Environment,
)> {
    let st = s.prefab_stages.last().filter(|st| st.in_context)?;
    let root = s
        .scene
        .contains(st.root)
        .then(|| s.scene.world_matrix(st.root))?;
    let scene = &st.stash.scene;
    let offset = st
        .context
        .filter(|c| scene.contains(*c))
        .map(|c| root * scene.world_matrix(c).inverse())
        .unwrap_or(glam::Mat4::IDENTITY);
    let mut lights = misty_engine::render_scene::lights(scene);
    for l in &mut lights {
        l.position = offset.transform_point3(l.position);
        l.direction = offset.transform_vector3(l.direction).normalize_or_zero();
    }
    let env = misty_engine::render_scene::environment(scene, &lights);
    Some((lights, env))
}

/// Grey and darken a context material (the familiar greyed-out scene behind a prefab).
fn dim(m: &mut misty_engine::render::MaterialParams) {
    let [r, g, b, a] = m.color;
    let grey = (r * 0.3 + g * 0.59 + b * 0.11) * 0.45 + 0.08;
    m.color = [grey, grey, grey * 1.05, a];
    m.emission_intensity *= 0.2;
    m.albedo_map = None;
    m.custom = None;
}

/// Background of the Prefab Mode bar (a blue-grey band, the prefab colour family).
pub const BAR_FILL: egui::Color32 = egui::Color32::from_rgb(38, 52, 74);

/// The bar above the Scene view in Prefab Mode: ◀ back, the breadcrumb ("Scenes ▸ Cart ▸
/// Wheel"), In Context, Auto Save and Save; or the Save / Discard / Cancel question.
pub fn bar(ui: &mut egui::Ui, s: &mut EditorState) {
    let Some(st) = s.prefab_stages.last() else {
        return;
    };
    let (confirm, auto_save, in_context, has_context) = (
        st.confirm,
        st.auto_save,
        st.in_context,
        st.context.is_some(),
    );
    let name = crate::prefabs::display_name(&st.path);
    let crumbs: Vec<String> = s
        .prefab_stages
        .iter()
        .map(|st| crate::prefabs::display_name(&st.path))
        .collect();
    let dirty = s.scene_dirty;
    egui::Frame::new()
        .fill(BAR_FILL)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if confirm.is_some() {
                    let text = format!("Save changes to {name}?");
                    ui.label(
                        egui::RichText::new(text)
                            .strong()
                            .color(crate::theme::PREFAB),
                    );
                    if ui.button("Save").clicked() {
                        s.push(Action::PrefabModeAnswer(true));
                    }
                    if ui
                        .button("Discard")
                        .on_hover_text("Leave without saving")
                        .clicked()
                    {
                        s.push(Action::PrefabModeAnswer(false));
                    }
                    if ui.button("Cancel").clicked() {
                        answer(s, None);
                    }
                    return;
                }
                if ui
                    .button("◀")
                    .on_hover_text("Back: leave this prefab")
                    .clicked()
                {
                    s.push(Action::ClosePrefab { levels: 1 });
                }
                if ui
                    .link("Scenes")
                    .on_hover_text("Back to the scenes")
                    .clicked()
                {
                    s.push(Action::ClosePrefab { levels: usize::MAX });
                }
                let last = crumbs.len() - 1;
                for (i, crumb) in crumbs.iter().enumerate() {
                    ui.label("▸");
                    let icon = crate::state::PREFAB_ICON;
                    if i == last {
                        let star = if dirty { "*" } else { "" };
                        let text = egui::RichText::new(format!("{icon} {crumb}{star}"))
                            .strong()
                            .color(crate::theme::PREFAB);
                        ui.label(text);
                    } else if ui.link(format!("{icon} {crumb}")).clicked() {
                        s.push(Action::ClosePrefab { levels: last - i });
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui
                        .add_enabled(dirty, egui::Button::new("Save"))
                        .on_hover_text("Write the prefab; its instances follow (Ctrl+S)");
                    if save.clicked() {
                        s.push(Action::SavePrefabStage);
                    }
                    let mut auto = auto_save;
                    if ui
                        .checkbox(&mut auto, "Auto Save")
                        .on_hover_text("Save each change as soon as it is made")
                        .changed()
                        && let Some(st) = s.prefab_stages.last_mut()
                    {
                        st.auto_save = auto;
                    }
                    if has_context {
                        let mut ctx = in_context;
                        if ui
                            .checkbox(&mut ctx, "In Context")
                            .on_hover_text("Show the scene around the instance, dimmed")
                            .changed()
                            && let Some(st) = s.prefab_stages.last_mut()
                        {
                            st.in_context = ctx;
                            st.context_draws = None;
                        }
                    }
                });
            });
        });
}

/// The Hierarchy's first row in Prefab Mode: ◀ and the prefab's name (in place of the scene
/// headers).
pub fn hierarchy_header(ui: &mut egui::Ui, s: &mut EditorState) {
    let Some(st) = s.prefab_stages.last() else {
        return;
    };
    let name = crate::prefabs::display_name(&st.path);
    let variant = s.scene.prefab_link(st.root).is_some();
    let star = if s.scene_dirty { "*" } else { "" };
    egui::Frame::new()
        .fill(BAR_FILL)
        .inner_margin(egui::Margin::symmetric(4, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if ui
                    .small_button("◀")
                    .on_hover_text("Back: leave this prefab")
                    .clicked()
                {
                    s.push(Action::ClosePrefab { levels: 1 });
                }
                let kind = if variant { "Prefab Variant" } else { "Prefab" };
                let text = format!("{} {name}{star}", crate::state::PREFAB_ICON);
                ui.label(
                    egui::RichText::new(text)
                        .strong()
                        .color(crate::theme::PREFAB),
                )
                .on_hover_text(kind);
            });
        });
}
