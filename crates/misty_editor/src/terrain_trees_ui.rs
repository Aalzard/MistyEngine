//! The Terrain Inspector's Paint Trees tab: the terrain's tree prototypes (`.tree` assets) to
//! pick from, the brush (plant with a left click, remove with Shift), how new trees vary, Mass
//! Place Trees, and how the trees are drawn. Every change is one undo step.

use egui::{RichText, Ui};
use misty_engine::terrain::trees::{self, TreePrototype};
use misty_engine::terrain::{TerrainData, store};

use crate::state::EditorState;
use crate::terrain_tools;

/// Icon of tree assets (Project panel, Inspector, the Paint Trees list).
pub const TREE_ICON: &str = "🌳";

/// The sample oak's textures (the default tree's), written into a project that lacks them.
const OAK_TEXTURES: [(&str, &[u8]); 4] = [
    (
        "OakBark.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBark.png"),
    ),
    (
        "OakBarkNormal.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakBarkNormal.png"),
    ),
    (
        "OakLeaves.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeaves.png"),
    ),
    (
        "OakLeavesNormal.png",
        include_bytes!("../../../SampleProject/Assets/Trees/Textures/OakLeavesNormal.png"),
    ),
];

/// Assets ▸ Create ▸ Tree: a new oak in `dir`, with the oak textures written into
/// `Assets/Trees/Textures` when the project has none yet.
pub fn create_tree_asset(root: &std::path::Path, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
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

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body);
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

fn range(ui: &mut Ui, v: &mut [f32; 2], lo: f32, hi: f32) {
    ui.horizontal(|ui| {
        let (a, b) = (v[0], v[1]);
        ui.add(egui::DragValue::new(&mut v[0]).speed(0.01).range(lo..=b));
        ui.label("–");
        ui.add(egui::DragValue::new(&mut v[1]).speed(0.01).range(a..=hi));
    });
}

