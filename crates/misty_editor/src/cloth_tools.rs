//! Cloth ▸ Edit Constraints: paint the max distance of a Cloth's vertices in the Scene view.
//!
//! While editing, every vertex is drawn as a dot (red = pinned, yellow to green = how far it
//! may move, grey = free) and the left mouse button paints the brush value onto the vertices
//! under the brush circle; Shift paints "free". One undo step per stroke.

use std::cell::RefCell;

use egui::{Color32, Pos2, Rect, Response, Stroke, Ui};
use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Cloth, SkinnedMeshRenderer};

use crate::picking::to_screen;
use crate::state::EditorState;

/// Brush settings and the cloth being edited.
#[derive(Clone, Copy, Debug)]
pub struct Brush {
    pub target: Option<Entity>,
    /// Max distance painted (metres; 0 pins).
    pub value: f32,
    /// Paint "free" instead of `value`.
    pub free: bool,
    /// Brush radius in points.
    pub radius: f32,
    painting: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            target: None,
            value: 0.0,
            free: false,
            radius: 24.0,
            painting: false,
        }
    }
}

thread_local! {
    static BRUSH: RefCell<Brush> = RefCell::new(Brush::default());
}

pub fn brush() -> Brush {
    BRUSH.with(|b| *b.borrow())
}

fn set_brush(f: impl FnOnce(&mut Brush)) {
    BRUSH.with(|b| f(&mut b.borrow_mut()));
}

/// The cloth whose constraints are being painted.
pub fn editing() -> Option<Entity> {
    brush().target
}

pub fn set_editing(target: Option<Entity>) {
    set_brush(|b| {
        b.target = target;
        b.painting = false;
    });
}

/// The edited cloth, if it still exists and has a mesh.
fn target(state: &EditorState) -> Option<Entity> {
    let e = editing()?;
    (state.scene.contains(e)
        && state.scene.world.get::<&Cloth>(e).is_ok()
        && state.scene.world.get::<&SkinnedMeshRenderer>(e).is_ok())
    .then_some(e)
}

/// Is a constraint brush taking the mouse in the Scene view?
pub fn active(state: &EditorState) -> bool {
    target(state).is_some() && !state.play.in_play_mode()
}

/// World positions of the cloth's mesh vertices as drawn now (the skinned pose).
pub fn vertex_positions(state: &EditorState, e: Entity) -> Vec<Vec3> {
    let s = &state.scene;
    let Ok(r) = s.world.get::<&SkinnedMeshRenderer>(e).map(|r| (*r).clone()) else {
        return Vec::new();
    };
    let world = s.world_matrix(e);
    let local: Vec<Vec3> = misty_engine::skinning::compute_pose(s, e, &r, &|x| s.world_matrix(x))
        .and_then(|p| misty_engine::skinning::deform_pose(&p))
        .map(|v| v.iter().map(|v| Vec3::from(v.position)).collect())
        .or_else(|| {
            let m = misty_engine::models::mesh(r.mesh.as_ref()?)?;
            Some(m.vertices.iter().map(|v| Vec3::from(v.position)).collect())
        })
        .unwrap_or_default();
    local.iter().map(|p| world.transform_point3(*p)).collect()
}

/// Paint `value` (or free) onto the vertices within `radius` of `at` on screen. Returns how
/// many changed.
pub fn paint(
    cloth: &mut Cloth,
    screen: &[Option<Pos2>],
    at: Pos2,
    radius: f32,
    value: Option<f32>,
) -> usize {
    if cloth.max_distances.len() < screen.len() {
        cloth.resize_max_distances(screen.len());
    }
    let v = value.unwrap_or(Cloth::FREE);
    let mut n = 0;
    for (i, p) in screen.iter().enumerate() {
        if let Some(p) = p
            && p.distance(at) <= radius
            && cloth.max_distances[i] != v
        {
            cloth.max_distances[i] = v;
            n += 1;
        }
    }
    n
}

