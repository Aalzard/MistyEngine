//! Scene view: toolbar, 3D viewport, tools overlay, scene gizmo and object icons.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Ui, pos2, vec2};
use glam::Vec3;
use misty_engine::render::{Frame, GridPlane};

use super::AssetPayload;
use crate::scene_camera::{AxisView, SceneCamera};
use crate::state::{EditorState, Tool, object_icon};
use crate::theme;
use crate::viewport::{Gpu, ViewId};
use crate::widgets::{panel_toolbar, search_field, toggle_button};
use crate::{gizmos, handles, picking, render_scene};

const DRAW_MODES: [&str; 3] = ["Shaded", "Wireframe", "Shaded Wireframe"];

pub fn show(ui: &mut Ui, state: &mut EditorState, gpu: Option<&mut Gpu>) {
    toolbar(ui, state);

    let rect = ui.available_rect_before_wrap();
    state.scene_view.view_rect = rect;
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();

    // Camera input first so this frame renders the updated view.
    let view_tool = state.tool == Tool::Hand;
    let cam = &mut state.scene_view.camera;
    cam.set_2d(state.scene_view.is_2d);
    if cam.handle_input(ui.ctx(), &response, rect.height(), view_tool) {
        ui.ctx().request_repaint();
    }

    let camera = cam.camera();
    match gpu {
        Some(gpu) => {
            let sv = &state.scene_view;
            let meshes =
                render_scene::mesh_draws(&state.scene, &state.assets, Some(&state.selection));
            let mut lines = if sv.gizmos {
                let mut l = gizmos::scene_lines(&state.scene, &state.selection);
                let screen = state.game_view.screen_px;
                crate::ui_gizmos::ui_lines(&mut l, &state.scene, &state.selection, screen);
                l
            } else {
                gizmos::Lines::default()
            };
            // Draw modes: 0 Shaded, 1 Wireframe, 2 Shaded Wireframe.
            match sv.draw_mode {
                1 => gizmos::wireframe(&mut lines, &meshes, gizmos::WIREFRAME),
                2 => gizmos::wireframe(&mut lines, &meshes, [0.0, 0.0, 0.0, 0.35]),
                _ => {}
            }
            let lights = if sv.lighting {
                render_scene::lights(&state.scene)
            } else {
                render_scene::headlight(&camera)
            };
            let frame = Frame {
                camera: &camera,
                grid: sv.grid.then(|| grid_plane(&sv.camera)),
                // 2D mode uses a flat editor background instead of the sky.
                skybox: sv.skybox && !sv.camera.is_2d(),
                background: [0.28, 0.28, 0.28],
                lines: &lines.0,
                meshes: &meshes,
                shaded: sv.draw_mode != 1,
                lights: &lights,
                ambient_intensity: 1.0,
            };
            let size = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
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
    handles::show(ui, state, &camera, rect, response.hovered());
    let suppressed = state.handles.click_suppressed();
    if response.clicked()
        && !suppressed
        && !view_tool
        && !ui.input(|i| i.modifiers.alt)
        && let Some(click) = response.interact_pointer_pos()
    {
        let hit = picking::pick(&state.scene, &camera, rect, click);
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

    // Drop a prefab or model into the view (instantiated at the pivot) or a material onto an object.
    let payload = response.dnd_release_payload::<AssetPayload>();
    if let Some(p) = payload.as_ref().filter(|p| p.0.ends_with(".prefab")) {
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
        && let Some(e) = picking::pick(&state.scene, &camera, rect, at)
    {
        super::hierarchy::drop_asset(state, e, &payload.0);
    }

    tools_overlay(ui, rect, state);
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
        let color = if state.selection.contains(e) {
            Color32::from_rgb(255, 160, 40)
        } else {
            Color32::from_gray(235)
        };
        painter.text(
            p,
            Align2::CENTER_CENTER,
            object_icon(&state.scene, e),
            FontId::proportional(22.0),
            color,
        );
    }
}

/// Vertical tool strip in the top-left corner of the view.
fn tools_overlay(ui: &mut Ui, rect: Rect, state: &mut EditorState) {
    let button = vec2(28.0, 26.0);
    let pad = 4.0;
    let panel = Rect::from_min_size(
        rect.left_top() + vec2(8.0, 8.0),
        vec2(
            button.x + pad * 2.0,
            (button.y + 2.0) * Tool::ALL.len() as f32 + pad * 2.0,
        ),
    );
    ui.painter()
        .rect_filled(panel, 4.0, Color32::from_rgba_unmultiplied(40, 40, 40, 235));
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(
            panel.left_top() + vec2(pad, pad + i as f32 * (button.y + 2.0)),
            button,
        );
        let selected = state.tool == tool;
        let b = egui::Button::new(egui::RichText::new(tool.icon()).size(15.0)).selected(selected);
        if ui.put(r, b).on_hover_text(tool.tooltip()).clicked() {
            state.tool = tool;
        }
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
        let sv = &mut s.scene_view;
        if toggle_button(ui, sv.grid, "#", "Toggle grid visibility").clicked() {
            sv.grid = !sv.grid;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut sv.search, 110.0);
            if toggle_button(ui, sv.gizmos, "Gizmos", "Toggle gizmos").clicked() {
                sv.gizmos = !sv.gizmos;
            }
            ui.menu_button("📷 ⏷", |ui| camera_settings(ui, &mut sv.camera))
                .response
                .on_hover_text("Scene camera settings");
            if toggle_button(ui, sv.skybox, "☁", "Toggle skybox").clicked() {
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
