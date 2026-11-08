//! Particle System Inspector: the main module, then one collapsible section per module with an
//! enable checkbox in its header, in the familiar order (Emission, Shape, Velocity / Limit
//! Velocity / Force over Lifetime, Color over Lifetime, Color by Speed, Size over Lifetime /
//! by Speed, Rotation over Lifetime / by Speed, Noise, Collision, Sub Emitters, Texture Sheet
//! Animation, Trails, Renderer).
//!
//! Number fields have the ▾ menu of modes (Constant, Curve, Random Between Two Constants,
//! Random Between Two Curves); curves show a small preview that opens an editor below the row.
//! Colour fields likewise switch between colours and gradients.

use egui::{Color32, DragValue, RichText, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::particles::{
    AlphaKey, Burst, CollisionPlane, CollisionType, ColorKey, ColorMode, Curve, CurveMode,
    Gradient, MinMax, MinMaxColor, ParticleBlend, ParticleSystem, RenderMode, ShapeKind,
    SheetAnimation, SimulationSpace, SubEmitter, SubEmitterTrigger, TrailMode,
};

use super::editors::{combo, grid};
use crate::state::EditorState;
use crate::theme;

pub fn particle_system(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut p) = state
        .scene
        .world
        .get::<&ParticleSystem>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let textures = state.assets.textures.clone();
    let children: Vec<String> = state
        .scene
        .children(e)
        .into_iter()
        .filter(|c| state.scene.world.get::<&ParticleSystem>(*c).is_ok())
        .map(|c| state.scene.name(c))
        .collect();
    let mut ch = main_module(ui, &mut p);
    // Each module's checkbox edits a copy of its flag, so the body can borrow the module.
    macro_rules! section {
        ($title:expr, $field:ident, $body:expr) => {{
            let mut on = p.$field.enabled;
            ch |= module(ui, $title, &mut on, |ui| $body(ui, &mut p.$field));
            p.$field.enabled = on;
        }};
    }
    section!("Emission", emission, emission);
    section!("Shape", shape, shape);
    section!(
        "Velocity over Lifetime",
        velocity_over_lifetime,
        velocity_over_lifetime
    );
    section!(
        "Limit Velocity over Lifetime",
        limit_velocity_over_lifetime,
        limit_velocity
    );
    section!(
        "Force over Lifetime",
        force_over_lifetime,
        force_over_lifetime
    );
    section!(
        "Color over Lifetime",
        color_over_lifetime,
        color_over_lifetime
    );
    section!("Color by Speed", color_by_speed, color_by_speed);
    section!("Size over Lifetime", size_over_lifetime, size_over_lifetime);
    section!("Size by Speed", size_by_speed, size_by_speed);
    section!(
        "Rotation over Lifetime",
        rotation_over_lifetime,
        rotation_over_lifetime
    );
    section!("Rotation by Speed", rotation_by_speed, rotation_by_speed);
    section!("Noise", noise, noise);
    section!("Collision", collision, collision);
    let mut on = p.sub_emitters.enabled;
    ch |= module(ui, "Sub Emitters", &mut on, |ui| {
        sub_emitters(ui, &mut p.sub_emitters, &children)
    });
    p.sub_emitters.enabled = on;
    section!(
        "Texture Sheet Animation",
        texture_sheet_animation,
        texture_sheet
    );
    let mut on = p.trails.enabled;
    ch |= module(ui, "Trails", &mut on, |ui| {
        trails(ui, &mut p.trails, &textures)
    });
    p.trails.enabled = on;
    let mut on = p.renderer.enabled;
    ch |= module(ui, "Renderer", &mut on, |ui| {
        renderer(ui, &mut p.renderer, &textures)
    });
    p.renderer.enabled = on;
    if ch {
        let _ = state.scene.world.insert_one(e, p);
    }
    ch
}

/// A module foldout: enable checkbox + title in the header, `body` inside.
fn module(
    ui: &mut Ui,
    title: &str,
    enabled: &mut bool,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let id = ui.make_persistent_id(("particle_module", title));
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let mut ch = false;
    let header = state.show_header(ui, |ui| {
        ch |= ui.checkbox(enabled, "").changed();
        ui.label(crate::theme::bold(ui.ctx(), title));
    });
    header.body(|ui| ch |= body(ui));
    ch
}

