//! Project Settings ▸ Tags and Layers: user tags (add / remove) and the 32 layer names.
//! Names are committed when their field loses focus, so trimming and duplicate checks don't
//! fight the typing.

use egui::Ui;

use misty_engine::tags_layers::{BUILTIN_TAGS, FIRST_USER_LAYER, LAYER_COUNT, TagsAndLayers};

/// Draw the page; true when something changed.
pub fn page(ui: &mut Ui, t: &mut TagsAndLayers) -> bool {
    let mut edited = false;
    egui::CollapsingHeader::new("Tags")
        .id_salt("tags_list")
        .default_open(true)
        .show(ui, |ui| {
            let mut remove = None;
            for i in 0..t.tags.len() {
                ui.horizontal(|ui| {
                    ui.add_sized([110.0, 18.0], egui::Label::new(format!("Tag {i}")));
                    if let Some(name) = committed_text(ui, ("tag", i), &t.tags[i]) {
                        t.tags[i] = name;
                        edited = true;
                    }
                    if ui.small_button("−").on_hover_text("Remove tag").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                t.tags.remove(i);
                edited = true;
            }
            if ui.small_button("+").on_hover_text("Add a tag").clicked() {
                t.tags.push(unique_tag(t));
                edited = true;
            }
            ui.weak(format!("Built in: {}", BUILTIN_TAGS.join(", ")));
        });
    egui::CollapsingHeader::new("Layers")
        .id_salt("layers_list")
        .default_open(true)
        .show(ui, |ui| {
            for i in 0..LAYER_COUNT.min(t.layers.len()) {
                ui.horizontal(|ui| {
                    let builtin = i < FIRST_USER_LAYER;
                    let label = if builtin {
                        "Builtin Layer"
                    } else {
                        "User Layer"
                    };
                    ui.add_sized([110.0, 18.0], egui::Label::new(format!("{label} {i}")));
                    if builtin {
                        let mut name = t.layers[i].clone();
                        ui.add_enabled(
                            false,
                            egui::TextEdit::singleline(&mut name).desired_width(200.0),
                        );
                    } else if let Some(name) = committed_text(ui, ("layer", i), &t.layers[i]) {
                        t.layers[i] = name;
                        edited = true;
                    }
                });
            }
        });
    edited
}

/// "New Tag", "New Tag 1", … whichever is free.
fn unique_tag(t: &TagsAndLayers) -> String {
    (0..)
        .map(|n| {
            if n == 0 {
                "New Tag".to_owned()
            } else {
                format!("New Tag {n}")
            }
        })
        .find(|name| !t.has_tag(name))
        .unwrap_or_default()
}

/// A text field editing a copy of `current`; returns the new text once editing ends.
fn committed_text(ui: &mut Ui, salt: (&str, usize), current: &str) -> Option<String> {
    let id = ui.make_persistent_id(salt);
    let mut buf = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_owned());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(200.0));
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    (r.lost_focus() && buf != current).then_some(buf)
}
