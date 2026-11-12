//! LOD Group Inspector: the LOD bar (one coloured segment per level from 100% screen height
//! down to Culled, dividers to drag, a marker where the Scene camera is), then the selected
//! level's renderers and the fade settings.

use egui::{Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::render::culling::Aabb;
use misty_engine::render::lod::screen_height;
use misty_engine::scene::{LodFadeMode, LodGroup, LodLevel, MeshFilter, MeshRenderer, Scene};

use crate::i18n::tr;
use crate::state::EditorState;

pub const NAMES: [&str; 1] = ["LOD Group"];

const COLORS: [Color32; 8] = [
    Color32::from_rgb(78, 120, 64),
    Color32::from_rgb(58, 98, 120),
    Color32::from_rgb(54, 76, 128),
    Color32::from_rgb(98, 70, 120),
    Color32::from_rgb(120, 70, 86),
    Color32::from_rgb(124, 92, 60),
    Color32::from_rgb(110, 110, 60),
    Color32::from_rgb(70, 110, 104),
];
const CULLED: Color32 = Color32::from_rgb(110, 40, 40);

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "LOD Group").then_some("▤")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&LodGroup>(e)
        .ok()
        .map(|g| g.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _: &str, on: bool) {
    if let Ok(mut g) = state.scene.world.get::<&mut LodGroup>(e) {
        g.enabled = on;
    }
}

