//! Asset Groups window (Window ▸ Asset Management ▸ Asset Groups): the project's addressable
//! assets by group. Drag entries between groups (or assets in from the Project window), edit
//! addresses and labels, set each group's packing options, pick the Play Mode Script, build the
//! bundles (Build ▸ Asset Bundles) and run the duplicate-dependency analysis.
//!
//! Also the "Addressable" row at the top of the asset Inspector ([`addressable_header`]).

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use misty_engine::asset_groups::{
    self, AssetGroupSettings, BundleCompression, BundleLocation, BundleMode, DEFAULT_GROUP,
    Duplicate, PlayModeScript, build,
};

use super::AssetPayload;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field};

/// Window state (the settings file is the source of truth; edits save at once).
#[derive(Default)]
pub struct AssetGroupsWindow {
    pub settings: AssetGroupSettings,
    pub loaded_for: Option<PathBuf>,
    pub selected_group: Option<String>,
    /// Selected entry (asset path).
    pub selected_entry: Option<String>,
    pub search: String,
    /// Last Analyze result.
    pub analysis: Option<Vec<Duplicate>>,
    /// Last build / analyze message (error = true).
    pub status: Option<(bool, String)>,
}

/// An entry dragged between groups.
#[derive(Clone)]
struct EntryPayload(String);

impl AssetGroupsWindow {
    /// Read the settings once per project.
    pub fn ensure_loaded(&mut self, project: &Path) {
        if self.loaded_for.as_deref() != Some(project) {
            self.reload(project);
        }
    }

    pub fn reload(&mut self, project: &Path) {
        self.settings = AssetGroupSettings::load(project).unwrap_or_default();
        self.loaded_for = Some(project.to_path_buf());
    }

    fn save(&mut self, project: &Path) {
        self.settings.sanitize();
        if let Err(e) = self.settings.save(project) {
            self.status = Some((true, format!("Could not save Asset Groups: {e}")));
        }
    }

    /// Mark `rel` addressable (or not) and save.
    pub fn set_addressable(&mut self, project: &Path, rel: &str, on: bool) {
        let guid = misty_engine::asset_db::guid_of(rel).map(|g| g.to_string());
        if self.settings.set_addressable(rel, on, guid) {
            self.save(project);
        }
    }

    /// Make `rel` addressable in `group` (moving it there if it already is).
    pub fn add_to_group(&mut self, project: &Path, rel: &str, group: &str) {
        self.set_addressable(project, rel, true);
        if self.settings.move_entry(rel, group) {
            self.save(project);
        }
    }

    /// Build ▸ Asset Bundles for `platform`; returns the message logged.
    pub fn build(&mut self, project: &Path, platform: &str) -> Result<String, String> {
        let out = build::build_dir(project, platform);
        let r = build::build_bundles(project, &self.settings, platform, &out).map(|b| {
            let size: u64 = b.catalog.bundles.iter().map(|x| x.size).sum();
            let msg = format!(
                "Built {} bundles ({} addressables, {:.1} KB) into {}",
                b.catalog.bundles.len(),
                b.catalog.entries.len(),
                size as f64 / 1024.0,
                out.strip_prefix(project)
                    .unwrap_or(&out)
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            self.analysis = Some(b.duplicates.clone());
            (msg, b.warnings)
        });
        match r {
            Ok((msg, warnings)) => {
                let full = if warnings.is_empty() {
                    msg
                } else {
                    format!("{msg}; {} warnings", warnings.len())
                };
                self.status = Some((false, full.clone()));
                Ok(full)
            }
            Err(e) => {
                self.status = Some((true, e.clone()));
                Err(e)
            }
        }
    }

    pub fn analyze(&mut self, project: &Path) {
        let dups = asset_groups::analyze(project, &self.settings);
        self.status = Some((
            !dups.is_empty(),
            if dups.is_empty() {
                "Analyze: no asset is packed into more than one bundle.".into()
            } else {
                format!(
                    "Analyze: {} assets are duplicated across bundles (make them addressable \
                     or pack their users together).",
                    dups.len()
                )
            },
        ));
        self.analysis = Some(dups);
    }
}

pub const ICON: &str = "▦";

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let project = state.project.root.clone();
    let platform = state.build_settings.target.slug().to_owned();
    state.asset_groups.ensure_loaded(&project);
    let mut log: Vec<(bool, String)> = Vec::new();
    let w = &mut state.asset_groups;

