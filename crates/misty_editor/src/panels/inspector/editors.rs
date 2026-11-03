//! Per-component Inspector editors and the Add Component popup.

use egui::{Grid, Key, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Animation, AudioListener, AudioRolloff, AudioSource, BoxCollider, Camera, CapsuleCollider,
    ClearFlags, Light, LightType, MeshCollider, MeshRenderer, PhysicsSurface, PostProcessVolume,
    Rigidbody, SceneComponent, Shadows, SphereCollider, registry,
};

use misty_engine::assets::Material;
use misty_engine::scene::{ScriptValue, Scripts};

use crate::actions::Action;
use crate::assets::AssetDatabase;
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
        "Rigidbody" => "⚛",
        "Box Collider" | "Sphere Collider" | "Capsule Collider" | "Mesh Collider" => "⬡",
        "Audio Source" => crate::state::AUDIO_ICON,
        "Audio Listener" => "🎧",
        "Animation" => "⏵",
        "Particle System" => crate::state::PARTICLE_ICON,
        "Post-process Volume" => "◐",
        other => super::ui_editors::icon(other).unwrap_or("▣"),
    }
}

/// Components with an enable checkbox in their header.
pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    if super::ui_editors::NAMES.contains(&name) {
        return super::ui_editors::enabled_flag(state, e, name);
    }
    match name {
        "Camera" => w.get::<&Camera>(e).ok().map(|c| c.enabled),
        "Light" => w.get::<&Light>(e).ok().map(|c| c.enabled),
        "Mesh Renderer" => w.get::<&MeshRenderer>(e).ok().map(|c| c.enabled),
        "Box Collider" => w.get::<&BoxCollider>(e).ok().map(|c| c.enabled),
        "Sphere Collider" => w.get::<&SphereCollider>(e).ok().map(|c| c.enabled),
        "Capsule Collider" => w.get::<&CapsuleCollider>(e).ok().map(|c| c.enabled),
        "Mesh Collider" => w.get::<&MeshCollider>(e).ok().map(|c| c.enabled),
        "Audio Source" => w.get::<&AudioSource>(e).ok().map(|c| c.enabled),
        "Audio Listener" => w.get::<&AudioListener>(e).ok().map(|c| c.enabled),
        "Animation" => w.get::<&Animation>(e).ok().map(|c| c.enabled),
        "Post-process Volume" => w.get::<&PostProcessVolume>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if super::ui_editors::NAMES.contains(&name) {
        return super::ui_editors::set_enabled(state, e, name, on);
    }
    let w = &mut state.scene.world;
    match name {
        "Camera" => w.get::<&mut Camera>(e).map(|mut c| c.enabled = on).ok(),
        "Light" => w.get::<&mut Light>(e).map(|mut c| c.enabled = on).ok(),
        "Mesh Renderer" => w
            .get::<&mut MeshRenderer>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Box Collider" => w
            .get::<&mut BoxCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Sphere Collider" => w
            .get::<&mut SphereCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Capsule Collider" => w
            .get::<&mut CapsuleCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Mesh Collider" => w
            .get::<&mut MeshCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Audio Source" => w
            .get::<&mut AudioSource>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Audio Listener" => w
            .get::<&mut AudioListener>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Animation" => w.get::<&mut Animation>(e).map(|mut c| c.enabled = on).ok(),
        "Post-process Volume" => w
            .get::<&mut PostProcessVolume>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        _ => None,
    };
}

