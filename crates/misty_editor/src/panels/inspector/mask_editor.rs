//! Inspectors for rig assets. An Avatar Mask (`.mask`): the transform tree with a checkbox per bone
//! (switching one switches everything below it), All / None, and "Import skeleton" from an
//! object in the scene (its hierarchy becomes the list, keeping flags already set). A humanoid
//! Avatar (`.avatar`): its bone mapping and converting clips to humanoid clips.

use egui::{RichText, Ui};
use misty_engine::animation::AvatarMask;
use misty_engine::animation::mask::path_within;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animator, clip_name};

use crate::avatar_masks;
use crate::panels::animator::MASK_ICON;
use crate::state::EditorState;
use crate::theme;

/// Label of the skeleton import button (tests click it).
pub const IMPORT_LABEL: &str = "Import skeleton";

pub fn mask_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let mask = match state.anim_clips.masks.get(rel) {
        Ok(m) => m.clone(),
        Err(e) => {
            ui.colored_label(theme::AXIS_X, e);
            return;
        }
    };
    let busy = crate::anim_controllers::interacting(ui.ctx());
    state.anim_clips.masks.end_interaction(busy);
    ui.horizontal(|ui| {
        ui.label(RichText::new(MASK_ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Avatar Mask)", clip_name(rel)),
        ));
    });
    ui.separator();
    crate::asset_guids::path_label(ui, rel);
    super::card::sub_header(ui, "Transform");
    import_row(ui, state, rel);
    if mask.transforms.is_empty() {
        ui.weak("No transforms listed: the mask lets every bone through. Import a skeleton to choose bones.");
        return;
    }
    let active = mask.transforms.iter().filter(|t| t.active).count();
    ui.horizontal(|ui| {
        ui.weak(format!("{active} of {} active", mask.transforms.len()));
        if ui.small_button("All").clicked() {
            avatar_masks::edit(state, rel, "Mask All", false, |m| {
                m.set_active("", true, true)
            });
        }
        if ui.small_button("None").clicked() {
            avatar_masks::edit(state, rel, "Mask None", false, |m| {
                m.set_active("", false, true)
            });
        }
        if ui
            .small_button("Clear")
            .on_hover_text("Empty list: everything passes")
            .clicked()
        {
            avatar_masks::edit(state, rel, "Clear Mask", false, |m| m.transforms.clear());
        }
    });
    tree(ui, state, rel, &mask);
}

/// "Import skeleton" from an object of the scene (objects with an Animator are listed first).
fn import_row(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let id = ui.id().with(("mask_import_from", rel));
    let mut candidates: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| state.scene.world.get::<&Animator>(*e).is_ok())
        .map(|e| (e, state.scene.name(e)))
        .collect();
    if candidates.is_empty() {
        candidates = state
            .scene
            .roots()
            .iter()
            .map(|&e| (e, state.scene.name(e)))
            .collect();
    }
    let mut from: Option<Entity> = ui
        .data(|d| d.get_temp::<Entity>(id))
        .filter(|e| candidates.iter().any(|(c, _)| c == e))
        .or_else(|| candidates.first().map(|c| c.0));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("mask_import_combo", rel))
            .selected_text(
                from.map(|e| state.scene.name(e))
                    .unwrap_or_else(|| "No objects".into()),
            )
            .show_ui(ui, |ui| {
                for (e, name) in &candidates {
                    ui.selectable_value(&mut from, Some(*e), name);
                }
            });
        if let Some(e) = from {
            ui.data_mut(|d| d.insert_temp(id, e));
        }
        if ui
            .add_enabled(from.is_some(), egui::Button::new(IMPORT_LABEL))
            .on_hover_text("List this object's hierarchy (flags already set are kept)")
            .clicked()
            && let Some(e) = from
        {
            let paths = AvatarMask::skeleton(&state.scene, e);
            avatar_masks::edit(state, rel, "Import Skeleton", false, |m| m.import(&paths));
        }
    });
}

