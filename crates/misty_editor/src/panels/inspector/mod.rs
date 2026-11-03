//! Inspector panel: GameObject header and one foldout per component.
//! With several objects selected, edits apply to all of them (only shared components are shown).

mod editors;
mod slots;

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::registry;

use crate::actions::Action;
use crate::state::{EditorState, object_icon};
use crate::theme;

pub const TAGS: [&str; 7] = [
    "Untagged",
    "Respawn",
    "Finish",
    "EditorOnly",
    "MainCamera",
    "Player",
    "GameController",
];
pub const LAYERS: [&str; 6] = [
    "Default",
    "TransparentFX",
    "Ignore Raycast",
    "Water",
    "UI",
    "Player",
];

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    lock_row(ui, state);

    // A locked Inspector keeps showing the objects it was locked on.
    if !state.inspector_locked {
        state.inspector_targets = state.selection.list().to_vec();
        if let Some(a) = state.selection.active() {
            // Active object first: its values are the ones displayed.
            state.inspector_targets.retain(|e| *e != a);
            state.inspector_targets.insert(0, a);
        }
    }
    let scene = &state.scene;
    state.inspector_targets.retain(|e| scene.contains(*e));
    let targets = state.inspector_targets.clone();
    let Some(&e) = targets.first() else {
        // Nothing in the scene selected: show the selected asset, if any.
        if let Some(rel) = state.inspector_asset.clone() {
            asset_inspector(ui, state, &rel);
        }
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            header(ui, state, &targets);
            if targets.len() == 1 {
                prefab_row(ui, state, e);
            }
            if targets.len() > 1 {
                ui.label(
                    RichText::new(format!("{} objects selected", targets.len()))
                        .color(theme::TEXT_DIM),
                );
            }
            ui.separator();
            for name in shared_components(state, &targets) {
                if name == "Scripts" {
                    editors::script_sections(ui, state, e);
                } else {
                    component_section(ui, state, &targets, &name);
                }
            }
            material_previews(ui, state, e);
            ui.add_space(8.0);
            editors::add_component_button(ui, state, e);
        });
}

/// Components present on every target, in the active object's order.
fn shared_components(state: &EditorState, targets: &[Entity]) -> Vec<String> {
    let names = state.scene.component_names(targets[0]);
    names
        .into_iter()
        .filter(|n| {
            registry::find(n)
                .is_some_and(|info| targets.iter().all(|t| (info.has)(&state.scene.world, *t)))
        })
        .collect()
}

fn lock_row(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lock = if state.inspector_locked {
                "🔒"
            } else {
                "🔓"
            };
            if ui
                .add(egui::Button::new(lock).frame(false))
                .on_hover_text("Lock Inspector")
                .clicked()
            {
                state.inspector_locked = !state.inspector_locked;
            }
        });
    });
}

/// Icon, active toggle, name, Static; then Tag and Layer. Name edits only the active object.
fn header(ui: &mut Ui, state: &mut EditorState, targets: &[Entity]) {
    let e = targets[0];
    let icon = object_icon(&state.scene, e);
    let before = (*state.scene.game_object(e)).clone();
    let mut go = before.clone();
    let multi = targets.len() > 1;
    let mut changed = false;
    ui.horizontal(|ui| {
        let icon = RichText::new(icon).size(22.0);
        let icon = if state.scene.prefab_link(e).is_some() {
            icon.color(theme::PREFAB)
        } else {
            icon
        };
        ui.label(icon);
        changed |= ui.checkbox(&mut go.active, "").changed();
        let name_w = ui.available_width() - 70.0;
        if multi {
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut "—".to_owned()).desired_width(name_w),
            );
        } else {
            changed |= ui
                .add(egui::TextEdit::singleline(&mut go.name).desired_width(name_w))
                .changed();
        }
        changed |= ui.checkbox(&mut go.is_static, "Static").changed();
    });
    ui.horizontal(|ui| {
        ui.label("Tag");
        egui::ComboBox::from_id_salt("insp_tag")
            .selected_text(go.tag.as_str())
            .width(110.0)
            .show_ui(ui, |ui| {
                for t in TAGS {
                    if ui.selectable_label(go.tag == t, t).clicked() {
                        go.tag = t.to_owned();
                        changed = true;
                    }
                }
            });
        ui.label("Layer");
        let layer_name = LAYERS.get(go.layer as usize).copied().unwrap_or("Default");
        egui::ComboBox::from_id_salt("insp_layer")
            .selected_text(layer_name)
            .width(110.0)
            .show_ui(ui, |ui| {
                for (i, l) in LAYERS.iter().enumerate() {
                    changed |= ui.selectable_value(&mut go.layer, i as u8, *l).changed();
                }
            });
    });
    if !changed {
        return;
    }
    for &t in targets {
        let mut g = state.scene.game_object_mut(t);
        if go.active != before.active {
            g.active = go.active;
        }
        if go.is_static != before.is_static {
            g.is_static = go.is_static;
        }
        if go.tag != before.tag {
            g.tag = go.tag.clone();
        }
        if go.layer != before.layer {
            g.layer = go.layer;
        }
        if t == e && go.name != before.name {
            g.name = go.name.clone();
        }
    }
    state.mark_edited("GameObject");
}

