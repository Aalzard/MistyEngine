//! Lighting window ▸ Environment ▸ Skybox = HDRI: the image slot (pick or drag from the
//! Project window), Rotation, Exposure, Tint and "Align Sun to HDRI", plus dropping an HDRI
//! onto the Scene view to make it the sky.

use egui::Ui;
use misty_engine::hdri;
use misty_engine::scene::render_settings::{HdriSettings, SkyboxKind};
use misty_engine::scene::{Light, LightType, Scene};

use super::AssetPayload;
use super::lighting::{color, row};
use crate::state::EditorState;

/// What the HDRI rows need from the editor.
pub(super) struct HdriContext {
    /// Project HDR images.
    pub files: Vec<String>,
    /// Tonemapped preview of the current image.
    pub thumbnail: Option<egui::TextureHandle>,
    /// Why the current image could not be loaded.
    pub error: Option<String>,
    /// A Directional Light exists (the sun button needs one).
    pub has_sun: bool,
}

/// Edits of one frame.
#[derive(Default)]
pub(super) struct HdriEdit {
    pub changed: bool,
    /// "Align Sun to HDRI" was clicked.
    pub align_sun: bool,
}

fn name(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The editor side of the rows: file list, preview and load status of `rel`.
pub(super) fn context(ui: &Ui, state: &mut EditorState, has_sun: bool) -> HdriContext {
    let rel = state.scene.render_settings.skybox.hdri.texture.clone();
    let thumbnail = rel.as_ref().and_then(|r| {
        let abs = state.assets.absolute(r);
        state.project.thumbnails.get(ui.ctx(), &abs)
    });
    let error = rel.as_deref().and_then(|r| {
        hdri::store::load(r)
            .is_none()
            .then(|| hdri::store::error(r).unwrap_or_else(|| format!("{r} not found")))
    });
    HdriContext {
        files: state.assets.hdris.clone(),
        thumbnail,
        error,
        has_sun,
    }
}

/// Grid rows of the HDRI skybox.
pub(super) fn rows(ui: &mut Ui, h: &mut HdriSettings, cx: &HdriContext) -> HdriEdit {
    let mut edit = HdriEdit::default();
    edit.changed |= row(ui, "HDRI", |ui| slot(ui, h, cx));
    if let Some(thumb) = &cx.thumbnail {
        ui.label("");
        ui.image((thumb.id(), egui::vec2(160.0, 80.0)));
        ui.end_row();
    }
    if let Some(e) = &cx.error {
        ui.label("");
        ui.colored_label(crate::theme::AXIS_X, e);
        ui.end_row();
    }
    edit.changed |= row(ui, "Rotation", |ui| {
        ui.add(egui::Slider::new(&mut h.rotation, 0.0..=360.0).suffix("°"))
            .changed()
    });
    edit.changed |= row(ui, "Exposure", |ui| {
        ui.add(egui::Slider::new(&mut h.exposure, -10.0..=10.0).suffix(" EV"))
            .changed()
    });
    edit.changed |= row(ui, "Tint", |ui| color(ui, &mut h.tint));
    let can_align = cx.has_sun && h.texture.is_some() && cx.error.is_none();
    row(ui, "Sun", |ui| {
        edit.align_sun = ui
            .add_enabled(can_align, egui::Button::new("Align Sun to HDRI"))
            .on_hover_text("Point the brightest Directional Light from the HDRI's brightest spot")
            .on_disabled_hover_text("Needs a loaded HDRI and a Directional Light")
            .clicked();
        false
    });
    edit
}

/// Image picker: project HDR images, or one dragged from the Project window.
fn slot(ui: &mut Ui, h: &mut HdriSettings, cx: &HdriContext) -> bool {
    let mut ch = false;
    let current = h.texture.as_deref().map(name);
    let slot = egui::ComboBox::from_id_salt("hdri_texture")
        .selected_text(current.unwrap_or_else(|| "None (HDR Texture)".into()))
        .width(160.0)
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(&mut h.texture, None, "None (HDR Texture)")
                .changed();
            for f in &cx.files {
                ch |= ui
                    .selectable_value(&mut h.texture, Some(f.clone()), name(f))
                    .on_hover_text(f)
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
        && hdri::is_hdri(&p.0)
    {
        h.texture = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// The brightest enabled Directional Light.
fn main_sun(scene: &Scene) -> Option<misty_engine::hecs::Entity> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let l = scene.world.get::<&Light>(e).ok()?;
            (l.enabled && l.kind == LightType::Directional).then_some((e, l.intensity))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

/// Rotate the main Directional Light to shine from the HDRI's brightest point (undoable).
pub(super) fn align_sun(state: &mut EditorState) {
    let h = &state.scene.render_settings.skybox.hdri;
    let (Some(rel), Some(sun)) = (h.texture.clone(), main_sun(&state.scene)) else {
        return;
    };
    let Some(sky) = hdri::store::load(&rel) else {
        return;
    };
    let towards = sky.0.sun_direction(h.rotation);
    // World rotation, expressed in the parent's space.
    let world = misty_engine::camera::euler_to_quat(hdri::sun_euler(towards));
    let parent = state
        .scene
        .parent(sun)
        .map(|p| {
            state
                .scene
                .world_matrix(p)
                .to_scale_rotation_translation()
                .1
        })
        .unwrap_or_default();
    let mut t = state.scene.transform(sun);
    t.set_euler(misty_engine::camera::quat_to_euler(
        parent.inverse() * world,
    ));
    state.scene.set_transform(sun, t);
    state.mark_edited("Align Sun to HDRI");
}

/// An HDRI dropped onto the Scene view becomes the skybox (undoable). True when handled.
pub fn drop_on_scene(state: &mut EditorState, rel: &str) -> bool {
    if !hdri::is_hdri(rel) {
        return false;
    }
    let sky = &mut state.scene.render_settings.skybox;
    sky.kind = SkyboxKind::Hdri;
    sky.hdri.texture = Some(rel.to_owned());
    state.mark_edited("Set HDRI Sky");
    true
}
