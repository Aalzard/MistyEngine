//! The Terrain Inspector's Paint Details tab: the terrain grass and the terrain's extra grass
//! species and detail meshes (`terrain::detail_layers`) as a palette of live preview cards, the
//! brush's target density with Fill / Clear, and the selected type's settings in sections with
//! live previews, reset buttons and tooltips. Every change is one undo step.
//!
//! Cards can stand for assets (detail meshes made from a model or prefab point at it); models
//! and prefabs dragged from the Project window onto the palette become detail meshes.

use std::hash::{Hash, Hasher};

use egui::Ui;
use misty_engine::glam::Mat4;
use misty_engine::render::MeshDraw;
use misty_engine::scene::{MeshFilter, MeshRef, MeshRenderer, PrimitiveMesh};
use misty_engine::terrain::detail_layers::{
    self, DetailKind, DetailLayer, DetailTarget, MAX_DETAIL_LAYERS, MAX_MESH_DENSITY,
};
use misty_engine::terrain::details::{self, DetailSettings, MAX_DENSITY};
use misty_engine::terrain::{TerrainData, store};

use crate::branding::icons::Icon;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::terrain_kit::{self as kit, AddCard, Card, CardEvent, Thumb};
use crate::terrain_previews::{MeshJob, TuftParams};
use crate::terrain_tools;
use crate::tutorials::targets;

/// The brush's target: palette card 0 is the terrain grass, card `i + 1` is detail layer `i`.
pub fn target(row: usize) -> DetailTarget {
    match row {
        0 => DetailTarget::TerrainGrass,
        r => DetailTarget::Layer(r - 1),
    }
}

fn edit(state: &mut EditorState, path: &str, label: &str, f: impl FnOnce(&mut TerrainData)) {
    terrain_tools::begin_edit(state, path, label);
    store::modify(path, f);
}

fn edit_settings(
    state: &mut EditorState,
    path: &str,
    label: &str,
    f: impl FnOnce(&mut TerrainData),
) {
    terrain_tools::begin_edit(state, path, label);
    store::modify_settings(path, f);
}

/// A file name without its folder and extension.
pub(crate) fn stem(p: &str) -> String {
    let p = p.split('#').next().unwrap_or(p);
    std::path::Path::new(p)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Compact count for badges: 950, 12k, 3.4M.
pub fn short_count(n: usize) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => format!("{:.1}k", n as f32 / 1000.0),
        10_000..1_000_000 => format!("{}k", n / 1000),
        _ => format!("{:.1}M", n as f32 / 1_000_000.0),
    }
}

/// The model or prefab a mesh layer was made from (shown as the card's asset).
fn layer_asset(l: &DetailLayer) -> Option<String> {
    match &l.mesh {
        MeshRef::Asset(a) => Some(a.split('#').next().unwrap_or(a).to_owned()),
        MeshRef::Primitive(_) => None,
    }
}

/// Offscreen thumbnail key of a mesh with a material (changes with the material file).
fn mesh_key(mesh: &MeshRef, material: Option<&str>, state: &EditorState) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{mesh:?}").hash(&mut h);
    material.hash(&mut h);
    if let Some(m) = material {
        std::fs::metadata(state.assets.absolute(m))
            .and_then(|m| m.modified())
            .ok()
            .hash(&mut h);
    }
    h.finish()
}

/// Thumbnail of a mesh layer (rendered offscreen; an icon until then).
pub(crate) fn mesh_thumb(state: &mut EditorState, mesh: &MeshRef, material: Option<&str>) -> Thumb {
    let key = mesh_key(mesh, material, state);
    let params = state.assets.params(material);
    let mesh = mesh.clone();
    let tex = state.project.thumbnails.terrain.mesh_thumb(key, || {
        MeshJob::new(vec![MeshDraw {
            mesh,
            model: Mat4::IDENTITY,
            material: params,
            cast_shadows: false,
            receive_shadows: false,
            outline: false,
            sorting: Default::default(),
            lod: None,
        }])
    });
    match tex {
        Some(id) => Thumb::Image(id, None),
        None => Thumb::Icon {
            icon: Icon::TerrainPlace,
            pending: true,
        },
    }
}