pub fn trees_tab(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    // Trees to paint with.
    ui.add_space(4.0);
    ui.label(crate::theme::bold(ui.ctx(), "Trees"));
    let selected = state.terrain_tools.tree;
    let mut open = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        if d.trees.prototypes.is_empty() {
            ui.weak("No trees yet. Add a tree asset (Assets ▸ Create ▸ Tree makes one).");
        }
        for (i, p) in d.trees.prototypes.iter().enumerate() {
            let count = d.trees.instances.iter().filter(|t| t.prototype as usize == i).count();
            let text = format!("{TREE_ICON} {}   ({count})", stem(&p.tree));
            let r = ui
                .selectable_label(i == selected, text)
                .on_hover_text(format!("{}
Double-click to edit the tree", p.tree));
            if r.clicked() {
                state.terrain_tools.tree = i;
            }
            if r.double_clicked() {
                open = Some(p.tree.clone());
            }
        }
    });
    if let Some(rel) = open {
        crate::prefabs::select_asset(state, &rel);
    }
    ui.horizontal(|ui| {
        let assets = tree_assets();
        let mut add = None;
        egui::ComboBox::from_id_salt("terrain_add_tree")
            .selected_text("+ Add Tree")
            .show_ui(ui, |ui| {
                if assets.is_empty() {
                    ui.weak("No .tree assets in the project");
                }
                for a in &assets {
                    if ui.selectable_label(false, stem(a)).on_hover_text(a).clicked() {
                        add = Some(a.clone());
                    }
                }
            });
        if let Some(tree) = add {
            let n = d.trees.prototypes.len();
            edit(state, path, "Add Terrain Tree", |d| {
                d.trees.prototypes.push(TreePrototype { tree });
            });
            state.terrain_tools.tree = n;
        }
        if ui
            .add_enabled(selected < d.trees.prototypes.len(), egui::Button::new("− Remove"))
            .on_hover_text("Remove this tree and every tree placed with it")
            .clicked()
        {
            edit(state, path, "Remove Terrain Tree", |d| d.trees.remove_prototype(selected));
            state.terrain_tools.tree = selected.saturating_sub(1);
        }
    });

    // Brush and placement.
    ui.add_space(6.0);
    let tools = &mut state.terrain_tools;
    grid(ui, "terrain_tree_brush", |ui| {
        ui.label("Brush Size");
        ui.add(egui::Slider::new(&mut tools.size, 2.0..=300.0).logarithmic(true).suffix(" m"));
        ui.end_row();
        ui.label("Density").on_hover_text("Trees per 100 m² the brush fills up to");
        ui.add(egui::Slider::new(&mut tools.tree_density, 0.05..=4.0).logarithmic(true));
        ui.end_row();
        let p = &mut tools.tree_placement;
        ui.label("Tree Height").on_hover_text("Random height scale of new trees");
        range(ui, &mut p.height, 0.2, 4.0);
        ui.end_row();
        ui.label("Tree Width").on_hover_text("Random width scale of new trees");
        range(ui, &mut p.width, 0.2, 4.0);
        ui.end_row();
        ui.label("Spacing").on_hover_text("Closest two trees stand, trunk to trunk");
        ui.add(egui::Slider::new(&mut p.spacing, 0.5..=40.0).suffix(" m"));
        ui.end_row();
        ui.label("Max Slope").on_hover_text("No trees on steeper ground");
        ui.add(egui::Slider::new(&mut p.max_slope, 0.0..=60.0).suffix("°"));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let tools = &mut state.terrain_tools;
        ui.add(egui::DragValue::new(&mut tools.mass_count).range(1..=20_000).suffix(" trees"));
        let any = !d.trees.prototypes.is_empty();
        if ui
            .add_enabled(any, egui::Button::new("Mass Place Trees"))
            .on_hover_text("Scatter this many trees of every tree in the list over the whole terrain")
            .clicked()
        {
            let count = tools.mass_count;
            let mut p = tools.tree_placement;
            p.seed = d.trees.instances.len() as u32 + 1;
            let protos: Vec<u16> = (0..d.trees.prototypes.len() as u16).collect();
            let mut placed = 0;
            edit(state, path, "Mass Place Trees", |d| {
                placed = trees::mass_place(d, count, &protos, &p);
            });
            state.console.info(format!("Placed {placed} trees."));
        }
        if ui
            .add_enabled(!d.trees.instances.is_empty(), egui::Button::new("Clear Trees"))
            .on_hover_text("Remove every placed tree (the list stays)")
            .clicked()
        {
            edit(state, path, "Clear Terrain Trees", |d| d.trees.instances.clear());
        }
    });

    // How they are drawn.
    ui.add_space(6.0);
    let mut s = d.trees.clone();
    let mut ch = false;
    egui::CollapsingHeader::new(RichText::new("Tree Rendering").strong())
        .id_salt("terrain_tree_rendering")
        .default_open(false)
        .show(ui, |ui| {
            grid(ui, "terrain_tree_rendering_grid", |ui| {
                ui.label("Draw Distance").on_hover_text("Trees further away are not drawn");
                ch |= ui.add(egui::Slider::new(&mut s.draw_distance, 50.0..=5000.0).logarithmic(true).suffix(" m")).changed();
                ui.end_row();
                ui.label("Detail Distance").on_hover_text("Full-detail meshes up to here, lighter ones beyond (× the quality level's LOD bias)");
                ch |= ui.add(egui::Slider::new(&mut s.lod_distance, 5.0..=200.0).suffix(" m")).changed();
                ui.end_row();
                ui.label("Billboard Start").on_hover_text("Meshes up to here, billboards beyond (× the LOD bias)");
                ch |= ui.add(egui::Slider::new(&mut s.billboard_start, 10.0..=1000.0).logarithmic(true).suffix(" m")).changed();
                ui.end_row();
                ui.label("Cast Shadows");
                ch |= ui.checkbox(&mut s.cast_shadows, "").changed();
                ui.end_row();
            });
        });
    if ch {
        edit(state, path, "Change Tree Rendering", |d| {
            d.trees.draw_distance = s.draw_distance;
            d.trees.lod_distance = s.lod_distance;
            d.trees.billboard_start = s.billboard_start;
            d.trees.cast_shadows = s.cast_shadows;
        });
    }
    ui.weak(format!("{} trees placed.", d.trees.instances.len()));
}
