//! `GUI`: the small immediate-mode UI that tool windows and custom Inspectors draw with.
//!
//! Controls take the current value and return the new one (`self.count = GUI.IntField("Count",
//! self.count)`); `GUI.Button` returns true on the frame it is clicked. Layout helpers take a
//! function that draws the contents: `GUI.Horizontal(function() … end)`.
//!
//! The functions draw into the `egui::Ui` of the window being drawn. That `Ui` is reached
//! through a pointer stack that only holds entries while the Rust frame owning the `Ui` runs
//! the Lua code (see [`run`]); outside `OnGUI` / `OnInspectorGUI` the functions raise an error.

use egui::{Align, Color32, Layout, RichText, Ui, vec2};
use misty_engine::scripting::api::{LGameObject, LVec3, with_world};
use misty_engine::scripting::ui_api::LColor;
use mlua::{Function, Lua, Result, Value};

use super::api::entity_of;
use crate::panels::AssetPayload;

/// The `Ui`s being drawn into, innermost last.
#[derive(Default)]
struct GuiStack {
    uis: Vec<*mut Ui>,
    /// Ids for scroll areas, in call order.
    next_id: u64,
}

/// Run `f` with `ui` as the GUI target.
pub(crate) fn run<R>(lua: &Lua, ui: &mut Ui, f: impl FnOnce() -> R) -> R {
    let fresh = lua.app_data_ref::<GuiStack>().is_none();
    if fresh {
        lua.set_app_data(GuiStack::default());
    }
    push(lua, ui);
    let r = f();
    pop(lua);
    if fresh {
        lua.remove_app_data::<GuiStack>();
    }
    r
}

fn push(lua: &Lua, ui: &mut Ui) {
    if let Some(mut s) = lua.app_data_mut::<GuiStack>() {
        s.uis.push(ui as *mut Ui);
    }
}

fn pop(lua: &Lua) {
    if let Some(mut s) = lua.app_data_mut::<GuiStack>() {
        s.uis.pop();
    }
}

/// The innermost `Ui`, or an error outside a GUI callback.
///
/// SAFETY: entries are pushed by [`run`] / [`nested`] from a `&mut Ui` that stays borrowed
/// (and unused by Rust) until the matching pop, so dereferencing the top entry here is the only
/// access. Lua runs on this thread only.
pub(crate) fn with_ui<R>(lua: &Lua, f: impl FnOnce(&mut Ui) -> R) -> Result<R> {
    let p = lua
        .app_data_ref::<GuiStack>()
        .and_then(|s| s.uis.last().copied())
        .ok_or_else(|| {
            mlua::Error::runtime("GUI functions only work inside OnGUI or OnInspectorGUI")
        })?;
    Ok(f(unsafe { &mut *p }))
}

/// Call `body` with `child` as the GUI target (layout helpers).
fn nested(lua: &Lua, child: &mut Ui, body: &Function) -> Result<()> {
    push(lua, child);
    let r = body.call::<()>(());
    pop(lua);
    r
}

fn next_id(lua: &Lua) -> u64 {
    lua.app_data_mut::<GuiStack>()
        .map(|mut s| {
            s.next_id += 1;
            s.next_id
        })
        .unwrap_or(0)
}

/// A labelled row: the label column, then the control.
fn row<R>(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui) -> R) -> R {
    if label.is_empty() {
        return f(ui);
    }
    ui.horizontal(|ui| {
        let w = (ui.available_width() * 0.4).clamp(70.0, 170.0);
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(w);
            ui.add(egui::Label::new(label).truncate());
        });
        f(ui)
    })
    .inner
}

