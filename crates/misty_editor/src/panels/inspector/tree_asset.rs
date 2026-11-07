//! Inspector of a `.tree` asset (a procedural tree recipe): a species preset to start from,
//! then the shape, branches, leaves, bark, textures and wind, with the built tree's triangle
//! counts. Edits save when the mouse is released, and every terrain using the tree rebuilds it
//! (the renderer keeps drawing the old one meanwhile).

use egui::{RichText, Ui};
use misty_engine::trees::{CrownShape, TreeAsset, store};

use super::card::grid;
use super::slots::texture_slot;
use crate::state::EditorState;
use crate::theme;

use crate::terrain_trees_ui::TREE_ICON;

fn draft_id(rel: &str) -> egui::Id {
    egui::Id::new(("tree_draft", rel.to_owned()))
}

fn name(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn srgb(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut v = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    let ch = ui.color_edit_button_srgb(&mut v).changed();
    if ch {
        *c = v.map(|x| x as f32 / 255.0);
    }
    ch
}

fn slider(
    ui: &mut Ui,
    label: &str,
    tip: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    ui.label(label).on_hover_text(tip);
    ui.add(egui::Slider::new(v, range).suffix(suffix))
        .on_hover_text(tip);
    ui.end_row();
}

fn count(ui: &mut Ui, label: &str, tip: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.label(label).on_hover_text(tip);
    ui.add(egui::Slider::new(v, range)).on_hover_text(tip);
    ui.end_row();
}

fn section(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(("tree_section", title))
        .default_open(open)
        .show(ui, body);
}

pub fn tree_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(TREE_ICON).size(22.0));
        ui.label(theme::bold(ui.ctx(), format!("{} (Tree)", name(rel))));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let abs = state.assets.absolute(rel);
    let saved = match TreeAsset::load(&abs) {
        Ok(t) => t,
        Err(e) => {
            ui.colored_label(theme::AXIS_X, format!("Could not read the tree: {e}"));
            return;
        }
    };
    let id = draft_id(rel);
    let mut t = ui
        .ctx()
        .data(|d| d.get_temp::<TreeAsset>(id))
        .unwrap_or_else(|| saved.clone());
    let textures = state.assets.textures.clone();

    ui.horizontal(|ui| {
        ui.label("Start from");
        for (label, preset) in [("Oak", TreeAsset::oak()), ("Linden", TreeAsset::linden()), ("Birch", TreeAsset::birch())] {
            if ui
                .button(label)
                .on_hover_text(format!("Replace every setting with the {label} preset (undo with Ctrl+Z in the file's history is not kept: copy the asset first to keep it)"))
                .clicked()
            {
                t = preset;
            }
        }
    });
    ui.add_space(4.0);

    section(ui, "Shape", true, |ui| {
        grid(ui, "tree_shape", |ui| {
            ui.label("Seed")
                .on_hover_text("Another branching and leaf layout of the same kind of tree");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut t.seed).range(0..=999_999));
                if ui.button("🎲").on_hover_text("New random seed").clicked() {
                    t.seed = (t.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345)) % 1_000_000;
                }
            });
            ui.end_row();
            slider(
                ui,
                "Height",
                "From the foot to the top of the crown",
                &mut t.height,
                2.0..=40.0,
                " m",
            );
            ui.label("Crown Shape");
            egui::ComboBox::from_id_salt("tree_crown_shape")
                .selected_text(t.crown_shape.label())
                .show_ui(ui, |ui| {
                    for s in CrownShape::ALL {
                        ui.selectable_value(&mut t.crown_shape, s, s.label());
                    }
                });
            ui.end_row();
            slider(
                ui,
                "Crown Width",
                "Crown diameter as a share of the height",
                &mut t.crown_width,
                0.2..=1.6,
                "",
            );
            slider(
                ui,
                "Crown Base",
                "Where the crown starts (the bare trunk below)",
                &mut t.crown_base,
                0.05..=0.8,
                "",
            );
            slider(
                ui,
                "Trunk Radius",
                "Trunk radius at the foot",
                &mut t.trunk_radius,
                0.05..=1.2,
                " m",
            );
            slider(
                ui,
                "Leader",
                "0 = the trunk divides into limbs inside the crown (oak), 1 = it runs to the top (birch)",
                &mut t.leader,
                0.0..=1.0,
                "",
            );
            slider(
                ui,
                "Trunk Bend",
                "Lean and wobble of the trunk",
                &mut t.trunk_bend,
                0.0..=1.0,
                "",
            );
        });
    });
    section(ui, "Branches", true, |ui| {
        grid(ui, "tree_branches", |ui| {
            count(ui, "Limbs", "Main limbs", &mut t.branches, 1..=30);
            slider(
                ui,
                "Angle",
                "Angle between the limbs and the trunk",
                &mut t.branch_angle,
                10.0..=100.0,
                "°",
            );
            slider(
                ui,
                "Curve",
                "Limbs curve up towards the light (+) or droop (−)",
                &mut t.branch_curve,
                -1.0..=1.0,
                "",
            );
            count(
                ui,
                "Side Branches",
                "Side branches on the longest limb",
                &mut t.twigs,
                0..=24,
            );
        });
    });
    section(ui, "Leaves", true, |ui| {
        grid(ui, "tree_leaves", |ui| {
            slider(
                ui,
                "Density",
                "How thickly the leaf clusters cover the crown",
                &mut t.leaf_density,
                0.1..=2.5,
                "",
            );
            slider(
                ui,
                "Cluster Size",
                "Side of one leaf cluster card",
                &mut t.leaf_size,
                0.2..=3.0,
                " m",
            );
            ui.label("Tint");
            srgb(ui, &mut t.leaf_color);
            ui.end_row();
            slider(
                ui,
                "Variation",
                "Colour differences between clusters and trees",
                &mut t.leaf_variation,
                0.0..=1.0,
                "",
            );
            slider(
                ui,
                "Translucency",
                "Sunlight shining through the leaves",
                &mut t.translucency,
                0.0..=2.0,
                "",
            );
        });
    });
    section(ui, "Bark & Textures", false, |ui| {
        grid(ui, "tree_textures", |ui| {
            ui.label("Bark Tint");
            srgb(ui, &mut t.bark_color);
            ui.end_row();
            let slot = |ui: &mut Ui, label: &str, id: &str, v: &mut String| {
                ui.label(label);
                let mut o = (!v.is_empty()).then(|| v.clone());
                if texture_slot(ui, id, &mut o, &textures) {
                    *v = o.unwrap_or_default();
                }
                ui.end_row();
            };
            slot(ui, "Bark", "tree_bark", &mut t.bark_texture);
            slot(ui, "Bark Normal", "tree_bark_n", &mut t.bark_normal);
            slot(ui, "Bark Roughness", "tree_bark_r", &mut t.bark_roughness);
            slot(ui, "Bark Occlusion", "tree_bark_ao", &mut t.bark_occlusion);
            slot(ui, "Bark Height", "tree_bark_h", &mut t.bark_height);
            slot(ui, "Leaves", "tree_leaf", &mut t.leaf_texture);
            slot(ui, "Leaf Normal", "tree_leaf_n", &mut t.leaf_normal);
            slot(ui, "Leaf Roughness", "tree_leaf_r", &mut t.leaf_roughness);
            slot(
                ui,
                "Leaf Translucency",
                "tree_leaf_t",
                &mut t.leaf_translucency,
            );
        });
        ui.weak("Bark: square, tileable maps (u around the trunk). Leaves: a 2 × 2 atlas of leafy twigs, the twig entering at the bottom centre of each cell, alpha = cut-out. Roughness, occlusion, height and translucency are grayscale; empty slots use plain values.");
    });
    section(ui, "Wind", false, |ui| {
        grid(ui, "tree_wind", |ui| {
            slider(
                ui,
                "Trunk Bend",
                "The whole tree bending from its foot",
                &mut t.wind.trunk_bend,
                0.0..=3.0,
                "",
            );
            slider(
                ui,
                "Branch Sway",
                "Limbs and their twigs swaying",
                &mut t.wind.branch_sway,
                0.0..=3.0,
                "",
            );
            slider(
                ui,
                "Leaf Flutter",
                "Leaf clusters fluttering",
                &mut t.wind.leaf_flutter,
                0.0..=3.0,
                "",
            );
        });
        ui.weak("Multiplies the terrain's wind (Grass & Details) and the scene's Wind Zone.");
    });

    // The built tree.
    ui.add_space(4.0);
    match store::get(rel) {
        Some(b) => {
            let m = &b.model;
            ui.label(
                RichText::new(format!(
                    "{} × {:.1} m · LOD 0 {} triangles · LOD 1 {} · far: billboard",
                    TREE_ICON,
                    m.max.y,
                    m.lods[0].triangles(),
                    m.lods[1].triangles()
                ))
                .color(theme::TEXT_DIM),
            );
        }
        None => match store::error(rel) {
            Some(e) => {
                ui.colored_label(theme::AXIS_X, e);
            }
            None => {
                ui.weak("Building…");
                ui.ctx().request_repaint();
            }
        },
    }
    ui.weak("Place trees with a Terrain's Paint Trees tab.");

    t.sanitize();
    if t == saved {
        ui.ctx().data_mut(|d| d.remove::<TreeAsset>(id));
        return;
    }
    // Save once the mouse is released (no rebuild per slider step).
    if ui.input(|i| i.pointer.any_down()) {
        ui.ctx().data_mut(|d| d.insert_temp(id, t));
        return;
    }
    match t.save(&abs) {
        Ok(()) => store::invalidate(rel),
        Err(e) => state.console.warn(format!("Could not save {rel}: {e}")),
    }
    ui.ctx().data_mut(|d| d.remove::<TreeAsset>(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_file() {
        assert_eq!(name("Assets/Trees/Young Oak.tree"), "Young Oak");
    }
}
