//! Inspector editors for the Camera Brain, Virtual Camera, Target Group and Impulse Source
//! components.

use egui::{DragValue, Slider, Ui};
use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::vcam::{
    AimKind, BLEND_LIST_EXTENSION, BindingMode, BlendDefinition, BlendList, BlendStyle, BodyKind,
    BrainUpdate, CameraBrain, CameraCollider, Composer, Confiner, CustomBlend, FramingMode,
    GroupAdjustment, GroupFraming, GroupMember, GroupPositionMode, ImpulseListener, ImpulseShape,
    ImpulseSource, ImpulseTrigger, Lens, Noise, NoiseProfile, OrbitAxis, OrbitStyle, Orbital,
    TargetGroup, ThirdPerson, Transposer, VirtualCamera,
};

use super::card::{grid_sub_header, sub_header};
use super::editors::{combo, grid};
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 4] = [
    "Camera Brain",
    "Virtual Camera",
    "Target Group",
    "Impulse Source",
];

pub fn icon(name: &str) -> Option<&'static str> {
    match name {
        "Camera Brain" => Some("🧠"),
        "Virtual Camera" => Some("📹"),
        "Target Group" => Some("👥"),
        "Impulse Source" => Some("💥"),
        _ => None,
    }
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Camera Brain" => w.get::<&CameraBrain>(e).ok().map(|c| c.enabled),
        "Target Group" => None,
        "Impulse Source" => w.get::<&ImpulseSource>(e).ok().map(|c| c.enabled),
        _ => w.get::<&VirtualCamera>(e).ok().map(|c| c.enabled),
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Camera Brain" => w.get::<&mut CameraBrain>(e).map(|mut c| c.enabled = on),
        "Target Group" => Ok(()),
        "Impulse Source" => w.get::<&mut ImpulseSource>(e).map(|mut c| c.enabled = on),
        _ => w.get::<&mut VirtualCamera>(e).map(|mut c| c.enabled = on),
    };
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Camera Brain" => brain(ui, state, e),
        "Target Group" => target_group(ui, state, e),
        "Impulse Source" => impulse_source(ui, state, e),
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
    let mut ch = ui
        .horizontal(|ui| {
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
        .inner;
    if b.style == BlendStyle::Custom {
        ch |= curve_editor(ui, id, &mut b.curve);
    }
    ch
}

/// A Custom blend's Bézier control points, with a small plot of the curve.
fn curve_editor(ui: &mut Ui, id: &str, c: &mut [f32; 4]) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 40.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 3.0, crate::theme::CARD_BG);
        let at = |x: f32, y: f32| {
            egui::pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        };
        let pts: Vec<egui::Pos2> = (0..=24)
            .map(|i| {
                let t = i as f32 / 24.0;
                at(t, misty_engine::vcam::bezier_ease(*c, t))
            })
            .collect();
        p.add(egui::Shape::line(
            pts,
            egui::Stroke::new(1.5, crate::theme::PREFAB),
        ));
        ui.vertical(|ui| {
            for (row, label) in [(0, "P1"), (2, "P2")] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ch |= ui
                        .add(
                            DragValue::new(&mut c[row])
                                .speed(0.01)
                                .range(0.0..=1.0)
                                .prefix("x "),
                        )
                        .changed();
                    ch |= ui
                        .add(
                            DragValue::new(&mut c[row + 1])
                                .speed(0.01)
                                .range(-1.0..=2.0)
                                .prefix("y "),
                        )
                        .changed();
                });
            }
        });
    })
    .response
    .on_hover_text(format!("{id}: cubic Bézier from (0, 0) to (1, 1)"));
    ch
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
    ch |= blend_list_field(ui, state, &mut b);
    if let Some(path) = b.blend_list.clone() {
        blend_list_entries(ui, state, &path, &names);
        ui.weak("The brain's own blends below apply when the list has no match.");
    }
    ch |= custom_blend_rows(ui, "cb", &mut b.custom_blends, &names);
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
    if v.aim == AimKind::GroupComposer {
        ch |= group_framing(ui, &mut v.group_framing);
        let is_group = v
            .look_at
            .is_some_and(|t| state.scene.world.get::<&TargetGroup>(t).is_ok());
        if v.look_at.is_some() && !is_group {
            ui.weak("Look At has no Target Group: it is framed as one small target.");
        }
        ui.collapsing("Composition", |ui| ch |= composer(ui, &mut v.composer));
    }
    if v.aim != AimKind::DoNothing && v.look_at.is_none() {
        ui.weak("Needs a Look At target.");
    }

    sub_header(ui, "Noise");
    ch |= noise(ui, &mut v.noise);
    sub_header(ui, "Collider");
    ch |= collider(ui, &mut v.collider);
    sub_header(ui, "Confiner");
    ch |= confiner(ui, &mut v.confiner, &objs);
    sub_header(ui, "Impulse Listener");
    ch |= impulse_listener(ui, &mut v.impulse);
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
    ui.label("Input Action").on_hover_text(
        "Input System action (\"Map/Action\") from the playing Player Inputs; wins over the axis. The horizontal axis reads its x, the vertical axis its y.",
    );
    ui.horizontal(|ui| {
        ch |= ui
            .add(
                egui::TextEdit::singleline(&mut a.input_action)
                    .id_salt((id, "action"))
                    .hint_text("Player/Look"),
            )
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
        let style = o.style;
        ch |= pick(
            ui,
            "Orbits",
            "orb_style",
            &mut o.style,
            &OrbitStyle::ALL,
            OrbitStyle::label,
        );
        if o.style != style {
            // The vertical axis changes meaning: an angle (sphere) or 0..1 (rings).
            let fresh = match o.style {
                OrbitStyle::ThreeRings => Orbital::three_rings().y_axis,
                OrbitStyle::Sphere => Orbital::default().y_axis,
            };
            o.y_axis = OrbitAxis {
                input_axis: o.y_axis.input_axis.clone(),
                input_action: o.y_axis.input_action.clone(),
                ..fresh
            };
        }
        if o.style == OrbitStyle::ThreeRings {
            for (i, name) in ["Top Rig", "Middle Rig", "Bottom Rig"].iter().enumerate() {
                ui.label(*name)
                    .on_hover_text("Height above the target and radius of this ring.");
                ui.horizontal(|ui| {
                    ch |= ui
                        .add(
                            DragValue::new(&mut o.orbits[i].height)
                                .speed(0.01)
                                .prefix("Height "),
                        )
                        .changed();
                    ch |= ui
                        .add(
                            DragValue::new(&mut o.orbits[i].radius)
                                .speed(0.01)
                                .range(0.0..=1e4)
                                .prefix("Radius "),
                        )
                        .changed();
                });
                ui.end_row();
            }
            ch |= fraction(
                ui,
                "Spline Curvature",
                "0 = straight lines between the rings, 1 = a smooth curve through them.",
                &mut o.spline_curvature,
                1.0,
            );
        } else {
            ch |= number(ui, "Radius", "", &mut o.radius, 0.01, 0.0, 1e4);
        }
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

fn group_framing(ui: &mut Ui, g: &mut GroupFraming) -> bool {
    grid(ui, "vcam_group", |ui| {
        let mut ch = fraction(
            ui,
            "Framing Size",
            "Fraction of the screen the group's bounds fill.",
            &mut g.framing_size,
            1.0,
        );
        ch |= pick(
            ui,
            "Framing Mode",
            "grp_mode",
            &mut g.framing_mode,
            &FramingMode::ALL,
            FramingMode::label,
        );
        ch |= pick(
            ui,
            "Adjustment",
            "grp_adjust",
            &mut g.adjustment,
            &GroupAdjustment::ALL,
            GroupAdjustment::label,
        );
        ch |= number(
            ui,
            "Damping",
            "Seconds to settle on the new distance and field of view.",
            &mut g.damping,
            0.01,
            0.0,
            20.0,
        );
        let dolly = g.adjustment != GroupAdjustment::ZoomOnly;
        let zoom = g.adjustment != GroupAdjustment::DollyOnly;
        if dolly {
            ch |= number(
                ui,
                "Max Dolly In",
                "Furthest it moves towards the group.",
                &mut g.max_dolly_in,
                0.1,
                0.0,
                1e4,
            );
            ch |= number(ui, "Max Dolly Out", "", &mut g.max_dolly_out, 0.1, 0.0, 1e4);
            ch |= number(
                ui,
                "Minimum Distance",
                "",
                &mut g.minimum_distance,
                0.1,
                0.0,
                1e4,
            );
            ch |= number(
                ui,
                "Maximum Distance",
                "",
                &mut g.maximum_distance,
                0.1,
                0.0,
                1e5,
            );
        }
        if zoom {
            ch |= number(ui, "Minimum FOV", "", &mut g.minimum_fov, 0.1, 1.0, 179.0);
            ch |= number(ui, "Maximum FOV", "", &mut g.maximum_fov, 0.1, 1.0, 179.0);
        }
        g.maximum_fov = g.maximum_fov.max(g.minimum_fov);
        g.maximum_distance = g.maximum_distance.max(g.minimum_distance);
        ch
    })
}

fn confiner(ui: &mut Ui, c: &mut Confiner, objs: &[(Entity, String)]) -> bool {
    grid(ui, "vcam_confiner", |ui| {
        ui.label("Enabled")
            .on_hover_text("Keep the camera inside a volume.");
        let mut ch = ui.checkbox(&mut c.enabled, "").changed();
        ui.end_row();
        ch |= object_field(ui, "Bounding Volume", "vcam_conf_vol", &mut c.volume, objs);
        ch |= number(
            ui,
            "Damping",
            "Seconds to settle back inside (0 = at once).",
            &mut c.damping,
            0.01,
            0.0,
            10.0,
        );
        ui.label("");
        ui.weak("Its Box or Sphere Collider, else its unit cube.");
        ui.end_row();
        ch
    })
}

fn impulse_listener(ui: &mut Ui, l: &mut ImpulseListener) -> bool {
    grid(ui, "vcam_impulse", |ui| {
        ui.label("Enabled")
            .on_hover_text("Shake with impulses from Impulse Sources near the camera.");
        let mut ch = ui.checkbox(&mut l.enabled, "").changed();
        ui.end_row();
        ch |= number(ui, "Gain", "", &mut l.gain, 0.01, 0.0, 20.0);
        ui.label("Channel Mask");
        ch |= super::layer_mask::mask_field(ui, "vcam_imp_mask", &mut l.channel_mask);
        ui.end_row();
        ui.label("Use 2D Distance")
            .on_hover_text("Ignore height when measuring the distance to a source.");
        ch |= ui.checkbox(&mut l.use_2d_distance, "").changed();
        ui.end_row();
        ch
    })
}

fn target_group(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut g) = state
        .scene
        .world
        .get::<&TargetGroup>(e)
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
    let mut ch = grid(ui, "tg_grid", |ui| {
        pick(
            ui,
            "Position Mode",
            "tg_mode",
            &mut g.position_mode,
            &GroupPositionMode::ALL,
            GroupPositionMode::label,
        )
    });
    sub_header(ui, "Targets");
    let mut remove = None;
    for (i, m) in g.members.iter_mut().enumerate() {
        ui.push_id(("tg_member", i), |ui| {
            ch |= grid(ui, "tg_member_grid", |ui| {
                let mut ch = object_field(ui, "Object", "tg_obj", &mut m.target, &objs);
                ch |= number(
                    ui,
                    "Weight",
                    "0 = ignored; lighter members count less.",
                    &mut m.weight,
                    0.01,
                    0.0,
                    100.0,
                );
                ch |= number(
                    ui,
                    "Radius",
                    "Room the member needs on screen.",
                    &mut m.radius,
                    0.01,
                    0.0,
                    1e3,
                );
                ch
            });
            if ui.small_button("✕ Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.separator();
    }
    if let Some(i) = remove {
        g.members.remove(i);
        ch = true;
    }
    ui.horizontal(|ui| {
        if ui.button("+ Add Target").clicked() {
            g.members.push(GroupMember::default());
            ch = true;
        }
    });
    if let Some(b) = g.bounds(&state.scene, e) {
        ui.weak(format!("Bounds radius {:.2} m", b.radius));
    }
    if ch {
        let _ = state.scene.world.insert_one(e, g);
    }
    ch
}

fn impulse_source(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut s) = state
        .scene
        .world
        .get::<&ImpulseSource>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = grid(ui, "imp_grid", |ui| {
        let mut ch = pick(
            ui,
            "Shape",
            "imp_shape",
            &mut s.shape,
            &ImpulseShape::ALL,
            ImpulseShape::label,
        );
        ch |= number(
            ui,
            "Amplitude Gain",
            "",
            &mut s.amplitude_gain,
            0.01,
            0.0,
            100.0,
        );
        ch |= number(
            ui,
            "Frequency Gain",
            "Speeds the shake up (Explosion, Rumble).",
            &mut s.frequency_gain,
            0.01,
            0.0,
            100.0,
        );
        grid_sub_header(ui, "Envelope");
        ch |= number(
            ui,
            "Attack",
            "Seconds to rise.",
            &mut s.attack,
            0.01,
            0.0,
            10.0,
        );
        ch |= number(
            ui,
            "Sustain",
            "Seconds at full strength.",
            &mut s.sustain,
            0.01,
            0.0,
            10.0,
        );
        ch |= number(
            ui,
            "Decay",
            "Seconds to fade out.",
            &mut s.decay,
            0.01,
            0.0,
            10.0,
        );
        grid_sub_header(ui, "Spread");
        ch |= number(
            ui,
            "Impact Radius",
            "Listeners this close feel it fully.",
            &mut s.impact_radius,
            0.1,
            0.0,
            1e5,
        );
        ch |= number(
            ui,
            "Dissipation Distance",
            "Beyond the impact radius it fades over this distance.",
            &mut s.dissipation_distance,
            0.1,
            0.0,
            1e5,
        );
        ui.label("Channel");
        ch |= super::layer_mask::mask_field(ui, "imp_channel", &mut s.channel);
        ui.end_row();
        ch |= pick(
            ui,
            "Trigger",
            "imp_trigger",
            &mut s.trigger,
            &ImpulseTrigger::ALL,
            ImpulseTrigger::label,
        );
        if s.trigger == ImpulseTrigger::CollisionEnter {
            ui.label("Collide With");
            ch |= super::layer_mask::mask_field(ui, "imp_collide", &mut s.collide_with);
            ui.end_row();
        }
        ch
    });
    ch |= vec3(ui, "Default Velocity", &mut s.default_velocity);
    ui.weak("Scripts: GetComponent(\"ImpulseSource\"):GenerateImpulse(). Signals and Control clips fire it too.");
    if ch {
        let _ = state.scene.world.insert_one(e, s);
    }
    ch
}

/// `.blends` files in the project's Assets folder (project-relative paths).
fn blend_lists(state: &EditorState) -> Vec<String> {
    let root = &state.project.root;
    let mut out = Vec::new();
    let mut stack = vec![root.join("Assets")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case(BLEND_LIST_EXTENSION))
                && let Ok(rel) = p.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// Brain: the Blend List asset field (custom blends shared by brains). "New" moves the
/// inline custom blends into a new asset. Returns true when the brain changed.
fn blend_list_field(ui: &mut Ui, state: &mut EditorState, b: &mut CameraBrain) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("Blend List")
            .on_hover_text("A .blends asset with custom blends, checked before the inline ones.");
        let shown = b
            .blend_list
            .as_deref()
            .map(misty_engine::scene::clip_name)
            .unwrap_or_else(|| "None".into());
        let r = egui::ComboBox::from_id_salt("brain_blend_list")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut b.blend_list, None, "None")
                    .changed();
                for p in blend_lists(state) {
                    let n = misty_engine::scene::clip_name(&p);
                    ch |= ui.selectable_value(&mut b.blend_list, Some(p), n).changed();
                }
            });
        let _ = r;
        if ui
            .small_button("New")
            .on_hover_text("Create Assets/Cameras/Blends.blends from the inline blends")
            .clicked()
        {
            let rel = (0..)
                .map(|n| match n {
                    0 => "Assets/Cameras/Blends.blends".to_owned(),
                    n => format!("Assets/Cameras/Blends {n}.blends"),
                })
                .find(|r| !state.project.root.join(r).exists())
                .unwrap_or_default();
            let list = BlendList {
                blends: std::mem::take(&mut b.custom_blends),
            };
            match list.save(&state.project.root.join(&rel)) {
                Ok(()) => {
                    state.console.info(format!("Created {rel}"));
                    b.blend_list = Some(rel);
                    state.refresh_assets();
                }
                Err(e) => state
                    .console
                    .warn(format!("Could not create blend list: {e}")),
            }
            ch = true;
        }
    });
    ch
}