fn main_module(ui: &mut Ui, p: &mut ParticleSystem) -> bool {
    let m = &mut p.main;
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Particle System"))
        .id_salt("particle_main")
        .default_open(true)
        .show(ui, |ui| {
            ch = grid(ui, "particle_main_grid", |ui| {
                let mut ch = false;
                ch |= row(ui, "Duration", |ui| drag(ui, &mut m.duration, 0.05, 0.05));
                ch |= row(ui, "Looping", |ui| {
                    ui.checkbox(&mut m.looping, "").changed()
                });
                ch |= row(ui, "Prewarm", |ui| {
                    ui.add_enabled(m.looping, egui::Checkbox::without_text(&mut m.prewarm))
                        .changed()
                });
                ch |= row(ui, "Start Delay", |ui| {
                    min_max(ui, "sd", &mut m.start_delay, 0.0)
                });
                ch |= row(ui, "Start Lifetime", |ui| {
                    min_max(ui, "sl", &mut m.start_lifetime, 0.01)
                });
                ch |= row(ui, "Start Speed", |ui| {
                    min_max(ui, "ss", &mut m.start_speed, f32::MIN)
                });
                ch |= row(ui, "Start Size", |ui| {
                    min_max(ui, "sz", &mut m.start_size, 0.0)
                });
                ch |= row(ui, "Start Rotation", |ui| {
                    min_max(ui, "sr", &mut m.start_rotation, f32::MIN)
                });
                ch |= row(ui, "Start Color", |ui| {
                    min_max_color(ui, "sc", &mut m.start_color)
                });
                ch |= row(ui, "Gravity Modifier", |ui| {
                    drag(ui, &mut m.gravity_modifier, 0.01, f32::MIN)
                });
                ch |= row(ui, "Simulation Space", |ui| {
                    space(ui, "p_space", &mut m.simulation_space)
                });
                ch |= row(ui, "Play On Awake*", |ui| {
                    ui.checkbox(&mut m.play_on_awake, "").changed()
                });
                ch |= row(ui, "Max Particles", |ui| {
                    ui.add(DragValue::new(&mut m.max_particles).range(0..=100_000))
                        .changed()
                });
                let mut auto = p.seed == 0;
                ch |= row(ui, "Auto Random Seed", |ui| {
                    let c = ui.checkbox(&mut auto, "").changed();
                    if c {
                        p.seed = if auto { 0 } else { 1 };
                    }
                    c
                });
                if !auto {
                    ch |= row(ui, "Random Seed", |ui| {
                        ui.add(DragValue::new(&mut p.seed).range(1..=u32::MAX))
                            .changed()
                    });
                }
                ch
            });
        });
    ch
}

fn row(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.label(label);
    let ch = field(ui);
    ui.end_row();
    ch
}

fn drag(ui: &mut Ui, v: &mut f32, speed: f32, min: f32) -> bool {
    ui.add(DragValue::new(v).speed(speed).range(min..=f32::MAX))
        .changed()
}

fn unit(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(DragValue::new(v).speed(0.01).range(0.0..=1.0))
        .changed()
}

fn space(ui: &mut Ui, id: &str, v: &mut SimulationSpace) -> bool {
    combo(ui, id, v, &SimulationSpace::ALL, SimulationSpace::label)
}

/// Is the curve editor under the field `id` open?
fn editor_open(ui: &Ui, id: egui::Id) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false))
}

fn toggle_editor(ui: &Ui, id: egui::Id) {
    let open = editor_open(ui, id);
    ui.ctx().data_mut(|d| d.insert_temp(id, !open));
}

/// A clickable curve preview.
fn curve_button(ui: &mut Ui, c: &Curve, open_id: egui::Id) {
    let (rect, resp) = ui.allocate_exact_size(vec2(56.0, 18.0), egui::Sense::click());
    crate::panels::effect_graph::fields::paint_curve(ui.painter(), rect, c);
    if resp.on_hover_text("Click to edit the curve").clicked() {
        toggle_editor(ui, open_id);
    }
}

