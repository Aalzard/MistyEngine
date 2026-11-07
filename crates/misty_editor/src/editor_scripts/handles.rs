//! Scene-view handles for custom Inspectors: `OnSceneGUI` runs while the inspected object is
//! the active selection, and `Handles.Position(p)` draws a three-axis move handle at `p` and
//! returns where it was dragged to.
//!
//! ```lua
//! function OrbiterInspector:OnSceneGUI()
//!     self.target.center = Handles.Position(self.target.center)
//!     Handles.Label(self.target.center, "centre")
//! end
//! ```
//!
//! Assigning the result to a field of `self.target` is an undoable Inspector edit (nothing is
//! recorded while the handle stays put).

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Ui};
use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scripting::api::LVec3;
use mlua::{Function, Lua, Result};

use crate::handles::{arrow_head, closest_on_axis, dist_to_segment};
use crate::picking::{ray, to_screen};
use crate::state::EditorState;
use crate::theme;

const AXIS_PX: f32 = 80.0;
const PICK_PX: f32 = 7.0;
const HOT: Color32 = Color32::from_rgb(255, 230, 0);

/// The Scene view while `OnSceneGUI` runs (Lua app data).
struct HandleCx {
    ui: *const Ui,
    camera: Camera,
    rect: Rect,
    hovered: bool,
    /// Handles drawn so far this call (each one's id).
    next: usize,
    /// The object whose Inspector draws (handle ids include it).
    owner: Entity,
    /// A handle is hot or dragged: the transform handles and clicks stay off.
    claimed: bool,
}

/// A handle drag in progress.
#[derive(Clone, Copy, Debug)]
struct Drag {
    axis: usize,
    start: Vec3,
    grab: f32,
}

fn with_cx<R>(lua: &Lua, f: impl FnOnce(&mut HandleCx, &Ui) -> R) -> Result<R> {
    let mut cx = lua
        .app_data_mut::<HandleCx>()
        .ok_or_else(|| mlua::Error::runtime("Handles only work inside OnSceneGUI"))?;
    // SAFETY: `scene_gui` sets the pointer from a `&Ui` it holds (unused) for the whole call
    // and removes the context before returning.
    let ui = unsafe { &*cx.ui };
    Ok(f(&mut cx, ui))
}

