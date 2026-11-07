//! Script gizmos: `OnDrawGizmos` (every object) and `OnDrawGizmosSelected` (selected objects)
//! on game script components, drawn in the Scene view while editing.
//!
//! ```lua
//! function Orbiter:OnDrawGizmos()
//!     Gizmos.color = Color.new(1, 0.8, 0.2)
//!     Gizmos.WireSphere(self.center, self.radius)
//! end
//! ```
//!
//! `Gizmos.Line(a, b)`, `Gizmos.Ray(from, direction)`, `Gizmos.WireSphere(center, radius)`,
//! `Gizmos.WireCube(center, size)`, `Gizmos.Icon(position, glyph)` and
//! `Gizmos.Label(position, text)`, in the colour of `Gizmos.color`.

use egui::{Align2, Color32, FontId, Rect, Ui};
use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{ScriptRef, Scripts};
use misty_engine::scripting::api::{LVec3, with_world};
use misty_engine::scripting::ui_api::LColor;
use mlua::{Function, Lua, Result, Table, Value};

use crate::gizmos::Lines;
use crate::state::EditorState;

/// What the script gizmos drew this frame.
#[derive(Clone, Debug, Default)]
pub struct GizmoFrame {
    pub lines: Vec<(Vec3, Vec3, [f32; 4])>,
    pub icons: Vec<(Vec3, String, [f32; 4])>,
    pub labels: Vec<(Vec3, String, [f32; 4])>,
}

/// Collects gizmos while the gizmo callbacks run (Lua app data).
struct Sink {
    color: [f32; 4],
    frame: GizmoFrame,
}

const DEFAULT_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Lines one frame of script gizmos may add (a runaway loop stops drawing, not the editor).
const MAX_LINES: usize = 200_000;

fn with_sink<R>(lua: &Lua, f: impl FnOnce(&mut Sink) -> R) -> Result<R> {
    let mut s = lua.app_data_mut::<Sink>().ok_or_else(|| {
        mlua::Error::runtime("Gizmos only draw inside OnDrawGizmos or OnDrawGizmosSelected")
    })?;
    Ok(f(&mut s))
}

fn line(s: &mut Sink, a: Vec3, b: Vec3) {
    if s.frame.lines.len() < MAX_LINES {
        s.frame.lines.push((a, b, s.color));
    }
}

fn circle(s: &mut Sink, c: Vec3, r: f32, u: Vec3, v: Vec3) {
    const N: usize = 32;
    let p = |i: usize| {
        let a = i as f32 / N as f32 * std::f32::consts::TAU;
        c + (u * a.cos() + v * a.sin()) * r
    };
    for i in 0..N {
        line(s, p(i), p(i + 1));
    }
}

pub fn register(lua: &Lua) -> Result<()> {
    let g = lua.create_table()?;
    g.set(
        "Line",
        lua.create_function(|lua, (a, b): (LVec3, LVec3)| with_sink(lua, |s| line(s, a.0, b.0)))?,
    )?;
    g.set(
        "Ray",
        lua.create_function(|lua, (a, d): (LVec3, LVec3)| {
            with_sink(lua, |s| line(s, a.0, a.0 + d.0))
        })?,
    )?;
    g.set(
        "WireSphere",
        lua.create_function(|lua, (c, r): (LVec3, f32)| {
            with_sink(lua, |s| {
                circle(s, c.0, r, Vec3::X, Vec3::Y);
                circle(s, c.0, r, Vec3::X, Vec3::Z);
                circle(s, c.0, r, Vec3::Y, Vec3::Z);
            })
        })?,
    )?;
    g.set(
        "WireCube",
        lua.create_function(|lua, (c, size): (LVec3, LVec3)| {
            with_sink(lua, |s| {
                let h = size.0 * 0.5;
                let k = |x: f32, y: f32, z: f32| c.0 + Vec3::new(x, y, z) * h;
                let corners = [
                    k(-1., -1., -1.),
                    k(1., -1., -1.),
                    k(1., 1., -1.),
                    k(-1., 1., -1.),
                    k(-1., -1., 1.),
                    k(1., -1., 1.),
                    k(1., 1., 1.),
                    k(-1., 1., 1.),
                ];
                for (a, b) in [
                    (0, 1),
                    (1, 2),
                    (2, 3),
                    (3, 0),
                    (4, 5),
                    (5, 6),
                    (6, 7),
                    (7, 4),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    line(s, corners[a], corners[b]);
                }
            })
        })?,
    )?;
    g.set(
        "Icon",
        lua.create_function(|lua, (p, glyph): (LVec3, Option<String>)| {
            with_sink(lua, |s| {
                let glyph = glyph.unwrap_or_else(|| "◆".into());
                s.frame.icons.push((p.0, glyph, s.color));
            })
        })?,
    )?;
    g.set(
        "Label",
        lua.create_function(|lua, (p, text): (LVec3, String)| {
            with_sink(lua, |s| s.frame.labels.push((p.0, text, s.color)))
        })?,
    )?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, key): (Table, String)| -> Result<Value> {
            if key != "color" {
                return Ok(Value::Nil);
            }
            let c = with_sink(lua, |s| s.color).unwrap_or(DEFAULT_COLOR);
            Ok(Value::UserData(lua.create_userdata(LColor(c))?))
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(|lua, (_, key, c): (Table, String, LColor)| {
            if key != "color" {
                return Err(mlua::Error::runtime(format!("Gizmos.{key} cannot be set")));
            }
            with_sink(lua, |s| s.color = c.0)
        })?,
    )?;
    g.set_metatable(Some(meta))?;
    lua.globals().set("Gizmos", g)
}

