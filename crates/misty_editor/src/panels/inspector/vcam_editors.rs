//! Inspector editors for the Camera Brain and Virtual Camera components.

use egui::{DragValue, Slider, Ui};
use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::vcam::{
    AimKind, BindingMode, BlendDefinition, BlendStyle, BodyKind, BrainUpdate, CameraBrain,
    CameraCollider, Composer, CustomBlend, Lens, Noise, NoiseProfile, OrbitAxis, Orbital,
    ThirdPerson, Transposer, VirtualCamera,
};

use super::card::{grid_sub_header, sub_header};
use super::editors::{combo, grid};
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 2] = ["Camera Brain", "Virtual Camera"];

pub fn icon(name: &str) -> Option<&'static str> {
    match name {
        "Camera Brain" => Some("🧠"),
        "Virtual Camera" => Some("📹"),
        _ => None,
    }
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Camera Brain" => w.get::<&CameraBrain>(e).ok().map(|c| c.enabled),
        _ => w.get::<&VirtualCamera>(e).ok().map(|c| c.enabled),
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Camera Brain" => w.get::<&mut CameraBrain>(e).map(|mut c| c.enabled = on),
        _ => w.get::<&mut VirtualCamera>(e).map(|mut c| c.enabled = on),
    };
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Camera Brain" => brain(ui, state, e),
        _ => vcam(ui, state, e),
    }
}

fn number(ui: &mut Ui, label: &str, tip: &str, v: &mut f32, speed: f64, lo: f32, hi: f32) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = ui
        .add(
            DragValue::new(v)
                .speed(speed)
                .range(lo..=hi)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

fn fraction(ui: &mut Ui, label: &str, tip: &str, v: &mut f32, hi: f32) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = ui.add(Slider::new(v, 0.0..=hi)).changed();
    ui.end_row();
    ch
}

fn pick<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    id: &str,
    v: &mut T,
    all: &[T],
    name: fn(T) -> &'static str,
) -> bool {
    ui.label(label);
    let ch = combo(ui, id, v, all, name);
    ui.end_row();
    ch
}

/// Vector row outside a grid; returns true when changed.
fn vec3(ui: &mut Ui, label: &str, v: &mut Vec3) -> bool {
    let mut a = v.to_array();
    let ch = vec3_row(ui, label, &mut a, 0.01);
    if ch {
        *v = Vec3::from(a);
    }
    ch
}

/// Scene object picker (`None` clears it).
fn object_field(
    ui: &mut Ui,
    label: &str,
    id: &str,
    v: &mut Option<Entity>,
    objs: &[(Entity, String)],
) -> bool {
    ui.label(label);
    let name = v
        .and_then(|t| objs.iter().find(|(e, _)| *e == t))
        .map_or("None (Transform)".to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, "None (Transform)").changed();
            for (e, n) in objs {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ui.end_row();
    ch
}

fn blend_row(ui: &mut Ui, id: &str, b: &mut BlendDefinition) -> bool {
    ui.horizontal(|ui| {
        let mut ch = combo(ui, id, &mut b.style, &BlendStyle::ALL, BlendStyle::label);
        ui.add_enabled_ui(b.style != BlendStyle::Cut, |ui| {
            ch |= ui
                .add(
                    DragValue::new(&mut b.duration)
                        .speed(0.01)
                        .range(0.0..=60.0)
                        .suffix(" s"),
                )
                .changed();
        });
        ch
    })
    .inner
}

fn brain(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut b) = state
        .scene
        .world
        .get::<&CameraBrain>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    // Live status while playing.
    if let Some(rt) = state.runtime.as_ref() {
        let (live, weight) = rt.with_cameras(|r| (r.live_camera(e), r.blend_weight(e)));
        let live = live.map_or("None".to_owned(), |l| state.scene.name(l));
        let text = match weight {
            Some(w) => format!("Live: {live}   (blending, {:.0} %)", w * 100.0),
            None => format!("Live: {live}"),
        };
        ui.label(crate::theme::bold(ui.ctx(), &text));
    }
    let mut ch = grid(ui, "brain_grid", |ui| {
        ui.label("Default Blend")
            .on_hover_text("Used between cameras without a custom blend.");
        let mut ch = blend_row(ui, "brain_default", &mut b.default_blend);
        ui.end_row();
        ch |= pick(
            ui,
            "Update Method",
            "brain_update",
            &mut b.update_method,
            &BrainUpdate::ALL,
            BrainUpdate::label,
        );
        ch
    });
    sub_header(ui, "Custom Blends");
    let names: Vec<String> = misty_engine::vcam::rig::active_cameras(&state.scene)
        .into_iter()
        .map(|v| state.scene.name(v))
        .collect();
    let mut remove = None;
    for (i, cb) in b.custom_blends.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ch |= camera_name(ui, &format!("cb_from{i}"), &mut cb.from, &names);
            ui.label("→");
            ch |= camera_name(ui, &format!("cb_to{i}"), &mut cb.to, &names);
            if ui.small_button("✕").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ch |= blend_row(ui, &format!("cb_blend{i}"), &mut cb.blend);
        });
    }
    if let Some(i) = remove {
        b.custom_blends.remove(i);
        ch = true;
    }
    if ui.button("+ Add Blend").clicked() {
        b.custom_blends.push(CustomBlend::default());
        ch = true;
    }
    if state
        .scene
        .world
        .get::<&misty_engine::scene::Camera>(e)
        .is_err()
    {
        ui.weak("Add a Camera to this object: the brain moves it.");
    }
    if ch {
        let _ = state.scene.world.insert_one(e, b);
    }
    ch
}