/// Draw the fields for component `name`. Returns true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, targets: &[Entity], name: &str) -> bool {
    let e = targets[0];
    match name {
        "Transform" => transform(ui, state, targets),
        "Camera" => with::<Camera>(ui, state, e, camera),
        "Light" => with::<Light>(ui, state, e, light),
        "Mesh Filter" => super::slots::mesh_filter(ui, state, e),
        "Mesh Renderer" => mesh_renderer(ui, state, e),
        "Rigidbody" => with::<Rigidbody>(ui, state, e, rigidbody),
        "Box Collider" => with::<BoxCollider>(ui, state, e, box_collider),
        "Sphere Collider" => with::<SphereCollider>(ui, state, e, sphere_collider),
        "Capsule Collider" => with::<CapsuleCollider>(ui, state, e, capsule_collider),
        "Mesh Collider" => with::<MeshCollider>(ui, state, e, mesh_collider),
        "Audio Source" => audio_source(ui, state, e),
        "Animation" => super::anim::animation(ui, state, e),
        "Particle System" => super::particles::particle_system(ui, state, e),
        "Post-process Volume" => super::post_volume::post_process_volume(ui, state, e),
        "Audio Listener" => {
            ui.weak("Hears every Audio Source in the scene.");
            false
        }
        ui_name if super::ui_editors::NAMES.contains(&ui_name) => {
            super::ui_editors::edit(ui, state, e, ui_name)
        }
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

/// Local position / Euler rotation / scale. Changed axes are applied to every target.
fn transform(ui: &mut Ui, state: &mut EditorState, targets: &[Entity]) -> bool {
    let t0 = state.scene.transform(targets[0]);
    let old = [
        t0.position.to_array(),
        t0.euler().to_array(),
        t0.scale.to_array(),
    ];
    let mut new = old;
    let mut changed = vec3_row(ui, "Position", &mut new[0], 0.1);
    changed |= vec3_row(ui, "Rotation", &mut new[1], 1.0);
    ui.horizontal(|ui| {
        let lock = if state.scale_locked { "🔗" } else { "⛓" };
        if ui
            .add(
                egui::Button::new(lock)
                    .frame(false)
                    .selected(state.scale_locked),
            )
            .on_hover_text("Constrain Proportions")
            .clicked()
        {
            state.scale_locked = !state.scale_locked;
        }
        changed |= vec3_row(ui, "Scale", &mut new[2], 0.01);
    });
    if !changed {
        return false;
    }
    // Constrained scale: the edited axis scales the other two by the same ratio.
    let scale_ratio = (0..3)
        .find(|&i| new[2][i] != old[2][i] && old[2][i].abs() > 1e-6)
        .map(|i| new[2][i] / old[2][i]);
    for &e in targets {
        let mut t = state.scene.transform(e);
        let mut cur = [
            t.position.to_array(),
            t.euler().to_array(),
            t.scale.to_array(),
        ];
        for field in 0..3 {
            for axis in 0..3 {
                if new[field][axis] != old[field][axis] {
                    cur[field][axis] = new[field][axis];
                }
            }
        }
        if state.scale_locked
            && let Some(r) = scale_ratio
        {
            cur[2] = (Vec3::from(t.scale.to_array()) * r).to_array();
        }
        t.position = Vec3::from(cur[0]);
        if new[1] != old[1] {
            t.set_euler(Vec3::from(cur[1]));
        }
        t.scale = Vec3::from(cur[2]);
        state.scene.set_transform(e, t);
    }
    true
}

/// Two-column property grid like the Inspector's label/value layout.
pub(super) fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

pub(super) fn combo<T: Copy + PartialEq>(
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

/// Mesh Renderer: material slot (pick from project materials, or drop one from the Project window).
fn mesh_renderer(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut r) = state
        .scene
        .world
        .get::<&MeshRenderer>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let names: Vec<String> = state.assets.materials.keys().cloned().collect();
    let changed = grid(ui, "meshrenderer_grid", |ui| {
        let mut ch = false;
        ui.label("Materials");
        ui.label(RichText::new("Size 1").color(theme::TEXT_DIM));
        ui.end_row();
        ui.label("    Element 0");
        let current = AssetDatabase::material_name(r.material.as_deref());
        let slot = egui::ComboBox::from_id_salt("mr_material")
            .selected_text(format!("◍ {current}"))
            .width(150.0)
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.material, None, crate::assets::DEFAULT_MATERIAL)
                    .changed();
                for n in &names {
                    let label = AssetDatabase::material_name(Some(n));
                    ch |= ui
                        .selectable_value(&mut r.material, Some(n.clone()), label)
                        .changed();
                }
            })
            .response;
        if let Some(path) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
            && path.0.ends_with(".mat")
        {
            r.material = Some(path.0.clone());
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
    });
    if changed {
        let _ = state.scene.world.insert_one(e, r);
    }
    changed
}

