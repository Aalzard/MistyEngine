//! Scene view gizmos for stamps and areas: outlines draped on the terrain (radius, falloff,
//! river banks, zone polygons) and drag handles for the radius, falloff and spline/polygon points.
//! Moving, rotating and scaling a stamp uses the normal transform tools.

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::{Vec2, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::terrain::{TerrainData, store};
use misty_engine::world_gen::stamps::{self, Placement};
use misty_engine::world_gen::{self, StampKind, WorldBuilder, WorldStamp};

use crate::gizmos::Lines;
use crate::picking::{ray, to_screen};
use crate::state::EditorState;

const PICK_PX: f32 = 8.0;

/// What a handle changes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Radius,
    Falloff,
    /// River control point.
    Point(usize),
    /// Biome Zone polygon vertex.
    Vertex(usize),
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    entity: Entity,
    part: Part,
}

fn drag_id() -> egui::Id {
    egui::Id::new("misty_world_builder_drag")
}

/// A stamp's frame on its terrain.
struct Frame {
    at: Placement,
    corner: Vec3,
    data: Option<std::sync::Arc<TerrainData>>,
}

impl Frame {
    fn of(state: &EditorState, e: Entity) -> Option<Frame> {
        let builder = world_gen::builder_of(&state.scene, e)?;
        let (corner, data) = match world_gen::terrain_of(&state.scene, builder) {
            Some(t) => {
                let path = state
                    .scene
                    .world
                    .get::<&WorldBuilder>(builder)
                    .ok()?
                    .terrain_data
                    .clone();
                (
                    state.scene.world_position(t),
                    store::get(&path).map(|(d, _)| d),
                )
            }
            None => (Vec3::ZERO, None),
        };
        Some(Frame {
            at: Placement::from_matrix(&state.scene.world_matrix(e), corner),
            corner,
            data,
        })
    }

    /// World point on the ground above terrain-local `p` (or at the stamp's height).
    fn ground(&self, p: Vec2) -> Vec3 {
        let y = match &self.data {
            Some(d) => d.sample_height(p.x, p.y) + 0.4,
            None => self.at.y,
        };
        self.corner + Vec3::new(p.x, y, p.y)
    }

    fn local_to_world(&self, l: Vec2) -> Vec3 {
        self.ground(self.at.to_terrain(l))
    }
}

/// Show stamp gizmos while the World Builder window is open or a stamp is selected.
fn visible(state: &EditorState, ctx: &egui::Context) -> bool {
    super::is_open(state, ctx)
        || state
            .selection
            .list()
            .iter()
            .any(|e| state.scene.world.get::<&WorldStamp>(*e).is_ok())
}

/// Outlines of every stamp (the selected ones brighter).
pub fn lines(lines: &mut Lines, state: &EditorState, ctx: &egui::Context) {
    if !visible(state, ctx) {
        return;
    }
    for builder in world_gen::builders(&state.scene) {
        for e in world_gen::stamps_of(&state.scene, builder) {
            if !state.scene.active_in_hierarchy(e) {
                continue;
            }
            let Ok(s) = state
                .scene
                .world
                .get::<&WorldStamp>(e)
                .map(|s| (*s).clone())
            else {
                continue;
            };
            let Some(f) = Frame::of(state, e) else {
                continue;
            };
            let mut color = s.kind.color();
            if !state.selection.contains(e) {
                color[3] = 0.45;
            }
            let dim = [color[0], color[1], color[2], color[3] * 0.5];
            let ring = |lines: &mut Lines, r: f32, c: [f32; 4]| {
                let pts: Vec<Vec3> = (0..=48)
                    .map(|i| {
                        let a = i as f32 / 48.0 * std::f32::consts::TAU;
                        f.local_to_world(Vec2::new(a.cos(), a.sin()) * r)
                    })
                    .collect();
                polyline(lines, &pts, c);
            };
            match &s.kind {
                StampKind::River(r) => {
                    let line = stamps::river_line(
                        &stamps::PlacedStamp {
                            stamp: s.clone(),
                            at: f.at,
                            seed: 0,
                        },
                        r,
                    );
                    let centre: Vec<Vec3> = line.iter().map(|p| f.ground(*p)).collect();
                    polyline(lines, &centre, color);
                    for side in [-1.0, 1.0] {
                        for (off, c) in [(r.width * 0.5, color), (r.width * 0.5 + r.bank, dim)] {
                            let edge: Vec<Vec3> = offset_line(&line, off * side)
                                .into_iter()
                                .map(|p| f.ground(p))
                                .collect();
                            polyline(lines, &edge, c);
                        }
                    }
                }
                StampKind::BiomeZone(z) if z.polygon.len() >= 3 => {
                    let mut pts: Vec<Vec3> = z
                        .polygon
                        .iter()
                        .map(|p| f.local_to_world(Vec2::from(*p)))
                        .collect();
                    pts.push(pts[0]);
                    polyline(lines, &pts, color);
                }
                StampKind::Water(_) => {
                    // Flat ring at the water level.
                    let pts: Vec<Vec3> = (0..=48)
                        .map(|i| {
                            let a = i as f32 / 48.0 * std::f32::consts::TAU;
                            let p = f.at.to_terrain(Vec2::new(a.cos(), a.sin()) * s.radius);
                            f.corner + Vec3::new(p.x, f.at.y, p.y)
                        })
                        .collect();
                    polyline(lines, &pts, color);
                    let c = f.corner + Vec3::new(f.at.center.x, f.at.y, f.at.center.y);
                    lines.line(c, f.ground(f.at.center), dim);
                }
                _ => {
                    ring(lines, s.radius, color);
                    if s.falloff > 0.01 {
                        ring(lines, s.radius * (1.0 - s.falloff), dim);
                    }
                }
            }
        }
    }
}