/// Thumbnail of a grass tuft (built at once unless this frame's budget is spent).
pub(crate) fn tuft_thumb(ui: &Ui, state: &mut EditorState, p: &TuftParams) -> Thumb {
    match state.project.thumbnails.terrain.tuft(ui.ctx(), p) {
        Some(t) => Thumb::Tuft(t),
        None => Thumb::Icon {
            icon: Icon::TerrainGrass,
            pending: true,
        },
    }
}

/// A model, a mesh asset or a prefab with a mesh (what can become a detail mesh).
fn accepts(rel: &str) -> bool {
    misty_engine::models::is_model(rel) || rel.ends_with(".prefab")
}

/// The mesh and material of a dropped model or prefab (a prefab's first renderer).
fn dropped_mesh(state: &mut EditorState, rel: &str) -> Option<(MeshRef, Option<String>)> {
    if misty_engine::models::is_model(rel) {
        return Some((MeshRef::asset(rel, 0), None));
    }
    let prefab = state.assets.prefab(rel).ok()?;
    let (scene, _) = prefab.to_scene("Detail");
    let mut found = None;
    for (e, _) in scene.walk() {
        let Ok(f) = scene.world.get::<&MeshFilter>(e) else {
            continue;
        };
        let Some(mesh) = f.mesh.clone() else {
            continue;
        };
        let material = scene
            .world
            .get::<&MeshRenderer>(e)
            .ok()
            .and_then(|r| r.material.clone());
        found = Some((mesh, material));
        break;
    }
    found
}

pub fn details_tab(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let rows = d.detail_layers.len() + 1;
    if state.terrain_tools.detail >= rows {
        state.terrain_tools.detail = rows - 1;
    }
    let selected = state.terrain_tools.detail;
    let version = store::get(path).map(|(_, v)| v).unwrap_or(0);
    let counts = state.project.thumbnails.terrain.counts(path, d, version);

    // The palette: terrain grass first, then the species and meshes.
    let full = d.detail_layers.len() >= MAX_DETAIL_LAYERS;
    kit::section(
        ui,
        "details_types",
        Icon::TerrainGrass,
        &tr!("Grass & Detail Types"),
        |ui| {
            let mut cards = Vec::with_capacity(rows);
            let grass = TuftParams::terrain_grass(&d.details);
            cards.push(Card {
            name: tr!("Terrain Grass"),
            thumb: tuft_thumb(ui, state, &grass),
            badge: (counts.terrain_grass > 0).then(|| short_count(counts.terrain_grass)),
            tip: if d.details.enabled {
                tr!("The terrain's own grass. Generate (Paint Texture ▸ Grass & Details) fills it from its rules.")
            } else {
                tr!("The terrain's own grass, switched off. Turn it on below or under Paint Texture ▸ Grass & Details.")
            },
            asset: None,
            can_duplicate: false,
            can_remove: false,
            dimmed: !d.details.enabled,
        });
            for (i, l) in d.detail_layers.iter().enumerate() {
                let (thumb, kind) = match l.kind {
                    DetailKind::Grass => (
                        tuft_thumb(ui, state, &TuftParams::species(l, &d.details)),
                        tr!("Grass species"),
                    ),
                    DetailKind::Mesh => (
                        mesh_thumb(state, &l.mesh, l.material.as_deref()),
                        tr!("Detail mesh"),
                    ),
                };
                let count = counts.details.get(i).copied().unwrap_or(0);
                cards.push(Card {
                    name: l.name.clone(),
                    thumb,
                    badge: Some(short_count(count)),
                    tip: format!(
                        "{kind}\n{}",
                        tr!("{count} placed. Drag to reorder.", count = count)
                    ),
                    asset: layer_asset(l),
                    can_duplicate: !full,
                    can_remove: true,
                    dimmed: false,
                });
            }
            let out = kit::palette(
                ui,
                "details",
                &cards,
                Some(selected),
                Some(AddCard {
                    label: &tr!("Add"),
                    tip: &tr!(
                        "Add a grass species or a detail mesh (or drop a model or prefab here)"
                    ),
                    enabled: !full,
                    target: "+ Add Detail",
                }),
                true,
                &accepts,
                "",
                |ui, name, rect| targets::header(ui, name, rect),
            );
            if let Some(add) = &out.add {
                egui::Popup::menu(add).show(|ui| {
                    if kit::icon_button(
                        ui,
                        Icon::TerrainGrass,
                        &tr!("Grass Species"),
                        &tr!("Another grass with its own height, colour and density"),
                    )
                    .clicked()
                    {
                        let n = d.detail_layers.len();
                        edit(state, path, "Add Detail Grass", |d| {
                            d.detail_layers
                                .push(DetailLayer::grass(&format!("Grass {}", n + 1)));
                            // Species draw with the terrain grass.
                            d.details.enabled = true;
                        });
                        state.terrain_tools.detail = n + 1;
                        ui.close();
                    }
                    if kit::icon_button(
                        ui,
                        Icon::TerrainPlace,
                        &tr!("Detail Mesh"),
                        &tr!("Small meshes (pebbles, ferns, stones) painted with a density"),
                    )
                    .clicked()
                    {
                        add_mesh(
                            state,
                            path,
                            d,
                            MeshRef::Primitive(PrimitiveMesh::Sphere),
                            None,
                            None,
                        );
                        ui.close();
                    }
                });
            }
            for ev in out.events {
                handle(state, path, d, ev);
            }
        },
    );

    paint_section(ui, state, path, d, selected);

    // The selected type's settings.
    match selected.checked_sub(1).and_then(|i| d.detail_layers.get(i)) {
        None => terrain_grass_settings(ui, state, path, d),
        Some(l) => {
            let mut l = l.clone();
            let ch = match l.kind {
                DetailKind::Grass => species_settings(ui, state, d, &mut l),
                DetailKind::Mesh => mesh_settings(ui, state, &mut l),
            };
            if ch {
                let i = selected - 1;
                terrain_tools::begin_edit(state, path, "Change Detail Layer");
                // Look only: no map changes, so no re-upload of the terrain's maps.
                store::modify_settings(path, |d| {
                    if let Some(slot) = d.detail_layers.get_mut(i) {
                        let map = std::mem::take(&mut slot.map);
                        *slot = DetailLayer { map, ..l };
                    }
                });
            }
        }
    }
}

