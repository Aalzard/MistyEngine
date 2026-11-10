//! The Terrain Inspector's Paint Trees tab: the terrain's tree prototypes (`.tree` assets) as a
//! palette of thumbnail cards (each tree's impostor, lit), the brush (plant with a left click,
//! remove with Shift), how new trees vary, Mass Place Trees, and how the trees are drawn.
//! Cards stand for their tree asset (Select in Project, double-click to edit) and `.tree` assets
//! dropped from the Project window are added. Every change is one undo step.

use egui::Ui;
use misty_engine::terrain::trees::{self, TreePrototype};
use misty_engine::terrain::{TerrainData, store};

use crate::branding::icons::Icon;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::terrain_kit::{self as kit, AddCard, Card, CardEvent, Thumb};
use crate::terrain_tools;
use crate::tutorials::targets;

/// Icon of tree assets (Project panel, Inspector, the Paint Trees list).
pub const TREE_ICON: &str = "🌳";

/// The sample oak's textures (the default tree's), written into a project that lacks them.
const OAK_TEXTURES: [(&str, &[u8]); 9] = [
    (
        "OakBark.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBark.png"),
    ),
    (
        "OakBarkNormal.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBarkNormal.png"),
    ),
    (
        "OakBarkRoughness.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBarkRoughness.png"),
    ),
    (
        "OakBarkAO.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBarkAO.png"),
    ),
    (
        "OakBarkHeight.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBarkHeight.png"),
    ),
    (
        "OakLeaves.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeaves.png"),
    ),
    (
        "OakLeavesNormal.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeavesNormal.png"),
    ),
    (
        "OakLeavesRoughness.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeavesRoughness.png"),
    ),
    (
        "OakLeavesTranslucency.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeavesTranslucency.png"),
    ),
];