    panel_toolbar(ui, |ui| {
        ui.menu_button("New ⏷", |ui| {
            if ui.button("Packed Assets Group").clicked() {
                let name = w.settings.add_group();
                w.selected_group = Some(name);
                w.save(&project);
                ui.close();
            }
        });
        ui.menu_button("Play Mode Script ⏷", |ui| {
            for m in PlayModeScript::ALL {
                if ui.radio(w.settings.play_mode == m, m.label()).clicked() {
                    w.settings.play_mode = m;
                    w.save(&project);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(w.settings.play_mode.label());
        ui.menu_button("Build ⏷", |ui| {
            if ui.button("Asset Bundles").clicked() {
                let r = w.build(&project, &platform);
                log.push(match r {
                    Ok(m) => (false, m),
                    Err(e) => (true, format!("Asset bundle build failed: {e}")),
                });
                ui.close();
            }
            if ui.button("Clean Build Cache").clicked() {
                let dir = project.join(asset_groups::catalog::BUILD_DIR);
                let _ = std::fs::remove_dir_all(&dir);
                let _ = std::fs::remove_dir_all(project.join("Library/BundleCache"));
                w.status = Some((
                    false,
                    "Removed built bundles and the download cache.".into(),
                ));
                ui.close();
            }
        });
        if ui.button("Analyze").clicked() {
            w.analyze(&project);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut w.search, 160.0);
        });
    });

    if let Some((err, msg)) = &w.status {
        let color = if *err { theme::WARN } else { theme::TEXT_DIM };
        ui.label(RichText::new(msg).color(color));
    }

    // Group settings / entry details at the bottom.
    egui::Panel::bottom("asset_groups_details")
        .resizable(true)
        .default_size(170.0)
        .show(ui, |ui| details(ui, w, &project, &platform));

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                groups(ui, w, &project);
                if let Some(dups) = &w.analysis
                    && !dups.is_empty()
                {
                    ui.separator();
                    ui.label(RichText::new("Duplicate dependencies").strong());
                    for d in dups {
                        ui.label(
                            RichText::new(format!("⚠ {} is in {}", d.path, d.bundles.join(", ")))
                                .color(theme::WARN),
                        );
                    }
                }
            });
    });
    for (err, m) in log {
        if err {
            state.console.warn(m);
        } else {
            state.console.info(m);
        }
    }
}