pub fn register(lua: &Lua) -> Result<()> {
    let gui = lua.create_table()?;
    gui.set(
        "Label",
        lua.create_function(|lua, text: String| {
            with_ui(lua, |ui| {
                ui.label(text);
            })
        })?,
    )?;
    gui.set(
        "Header",
        lua.create_function(|lua, text: String| {
            with_ui(lua, |ui| {
                ui.add_space(4.0);
                ui.label(crate::theme::bold(ui.ctx(), text));
            })
        })?,
    )?;
    gui.set(
        "HelpBox",
        lua.create_function(|lua, (text, kind): (String, Option<String>)| {
            with_ui(lua, |ui| {
                help_box(ui, &text, kind.as_deref().unwrap_or("info"))
            })
        })?,
    )?;
    gui.set(
        "Button",
        lua.create_function(|lua, text: String| with_ui(lua, |ui| ui.button(text).clicked()))?,
    )?;
    gui.set(
        "Toggle",
        lua.create_function(|lua, (label, mut v): (String, bool)| {
            with_ui(lua, |ui| {
                row(ui, "", |ui| ui.checkbox(&mut v, label));
                v
            })
        })?,
    )?;
    gui.set(
        "Slider",
        lua.create_function(|lua, (label, mut v, lo, hi): (String, f64, f64, f64)| {
            let (lo, hi) = (lo.min(hi), hi.max(lo));
            with_ui(lua, |ui| {
                row(ui, &label, |ui| ui.add(egui::Slider::new(&mut v, lo..=hi)));
                v
            })
        })?,
    )?;
    gui.set(
        "IntField",
        lua.create_function(|lua, (label, v): (String, Option<f64>)| {
            let mut i = v.unwrap_or(0.0).round() as i64;
            with_ui(lua, |ui| {
                row(ui, &label, |ui| ui.add(egui::DragValue::new(&mut i)));
                i
            })
        })?,
    )?;
    gui.set(
        "FloatField",
        lua.create_function(|lua, (label, v): (String, Option<f64>)| {
            let mut f = v.unwrap_or(0.0);
            with_ui(lua, |ui| {
                row(ui, &label, |ui| {
                    ui.add(egui::DragValue::new(&mut f).speed(0.1))
                });
                f
            })
        })?,
    )?;
    gui.set(
        "TextField",
        lua.create_function(|lua, (label, v): (String, Option<String>)| {
            let mut s = v.unwrap_or_default();
            with_ui(lua, |ui| {
                row(ui, &label, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s).desired_width(f32::INFINITY))
                });
                s
            })
        })?,
    )?;
    gui.set(
        "ColorField",
        lua.create_function(|lua, (label, c): (String, Option<LColor>)| {
            let mut rgba = c.map(|c| c.0).unwrap_or([1.0; 4]);
            with_ui(lua, |ui| {
                row(ui, &label, |ui| {
                    ui.color_edit_button_rgba_unmultiplied(&mut rgba)
                });
                LColor(rgba)
            })
        })?,
    )?;
    gui.set(
        "Vector3Field",
        lua.create_function(|lua, (label, v): (String, Option<LVec3>)| {
            let mut a = v.map(|v| v.0.to_array()).unwrap_or_default();
            with_ui(lua, |ui| {
                row(ui, &label, |ui| {
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.label(*axis);
                        ui.add(egui::DragValue::new(&mut a[i]).speed(0.05));
                    }
                });
                LVec3(glam::Vec3::from(a))
            })
        })?,
    )?;
    gui.set("ObjectField", lua.create_function(object_field)?)?;
    gui.set(
        "Foldout",
        lua.create_function(|lua, (label, open): (String, bool)| {
            with_ui(lua, |ui| {
                let arrow = if open { "⏷" } else { "⏵" };
                let text = crate::theme::bold(ui.ctx(), format!("{arrow} {label}"));
                let clicked = ui.add(egui::Button::new(text).frame(false)).clicked();
                open != clicked
            })
        })?,
    )?;
    gui.set(
        "Horizontal",
        lua.create_function(|lua, body: Function| {
            with_ui(lua, |ui| ui.horizontal(|ui| nested(lua, ui, &body)).inner)?
        })?,
    )?;
    gui.set(
        "Vertical",
        lua.create_function(|lua, body: Function| {
            with_ui(lua, |ui| ui.vertical(|ui| nested(lua, ui, &body)).inner)?
        })?,
    )?;
    gui.set(
        "Scroll",
        lua.create_function(|lua, body: Function| {
            let id = next_id(lua);
            with_ui(lua, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("misty_lua_scroll", id))
                    .auto_shrink([false, true])
                    .show(ui, |ui| nested(lua, ui, &body))
                    .inner
            })?
        })?,
    )?;
    gui.set(
        "Space",
        lua.create_function(|lua, px: Option<f32>| {
            with_ui(lua, |ui| ui.add_space(px.unwrap_or(8.0).clamp(0.0, 200.0)))
        })?,
    )?;
    gui.set(
        "Separator",
        lua.create_function(|lua, ()| {
            with_ui(lua, |ui| {
                ui.separator();
            })
        })?,
    )?;
    lua.globals().set("GUI", gui)
}

