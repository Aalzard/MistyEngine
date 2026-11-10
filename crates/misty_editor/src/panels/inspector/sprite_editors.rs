//! Inspector editors for the 2D components: Sprite Renderer, Rigidbody 2D and the 2D colliders.

use egui::Ui;
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BodyType2D, BoxCollider2D, CapsuleCollider2D, CapsuleDirection2D, CircleCollider2D,
    Collider2DCommon, CollisionDetection2D, EdgeCollider2D, PolygonCollider2D, Rigidbody2D,
    SceneComponent, SpriteRenderer,
};
use misty_engine::sprites;

use crate::panels::AssetPayload;
use crate::state::EditorState;

/// Names of the components handled here.
pub const NAMES: [&str; 7] = [
    "Sprite Renderer",
    "Rigidbody 2D",
    "Box Collider 2D",
    "Circle Collider 2D",
    "Capsule Collider 2D",
    "Polygon Collider 2D",
    "Edge Collider 2D",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Sprite Renderer" => "🖼",
        "Rigidbody 2D" => "⚛",
        n if n.ends_with("Collider 2D") => "⬠",
        _ => return None,
    })
}

/// The collider settings every 2D collider shares, by component name.
fn with_common<R>(
    state: &mut EditorState,
    e: Entity,
    name: &str,
    f: impl FnOnce(&mut Collider2DCommon) -> R,
) -> Option<R> {
    let w = &state.scene.world;
    match name {
        "Box Collider 2D" => w
            .get::<&mut BoxCollider2D>(e)
            .ok()
            .map(|mut c| f(&mut c.common)),
        "Circle Collider 2D" => w
            .get::<&mut CircleCollider2D>(e)
            .ok()
            .map(|mut c| f(&mut c.common)),
        "Capsule Collider 2D" => w
            .get::<&mut CapsuleCollider2D>(e)
            .ok()
            .map(|mut c| f(&mut c.common)),
        "Polygon Collider 2D" => w
            .get::<&mut PolygonCollider2D>(e)
            .ok()
            .map(|mut c| f(&mut c.common)),
        "Edge Collider 2D" => w
            .get::<&mut EdgeCollider2D>(e)
            .ok()
            .map(|mut c| f(&mut c.common)),
        _ => None,
    }
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Sprite Renderer" => w.get::<&SpriteRenderer>(e).ok().map(|c| c.enabled),
        "Rigidbody 2D" => None,
        "Box Collider 2D" => w.get::<&BoxCollider2D>(e).ok().map(|c| c.common.enabled),
        "Circle Collider 2D" => w.get::<&CircleCollider2D>(e).ok().map(|c| c.common.enabled),
        "Capsule Collider 2D" => w
            .get::<&CapsuleCollider2D>(e)
            .ok()
            .map(|c| c.common.enabled),
        "Polygon Collider 2D" => w
            .get::<&PolygonCollider2D>(e)
            .ok()
            .map(|c| c.common.enabled),
        "Edge Collider 2D" => w.get::<&EdgeCollider2D>(e).ok().map(|c| c.common.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if name == "Sprite Renderer" {
        let _ = state
            .scene
            .world
            .get::<&mut SpriteRenderer>(e)
            .map(|mut c| c.enabled = on);
    } else {
        with_common(state, e, name, |c| c.enabled = on);
    }
}

/// Draw the editor for component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Sprite Renderer" => sprite_renderer(ui, state, e),
        "Rigidbody 2D" => edit_copy::<Rigidbody2D>(state, e, |r| rigidbody(ui, r)),
        "Box Collider 2D" => {
            edit_collider_button(ui, e);
            edit_copy::<BoxCollider2D>(state, e, |c| {
                grid(ui, "box2d", |ui| {
                    let mut ch = common(ui, &mut c.common);
                    ch |= vec2(ui, "Size", &mut c.size, 0.01);
                    ch |= number(ui, "Edge Radius", &mut c.edge_radius, 0.01, 0.0, 1000.0);
                    ch
                })
            })
        }
        "Circle Collider 2D" => {
            edit_collider_button(ui, e);
            edit_copy::<CircleCollider2D>(state, e, |c| {
                grid(ui, "circle2d", |ui| {
                    let ch = common(ui, &mut c.common);
                    ch | number(ui, "Radius", &mut c.radius, 0.01, 0.0, 10000.0)
                })
            })
        }
        "Capsule Collider 2D" => edit_copy::<CapsuleCollider2D>(state, e, |c| {
            grid(ui, "capsule2d", |ui| {
                let mut ch = common(ui, &mut c.common);
                ch |= vec2(ui, "Size", &mut c.size, 0.01);
                ui.label("Direction");
                egui::ComboBox::from_id_salt("capsule2d_dir")
                    .selected_text(format!("{:?}", c.direction))
                    .show_ui(ui, |ui| {
                        for d in [CapsuleDirection2D::Vertical, CapsuleDirection2D::Horizontal] {
                            ch |= ui
                                .selectable_value(&mut c.direction, d, format!("{d:?}"))
                                .changed();
                        }
                    });
                ui.end_row();
                ch
            })
        }),
        "Polygon Collider 2D" => edit_copy::<PolygonCollider2D>(state, e, |c| {
            let mut ch = grid(ui, "poly2d", |ui| common(ui, &mut c.common));
            if c.points.is_empty() {
                ui.weak("No points: uses the sprite's rect (or a unit square).");
            }
            ch |= points(ui, "poly2d_pts", &mut c.points);
            ch
        }),
        "Edge Collider 2D" => edit_copy::<EdgeCollider2D>(state, e, |c| {
            let mut ch = grid(ui, "edge2d", |ui| {
                let ch = common(ui, &mut c.common);
                ch | number(ui, "Edge Radius", &mut c.edge_radius, 0.01, 0.0, 1000.0)
            });
            ch |= points(ui, "edge2d_pts", &mut c.points);
            ch
        }),
        _ => false,
    }
}

fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = f(&mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::panels::inspector::card::PropertyGrid::new(id)
        .show(ui, body)
        .inner
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, min: f32, max: f32) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            egui::DragValue::new(v)
                .speed(speed)
                .range(min..=max)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

fn vec2(ui: &mut Ui, label: &str, v: &mut Vec2, speed: f64) -> bool {
    ui.label(label);
    let ch = ui
        .horizontal(|ui| {
            let x = ui.add(egui::DragValue::new(&mut v.x).speed(speed).prefix("X "));
            let y = ui.add(egui::DragValue::new(&mut v.y).speed(speed).prefix("Y "));
            x.changed() || y.changed()
        })
        .inner;
    ui.end_row();
    ch
}

/// Material (friction, bounciness), Is Trigger and Offset rows.
fn common(ui: &mut Ui, c: &mut Collider2DCommon) -> bool {
    ui.label("Material");
    ui.weak("Physics Material 2D");
    ui.end_row();
    let mut ch = number(ui, "    Friction", &mut c.material.friction, 0.01, 0.0, 1e6);
    ch |= number(
        ui,
        "    Bounciness",
        &mut c.material.bounciness,
        0.01,
        0.0,
        1.0,
    );
    ui.label("Is Trigger");
    ch |= ui.checkbox(&mut c.is_trigger, "").changed();
    ui.end_row();
    ch | vec2(ui, "Offset", &mut c.offset, 0.01)
}

/// Editable list of points with Size, like the Inspector's array fields.
fn points(ui: &mut Ui, id: &str, pts: &mut Vec<Vec2>) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Points"))
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            let mut n = pts.len();
            ui.horizontal(|ui| {
                ui.label("Size");
                if ui
                    .add(egui::DragValue::new(&mut n).range(0..=256))
                    .changed()
                {
                    let last = pts.last().copied().unwrap_or_default();
                    pts.resize(n, last + Vec2::X);
                    ch = true;
                }
            });
            grid(ui, &format!("{id}_grid"), |ui| {
                for (i, p) in pts.iter_mut().enumerate() {
                    ch |= vec2(ui, &format!("Element {i}"), p, 0.01);
                }
                false
            });
        });
    ch
}

/// Edit Collider toggle: shows draggable handles on the Box/Circle Collider 2D in the Scene view.
fn edit_collider_button(ui: &mut Ui, e: Entity) {
    let id = crate::sprite_handles::edit_collider_id();
    let on = ui.ctx().data(|d| d.get_temp::<u64>(id)) == Some(e.to_bits().get());
    ui.horizontal(|ui| {
        ui.label("Edit Collider");
        let b = egui::Button::new("⬠").selected(on);
        if ui
            .add(b)
            .on_hover_text("Edit the collider's shape in the Editor Scene")
            .clicked()
        {
            ui.ctx().data_mut(|d| {
                if on {
                    d.remove::<u64>(id);
                } else {
                    d.insert_temp(id, e.to_bits().get());
                }
            });
        }
    });
}