fn groups(ui: &mut Ui, w: &mut AssetGroupsWindow, project: &Path) {
    let filter = w.search.to_lowercase();
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut adds: Vec<(String, String)> = Vec::new();
    let names: Vec<String> = w.settings.groups.iter().map(|g| g.name.clone()).collect();
    egui::Grid::new("asset_groups_header")
        .num_columns(3)
        .min_col_width(120.0)
        .show(ui, |ui| {
            ui.label(RichText::new("Group / Address").strong());
            ui.label(RichText::new("Path").strong());
            ui.label(RichText::new("Labels").strong());
            ui.end_row();
        });
    for name in names {
        let Some(g) = w.settings.group(&name).cloned() else {
            continue;
        };
        let selected =
            w.selected_group.as_deref() == Some(name.as_str()) && w.selected_entry.is_none();
        let title = format!(
            "{}{}  ({})",
            g.name,
            if g.name == DEFAULT_GROUP {
                " (Default)"
            } else {
                ""
            },
            g.entries.len()
        );
        let header = egui::CollapsingHeader::new(RichText::new(title).strong())
            .id_salt(("asset_group", &g.name))
            .default_open(true)
            .show(ui, |ui| {
                for e in &g.entries {
                    let hit = filter.is_empty()
                        || e.address.to_lowercase().contains(&filter)
                        || e.path.to_lowercase().contains(&filter)
                        || e.labels.iter().any(|l| l.to_lowercase().contains(&filter));
                    if !hit {
                        continue;
                    }
                    let sel = w.selected_entry.as_deref() == Some(e.path.as_str());
                    let resp = ui
                        .horizontal(|ui| {
                            let r = ui.selectable_label(sel, &e.address);
                            if e.path != e.address {
                                ui.weak(&e.path);
                            }
                            if !e.labels.is_empty() {
                                ui.label(RichText::new(e.labels.join(", ")).color(theme::PREFAB));
                            }
                            r
                        })
                        .inner;
                    let resp = resp.interact(egui::Sense::click_and_drag());
                    resp.dnd_set_drag_payload(EntryPayload(e.path.clone()));
                    if resp.clicked() {
                        w.selected_group = Some(g.name.clone());
                        w.selected_entry = Some(e.path.clone());
                    }
                }
                if g.entries.is_empty() {
                    ui.weak("Drag assets here from the Project window.");
                }
            });
        let resp = header.header_response.union(
            header
                .body_response
                .unwrap_or_else(|| header.header_response.clone()),
        );
        if header.header_response.clicked() || header.header_response.secondary_clicked() {
            w.selected_group = Some(g.name.clone());
            w.selected_entry = None;
        }
        if selected {
            ui.painter().rect_stroke(
                header.header_response.rect,
                2.0,
                egui::Stroke::new(1.0, theme::PREFAB),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(p) = resp.dnd_release_payload::<EntryPayload>() {
            moves.push((p.0.clone(), g.name.clone()));
        }
        if let Some(p) = resp.dnd_release_payload::<AssetPayload>() {
            adds.push((p.0.clone(), g.name.clone()));
        }
    }
    for (path, group) in moves {
        if w.settings.move_entry(&path, &group) {
            w.save(project);
        }
    }
    for (path, group) in adds {
        w.add_to_group(project, &path, &group);
    }
}

fn details(ui: &mut Ui, w: &mut AssetGroupsWindow, project: &Path, platform: &str) {
    let mut changed = false;
    if let Some(path) = w.selected_entry.clone() {
        let groups: Vec<String> = w.settings.groups.iter().map(|g| g.name.clone()).collect();
        let current_group = w.settings.entry(&path).map(|(g, _)| g.to_owned());
        let Some(e) = w.settings.entry_mut(&path) else {
            w.selected_entry = None;
            return;
        };
        ui.label(RichText::new(&e.path).strong());
        egui::Grid::new("asset_entry_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Address");
                changed |= ui.text_edit_singleline(&mut e.address).lost_focus();
                ui.end_row();
                ui.label("Labels");
                let mut text = e.labels.join(", ");
                if ui
                    .text_edit_singleline(&mut text)
                    .on_hover_text("Comma-separated; Assets.LoadAsync(label) loads them all")
                    .changed()
                {
                    e.labels = text
                        .split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect();
                    changed = true;
                }
                ui.end_row();
            });
        let mut move_to = None;
        ui.horizontal(|ui| {
            ui.label("Group");
            egui::ComboBox::from_id_salt("asset_entry_group")
                .selected_text(current_group.clone().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for g in &groups {
                        if ui
                            .selectable_label(current_group.as_ref() == Some(g), g)
                            .clicked()
                        {
                            move_to = Some(g.clone());
                        }
                    }
                });
            if ui
                .button("Remove")
                .on_hover_text("No longer addressable")
                .clicked()
            {
                w.settings.set_addressable(&path, false, None);
                w.selected_entry = None;
                changed = true;
            }
        });
        if let Some(g) = move_to {
            changed |= w.settings.move_entry(&path, &g);
        }
    } else if let Some(name) = w.selected_group.clone() {
        let Some(g) = w.settings.group_mut(&name) else {
            w.selected_group = None;
            return;
        };
        let is_default = g.name == DEFAULT_GROUP;
        egui::Grid::new("asset_group_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Name");
                ui.add_enabled_ui(!is_default, |ui| {
                    let mut n = g.name.clone();
                    if ui.text_edit_singleline(&mut n).lost_focus() && n.trim() != g.name {
                        g.name = n.trim().to_owned();
                        w.selected_group = Some(g.name.clone());
                        changed = true;
                    }
                });
                ui.end_row();
                ui.label("Bundle Mode");
                egui::ComboBox::from_id_salt("group_bundle_mode")
                    .selected_text(g.bundle_mode.label())
                    .show_ui(ui, |ui| {
                        for m in BundleMode::ALL {
                            changed |= ui
                                .selectable_value(&mut g.bundle_mode, m, m.label())
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label("Compression");
                egui::ComboBox::from_id_salt("group_compression")
                    .selected_text(g.compression.label())
                    .show_ui(ui, |ui| {
                        for c in BundleCompression::ALL {
                            changed |= ui
                                .selectable_value(&mut g.compression, c, c.label())
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label("Include in Build");
                changed |= ui.checkbox(&mut g.include_in_build, "").changed();
                ui.end_row();
                ui.label("Load Path");
                ui.horizontal(|ui| {
                    changed |= ui
                        .selectable_value(&mut g.location, BundleLocation::Local, "Local")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut g.location, BundleLocation::Remote, "Remote")
                        .changed();
                });
                ui.end_row();
                if g.location == BundleLocation::Remote {
                    ui.label("Remote URL");
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut g.remote_url).desired_width(260.0))
                        .on_hover_text("{Platform} becomes the build platform")
                        .lost_focus();
                    ui.end_row();
                    ui.label("");
                    ui.weak(format!("→ {}/<bundle>", g.remote_url_for(platform)));
                    ui.end_row();
                }
            });
        if !is_default && ui.button("Remove Group").clicked() {
            w.settings.remove_group(&name);
            w.selected_group = None;
            changed = true;
        }
    } else {
        ui.weak("Select a group or an entry to edit it.");
        ui.weak(format!(
            "Play Mode Script: {}.  Bundles build into {}/{platform}/.",
            w.settings.play_mode.label(),
            asset_groups::catalog::BUILD_DIR
        ));
    }
    if changed {
        w.save(project);
    }
}