/// A number field with the mode menu: constants as drag values, curves as previews that open
/// an editor (curve keys) with the multiplier.
fn min_max(ui: &mut Ui, id: &str, v: &mut MinMax, min: f32) -> bool {
    ui.push_id(id, |ui| {
        let open_id = ui.id().with("curve_open");
        let mut ch = false;
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                match v.mode() {
                    CurveMode::Constant => ch |= drag(ui, &mut v.a, 0.05, min),
                    CurveMode::RandomBetweenTwoConstants => {
                        ch |= drag(ui, &mut v.a, 0.05, min);
                        ch |= drag(ui, &mut v.b, 0.05, min);
                    }
                    CurveMode::Curve => {
                        ch |= ui
                            .add(DragValue::new(&mut v.a).speed(0.05).prefix("× "))
                            .changed();
                        curve_button(ui, &v.curve_max, open_id);
                    }
                    CurveMode::RandomBetweenTwoCurves => {
                        ch |= ui
                            .add(DragValue::new(&mut v.a).speed(0.05).prefix("× "))
                            .changed();
                        curve_button(ui, &v.curve_min, open_id);
                        curve_button(ui, &v.curve_max, open_id);
                    }
                }
                let mut mode = v.mode();
                ui.menu_button("▾", |ui| {
                    for m in CurveMode::ALL {
                        if ui.radio(mode == m, m.label()).clicked() {
                            mode = m;
                            ui.close();
                        }
                    }
                });
                if mode != v.mode() {
                    let was_curve = v.curve;
                    v.set_mode(mode);
                    if v.curve && !was_curve {
                        // Start the curve at the constant's value (multiplier 1).
                        let k = v.a;
                        v.curve_max = Curve::linear(k, k);
                        v.curve_min = Curve::linear(0.0, 0.0);
                        v.a = 1.0;
                    } else if !v.curve && was_curve {
                        v.a *= v.curve_max.evaluate(0.0);
                        v.b = v.a;
                    } else if mode == CurveMode::RandomBetweenTwoConstants {
                        v.b = v.a;
                    }
                    ch = true;
                }
            });
            if v.curve && editor_open(ui, open_id) {
                if v.random {
                    ui.label(RichText::new("Min").small().color(theme::TEXT_DIM));
                    ch |= crate::panels::effect_graph::fields::curve_editor(ui, &mut v.curve_min);
                    ui.label(RichText::new("Max").small().color(theme::TEXT_DIM));
                }
                ch |= crate::panels::effect_graph::fields::curve_editor(ui, &mut v.curve_max);
            }
        });
        ch
    })
    .inner
}

/// sRGB + alpha colour button.
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

fn color_rgb(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut srgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed =
        crate::panels::inspector::card::wide(ui, |ui| ui.color_edit_button_srgb(&mut srgb))
            .changed();
    if changed {
        *c = srgb.map(|v| v as f32 / 255.0);
    }
    changed
}

/// A clickable gradient strip that opens the key editor below the row.
fn gradient_field(ui: &mut Ui, id: &str, g: &mut Gradient) -> bool {
    ui.push_id(id, |ui| {
        let open_id = ui.id().with("gradient_open");
        let mut ch = false;
        ui.vertical(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 16.0), egui::Sense::click());
            crate::panels::effect_graph::fields::paint_gradient(ui.painter(), rect, g);
            if resp.on_hover_text("Click to edit the gradient").clicked() {
                toggle_editor(ui, open_id);
            }
            if editor_open(ui, open_id) {
                ch |= gradient_keys(ui, g);
            }
        });
        ch
    })
    .inner
}

fn min_max_color(ui: &mut Ui, id: &str, v: &mut MinMaxColor) -> bool {
    ui.push_id(id, |ui| {
        ui.horizontal_top(|ui| {
            let mut ch = false;
            match v.mode() {
                ColorMode::Color => ch |= color_rgba(ui, &mut v.a),
                ColorMode::RandomBetweenTwoColors => {
                    ch |= color_rgba(ui, &mut v.a);
                    ch |= color_rgba(ui, &mut v.b);
                }
                ColorMode::Gradient => ch |= gradient_field(ui, "ga", &mut v.gradient_a),
                ColorMode::RandomBetweenTwoGradients => {
                    ui.vertical(|ui| {
                        ch |= gradient_field(ui, "ga", &mut v.gradient_a);
                        ch |= gradient_field(ui, "gb", &mut v.gradient_b);
                    });
                }
            }
            let mut mode = v.mode();
            ui.menu_button("▾", |ui| {
                for m in ColorMode::ALL {
                    if ui.radio(mode == m, m.label()).clicked() {
                        mode = m;
                        ui.close();
                    }
                }
            });
            if mode != v.mode() {
                v.set_mode(mode);
                if mode == ColorMode::RandomBetweenTwoColors {
                    v.b = v.a;
                }
                ch = true;
            }
            ch
        })
        .inner
    })
    .inner
}

