//! Window ▸ General ▸ Save Data: the save slots Play mode wrote to `Library/Saves`, with
//! their metadata, thumbnail and contents, and Delete.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use misty_engine::save::{self, SlotInfo};

use crate::state::EditorState;

#[derive(Default)]
pub struct SaveDataState {
    pub selected: Option<String>,
    /// Show the selected save's JSON.
    pub view_json: bool,
    /// Slot waiting for a second click on Delete ("*" = all).
    pub confirm_delete: Option<String>,
    pub slots: Vec<SlotInfo>,
    scanned: Option<Instant>,
    /// Selected slot and its pretty JSON.
    json: Option<(String, String)>,
    thumb: Option<(PathBuf, egui::TextureHandle)>,
    pub status: String,
}

/// The editor's saves folder for the open project.
pub fn folder(s: &EditorState) -> PathBuf {
    save::folder_for(&s.project.root)
}

/// Read the slot list again now.
pub fn rescan(s: &mut EditorState) {
    let folder = folder(s);
    let st = &mut s.saveloc.save_data;
    let slots = save::list_slots(&folder);
    st.scanned = Some(Instant::now());
    // A rewritten save shows its new contents and thumbnail.
    if slots != st.slots {
        st.json = None;
        st.thumb = None;
    }
    st.slots = slots;
    if let Some(sel) = &st.selected
        && !st.slots.iter().any(|i| &i.slot == sel)
    {
        st.selected = None;
    }
}

pub fn open(s: &mut EditorState) {
    s.saveloc.save_data_open = true;
    rescan(s);
    // Show the newest slot right away.
    let st = &mut s.saveloc.save_data;
    if st.selected.is_none() {
        st.selected = st.slots.first().map(|i| i.slot.clone());
    }
}

pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.saveloc.save_data_open;
    if !open {
        return;
    }
    egui::Window::new("Save Data")
        .open(&mut open)
        .default_size([640.0, 520.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.saveloc.save_data_open = open;
}

/// Delete a slot's files and update the list.
pub fn delete(s: &mut EditorState, slot: &str) {
    let folder = folder(s);
    if save::delete_slot(&folder, slot) {
        s.saveloc.save_data.status = format!("Deleted '{slot}'.");
    }
    rescan(s);
}

pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    // Saves change while playing: re-read the folder now and then.
    let stale = s
        .saveloc
        .save_data
        .scanned
        .is_none_or(|t| t.elapsed() > Duration::from_secs(1));
    if stale {
        rescan(s);
    }
    let folder = folder(s);
    let rel = folder
        .strip_prefix(&s.project.root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| folder.display().to_string());
    ui.horizontal(|ui| {
        ui.label("Folder:");
        ui.monospace(&rel)
            .on_hover_text("Play mode saves here; built games use the player's user folder.");
        if ui.button("Refresh").clicked() {
            rescan(s);
        }
        if ui.button("Copy Path").clicked() {
            ui.ctx().copy_text(folder.display().to_string());
        }
        let all = s.saveloc.save_data.confirm_delete.as_deref() == Some("*");
        let label = if all {
            "Confirm Delete All"
        } else {
            "Delete All"
        };
        if ui
            .add_enabled(
                !s.saveloc.save_data.slots.is_empty(),
                egui::Button::new(label),
            )
            .clicked()
        {
            if all {
                let slots: Vec<String> = s
                    .saveloc
                    .save_data
                    .slots
                    .iter()
                    .map(|i| i.slot.clone())
                    .collect();
                for slot in slots {
                    save::delete_slot(&folder, &slot);
                }
                s.saveloc.save_data.confirm_delete = None;
                s.saveloc.save_data.status = "Deleted every save.".into();
                rescan(s);
            } else {
                s.saveloc.save_data.confirm_delete = Some("*".into());
            }
        }
    });
    if !s.saveloc.save_data.status.is_empty() {
        ui.weak(&s.saveloc.save_data.status);
    }
    ui.separator();
    if s.saveloc.save_data.slots.is_empty() {
        ui.add_space(8.0);
        ui.label("No saves yet.");
        ui.weak("Enter Play mode and call SaveSystem.Save(slot) from a script.");
        return;
    }
    slot_table(ui, s);
    ui.separator();
    details(ui, s);
}

