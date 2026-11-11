//! Window ▸ General ▸ Localization Tables: a spreadsheet of a String Table's keys × locales.
//! Add keys and locales, edit cells (empty ones are highlighted), import and export CSV.
//! Every edit is saved to the `.strings` file right away.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Ui};
use misty_engine::localization::{StringTable, TableKind, is_right_to_left};

use crate::state::EditorState;

/// Folder new tables are created in.
pub const FOLDER: &str = "Assets/Localization";
/// Background of cells with no text.
pub const MISSING: Color32 = Color32::from_rgb(110, 38, 38);

#[derive(Default)]
pub struct TablesState {
    /// Project-relative path of the open table.
    pub selected: Option<String>,
    /// Working copy of the open table.
    pub table: Option<StringTable>,
    pub new_key: String,
    pub new_locale: String,
    pub new_table: String,
    pub filter: String,
    pub only_missing: bool,
    pub status: String,
    paths: Vec<String>,
    scanned: Option<Instant>,
}

/// `.strings` files under the project's `Assets/`, project-relative.
pub fn list_tables(root: &Path) -> Vec<String> {
    misty_engine::vfs::files_under(&root.join("Assets"))
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("strings"))
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

fn rescan(s: &mut EditorState) {
    s.saveloc.tables.paths = list_tables(&s.project.root);
    s.saveloc.tables.scanned = Some(Instant::now());
}

/// Open the window on a table (double-click in the Project window).
pub fn open(s: &mut EditorState, rel: &str) {
    s.saveloc.tables_open = true;
    select(s, rel);
}

/// Show the window (on the first table when none is open).
pub fn open_window(s: &mut EditorState) {
    s.saveloc.tables_open = true;
    rescan(s);
    if s.saveloc.tables.table.is_none()
        && let Some(first) = s.saveloc.tables.paths.first().cloned()
    {
        select(s, &first);
    }
}

/// Load a table into the window.
pub fn select(s: &mut EditorState, rel: &str) {
    match StringTable::load(&s.project.root.join(rel)) {
        Ok(t) => {
            s.saveloc.tables.selected = Some(rel.to_owned());
            s.saveloc.tables.table = Some(t);
            s.saveloc.tables.status.clear();
        }
        Err(e) => s.saveloc.tables.status = e,
    }
    rescan(s);
}

/// Write the working copy to its file.
pub fn save_current(s: &mut EditorState) {
    let (Some(rel), Some(t)) = (&s.saveloc.tables.selected, &s.saveloc.tables.table) else {
        return;
    };
    if let Err(e) = t.save(&s.project.root.join(rel)) {
        s.saveloc.tables.status = format!("Could not save: {e}");
    }
    s.saveloc.tables_changed();
}

/// A new table in `Assets/Localization` with the project's locales. Returns its path.
pub fn create_table(s: &mut EditorState, name: &str) -> Result<String, String> {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || " _-".contains(*c))
        .collect();
    if name.is_empty() {
        return Err("Type a table name first.".into());
    }
    let rel = format!("{FOLDER}/{name}.strings");
    let path = s.project.root.join(&rel);
    if path.exists() {
        return Err(format!("{rel} already exists."));
    }
    let mut locales: Vec<String> = s
        .project_settings
        .localization
        .locales
        .iter()
        .map(|l| l.code.clone())
        .collect();
    if locales.is_empty() {
        locales.push(s.project_settings.localization.default_locale.clone());
    }
    let refs: Vec<&str> = locales.iter().map(String::as_str).collect();
    StringTable::new(&name, &refs).save(&path)?;
    select(s, &rel);
    Ok(rel)
}

/// Merge a CSV file into the open table (and save it).
pub fn import_csv(s: &mut EditorState, path: &Path) -> Result<(usize, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let t = s
        .saveloc
        .tables
        .table
        .as_mut()
        .ok_or("Open a table first.")?;
    let counts = t.merge_csv(&text)?;
    save_current(s);
    s.saveloc.tables.status = format!(
        "Imported {}: {} new keys, {} cells changed.",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        counts.0,
        counts.1
    );
    Ok(counts)
}

/// Write the open table as CSV.
pub fn export_csv(s: &mut EditorState, path: &Path) -> Result<(), String> {
    let t = s
        .saveloc
        .tables
        .table
        .as_ref()
        .ok_or("Open a table first.")?;
    std::fs::write(path, t.to_csv()).map_err(|e| format!("{}: {e}", path.display()))?;
    s.saveloc.tables.status = format!("Exported {}.", path.display());
    Ok(())
}

pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.saveloc.tables_open;
    if !open {
        return;
    }
    egui::Window::new("Localization Tables")
        .open(&mut open)
        .default_size([980.0, 560.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.saveloc.tables_open = open;
}

pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    if s.saveloc
        .tables
        .scanned
        .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
    {
        rescan(s);
    }
    toolbar(ui, s);
    if !s.saveloc.tables.status.is_empty() {
        ui.weak(&s.saveloc.tables.status);
    }
    ui.separator();
    if s.saveloc.tables.table.is_none() {
        ui.add_space(8.0);
        ui.label("No String Table open.");
        ui.weak("Create one above, or double-click a .strings file in Asset Files.");
        return;
    }
    edit_row(ui, s);
    ui.separator();
    grid(ui, s);
}

fn toolbar(ui: &mut Ui, s: &mut EditorState) {
    ui.horizontal(|ui| {
        ui.label("Table");
        let current = s.saveloc.tables.selected.clone();
        let label = current
            .as_deref()
            .map(|p| {
                p.rsplit('/')
                    .next()
                    .unwrap_or(p)
                    .trim_end_matches(".strings")
                    .to_owned()
            })
            .unwrap_or_else(|| "(none)".into());
        let mut pick = None;
        egui::ComboBox::from_id_salt("loc_table_pick")
            .selected_text(label)
            .width(160.0)
            .show_ui(ui, |ui| {
                for p in &s.saveloc.tables.paths {
                    if ui
                        .selectable_label(current.as_deref() == Some(p.as_str()), p)
                        .clicked()
                    {
                        pick = Some(p.clone());
                    }
                }
            });
        if let Some(p) = pick {
            select(s, &p);
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut s.saveloc.tables.new_table)
                .hint_text("New table name")
                .desired_width(120.0),
        );
        if ui.button("Create Table").clicked() {
            let name = s.saveloc.tables.new_table.clone();
            match create_table(s, &name) {
                Ok(rel) => {
                    s.saveloc.tables.status = format!("Created {rel}.");
                    s.saveloc.tables.new_table.clear();
                }
                Err(e) => s.saveloc.tables.status = e,
            }
        }
        ui.separator();
        let has = s.saveloc.tables.table.is_some();
        if ui
            .add_enabled(has, egui::Button::new("Import CSV..."))
            .clicked()
            && let Some(p) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .pick_file()
            && let Err(e) = import_csv(s, &p)
        {
            s.saveloc.tables.status = e;
        }
        if ui
            .add_enabled(has, egui::Button::new("Export CSV..."))
            .clicked()
        {
            let name = s
                .saveloc
                .tables
                .table
                .as_ref()
                .map(|t| format!("{}.csv", t.name))
                .unwrap_or_default();
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name(name)
                .save_file()
                && let Err(e) = export_csv(s, &p)
            {
                s.saveloc.tables.status = e;
            }
        }
    });
}

fn edit_row(ui: &mut Ui, s: &mut EditorState) {
    let project_locales: Vec<String> = s
        .project_settings
        .localization
        .locales
        .iter()
        .map(|l| l.code.clone())
        .collect();
    let st = &mut s.saveloc.tables;
    let mut changed = false;
    ui.horizontal(|ui| {
        let key = ui.add(
            egui::TextEdit::singleline(&mut st.new_key)
                .hint_text("new.key")
                .desired_width(140.0),
        );
        let enter = key.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Add Key").clicked() || enter)
            && let Some(t) = st.table.as_mut()
        {
            if t.add_key(&st.new_key) {
                st.status = format!("Added '{}'.", st.new_key.trim());
                st.new_key.clear();
                changed = true;
            } else {
                st.status = "Type a key that isn't in the table yet.".into();
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut st.new_locale)
                .hint_text("fr")
                .desired_width(50.0),
        );
        if ui.button("Add Locale").clicked()
            && let Some(t) = st.table.as_mut()
            && t.add_locale(&st.new_locale)
        {
            st.new_locale.clear();
            changed = true;
        }
        // Locales from Project Settings the table lacks.
        if let Some(t) = st.table.as_mut() {
            let absent: Vec<&String> = project_locales
                .iter()
                .filter(|l| !t.locales.contains(l))
                .collect();
            if !absent.is_empty() {
                ui.menu_button("+ Project Locale", |ui| {
                    for l in absent {
                        if ui.button(l).clicked() {
                            t.add_locale(l);
                            changed = true;
                            ui.close();
                        }
                    }
                });
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut st.filter)
                .hint_text("Search keys and text")
                .desired_width(140.0),
        );
        ui.checkbox(&mut st.only_missing, "Only Missing");
    });
    if let Some(t) = &st.table {
        let missing = t.missing().len();
        let kind = if t.kind == TableKind::Assets {
            "Asset Table (cells are asset paths)"
        } else {
            "String Table"
        };
        ui.weak(format!(
            "{kind} · {} keys · {} locales · {missing} missing translations",
            t.entries.len(),
            t.locales.len()
        ));
    }
    if changed {
        save_current(s);
    }
}

