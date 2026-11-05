//! Templates: the World Builder's one-click presets (Desert, Forest, Mountains, …). The picker
//! card shows a thumbnail per template (committed PNGs, rendered by
//! `write_template_thumbnails`); applying one replaces the builder's settings and stamps as one
//! undo step, asking first when the current setup has been edited. GameObject ▸ 3D Object ▸
//! World makes a terrain and World Builder from a template in one go.

use egui::{RichText, Ui};
use glam::{Mat4, Quat};
use misty_engine::hecs::Entity;
use misty_engine::world_gen::templates::Template;
use misty_engine::world_gen::{self, WorldBuilder, WorldStamp};

use crate::state::EditorState;

/// Thumbnail side in pixels (the committed PNGs).
#[cfg(test)]
pub const THUMB: u32 = 96;

/// Committed thumbnail PNG of a template.
pub fn thumbnail_png(t: Template) -> &'static [u8] {
    match t {
        Template::Desert => include_bytes!("../../../assets/world_templates/Desert.png"),
        Template::Forest => include_bytes!("../../../assets/world_templates/Forest.png"),
        Template::Mountains => include_bytes!("../../../assets/world_templates/Mountains.png"),
        Template::Plains => include_bytes!("../../../assets/world_templates/Plains.png"),
        Template::Islands => include_bytes!("../../../assets/world_templates/Islands.png"),
        Template::Canyon => include_bytes!("../../../assets/world_templates/Canyon.png"),
        Template::Tundra => include_bytes!("../../../assets/world_templates/Tundra.png"),
        Template::Volcanic => include_bytes!("../../../assets/world_templates/Volcanic.png"),
    }
}

/// The thumbnail as an egui texture (decoded once).
fn texture(ctx: &egui::Context, t: Template) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("world_template_thumb", t.label()));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let img = misty_engine::image::load_from_memory(thumbnail_png(t))
        .ok()?
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let tex = ctx.load_texture(
        format!("world_template_{}", t.label()),
        color,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// The template grid. Clicking one applies it to `builder` (asking first when edited), or
/// creates a terrain and World Builder from it when there is none.
pub fn picker(ui: &mut Ui, state: &mut EditorState, builder: Option<Entity>) {
    let width = ui.available_width();
    let cell = 84.0f32;
    let per_row = ((width + 6.0) / (cell + 6.0)).floor().max(1.0) as usize;
    let mut clicked = None;
    egui::Grid::new("wb_templates")
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            for (i, t) in Template::ALL.into_iter().enumerate() {
                if i > 0 && i % per_row == 0 {
                    ui.end_row();
                }
                if tile(ui, t, cell) {
                    clicked = Some(t);
                }
            }
        });
    let Some(t) = clicked else {
        return;
    };
    match builder {
        Some(b) if is_edited(state, b) => state.world_builder.confirm_template = Some(t),
        Some(b) => apply(state, b, t, true),
        None => {
            create_world(state, t, None, false);
        }
    }
}

/// One thumbnail button with its name under it. True when clicked.
fn tile(ui: &mut Ui, t: Template, cell: f32) -> bool {
    let tex = texture(ui.ctx(), t);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell + 18.0), egui::Sense::click());
    let resp = resp
        .on_hover_text(t.description())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(cell, cell));
    let painter = ui.painter_at(rect);
    if let Some(tex) = tex {
        painter.image(
            tex.id(),
            img_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    let stroke = if resp.hovered() {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    painter.rect_stroke(img_rect, 4.0, stroke, egui::StrokeKind::Inside);
    painter.text(
        egui::pos2(rect.center().x, img_rect.bottom() + 9.0),
        egui::Align2::CENTER_CENTER,
        t.label(),
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    // Accessible name ("Desert template") for tests and screen readers.
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{} template", t.label()),
        )
    });
    resp.clicked()
}

/// "Replace the current setup?" dialog for a pending template.
pub fn confirm_dialog(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    let Some(t) = state.world_builder.confirm_template else {
        return;
    };
    let stamps = world_gen::stamps_of(&state.scene, builder).len();
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("wb_template_confirm")).show(ui.ctx(), |ui| {
        ui.set_width(360.0);
        ui.heading(format!("Apply the {} template?", t.label()));
        ui.add_space(4.0);
        ui.label(format!(
            "This replaces the World Builder's settings and its {stamps} stamp{}. \
             Undo brings them back.",
            if stamps == 1 { "" } else { "s" }
        ));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Replace").strong()).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    if let Some(yes) = answer {
        state.world_builder.confirm_template = None;
        if yes {
            apply(state, builder, t, true);
        }
    }
}