/// The transform paths as an indented tree of checkboxes.
fn tree(ui: &mut Ui, state: &mut EditorState, rel: &str, mask: &AvatarMask) {
    let mut toggle = None;
    egui::ScrollArea::vertical()
        .id_salt("mask_tree")
        .max_height(420.0)
        .show(ui, |ui| {
            for t in &mask.transforms {
                let depth = if t.path.is_empty() {
                    0
                } else {
                    t.path.matches('/').count() + 1
                };
                let name = if t.path.is_empty() {
                    "(Root)"
                } else {
                    t.path.rsplit('/').next().unwrap_or(&t.path)
                };
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 14.0);
                    let mut on = t.active;
                    let parent_off = mask
                        .transforms
                        .iter()
                        .any(|p| p.path != t.path && !p.active && is_ancestor(&p.path, &t.path));
                    let text = if parent_off {
                        RichText::new(name).color(theme::TEXT_DIM)
                    } else {
                        RichText::new(name)
                    };
                    let r = ui
                        .checkbox(&mut on, text)
                        .on_hover_text(if t.path.is_empty() {
                            "The Animator's own object"
                        } else {
                            t.path.as_str()
                        });
                    if r.changed() {
                        toggle = Some((t.path.clone(), on));
                    }
                });
            }
        });
    if let Some((path, on)) = toggle {
        avatar_masks::edit(state, rel, "Toggle Mask Bone", false, |m| {
            m.set_active(&path, on, true)
        });
    }
}

/// Whether `a` is a proper ancestor path of `b` ("" is everyone's ancestor).
fn is_ancestor(a: &str, b: &str) -> bool {
    a != b && path_within(b, a)
}

/// Label of the clip conversion button (tests click it).
pub const CONVERT_LABEL: &str = "Convert to Humanoid";

/// Inspector for a humanoid Avatar (`.avatar`): the human bones and the rig bones they map to
/// (required bones the rig lacks in red), and converting a clip made for this rig into a
/// humanoid clip any rig with an Avatar can play.
pub fn avatar_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    use misty_engine::animation::{Avatar, HumanBone, humanoid};
    let avatar = match Avatar::load(&state.assets.absolute(rel)) {
        Ok(a) => a,
        Err(e) => {
            ui.colored_label(theme::AXIS_X, e);
            return;
        }
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::panels::animator::AVATAR_ICON).size(22.0));
        ui.label(crate::theme::bold(
            ui.ctx(),
            format!("{} (Avatar)", clip_name(rel)),
        ));
    });
    ui.separator();
    crate::asset_guids::path_label(ui, rel);
    super::card::sub_header(ui, "Humanoid Bones");
    egui::Grid::new("avatar_bones")
        .num_columns(2)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for b in HumanBone::ALL {
                ui.label(b.name());
                match avatar.bone(b) {
                    Some(x) => {
                        ui.label(RichText::new(&x.path).color(theme::TEXT_DIM));
                    }
                    None if b.required() => {
                        ui.colored_label(theme::AXIS_X, "Missing");
                    }
                    None => {
                        ui.weak("(optional)");
                    }
                }
                ui.end_row();
            }
        });
    ui.weak(format!("Hips height {:.2} m", avatar.hips_height));
    super::card::sub_header(ui, "Retargeting");
    ui.weak(
        "Turn a clip made for this rig into a humanoid clip that plays on any rig with an Avatar.",
    );
    let id = ui.id().with(("avatar_convert", rel));
    let clips: Vec<String> = state
        .assets
        .anims
        .iter()
        .filter(|c| !c.contains("(Humanoid)"))
        .cloned()
        .collect();
    let mut pick: Option<String> = ui
        .data(|d| d.get_temp::<String>(id))
        .filter(|c| clips.contains(c))
        .or_else(|| clips.first().cloned());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("avatar_clip", rel))
            .selected_text(
                pick.as_deref()
                    .map(clip_name)
                    .unwrap_or_else(|| "No clips".into()),
            )
            .show_ui(ui, |ui| {
                for c in &clips {
                    ui.selectable_value(&mut pick, Some(c.clone()), clip_name(c));
                }
            });
        if let Some(c) = &pick {
            ui.data_mut(|d| d.insert_temp(id, c.clone()));
        }
        if ui
            .add_enabled(pick.is_some(), egui::Button::new(CONVERT_LABEL))
            .clicked()
            && let Some(src) = pick.clone()
        {
            let result = misty_engine::animation::AnimationClip::load(&state.assets.absolute(&src))
                .map(|clip| humanoid::humanoid_clip(&clip, &avatar, 30.0))
                .and_then(|human| {
                    let stem = clip_name(&src);
                    let dir = state
                        .assets
                        .absolute(&src)
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default();
                    let path =
                        crate::asset_ops::unique_path(&dir, &format!("{stem} (Humanoid)"), "anim");
                    human.save(&path).map(|_| state.assets.relative(&path))
                });
            match result {
                Ok(out) => {
                    state.assets.anims.insert(out.clone());
                    state.console.info(format!("Humanoid clip written: {out}"));
                }
                Err(e) => state.console.warn(format!("Could not convert {src}: {e}")),
            }
        }
    });
}