/// Asset Inspector: the "Addressable" checkbox with the address, group and labels.
pub fn addressable_header(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    if !(rel.starts_with("Assets/") || rel.starts_with("Packages/")) {
        return;
    }
    let project = state.project.root.clone();
    let w = &mut state.asset_groups;
    w.ensure_loaded(&project);
    let entry = w
        .settings
        .entry(rel)
        .map(|(g, e)| (g.to_owned(), e.clone()));
    let mut on = entry.is_some();
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut on, "Addressable")
            .on_hover_text("Load this asset by address at runtime (Assets.LoadAsync)")
            .changed()
        {
            w.set_addressable(&project, rel, on);
        }
        if let Some((group, e)) = &entry {
            let mut address = e.address.clone();
            let r = ui.add(
                egui::TextEdit::singleline(&mut address)
                    .desired_width(f32::INFINITY.min(ui.available_width() - 4.0)),
            );
            if r.changed()
                && let Some(x) = w.settings.entry_mut(rel)
            {
                x.address = address;
            }
            if r.lost_focus() {
                w.save(&project);
            }
            r.on_hover_text(format!("Address (group: {group})"));
        }
    });
    if let Some((group, e)) = entry
        && on
    {
        ui.horizontal(|ui| {
            ui.weak(format!("Group: {group}"));
            let mut text = e.labels.join(", ");
            ui.label("Labels");
            let r = ui.text_edit_singleline(&mut text);
            if r.changed()
                && let Some(x) = w.settings.entry_mut(rel)
            {
                x.labels = text
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if r.lost_focus() {
                w.save(&project);
            }
        });
    }
    ui.separator();
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::project::ProjectBrowser;

    fn project(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_groups_ui_{tag}_{n}"));
        std::fs::create_dir_all(root.join("Assets/Prefabs")).unwrap();
        std::fs::write(root.join("Assets/Prefabs/A.prefab"), "{}").unwrap();
        std::fs::write(root.join("Assets/Prefabs/B.prefab"), "{}").unwrap();
        root
    }

    fn window(s: EditorState) -> Harness<'static, EditorState> {
        let mut h = Harness::builder()
            .with_size([700.0, 600.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h
    }

    #[test]
    fn groups_window_lists_edits_builds_and_analyzes() {
        let root = project("window");
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        s.asset_groups.ensure_loaded(&root);
        s.asset_groups
            .set_addressable(&root, "Assets/Prefabs/A.prefab", true);
        let mut h = window(s);
        h.get_by_label_contains("Default Local Group (Default)");
        h.get_by_label("Assets/Prefabs/A.prefab").click();
        h.run();
        // The entry's details: address and labels.
        assert_eq!(
            h.state().asset_groups.selected_entry.as_deref(),
            Some("Assets/Prefabs/A.prefab")
        );
        h.get_by_label("Remove").accesskit_node();

        // New ▸ Group.
        h.get_by_label("New ⏷").click();
        h.run();
        h.get_by_label("Packed Assets Group").click();
        h.run();
        h.get_by_label_contains("New Group  (0)");
        assert_eq!(h.state().asset_groups.settings.groups.len(), 2);
        let saved = AssetGroupSettings::load(&root).unwrap();
        assert_eq!(saved.groups[1].name, "New Group");

        // Play Mode Script.
        h.get_by_label("Play Mode Script ⏷").click();
        h.run();
        h.get_by_label(PlayModeScript::SimulateGroups.label())
            .click();
        h.run();
        assert_eq!(
            AssetGroupSettings::load(&root).unwrap().play_mode,
            PlayModeScript::SimulateGroups
        );

        // Build ▸ Asset Bundles.
        h.get_by_label("Build ⏷").click();
        h.run();
        h.get_by_label("Asset Bundles").click();
        h.run();
        let platform = h.state().build_settings.target.slug().to_owned();
        let out = build::build_dir(&root, &platform);
        assert!(
            out.join("catalog.json").is_file(),
            "{:?}",
            h.state().asset_groups.status
        );
        assert!(out.join("default_local_group_all.bundle").is_file());

        h.get_by_label("Analyze").click();
        h.run();
        h.get_by_label_contains("no asset is packed into more than one bundle");
    }

    #[test]
    fn inspector_header_marks_assets_addressable() {
        let root = project("inspector");
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        s.inspector_asset = Some("Assets/Prefabs/B.prefab".into());
        let mut h = Harness::builder().with_size([400.0, 500.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::panels::inspector::show(ui, s),
            s,
        );
        h.run();
        h.get_by_label("Addressable").click();
        h.run();
        let saved = AssetGroupSettings::load(&root).unwrap();
        let (group, e) = saved.entry("Assets/Prefabs/B.prefab").unwrap();
        assert_eq!(group, DEFAULT_GROUP);
        assert_eq!(e.address, "Assets/Prefabs/B.prefab");
        h.get_by_label("Labels");
        h.get_by_label("Addressable").click();
        h.run();
        assert!(
            AssetGroupSettings::load(&root)
                .unwrap()
                .entry("Assets/Prefabs/B.prefab")
                .is_none()
        );
    }

    #[test]
    fn moving_entries_between_groups() {
        let root = project("move");
        let mut w = AssetGroupsWindow::default();
        w.ensure_loaded(&root);
        w.add_to_group(&root, "Assets/Prefabs/A.prefab", DEFAULT_GROUP);
        let g = w.settings.add_group();
        w.add_to_group(&root, "Assets/Prefabs/A.prefab", &g);
        w.add_to_group(&root, "Assets/Prefabs/B.prefab", &g);
        let saved = AssetGroupSettings::load(&root).unwrap();
        assert_eq!(saved.group(&g).unwrap().entries.len(), 2);
        assert!(saved.group(DEFAULT_GROUP).unwrap().entries.is_empty());
    }
}