/// The settings with what templates keep (terrain asset, width / length, resolutions, storage,
/// Live in Scene view) taken from `keep`, so two setups compare on what a template sets.
fn normalized(s: &WorldBuilder, keep: &WorldBuilder) -> WorldBuilder {
    WorldBuilder {
        terrain_data: keep.terrain_data.clone(),
        size: [keep.size[0], s.size[1], keep.size[2]],
        heightmap_resolution: keep.heightmap_resolution,
        alphamap_resolution: keep.alphamap_resolution,
        live_preview: keep.live_preview,
        scatter_storage: keep.scatter_storage,
        ..s.clone()
    }
}

/// The builder has work a template would overwrite: stamps or settings that are neither the
/// defaults nor some template's unchanged setup.
pub fn is_edited(state: &EditorState, builder: Entity) -> bool {
    let scene = &state.scene;
    let Ok(s) = scene
        .world
        .get::<&WorldBuilder>(builder)
        .map(|s| (*s).clone())
    else {
        return false;
    };
    let stamps: Vec<WorldStamp> = world_gen::stamps_of(scene, builder)
        .into_iter()
        .filter_map(|e| scene.world.get::<&WorldStamp>(e).ok().map(|w| (*w).clone()))
        .collect();
    let default = normalized(&WorldBuilder::default(), &s);
    if stamps.is_empty() && normalized(&s, &s) == default {
        return false;
    }
    !Template::ALL.into_iter().any(|t| {
        let setup = t.setup(&s);
        normalized(&setup.settings, &s) == normalized(&s, &s)
            && setup.stamps.len() == stamps.len()
            && setup.stamps.iter().zip(&stamps).all(|(a, b)| a.stamp == *b)
    })
}

/// Replace `builder`'s settings and stamps with the template's (and drop what the last
/// Generate made). `record` = make it its own undo step.
pub fn apply(state: &mut EditorState, builder: Entity, t: Template, record: bool) {
    let root = state.project.root.clone();
    if let Err(e) = super::biome_ui::ensure_presets(&root) {
        state.console.warn(format!("World Builder presets: {e}"));
    }
    let Ok(current) = state
        .scene
        .world
        .get::<&WorldBuilder>(builder)
        .map(|s| (*s).clone())
    else {
        return;
    };
    if record {
        state.record(&format!("Apply {} Template", t.label()));
    }
    let setup = t.setup(&current);
    for e in world_gen::stamps_of(&state.scene, builder) {
        if state.scene.contains(e) {
            state.scene.despawn(e);
        }
    }
    if let Some(g) = state
        .scene
        .children(builder)
        .into_iter()
        .find(|c| state.scene.name(*c) == world_gen::GENERATED)
    {
        state.scene.despawn(g);
    }
    let _ = state.scene.world.insert_one(builder, setup.settings);
    let corner = world_gen::terrain_of(&state.scene, builder)
        .map(|t| state.scene.world_position(t))
        .unwrap_or_default();
    for st in setup.stamps {
        let e = state.scene.spawn(st.name, Some(builder));
        world_gen::set_world_matrix(
            &mut state.scene,
            e,
            Mat4::from_rotation_translation(
                Quat::from_rotation_y(st.yaw.to_radians()),
                corner + st.position,
            ),
        );
        state.scene.add(e, st.stamp);
    }
    state.scene_dirty = true;
    state.refresh_assets();
    state.world_builder.status = format!(
        "{} template applied. Press Generate to build it.",
        t.label()
    );
}

/// GameObject ▸ 3D Object ▸ World ▸ <template>: a new terrain and World Builder set up from
/// the template (one undo step), selected, with the World Builder window opened. `generate`
/// also starts a full generation.
pub fn create_world(
    state: &mut EditorState,
    t: Template,
    parent: Option<Entity>,
    generate: bool,
) -> Option<Entity> {
    let name = format!("{} World", t.label());
    let b = super::create_builder_named(state, None, &name)?;
    if let Some(p) = parent {
        state.scene.set_parent(b, Some(p), None, true);
    }
    state.scene.game_object_mut(b).name = name;
    apply(state, b, t, false);
    state.select(b);
    state.world_builder.builder = Some(b);
    state.push(crate::actions::Action::FocusTab(
        crate::layout::Tab::WorldBuilder,
    ));
    if generate {
        super::job::start(state, b, world_gen::Stages::All);
    }
    Some(b)
}