/// Virtual camera name picker; empty = any camera.
fn camera_name(ui: &mut Ui, id: &str, v: &mut String, names: &[String]) -> bool {
    let shown = if v.is_empty() {
        "Any Camera"
    } else {
        v.as_str()
    };
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .width(110.0)
        .selected_text(shown.to_owned())
        .show_ui(ui, |ui| {
            if ui.selectable_label(v.is_empty(), "Any Camera").clicked() {
                v.clear();
                ch = true;
            }
            for n in names {
                if ui.selectable_label(v == n, n).clicked() {
                    *v = n.clone();
                    ch = true;
                }
            }
        });
    ch
}

fn vcam(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut v) = state
        .scene
        .world
        .get::<&VirtualCamera>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let objs: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .filter(|(o, _)| *o != e)
        .map(|(o, _)| (o, state.scene.name(o)))
        .collect();
    if let Some(rt) = state.runtime.as_ref() {
        let live = rt.with_cameras(|r| r.is_live(e));
        let text = if !v.enabled {
            "Status: Disabled"
        } else if live {
            "Status: Live"
        } else {
            "Status: Standby"
        };
        ui.label(crate::theme::bold(ui.ctx(), text));
    }
    let mut ch = grid(ui, "vcam_grid", |ui| {
        ui.label("Priority")
            .on_hover_text("The highest-priority active camera is live.");
        let mut ch = ui.add(DragValue::new(&mut v.priority)).changed();
        ui.end_row();
        ch |= object_field(ui, "Follow", "vcam_follow", &mut v.follow, &objs);
        ch |= object_field(ui, "Look At", "vcam_look", &mut v.look_at, &objs);
        ch
    });
    sub_header(ui, "Lens");
    ch |= grid(ui, "vcam_lens", |ui| lens(ui, &mut v.lens));

    sub_header(ui, "Body");
    ch |= grid(ui, "vcam_body_kind", |ui| {
        pick(
            ui,
            "Algorithm",
            "vcam_body",
            &mut v.body,
            &BodyKind::ALL,
            BodyKind::label,
        )
    });
    ch |= match v.body {
        BodyKind::DoNothing => {
            ui.weak("The camera stays where its Transform puts it.");
            false
        }
        BodyKind::Transposer => transposer(ui, &mut v.transposer),
        BodyKind::ThirdPerson => third_person(ui, &mut v.third_person),
        BodyKind::Orbital => orbital(ui, &mut v.orbital),
    };
    if v.body != BodyKind::DoNothing && v.follow.is_none() {
        ui.weak("Needs a Follow target.");
    }

    sub_header(ui, "Aim");
    ch |= grid(ui, "vcam_aim_kind", |ui| {
        pick(
            ui,
            "Algorithm",
            "vcam_aim",
            &mut v.aim,
            &AimKind::ALL,
            AimKind::label,
        )
    });
    if v.aim == AimKind::Composer {
        ch |= composer(ui, &mut v.composer);
    }
    if v.aim != AimKind::DoNothing && v.look_at.is_none() {
        ui.weak("Needs a Look At target.");
    }

    sub_header(ui, "Noise");
    ch |= noise(ui, &mut v.noise);
    sub_header(ui, "Collider");
    ch |= collider(ui, &mut v.collider);
    if ch {
        let _ = state.scene.world.insert_one(e, v);
    }
    ch
}

fn lens(ui: &mut Ui, l: &mut Lens) -> bool {
    let mut ch = false;
    ui.label("Field of View");
    ch |= ui
        .add(Slider::new(&mut l.fov, 1.0..=179.0).suffix("°"))
        .changed();
    ui.end_row();
    ch |= number(ui, "Near Clip", "", &mut l.near, 0.01, 0.01, 1e4);
    ch |= number(ui, "Far Clip", "", &mut l.far, 1.0, 0.02, 1e6);
    ui.label("Dutch")
        .on_hover_text("Roll around the view axis.");
    ch |= ui
        .add(Slider::new(&mut l.dutch, -180.0..=180.0).suffix("°"))
        .changed();
    ui.end_row();
    if l.far <= l.near {
        l.far = l.near + 0.01;
    }
    ch
}