fn slot_table(ui: &mut Ui, s: &mut EditorState) {
    let slots = s.saveloc.save_data.slots.clone();
    let mut select = None;
    let mut delete_now = None;
    egui::ScrollArea::vertical()
        .id_salt("save_slots")
        .max_height(170.0)
        .show(ui, |ui| {
            egui::Grid::new("save_slot_grid")
                .striped(true)
                .num_columns(6)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Slot", "Saved", "Scene", "Play Time", "Size", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for info in &slots {
                        let selected = s.saveloc.save_data.selected.as_deref() == Some(&info.slot);
                        if ui.selectable_label(selected, &info.slot).clicked() {
                            select = Some(info.slot.clone());
                        }
                        ui.label(save::format_time(info.time));
                        ui.label(&info.scene);
                        ui.label(save::format_duration(info.play_time));
                        ui.label(size_text(info.size));
                        ui.horizontal(|ui| {
                            if ui.small_button("View").clicked() {
                                select = Some(info.slot.clone());
                                s.saveloc.save_data.view_json = true;
                            }
                            let confirm =
                                s.saveloc.save_data.confirm_delete.as_deref() == Some(&info.slot);
                            let label = if confirm { "Confirm" } else { "Delete" };
                            let b = ui
                                .small_button(label)
                                .on_hover_text(format!("Delete slot '{}'", info.slot));
                            if b.clicked() {
                                if confirm {
                                    delete_now = Some(info.slot.clone());
                                } else {
                                    s.saveloc.save_data.confirm_delete = Some(info.slot.clone());
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
        });
    if let Some(slot) = select {
        s.saveloc.save_data.selected = Some(slot);
        s.saveloc.save_data.json = None;
    }
    if let Some(slot) = delete_now {
        s.saveloc.save_data.confirm_delete = None;
        delete(s, &slot);
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn details(ui: &mut Ui, s: &mut EditorState) {
    let Some(slot) = s.saveloc.save_data.selected.clone() else {
        ui.weak("Select a slot to see what it holds.");
        return;
    };
    let folder = folder(s);
    let Some(info) = s
        .saveloc
        .save_data
        .slots
        .iter()
        .find(|i| i.slot == slot)
        .cloned()
    else {
        return;
    };
    if s.saveloc
        .save_data
        .json
        .as_ref()
        .is_none_or(|(n, _)| *n != slot)
    {
        let text = match save::read_slot_value(&folder, &slot) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("// {e}"),
        };
        s.saveloc.save_data.json = Some((slot.clone(), text));
    }
    let json = s
        .saveloc
        .save_data
        .json
        .as_ref()
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let count = |k: &str| match &parsed[k] {
        serde_json::Value::Array(a) => a.len(),
        serde_json::Value::Object(o) => o.len(),
        _ => 0,
    };
    ui.horizontal_top(|ui| {
        if info.thumbnail {
            let path = save::thumbnail_path(&folder, &slot);
            if let Some(tex) = thumbnail(ui.ctx(), s, path) {
                let size = tex.size_vec2();
                let w = 160.0_f32.min(size.x);
                ui.image((tex.id(), egui::vec2(w, w * size.y / size.x.max(1.0))));
            }
        }
        egui::Grid::new("save_slot_info")
            .num_columns(2)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.weak(k);
                    ui.label(v);
                    ui.end_row();
                };
                row("Slot", info.slot.clone());
                row("Saved", save::format_time(info.time));
                row("Scene", info.scene.clone());
                row(
                    "Scene File",
                    info.scene_path
                        .clone()
                        .unwrap_or_else(|| "(not saved)".into()),
                );
                row("Play Time", save::format_duration(info.play_time));
                row("Data Version", info.version.to_string());
                row("Objects", count("objects").to_string());
                row("Spawned", count("spawned").to_string());
                row("Destroyed", count("destroyed").to_string());
            });
    });
    ui.checkbox(&mut s.saveloc.save_data.view_json, "Show Contents");
    if s.saveloc.save_data.view_json {
        egui::ScrollArea::both()
            .id_salt("save_json")
            .max_height(220.0)
            .show(ui, |ui| {
                let mut text = json.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
            });
    }
}

fn thumbnail(
    ctx: &egui::Context,
    s: &mut EditorState,
    path: PathBuf,
) -> Option<egui::TextureHandle> {
    if let Some((p, tex)) = &s.saveloc.save_data.thumb
        && *p == path
    {
        return Some(tex.clone());
    }
    let img = misty_engine::image::open(&path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let tex = ctx.load_texture(
        format!("save_thumb:{}", path.display()),
        color,
        egui::TextureOptions::LINEAR,
    );
    s.saveloc.save_data.thumb = Some((path, tex.clone()));
    Some(tex)
}