fn emission(ui: &mut Ui, em: &mut misty_engine::particles::EmissionModule) -> bool {
    let mut ch = grid(ui, "particle_emission", |ui| {
        row(ui, "Rate over Time", |ui| {
            min_max(ui, "rot", &mut em.rate_over_time, 0.0)
        })
    });
    ui.label("Bursts");
    let mut remove = None;
    egui::Grid::new("particle_bursts")
        .num_columns(6)
        .striped(true)
        .show(ui, |ui| {
            for h in ["Time", "Count", "Cycles", "Interval", "Probability", ""] {
                ui.label(RichText::new(h).small().color(theme::TEXT_DIM));
            }
            ui.end_row();
            for (i, b) in em.bursts.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ch |= drag(ui, &mut b.time, 0.01, 0.0);
                    ch |= ui
                        .add(DragValue::new(&mut b.count).range(0..=100_000))
                        .changed();
                    ch |= ui
                        .add(DragValue::new(&mut b.cycles).range(0..=10_000))
                        .on_hover_text("0 = Infinite")
                        .changed();
                    ch |= drag(ui, &mut b.interval, 0.01, 0.01);
                    ch |= ui
                        .add(
                            DragValue::new(&mut b.probability)
                                .speed(0.01)
                                .range(0.0..=1.0),
                        )
                        .changed();
                    if ui.small_button("−").clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        em.bursts.remove(i);
        ch = true;
    }
    if ui.small_button("+ Add Burst").clicked() {
        em.bursts.push(Burst::default());
        ch = true;
    }
    ch
}

fn vec3(ui: &mut Ui, v: &mut glam::Vec3, speed: f32) -> bool {
    let mut a = v.to_array();
    let mut c = false;
    ui.horizontal(|ui| {
        for (axis, x) in ["X", "Y", "Z"].iter().zip(a.iter_mut()) {
            ui.label(*axis);
            c |= ui.add(DragValue::new(x).speed(speed)).changed();
        }
    });
    *v = a.into();
    c
}

fn shape(ui: &mut Ui, s: &mut misty_engine::particles::ShapeModule) -> bool {
    grid(ui, "particle_shape", |ui| {
        let mut ch = row(ui, "Shape", |ui| {
            combo(
                ui,
                "p_shape",
                &mut s.shape,
                &ShapeKind::ALL,
                ShapeKind::label,
            )
        });
        if s.shape == ShapeKind::Cone {
            ch |= row(ui, "Angle", |ui| {
                ui.add(DragValue::new(&mut s.angle).speed(0.2).range(0.0..=90.0))
                    .changed()
            });
        }
        if s.shape != ShapeKind::Box {
            ch |= row(ui, "Radius", |ui| drag(ui, &mut s.radius, 0.02, 0.0));
        } else {
            ch |= row(ui, "Scale", |ui| vec3(ui, &mut s.box_size, 0.02));
        }
        ch
    })
}

fn xyz(
    ui: &mut Ui,
    id: &str,
    x: &mut MinMax,
    y: &mut MinMax,
    z: &mut MinMax,
    sp: &mut SimulationSpace,
) -> bool {
    let mut ch = row(ui, "X", |ui| min_max(ui, &format!("{id}x"), x, f32::MIN));
    ch |= row(ui, "Y", |ui| min_max(ui, &format!("{id}y"), y, f32::MIN));
    ch |= row(ui, "Z", |ui| min_max(ui, &format!("{id}z"), z, f32::MIN));
    ch |= row(ui, "Space", |ui| space(ui, &format!("{id}space"), sp));
    ch
}

fn velocity_over_lifetime(
    ui: &mut Ui,
    v: &mut misty_engine::particles::VelocityOverLifetime,
) -> bool {
    grid(ui, "particle_vol", |ui| {
        let mut ch = xyz(ui, "vol", &mut v.x, &mut v.y, &mut v.z, &mut v.space);
        ch |= row(ui, "Speed Modifier", |ui| {
            min_max(ui, "volsm", &mut v.speed_modifier, f32::MIN)
        });
        ch
    })
}

fn limit_velocity(ui: &mut Ui, v: &mut misty_engine::particles::LimitVelocityOverLifetime) -> bool {
    grid(ui, "particle_lvol", |ui| {
        let mut ch = row(ui, "Speed", |ui| min_max(ui, "lim", &mut v.limit, 0.0));
        ch |= row(ui, "Dampen", |ui| unit(ui, &mut v.dampen));
        ch |= row(ui, "Drag", |ui| min_max(ui, "drag", &mut v.drag, 0.0));
        ch
    })
}

fn force_over_lifetime(ui: &mut Ui, f: &mut misty_engine::particles::ForceOverLifetime) -> bool {
    grid(ui, "particle_fol", |ui| {
        xyz(ui, "fol", &mut f.x, &mut f.y, &mut f.z, &mut f.space)
    })
}

fn color_over_lifetime(ui: &mut Ui, c: &mut misty_engine::particles::ColorOverLifetime) -> bool {
    grid(ui, "particle_col", |ui| {
        let mut ch = row(ui, "Color", |ui| {
            gradient_field(ui, "col_a", &mut c.gradient)
        });
        ch |= row(ui, "Random Between Two", |ui| {
            ui.checkbox(&mut c.random, "").changed()
        });
        if c.random {
            ch |= row(ui, "Second Gradient", |ui| {
                gradient_field(ui, "col_b", &mut c.gradient_b)
            });
        }
        ch
    })
}

fn speed_range(ui: &mut Ui, r: &mut [f32; 2]) -> bool {
    ui.horizontal(|ui| {
        let mut c = drag(ui, &mut r[0], 0.05, 0.0);
        c |= drag(ui, &mut r[1], 0.05, 0.0);
        c
    })
    .inner
}

fn color_by_speed(ui: &mut Ui, c: &mut misty_engine::particles::ColorBySpeed) -> bool {
    grid(ui, "particle_cbs", |ui| {
        let mut ch = row(ui, "Color", |ui| gradient_field(ui, "cbs", &mut c.gradient));
        ch |= row(ui, "Speed Range", |ui| speed_range(ui, &mut c.range));
        ch
    })
}

fn size_over_lifetime(ui: &mut Ui, s: &mut misty_engine::particles::SizeOverLifetime) -> bool {
    grid(ui, "particle_size", |ui| {
        let mut curve = s.curve();
        let ch = row(ui, "Size", |ui| min_max(ui, "sol", &mut curve, 0.0));
        if ch {
            s.size = Some(curve);
        }
        ch
    })
}

fn size_by_speed(ui: &mut Ui, s: &mut misty_engine::particles::SizeBySpeed) -> bool {
    grid(ui, "particle_sbs", |ui| {
        let mut ch = row(ui, "Size", |ui| min_max(ui, "sbs", &mut s.size, 0.0));
        ch |= row(ui, "Speed Range", |ui| speed_range(ui, &mut s.range));
        ch
    })
}

fn rotation_over_lifetime(
    ui: &mut Ui,
    r: &mut misty_engine::particles::RotationOverLifetime,
) -> bool {
    grid(ui, "particle_rol", |ui| {
        row(ui, "Angular Velocity", |ui| {
            min_max(ui, "rol", &mut r.angular_velocity, f32::MIN)
        })
    })
}

fn rotation_by_speed(ui: &mut Ui, r: &mut misty_engine::particles::RotationBySpeed) -> bool {
    grid(ui, "particle_rbs", |ui| {
        let mut ch = row(ui, "Angular Velocity", |ui| {
            min_max(ui, "rbs", &mut r.angular_velocity, f32::MIN)
        });
        ch |= row(ui, "Speed Range", |ui| speed_range(ui, &mut r.range));
        ch
    })
}

fn noise(ui: &mut Ui, n: &mut misty_engine::particles::NoiseModule) -> bool {
    grid(ui, "particle_noise", |ui| {
        let mut ch = row(ui, "Strength", |ui| {
            min_max(ui, "nstr", &mut n.strength, 0.0)
        });
        ch |= row(ui, "Frequency", |ui| {
            drag(ui, &mut n.frequency, 0.01, 0.0001)
        });
        ch |= row(ui, "Scroll Speed", |ui| {
            drag(ui, &mut n.scroll_speed, 0.01, f32::MIN)
        });
        ch |= row(ui, "Damping", |ui| {
            ui.checkbox(&mut n.damping, "").changed()
        });
        ch |= row(ui, "Octaves", |ui| {
            ui.add(DragValue::new(&mut n.octaves).range(1..=4))
                .changed()
        });
        if n.octaves > 1 {
            ch |= row(ui, "Octave Multiplier", |ui| {
                unit(ui, &mut n.octave_multiplier)
            });
            ch |= row(ui, "Octave Scale", |ui| {
                ui.add(
                    DragValue::new(&mut n.octave_scale)
                        .speed(0.05)
                        .range(1.0..=4.0),
                )
                .changed()
            });
        }
        ch |= row(ui, "Position Amount", |ui| {
            drag(ui, &mut n.position_amount, 0.01, f32::MIN)
        });
        ch |= row(ui, "Rotation Amount", |ui| {
            drag(ui, &mut n.rotation_amount, 0.01, f32::MIN)
        });
        ch |= row(ui, "Size Amount", |ui| {
            drag(ui, &mut n.size_amount, 0.01, f32::MIN)
        });
        ch
    })
}

fn collision(ui: &mut Ui, c: &mut misty_engine::particles::CollisionModule) -> bool {
    let mut ch = grid(ui, "particle_collision", |ui| {
        let mut ch = row(ui, "Type", |ui| {
            combo(
                ui,
                "p_coll",
                &mut c.kind,
                &CollisionType::ALL,
                CollisionType::label,
            )
        });
        ch |= row(ui, "Dampen", |ui| unit(ui, &mut c.dampen));
        ch |= row(ui, "Bounce", |ui| drag(ui, &mut c.bounce, 0.01, 0.0));
        ch |= row(ui, "Lifetime Loss", |ui| unit(ui, &mut c.lifetime_loss));
        ch |= row(ui, "Min Kill Speed", |ui| {
            drag(ui, &mut c.min_kill_speed, 0.05, 0.0)
        });
        ch |= row(ui, "Max Kill Speed", |ui| {
            drag(ui, &mut c.max_kill_speed, 0.5, 0.0)
        });
        ch |= row(ui, "Radius Scale", |ui| {
            drag(ui, &mut c.radius_scale, 0.01, 0.0)
        });
        if c.kind == CollisionType::World {
            ch |= row(ui, "Collides With", |ui| {
                let mut all = c.collides_with == u32::MAX;
                let r = ui.checkbox(&mut all, "Everything").changed();
                if r {
                    c.collides_with = if all { u32::MAX } else { 1 };
                }
                r
            });
        }
        ch
    });
    if c.kind == CollisionType::Planes {
        ui.label(
            RichText::new("Planes (world point, normal)")
                .small()
                .color(theme::TEXT_DIM),
        );
        let mut remove = None;
        for (i, pl) in c.planes.iter_mut().enumerate() {
            ui.push_id(("plane", i), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Point");
                    ch |= vec3(ui, &mut pl.point, 0.05);
                });
                ui.horizontal(|ui| {
                    ui.label("Normal");
                    ch |= vec3(ui, &mut pl.normal, 0.02);
                    if ui.small_button("−").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            c.planes.remove(i);
            ch = true;
        }
        if ui.small_button("+ Add Plane").clicked() {
            c.planes.push(CollisionPlane::default());
            ch = true;
        }
    }
    ch
}

fn sub_emitters(
    ui: &mut Ui,
    s: &mut misty_engine::particles::SubEmittersModule,
    children: &[String],
) -> bool {
    let mut ch = false;
    let mut remove = None;
    if children.is_empty() {
        ui.label(
            RichText::new("Add a child object with a Particle System to use as a sub emitter.")
                .small()
                .color(theme::TEXT_DIM),
        );
    }
    for (i, sub) in s.list.iter_mut().enumerate() {
        ui.push_id(("sub", i), |ui| {
            ui.horizontal(|ui| {
                ch |= combo(
                    ui,
                    "trigger",
                    &mut sub.trigger,
                    &SubEmitterTrigger::ALL,
                    SubEmitterTrigger::label,
                );
                let text = if sub.emitter.is_empty() {
                    "(None)".to_owned()
                } else {
                    sub.emitter.clone()
                };
                egui::ComboBox::from_id_salt("emitter")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        for c in children {
                            if ui.selectable_label(sub.emitter == *c, c).clicked() {
                                sub.emitter = c.clone();
                                ch = true;
                            }
                        }
                    });
                if ui.small_button("−").clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Count");
                ch |= ui
                    .add(DragValue::new(&mut sub.count).range(0..=10_000))
                    .changed();
                ui.label("Probability");
                ch |= unit(ui, &mut sub.probability);
            });
            ui.horizontal(|ui| {
                ui.label("Inherit");
                ch |= ui.checkbox(&mut sub.inherit_color, "Color").changed();
                ch |= ui.checkbox(&mut sub.inherit_size, "Size").changed();
                ui.label("Velocity");
                ch |= ui
                    .add(
                        DragValue::new(&mut sub.inherit_velocity)
                            .speed(0.01)
                            .range(0.0..=1.0),
                    )
                    .changed();
            });
        });
    }
    if let Some(i) = remove {
        s.list.remove(i);
        ch = true;
    }
    if ui.small_button("+ Add Sub Emitter").clicked() {
        s.list.push(SubEmitter {
            emitter: children.first().cloned().unwrap_or_default(),
            ..Default::default()
        });
        ch = true;
    }
    ch
}