/// The brain's blend list asset entries, edited in place (saved on change).
fn blend_list_entries(ui: &mut Ui, state: &mut EditorState, path: &str, names: &[String]) {
    let file = state.project.root.join(path);
    let mut list = match BlendList::load(&file) {
        Ok(l) => l,
        Err(e) => {
            ui.colored_label(crate::theme::WARN, e);
            return;
        }
    };
    let mut ch = custom_blend_rows(ui, "bl", &mut list.blends, names);
    if ui.button("+ Add To Blend List").clicked() {
        list.blends.push(CustomBlend::default());
        ch = true;
    }
    if ch && let Err(e) = list.save(&file) {
        state.console.warn(format!("Could not save {path}: {e}"));
    }
}

/// From → to rows with their blends; returns true when something changed.
fn custom_blend_rows(
    ui: &mut Ui,
    id: &str,
    blends: &mut Vec<CustomBlend>,
    names: &[String],
) -> bool {
    let mut ch = false;
    let mut remove = None;
    for (i, cb) in blends.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ch |= camera_name(ui, &format!("{id}_from{i}"), &mut cb.from, names);
            ui.label("→");
            ch |= camera_name(ui, &format!("{id}_to{i}"), &mut cb.to, names);
            if ui.small_button("✕").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.vertical(|ui| ch |= blend_row(ui, &format!("{id}_blend{i}"), &mut cb.blend));
        });
    }
    if let Some(i) = remove {
        blends.remove(i);
        ch = true;
    }
    ch
}