fn help_box(ui: &mut Ui, text: &str, kind: &str) {
    let (icon, color) = match kind {
        "warning" => ("⚠", Color32::from_rgb(255, 200, 60)),
        "error" => ("⛔", Color32::from_rgb(255, 90, 80)),
        _ => ("ℹ", Color32::from_gray(200)),
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.label(text);
            });
        });
}

/// `GUI.ObjectField(label, value, kind)`: a scene object (`kind` "GameObject", the default) or
/// an asset path of that extension ("prefab", "mat", "texture", …). Pick from the list, or drop
/// an asset from the Project window. Returns the new value (or nil for None).
fn object_field(lua: &Lua, (label, value, kind): (String, Value, Option<String>)) -> Result<Value> {
    let kind = kind.unwrap_or_else(|| "GameObject".into());
    let objects = kind.eq_ignore_ascii_case("GameObject");
    let current = if objects {
        entity_of(&value)
            .and_then(|e| with_world(lua, |w| w.scene.contains(e).then(|| w.scene.name(e))).ok()?)
            .unwrap_or_else(|| "None".into())
    } else {
        match &value {
            Value::String(s) => {
                let p = s.to_string_lossy();
                p.rsplit('/').next().unwrap_or(&p).to_owned()
            }
            _ => "None".into(),
        }
    };
    // Choices are only listed while the popup is open.
    let mut picked: Option<Option<Choice>> = None;
    with_ui(lua, |ui| {
        row(ui, &label, |ui| {
            let icon = if objects { "⬚" } else { "🗋" };
            let r = ui.menu_button(format!("{icon} {current}"), |ui| {
                ui.set_min_width(180.0);
                if ui.button("None").clicked() {
                    picked = Some(None);
                    ui.close();
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for c in choices(lua, &kind, objects) {
                            if ui.button(c.label()).clicked() {
                                picked = Some(Some(c));
                                ui.close();
                            }
                        }
                    });
            });
            if !objects
                && let Some(p) = r.response.dnd_release_payload::<AssetPayload>()
                && asset_matches(&p.0, &kind)
            {
                picked = Some(Some(Choice::Asset(p.0.clone())));
            }
        });
    })?;
    match picked {
        None => Ok(value),
        Some(None) => Ok(Value::Nil),
        Some(Some(Choice::Object(e, _))) => {
            Ok(Value::UserData(lua.create_userdata(LGameObject(e))?))
        }
        Some(Some(Choice::Asset(p))) => Ok(Value::String(lua.create_string(p)?)),
    }
}

enum Choice {
    Object(misty_engine::hecs::Entity, String),
    Asset(String),
}

impl Choice {
    fn label(&self) -> String {
        match self {
            Choice::Object(_, n) => n.clone(),
            Choice::Asset(p) => p.strip_prefix("Assets/").unwrap_or(p).to_owned(),
        }
    }
}

const MAX_CHOICES: usize = 500;

fn choices(lua: &Lua, kind: &str, objects: bool) -> Vec<Choice> {
    if objects {
        return with_world(lua, |w| {
            w.scene
                .walk()
                .into_iter()
                .take(MAX_CHOICES)
                .map(|(e, depth)| {
                    Choice::Object(e, format!("{}{}", "  ".repeat(depth), w.scene.name(e)))
                })
                .collect()
        })
        .unwrap_or_default();
    }
    let Ok(root) = super::with_state(lua, |st| st.project.root.clone()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = misty_engine::vfs::files_under(&root.join("Assets"))
        .into_iter()
        .filter_map(|p| {
            Some(
                p.strip_prefix(&root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/"),
            )
        })
        .filter(|rel| asset_matches(rel, kind))
        .collect();
    out.sort();
    out.truncate(MAX_CHOICES);
    out.into_iter().map(Choice::Asset).collect()
}

/// Does the asset at `rel` fit an ObjectField / `AssetDB.Find` kind ("prefab", "texture", …)?
pub(crate) fn asset_matches(rel: &str, kind: &str) -> bool {
    if rel.ends_with(".meta") || super::is_editor_script(rel) {
        return false;
    }
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match kind.to_ascii_lowercase().as_str() {
        "" | "any" | "asset" => true,
        "texture" => misty_engine::assets::is_texture(rel),
        "material" => ext == "mat",
        "script" => ext == "lua",
        "model" => misty_engine::models::is_model(rel),
        "audio" => misty_engine::audio::is_clip(rel),
        k => ext == k.trim_start_matches('.'),
    }
}
