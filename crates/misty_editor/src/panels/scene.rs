//! Scene view: toolbar, 3D viewport, tools overlay, scene gizmo and object icons.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Ui, pos2, vec2};
use glam::Vec3;
use misty_engine::render::{FogDebug, FogSettings, Frame, GridPlane, PostSettings};

use super::AssetPayload;
use crate::branding::icons::{self, Icon};
use crate::scene_camera::{AxisView, SceneCamera};
use crate::state::{EditorState, Tool, object_icon};
use crate::theme;
use crate::viewport::{Gpu, ViewId};
use crate::widgets::{panel_toolbar, search_field, toggle_button};
use crate::{gizmos, handles, picking, render_scene};

/// Scene view draw modes, indexed by `SceneView::draw_mode` (see the `*_MODE` constants).
pub const DRAW_MODES: [&str; 8] = [
    "Shaded",
    "Wireframe",
    "Shaded Wireframe",
    "Shadow Cascades",
    "Reflection Probes",
    "Fog Density",
    "Fog Lighting",
    "Ambient Occlusion",
];
/// Wireframe only, no shading.
const WIREFRAME_MODE: usize = 1;
/// Shaded with a faint wireframe on top.
const SHADED_WIREFRAME_MODE: usize = 2;
/// Shaded, each sun cascade tinted.
const CASCADES_MODE: usize = 3;
/// Colours pixels by the reflection probes that light them.
const PROBES_MODE: usize = 4;
/// Volumetric fog alone: density = 1 - transmittance.
const FOG_DENSITY_MODE: usize = 5;
/// Volumetric fog alone: the light it scatters.
const FOG_LIGHTING_MODE: usize = 6;
/// The screen-space AO term alone.
const AO_DRAW_MODE: usize = 7;