fn add_mesh(
    state: &mut EditorState,
    path: &str,
    d: &TerrainData,
    mesh: MeshRef,
    material: Option<String>,
    name: Option<String>,
) {
    let n = d.detail_layers.len();
    if n >= MAX_DETAIL_LAYERS {
        return;
    }
    let name = name.unwrap_or_else(|| format!("Detail Mesh {}", n + 1));
    edit(state, path, "Add Detail Mesh", |d| {
        d.detail_layers
            .push(DetailLayer::mesh(&name, mesh, material));
    });
    state.terrain_tools.detail = n + 1;
}

/// Show `rel` in the Project window (its folder open, the file selected).
pub(crate) fn locate_asset(state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    if let Some(dir) = abs.parent() {
        state.project.search.clear();
        state.project.open_folder(dir.to_path_buf());
    }
    state.project.selected = Some(abs);
}

fn handle(state: &mut EditorState, path: &str, d: &TerrainData, ev: CardEvent) {
    let layer = |i: usize| i.checked_sub(1).filter(|i| *i < d.detail_layers.len());
    match ev {
        CardEvent::Select(i) => state.terrain_tools.detail = i,
        CardEvent::Duplicate(i) => {
            if let Some(li) = layer(i) {
                let mut copy = d.detail_layers[li].settings();
                copy.name = tr!("{name} Copy", name = copy.name);
                edit(state, path, "Duplicate Detail Layer", |d| {
                    d.detail_layers.insert(li + 1, copy);
                });
                state.terrain_tools.detail = i + 1;
            }
        }
        CardEvent::Remove(i) => {
            if let Some(li) = layer(i) {
                edit(state, path, "Remove Detail Layer", |d| {
                    if li < d.detail_layers.len() {
                        d.detail_layers.remove(li);
                    }
                });
                let sel = state.terrain_tools.detail;
                if sel >= i && sel > 0 {
                    state.terrain_tools.detail = sel - 1;
                }
            }
        }
        CardEvent::Move { from, to } => {
            if let (Some(a), Some(b)) = (layer(from), layer(to)) {
                edit(state, path, "Reorder Detail Layers", |d| {
                    let l = d.detail_layers.remove(a);
                    d.detail_layers.insert(b, l);
                });
                state.terrain_tools.detail = to;
            }
        }
        CardEvent::Locate(i) => {
            if let Some(a) = layer(i).and_then(|li| layer_asset(&d.detail_layers[li])) {
                locate_asset(state, &a);
            }
        }
        CardEvent::Open(i) => {
            if let Some(a) = layer(i).and_then(|li| layer_asset(&d.detail_layers[li])) {
                crate::prefabs::select_asset(state, &a);
            }
        }
        CardEvent::Drop(rel) => {
            if let Some((mesh, material)) = dropped_mesh(state, &rel) {
                add_mesh(state, path, d, mesh, material, Some(stem(&rel)));
            } else {
                state.console.warn(tr!(
                    "{asset} has no mesh to paint as a detail.",
                    asset = rel
                ));
            }
        }
    }
}