fn transposer(ui: &mut Ui, t: &mut Transposer) -> bool {
    let mut ch = grid(ui, "vcam_transposer", |ui| {
        pick(
            ui,
            "Binding Mode",
            "tr_binding",
            &mut t.binding,
            &BindingMode::ALL,
            BindingMode::label,
        )
    });
    ch |= vec3(ui, "Follow Offset", &mut t.offset);
    ch |= vec3(ui, "Damping", &mut t.damping);
    t.damping = t.damping.max(Vec3::ZERO);
    ch
}

fn third_person(ui: &mut Ui, t: &mut ThirdPerson) -> bool {
    let mut ch = vec3(ui, "Shoulder Offset", &mut t.shoulder_offset);
    ch |= grid(ui, "vcam_3rd", |ui| {
        let mut ch = number(
            ui,
            "Vertical Arm",
            "Height of the arm above the shoulder.",
            &mut t.vertical_arm_length,
            0.01,
            -10.0,
            10.0,
        );
        ch |= fraction(
            ui,
            "Camera Side",
            "0 = left shoulder, 1 = right shoulder.",
            &mut t.camera_side,
            1.0,
        );
        ch |= number(
            ui,
            "Camera Distance",
            "",
            &mut t.camera_distance,
            0.01,
            0.0,
            1e3,
        );
        ch
    });
    ch |= vec3(ui, "Damping", &mut t.damping);
    t.damping = t.damping.max(Vec3::ZERO);
    ch |= grid(ui, "vcam_3rd_dz", |ui| {
        let mut ch = fraction(
            ui,
            "Dead Zone Width",
            "Screen fraction the target moves in before the camera follows.",
            &mut t.dead_zone.x,
            1.0,
        );
        ch |= fraction(ui, "Dead Zone Height", "", &mut t.dead_zone.y, 1.0);
        ch
    });
    t.dead_zone = t.dead_zone.clamp(Vec2::ZERO, Vec2::ONE);
    ch
}

fn axis_rows(ui: &mut Ui, id: &str, a: &mut OrbitAxis) -> bool {
    let mut ch = number(ui, "Value", "Current angle.", &mut a.value, 0.5, -1e4, 1e4);
    ui.label("Range");
    ui.horizontal(|ui| {
        ch |= ui
            .add(DragValue::new(&mut a.min).speed(0.5).range(-360.0..=360.0))
            .changed();
        ch |= ui
            .add(DragValue::new(&mut a.max).speed(0.5).range(-360.0..=360.0))
            .changed();
        ch |= ui.checkbox(&mut a.wrap, "Wrap").changed();
    });
    ui.end_row();
    ch |= number(
        ui,
        "Speed",
        "Degrees per second at full input.",
        &mut a.speed,
        1.0,
        0.0,
        1e4,
    );
    ui.label("Input Axis")
        .on_hover_text("Input Manager axis; empty = no input.");
    ui.horizontal(|ui| {
        ch |= ui
            .add(egui::TextEdit::singleline(&mut a.input_axis).id_salt(id))
            .changed();
    });
    ui.end_row();
    ui.label("Invert");
    ch |= ui.checkbox(&mut a.invert, "").changed();
    ui.end_row();
    a.clamp();
    ch
}

fn orbital(ui: &mut Ui, o: &mut Orbital) -> bool {
    let mut ch = grid(ui, "vcam_orbital", |ui| {
        let mut ch = pick(
            ui,
            "Binding Mode",
            "orb_binding",
            &mut o.binding,
            &[BindingMode::World, BindingMode::LockToTargetWithWorldUp],
            BindingMode::label,
        );
        ch |= number(ui, "Radius", "", &mut o.radius, 0.01, 0.0, 1e4);
        ch |= number(
            ui,
            "Height Offset",
            "Orbit centre above the target.",
            &mut o.height_offset,
            0.01,
            -1e3,
            1e3,
        );
        grid_sub_header(ui, "Horizontal Axis");
        ch |= axis_rows(ui, "orb_x_input", &mut o.x_axis);
        grid_sub_header(ui, "Vertical Axis");
        ch |= axis_rows(ui, "orb_y_input", &mut o.y_axis);
        ch
    });
    ch |= vec3(ui, "Damping", &mut o.damping);
    o.damping = o.damping.max(Vec3::ZERO);
    ch
}