fn texture_sheet(ui: &mut Ui, t: &mut misty_engine::particles::TextureSheetAnimation) -> bool {
    grid(ui, "particle_sheet", |ui| {
        let mut ch = row(ui, "Tiles", |ui| {
            ui.horizontal(|ui| {
                ui.label("X");
                let mut c = ui
                    .add(DragValue::new(&mut t.tiles_x).range(1..=64))
                    .changed();
                ui.label("Y");
                c |= ui
                    .add(DragValue::new(&mut t.tiles_y).range(1..=64))
                    .changed();
                c
            })
            .inner
        });
        ch |= row(ui, "Animation", |ui| {
            combo(
                ui,
                "p_sheet",
                &mut t.animation,
                &SheetAnimation::ALL,
                SheetAnimation::label,
            )
        });
        if t.animation == SheetAnimation::SingleRow {
            ch |= row(ui, "Random Row", |ui| {
                ui.checkbox(&mut t.random_row, "").changed()
            });
            if !t.random_row {
                ch |= row(ui, "Row", |ui| {
                    ui.add(DragValue::new(&mut t.row).range(0..=t.tiles_y.saturating_sub(1)))
                        .changed()
                });
            }
        }
        ch |= row(ui, "Frame over Time", |ui| {
            min_max(ui, "fot", &mut t.frame_over_time, 0.0)
        });
        ch |= row(ui, "Start Frame", |ui| {
            min_max(ui, "sf", &mut t.start_frame, 0.0)
        });
        ch |= row(ui, "Cycles", |ui| drag(ui, &mut t.cycles, 0.05, 0.0));
        ch
    })
}