pub fn show(ui: &mut Ui, state: &mut EditorState, gpu: Option<&mut Gpu>) {
    toolbar(ui, state);

    let rect = ui.available_rect_before_wrap();
    state.scene_view.view_rect = rect;
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();

    // Lock View to Selected: the pivot stays on the selection while it moves.
    if state.scene_view.lock_to_selection
        && let Some((center, _)) = crate::commands::selection_bounds(state)
    {
        state.scene_view.camera.follow(center);
    }
    // Camera input first so this frame renders the updated view.
    let view_tool = state.tool == Tool::Hand;
    let cam = &mut state.scene_view.camera;
    cam.set_2d(state.scene_view.is_2d);
    if cam.handle_input(ui.ctx(), &response, rect.height(), view_tool) {
        ui.ctx().request_repaint();
    }

    let camera = cam.camera();
    // Terrain brushes take the left mouse button while a brush tool is active.
    let brush_active = crate::terrain_tools::brush_active(state);
    let brush_click = crate::terrain_tools::scene_view(ui, state, &camera, rect, &response);
    match gpu {
        Some(gpu) => {
            let mut meshes = render_scene::cached_draws(state);
            let sv = &state.scene_view;
            // Probe gizmo spheres join a copy of the cached draws (only while probes exist).
            if sv.gizmos {
                let mut spheres = Vec::new();
                crate::probe_gizmos::spheres(state, &mut spheres);
                if !spheres.is_empty() {
                    let mut all = (*meshes).clone();
                    all.extend(spheres);
                    meshes = std::sync::Arc::new(all);
                }
            }
            let mut lines = if sv.gizmos {
                let mut l = gizmos::scene_lines(&state.scene, &state.selection);
                let screen = state.game_view.screen_px;
                crate::ui_gizmos::ui_lines(&mut l, &state.scene, &state.selection, screen);
                crate::probe_gizmos::lines(&mut l, state, sv.draw_mode == PROBES_MODE);
                l
            } else {
                gizmos::Lines::default()
            };
            match sv.draw_mode {
                WIREFRAME_MODE => gizmos::wireframe(&mut lines, &meshes, gizmos::WIREFRAME),
                SHADED_WIREFRAME_MODE => {
                    gizmos::wireframe(&mut lines, &meshes, [0.0, 0.0, 0.0, 0.35])
                }
                _ => {}
            }
            crate::terrain_tools::brush_lines(&mut lines, state);
            if sv.gizmos {
                crate::panels::world_builder::gizmos::lines(&mut lines, state, ui.ctx());
            }
            let overlay = gizmos::nav_overlay(state, ui.ctx(), &mut lines);
            let terrains = misty_engine::terrain::draws(&state.scene);
            let water = misty_engine::water::draws(&state.scene);
            let particles = misty_engine::particles::batches(&state.scene);
            let scene_lights = render_scene::lights(&state.scene);
            // The sky follows the scene's sun even when the view uses the headlight.
            let mut environment = render_scene::environment(&state.scene, &scene_lights);
            environment.probes.debug = sv.draw_mode == PROBES_MODE;
            let fx = sv.effects;
            if !(sv.skybox && fx.fog) {
                environment = environment.without_fog();
            }
            // Halos and lens flares are screen effects of the scene's lights: they have their own
            // toggle, also follow the Post Processing toggle and hide with the Lighting toggle.
            if !(sv.skybox && fx.glows && fx.post_processing && sv.lighting) {
                environment.glows.clear();
            }
            let volume = render_scene::post_settings(&state.scene);
            let mut post = if sv.skybox && fx.post_processing {
                volume
            } else {
                PostSettings::default()
            };
            // Volumetric fog follows the Fog toggle; the fog debug modes always show it.
            let debug = match sv.draw_mode {
                FOG_DENSITY_MODE => FogDebug::Density,
                FOG_LIGHTING_MODE => FogDebug::Lighting,
                _ => FogDebug::Off,
            };
            post.fog = if sv.skybox && fx.fog || debug != FogDebug::Off {
                volume.fog
            } else {
                FogSettings::default()
            };
            post.fog.debug = debug;
            // Clouds follow their own toggle (drawn with the skybox).
            post.clouds = if sv.skybox && fx.clouds {
                volume.clouds
            } else {
                Default::default()
            };
            // The AO debug view shows the AO term alone (no other effects).
            if sv.draw_mode == AO_DRAW_MODE {
                post = ao_debug_post(&state.scene);
            }
            let lights = if sv.lighting {
                scene_lights
            } else {
                render_scene::headlight(&camera)
            };
            let frame = Frame {
                camera: &camera,
                grid: sv.grid.then(|| grid_plane(&sv.camera)),
                // 2D mode uses a flat editor background instead of the sky.
                skybox: sv.skybox && fx.skybox && !sv.camera.is_2d(),
                background: [0.28, 0.28, 0.28],
                environment: &environment,
                post,
                lines: &lines.0,
                overlay: &overlay,
                meshes: &meshes,
                particles: &particles,
                terrains: &terrains,
                shaded: sv.draw_mode != WIREFRAME_MODE,
                lights: &lights,
                water: &water,
            };
            // Quality ▸ Render Scale: render smaller (or larger) and stretch to the panel.
            let px = ppp * misty_engine::quality::active().render_scale;
            let size = [(rect.width() * px) as u32, (rect.height() * px) as u32];
            gpu.renderer
                .set_cascade_debug(sv.draw_mode == CASCADES_MODE);
            let texture = gpu.render(ViewId::Scene, size, &frame);
            ui.painter().image(
                texture,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_rgb(70, 75, 82));
        }
    }

    // Object icons and click-to-select (Ctrl toggles, Shift adds).
    if state.scene_view.gizmos {
        draw_icons(ui, rect, state, &camera);
    }
    // 2D handles (sprite Rect tool, Edit Collider) take the mouse before the transform handles.
    let hovered = response.hovered();
    if !brush_active
        && !crate::sprite_handles::show(ui, state, &camera, rect, hovered)
        && !crate::probe_gizmos::handles(ui, state, &camera, rect, hovered)
        && !crate::area_light_handles::handles(ui, state, &camera, rect, hovered)
        && !crate::panels::world_builder::gizmos::show(ui, state, &camera, rect, hovered)
    {
        handles::show(ui, state, &camera, rect, hovered);
    }
    let suppressed = state.handles.click_suppressed() || brush_click;
    if response.clicked()
        && !suppressed
        && !view_tool
        && !ui.input(|i| i.modifiers.alt)
        && let Some(click) = response.interact_pointer_pos()
    {
        let hit = picking::pick(&state.scene, &camera, rect, click, &state.scene_view.icons);
        let mods = ui.input(|i| i.modifiers);
        match hit {
            Some(e) if mods.command => state.selection.toggle(e),
            Some(e) if mods.shift => {
                if !state.selection.contains(e) {
                    state.selection.toggle(e);
                }
            }
            Some(e) => state.select(e),
            None if mods.command || mods.shift => {}
            None => state.selection.clear(),
        }
    }

    // Drop a prefab or model into the view (instantiated at the pivot), a material onto an
    // object, or an HDRI to make it the sky.
    let payload = response.dnd_release_payload::<AssetPayload>();
    if let Some(p) = payload
        .as_ref()
        .filter(|p| misty_engine::hdri::is_hdri(&p.0))
    {
        // An HDRI anywhere in the view becomes the sky.
        super::lighting_hdri::drop_on_scene(state, &p.0);
    } else if let Some(p) = payload.as_ref().filter(|p| p.0.ends_with(".prefab")) {
        state.push(crate::actions::Action::InstantiatePrefab {
            path: p.0.clone(),
            parent: None,
        });
    } else if let Some(p) = payload
        .as_ref()
        .filter(|p| misty_engine::models::is_model(&p.0))
    {
        state.push(crate::actions::Action::InstantiateModel {
            path: p.0.clone(),
            parent: None,
        });
    } else if let Some(payload) = payload
        && let Some(at) = ui.input(|i| i.pointer.hover_pos())
        && let Some(e) = picking::pick(&state.scene, &camera, rect, at, &state.scene_view.icons)
    {
        super::hierarchy::drop_asset(state, e, &payload.0);
    }

    if state.scene_view.gizmos {
        gizmos::paint_bones(ui, rect, &camera, &state.scene, &state.selection);
    }
    tools_overlay(ui, rect, state);
    crate::particle_preview::overlay(ui, rect, state);
    if !state.scene_view.camera.is_2d() {
        scene_gizmo(ui, rect, &mut state.scene_view.camera);
    }
}

