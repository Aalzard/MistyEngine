//! Inspector editor for the Animation component: default clip, clip list, Play Automatically and
//! Wrap Mode.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animation, WrapMode, clip_name};

use super::editors::{combo, grid};
use crate::state::EditorState;
use crate::theme;

/// Icon shown in clip slots.
pub(super) const CLIP_ICON: &str = "⏵";

pub fn animation(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut a) = state.scene.world.get::<&Animation>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let clips: Vec<String> = state.assets.anims.iter().cloned().collect();
    let mut changed = grid(ui, "animation_grid", |ui| {
        let mut ch = false;
        ui.label("Animation");
        ch |= clip_slot(ui, "anim_default", &clips, &mut a.clip);
        ui.end_row();
        ui.label("Play Automatically");
        ch |= ui.checkbox(&mut a.play_automatically, "").changed();
        ui.end_row();
        ui.label("Wrap Mode");
        ch |= combo(
            ui,
            "anim_wrap",
            &mut a.wrap_mode,
            &WrapMode::ALL,
            WrapMode::label,
        );
        ui.end_row();
        ch
    });
    egui::CollapsingHeader::new(format!("Animations ({})", a.clips.len()))
        .id_salt("anim_list")
        .default_open(true)
        .show(ui, |ui| {
            let mut remove = None;
            for (i, c) in a.clips.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Element {i}"));
                    let mut slot = Some(c.clone());
                    if clip_slot(ui, ("anim_el", i), &clips, &mut slot) {
                        match slot {
                            Some(s) => *c = s,
                            None => remove = Some(i),
                        }
                        changed = true;
                    }
                    if ui.small_button("−").on_hover_text("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                a.clips.remove(i);
                changed = true;
            }
            if ui
                .small_button("+")
                .on_hover_text("Add a clip slot")
                .clicked()
                && let Some(first) = a.clip.clone().or_else(|| clips.first().cloned())
            {
                a.clips.push(first);
                changed = true;
            }
        });
    if changed {
        // The default clip is always playable by name too.
        if let Some(c) = &a.clip
            && !a.clips.contains(c)
        {
            a.clips.insert(0, c.clone());
        }
        let _ = state.scene.world.insert_one(e, a);
    }
    changed
}

/// Clip picker: project `.anim` files, or a clip dragged from the Project window.
pub(super) fn clip_slot(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    clips: &[String],
    clip: &mut Option<String>,
) -> bool {
    let mut ch = false;
    let current = clip
        .as_deref()
        .map(clip_name)
        .unwrap_or_else(|| "None (Animation Clip)".into());
    let slot = egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{CLIP_ICON} {current}"))
        .width(150.0)
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(clip, None, "None (Animation Clip)")
                .changed();
            for c in clips {
                ch |= ui
                    .selectable_value(clip, Some(c.clone()), clip_name(c))
                    .changed();
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::animation::is_clip(&path.0)
    {
        *clip = Some(path.0.clone());
        ch = true;
    }
    ch
}

/// Animation clip asset: Loop Time, length, sample rate, curves and events.
pub fn clip_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(CLIP_ICON).size(22.0));
        ui.label(RichText::new(format!("{} (Animation Clip)", clip_name(rel))).strong());
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let clip = match state.anim_clips.get(rel) {
        Ok(c) => c.clone(),
        Err(e) => {
            ui.colored_label(theme::AXIS_X, e);
            return;
        }
    };
    let mut looping = clip.looping;
    let changed = grid(ui, "anim_clip_asset", |ui| {
        ui.label("Loop Time");
        let ch = ui.checkbox(&mut looping, "").changed();
        ui.end_row();
        ui.label("Length");
        ui.label(format!("{:.3} s", clip.length));
        ui.end_row();
        ui.label("Sample Rate");
        ui.label(format!("{}", clip.sample_rate));
        ui.end_row();
        ui.label("Curves");
        ui.label(clip.curves.len().to_string());
        ui.end_row();
        ch
    });
    if !clip.events.is_empty() {
        ui.label("Events");
        for ev in &clip.events {
            ui.weak(format!("{:.3} s  {}()", ev.time, ev.function));
        }
    }
    if changed && let Some(snap) = state.anim_clips.snapshot(rel) {
        let (scene, selection) = (&state.scene, &state.selection);
        state
            .undo
            .push_with_clip("Change Loop Time", scene, selection, Some(snap));
        if let Some(c) = state.anim_clips.get_mut(rel) {
            c.looping = looping;
        }
    }
}