/// Scene view input: paints while the left button is down. True when the click was used.
pub fn scene_view(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    response: &Response,
) -> bool {
    if !active(state) {
        return false;
    }
    let Some(e) = target(state) else {
        return false;
    };
    let b = brush();
    let (down, pressed, shift) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.modifiers.shift,
        )
    });
    let Some(at) = response.hover_pos() else {
        if !down {
            set_brush(|b| b.painting = false);
        }
        return false;
    };
    if !rect.contains(at) {
        return false;
    }
    if pressed {
        state.record("Paint Cloth Constraints");
        set_brush(|b| b.painting = true);
    }
    if !down {
        set_brush(|b| b.painting = false);
        return false;
    }
    if !brush().painting {
        return false;
    }
    let screen: Vec<Option<Pos2>> = vertex_positions(state, e)
        .iter()
        .map(|p| to_screen(camera, rect, *p))
        .collect();
    let Ok(mut cloth) = state.scene.world.get::<&Cloth>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let value = (!(b.free || shift)).then_some(b.value.max(0.0));
    if paint(&mut cloth, &screen, at, b.radius, value) > 0 {
        let _ = state.scene.world.insert_one(e, cloth);
        state.scene_dirty = true;
    }
    true
}

/// Dot colour of a max distance (`scale` = the largest limited value).
pub fn dot_color(max_distance: f32, scale: f32) -> Color32 {
    if max_distance < 0.0 {
        Color32::from_gray(150)
    } else if max_distance == 0.0 {
        Color32::from_rgb(235, 60, 50)
    } else {
        let t = (max_distance / scale.max(1e-3)).clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
        Color32::from_rgb(lerp(240.0, 70.0), lerp(210.0, 200.0), lerp(60.0, 90.0))
    }
}

/// Draw the vertex dots and the brush over the Scene view.
pub fn overlay(ui: &Ui, state: &EditorState, camera: &Camera, rect: Rect) {
    let Some(e) = target(state).filter(|_| !state.play.in_play_mode()) else {
        return;
    };
    let Ok(cloth) = state.scene.world.get::<&Cloth>(e).map(|c| (*c).clone()) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let scale = cloth.max_distances.iter().copied().fold(0.0f32, f32::max);
    for (i, p) in vertex_positions(state, e).iter().enumerate() {
        let Some(s) = to_screen(camera, rect, *p) else {
            continue;
        };
        let d = cloth.max_distances.get(i).copied().unwrap_or(Cloth::FREE);
        painter.circle_filled(s, 3.0, dot_color(d, scale));
    }
    let b = brush();
    if let Some(at) = ui.input(|i| i.pointer.hover_pos())
        && rect.contains(at)
    {
        painter.circle_stroke(
            at,
            b.radius,
            Stroke::new(1.5, Color32::from_rgb(90, 170, 255)),
        );
    }
    let text = if b.free {
        "Painting: free (Shift)".to_owned()
    } else {
        format!("Painting max distance {:.2} m (Shift: free)", b.value)
    };
    painter.text(
        rect.left_bottom() + egui::vec2(10.0, -12.0),
        egui::Align2::LEFT_BOTTOM,
        text,
        egui::FontId::proportional(13.0),
        Color32::WHITE,
    );
}

/// Brush controls (in the Cloth Inspector while editing).
pub fn brush_ui(ui: &mut Ui) {
    let mut b = brush();
    egui::Grid::new("cloth_brush")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Max Distance");
            ui.add(
                egui::DragValue::new(&mut b.value)
                    .speed(0.01)
                    .range(0.0..=100.0)
                    .suffix(" m"),
            );
            ui.end_row();
            ui.label("Paint Free");
            ui.checkbox(&mut b.free, "");
            ui.end_row();
            ui.label("Brush Size");
            ui.add(egui::Slider::new(&mut b.radius, 4.0..=200.0));
            ui.end_row();
        });
    set_brush(|x| {
        x.value = b.value;
        x.free = b.free;
        x.radius = b.radius;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painting_sets_the_vertices_under_the_brush() {
        let mut c = Cloth::default();
        let screen = vec![
            Some(Pos2::new(10.0, 10.0)),
            Some(Pos2::new(15.0, 10.0)),
            Some(Pos2::new(80.0, 10.0)),
            None,
        ];
        assert_eq!(
            paint(&mut c, &screen, Pos2::new(12.0, 10.0), 8.0, Some(0.0)),
            2
        );
        assert_eq!(c.max_distances, vec![0.0, 0.0, Cloth::FREE, Cloth::FREE]);
        assert_eq!(
            paint(&mut c, &screen, Pos2::new(80.0, 12.0), 5.0, Some(0.3)),
            1
        );
        assert_eq!(c.max_distance(2), Some(0.3));
        // Painting free again.
        assert_eq!(paint(&mut c, &screen, Pos2::new(10.0, 10.0), 1.0, None), 1);
        assert_eq!(c.max_distance(0), None);
        assert_eq!(dot_color(0.0, 1.0), Color32::from_rgb(235, 60, 50));
        assert_eq!(dot_color(-1.0, 1.0), Color32::from_gray(150));
    }
}