/// Target density, Fill and Clear for the selected type.
fn paint_section(
    ui: &mut Ui,
    state: &mut EditorState,
    path: &str,
    d: &TerrainData,
    selected: usize,
) {
    let which = target(selected);
    let name = match selected.checked_sub(1).and_then(|i| d.detail_layers.get(i)) {
        Some(l) => l.name.clone(),
        None => tr!("Terrain Grass"),
    };
    kit::section(ui, "details_paint", Icon::Brush, &tr!("Paint"), |ui| {
        kit::slider(
            ui,
            &tr!("Target Density"),
            &tr!(
                "The density the brush paints towards: 100 % paints the full density, lower values thin it out. Shift + click erases."
            ),
            &mut state.terrain_tools.detail_target,
            0.0..=1.0,
            1.0,
            "",
            false,
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let target = state.terrain_tools.detail_target;
            if kit::icon_button(
                ui,
                Icon::Fill,
                &tr!("Fill Terrain"),
                &tr!(
                    "Paint {name} at the target density over the whole terrain",
                    name = name
                ),
            )
            .clicked()
            {
                edit(state, path, "Fill Details", |d| {
                    detail_layers::fill(d, which, target)
                });
            }
            if kit::confirm_button(
                ui,
                Icon::Clear,
                &tr!("Clear"),
                &tr!("Erase {name} everywhere on this terrain", name = name),
                &tr!("Erase {name} everywhere on this terrain?", name = name),
                &tr!("Clear"),
                true,
            ) {
                edit(state, path, "Clear Details", |d| {
                    detail_layers::fill(d, which, 0.0)
                });
            }
        });
        ui.weak(tr!(
            "Left click paints {name}, Shift + click erases. [ and ] change the brush size.",
            name = name
        ));
    });
}