/// Grid follows the view: XZ normally, the facing plane in axis views and 2D.
fn grid_plane(cam: &SceneCamera) -> GridPlane {
    if cam.is_2d() {
        return GridPlane::XY;
    }
    if cam.orthographic {
        let f = cam.rotation() * Vec3::Z;
        let a = f.abs();
        if a.x > 0.99 {
            return GridPlane::YZ;
        }
        if a.z > 0.99 {
            return GridPlane::XY;
        }
    }
    GridPlane::XZ
}

/// Billboard icons for cameras and lights (objects without a mesh).
fn draw_icons(ui: &Ui, rect: Rect, state: &EditorState, camera: &misty_engine::camera::Camera) {
    let painter = ui.painter().with_clip_rect(rect);
    for e in picking::icon_objects(&state.scene) {
        let Some(p) = picking::to_screen(camera, rect, state.scene.world_position(e)) else {
            continue;
        };
        let icons = &state.scene_view.icons;
        let side = icons.side(camera.position, state.scene.world_position(e));
        let color = if state.selection.contains(e) {
            Color32::from_rgb(255, 160, 40)
        } else {
            icons.tint()
        };
        match crate::state::object_brand_icon(&state.scene, e) {
            Some(icon) => {
                let r = Rect::from_center_size(p, vec2(side, side));
                icons::paint_with(&painter, r, icon, color);
            }
            None => {
                painter.text(
                    p,
                    Align2::CENTER_CENTER,
                    object_icon(&state.scene, e),
                    FontId::proportional(side * (22.0 / 24.0)),
                    color,
                );
            }
        }
    }
}

/// Tool row in the top-left corner of the view: View / Move / Rotate / Scale (the designer's
/// icons), Rect and Transform, then the grid toggle.
fn tools_overlay(ui: &mut Ui, rect: Rect, state: &mut EditorState) {
    let button = vec2(28.0, 26.0);
    let (pad, gap, divider) = (3.0, 2.0, 9.0);
    let count = Tool::ALL.len() as f32 + 1.0;
    let panel = Rect::from_min_size(
        rect.left_top() + vec2(8.0, 8.0),
        vec2(
            count * (button.x + gap) - gap + divider + pad * 2.0,
            button.y + pad * 2.0,
        ),
    );
    // Theme colours, so the tinted icons always contrast with the row.
    let v = ui.visuals();
    let (fill, line) = (v.extreme_bg_color.gamma_multiply(0.92), v.weak_text_color());
    ui.painter().rect_filled(panel, 5.0, fill);
    let slot = |i: f32, extra: f32| {
        let left = panel.left() + pad + i * (button.x + gap) + extra;
        Rect::from_min_size(egui::pos2(left, panel.top() + pad), button)
    };
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let r = slot(i as f32, 0.0);
        let selected = state.tool == tool;
        let clicked = match tool.brand_icon() {
            Some(icon) => icons::button_at(ui, r, icon, selected, tool.tooltip()).clicked(),
            None => {
                let b = egui::Button::new(egui::RichText::new(tool.icon()).size(15.0))
                    .selected(selected)
                    .frame_when_inactive(false);
                ui.put(r, b).on_hover_text(tool.tooltip()).clicked()
            }
        };
        if clicked {
            state.tool = tool;
        }
    }
    // Divider, then the grid toggle.
    let r = slot(Tool::ALL.len() as f32, divider);
    ui.painter().vline(
        r.left() - divider * 0.5 - gap * 0.5,
        panel.y_range().shrink(6.0),
        egui::Stroke::new(1.0, line.gamma_multiply(0.6)),
    );
    let grid = &mut state.scene_view.grid;
    if icons::button_at(ui, r, Icon::Grid, *grid, "Toggle grid visibility").clicked() {
        *grid = !*grid;
    }
}