/// "Prefab" row on an instance root: Select, Revert, Apply All (the last two only with overrides).
fn prefab_row(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let Some(rel) = state.scene.prefab_link(e) else {
        return;
    };
    let overrides = match state.assets.prefab(&rel) {
        Ok(p) => Some(state.scene.has_prefab_overrides(e, &p)),
        Err(_) => None,
    };
    ui.horizontal(|ui| {
        ui.label("Prefab");
        let name = std::path::Path::new(&rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = match overrides {
            Some(_) => {
                RichText::new(format!("{} {name}", crate::state::PREFAB_ICON)).color(theme::PREFAB)
            }
            None => RichText::new(format!("Missing: {name}")).color(theme::AXIS_X),
        };
        ui.label(text).on_hover_text(&rel);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let changed = overrides == Some(true);
            if ui
                .add_enabled(changed, egui::Button::new("Apply All"))
                .on_hover_text("Write this instance's changes to the prefab asset")
                .clicked()
            {
                state.push(Action::ApplyPrefab(e));
            }
            if ui
                .add_enabled(changed, egui::Button::new("Revert"))
                .on_hover_text("Discard this instance's changes")
                .clicked()
            {
                state.push(Action::RevertPrefab(e));
            }
            if ui
                .add_enabled(overrides.is_some(), egui::Button::new("Select"))
                .clicked()
            {
                state.push(Action::SelectPrefabAsset(e));
            }
        });
    });
}

/// Foldout header with enable toggle and ⋮ menu, then the component's fields.
fn component_section(ui: &mut Ui, state: &mut EditorState, targets: &[Entity], name: &str) {
    let e = targets[0];
    let id = ui.make_persistent_id(("component", name));
    let removable = registry::find(name).is_some_and(|i| i.removable);
    let open_state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);

    let header = open_state.show_header(ui, |ui| {
        if let Some(enabled) = editors::enabled_flag(state, e, name) {
            let mut on = enabled;
            if ui.checkbox(&mut on, "").changed() {
                for &t in targets {
                    editors::set_enabled(state, t, name, on);
                }
                state.mark_edited(name);
            }
        }
        ui.label(RichText::new(format!("{} {name}", editors::icon(name))).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⋮", |ui| component_menu(ui, state, e, name, removable));
        });
    });
    header.body(|ui| {
        let before = to_json(state, e, name);
        if editors::edit(ui, state, targets, name) {
            if name != "Transform" {
                propagate(state, targets, name, before);
            }
            state.mark_edited(name);
        }
    });
    ui.separator();
}