/// The big preview at the top of the settings.
fn hero(ui: &mut Ui, thumb: &Thumb, title: &str, subtitle: &str) {
    ui.add_space(kit::SECTION_GAP * 0.5);
    let w = ui.available_width();
    let h = (w * 0.36).clamp(84.0, 150.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    match thumb {
        Thumb::Image(..) => {
            kit::preview_backdrop(ui, rect, 0.0);
            let sq = egui::Rect::from_center_size(rect.center(), egui::vec2(h, h));
            kit::paint_thumb(&child, sq, thumb);
        }
        _ => kit::paint_thumb(&child, rect, thumb),
    }
    let p = ui.painter();
    p.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let font = egui::FontId::proportional(13.0);
    p.text(
        rect.left_top() + egui::vec2(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        crate::i18n::display(title),
        font,
        egui::Color32::WHITE,
    );
    p.text(
        rect.left_top() + egui::vec2(8.0, 24.0),
        egui::Align2::LEFT_TOP,
        subtitle,
        egui::FontId::proportional(11.0),
        egui::Color32::from_white_alpha(190),
    );
}

fn name_row(ui: &mut Ui, name: &mut String) -> bool {
    kit::row(
        ui,
        &tr!("Name"),
        &tr!("Shown on the card and in the brush hints"),
        None,
        |ui| ui.text_edit_singleline(name).changed(),
    )
    .0
}

/// Colour of a species from its dry mix and brightness (the preview's mean colour).
fn species_color(s: &DetailSettings, dryness: f32, tint: f32) -> egui::Color32 {
    let mix = |a: f32, b: f32| a + (b - a) * dryness;
    let c = [
        mix(s.healthy_color[0], s.dry_color[0]),
        mix(s.healthy_color[1], s.dry_color[1]),
        mix(s.healthy_color[2], s.dry_color[2]),
    ];
    let k = 1.0 + 0.5 * tint;
    kit::color32(c.map(|v| v * k))
}

fn species_settings(
    ui: &mut Ui,
    state: &mut EditorState,
    d: &TerrainData,
    l: &mut DetailLayer,
) -> bool {
    let def = DetailLayer::default();
    let thumb = tuft_thumb(ui, state, &TuftParams::species(l, &d.details).patch());
    hero(ui, &thumb, &l.name, &tr!("Grass species · live preview"));
    let mut ch = name_row(ui, &mut l.name);
    kit::section(
        ui,
        "species_shape",
        Icon::TerrainGrass,
        &tr!("Shape"),
        |ui| {
            ch |= kit::range(
                ui,
                &tr!("Height"),
                &tr!("Shortest and tallest blades (metres)"),
                &mut l.height,
                0.02..=3.0,
                def.height,
                " m",
                0.01,
            );
            ch |= kit::slider(
                ui,
                &tr!("Blade Width"),
                &tr!("Width of one blade"),
                &mut l.width,
                0.005..=0.2,
                def.width,
                " m",
                false,
            );
            ch |= kit::slider(
                ui,
                &tr!("Bend"),
                &tr!("How far blades arch over (0 = upright)"),
                &mut l.bend,
                0.0..=1.5,
                def.bend,
                "",
                false,
            );
            ch |= kit::slider(
                ui,
                &tr!("Seed Heads"),
                &tr!("Share of stems that carry a seed head"),
                &mut l.seed_heads,
                0.0..=1.0,
                def.seed_heads,
                "",
                false,
            );
        },
    );
    kit::section(
        ui,
        "species_colour",
        Icon::TerrainPaint,
        &tr!("Colour"),
        |ui| {
            let s = &d.details;
            kit::row(
                ui,
                &tr!("Terrain Colours"),
                &tr!(
                    "The healthy and dry colours this species mixes between (shared with the terrain grass: change them on the Terrain Grass card)"
                ),
                None,
                |ui| {
                    for (c, tip) in [(s.healthy_color, tr!("Healthy")), (s.dry_color, tr!("Dry"))] {
                        let (r, resp) =
                            ui.allocate_exact_size(egui::vec2(30.0, 16.0), egui::Sense::hover());
                        ui.painter().rect(
                            r,
                            3.0,
                            kit::color32(c),
                            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120)),
                            egui::StrokeKind::Outside,
                        );
                        resp.on_hover_text(tip);
                    }
                },
            );
            let stops: Vec<egui::Color32> = (0..=4)
                .map(|k| species_color(s, k as f32 / 4.0, l.tint))
                .collect();
            ch |= kit::gradient(
                ui,
                &tr!("Healthy ↔ Dry"),
                &tr!("Between the terrain grass's healthy and dry colours"),
                &mut l.dryness,
                0.0..=1.0,
                def.dryness,
                &stops,
            );
            let stops: Vec<egui::Color32> = (0..=4)
                .map(|k| species_color(s, l.dryness, -1.0 + k as f32 * 0.5))
                .collect();
            ch |= kit::gradient(
                ui,
                &tr!("Brightness"),
                &tr!("Darker or lighter than the mixed colour"),
                &mut l.tint,
                -1.0..=1.0,
                def.tint,
                &stops,
            );
        },
    );
    kit::section(ui, "species_density", Icon::Fill, &tr!("Density"), |ui| {
        ch |= kit::slider(
            ui,
            &tr!("Density"),
            &tr!("Blades per m² where it is painted at full density"),
            &mut l.density,
            0.0..=MAX_DENSITY,
            def.density,
            " /m²",
            true,
        );
    });
    ch
}