fn trails(ui: &mut Ui, t: &mut misty_engine::particles::TrailsModule, textures: &[String]) -> bool {
    grid(ui, "particle_trails", |ui| {
        let mut ch = row(ui, "Mode", |ui| {
            combo(
                ui,
                "p_trail",
                &mut t.mode,
                &TrailMode::ALL,
                TrailMode::label,
            )
        });
        if t.mode == TrailMode::Particles {
            ch |= row(ui, "Ratio", |ui| unit(ui, &mut t.ratio));
            ch |= row(ui, "Lifetime", |ui| {
                min_max(ui, "tlife", &mut t.lifetime, 0.0)
            });
            ch |= row(ui, "Minimum Vertex Distance", |ui| {
                drag(ui, &mut t.min_vertex_distance, 0.01, 0.0)
            });
            ch |= row(ui, "World Space", |ui| {
                ui.checkbox(&mut t.world_space, "").changed()
            });
            ch |= row(ui, "Die with Particles", |ui| {
                ui.checkbox(&mut t.die_with_particles, "").changed()
            });
        }
        ch |= row(ui, "Size affects Width", |ui| {
            ui.checkbox(&mut t.size_affects_width, "").changed()
        });
        ch |= row(ui, "Inherit Particle Color", |ui| {
            ui.checkbox(&mut t.inherit_particle_color, "").changed()
        });
        ch |= row(ui, "Color over Lifetime", |ui| {
            gradient_field(ui, "tcol", &mut t.color_over_lifetime)
        });
        ch |= row(ui, "Width over Trail", |ui| {
            min_max(ui, "twidth", &mut t.width_over_trail, 0.0)
        });
        ch |= row(ui, "Color over Trail", |ui| {
            gradient_field(ui, "tcot", &mut t.color_over_trail)
        });
        ch |= row(ui, "Texture", |ui| {
            super::slots::texture_slot(ui, "p_trail_tex", &mut t.texture, textures)
        });
        ch
    })
}