/// Audio clip slot: pick from the project's clips, or drop one from the Project window.
fn clip_field(ui: &mut Ui, id: &str, clips: &[String], clip: &mut Option<String>) -> bool {
    let mut ch = false;
    let current = clip
        .as_deref()
        .map(clip_name)
        .unwrap_or_else(|| "None (Audio Clip)".into());
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{} {current}", crate::state::AUDIO_ICON))
        .width(150.0)
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(clip, None, "None (Audio Clip)")
                .changed();
            for c in clips {
                ch |= ui
                    .selectable_value(clip, Some(c.clone()), clip_name(c))
                    .changed();
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::audio::is_clip(&path.0)
    {
        *clip = Some(path.0.clone());
        ch = true;
    }
    ch
}

/// File stem of a clip path ("Beep").
pub fn clip_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(egui::Slider::new(v, range)).changed()
}

fn audio_source(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut a) = state
        .scene
        .world
        .get::<&AudioSource>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let clips: Vec<String> = state.assets.clips.iter().cloned().collect();
    let changed = grid(ui, "audiosource_grid", |ui| {
        let mut ch = false;
        ui.label("AudioClip");
        ch |= clip_field(ui, "as_clip", &clips, &mut a.clip);
        ui.end_row();
        for (label, flag) in [
            ("Mute", &mut a.mute),
            ("Play On Awake", &mut a.play_on_awake),
            ("Loop", &mut a.looping),
        ] {
            ui.label(label);
            ch |= ui.checkbox(flag, "").changed();
            ui.end_row();
        }
        ui.label("Volume");
        ch |= slider(ui, &mut a.volume, 0.0..=1.0);
        ui.end_row();
        ui.label("Pitch");
        ch |= slider(ui, &mut a.pitch, -3.0..=3.0);
        ui.end_row();
        ui.label("Stereo Pan");
        ch |= slider(ui, &mut a.stereo_pan, -1.0..=1.0);
        ui.end_row();
        ui.label("Spatial Blend");
        ui.vertical(|ui| {
            ch |= slider(ui, &mut a.spatial_blend, 0.0..=1.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("2D").small().color(theme::TEXT_DIM));
                ui.add_space(110.0);
                ui.label(RichText::new("3D").small().color(theme::TEXT_DIM));
            });
        });
        ui.end_row();
        ch
    });
    let mut ch3d = false;
    egui::CollapsingHeader::new("3D Sound Settings")
        .id_salt("as_3d")
        .default_open(true)
        .show(ui, |ui| {
            ch3d = grid(ui, "audiosource_3d", |ui| {
                let mut ch = false;
                ui.label("Volume Rolloff");
                ch |= combo(
                    ui,
                    "as_rolloff",
                    &mut a.rolloff,
                    &AudioRolloff::ALL,
                    AudioRolloff::label,
                );
                ui.end_row();
                ui.label("Min Distance");
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut a.min_distance)
                            .speed(0.1)
                            .range(0.0..=a.max_distance),
                    )
                    .changed();
                ui.end_row();
                ui.label("Max Distance");
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut a.max_distance)
                            .speed(1.0)
                            .range(a.min_distance + 0.01..=1e6),
                    )
                    .changed();
                ui.end_row();
                ch
            });
        });
    let changed = changed || ch3d;
    if changed {
        let _ = state.scene.world.insert_one(e, a);
    }
    changed
}

fn rigidbody(ui: &mut Ui, r: &mut Rigidbody) -> bool {
    let mut ch = grid(ui, "rigidbody_grid", |ui| {
        let mut ch = false;
        ui.label("Mass");
        ch |= ui
            .add(
                egui::DragValue::new(&mut r.mass)
                    .speed(0.05)
                    .range(1e-4..=1e6),
            )
            .changed();
        ui.end_row();
        ui.label("Drag");
        ch |= ui
            .add(
                egui::DragValue::new(&mut r.drag)
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ui.label("Angular Drag");
        ch |= ui
            .add(
                egui::DragValue::new(&mut r.angular_drag)
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ui.label("Use Gravity");
        ch |= ui.checkbox(&mut r.use_gravity, "").changed();
        ui.end_row();
        ui.label("Is Kinematic");
        ch |= ui.checkbox(&mut r.is_kinematic, "").changed();
        ui.end_row();
        ui.label("Collision Detection");
        egui::ComboBox::from_id_salt("rb_ccd")
            .selected_text(if r.continuous {
                "Continuous"
            } else {
                "Discrete"
            })
            .width(150.0)
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.continuous, false, "Discrete")
                    .changed();
                ch |= ui
                    .selectable_value(&mut r.continuous, true, "Continuous")
                    .changed();
            });
        ui.end_row();
        ch
    });
    egui::CollapsingHeader::new("Constraints")
        .id_salt("rb_constraints")
        .default_open(false)
        .show(ui, |ui| {
            for (label, flags) in [
                ("Freeze Position", &mut r.freeze_position),
                ("Freeze Rotation", &mut r.freeze_rotation),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized([110.0, 18.0], egui::Label::new(label));
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ch |= ui.checkbox(&mut flags[i], *axis).changed();
                    }
                });
            }
        });
    ch
}

