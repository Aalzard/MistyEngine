//! The Terrain Inspector's Place Objects tab: a palette of prefabs (rocks, bushes, props) shown
//! as rendered thumbnails, scattered with the brush as scene objects under the terrain's
//! "Placed Objects" child (left click places, Shift erases), with spacing, random scale and
//! turn, align-to-normal and slope and height limits (`terrain::placement`). Prefabs dragged
//! from the Project window onto the palette are added to it. Each stroke is one undo step.

use std::hash::{Hash, Hasher};

use egui::Ui;
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::terrain::placement::{self, ObjectPlacement};
use misty_engine::terrain::{TerrainData, store};

use crate::branding::icons::Icon;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::terrain_kit::{self as kit, AddCard, Card, CardEvent, Thumb};
use crate::terrain_previews::MeshJob;
use crate::tutorials::targets;

fn stem(p: &str) -> String {
    std::path::Path::new(p)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every prefab asset of the project (the GUID database lists them).
fn prefab_assets() -> Vec<String> {
    let mut out = misty_engine::asset_db::with(|db| {
        db.paths()
            .filter(|p| p.ends_with(".prefab"))
            .map(str::to_owned)
            .collect::<Vec<String>>()
    })
    .unwrap_or_default();
    out.sort();
    out
}

fn is_prefab(rel: &str) -> bool {
    rel.ends_with(".prefab")
}

/// The prefab rendered offscreen (re-rendered when the file changes), an icon until then.
pub(crate) fn prefab_thumb(state: &mut EditorState, rel: &str) -> Thumb {
    let abs = state.assets.absolute(rel);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ("prefab", rel).hash(&mut h);
    misty_engine::file_watch::cached_modified(&abs).hash(&mut h);
    let key = h.finish();
    let mut job = || {
        let prefab = state.assets.prefab(rel).ok()?;
        let (scene, _) = prefab.to_scene("Thumbnail");
        let draws = misty_engine::render_scene::mesh_draws(&scene, &state.assets, |_| false);
        MeshJob::new(draws)
    };
    // The job needs the asset cache, the cache the previews: make the job first if needed.
    let tex = if state.project.thumbnails.terrain.has_mesh_thumb(key) {
        state.project.thumbnails.terrain.mesh_thumb(key, || None)
    } else {
        let made = job();
        state.project.thumbnails.terrain.mesh_thumb(key, || made)
    };
    match tex {
        Some(id) => Thumb::Image(id, None),
        None => Thumb::Icon {
            icon: Icon::TerrainPlace,
            pending: true,
        },
    }
}

pub fn place_tab(ui: &mut Ui, state: &mut EditorState, e: Entity, d: &TerrainData) {
    // An empty palette starts with the prefabs already placed on this terrain.
    if state.terrain_tools.prefabs.is_empty() {
        let scene = &state.scene;
        let mut found: Vec<String> = (scene.children(e).into_iter())
            .filter(|c| scene.name(*c) == placement::HOLDER)
            .flat_map(|h| scene.children(h))
            .filter_map(|c| scene.prefab_link(c))
            .collect();
        found.sort();
        found.dedup();
        state.terrain_tools.prefabs = found;
    }
    {
        let tools = &mut state.terrain_tools;
        if tools.prefab >= tools.prefabs.len() {
            tools.prefab = tools.prefabs.len().saturating_sub(1);
        }
    }
    let placed = placed_counts(&state.scene, e);
    kit::section(
        ui,
        "place_palette",
        Icon::TerrainPlace,
        &tr!("Prefabs"),
        |ui| {
            let list = state.terrain_tools.prefabs.clone();
            let cards: Vec<Card> = list
                .iter()
                .map(|p| {
                    let count = placed.get(p).copied().unwrap_or(0);
                    Card {
                        name: stem(p),
                        thumb: prefab_thumb(state, p),
                        badge: Some(crate::terrain_details_ui::short_count(count)),
                        tip: format!(
                            "{p}\n{}",
                            tr!(
                                "{count} placed. Double-click to open the prefab, drag to reorder.",
                                count = count
                            )
                        ),
                        asset: Some(p.clone()),
                        can_duplicate: false,
                        can_remove: true,
                        dimmed: false,
                    }
                })
                .collect();
            let selected = (!list.is_empty()).then_some(state.terrain_tools.prefab);
            let out = kit::palette(
                ui,
                "prefabs",
                &cards,
                selected,
                Some(AddCard {
                    label: &tr!("Add Prefab"),
                    tip: &tr!(
                        "Add a prefab to place (or drop prefabs from the Project window here)"
                    ),
                    enabled: true,
                    target: "+ Add Prefab",
                }),
                true,
                &is_prefab,
                &tr!("No prefabs yet: add a rock, bush or prop prefab to place it."),
                targets::header,
            );
            if let Some(add) = &out.add {
                egui::Popup::menu(add).show(|ui| {
                    ui.set_min_width(180.0);
                    let assets = prefab_assets();
                    if assets.is_empty() {
                        ui.weak(tr!("No prefabs in the project"));
                    }
                    for a in &assets {
                        if ui
                            .selectable_label(
                                false,
                                format!("{} {}", crate::state::PREFAB_ICON, stem(a)),
                            )
                            .on_hover_text(a)
                            .clicked()
                        {
                            add_prefab(state, a.clone());
                            ui.close();
                        }
                    }
                });
            }
            let tools_len = state.terrain_tools.prefabs.len();
            for ev in out.events {
                let tools = &mut state.terrain_tools;
                match ev {
                    CardEvent::Select(i) => tools.prefab = i,
                    CardEvent::Remove(i) if i < tools_len => {
                        tools.prefabs.remove(i);
                        if tools.prefab >= i {
                            tools.prefab = tools.prefab.saturating_sub(1);
                        }
                    }
                    CardEvent::Move { from, to } if from < tools_len && to < tools_len => {
                        let p = tools.prefabs.remove(from);
                        tools.prefabs.insert(to, p);
                        tools.prefab = to;
                    }
                    CardEvent::Locate(i) => {
                        if let Some(p) = list.get(i) {
                            crate::terrain_details_ui::locate_asset(state, p);
                        }
                    }
                    CardEvent::Open(i) => {
                        if let Some(p) = list.get(i) {
                            crate::prefabs::select_asset(state, p);
                        }
                    }
                    CardEvent::Drop(rel) => add_prefab(state, rel),
                    _ => {}
                }
            }
        },
    );

    let max_h = d.size.y;
    kit::section(ui, "place_brush", Icon::Brush, &tr!("Brush"), |ui| {
        let tools = &mut state.terrain_tools;
        kit::slider(
            ui,
            &tr!("Brush Size"),
            &tr!("Brush diameter ([ and ] change it)"),
            &mut tools.size,
            1.0..=200.0,
            25.0,
            " m",
            true,
        );
        let def = ObjectPlacement::default();
        let p: &mut ObjectPlacement = &mut tools.object_placement;
        kit::slider(
            ui,
            &tr!("Density"),
            &tr!("Objects per 100 m² the brush fills up to"),
            &mut p.density,
            0.05..=50.0,
            def.density,
            "",
            true,
        );
        kit::slider(
            ui,
            &tr!("Spacing"),
            &tr!("Closest two objects stand, centre to centre"),
            &mut p.spacing,
            0.1..=40.0,
            def.spacing,
            " m",
            false,
        );
        ui.weak(tr!(
            "Left click places the selected prefab, Shift + click erases."
        ));
    });
    kit::section(
        ui,
        "place_placement",
        Icon::TerrainPlace,
        &tr!("New Objects"),
        |ui| {
            let def = ObjectPlacement::default();
            let p: &mut ObjectPlacement = &mut state.terrain_tools.object_placement;
            kit::range(
                ui,
                &tr!("Scale"),
                &tr!("Random uniform scale of new objects"),
                &mut p.scale,
                0.05..=10.0,
                def.scale,
                "×",
                0.01,
            );
            kit::check(
                ui,
                &tr!("Random Rotation"),
                &tr!("Turn every new object at random about its up axis"),
                &mut p.random_rotation,
                def.random_rotation,
            );
            kit::check(
                ui,
                &tr!("Align to Normal"),
                &tr!("Tilt with the ground instead of standing upright"),
                &mut p.align_to_normal,
                def.align_to_normal,
            );
            kit::range(
                ui,
                &tr!("Slope"),
                &tr!("Ground steepness objects go on"),
                &mut p.slope,
                0.0..=90.0,
                def.slope,
                "°",
                0.5,
            );
            p.height[1] = p.height[1].min(max_h);
            kit::range(
                ui,
                &tr!("Height"),
                &tr!("Terrain heights objects go at (above the terrain's base)"),
                &mut p.height,
                0.0..=max_h,
                [0.0, max_h],
                " m",
                0.1,
            );
        },
    );
    let count = placement::placed(&state.scene, e).len();
    kit::section(ui, "place_erase", Icon::Clear, &tr!("Erase"), |ui| {
        kit::check(
            ui,
            &tr!("Erase Selected Only"),
            &tr!("Shift + click removes only instances of the selected prefab"),
            &mut state.terrain_tools.erase_selected_only,
            false,
        );
        ui.horizontal(|ui| {
            if kit::confirm_button(
                ui,
                Icon::Clear,
                &tr!("Clear All"),
                &tr!("Delete every placed object of this terrain"),
                &tr!(
                    "Delete all {count} placed objects of this terrain?",
                    count = count
                ),
                &tr!("Delete"),
                count > 0,
            ) {
                state.record("Clear Placed Objects");
                if let Some(h) = placement::holder(&mut state.scene, e, false) {
                    state.scene.despawn(h);
                }
            }
            ui.weak(tr!(
                "{count} objects placed on this terrain.",
                count = count
            ));
        });
    });
}

/// Objects placed on terrain `e` per prefab.
fn placed_counts(
    scene: &misty_engine::scene::Scene,
    e: Entity,
) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    for h in scene.children(e) {
        if scene.name(h) != placement::HOLDER {
            continue;
        }
        for c in scene.children(h) {
            if let Some(p) = scene.prefab_link(c) {
                *out.entry(p).or_insert(0) += 1;
            }
        }
    }
    out
}