fn mesh_settings(ui: &mut Ui, state: &mut EditorState, l: &mut DetailLayer) -> bool {
    let def = DetailLayer::mesh("", MeshRef::Primitive(PrimitiveMesh::Sphere), None);
    let thumb = mesh_thumb(state, &l.mesh, l.material.as_deref());
    hero(ui, &thumb, &l.name, &tr!("Detail mesh"));
    let mut ch = name_row(ui, &mut l.name);
    kit::section(ui, "mesh_source", Icon::TerrainPlace, &tr!("Mesh"), |ui| {
        let choices = crate::panels::mesh_choices(state);
        let current = misty_engine::models::display_name(&l.mesh);
        let (r, _) = kit::row(
            ui,
            &tr!("Mesh"),
            &tr!("What to place (drop a model from the Project window here)"),
            None,
            |ui| {
                egui::ComboBox::from_id_salt("terrain_detail_mesh")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        let mut c = false;
                        for (i, (r, label)) in choices.iter().enumerate() {
                            if i == PrimitiveMesh::ALL.len() {
                                ui.separator();
                            }
                            c |= ui.selectable_value(&mut l.mesh, r.clone(), label).changed();
                        }
                        c
                    })
            },
        );
        ch |= r.inner.unwrap_or(false);
        if let Some(p) = r
            .response
            .dnd_release_payload::<crate::panels::AssetPayload>()
            && misty_engine::models::is_model(&p.0)
        {
            l.mesh = MeshRef::asset(&p.0, 0);
            ch = true;
        }
        let materials: Vec<String> = state.assets.materials.keys().cloned().collect();
        let shown = l
            .material
            .as_deref()
            .map(stem)
            .unwrap_or_else(|| "Default-Material".into());
        let (r, _) = kit::row(
            ui,
            &tr!("Material"),
            &tr!("Material every instance shares (drop a material here)"),
            None,
            |ui| {
                egui::ComboBox::from_id_salt("terrain_detail_material")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        let mut c = ui
                            .selectable_value(&mut l.material, None, "Default-Material")
                            .changed();
                        for m in &materials {
                            c |= ui
                                .selectable_value(&mut l.material, Some(m.clone()), stem(m))
                                .on_hover_text(m)
                                .changed();
                        }
                        c
                    })
            },
        );
        ch |= r.inner.unwrap_or(false);
        if let Some(p) = r
            .response
            .dnd_release_payload::<crate::panels::AssetPayload>()
            && p.0.ends_with(".mat")
        {
            l.material = Some(p.0.clone());
            ch = true;
        }
    });
    kit::section(ui, "mesh_placement", Icon::Fill, &tr!("Placement"), |ui| {
        ch |= kit::slider(
            ui,
            &tr!("Density"),
            &tr!("Meshes per m² where it is painted at full density"),
            &mut l.density,
            0.001..=MAX_MESH_DENSITY,
            def.density,
            " /m²",
            true,
        );
        ch |= kit::range(
            ui,
            &tr!("Scale"),
            &tr!("Random uniform scale of each mesh"),
            &mut l.scale,
            0.05..=10.0,
            def.scale,
            "",
            0.01,
        );
        ch |= kit::check(
            ui,
            &tr!("Random Rotation"),
            &tr!("Turn every mesh at random about its up axis"),
            &mut l.random_rotation,
            def.random_rotation,
        );
        ch |= kit::check(
            ui,
            &tr!("Align to Normal"),
            &tr!("Tilt with the ground instead of standing upright"),
            &mut l.align_to_normal,
            def.align_to_normal,
        );
    });
    kit::section(
        ui,
        "mesh_rendering",
        Icon::TerrainSettings,
        &tr!("Rendering"),
        |ui| {
            ch |= kit::slider(
                ui,
                &tr!("Draw Distance"),
                &tr!("Meshes are drawn up to about this far (smaller ones stop sooner)"),
                &mut l.draw_distance,
                5.0..=500.0,
                def.draw_distance,
                " m",
                false,
            );
            ch |= kit::check(
                ui,
                &tr!("Cast Shadows"),
                &tr!("Meshes cast shadows"),
                &mut l.cast_shadows,
                def.cast_shadows,
            );
        },
    );
    ch
}