/// Is Trigger + friction/bounciness rows shared by every collider.
fn collider_common(
    ui: &mut Ui,
    id: &str,
    trigger: &mut bool,
    surface: &mut PhysicsSurface,
) -> bool {
    grid(ui, id, |ui| {
        let mut ch = false;
        ui.label("Is Trigger");
        ch |= ui.checkbox(trigger, "").changed();
        ui.end_row();
        ui.label("Friction");
        ch |= ui
            .add(egui::Slider::new(&mut surface.friction, 0.0..=2.0))
            .changed();
        ui.end_row();
        ui.label("Bounciness");
        ch |= ui
            .add(egui::Slider::new(&mut surface.bounciness, 0.0..=1.0))
            .changed();
        ui.end_row();
        ch
    })
}

fn vec3_field(ui: &mut Ui, label: &str, v: &mut Vec3, speed: f32) -> bool {
    let mut a = v.to_array();
    let ch = vec3_row(ui, label, &mut a, speed);
    if ch {
        *v = Vec3::from(a);
    }
    ch
}

fn box_collider(ui: &mut Ui, c: &mut BoxCollider) -> bool {
    let mut ch = collider_common(ui, "box_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", &mut c.center, 0.01);
    if vec3_field(ui, "Size", &mut c.size, 0.01) {
        c.size = c.size.max(Vec3::ZERO);
        ch = true;
    }
    ch
}

fn sphere_collider(ui: &mut Ui, c: &mut SphereCollider) -> bool {
    let mut ch = collider_common(ui, "sphere_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", &mut c.center, 0.01);
    ch |= grid(ui, "sphere_col_r", |ui| {
        ui.label("Radius");
        let r = ui
            .add(
                egui::DragValue::new(&mut c.radius)
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        r
    });
    ch
}

fn capsule_collider(ui: &mut Ui, c: &mut CapsuleCollider) -> bool {
    let mut ch = collider_common(ui, "capsule_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", &mut c.center, 0.01);
    ch |= grid(ui, "capsule_col_dims", |ui| {
        let mut ch = false;
        ui.label("Radius");
        ch |= ui
            .add(
                egui::DragValue::new(&mut c.radius)
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        ui.label("Height");
        ch |= ui
            .add(
                egui::DragValue::new(&mut c.height)
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        ui.label("Direction");
        let names = ["X-Axis", "Y-Axis", "Z-Axis"];
        egui::ComboBox::from_id_salt("capsule_dir")
            .selected_text(names[c.direction.min(2) as usize])
            .width(150.0)
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ch |= ui.selectable_value(&mut c.direction, i as u8, *n).changed();
                }
            });
        ui.end_row();
        ch
    });
    ch
}

fn mesh_collider(ui: &mut Ui, c: &mut MeshCollider) -> bool {
    let mut ch = grid(ui, "mesh_col_convex", |ui| {
        ui.label("Convex");
        let r = ui.checkbox(&mut c.convex, "").changed();
        ui.end_row();
        r
    });
    ch |= collider_common(ui, "mesh_col", &mut c.is_trigger, &mut c.surface);
    ui.weak("Uses the Mesh Filter's mesh.");
    ch
}