fn polyline(lines: &mut Lines, pts: &[Vec3], color: [f32; 4]) {
    for w in pts.windows(2) {
        lines.line(w[0], w[1], color);
    }
}

/// A line pushed sideways by `d` (positive = left of the direction of travel).
fn offset_line(line: &[Vec2], d: f32) -> Vec<Vec2> {
    let n = line.len();
    (0..n)
        .map(|i| {
            let a = line[i.saturating_sub(1)];
            let b = line[(i + 1).min(n - 1)];
            let t = (b - a).try_normalize().unwrap_or(Vec2::X);
            line[i] + Vec2::new(-t.y, t.x) * d
        })
        .collect()
}

/// Handles of the selected stamp: (part, world position).
/// The selected stamp, its frame, settings and handles.
type Handles = (Entity, Frame, WorldStamp, Vec<(Part, Vec3)>);

fn handles(state: &EditorState) -> Option<Handles> {
    let e = state.selection.active()?;
    let s = (*state.scene.world.get::<&WorldStamp>(e).ok()?).clone();
    let f = Frame::of(state, e)?;
    let mut list = Vec::new();
    match &s.kind {
        StampKind::River(r) => {
            for (i, p) in r.points.iter().enumerate() {
                list.push((Part::Point(i), f.local_to_world(Vec2::from(*p))));
            }
        }
        StampKind::BiomeZone(z) if z.polygon.len() >= 3 => {
            for (i, p) in z.polygon.iter().enumerate() {
                list.push((Part::Vertex(i), f.local_to_world(Vec2::from(*p))));
            }
        }
        _ => {
            list.push((Part::Radius, f.local_to_world(Vec2::new(s.radius, 0.0))));
            if !matches!(s.kind, StampKind::Water(_)) {
                let inner = s.radius * (1.0 - s.falloff);
                list.push((Part::Falloff, f.local_to_world(Vec2::new(0.0, inner))));
            }
        }
    }
    Some((e, f, s, list))
}

/// Draw and drive the handles. True when they own the mouse this frame.
pub fn show(ui: &Ui, state: &mut EditorState, camera: &Camera, rect: Rect, hovered: bool) -> bool {
    let id = drag_id();
    let Some((e, frame, stamp, list)) = handles(state) else {
        ui.ctx().data_mut(|d| d.remove::<Drag>(id));
        return false;
    };
    let (mouse, pressed, down) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
        )
    });
    let screen: Vec<(Part, Pos2)> = list
        .iter()
        .filter_map(|(p, w)| Some((*p, to_screen(camera, rect, *w)?)))
        .collect();
    let hot = mouse
        .filter(|m| hovered && rect.contains(*m))
        .and_then(|m| {
            screen
                .iter()
                .map(|(p, s)| (*p, s.distance(m)))
                .filter(|(_, d)| *d < PICK_PX)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(p, _)| p)
        });
    let mut owned = hot.is_some();
    if pressed
        && let Some(part) = hot
        && ui.ctx().data(|d| d.get_temp::<Drag>(id)).is_none()
    {
        ui.ctx()
            .data_mut(|d| d.insert_temp(id, Drag { entity: e, part }));
        state.handles.suppress_click = true;
    }
    if let Some(d) = ui.ctx().data(|data| data.get_temp::<Drag>(id)) {
        owned = true;
        if down && d.entity == e {
            if let Some(m) = mouse
                && let Some(p) = ground_hit(camera, rect, m, &frame)
            {
                apply(state, e, &stamp, d.part, frame.at.to_local(p));
                ui.ctx().request_repaint();
            }
        } else {
            ui.ctx().data_mut(|data| data.remove::<Drag>(id));
        }
    }
    let color = stamp.kind.color();
    let stroke = Color32::from_rgb(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
    );
    let painter = ui.painter().with_clip_rect(rect);
    for (part, p) in &screen {
        let fill = if Some(*part) == hot {
            Color32::from_rgb(255, 230, 0)
        } else {
            Color32::WHITE
        };
        painter.circle_filled(*p, 4.5, fill);
        painter.circle_stroke(*p, 4.5, Stroke::new(1.5, stroke));
    }
    owned
}

/// Terrain-local x/z under the mouse: on the terrain, else on the stamp's height plane.
fn ground_hit(camera: &Camera, rect: Rect, at: Pos2, f: &Frame) -> Option<Vec2> {
    let (o, d) = ray(camera, rect, at);
    if let Some(data) = &f.data
        && let Some(p) = misty_engine::terrain::raycast_data(data, o - f.corner, d)
    {
        return Some(Vec2::new(p.x, p.z));
    }
    let y = f.corner.y + f.at.y;
    if d.y.abs() < 1e-6 {
        return None;
    }
    let t = (y - o.y) / d.y;
    (t > 0.0).then(|| {
        let p = o + d * t - f.corner;
        Vec2::new(p.x, p.z)
    })
}

/// Apply a handle drag (`l` = stamp-local point under the mouse).
fn apply(state: &mut EditorState, e: Entity, stamp: &WorldStamp, part: Part, l: Vec2) {
    let mut s = stamp.clone();
    match (part, &mut s.kind) {
        (Part::Radius, _) => s.radius = l.length().max(0.5),
        (Part::Falloff, _) => s.falloff = (1.0 - l.length() / s.radius.max(1e-3)).clamp(0.0, 1.0),
        (Part::Point(i), StampKind::River(r)) if i < r.points.len() => r.points[i] = l.into(),
        (Part::Vertex(i), StampKind::BiomeZone(z)) if i < z.polygon.len() => {
            z.polygon[i] = l.into()
        }
        _ => return,
    }
    let _ = state.scene.world.insert_one(e, s);
    state.mark_edited("World Stamp");
}
