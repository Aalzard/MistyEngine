//! The New Shape window: pick a shape, set its parameters, create it at the Scene view pivot.

use egui::Ui;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::shapes::Shape;
use misty_engine::scene::{MeshCollider, MeshFilter, MeshRenderer, Transform};

use crate::state::EditorState;

/// Open New Shape window state: the chosen shape with its parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct NewShape {
    pub shape: Shape,
}

impl Default for NewShape {
    fn default() -> Self {
        Self {
            shape: Shape::ALL[0],
        }
    }
}

/// GameObject ▸ 3D Object ▸ Mesh Builder Shape…, or the strip's "+".
pub fn open_new_shape(state: &mut EditorState) {
    if state.mesh_builder.new_shape.is_none() {
        state.mesh_builder.new_shape = Some(NewShape::default());
    }
}

/// Create a shape object (Mesh Filter, Mesh Renderer, Editable Mesh, Mesh Collider) at the
/// Scene view pivot and select it. One undo step.
pub fn create_shape(state: &mut EditorState, shape: Shape) -> Entity {
    state.record(&format!("Create {}", shape.name()));
    let name = state.scene.unique_name(shape.name(), None);
    let e = state.scene.spawn(&name, None);
    let mut pivot = state.scene_view.camera.pivot().round();
    pivot.y = pivot.y.max(0.0);
    state
        .scene
        .set_transform(e, Transform::from_position(pivot));
    state.scene.add(e, MeshFilter::default());
    state.scene.add(e, MeshRenderer::default());
    state.scene.add(e, shape.build());
    state.scene.add(e, MeshCollider::default());
    misty_engine::mesh_builder::sync(&state.scene);
    state.select(e);
    e
}

/// Draw the window while open.
pub fn window(ctx: &egui::Context, state: &mut EditorState) {
    let Some(mut ns) = state.mesh_builder.new_shape.clone() else {
        return;
    };
    let mut open = true;
    let mut create = false;
    egui::Window::new("New Shape")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_width(260.0)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for s in Shape::ALL {
                    let on = std::mem::discriminant(&s) == std::mem::discriminant(&ns.shape);
                    if ui.selectable_label(on, s.name()).clicked() && !on {
                        ns.shape = s;
                    }
                }
            });
            ui.separator();
            egui::Grid::new("new_shape_params")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| params(ui, &mut ns.shape));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    create = true;
                }
                if ui.button("Reset").clicked() {
                    ns = NewShape {
                        shape: Shape::ALL
                            .into_iter()
                            .find(|s| s.name() == ns.shape.name())
                            .unwrap_or(Shape::ALL[0]),
                    };
                }
            });
        });
    if create {
        create_shape(state, ns.shape);
    }
    state.mesh_builder.new_shape = (open && !create).then_some(ns);
}

fn length(ui: &mut Ui, label: &str, v: &mut f32) {
    ui.label(label);
    ui.add(
        egui::DragValue::new(v)
            .speed(0.02)
            .range(0.01..=1000.0)
            .suffix(" m"),
    );
    ui.end_row();
}

fn count(ui: &mut Ui, label: &str, v: &mut u32, min: u32, max: u32) {
    ui.label(label);
    ui.add(egui::DragValue::new(v).range(min..=max));
    ui.end_row();
}

/// Parameter rows of a shape.
pub fn params(ui: &mut Ui, shape: &mut Shape) {
    match shape {
        Shape::Cube { size } => {
            let mut s: Vec3 = *size;
            length(ui, "Width", &mut s.x);
            length(ui, "Height", &mut s.y);
            length(ui, "Depth", &mut s.z);
            *size = s;
        }
        Shape::Plane {
            width,
            depth,
            segments,
        } => {
            length(ui, "Width", width);
            length(ui, "Depth", depth);
            count(ui, "Segments", segments, 1, 64);
        }
        Shape::Cylinder {
            radius,
            height,
            sides,
        } => {
            length(ui, "Radius", radius);
            length(ui, "Height", height);
            count(ui, "Sides", sides, 3, 128);
        }
        Shape::Stairs {
            width,
            height,
            depth,
            steps,
        } => {
            length(ui, "Width", width);
            length(ui, "Height", height);
            length(ui, "Depth", depth);
            count(ui, "Steps", steps, 1, 64);
        }
        Shape::Arch {
            radius,
            thickness,
            depth,
            segments,
        } => {
            length(ui, "Radius", radius);
            length(ui, "Thickness", thickness);
            length(ui, "Depth", depth);
            count(ui, "Segments", segments, 2, 64);
        }
        Shape::Prism {
            width,
            height,
            depth,
        } => {
            length(ui, "Width", width);
            length(ui, "Height", height);
            length(ui, "Depth", depth);
        }
        Shape::Torus {
            radius,
            tube,
            rings,
            sides,
        } => {
            length(ui, "Radius", radius);
            length(ui, "Tube Radius", tube);
            count(ui, "Rings", rings, 3, 128);
            count(ui, "Sides", sides, 3, 64);
        }
    }
}