/// Standard material fields. Returns true when changed.
/// Material properties; `textures` lists the project's textures for the map slots.
pub fn material_fields(ui: &mut Ui, id: &str, m: &mut Material, textures: &[String]) -> bool {
    use super::slots::texture_slot;
    grid(ui, id, |ui| {
        let mut ch = false;
        ui.label("Shader");
        ui.label(&m.shader);
        ui.end_row();
        ui.label("Albedo");
        ui.horizontal(|ui| {
            ch |= texture_slot(ui, &format!("{id}_albedo"), &mut m.albedo_map, textures);
            let mut rgb = [m.color[0], m.color[1], m.color[2]];
            if color_rgb(ui, &mut rgb) {
                m.color[..3].copy_from_slice(&rgb);
                ch = true;
            }
        });
        ui.end_row();
        ui.label("Metallic");
        ch |= ui
            .add(egui::Slider::new(&mut m.metallic, 0.0..=1.0))
            .changed();
        ui.end_row();
        ui.label("Smoothness");
        ch |= ui
            .add(egui::Slider::new(&mut m.smoothness, 0.0..=1.0))
            .changed();
        ui.end_row();
        ui.label("Normal Map");
        ui.horizontal(|ui| {
            ch |= texture_slot(ui, &format!("{id}_normal"), &mut m.normal_map, textures);
            if m.normal_map.is_some() {
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut m.normal_scale)
                            .speed(0.01)
                            .range(-4.0..=4.0),
                    )
                    .on_hover_text("Normal map strength")
                    .changed();
            }
        });
        ui.end_row();
        ui.label("Emission");
        ch |= ui.checkbox(&mut m.emission_enabled, "").changed();
        ui.end_row();
        if m.emission_enabled {
            ui.label("    Color");
            ch |= color_rgb(ui, &mut m.emission);
            ui.end_row();
            ui.label("    Intensity");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut m.emission_intensity)
                        .speed(0.05)
                        .range(0.0..=100.0),
                )
                .changed();
            ui.end_row();
        }
        for (label, v) in [("Tiling", &mut m.tiling), ("Offset", &mut m.offset)] {
            ui.label(label);
            ui.horizontal(|ui| {
                for (axis, c) in ["X", "Y"].into_iter().zip(v.iter_mut()) {
                    ui.label(axis);
                    ch |= ui
                        .add(egui::DragValue::new(c).speed(0.01).max_decimals(3))
                        .changed();
                }
            });
            ui.end_row();
        }
        ch
    })
}

/// Edit a material asset in place and save it. Used inline under Mesh Renderer and for Project selection.
pub fn material_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let Some(mut m) = state.assets.materials.get(rel).cloned() else {
        ui.weak(format!("Missing material: {rel}"));
        return;
    };
    let textures = state.assets.textures.clone();
    if material_fields(ui, &format!("mat_{rel}"), &mut m, &textures)
        && let Err(err) = state.assets.update_material(rel, m)
    {
        state.console.warn(format!("Could not save {rel}: {err}"));
    }
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
    let mut chosen_script: Option<String> = None;
    let mut new_script = false;
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
                // Project scripts, then "New script".
                let scripts: Vec<String> = state
                    .assets
                    .scripts
                    .keys()
                    .filter(|p| needle.is_empty() || p.to_lowercase().contains(&needle))
                    .cloned()
                    .collect();
                ui.label(RichText::new("Scripts").color(theme::TEXT_DIM).small());
                for path in scripts {
                    let label = format!("📜 {}", AssetDatabase::material_name(Some(&path)));
                    if ui.add(egui::Button::new(label).frame(false)).clicked() {
                        chosen_script = Some(path);
                    }
                }
                if ui
                    .add(egui::Button::new("➕ New script").frame(false))
                    .clicked()
                {
                    new_script = true;
                }
            });
        });

    let clicked_outside = ui.input(|i| i.pointer.any_pressed())
        && !area.response.contains_pointer()
        && !resp.contains_pointer();
    if let Some(name) = chosen {
        state.push(Action::AddComponent(name));
    }
    if new_script {
        let folder = state.project.root.join("Assets").join("Scripts");
        let _ = std::fs::create_dir_all(&folder);
        match state.assets.create_script(&folder) {
            Ok(path) => {
                state.console.info(format!("Created {path}"));
                state.project.refresh();
                chosen_script = Some(path);
            }
            Err(err) => state
                .console
                .warn(format!("Could not create script: {err}")),
        }
    }
    let picked = chosen.is_some() || chosen_script.is_some();
    if let Some(path) = chosen_script {
        state.push(Action::AddScript { entity: e, path });
    }
    if picked || clicked_outside || ui.input(|i| i.key_pressed(Key::Escape)) {
        state.inspector_add_open = false;
        search.clear();
    }
    ui.data_mut(|d| d.insert_temp(search_id, search));
}

/// "moveSpeed" / "move_speed" → "Move Speed".
pub fn nicify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        if out.is_empty() || out.ends_with(' ') {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    out
}