/// What a click in the grid asks for (applied after drawing).
enum GridEdit {
    Cell(String, String, String),
    Rename(String, String),
    Remove(String),
    RemoveLocale(String),
}

fn grid(ui: &mut Ui, s: &mut EditorState) {
    let Some(t) = s.saveloc.tables.table.clone() else {
        return;
    };
    let filter = s.saveloc.tables.filter.to_lowercase();
    let only_missing = s.saveloc.tables.only_missing;
    let mut edits = Vec::new();
    egui::ScrollArea::both()
        .id_salt("loc_grid_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("loc_grid")
                .striped(true)
                .num_columns(t.locales.len() + 2)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Key").strong());
                    for l in &t.locales {
                        let missing = t
                            .entries
                            .iter()
                            .filter(|e| e.values.get(l).is_none_or(|v| v.is_empty()))
                            .count();
                        ui.horizontal(|ui| {
                            let mut head = RichText::new(l).strong();
                            if missing > 0 {
                                head = RichText::new(format!("{l} ({missing} missing)"))
                                    .strong()
                                    .color(Color32::from_rgb(230, 120, 110));
                            }
                            let r = ui.label(head);
                            if is_right_to_left(l) {
                                r.on_hover_text("Written right to left");
                            }
                            if ui
                                .small_button("✖")
                                .on_hover_text(format!("Remove the {l} column"))
                                .clicked()
                            {
                                edits.push(GridEdit::RemoveLocale(l.clone()));
                            }
                        });
                    }
                    ui.label("");
                    ui.end_row();
                    for e in &t.entries {
                        let has_missing = t
                            .locales
                            .iter()
                            .any(|l| e.values.get(l).is_none_or(|v| v.is_empty()));
                        if only_missing && !has_missing {
                            continue;
                        }
                        if !filter.is_empty()
                            && !e.key.to_lowercase().contains(&filter)
                            && !e
                                .values
                                .values()
                                .any(|v| v.to_lowercase().contains(&filter))
                        {
                            continue;
                        }
                        let mut key = e.key.clone();
                        let r = ui.add_sized(
                            [150.0, 20.0],
                            egui::TextEdit::singleline(&mut key).font(egui::TextStyle::Monospace),
                        );
                        if r.changed() {
                            edits.push(GridEdit::Rename(e.key.clone(), key));
                        }
                        if !e.comment.is_empty() {
                            r.on_hover_text(&e.comment);
                        }
                        for l in &t.locales {
                            let mut text = e.values.get(l).cloned().unwrap_or_default();
                            let empty = text.is_empty();
                            let mut edit = egui::TextEdit::singleline(&mut text)
                                .desired_width(170.0)
                                .hint_text("missing");
                            if empty {
                                edit = edit.background_color(MISSING);
                            }
                            if ui.add_sized([180.0, 20.0], edit).changed() {
                                edits.push(GridEdit::Cell(e.key.clone(), l.clone(), text));
                            }
                        }
                        if ui
                            .small_button("🗑")
                            .on_hover_text(format!("Remove '{}'", e.key))
                            .clicked()
                        {
                            edits.push(GridEdit::Remove(e.key.clone()));
                        }
                        ui.end_row();
                    }
                });
        });
    if edits.is_empty() {
        return;
    }
    let st = &mut s.saveloc.tables;
    let Some(t) = st.table.as_mut() else { return };
    for edit in edits {
        match edit {
            GridEdit::Cell(k, l, v) => t.set(&k, &l, &v),
            GridEdit::Rename(old, new) => {
                if !t.rename_key(&old, &new) {
                    st.status = format!("'{}' can't be used as a key here.", new.trim());
                }
            }
            GridEdit::Remove(k) => {
                t.remove_key(&k);
            }
            GridEdit::RemoveLocale(l) => {
                t.remove_locale(&l);
            }
        }
    }
    save_current(s);
}
