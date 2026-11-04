//! Particle System Inspector: the main module, then one collapsible section per module with an
//! enable checkbox in its header (Emission, Shape, Color over Lifetime, Size over Lifetime,
//! Renderer).

use egui::{Color32, DragValue, RichText, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::particles::{
    AlphaKey, Burst, ColorKey, Gradient, MinMax, MinMaxColor, ParticleBlend, ParticleSystem,
    ShapeKind, SimulationSpace,
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
    let mut ch = main_module(ui, &mut p);
    // Each module's checkbox edits a copy of its flag, so the body can borrow the module.
    let mut on = p.emission.enabled;
    ch |= module(ui, "Emission", &mut on, |ui| emission(ui, &mut p.emission));
    p.emission.enabled = on;
    let mut on = p.shape.enabled;
    ch |= module(ui, "Shape", &mut on, |ui| shape(ui, &mut p.shape));
    p.shape.enabled = on;
    let mut on = p.color_over_lifetime.enabled;
    ch |= module(ui, "Color over Lifetime", &mut on, |ui| {
        gradient(ui, "pcol", &mut p.color_over_lifetime.gradient)
    });
    p.color_over_lifetime.enabled = on;
    let mut on = p.size_over_lifetime.enabled;
    ch |= module(ui, "Size over Lifetime", &mut on, |ui| {
        size_over_lifetime(ui, &mut p.size_over_lifetime)
    });
    p.size_over_lifetime.enabled = on;
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
                ch |= row(ui, "Start Color", |ui| {
                    min_max_color(ui, "sc", &mut m.start_color)
                });
                ch |= row(ui, "Gravity Modifier", |ui| {
                    drag(ui, &mut m.gravity_modifier, 0.01, f32::MIN)
                });
                ch |= row(ui, "Simulation Space", |ui| {
                    combo(
                        ui,
                        "p_space",
                        &mut m.simulation_space,
                        &SimulationSpace::ALL,
                        SimulationSpace::label,
                    )
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

/// Mode menu shared by number and colour fields: Constant / Random Between Two Constants.
fn mode_menu(ui: &mut Ui, random: &mut bool) -> bool {
    let mut ch = false;
    ui.menu_button("▾", |ui| {
        if ui.radio(!*random, "Constant").clicked() {
            ch = *random;
            *random = false;
            ui.close();
        }
        if ui.radio(*random, "Random Between Two Constants").clicked() {
            ch = !*random;
            *random = true;
            ui.close();
        }
    });
    ch
}

fn min_max(ui: &mut Ui, id: &str, v: &mut MinMax, min: f32) -> bool {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            let mut ch = drag(ui, &mut v.a, 0.05, min);
            if v.random {
                ch |= drag(ui, &mut v.b, 0.05, min);
            }
            if mode_menu(ui, &mut v.random) {
                if v.random {
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

fn min_max_color(ui: &mut Ui, id: &str, v: &mut MinMaxColor) -> bool {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            let mut ch = color_rgba(ui, &mut v.a);
            if v.random {
                ch |= color_rgba(ui, &mut v.b);
            }
            if mode_menu(ui, &mut v.random) {
                if v.random {
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
            let mut v = s.box_size.to_array();
            ch |= row(ui, "Scale", |ui| {
                let mut c = false;
                ui.horizontal(|ui| {
                    for (axis, x) in ["X", "Y", "Z"].iter().zip(v.iter_mut()) {
                        ui.label(*axis);
                        c |= drag(ui, x, 0.02, 0.0);
                    }
                });
                c
            });
            s.box_size = v.into();
        }
        ch
    })
}

/// Gradient bar preview plus editable colour and alpha keys.
fn gradient(ui: &mut Ui, id: &str, g: &mut Gradient) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("Color");
        gradient_bar(ui, g);
    });
    ui.push_id(id, |ui| {
        ch |= keys(ui, "Color Keys", &mut g.color_keys, |ui, k| {
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
    });
    if ch {
        g.sort();
    }
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

fn size_over_lifetime(ui: &mut Ui, s: &mut misty_engine::particles::SizeOverLifetime) -> bool {
    grid(ui, "particle_size", |ui| {
        let mut ch = row(ui, "Size at Start", |ui| drag(ui, &mut s.start, 0.01, 0.0));
        ch |= row(ui, "Size at End", |ui| drag(ui, &mut s.end, 0.01, 0.0));
        ch
    })
}

fn renderer(
    ui: &mut Ui,
    r: &mut misty_engine::particles::RendererModule,
    textures: &[String],
) -> bool {
    grid(ui, "particle_renderer", |ui| {
        ui.label("Render Mode");
        ui.label("Billboard");
        ui.end_row();
        let mut ch = row(ui, "Texture", |ui| {
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