fn rigidbody(ui: &mut Ui, r: &mut Rigidbody2D) -> bool {
    let mut ch = grid(ui, "rb2d", |ui| {
        let mut ch = false;
        ui.label("Body Type");
        egui::ComboBox::from_id_salt("rb2d_type")
            .selected_text(r.body_type.label())
            .show_ui(ui, |ui| {
                for t in BodyType2D::ALL {
                    ch |= ui
                        .selectable_value(&mut r.body_type, t, t.label())
                        .changed();
                }
            });
        ui.end_row();
        if r.body_type == BodyType2D::Static {
            return ch;
        }
        if r.body_type == BodyType2D::Dynamic {
            ch |= number(ui, "Mass", &mut r.mass, 0.01, 0.0001, 1e6);
            ch |= number(ui, "Linear Drag", &mut r.linear_drag, 0.01, 0.0, 1e6);
            ch |= number(ui, "Angular Drag", &mut r.angular_drag, 0.01, 0.0, 1e6);
            ch |= number(ui, "Gravity Scale", &mut r.gravity_scale, 0.01, -1e6, 1e6);
        }
        ui.label("Collision Detection");
        egui::ComboBox::from_id_salt("rb2d_ccd")
            .selected_text(format!("{:?}", r.collision_detection))
            .show_ui(ui, |ui| {
                for c in [
                    CollisionDetection2D::Discrete,
                    CollisionDetection2D::Continuous,
                ] {
                    ch |= ui
                        .selectable_value(&mut r.collision_detection, c, format!("{c:?}"))
                        .changed();
                }
            });
        ui.end_row();
        ch
    });
    if r.body_type != BodyType2D::Static {
        egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Constraints"))
            .id_salt("rb2d_constraints")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Freeze Position");
                    ch |= ui.checkbox(&mut r.freeze_position[0], "X").changed();
                    ch |= ui.checkbox(&mut r.freeze_position[1], "Y").changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Freeze Rotation");
                    ch |= ui.checkbox(&mut r.freeze_rotation, "Z").changed();
                });
            });
    }
    ch
}

/// Every sprite the project offers: built-in shapes, then each Sprite-type texture's sprites.
pub fn sprite_choices(state: &EditorState) -> Vec<String> {
    sprite_choices_from(&state.assets.textures)
}

/// Sprite choices among the project textures `textures`.
pub fn sprite_choices_from(textures: &[String]) -> Vec<String> {
    let mut out: Vec<String> = sprites::builtin::SHAPES
        .iter()
        .map(|s| sprites::builtin::path(s))
        .collect();
    for t in textures {
        if sprites::importer(t).is_sprite() {
            out.extend(sprites::references(t));
        }
    }
    out
}

/// Sprite dropdown that also takes a texture dropped from the Project window.
pub fn sprite_slot(ui: &mut Ui, id: &str, value: &mut Option<String>, choices: &[String]) -> bool {
    let mut ch = false;
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(format!("🖼 {}", sprites::display_name(value.as_deref())))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, "None (Sprite)").changed();
            for s in choices {
                let label = sprites::display_name(Some(s));
                ch |= ui
                    .selectable_value(value, Some(s.clone()), label)
                    .on_hover_text(s)
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<AssetPayload>()
        && misty_engine::assets::is_texture(&p.0)
    {
        *value = sprites::references(&p.0).into_iter().next();
        ch = true;
    }
    ch
}

fn color_rgba(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut srgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = crate::panels::inspector::card::wide(ui, |ui| {
        ui.color_edit_button_srgba_unmultiplied(&mut srgba)
    })
    .changed();
    if changed {
        *c = srgba.map(|v| v as f32 / 255.0);
    }
    changed
}

fn sprite_renderer(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let choices = sprite_choices(state);
    let layers = state.project_settings.sorting_layers.clone();
    edit_copy::<SpriteRenderer>(state, e, |r| {
        grid(ui, "sprite_renderer", |ui| {
            ui.label("Sprite");
            let mut ch = sprite_slot(ui, "sprite_renderer_sprite", &mut r.sprite, &choices);
            ui.end_row();
            ui.label("Color");
            ch |= color_rgba(ui, &mut r.color);
            ui.end_row();
            ui.label("Flip");
            ui.horizontal(|ui| {
                ch |= ui.checkbox(&mut r.flip_x, "X").changed();
                ch |= ui.checkbox(&mut r.flip_y, "Y").changed();
            });
            ui.end_row();
            ui.label("Draw Mode");
            ui.weak("Simple");
            ui.end_row();
            ui.label("Material");
            ui.weak(format!("◈ {}", sprites::DEFAULT_MATERIAL));
            ui.end_row();
            ui.label(crate::theme::bold(ui.ctx(), "Additional Settings"));
            ui.label("");
            ui.end_row();
            ui.label("Sorting Layer");
            egui::ComboBox::from_id_salt("sprite_sorting_layer")
                .selected_text(r.sorting_layer.clone())
                .show_ui(ui, |ui| {
                    for l in &layers {
                        ch |= ui
                            .selectable_value(&mut r.sorting_layer, l.clone(), l)
                            .changed();
                    }
                });
            ui.end_row();
            ui.label("Order in Layer");
            ch |= ui
                .add(egui::DragValue::new(&mut r.order_in_layer))
                .changed();
            ui.end_row();
            ch
        })
    })
}