fn add_prefab(state: &mut EditorState, rel: String) {
    let tools = &mut state.terrain_tools;
    match tools.prefabs.iter().position(|p| *p == rel) {
        Some(i) => tools.prefab = i,
        None => {
            tools.prefabs.push(rel);
            tools.prefab = tools.prefabs.len() - 1;
        }
    }
}

/// One brush step of Place Objects at terrain-local `at` (Shift = erase).
pub fn apply(state: &mut EditorState, e: Entity, path: &str, at: Vec2, erase: bool) {
    let Some((d, _)) = store::get(path) else {
        return;
    };
    let t = &state.terrain_tools;
    let radius = t.size * 0.5;
    let selected = t.prefabs.get(t.prefab).cloned();
    if erase {
        let only = selected.as_deref().filter(|_| t.erase_selected_only);
        let near = placement::placed(&state.scene, e)
            .iter()
            .any(|q| q.distance(at) <= radius);
        if !near {
            return;
        }
        if !state.terrain_tools.place_recorded {
            state.record("Erase Placed Objects");
            state.terrain_tools.place_recorded = true;
        }
        placement::erase(&mut state.scene, e, at, radius, only);
        return;
    }
    let Some(rel) = selected else {
        return;
    };
    let prefab = match state.assets.prefab(&rel) {
        Ok(p) => p,
        Err(err) => {
            state.console.warn(format!("Could not place {rel}: {err}"));
            state.terrain_tools.prefabs.retain(|p| *p != rel);
            return;
        }
    };
    let p = state.terrain_tools.object_placement;
    let step = state.terrain_tools.place_step;
    state.terrain_tools.place_step = step.wrapping_add(1);
    let existing = placement::placed(&state.scene, e);
    if placement::spots(&d, at, radius, &existing, &p, step).is_empty() {
        return;
    }
    if !state.terrain_tools.place_recorded {
        state.record("Place Objects");
        state.terrain_tools.place_recorded = true;
    }
    placement::place(&mut state.scene, e, &d, &prefab, &rel, at, radius, &p, step);
}