/// The terrain grass's look (its rules, wind and patches stay under Paint Texture ▸ Grass &
/// Details).
fn terrain_grass_settings(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let def = DetailSettings::default();
    let mut s = d.details.clone();
    let thumb = tuft_thumb(ui, state, &TuftParams::terrain_grass(&s).patch());
    hero(
        ui,
        &thumb,
        &tr!("Terrain Grass"),
        &tr!("The terrain's own grass · live preview"),
    );
    let mut ch = false;
    let mut regenerate = false;
    kit::row(
        ui,
        &tr!("Enabled"),
        &tr!("Draw the terrain grass (turning it on fills it from its rules)"),
        None,
        |ui| {
            regenerate = ui.checkbox(&mut s.enabled, "").changed();
        },
    );
    kit::section(ui, "grass_shape", Icon::TerrainGrass, &tr!("Shape"), |ui| {
        ch |= kit::range(
            ui,
            &tr!("Height"),
            &tr!("Shortest and tallest stems of the turf (metres)"),
            &mut s.height,
            0.05..=2.5,
            def.height,
            " m",
            0.01,
        );
        ch |= kit::slider(
            ui,
            &tr!("Blade Width"),
            &tr!("Width of one blade"),
            &mut s.width,
            0.01..=0.3,
            def.width,
            " m",
            false,
        );
        ch |= kit::slider(
            ui,
            &tr!("Bend"),
            &tr!("How far blades arch over (0 = upright)"),
            &mut s.bend,
            0.0..=1.5,
            def.bend,
            "",
            false,
        );
        ch |= kit::slider(
            ui,
            &tr!("Seed Heads"),
            &tr!("Share of stems that carry a seed head"),
            &mut s.seed_heads,
            0.0..=1.0,
            def.seed_heads,
            "",
            false,
        );
    });
    kit::section(
        ui,
        "grass_colour",
        Icon::TerrainPaint,
        &tr!("Colour"),
        |ui| {
            ch |= kit::color(
                ui,
                &tr!("Healthy"),
                &tr!("Colour of healthy blades (grass species mix from it too)"),
                &mut s.healthy_color,
                def.healthy_color,
            );
            ch |= kit::color(
                ui,
                &tr!("Dry"),
                &tr!("Colour of dry blades and dry patches (grass species mix towards it)"),
                &mut s.dry_color,
                def.dry_color,
            );
            ch |= kit::color(
                ui,
                &tr!("Seed Heads"),
                &tr!("Colour of the seed heads"),
                &mut s.seed_color,
                def.seed_color,
            );
            let tips = [kit::color32(s.healthy_color), kit::color32(s.dry_color)];
            ch |= kit::gradient(
                ui,
                &tr!("Dry Tips"),
                &tr!("How much blade tips dry out towards the dry colour"),
                &mut s.tip_dryness,
                0.0..=1.0,
                def.tip_dryness,
                &tips,
            );
            ch |= kit::slider(
                ui,
                &tr!("Colour Jitter"),
                &tr!("Hue and brightness differences between tufts and blades"),
                &mut s.color_variation,
                0.0..=1.0,
                def.color_variation,
                "",
                false,
            );
        },
    );
    kit::section(ui, "grass_density", Icon::Fill, &tr!("Density"), |ui| {
        ch |= kit::slider(
            ui,
            &tr!("Density"),
            &tr!("Blades per m² where it grows at full density"),
            &mut s.density,
            0.0..=details::MAX_DENSITY,
            def.density,
            " /m²",
            true,
        );
        ch |= kit::slider(
            ui,
            &tr!("Draw Distance"),
            &tr!("Blades fade out towards this distance"),
            &mut s.draw_distance,
            10.0..=400.0,
            def.draw_distance,
            " m",
            false,
        );
    });
    ui.horizontal(|ui| {
        if kit::icon_button(
            ui,
            Icon::TerrainPaint,
            &tr!("Rules, Wind and Patches…"),
            &tr!("Where it grows, wind and lush / dry patches: Paint Texture ▸ Grass & Details"),
        )
        .clicked()
        {
            state.terrain_tools.tool = terrain_tools::TerrainTool::Paint;
        }
    });
    if regenerate {
        terrain_tools::begin_edit(state, path, "Grass & Details");
        store::modify(path, |d| {
            d.details = s;
            details::generate(d);
        });
    } else if ch {
        edit_settings(state, path, "Grass & Details", |d| d.details = s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_short_on_badges() {
        assert_eq!(short_count(950), "950");
        assert_eq!(short_count(12_345), "12k");
        assert_eq!(short_count(1_500), "1.5k");
        assert_eq!(short_count(3_400_000), "3.4M");
    }

    #[test]
    fn models_and_prefabs_can_be_dropped() {
        assert!(accepts("Assets/Rocks/Pebble.glb"));
        assert!(accepts("Assets/Rock.prefab"));
        assert!(!accepts("Assets/Grass.png"));
        assert_eq!(stem("Assets/Rocks/Pebble.glb#2"), "Pebble");
    }
}