/// Run the gizmo callbacks of every scripted object and keep what they drew in
/// `editor_scripts.gizmo_frame`; its lines are added to `lines`.
pub fn lines(state: &mut EditorState, lines: &mut Lines) {
    state.editor_scripts.gizmo_frame = GizmoFrame::default();
    if !state.editor_scripts.active() || state.play.in_play_mode() {
        return;
    }
    let scripted: Vec<(Entity, Vec<ScriptRef>)> = state
        .scene
        .world
        .query::<(Entity, &Scripts)>()
        .iter()
        .map(|(e, s)| (e, s.list.clone()))
        .collect();
    if scripted.is_empty() {
        return;
    }
    let selected: Vec<Entity> = state.selection.list().to_vec();
    let frame = super::call(state, super::FRAME_BUDGET, |lua| {
        lua.set_app_data(Sink {
            color: DEFAULT_COLOR,
            frame: GizmoFrame::default(),
        });
        for (e, list) in &scripted {
            let shown = with_world(lua, |w| w.scene.active_in_hierarchy(*e)).unwrap_or(false);
            if !shown {
                continue;
            }
            for script in list.iter().filter(|s| s.enabled) {
                draw_one(lua, *e, script, selected.contains(e));
            }
        }
        lua.remove_app_data::<Sink>().map(|s| s.frame)
    });
    if let Some(Some(frame)) = frame {
        for (a, b, c) in &frame.lines {
            lines.line(*a, *b, *c);
        }
        state.editor_scripts.gizmo_frame = frame;
    }
}

fn draw_one(lua: &Lua, e: Entity, script: &ScriptRef, selected: bool) {
    let site = format!("gizmos:{}", script.path);
    if super::state_mut(lua).is_ok_and(|st| st.editor_scripts.is_broken(&site)) {
        return;
    }
    let Ok(Some(class)) = super::api::game_class(lua, &script.path) else {
        return;
    };
    let mut methods: Vec<Function> = Vec::new();
    if let Ok(Some(f)) = class.get::<Option<Function>>("OnDrawGizmos") {
        methods.push(f);
    }
    if selected && let Ok(Some(f)) = class.get::<Option<Function>>("OnDrawGizmosSelected") {
        methods.push(f);
    }
    if methods.is_empty() {
        return;
    }
    let result = super::api::script_self(lua, &class, e, script).and_then(|me| {
        for f in methods {
            let _ = with_sink(lua, |s| s.color = DEFAULT_COLOR);
            f.call::<()>(me.clone())?;
        }
        Ok(())
    });
    if let Err(err) = result
        && let Ok(st) = super::state_mut(lua)
    {
        super::fail(st, &site, &err);
    }
}

fn color32(c: [f32; 4]) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(c[3]))
}

/// Icons and labels of this frame's script gizmos, over the Scene view.
pub fn paint(ui: &Ui, rect: Rect, camera: &Camera, state: &EditorState) {
    let frame = &state.editor_scripts.gizmo_frame;
    if frame.icons.is_empty() && frame.labels.is_empty() {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    for (p, glyph, c) in &frame.icons {
        if let Some(at) = crate::picking::to_screen(camera, rect, *p) {
            painter.text(
                at,
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(18.0),
                color32(*c),
            );
        }
    }
    for (p, text, c) in &frame.labels {
        if let Some(at) = crate::picking::to_screen(camera, rect, *p) {
            let font = FontId::proportional(12.0);
            let galley = painter.layout_no_wrap(text.clone(), font, color32(*c));
            let r =
                Rect::from_min_size(at + egui::vec2(6.0, -galley.size().y - 2.0), galley.size());
            painter.rect_filled(r.expand(2.0), 2.0, Color32::from_black_alpha(150));
            painter.galley(r.min, galley, color32(*c));
        }
    }
}