fn composer(ui: &mut Ui, c: &mut Composer) -> bool {
    let mut ch = vec3(ui, "Tracked Offset", &mut c.tracked_offset);
    ch |= grid(ui, "vcam_composer", |ui| {
        let mut ch = fraction(
            ui,
            "Screen X",
            "Where the target rests (0.5 = centre).",
            &mut c.screen_x,
            1.0,
        );
        ch |= fraction(ui, "Screen Y", "0 = top, 1 = bottom.", &mut c.screen_y, 1.0);
        ch |= fraction(
            ui,
            "Dead Zone Width",
            "The camera ignores target movement inside the dead zone.",
            &mut c.dead_zone_width,
            2.0,
        );
        ch |= fraction(ui, "Dead Zone Height", "", &mut c.dead_zone_height, 2.0);
        ch |= fraction(
            ui,
            "Soft Zone Width",
            "The target never leaves the soft zone.",
            &mut c.soft_zone_width,
            2.0,
        );
        ch |= fraction(ui, "Soft Zone Height", "", &mut c.soft_zone_height, 2.0);
        ch |= number(
            ui,
            "Horizontal Damping",
            "Seconds to turn towards the target.",
            &mut c.horizontal_damping,
            0.01,
            0.0,
            20.0,
        );
        ch |= number(
            ui,
            "Vertical Damping",
            "",
            &mut c.vertical_damping,
            0.01,
            0.0,
            20.0,
        );
        ch
    });
    c.soft_zone_width = c.soft_zone_width.max(c.dead_zone_width);
    c.soft_zone_height = c.soft_zone_height.max(c.dead_zone_height);
    ui.weak("Select this camera and look at the Game view to see the zones.");
    ch
}

fn noise(ui: &mut Ui, n: &mut Noise) -> bool {
    grid(ui, "vcam_noise", |ui| {
        ui.label("Enabled");
        let mut ch = ui.checkbox(&mut n.enabled, "").changed();
        ui.end_row();
        let on = n.enabled;
        ui.add_enabled_ui(on, |ui| ui.label("Profile"));
        ch |= ui
            .add_enabled_ui(on, |ui| {
                combo(
                    ui,
                    "noise_profile",
                    &mut n.profile,
                    &NoiseProfile::ALL,
                    NoiseProfile::label,
                )
            })
            .inner;
        ui.end_row();
        for (label, v, hi) in [
            ("Amplitude Gain", &mut n.amplitude_gain, 10.0),
            ("Frequency Gain", &mut n.frequency_gain, 10.0),
        ] {
            ui.add_enabled_ui(on, |ui| ui.label(label));
            ch |= ui
                .add_enabled_ui(on, |ui| ui.add(Slider::new(v, 0.0..=hi)).changed())
                .inner;
            ui.end_row();
        }
        ui.add_enabled_ui(on, |ui| ui.label("Seed"))
            .inner
            .on_hover_text("Same seed, same shake.");
        ch |= ui
            .add_enabled_ui(on, |ui| ui.add(DragValue::new(&mut n.seed)).changed())
            .inner;
        ui.end_row();
        ch
    })
}

fn collider(ui: &mut Ui, c: &mut CameraCollider) -> bool {
    grid(ui, "vcam_collider", |ui| {
        ui.label("Enabled")
            .on_hover_text("Pull the camera in front of colliders that hide the target.");
        let mut ch = ui.checkbox(&mut c.enabled, "").changed();
        ui.end_row();
        let on = c.enabled;
        ui.add_enabled_ui(on, |ui| ui.label("Collide Against"));
        ch |= ui
            .add_enabled_ui(on, |ui| {
                super::layer_mask::mask_field(ui, "vcam_col_mask", &mut c.collide_against)
            })
            .inner;
        ui.end_row();
        ui.add_enabled_ui(on, |ui| ui.label("Ignore Tag"));
        ch |= ui
            .add_enabled_ui(on, |ui| {
                ui.text_edit_singleline(&mut c.ignore_tag).changed()
            })
            .inner;
        ui.end_row();
        for (label, tip, v, hi) in [
            (
                "Camera Radius",
                "Gap kept from obstacles.",
                &mut c.camera_radius,
                10.0,
            ),
            (
                "Minimum Distance",
                "Never closer to the target than this.",
                &mut c.minimum_distance,
                100.0,
            ),
            (
                "Damping",
                "Seconds to move back out once the view clears.",
                &mut c.damping,
                10.0,
            ),
            (
                "Damping When Occluded",
                "Seconds to move in (0 = at once).",
                &mut c.damping_when_occluded,
                10.0,
            ),
        ] {
            ui.add_enabled_ui(on, |ui| ui.label(label).on_hover_text(tip));
            ch |= ui
                .add_enabled_ui(on, |ui| {
                    ui.add(DragValue::new(v).speed(0.01).range(0.0..=hi))
                        .changed()
                })
                .inner;
            ui.end_row();
        }
        ch
    })
}