pub fn register(lua: &Lua) -> Result<()> {
    let h = lua.create_table()?;
    h.set(
        "Position",
        lua.create_function(|lua, p: LVec3| with_cx(lua, |cx, ui| LVec3(position(cx, ui, p.0))))?,
    )?;
    h.set(
        "Label",
        lua.create_function(|lua, (p, text): (LVec3, String)| {
            with_cx(lua, |cx, ui| {
                if let Some(at) = to_screen(&cx.camera, cx.rect, p.0) {
                    ui.painter().with_clip_rect(cx.rect).text(
                        at + egui::vec2(8.0, -8.0),
                        Align2::LEFT_BOTTOM,
                        text,
                        FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                }
            })
        })?,
    )?;
    lua.globals().set("Handles", h)
}

/// Draw and drag one move handle at `p`; returns the new position.
fn position(cx: &mut HandleCx, ui: &Ui, p: Vec3) -> Vec3 {
    cx.next += 1;
    let id = egui::Id::new(("misty_lua_handle", cx.owner, cx.next));
    let (camera, rect) = (cx.camera, cx.rect);
    let Some(origin) = to_screen(&camera, rect, p) else {
        return p;
    };
    let len = camera.world_per_pixel(p, rect.height()) * AXIS_PX;
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let tips: Vec<Option<Pos2>> = axes
        .iter()
        .map(|a| to_screen(&camera, rect, p + *a * len))
        .collect();
    let (mouse, pressed, down) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
        )
    });
    let mut drag = ui.ctx().data(|d| d.get_temp::<Drag>(id));
    let hot = mouse
        .filter(|m| cx.hovered && rect.contains(*m))
        .and_then(|m| {
            (0..3)
                .filter_map(|i| Some((i, dist_to_segment(m, origin, tips[i]?))))
                .filter(|(_, d)| *d < PICK_PX)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
    let param = |m: Pos2, axis: usize| {
        let (o, d) = ray(&camera, rect, m);
        closest_on_axis(p, axes[axis], o, d)
    };
    if pressed
        && drag.is_none()
        && let (Some(axis), Some(m)) = (hot, mouse)
        && let Some(grab) = param(m, axis)
    {
        drag = Some(Drag {
            axis,
            start: p,
            grab,
        });
    }
    let mut out = p;
    match (drag, mouse) {
        (Some(d), Some(m)) if down => {
            // Measured from where the drag started, so the handle follows the mouse exactly.
            let (o, dir) = ray(&camera, rect, m);
            if let Some(t) = closest_on_axis(d.start, axes[d.axis], o, dir) {
                out = d.start + axes[d.axis] * (t - d.grab);
            }
            ui.ctx().data_mut(|data| data.insert_temp(id, d));
        }
        _ => {
            ui.ctx().data_mut(|data| data.remove::<Drag>(id));
            drag = None;
        }
    }
    cx.claimed |= hot.is_some() || drag.is_some();
    let painter = ui.painter().with_clip_rect(rect);
    let colors = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z];
    for i in 0..3 {
        let Some(tip) = tips[i] else { continue };
        let active = drag.is_some_and(|d| d.axis == i) || (drag.is_none() && hot == Some(i));
        let c = if active { HOT } else { colors[i] };
        painter.line_segment([origin, tip], Stroke::new(2.0, c));
        arrow_head(&painter, origin, tip, c);
    }
    painter.circle_filled(origin, 3.0, Color32::WHITE);
    out
}

/// Run `OnSceneGUI` of the custom Inspectors of the active selection's scripts. True while a
/// handle is hot or dragged (the transform handles and selection clicks stay off then).
pub fn scene_gui(
    ui: &Ui,
    state: &mut EditorState,
    camera: &Camera,
    rect: Rect,
    hovered: bool,
) -> bool {
    let es = &state.editor_scripts;
    if !es.active() || es.inspectors.is_empty() || state.play.in_play_mode() {
        return false;
    }
    let Some(e) = state.selection.active() else {
        return false;
    };
    let scripts = state
        .scene
        .world
        .get::<&misty_engine::scene::Scripts>(e)
        .map(|s| s.list.clone())
        .unwrap_or_default();
    let targets: Vec<(usize, String, String)> = scripts
        .iter()
        .enumerate()
        .filter(|(_, s)| es.inspectors.contains(&s.name()))
        .map(|(slot, s)| (slot, s.name(), s.path.clone()))
        .collect();
    if targets.is_empty() {
        return false;
    }
    let claimed = super::call(state, super::FRAME_BUDGET, |lua| {
        lua.set_app_data(HandleCx {
            ui: ui as *const Ui,
            camera: *camera,
            rect,
            hovered,
            next: 0,
            owner: e,
            claimed: false,
        });
        for (slot, name, path) in &targets {
            let site = format!("scene:{name}");
            if super::with_state(lua, |st| st.editor_scripts.is_broken(&site)).unwrap_or(false) {
                continue;
            }
            let result = super::inspector::instance(lua, e, *slot, name, path).and_then(|inst| {
                match inst.get::<Option<Function>>("OnSceneGUI")? {
                    Some(f) => f.call::<()>(inst),
                    None => Ok(()),
                }
            });
            if let Err(err) = result {
                let _ = super::with_state(lua, |st| super::fail(st, &site, &err));
            }
        }
        lua.remove_app_data::<HandleCx>()
            .is_some_and(|cx| cx.claimed)
    });
    let claimed = claimed.unwrap_or(false);
    if claimed && ui.input(|i| i.pointer.primary_pressed()) {
        state.handles.suppress_click = true;
    }
    claimed
}