/// Axis gizmo in the top-right corner: click a cone to snap, the centre or label to toggle Persp/Iso.
fn scene_gizmo(ui: &mut Ui, rect: Rect, cam: &mut SceneCamera) {
    let radius = 30.0;
    let center = pos2(rect.right() - 56.0, rect.top() + 52.0);
    let area = Rect::from_center_size(center, vec2(radius * 2.6, radius * 2.6));
    let label_pos = center + vec2(0.0, radius + 20.0);
    let label_rect = Rect::from_center_size(label_pos, vec2(70.0, 18.0));
    let painter = ui.painter().with_clip_rect(rect);

    let inv = cam.rotation().inverse();
    let gray = Color32::from_gray(170);
    let axes = [
        (Vec3::X, theme::AXIS_X, Some("x"), AxisView::Right),
        (-Vec3::X, gray, None, AxisView::Left),
        (Vec3::Y, theme::AXIS_Y, Some("y"), AxisView::Top),
        (-Vec3::Y, gray, None, AxisView::Bottom),
        (Vec3::Z, theme::AXIS_Z, Some("z"), AxisView::Back),
        (-Vec3::Z, gray, None, AxisView::Front),
    ];
    // Camera-space direction: x right, y up, z into the screen. Draw far cones first.
    let mut items: Vec<_> = axes
        .iter()
        .map(|(dir, color, label, view)| {
            let v = inv * *dir;
            (
                center + vec2(v.x, -v.y) * radius,
                v.z,
                *color,
                *label,
                *view,
            )
        })
        .collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));

    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut drew_center = false;
    for (base, depth, color, label, _) in &items {
        if !drew_center && *depth < 0.0 {
            draw_center(&painter, center);
            drew_center = true;
        }
        let hovered = hover.is_some_and(|h| h.distance(*base) < 9.0);
        let color = if hovered {
            color.gamma_multiply(1.3)
        } else {
            *color
        };
        draw_cone(&painter, center, *base, color);
        if let Some(l) = label {
            let off = *base - center;
            let lp = center + off * 1.45;
            painter.text(
                lp,
                Align2::CENTER_CENTER,
                *l,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }
    if !drew_center {
        draw_center(&painter, center);
    }
    painter.text(
        label_pos,
        Align2::CENTER_CENTER,
        format!("< {}", cam.view_label()),
        FontId::proportional(12.0),
        Color32::from_gray(220),
    );

    let response = ui.interact(
        area.union(label_rect),
        ui.id().with("scene_gizmo"),
        Sense::click(),
    );
    if response.clicked()
        && let Some(click) = response.interact_pointer_pos()
    {
        let hit = items
            .iter()
            .filter(|(base, ..)| base.distance(click) < 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((.., view)) = hit {
            cam.snap_to_axis(*view);
        } else if click.distance(center) < 10.0 || label_rect.contains(click) {
            cam.toggle_projection();
        }
    }
}

fn draw_center(painter: &egui::Painter, center: Pos2) {
    painter.rect_filled(
        Rect::from_center_size(center, vec2(12.0, 12.0)),
        2.0,
        Color32::from_gray(200),
    );
}

/// Cone pointing at the centre: triangle from the base to the centre plus a round cap.
fn draw_cone(painter: &egui::Painter, center: Pos2, base: Pos2, color: Color32) {
    let dir = base - center;
    let len = dir.length();
    if len > 2.0 {
        let n = dir / len;
        let perp = vec2(-n.y, n.x) * 6.5;
        let tip = center + n * (len * 0.25);
        painter.add(egui::Shape::convex_polygon(
            vec![tip, base + perp, base - perp],
            color,
            egui::Stroke::NONE,
        ));
    }
    painter.circle_filled(base, 6.5, color);
}

fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    let s = &mut *state;
    let mut quality_edited = false;
    panel_toolbar(ui, |ui| {
        let pivot = if s.pivot_center { "Center" } else { "Pivot" };
        egui::ComboBox::from_id_salt("scene_pivot")
            .selected_text(pivot)
            .width(64.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.pivot_center, false, "Pivot");
                ui.selectable_value(&mut s.pivot_center, true, "Center");
            });
        let space = if s.global_space { "Global" } else { "Local" };
        egui::ComboBox::from_id_salt("scene_space")
            .selected_text(space)
            .width(64.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.global_space, true, "Global");
                ui.selectable_value(&mut s.global_space, false, "Local");
            });
        // Grid snapping needs the world-aligned handle, like the familiar toolbar toggle.
        let snap_tip =
            "Toggle grid snapping (Global handle only). Hold Ctrl to snap by increments.";
        let snap_button = ui.add_enabled_ui(s.global_space, |ui| {
            toggle_button(ui, s.snap.grid_snap, "🧲", snap_tip)
        });
        if snap_button.inner.clicked() {
            s.snap.grid_snap = !s.snap.grid_snap;
            crate::snap_window::changed(s);
        }
        let sv = &mut s.scene_view;
        let quality = &mut s.project_settings.quality;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut sv.search, 110.0);
            // Right-to-left: the ▾ dropdown sits right of the Gizmos toggle.
            let icon_menu = crate::widgets::dropdown(ui, "⏷", |ui| {
                crate::scene_icons::menu(ui, &mut sv.icons)
            })
            .0
            .on_hover_text("Gizmo icon size and color");
            if std::mem::take(&mut sv.open_icon_menu) {
                crate::widgets::open_dropdown(ui.ctx(), &icon_menu);
            }
            if toggle_button(ui, sv.gizmos, "Gizmos", "Toggle gizmos").clicked() {
                sv.gizmos = !sv.gizmos;
            }
            crate::widgets::dropdown(ui, "📷 ⏷", |ui| camera_settings(ui, &mut sv.camera))
                .0
                .on_hover_text("Scene camera settings");
            let fx_menu = crate::widgets::dropdown(ui, "⏷", |ui| {
                let fx = &mut sv.effects;
                ui.checkbox(&mut fx.skybox, "Skybox");
                ui.checkbox(&mut fx.fog, "Fog");
                ui.checkbox(&mut fx.clouds, "Clouds");
                ui.checkbox(&mut fx.glows, "Halos & Flares");
                ui.checkbox(&mut fx.post_processing, "Post Processing");
            })
            .0
            .on_hover_text("Scene view effects");
            crate::tutorials::targets::mark_response(ui, "scene.effects_menu", &fx_menu);
            let fx_toggle = toggle_button(
                ui,
                sv.skybox,
                "☁",
                "Toggle skybox, fog, clouds, halos and post-processing",
            );
            crate::tutorials::targets::mark(
                ui,
                "scene.effects",
                fx_toggle.rect.union(fx_menu.rect),
            );
            if fx_toggle.clicked() {
                sv.skybox = !sv.skybox;
            }

            if toggle_button(ui, sv.audio, "🔊", "Toggle audio").clicked() {
                sv.audio = !sv.audio;
            }
            if toggle_button(ui, sv.lighting, "💡", "Toggle scene lighting").clicked() {
                sv.lighting = !sv.lighting;
            }
            if toggle_button(ui, sv.is_2d, "2D", "Toggle 2D view").clicked() {
                sv.is_2d = !sv.is_2d;
            }
            let quality_menu = crate::widgets::dropdown(ui, "Quality ⏷", |ui| {
                quality_edited |= crate::scene_quality::menu(ui, quality)
            })
            .0
            .on_hover_text("Quality level, anti-aliasing, render scale and upscale filter");
            if std::mem::take(&mut sv.open_quality_menu) {
                crate::widgets::open_dropdown(ui.ctx(), &quality_menu);
            }
            egui::ComboBox::from_id_salt("scene_draw_mode")
                .selected_text(DRAW_MODES[sv.draw_mode])
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, m) in DRAW_MODES.iter().enumerate() {
                        ui.selectable_value(&mut sv.draw_mode, i, *m);
                    }
                });
        });
    });
    // Same save and live update as Project Settings ▸ Quality.
    if quality_edited {
        crate::project_settings::changed(state);
    }
}