fn component_menu(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str, removable: bool) {
    let n = name.to_owned();
    let act = |ui: &mut Ui, state: &mut EditorState, label: &str, enabled: bool, action: Action| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            state.push(action);
            ui.close();
        }
    };
    act(
        ui,
        state,
        "Reset",
        true,
        Action::ResetComponent {
            entity: e,
            name: n.clone(),
        },
    );
    ui.separator();
    act(
        ui,
        state,
        "Remove Component",
        removable,
        Action::RemoveComponent {
            entity: e,
            name: n.clone(),
        },
    );
    act(
        ui,
        state,
        "Move Up",
        removable,
        Action::MoveComponent {
            entity: e,
            name: n.clone(),
            dir: -1,
        },
    );
    act(
        ui,
        state,
        "Move Down",
        removable,
        Action::MoveComponent {
            entity: e,
            name: n.clone(),
            dir: 1,
        },
    );
    ui.separator();
    if ui.button("Copy Component").clicked() {
        state.component_clipboard = to_json(state, e, name).map(|v| (n.clone(), v));
        ui.close();
    }
    let can_paste = state
        .component_clipboard
        .as_ref()
        .is_some_and(|(c, _)| c == name);
    act(
        ui,
        state,
        "Paste Component Values",
        can_paste,
        Action::PasteComponentValues { entity: e, name: n },
    );
}

fn to_json(state: &EditorState, e: Entity, name: &str) -> Option<serde_json::Value> {
    registry::find(name).and_then(|info| (info.to_json)(&state.scene.world, e))
}

/// Copy the fields that changed on the active object to the other targets.
fn propagate(
    state: &mut EditorState,
    targets: &[Entity],
    name: &str,
    before: Option<serde_json::Value>,
) {
    let Some(info) = registry::find(name) else {
        return;
    };
    let (Some(serde_json::Value::Object(before)), Some(serde_json::Value::Object(after))) =
        (before, to_json(state, targets[0], name))
    else {
        return;
    };
    let changed: Vec<(&String, &serde_json::Value)> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .collect();
    for &t in &targets[1..] {
        let Some(serde_json::Value::Object(mut obj)) = (info.to_json)(&state.scene.world, t) else {
            continue;
        };
        for (k, v) in &changed {
            obj.insert((*k).clone(), (*v).clone());
        }
        let _ = (info.from_json)(&mut state.scene.world, t, serde_json::Value::Object(obj));
    }
}

/// Inline editors for the materials the object renders with (like the material section under the components).
fn material_previews(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let Some(rel) = state
        .scene
        .world
        .get::<&misty_engine::scene::MeshRenderer>(e)
        .ok()
        .and_then(|r| r.material.clone())
    else {
        return;
    };
    let name = crate::assets::AssetDatabase::material_name(Some(&rel));
    egui::CollapsingHeader::new(RichText::new(format!("◍ {name} (Material)")).strong())
        .id_salt(("inline_material", &rel))
        .default_open(true)
        .show(ui, |ui| editors::material_asset(ui, state, &rel));
    ui.separator();
}

/// Inspector for a Project asset: material editor, or a read-only prefab summary.
fn asset_inspector(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    if rel.ends_with(".prefab") {
        prefab_asset(ui, state, rel);
        return;
    }
    if misty_engine::audio::is_clip(rel) {
        audio_clip_asset(ui, state, rel);
        return;
    }
    let name = crate::assets::AssetDatabase::material_name(Some(rel));
    ui.horizontal(|ui| {
        ui.label(RichText::new("◍").size(22.0));
        ui.label(RichText::new(format!("{name} (Material)")).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    ui.separator();
    if rel.ends_with(".mat") {
        editors::material_asset(ui, state, rel);
    } else {
        ui.weak("No import settings for this asset.");
    }
}

/// Audio clip: file info and a ▶ / ■ preview player.
fn audio_clip_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::state::AUDIO_ICON).size(22.0));
        ui.label(RichText::new(format!("{} (Audio Clip)", editors::clip_name(rel))).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    ui.separator();
    let abs = state.assets.absolute(rel);
    if let Some(info) = clip_info(&abs) {
        ui.label(info);
    }
    let playing = state.clip_preview.as_ref().is_some_and(|a| a.any_playing());
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!playing, egui::Button::new("▶ Play"))
            .on_hover_text("Preview this clip")
            .clicked()
        {
            let root = state.project.root.clone();
            let preview = state
                .clip_preview
                .get_or_insert_with(|| misty_engine::audio::Audio::with_device(root));
            preview.stop_all();
            preview.play_clip(rel, 1.0);
            for w in preview.take_logs() {
                state.console.warn(w);
            }
        }
        if ui
            .add_enabled(playing, egui::Button::new("■ Stop"))
            .clicked()
            && let Some(p) = state.clip_preview.as_mut()
        {
            p.stop_all();
        }
    });
    if playing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// "22050 Hz, Mono, 0.300 s" for PCM WAV files; file size otherwise.