/// Inspector sections for every script on `e`: "Name (Script)" with its public fields.
pub fn script_sections(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let Ok(scripts) = state.scene.world.get::<&Scripts>(e).map(|s| (*s).clone()) else {
        return;
    };
    for (slot, script) in scripts.list.iter().enumerate() {
        let id = ui.make_persistent_id(("script", slot, &script.path));
        let open =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
        let header = open.show_header(ui, |ui| {
            let mut on = script.enabled;
            if ui.checkbox(&mut on, "").changed() {
                if let Ok(mut s) = state.scene.world.get::<&mut Scripts>(e) {
                    s.list[slot].enabled = on;
                }
                state.mark_edited("Script");
            }
            ui.label(RichText::new(format!("📜 {} (Script)", nicify(&script.name()))).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("⋮", |ui| {
                    if ui.button("Reset").clicked() {
                        state.push(Action::ResetScript { entity: e, slot });
                        ui.close();
                    }
                    if ui.button("Remove Component").clicked() {
                        state.push(Action::RemoveScript { entity: e, slot });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Edit Script").clicked() {
                        open_script(state, &script.path);
                        ui.close();
                    }
                });
            });
        });
        header.body(|ui| script_fields(ui, state, e, slot, &script.path));
        ui.separator();
    }
}

fn script_fields(ui: &mut Ui, state: &mut EditorState, e: Entity, slot: usize, path: &str) {
    let meta = state.assets.scripts.get(path).cloned();
    let overrides = state
        .scene
        .world
        .get::<&Scripts>(e)
        .ok()
        .and_then(|s| s.list.get(slot).map(|r| r.fields.clone()))
        .unwrap_or_default();
    let mut edits: Vec<(String, ScriptValue)> = Vec::new();
    grid(ui, &format!("script_grid_{slot}"), |ui| {
        ui.label("Script");
        if ui
            .add(
                egui::Button::new(format!("📜 {}", AssetDatabase::material_name(Some(path))))
                    .frame(false),
            )
            .on_hover_text("Double-click to edit")
            .double_clicked()
        {
            open_script(state, path);
        }
        ui.end_row();
        match meta.as_ref().map(|m| &m.fields) {
            None => {
                ui.label(RichText::new("Missing script file").color(LogLevelColor::ERROR));
                ui.end_row();
            }
            Some(Err(err)) => {
                ui.label(RichText::new("Error").color(LogLevelColor::ERROR));
                ui.label(RichText::new(err).color(LogLevelColor::ERROR).small());
                ui.end_row();
            }
            Some(Ok(fields)) => {
                for (name, default) in fields {
                    let mut value = overrides
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| default.clone());
                    ui.label(nicify(name));
                    if script_value_ui(ui, &format!("{slot}_{name}"), &mut value) {
                        edits.push((name.clone(), value));
                    }
                    ui.end_row();
                }
            }
        }
        false
    });
    if !edits.is_empty() {
        if let Ok(mut s) = state.scene.world.get::<&mut Scripts>(e) {
            for (k, v) in edits {
                s.list[slot].fields.insert(k, v);
            }
        }
        state.mark_edited("Script");
    }
}

/// Widget for one script field value.
fn script_value_ui(ui: &mut Ui, id: &str, v: &mut ScriptValue) -> bool {
    match v {
        ScriptValue::Bool(b) => ui.checkbox(b, "").changed(),
        ScriptValue::Number(n) => ui.add(egui::DragValue::new(n).speed(0.1)).changed(),
        ScriptValue::Text(t) => ui
            .add(egui::TextEdit::singleline(t).desired_width(150.0))
            .changed(),
        ScriptValue::Vector3(a) => {
            ui.push_id(id, |ui| {
                ui.horizontal(|ui| {
                    let mut ch = false;
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.label(*axis);
                        ch |= ui
                            .add(egui::DragValue::new(&mut a[i]).speed(0.05))
                            .changed();
                    }
                    ch
                })
                .inner
            })
            .inner
        }
    }
}

/// Open a script in the system's default editor for `.lua` files.
pub fn open_script(state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    if let Err(err) = crate::assets::open_external(&abs) {
        state.console.warn(format!("Could not open {rel}: {err}"));
    }
}

/// Error colour for inline messages.
struct LogLevelColor;

impl LogLevelColor {
    const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 110, 100);
}
