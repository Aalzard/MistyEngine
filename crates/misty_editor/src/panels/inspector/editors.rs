//! Per-component Inspector editors and the Add Component popup.

use egui::{Grid, Key, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Camera, ClearFlags, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, SceneComponent,
    Shadows, registry,
};

use crate::actions::Action;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::vec3_row;

pub fn icon(name: &str) -> &'static str {
    match name {
        "Transform" => "⬚",
        "Camera" => "🎥",
        "Light" => "💡",
        "Mesh Filter" => "▦",
        "Mesh Renderer" => "◈",
        _ => "▣",
    }
}

/// Components with an enable checkbox in their header.
pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Camera" => w.get::<&Camera>(e).ok().map(|c| c.enabled),
        "Light" => w.get::<&Light>(e).ok().map(|c| c.enabled),
        "Mesh Renderer" => w.get::<&MeshRenderer>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    match name {
        "Camera" => w.get::<&mut Camera>(e).map(|mut c| c.enabled = on).ok(),
        "Light" => w.get::<&mut Light>(e).map(|mut c| c.enabled = on).ok(),
        "Mesh Renderer" => w
            .get::<&mut MeshRenderer>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        _ => None,
    };
}

/// Draw the fields for component `name`. Returns true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Transform" => transform(ui, state, e),
        "Camera" => with::<Camera>(ui, state, e, camera),
        "Light" => with::<Light>(ui, state, e, light),
        "Mesh Filter" => with::<MeshFilter>(ui, state, e, mesh_filter),
        "Mesh Renderer" => with::<MeshRenderer>(ui, state, e, mesh_renderer),
        _ => {
            ui.weak("No editor for this component.");
            false
        }
    }
}

/// Edit a copy of component `T` and write it back if the UI changed it.
fn with<T: SceneComponent>(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    draw: fn(&mut Ui, &mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = draw(ui, &mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

fn transform(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let mut t = state.scene.transform(e);
    let mut pos = t.position.to_array();
    let mut rot = t.euler().to_array();
    let mut scale = t.scale.to_array();
    let mut changed = false;
    if vec3_row(ui, "Position", &mut pos, 0.1) {
        t.position = Vec3::from(pos);
        changed = true;
    }
    if vec3_row(ui, "Rotation", &mut rot, 1.0) {
        t.set_euler(Vec3::from(rot));
        changed = true;
    }
    if vec3_row(ui, "Scale", &mut scale, 0.01) {
        t.scale = Vec3::from(scale);
        changed = true;
    }
    if changed {
        state.scene.set_transform(e, t);
    }
    changed
}

/// Two-column property grid like the Inspector's label/value layout.
fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .width(150.0)
        .show_ui(ui, |ui| {
            for &o in options {
                changed |= ui.selectable_value(value, o, label(o)).changed();
            }
        });
    changed
}

/// sRGB float colour edited through egui's sRGB colour picker.
fn color_rgb(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut srgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = ui.color_edit_button_srgb(&mut srgb).changed();
    if changed {
        *c = srgb.map(|v| v as f32 / 255.0);
    }
    changed
}

fn camera(ui: &mut Ui, c: &mut Camera) -> bool {
    grid(ui, "camera_grid", |ui| {
        let mut ch = false;
        ui.label("Clear Flags");
        ch |= combo(
            ui,
            "cam_clear",
            &mut c.clear_flags,
            &ClearFlags::ALL,
            ClearFlags::label,
        );
        ui.end_row();
        ui.label("Background");
        let mut rgb = [c.background[0], c.background[1], c.background[2]];
        if color_rgb(ui, &mut rgb) {
            c.background[..3].copy_from_slice(&rgb);
            ch = true;
        }
        ui.end_row();
        ui.label("Culling Mask");
        ui.label("Everything");
        ui.end_row();
        ui.label("Projection");
        let mut ortho = c.orthographic;
        egui::ComboBox::from_id_salt("cam_proj")
            .selected_text(if ortho { "Orthographic" } else { "Perspective" })
            .width(150.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ortho, false, "Perspective");
                ui.selectable_value(&mut ortho, true, "Orthographic");
            });
        if ortho != c.orthographic {
            c.orthographic = ortho;
            ch = true;
        }
        ui.end_row();
        if c.orthographic {
            ui.label("Size");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut c.ortho_size)
                        .speed(0.05)
                        .range(0.01..=1e5),
                )
                .changed();
        } else {
            ui.label("Field of View");
            ch |= ui.add(egui::Slider::new(&mut c.fov, 1.0..=179.0)).changed();
        }
        ui.end_row();
        ui.label("Clipping Planes");
        ui.horizontal(|ui| {
            ui.label("Near");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut c.near)
                        .speed(0.01)
                        .range(0.01..=c.far - 0.01),
                )
                .changed();
            ui.label("Far");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut c.far)
                        .speed(1.0)
                        .range(c.near + 0.01..=1e7),
                )
                .changed();
        });
        ui.end_row();
        ui.label("Viewport Rect");
        ui.horizontal(|ui| {
            for (i, l) in ["X", "Y", "W", "H"].iter().enumerate() {
                ui.label(*l);
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut c.viewport[i])
                            .speed(0.01)
                            .range(0.0..=1.0),
                    )
                    .changed();
            }
        });
        ui.end_row();
        ui.label("Depth");
        ch |= ui
            .add(egui::DragValue::new(&mut c.depth).speed(0.1))
            .changed();
        ui.end_row();
        ch
    })
}