/// Camera settings dropdown: field of view, clipping and fly speed.
fn camera_settings(ui: &mut Ui, cam: &mut SceneCamera) {
    ui.set_min_width(220.0);
    ui.strong("Scene Camera");
    egui::Grid::new("scene_cam_settings")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Field of View");
            ui.add(egui::Slider::new(&mut cam.fov, 4.0..=120.0).suffix("°"));
            ui.end_row();
            ui.label("Dynamic Clipping");
            ui.checkbox(&mut cam.dynamic_clipping, "");
            ui.end_row();
            ui.add_enabled_ui(!cam.dynamic_clipping, |ui| ui.label("Clipping Near"));
            ui.add_enabled(
                !cam.dynamic_clipping,
                egui::DragValue::new(&mut cam.near)
                    .speed(0.01)
                    .range(0.001..=100.0),
            );
            ui.end_row();
            ui.add_enabled_ui(!cam.dynamic_clipping, |ui| ui.label("Clipping Far"));
            ui.add_enabled(
                !cam.dynamic_clipping,
                egui::DragValue::new(&mut cam.far)
                    .speed(10.0)
                    .range(1.0..=1.0e6),
            );
            ui.end_row();
            ui.label("Camera Speed");
            ui.add(egui::Slider::new(&mut cam.speed, 0.01..=10.0).logarithmic(true));
            ui.end_row();
        });
}