/// Screen height of the group for the Scene view camera (None without renderers).
fn camera_height(state: &EditorState, g: &LodGroup) -> Option<f32> {
    let scene = &state.scene;
    let mut bounds: Option<Aabb> = None;
    for e in g.lods.iter().flat_map(|l| &l.renderers) {
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(*e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let Some((lo, hi)) = misty_engine::models::bounds(&mesh) else {
            continue;
        };
        let b = Aabb::new(lo, hi).transformed(&scene.world_matrix(*e));
        bounds = Some(match bounds {
            Some(a) => Aabb::new(a.min.min(b.min), a.max.max(b.max)),
            None => b,
        });
    }
    let b = bounds?;
    let camera = state.scene_view.camera.camera();
    let size = (b.max - b.min).max_element();
    Some(screen_height(&camera, (b.min + b.max) * 0.5, size, 1.0))
}

/// The bar; returns true when a threshold was dragged. `selected` = the level shown below.
fn lod_bar(ui: &mut Ui, g: &mut LodGroup, selected: &mut usize, camera: Option<f32>) -> bool {
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    let x_of = |h: f32| rect.left() + (1.0 - h.clamp(0.0, 1.0)) * rect.width();
    let painter = ui.painter_at(rect);
    let mut changed = false;
    let n = g.lods.len();
    for i in 0..=n {
        let (hi, lo) = match i {
            _ if i == n => (g.lods.last().map_or(1.0, |l| l.screen_height), 0.0),
            0 => (1.0, g.lods[0].screen_height),
            _ => (g.lods[i - 1].screen_height, g.lods[i].screen_height),
        };
        let seg = Rect::from_x_y_ranges(x_of(hi)..=x_of(lo), rect.y_range());
        let culled = i == n;
        let mut color = if culled {
            CULLED
        } else {
            COLORS[i % COLORS.len()]
        };
        if !culled && i == *selected {
            color = color.gamma_multiply(1.35);
        }
        painter.rect_filled(seg, CornerRadius::ZERO, color);
        let label = match culled {
            true => format!("Culled\n{:.0}%", lo * 100.0),
            false => format!("LOD {i}\n{:.0}%", lo * 100.0),
        };
        if seg.width() > 28.0 {
            painter.text(
                seg.left_top() + vec2(4.0, 3.0),
                egui::Align2::LEFT_TOP,
                label,
                FontId::proportional(10.5),
                Color32::from_gray(235),
            );
        }
        if !culled {
            let click = ui.interact(seg, ui.id().with(("lod_seg", i)), Sense::click());
            if click.clicked() {
                *selected = i;
            }
        }
    }
    // Dividers: drag to move a level's threshold.
    for i in 0..n {
        let x = x_of(g.lods[i].screen_height);
        let handle = Rect::from_center_size(pos2(x, rect.center().y), vec2(8.0, rect.height()));
        let r = ui
            .interact(handle, ui.id().with(("lod_div", i)), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            g.lods[i].screen_height = 1.0 - ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            g.clamp_thresholds();
            changed = true;
        }
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(if r.hovered() { 2.5 } else { 1.5 }, Color32::from_gray(20)),
        );
    }
    if let Some(h) = camera {
        let x = x_of(h);
        painter.line_segment(
            [pos2(x, rect.top() - 2.0), pos2(x, rect.bottom() + 2.0)],
            Stroke::new(2.0, Color32::WHITE),
        );
        painter.text(
            pos2(x + 3.0, rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("📷 {:.0}%", h * 100.0),
            FontId::proportional(10.0),
            Color32::WHITE,
        );
    }
    changed
}

/// Descendants of `root` that have a Mesh Renderer (choices for a level).
fn renderer_choices(scene: &Scene, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = scene.children(root);
    while let Some(e) = stack.pop() {
        if scene.world.get::<&MeshRenderer>(e).is_ok() {
            out.push(e);
        }
        stack.extend(scene.children(e));
    }
    out.sort_by_key(|e| scene.name(*e));
    out
}

pub fn lod_group(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut g) = state.scene.world.get::<&LodGroup>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let id = ui.id().with(("lod_selected", e));
    let mut selected: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    selected = selected.min(g.lods.len().saturating_sub(1));
    let camera = camera_height(state, &g);
    let mut changed = lod_bar(ui, &mut g, &mut selected, camera);

    ui.horizontal(|ui| {
        let add = g.lods.len() < LodGroup::MAX_LEVELS;
        if ui
            .add_enabled(add, egui::Button::new("+ Level").small())
            .clicked()
        {
            let h = g.lods.last().map_or(0.5, |l| l.screen_height * 0.5);
            g.lods.push(LodLevel {
                screen_height: h,
                renderers: Vec::new(),
            });
            selected = g.lods.len() - 1;
            changed = true;
        }
        let remove = g.lods.len() > 1;
        if ui
            .add_enabled(remove, egui::Button::new("− Level").small())
            .clicked()
        {
            g.lods.remove(selected);
            selected = selected.saturating_sub(1);
            changed = true;
        }
        if ui
            .add(egui::Button::new("Fill from Children").small())
            .on_hover_text("Child 0 → LOD 0, child 1 → LOD 1, …")
            .clicked()
        {
            let kids = state.scene.children(e);
            for (i, level) in g.lods.iter_mut().enumerate() {
                level.renderers = kids.get(i).copied().into_iter().collect();
            }
            changed = true;
        }
    });

    // The selected level's renderers.
    if let Some(level) = g.lods.get_mut(selected) {
        super::card::sub_header(ui, &format!("LOD {selected} renderers"));
        let mut remove = None;
        for (k, r) in level.renderers.iter().enumerate() {
            ui.horizontal(|ui| {
                let name = match state.scene.contains(*r) {
                    true => state.scene.name(*r),
                    false => "(missing)".to_owned(),
                };
                ui.label(format!("◈ {name}"));
                if ui.small_button("✕").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            level.renderers.remove(k);
            changed = true;
        }
        let choices = renderer_choices(&state.scene, e);
        egui::ComboBox::from_id_salt(("lod_add", selected))
            .selected_text("Add Renderer…")
            .show_ui(ui, |ui| {
                for c in choices {
                    let taken = level.renderers.contains(&c);
                    if ui
                        .add_enabled(!taken, egui::Button::new(state.scene.name(c)).frame(false))
                        .clicked()
                    {
                        level.renderers.push(c);
                        changed = true;
                    }
                }
            });
    }

    super::editors::grid(ui, "lod_group_grid", |ui| {
        ui.label(tr!("Fade Mode")).on_hover_text(tr!(
            "How neighbouring levels switch: None cuts at once, Cross Fade draws both through a fine dither over the Fade Width, so nothing pops"
        ));
        let mut cross = g.fade_mode == LodFadeMode::CrossFade;
        let before = cross;
        egui::ComboBox::from_id_salt("lod_fade_mode")
            .selected_text(if cross {
                tr!("Cross Fade")
            } else {
                tr!("None")
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut cross, false, tr!("None"));
                ui.selectable_value(&mut cross, true, tr!("Cross Fade"));
            });
        if cross != before {
            g.fade_mode = match cross {
                true => LodFadeMode::CrossFade,
                false => LodFadeMode::None,
            };
            changed = true;
        }
        ui.end_row();
        if cross {
            ui.label(tr!("Fade Width")).on_hover_text(tr!(
                "Width of the cross-fade at each switch, as a share of its screen height (0.1 = about a tenth of the switch distance). Only objects inside it draw twice."
            ));
            changed |= ui
                .add(egui::Slider::new(&mut g.fade_width, 0.0..=1.0))
                .changed();
            ui.end_row();
        }
        false
    });
    ui.data_mut(|d| d.insert_temp(id, selected));
    if changed {
        let _ = state.scene.world.insert_one(e, g);
    }
    changed
}