/// Assets ▸ Create ▸ Tree: a new oak in `dir`, with the oak textures written into
/// `Assets/Trees/Textures` when the project has none yet.
pub fn create_tree_asset(
    root: &std::path::Path,
    dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let textures = root.join(misty_engine::trees::SAMPLE_TEXTURES);
    for (name, bytes) in OAK_TEXTURES {
        let path = textures.join(name);
        if !path.exists() {
            std::fs::create_dir_all(&textures).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let path = crate::asset_ops::unique_path(dir, "New Tree", misty_engine::trees::EXTENSION);
    misty_engine::trees::TreeAsset::oak().save(&path)?;
    Ok(path)
}

fn edit(state: &mut EditorState, path: &str, label: &str, f: impl FnOnce(&mut TerrainData)) {
    terrain_tools::begin_edit(state, path, label);
    store::modify_settings(path, f);
}

fn stem(p: &str) -> String {
    std::path::Path::new(p)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(none)".into())
}

/// Every `.tree` asset of the project (the GUID database lists them).
fn tree_assets() -> Vec<String> {
    let ext = format!(".{}", misty_engine::trees::EXTENSION);
    let mut out = misty_engine::asset_db::with(|db| {
        db.paths()
            .filter(|p| p.ends_with(&ext))
            .map(str::to_owned)
            .collect::<Vec<String>>()
    })
    .unwrap_or_default();
    out.sort();
    out
}

fn is_tree(rel: &str) -> bool {
    rel.ends_with(&format!(".{}", misty_engine::trees::EXTENSION))
}

/// The tree's thumbnail (its impostor, lit), or a placeholder while the tree store builds it.
fn tree_thumb(ui: &Ui, state: &mut EditorState, rel: &str) -> Thumb {
    match state.project.thumbnails.terrain.tree(ui.ctx(), rel) {
        Some(id) => Thumb::Image(id, None),
        None => Thumb::Icon {
            icon: Icon::TerrainTree,
            pending: misty_engine::trees::store::error(rel).is_none(),
        },
    }
}

fn add_tree(state: &mut EditorState, path: &str, d: &TerrainData, tree: String) {
    if let Some(i) = d.trees.prototypes.iter().position(|p| p.tree == tree) {
        state.terrain_tools.tree = i;
        return;
    }
    let n = d.trees.prototypes.len();
    edit(state, path, "Add Terrain Tree", |d| {
        d.trees.prototypes.push(TreePrototype { tree });
    });
    state.terrain_tools.tree = n;
}

/// Move prototype `from` to `to`; placed trees keep their tree.
pub(crate) fn move_prototype(d: &mut TerrainData, from: usize, to: usize) {
    let n = d.trees.prototypes.len();
    if from >= n || to >= n || from == to {
        return;
    }
    let p = d.trees.prototypes.remove(from);
    d.trees.prototypes.insert(to, p);
    // Old index -> new index.
    let mut order: Vec<usize> = (0..n).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    let mut new_of = vec![0u16; n];
    for (new, old) in order.iter().enumerate() {
        new_of[*old] = new as u16;
    }
    for t in &mut d.trees.instances {
        if let Some(n) = new_of.get(t.prototype as usize) {
            t.prototype = *n;
        }
    }
}

pub fn trees_tab(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let n = d.trees.prototypes.len();
    if state.terrain_tools.tree >= n && n > 0 {
        state.terrain_tools.tree = n - 1;
    }
    let selected = state.terrain_tools.tree;
    let version = store::get(path).map(|(_, v)| v).unwrap_or(0);
    let counts = state.project.thumbnails.terrain.counts(path, d, version);

    kit::section(
        ui,
        "trees_palette",
        Icon::TerrainTree,
        &tr!("Trees"),
        |ui| {
            let mut cards = Vec::with_capacity(n);
            for (i, p) in d.trees.prototypes.iter().enumerate() {
                let count = counts.trees.get(i).copied().unwrap_or(0);
                let tip = match misty_engine::trees::store::error(&p.tree) {
                    Some(e) => tr!(
                        "{tree} could not be built: {error}",
                        tree = p.tree,
                        error = e
                    ),
                    None => format!(
                        "{}\n{}",
                        p.tree,
                        tr!(
                            "{count} planted. Double-click to edit the tree, drag to reorder.",
                            count = count
                        )
                    ),
                };
                cards.push(Card {
                    name: stem(&p.tree),
                    thumb: tree_thumb(ui, state, &p.tree),
                    badge: Some(crate::terrain_details_ui::short_count(count)),
                    tip,
                    asset: Some(p.tree.clone()),
                    can_duplicate: false,
                    can_remove: true,
                    dimmed: false,
                });
            }
            let out = kit::palette(
                ui,
                "trees",
                &cards,
                Some(selected),
                Some(AddCard {
                    label: &tr!("Add Tree"),
                    tip: &tr!("Add a tree asset to plant (or drop a .tree asset here)"),
                    enabled: true,
                    target: "+ Add Tree",
                }),
                true,
                &is_tree,
                &tr!("No trees yet: add a tree asset to start planting."),
                targets::header,
            );
            if let Some(add) = &out.add {
                egui::Popup::menu(add).show(|ui| {
                    ui.set_min_width(180.0);
                    let assets = tree_assets();
                    if assets.is_empty() {
                        ui.weak(tr!("No .tree assets in the project"));
                    }
                    for a in &assets {
                        if ui
                            .selectable_label(false, format!("{TREE_ICON} {}", stem(a)))
                            .on_hover_text(a)
                            .clicked()
                        {
                            add_tree(state, path, d, a.clone());
                            ui.close();
                        }
                    }
                    ui.separator();
                    if kit::icon_button(
                        ui,
                        Icon::Add,
                        &tr!("New Tree Asset"),
                        &tr!("Create an oak tree asset in Assets/Trees and add it"),
                    )
                    .clicked()
                    {
                        let root = state.project.root.clone();
                        let dir = root.join("Assets").join("Trees");
                        let made = std::fs::create_dir_all(&dir)
                            .map_err(|e| e.to_string())
                            .and_then(|_| create_tree_asset(&root, &dir));
                        match made {
                            Ok(file) => {
                                let rel = file
                                    .strip_prefix(&root)
                                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                                    .unwrap_or_default();
                                state.refresh_assets();
                                add_tree(state, path, d, rel);
                            }
                            Err(e) => state.console.warn(e),
                        }
                        ui.close();
                    }
                });
            }
            for ev in out.events {
                match ev {
                    CardEvent::Select(i) => state.terrain_tools.tree = i,
                    CardEvent::Remove(i) => {
                        edit(state, path, "Remove Terrain Tree", |d| {
                            d.trees.remove_prototype(i)
                        });
                        if state.terrain_tools.tree >= i {
                            state.terrain_tools.tree = state.terrain_tools.tree.saturating_sub(1);
                        }
                    }
                    CardEvent::Move { from, to } => {
                        edit(state, path, "Reorder Terrain Trees", |d| {
                            move_prototype(d, from, to)
                        });
                        state.terrain_tools.tree = to;
                    }
                    CardEvent::Locate(i) => {
                        if let Some(p) = d.trees.prototypes.get(i) {
                            crate::terrain_details_ui::locate_asset(state, &p.tree);
                        }
                    }
                    CardEvent::Open(i) => {
                        if let Some(p) = d.trees.prototypes.get(i) {
                            crate::prefabs::select_asset(state, &p.tree);
                        }
                    }
                    CardEvent::Drop(rel) => add_tree(state, path, d, rel),
                    CardEvent::Duplicate(_) => {}
                }
            }
        },
    );

    // The selected tree, large.
    if let Some(p) = d.trees.prototypes.get(selected) {
        let thumb = tree_thumb(ui, state, &p.tree);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child.set_clip_rect(ui.clip_rect().intersect(rect));
            kit::paint_thumb(&child, rect, &thumb);
            ui.vertical(|ui| {
                ui.label(crate::theme::bold(
                    ui.ctx(),
                    crate::i18n::display(&stem(&p.tree)),
                ));
                ui.weak(&p.tree);
                if kit::icon_button(
                    ui,
                    Icon::TerrainTree,
                    &tr!("Edit Tree"),
                    &tr!("Open the tree asset in the Inspector (trunk, branches, leaves)"),
                )
                .clicked()
                {
                    crate::prefabs::select_asset(state, &p.tree);
                }
            });
        });
    }

    let max_h = d.size.y;
    kit::section(ui, "trees_brush", Icon::Brush, &tr!("Brush"), |ui| {
        let tools = &mut state.terrain_tools;
        let r = kit::slider(
            ui,
            &tr!("Brush Size"),
            &tr!("Brush diameter ([ and ] change it)"),
            &mut tools.size,
            2.0..=300.0,
            25.0,
            " m",
            true,
        );
        let _ = r;
        kit::slider(
            ui,
            &tr!("Density"),
            &tr!("Trees per 100 m² the brush fills up to"),
            &mut tools.tree_density,
            0.05..=4.0,
            1.0,
            "",
            true,
        );
        ui.weak(tr!("Left click plants trees, Shift + click removes them."));
    });
    kit::section(
        ui,
        "trees_placement",
        Icon::TerrainPlace,
        &tr!("New Trees"),
        |ui| {
            let def = trees::Placement::default();
            let p = &mut state.terrain_tools.tree_placement;
            kit::range(
                ui,
                &tr!("Tree Height"),
                &tr!("Random height scale of new trees"),
                &mut p.height,
                0.2..=4.0,
                def.height,
                "×",
                0.01,
            );
            kit::range(
                ui,
                &tr!("Tree Width"),
                &tr!("Random width scale of new trees"),
                &mut p.width,
                0.2..=4.0,
                def.width,
                "×",
                0.01,
            );
            kit::slider(
                ui,
                &tr!("Spacing"),
                &tr!("Closest two trees stand, trunk to trunk"),
                &mut p.spacing,
                0.5..=40.0,
                def.spacing,
                " m",
                false,
            );
            let mut slope = [p.min_slope, p.max_slope];
            kit::range(
                ui,
                &tr!("Slope"),
                &tr!("Ground steepness trees grow on"),
                &mut slope,
                0.0..=90.0,
                [def.min_slope, def.max_slope],
                "°",
                0.5,
            );
            [p.min_slope, p.max_slope] = slope;
            p.altitude[1] = p.altitude[1].min(max_h);
            kit::range(
                ui,
                &tr!("Height"),
                &tr!("Terrain heights trees grow at (above the terrain's base)"),
                &mut p.altitude,
                0.0..=max_h,
                [0.0, max_h],
                " m",
                0.1,
            );
            kit::check(
                ui,
                &tr!("Random Rotation"),
                &tr!("Turn every new tree at random about its trunk"),
                &mut p.random_rotation,
                def.random_rotation,
            );
        },
    );
    kit::section(ui, "trees_mass", Icon::Fill, &tr!("Whole Terrain"), |ui| {
        ui.horizontal(|ui| {
            let tools = &mut state.terrain_tools;
            ui.add(
                egui::DragValue::new(&mut tools.mass_count)
                    .range(1..=20_000)
                    .suffix(tr!(" trees")),
            )
            .on_hover_text(tr!("How many trees of each tree in the palette Mass Place adds"));
            let any = !d.trees.prototypes.is_empty();
            let place = ui
                .add_enabled_ui(any, |ui| {
                    kit::icon_button(ui, Icon::Fill, &tr!("Mass Place"), &tr!("Scatter this many trees of every tree in the palette over the whole terrain"))
                })
                .inner;
            if place.clicked() {
                let count = tools.mass_count;
                let mut p = tools.tree_placement;
                p.seed = d.trees.instances.len() as u32 + 1;
                let protos: Vec<u16> = (0..d.trees.prototypes.len() as u16).collect();
                let mut placed = 0;
                edit(state, path, "Mass Place Trees", |d| {
                    placed = trees::mass_place(d, count, &protos, &p);
                });
                state.console.info(tr!("Placed {count} trees.", count = placed));
            }
            if kit::confirm_button(
                ui,
                Icon::Clear,
                &tr!("Clear Trees"),
                &tr!("Remove every planted tree (the palette stays)"),
                &tr!("Remove all {count} trees from this terrain?", count = d.trees.instances.len()),
                &tr!("Clear"),
                !d.trees.instances.is_empty(),
            ) {
                edit(state, path, "Clear Terrain Trees", |d| d.trees.instances.clear());
            }
        });
        ui.weak(tr!(
            "{count} trees planted.",
            count = d.trees.instances.len()
        ));
    });

    // How they are drawn.
    let mut s = d.trees.clone();
    let def = trees::TreeSettings::default();
    let mut ch = false;
    kit::section_closed(
        ui,
        "trees_rendering",
        Icon::TerrainSettings,
        &tr!("Tree Rendering"),
        |ui| {
            ch |= kit::slider(
                ui,
                &tr!("Draw Distance"),
                &tr!("Trees further away are not drawn"),
                &mut s.draw_distance,
                50.0..=5000.0,
                def.draw_distance,
                " m",
                true,
            );
            ch |= kit::slider(
                ui,
                &tr!("Detail Distance"),
                &tr!(
                    "Full-detail meshes up to here, lighter ones beyond (× the quality level's LOD bias)"
                ),
                &mut s.lod_distance,
                5.0..=200.0,
                def.lod_distance,
                " m",
                false,
            );
            ch |= kit::slider(
                ui,
                &tr!("Billboard Start"),
                &tr!("Meshes up to here, billboards beyond (× the LOD bias)"),
                &mut s.billboard_start,
                10.0..=1000.0,
                def.billboard_start,
                " m",
                true,
            );
            ch |= kit::check(
                ui,
                &tr!("Cast Shadows"),
                &tr!("Trees cast shadows"),
                &mut s.cast_shadows,
                def.cast_shadows,
            );
        },
    );
    if ch {
        edit(state, path, "Change Tree Rendering", |d| {
            d.trees.draw_distance = s.draw_distance;
            d.trees.lod_distance = s.lod_distance;
            d.trees.billboard_start = s.billboard_start;
            d.trees.cast_shadows = s.cast_shadows;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::terrain::trees::TreeInstance;

    #[test]
    fn moving_a_tree_keeps_planted_trees_on_their_tree() {
        let mut d = TerrainData::new(17, glam::Vec3::splat(16.0), 16);
        for t in ["A", "B", "C"] {
            d.trees.prototypes.push(TreePrototype { tree: t.into() });
        }
        for p in [0u16, 1, 2, 2] {
            d.trees.instances.push(TreeInstance {
                prototype: p,
                ..Default::default()
            });
        }
        move_prototype(&mut d, 2, 0);
        let names: Vec<&str> = d.trees.prototypes.iter().map(|p| p.tree.as_str()).collect();
        assert_eq!(names, ["C", "A", "B"]);
        let of = |i: usize| {
            d.trees.prototypes[d.trees.instances[i].prototype as usize]
                .tree
                .clone()
        };
        assert_eq!([of(0), of(1), of(2), of(3)], ["A", "B", "C", "C"]);
    }
}