/// Ambient Occlusion draw mode: the scene's AO settings (defaults when no volume has them)
/// shown as grey on ungraded surfaces.
fn ao_debug_post(scene: &misty_engine::scene::Scene) -> PostSettings {
    PostSettings {
        ao: render_scene::post_settings(scene).ao.debug_view(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    //! Drive the real Scene panel with synthetic mouse/keyboard input.

    use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
    use egui_kittest::Harness;
    use glam::Vec3;

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    const CENTER: Pos2 = pos2(400.0, 330.0);

    fn harness() -> Harness<'static, EditorState> {
        let root = std::env::temp_dir().join("misty_scene_test_project");
        let state = EditorState::new(ProjectBrowser::open(root));
        let mut h = Harness::builder()
            .with_size([800.0, 600.0])
            .build_ui_state(|ui, s: &mut EditorState| super::show(ui, s, None), state);
        h.run_steps(2);
        h
    }

    fn button(
        h: &mut Harness<'_, EditorState>,
        pos: Pos2,
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    ) {
        h.event(Event::ModifiersChanged(modifiers));
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        });
        h.step();
    }

    /// Press at CENTER, move by `delta` in a few steps, release.
    fn drag(
        h: &mut Harness<'_, EditorState>,
        b: PointerButton,
        delta: egui::Vec2,
        modifiers: Modifiers,
    ) {
        button(h, CENTER, b, true, modifiers);
        for i in 1..=5 {
            h.event(Event::PointerMoved(CENTER + delta * (i as f32 / 5.0)));
            h.step();
        }
        button(h, CENTER + delta, b, false, modifiers);
    }

    fn cam<'h>(h: &'h Harness<'_, EditorState>) -> &'h crate::scene_camera::SceneCamera {
        &h.state().scene_view.camera
    }

    #[test]
    fn right_drag_looks_around_from_a_fixed_eye() {
        let mut h = harness();
        let (eye, rot) = (cam(&h).position(), cam(&h).rotation());
        drag(
            &mut h,
            PointerButton::Secondary,
            egui::vec2(80.0, 30.0),
            Modifiers::NONE,
        );
        assert!(
            cam(&h).rotation().angle_between(rot) > 0.2,
            "view should rotate"
        );
        assert!(
            (cam(&h).position() - eye).length() < 0.01,
            "eye should stay put"
        );
    }

    #[test]
    fn right_mouse_plus_w_flies_forward() {
        let mut h = harness();
        let eye = cam(&h).position();
        let forward = cam(&h).rotation() * Vec3::Z;
        button(
            &mut h,
            CENTER,
            PointerButton::Secondary,
            true,
            Modifiers::NONE,
        );
        h.key_down(Key::W);
        h.run_steps(20);
        h.key_up(Key::W);
        button(
            &mut h,
            CENTER,
            PointerButton::Secondary,
            false,
            Modifiers::NONE,
        );
        let moved = cam(&h).position() - eye;
        assert!(moved.dot(forward) > 0.5, "moved {moved:?}");
    }

    #[test]
    fn middle_drag_pans_without_rotating() {
        let mut h = harness();
        let (pivot, rot) = (cam(&h).pivot(), cam(&h).rotation());
        drag(
            &mut h,
            PointerButton::Middle,
            egui::vec2(100.0, 0.0),
            Modifiers::NONE,
        );
        let right = rot * Vec3::X;
        let moved = cam(&h).pivot() - pivot;
        assert!(
            moved.dot(right) < -0.5,
            "dragging right moves the camera left: {moved:?}"
        );
        assert!(cam(&h).rotation().angle_between(rot) < 1e-2);
    }

    #[test]
    fn alt_left_drag_orbits_around_pivot() {
        let mut h = harness();
        let (pivot, eye) = (cam(&h).pivot(), cam(&h).position());
        drag(
            &mut h,
            PointerButton::Primary,
            egui::vec2(120.0, 0.0),
            Modifiers::ALT,
        );
        assert!((cam(&h).pivot() - pivot).length() < 1e-3);
        assert!((cam(&h).position() - eye).length() > 1.0);
    }

    #[test]
    fn scroll_wheel_zooms_towards_pivot() {
        let mut h = harness();
        let before = cam(&h).position().distance(cam(&h).pivot());
        h.event(Event::PointerMoved(CENTER));
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 200.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(30);
        let after = cam(&h).position().distance(cam(&h).pivot());
        assert!(after < before * 0.9, "{before} -> {after}");
    }

    #[test]
    fn view_tool_left_drag_pans() {
        let mut h = harness();
        h.state_mut().tool = crate::state::Tool::Hand;
        let pivot = cam(&h).pivot();
        drag(
            &mut h,
            PointerButton::Primary,
            egui::vec2(0.0, 80.0),
            Modifiers::NONE,
        );
        assert!((cam(&h).pivot() - pivot).length() > 0.5);
    }

    #[test]
    fn clicking_an_icon_selects_and_empty_space_deselects() {
        let mut h = harness();
        let cam_now = cam(&h).camera();
        let light = h.state().scene.find_by_name("Directional Light").unwrap();
        let p = h.state().scene.world_position(light);
        let view = h.state().scene_view.view_rect;
        let (uv, _) = cam_now
            .world_to_screen(view.width() / view.height(), p)
            .unwrap();
        let at = pos2(
            view.left() + uv.x * view.width(),
            view.top() + uv.y * view.height(),
        );
        button(&mut h, at, PointerButton::Primary, true, Modifiers::NONE);
        button(&mut h, at, PointerButton::Primary, false, Modifiers::NONE);
        assert_eq!(h.state().selection.active(), Some(light));

        button(
            &mut h,
            pos2(700.0, 550.0),
            PointerButton::Primary,
            true,
            Modifiers::NONE,
        );
        button(
            &mut h,
            pos2(700.0, 550.0),
            PointerButton::Primary,
            false,
            Modifiers::NONE,
        );
        assert!(h.state().selection.is_empty());
    }

    /// Bigger icons are easier to hit: 20 px off the light only selects at 2× size.
    #[test]
    fn icon_size_scales_the_click_area() {
        let mut h = harness();
        let light = h.state().scene.find_by_name("Directional Light").unwrap();
        let p = h.state().scene.world_position(light);
        let view = h.state().scene_view.view_rect;
        let (uv, _) = cam(&h)
            .camera()
            .world_to_screen(view.width() / view.height(), p)
            .unwrap();
        let at = pos2(
            view.left() + uv.x * view.width() + 20.0,
            view.top() + uv.y * view.height(),
        );
        for (size, hit) in [(1.0, false), (2.0, true)] {
            h.state_mut().selection.clear();
            h.state_mut().scene_view.icons.size = size;
            button(&mut h, at, PointerButton::Primary, true, Modifiers::NONE);
            button(&mut h, at, PointerButton::Primary, false, Modifiers::NONE);
            assert_eq!(
                h.state().selection.active() == Some(light),
                hit,
                "size {size}"
            );
        }
    }

    #[test]
    fn gizmos_dropdown_shows_icon_size_and_color() {
        use egui_kittest::kittest::Queryable;
        let mut h = harness();
        h.state_mut().scene_view.open_icon_menu = true;
        h.run_steps(3);
        assert!(h.query_by_label("Icon Size").is_some());
        assert!(h.query_by_label("Icon Color").is_some());
    }

    #[test]
    fn quality_dropdown_shows_render_scale_and_filter() {
        use egui_kittest::kittest::Queryable;
        let mut h = harness();
        h.state_mut().scene_view.open_quality_menu = true;
        h.run_steps(3);
        assert!(h.query_by_label("Render Scale").is_some());
        assert!(h.query_by_label("Upscale Filter").is_some());
    }

    fn gizmo_center(h: &Harness<'_, EditorState>) -> Pos2 {
        let r = h.state().scene_view.view_rect;
        pos2(r.right() - 56.0, r.top() + 52.0)
    }

    fn click(h: &mut Harness<'_, EditorState>, at: Pos2) {
        button(h, at, PointerButton::Primary, true, Modifiers::NONE);
        button(h, at, PointerButton::Primary, false, Modifiers::NONE);
        h.run_steps(30);
    }

    #[test]
    fn gizmo_centre_toggles_iso_and_y_cone_snaps_to_top() {
        let mut h = harness();
        let c = gizmo_center(&h);
        click(&mut h, c);
        assert_eq!(cam(&h).view_label(), "Iso");
        click(&mut h, c);
        assert_eq!(cam(&h).view_label(), "Persp");

        let v = cam(&h).rotation().inverse() * Vec3::Y;
        click(&mut h, c + egui::vec2(v.x, -v.y) * 30.0);
        assert_eq!(cam(&h).view_label(), "Top");
        let forward = cam(&h).rotation() * Vec3::Z;
        assert!(
            (forward + Vec3::Y).length() < 1e-3,
            "looking straight down: {forward:?}"
        );
    }

    /// A cube at the origin, selected, and the screen points of its handle origin and +X/+Y tips.
    fn cube_setup(
        h: &mut Harness<'_, EditorState>,
    ) -> (misty_engine::hecs::Entity, Pos2, Pos2, Pos2) {
        use misty_engine::scene::{MeshFilter, PrimitiveMesh};
        let s = h.state_mut();
        let cube = s.scene.spawn("Cube", None);
        s.scene.add(
            cube,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Cube.into()),
            },
        );
        s.selection.set(cube);
        h.run_steps(2);
        let cam = cam(h).camera();
        let rect = h.state().scene_view.view_rect;
        let len = cam.world_per_pixel(Vec3::ZERO, rect.height()) * 90.0;
        let at = |p: Vec3| crate::picking::to_screen(&cam, rect, p).unwrap();
        (cube, at(Vec3::ZERO), at(Vec3::X * len), at(Vec3::Y * len))
    }

    #[test]
    fn dragging_move_handle_x_axis_moves_along_x_only() {
        let mut h = harness();
        let (cube, origin, x_tip, _) = cube_setup(&mut h);
        let grab = origin + (x_tip - origin) * 0.6;
        let dir = (x_tip - origin).normalized();
        button(&mut h, grab, PointerButton::Primary, true, Modifiers::NONE);
        for i in 1..=5 {
            h.event(Event::PointerMoved(grab + dir * (12.0 * i as f32)));
            h.step();
        }
        button(
            &mut h,
            grab + dir * 60.0,
            PointerButton::Primary,
            false,
            Modifiers::NONE,
        );
        let p = h.state().scene.transform(cube).position;
        assert!(p.x > 0.3, "moved along +X: {p:?}");
        assert!(
            p.y.abs() < 1e-3 && p.z.abs() < 1e-3,
            "other axes untouched: {p:?}"
        );
        assert_eq!(
            h.state().selection.active(),
            Some(cube),
            "handle click must not change selection"
        );
    }

    /// Drag the move handle's X axis by `px` pixels with `mods` held; returns the cube position.
    fn drag_x_axis(h: &mut Harness<'_, EditorState>, px: f32, mods: Modifiers) -> Vec3 {
        let (cube, origin, x_tip, _) = cube_setup(h);
        let grab = origin + (x_tip - origin) * 0.6;
        let dir = (x_tip - origin).normalized();
        button(h, grab, PointerButton::Primary, true, mods);
        for i in 1..=5 {
            h.event(Event::PointerMoved(grab + dir * (px * i as f32 / 5.0)));
            h.step();
        }
        button(h, grab + dir * px, PointerButton::Primary, false, mods);
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.state().scene.transform(cube).position
    }

    #[test]
    fn ctrl_drag_snaps_by_the_move_increment() {
        let mut h = harness();
        h.state_mut().snap.move_step = [0.5; 3];
        let p = drag_x_axis(&mut h, 70.0, Modifiers::COMMAND);
        assert!(p.x >= 0.5, "moved: {p:?}");
        assert!((p.x / 0.5 - (p.x / 0.5).round()).abs() < 1e-4, "{p:?}");
        assert_eq!((p.y, p.z), (0.0, 0.0));
    }

    #[test]
    fn grid_snapping_rounds_the_dragged_axis_to_the_world_grid() {
        let mut h = harness();
        h.state_mut().snap.grid_snap = true;
        h.state_mut().snap.grid_size = [0.5, 1.0, 1.0];
        let p = drag_x_axis(&mut h, 55.0, Modifiers::NONE);
        assert!(
            p.x > 0.0 && (p.x * 2.0 - (p.x * 2.0).round()).abs() < 1e-4,
            "{p:?}"
        );
        // Local handle orientation turns grid snapping off.
        let mut h = harness();
        h.state_mut().snap.grid_snap = true;
        h.state_mut().global_space = false;
        let p = drag_x_axis(&mut h, 55.0, Modifiers::NONE);
        assert!((p.x - p.x.round()).abs() > 1e-3, "free move: {p:?}");
    }

    #[test]
    fn lock_view_keeps_the_pivot_on_the_selection() {
        let mut h = harness();
        let (cube, ..) = cube_setup(&mut h);
        h.state_mut().scene_view.lock_to_selection = true;
        h.state_mut()
            .scene
            .set_world_position(cube, Vec3::new(3.0, 1.0, -2.0));
        h.run_steps(2);
        assert!((cam(&h).pivot() - Vec3::new(3.0, 1.0, -2.0)).length() < 1e-4);
    }

    #[test]
    fn scale_handle_y_axis_scales_y() {
        let mut h = harness();
        h.state_mut().tool = crate::state::Tool::Scale;
        let (cube, origin, _, y_tip) = cube_setup(&mut h);
        let dir = (y_tip - origin).normalized();
        let grab = origin + (y_tip - origin) * 0.6;
        button(&mut h, grab, PointerButton::Primary, true, Modifiers::NONE);
        for i in 1..=5 {
            h.event(Event::PointerMoved(grab + dir * (9.0 * i as f32)));
            h.step();
        }
        button(
            &mut h,
            grab + dir * 45.0,
            PointerButton::Primary,
            false,
            Modifiers::NONE,
        );
        let sc = h.state().scene.transform(cube).scale;
        assert!(sc.y > 1.3 && (sc.x - 1.0).abs() < 1e-4, "{sc:?}");
    }

    #[test]
    fn rotate_ring_y_rotates_about_y() {
        let mut h = harness();
        h.state_mut().tool = crate::state::Tool::Rotate;
        let (cube, origin, x_tip, _) = cube_setup(&mut h);
        // The Y ring passes through the +X tip; drag across it.
        let along = (x_tip - origin).normalized();
        let across = egui::vec2(-along.y, along.x);
        button(&mut h, x_tip, PointerButton::Primary, true, Modifiers::NONE);
        for i in 1..=5 {
            h.event(Event::PointerMoved(x_tip + across * (10.0 * i as f32)));
            h.step();
        }
        button(
            &mut h,
            x_tip + across * 50.0,
            PointerButton::Primary,
            false,
            Modifiers::NONE,
        );
        let e = h.state().scene.transform(cube).euler();
        assert!(e.y.abs() > 5.0, "rotated about Y: {e:?}");
        assert!(e.x.abs() < 0.1 && e.z.abs() < 0.1, "only Y: {e:?}");
    }
}