fn light(ui: &mut Ui, l: &mut Light) -> bool {
    grid(ui, "light_grid", |ui| {
        let mut ch = false;
        ui.label("Type");
        ch |= combo(
            ui,
            "light_type",
            &mut l.kind,
            &LightType::ALL,
            LightType::label,
        );
        ui.end_row();
        if l.kind != LightType::Directional {
            ui.label("Range");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut l.range)
                        .speed(0.1)
                        .range(0.0..=1e5),
                )
                .changed();
            ui.end_row();
        }
        if l.kind == LightType::Spot {
            ui.label("Spot Angle");
            ch |= ui
                .add(egui::Slider::new(&mut l.spot_angle, 1.0..=179.0))
                .changed();
            ui.end_row();
        }
        ui.label("Color");
        ch |= color_rgb(ui, &mut l.color);
        ui.end_row();
        ui.label("Intensity");
        ch |= ui
            .add(
                egui::DragValue::new(&mut l.intensity)
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ui.label("Shadow Type");
        ch |= combo(
            ui,
            "light_shadows",
            &mut l.shadows,
            &Shadows::ALL,
            Shadows::label,
        );
        ui.end_row();
        ch
    })
}

fn mesh_filter(ui: &mut Ui, f: &mut MeshFilter) -> bool {
    grid(ui, "meshfilter_grid", |ui| {
        let mut ch = false;
        ui.label("Mesh");
        egui::ComboBox::from_id_salt("mesh_filter_mesh")
            .selected_text(f.mesh.map(|m| m.name()).unwrap_or("None (Mesh)"))
            .width(150.0)
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut f.mesh, None, "None (Mesh)")
                    .changed();
                for m in PrimitiveMesh::ALL {
                    ch |= ui
                        .selectable_value(&mut f.mesh, Some(m), m.name())
                        .changed();
                }
            });
        ui.end_row();
        ch
    })
}

fn mesh_renderer(ui: &mut Ui, r: &mut MeshRenderer) -> bool {
    grid(ui, "meshrenderer_grid", |ui| {
        let mut ch = false;
        ui.label("Color");
        let mut rgb = [r.color[0], r.color[1], r.color[2]];
        if color_rgb(ui, &mut rgb) {
            r.color[..3].copy_from_slice(&rgb);
            ch = true;
        }
        ui.end_row();
        ui.label("Cast Shadows");
        egui::ComboBox::from_id_salt("mr_cast")
            .selected_text(if r.cast_shadows { "On" } else { "Off" })
            .width(150.0)
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, true, "On")
                    .changed();
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, false, "Off")
                    .changed();
            });
        ui.end_row();
        ui.label("Receive Shadows");
        ch |= ui.checkbox(&mut r.receive_shadows, "").changed();
        ui.end_row();
        ch
    })
}

/// Wide "Add Component" button with a searchable popup of addable components.
pub fn add_component_button(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let resp = ui
        .vertical_centered(|ui| ui.add_sized([230.0, 24.0], egui::Button::new("Add Component")))
        .inner;
    if resp.clicked() {
        state.inspector_add_open = !state.inspector_add_open;
    }
    if !state.inspector_add_open {
        return;
    }

    let search_id = egui::Id::new("add_component_search");
    let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
    let present = state.scene.component_names(e);
    let mut chosen = None;
    let area = egui::Area::new(egui::Id::new("add_component_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 2.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(resp.rect.width());
                let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text("🔍 Search"));
                field.request_focus();
                ui.separator();
                let needle = search.to_lowercase();
                let mut last_cat = "";
                for info in registry::components()
                    .iter()
                    .filter(|c| !c.category.is_empty())
                {
                    if !needle.is_empty() && !info.name.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if info.category != last_cat {
                        ui.label(RichText::new(info.category).color(theme::TEXT_DIM).small());
                        last_cat = info.category;
                    }
                    let has = present.iter().any(|p| p == info.name);
                    if ui
                        .add_enabled(!has, egui::Button::new(info.name).frame(false))
                        .clicked()
                    {
                        chosen = Some(info.name);
                    }
                }
                ui.add_enabled(
                    false,
                    egui::Button::new("New script… (step 8)").frame(false),
                );
            });
        });

    let clicked_outside = ui.input(|i| i.pointer.any_pressed())
        && !area.response.contains_pointer()
        && !resp.contains_pointer();
    if let Some(name) = chosen {
        state.push(Action::AddComponent(name));
    }
    if chosen.is_some() || clicked_outside || ui.input(|i| i.key_pressed(Key::Escape)) {
        state.inspector_add_open = false;
        search.clear();
    }
    ui.data_mut(|d| d.insert_temp(search_id, search));
}