fn clip_info(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
        let rate = u32::from_le_bytes(bytes[24..28].try_into().ok()?);
        let byte_rate = u32::from_le_bytes(bytes[28..32].try_into().ok()?).max(1);
        let seconds = (bytes.len() - 44) as f32 / byte_rate as f32;
        let layout = if channels == 1 { "Mono" } else { "Stereo" };
        return Some(format!("{rate} Hz, {layout}, {seconds:.3} s"));
    }
    Some(format!("{:.1} KB", bytes.len() as f32 / 1024.0))
}

/// Prefab asset: its objects as a tree and the root's components.
fn prefab_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let name = crate::assets::AssetDatabase::material_name(Some(rel));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(crate::state::PREFAB_ICON)
                .size(22.0)
                .color(theme::PREFAB),
        );
        ui.label(RichText::new(format!("{name} (Prefab Asset)")).strong());
    });
    ui.label(RichText::new(rel).color(theme::TEXT_DIM).small());
    ui.separator();
    match state.assets.prefab(rel) {
        Ok(prefab) => {
            let outline = prefab.outline();
            ui.label(format!(
                "{} object{}",
                outline.len(),
                if outline.len() == 1 { "" } else { "s" }
            ));
            egui::Frame::new()
                .fill(theme::FIELD_BG)
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (n, depth) in &outline {
                        ui.label(
                            RichText::new(format!("{}{n}", "    ".repeat(*depth)))
                                .color(theme::PREFAB),
                        );
                    }
                });
            ui.add_space(6.0);
            ui.label(RichText::new("Root components").strong());
            for c in prefab.root_components() {
                ui.label(format!("{} {c}", editors::icon(&c)));
            }
            ui.add_space(6.0);
            ui.weak("Drag into the Scene view or Hierarchy to create an instance.");
        }
        Err(err) => {
            ui.colored_label(theme::AXIS_X, err);
        }
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use misty_engine::scene::Camera;

    use super::*;
    use crate::commands;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_insp_test"),
        ))
    }

    fn apply_actions(s: &mut EditorState) {
        for a in std::mem::take(&mut s.actions) {
            commands::apply(s, a);
        }
    }

    #[test]
    fn add_component_popup_adds_light() {
        let mut s = state();
        let cam = s.scene.roots()[0];
        s.selection.set(cam);
        let mut h = Harness::builder()
            .with_size([400.0, 1000.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h.get_by_label("Add Component").click();
        h.run();
        h.get_by_label("Light").click();
        h.run();
        apply_actions(h.state_mut());
        assert_eq!(
            h.state().scene.component_names(cam),
            ["Transform", "Camera", "Audio Listener", "Light"]
        );
        assert!(!h.state().inspector_add_open, "popup closes after choosing");
    }

    /// A project with "Assets/Crate.prefab" and one linked instance (selected).
    fn prefab_state(tag: &str) -> (EditorState, Entity, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("misty_insp_prefab_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let e = s.scene.spawn("Crate", None);
        s.scene.spawn("Lid", Some(e));
        let folder = s.project.assets_dir();
        s.push(Action::CreatePrefab { entity: e, folder });
        apply_actions(&mut s);
        s.selection.set(e);
        (s, e, root)
    }

    fn inspector(s: EditorState) -> Harness<'static, EditorState> {
        let mut h = Harness::builder()
            .with_size([400.0, 700.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h
    }

    #[test]
    fn prefab_row_applies_overrides_to_the_asset() {
        let (s, e, root) = prefab_state("apply");
        let mut h = inspector(s);
        assert!(
            !!h.get_by_label("Apply All").accesskit_node().is_disabled(),
            "nothing to apply without overrides"
        );
        h.state_mut().scene.add(e, Camera::default());
        h.run();
        h.get_by_label("Apply All").click();
        h.run();
        apply_actions(h.state_mut());
        let s = h.state_mut();
        let asset = s.assets.prefab("Assets/Crate.prefab").unwrap();
        assert_eq!(asset.root_components(), ["Transform", "Camera"]);
        assert!(!s.scene.has_prefab_overrides(e, &asset));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn prefab_row_reverts_and_selects_the_asset() {
        let (mut s, e, root) = prefab_state("revert");
        s.scene.spawn("Extra", Some(e));
        let mut h = inspector(s);
        h.get_by_label("Revert").click();
        h.run();
        apply_actions(h.state_mut());
        let reverted = h.state().selection.active().unwrap();
        assert_eq!(h.state().scene.children(reverted).len(), 1, "Extra removed");

        h.run();
        h.get_by_label("Select").click();
        h.run();
        apply_actions(h.state_mut());
        assert_eq!(
            h.state().inspector_asset.as_deref(),
            Some("Assets/Crate.prefab")
        );
        h.run();
        // The Inspector now shows the asset summary.
        h.get_by_label("Crate (Prefab Asset)");
        h.get_by_label("2 objects");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn multi_edit_propagates_only_changed_fields() {
        let mut s = state();
        let a = s.scene.spawn("A", None);
        let b = s.scene.spawn("B", None);
        s.scene.add(a, Camera::default());
        s.scene.add(
            b,
            Camera {
                near: 2.0,
                ..Default::default()
            },
        );
        let before = to_json(&s, a, "Camera");
        s.scene.world.get::<&mut Camera>(a).unwrap().fov = 90.0;
        propagate(&mut s, &[a, b], "Camera", before);
        let cb = s.scene.world.get::<&Camera>(b).unwrap();
        assert_eq!(cb.fov, 90.0);
        assert_eq!(cb.near, 2.0, "unchanged fields keep their own values");
    }

    #[test]
    fn copy_paste_component_values() {
        let mut s = state();
        let (a, b) = (s.scene.spawn("A", None), s.scene.spawn("B", None));
        s.scene.add(
            a,
            Camera {
                fov: 30.0,
                ..Default::default()
            },
        );
        s.scene.add(b, Camera::default());
        s.component_clipboard = to_json(&s, a, "Camera").map(|v| ("Camera".into(), v));
        s.push(Action::PasteComponentValues {
            entity: b,
            name: "Camera".into(),
        });
        apply_actions(&mut s);
        assert_eq!(s.scene.world.get::<&Camera>(b).unwrap().fov, 30.0);
        s.push(Action::Undo);
        apply_actions(&mut s);
        assert_eq!(s.scene.world.get::<&Camera>(b).unwrap().fov, 60.0);
    }

    fn audio_project(tag: &str) -> (EditorState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("misty_insp_audio_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Audio")).unwrap();
        let wav = misty_engine::audio::wav::sine_beep(440.0, 0.5, 22050);
        std::fs::write(root.join("Assets/Audio/Beep.wav"), wav).unwrap();
        (EditorState::new(ProjectBrowser::open(root.clone())), root)
    }

    #[test]
    fn audio_source_editor_picks_a_project_clip() {
        let (mut s, root) = audio_project("source");
        let e = s.scene.spawn("Speaker", None);
        s.scene.add(e, misty_engine::scene::AudioSource::default());
        s.selection.set(e);
        let mut h = inspector(s);
        h.get_by_label("3D Sound Settings");
        h.get_by_label("Spatial Blend");
        h.get_by_value("🔊 None (Audio Clip)").click();
        h.run();
        h.get_by_label("Beep").click();
        h.run();
        let clip = h
            .state()
            .scene
            .world
            .get::<&misty_engine::scene::AudioSource>(e)
            .unwrap()
            .clip
            .clone();
        assert_eq!(clip.as_deref(), Some("Assets/Audio/Beep.wav"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn audio_clip_asset_shows_info_and_preview_buttons() {
        let (mut s, root) = audio_project("clip");
        s.inspector_asset = Some("Assets/Audio/Beep.wav".into());
        let h = inspector(s);
        h.get_by_label("Beep (Audio Clip)");
        h.get_by_label("22050 Hz, Mono, 0.500 s");
        h.get_by_label("▶ Play");
        h.get_by_label("■ Stop");
        std::fs::remove_dir_all(root).ok();
    }
}
