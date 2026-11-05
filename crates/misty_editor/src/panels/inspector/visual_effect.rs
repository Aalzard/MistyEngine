//! Visual Effect Inspector: the Effect Graph asset (with Edit), Play On Awake, seed, the exposed
//! properties with per-instance overrides, play controls and live counters.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::vfx::{self, EffectInstance, PropertyValue, VisualEffect};

use super::editors::grid;
use crate::state::EditorState;
use crate::theme;

pub fn visual_effect(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut v) = state
        .scene
        .world
        .get::<&VisualEffect>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = v.clone();
    let effects: Vec<String> = state.assets.effects.iter().cloned().collect();
    let textures = state.assets.textures.clone();
    let mut edit = None;
    grid(ui, "vfx_component", |ui| {
        ui.label("Asset Template");
        ui.horizontal(|ui| {
            let current = v
                .effect
                .as_deref()
                .map(misty_engine::scene::clip_name)
                .unwrap_or_else(|| "None".into());
            egui::ComboBox::from_id_salt("vfx_asset")
                .selected_text(format!("{} {current}", crate::panels::effect_graph::ICON))
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut v.effect, None, "None");
                    for p in &effects {
                        ui.selectable_value(
                            &mut v.effect,
                            Some(p.clone()),
                            misty_engine::scene::clip_name(p),
                        )
                        .on_hover_text(p);
                    }
                });
            if let Some(p) = &v.effect
                && ui
                    .button("Edit")
                    .on_hover_text("Open in the Effect Graph window")
                    .clicked()
            {
                edit = Some(p.clone());
            }
        });
        ui.end_row();
        ui.label("Play On Awake");
        ui.checkbox(&mut v.play_on_awake, "");
        ui.end_row();
        ui.label("Random Seed");
        ui.add(egui::DragValue::new(&mut v.seed));
        ui.end_row();
        ui.label("Reseed On Play");
        ui.checkbox(&mut v.reseed_on_play, "");
        ui.end_row();
        false
    });
    if let Some(p) = edit {
        crate::panels::effect_graph::open(state, &p);
    }

    // Exposed properties (from the compiled asset, or the working copy being edited).
    let compiled = v.effect.as_deref().map(|p| vfx::library::get(p).1);
    match &compiled {
        Some(Err(errors)) => {
            let text = errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("\n");
            ui.label(RichText::new(format!("⚠ {text}")).color(theme::WARN));
        }
        Some(Ok(effect)) if !effect.properties.is_empty() => {
            ui.add_space(4.0);
            ui.label(crate::theme::bold(ui.ctx(), "Properties"));
            for p in &effect.properties {
                let mut on = v.override_value(&p.name).is_some();
                let mut value = v
                    .override_value(&p.name)
                    .filter(|o| o.same_type(&p.value))
                    .cloned()
                    .unwrap_or_else(|| p.value.clone());
                ui.horizontal(|ui| {
                    let r = ui.checkbox(&mut on, &p.name);
                    if !p.tooltip.is_empty() {
                        r.on_hover_text(&p.tooltip);
                    }
                    if !matches!(value, PropertyValue::Gradient(_)) {
                        ui.add_enabled_ui(on, |ui| {
                            if super::super::effect_graph::blackboard_value(
                                ui,
                                &format!("vfx_ov_{}", p.name),
                                &mut value,
                                &textures,
                            ) {
                                on = true;
                            }
                        });
                    }
                });
                if matches!(value, PropertyValue::Gradient(_)) && on {
                    ui.indent(("vfx_ov_g", &p.name), |ui| {
                        super::super::effect_graph::blackboard_value(
                            ui,
                            &format!("vfx_ov_{}", p.name),
                            &mut value,
                            &textures,
                        );
                    });
                }
                if on {
                    v.set_override(&p.name, value);
                } else {
                    v.clear_override(&p.name);
                }
            }
        }
        _ => {}
    }

    // Play controls and counters of the live instance.
    ui.add_space(4.0);
    let inst = state
        .scene
        .world
        .get::<&EffectInstance>(e)
        .map(|i| (i.id, i.playing, i.total_time))
        .ok();
    ui.horizontal(|ui| {
        let mut cmd = None;
        if ui.button("▶ Play").clicked() {
            cmd = Some(0);
        }
        if ui.button("■ Stop").clicked() {
            cmd = Some(1);
        }
        if ui
            .button("⟲ Reinit")
            .on_hover_text("Clear the particles and restart")
            .clicked()
        {
            cmd = Some(2);
        }
        if let Some(c) = cmd {
            vfx::with_instance(&mut state.scene, e, |i, comp| match c {
                0 => i.play(comp),
                1 => i.stop(),
                _ => i.reinit(comp),
            });
        }
        if let Some((id, playing, time)) = inst {
            let alive = vfx::stats(id).map_or(0, |s| s.alive);
            ui.label(
                RichText::new(format!(
                    "{} · {alive} alive · {time:.1}s",
                    if playing { "Playing" } else { "Stopped" }
                ))
                .color(theme::TEXT_DIM),
            );
        }
    });
    let changed = v != before;
    if changed {
        let _ = state.scene.world.insert_one(e, v);
    }
    changed
}