/// Editable colour and alpha keys of a gradient.
fn gradient_keys(ui: &mut Ui, g: &mut Gradient) -> bool {
    let mut ch = keys(ui, "Color Keys", &mut g.color_keys, |ui, k| {
        let mut c = drag_time(ui, &mut k.time);
        c |= color_rgb(ui, &mut k.color);
        c
    });
    ch |= keys(ui, "Alpha Keys", &mut g.alpha_keys, |ui, k| {
        let mut c = drag_time(ui, &mut k.time);
        c |= ui
            .add(DragValue::new(&mut k.alpha).speed(0.01).range(0.0..=1.0))
            .changed();
        c
    });
    if ch {
        g.sort();
    }
    ch
}

/// Gradient bar preview plus editable colour and alpha keys.
pub(crate) fn gradient(ui: &mut Ui, id: &str, g: &mut Gradient) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("Color");
        gradient_bar(ui, g);
    });
    ui.push_id(id, |ui| {
        ch |= gradient_keys(ui, g);
    });
    ch
}

fn drag_time(ui: &mut Ui, t: &mut f32) -> bool {
    ui.add(DragValue::new(t).speed(0.005).range(0.0..=1.0).prefix("t "))
        .changed()
}

/// Editable key list (at least 1 key, at most 8).
fn keys<K: Clone + KeyTime>(
    ui: &mut Ui,
    title: &str,
    list: &mut Vec<K>,
    edit: impl Fn(&mut Ui, &mut K) -> bool,
) -> bool {
    let mut ch = false;
    let mut remove = None;
    ui.label(RichText::new(title).small().color(theme::TEXT_DIM));
    for (i, k) in list.iter_mut().enumerate() {
        ui.push_id((title, i), |ui| {
            ui.horizontal(|ui| {
                ch |= edit(ui, k);
                if ui.small_button("−").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove.filter(|_| list.len() > 1) {
        list.remove(i);
        ch = true;
    }
    if list.len() < 8
        && ui
            .small_button(format!("+ {}", &title[..title.len() - 1]))
            .clicked()
    {
        // New key halfway between the last two (or at the end).
        let mut k = list.last().cloned().expect("at least one key");
        let t = match list.len() {
            1 => 1.0,
            n => (list[n - 2].time() + list[n - 1].time()) * 0.5,
        };
        k.set_time(t);
        list.push(k);
        ch = true;
    }
    ch
}

trait KeyTime {
    fn time(&self) -> f32;
    fn set_time(&mut self, t: f32);
}

impl KeyTime for ColorKey {
    fn time(&self) -> f32 {
        self.time
    }
    fn set_time(&mut self, t: f32) {
        self.time = t;
    }
}

impl KeyTime for AlphaKey {
    fn time(&self) -> f32 {
        self.time
    }
    fn set_time(&mut self, t: f32) {
        self.time = t;
    }
}

/// Horizontal strip showing the gradient over a checkerboard (alpha visible).
fn gradient_bar(ui: &mut Ui, g: &Gradient) {
    let (rect, _) = ui.allocate_exact_size(vec2(170.0, 16.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let cell = 4.0;
    let steps = 42;
    let w = rect.width() / steps as f32;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let c = g.evaluate(t);
        for half in 0..2 {
            let x0 = rect.left() + i as f32 * w;
            let y0 = rect.top() + half as f32 * rect.height() * 0.5;
            let r = egui::Rect::from_min_size(pos2(x0, y0), vec2(w + 0.5, rect.height() * 0.5));
            let checker = if ((x0 / cell) as i32 + half) % 2 == 0 {
                200.0
            } else {
                140.0
            };
            let mix = |v: f32| (v * c[3] * 255.0 + checker * (1.0 - c[3])) as u8;
            painter.rect_filled(r, 0.0, Color32::from_rgb(mix(c[0]), mix(c[1]), mix(c[2])));
        }
    }
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, Color32::from_gray(30)),
        egui::StrokeKind::Inside,
    );
}

fn renderer(
    ui: &mut Ui,
    r: &mut misty_engine::particles::RendererModule,
    textures: &[String],
) -> bool {
    grid(ui, "particle_renderer", |ui| {
        let mut ch = row(ui, "Render Mode", |ui| {
            combo(
                ui,
                "p_rmode",
                &mut r.render_mode,
                &RenderMode::ALL,
                RenderMode::label,
            )
        });
        if r.render_mode == RenderMode::Stretched {
            ch |= row(ui, "Speed Scale", |ui| {
                drag(ui, &mut r.speed_scale, 0.01, 0.0)
            });
            ch |= row(ui, "Length Scale", |ui| {
                drag(ui, &mut r.length_scale, 0.01, 0.0)
            });
        }
        ch |= row(ui, "Texture", |ui| {
            super::slots::texture_slot(ui, "p_tex", &mut r.texture, textures)
        });
        ch |= row(ui, "Blend Mode", |ui| {
            combo(
                ui,
                "p_blend",
                &mut r.blend,
                &ParticleBlend::ALL,
                ParticleBlend::label,
            )
        });
        ch |= row(ui, "Intensity", |ui| {
            ui.add(
                DragValue::new(&mut r.intensity)
                    .speed(0.02)
                    .range(0.0..=100.0),
            )
            .on_hover_text("HDR brightness: above 1 the particles glow (bloom)")
            .changed()
        });
        ch
    })
}
