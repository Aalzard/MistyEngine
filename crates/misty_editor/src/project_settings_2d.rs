//! Project Settings ▸ Physics 2D (gravity, query options; the Layer Collision Matrix is the one
//! on the Physics page) and ▸ Sorting Layers (add, rename, reorder, remove; Default stays).

use egui::Ui;
use misty_engine::settings::ProjectSettings;
use misty_engine::sprites::sorting;

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
    })
    .inner
}

pub fn physics2d(ui: &mut Ui, s: &mut ProjectSettings) -> bool {
    let p = &mut s.physics2d;
    let mut edited = row(ui, "Gravity", "Acceleration of 2D bodies (X, Y).", |ui| {
        let x = ui.add(
            egui::DragValue::new(&mut p.gravity.x)
                .speed(0.05)
                .prefix("X "),
        );
        let y = ui.add(
            egui::DragValue::new(&mut p.gravity.y)
                .speed(0.05)
                .prefix("Y "),
        );
        x.changed() || y.changed()
    });
    edited |= row(
        ui,
        "Queries Hit Triggers",
        "Physics2D raycasts and overlaps also return trigger colliders.",
        |ui| ui.checkbox(&mut p.queries_hit_triggers, "").changed(),
    );
    edited |= row(
        ui,
        "Queries Start In Colliders",
        "Raycasts that start inside a collider report it (at distance 0).",
        |ui| ui.checkbox(&mut p.queries_start_in_colliders, "").changed(),
    );
    ui.add_space(6.0);
    ui.weak("Friction and Bounciness are set on each 2D collider.");
    ui.weak("2D layers collide by the Layer Collision Matrix on the Physics page.");
    edited
}

pub fn sorting_layers(ui: &mut Ui, s: &mut ProjectSettings) -> bool {
    ui.weak("Renderers on later layers draw on top of earlier ones.");
    ui.add_space(4.0);
    let layers = &mut s.sorting_layers;
    let mut edited = false;
    // (index, -1 up / 1 down / 0 remove)
    let mut action: Option<(usize, i32)> = None;
    let n = layers.len();
    egui::Grid::new("sorting_layers_grid")
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            for (i, name) in layers.iter_mut().enumerate() {
                ui.label(format!("Layer {i}"));
                let is_default = name == sorting::DEFAULT;
                if is_default {
                    ui.label(sorting::DEFAULT);
                } else {
                    edited |= ui
                        .add(egui::TextEdit::singleline(name).desired_width(160.0))
                        .lost_focus();
                }
                if ui.add_enabled(i > 0, egui::Button::new("▲")).clicked() {
                    action = Some((i, -1));
                }
                if ui.add_enabled(i + 1 < n, egui::Button::new("▼")).clicked() {
                    action = Some((i, 1));
                }
                if ui
                    .add_enabled(!is_default, egui::Button::new("−"))
                    .clicked()
                {
                    action = Some((i, 0));
                }
                ui.end_row();
            }
        });
    match action {
        Some((i, 0)) => {
            layers.remove(i);
            edited = true;
        }
        Some((i, d)) => {
            layers.swap(i, (i as i32 + d) as usize);
            edited = true;
        }
        None => {}
    }
    if ui.button("+ Add Sorting Layer").clicked() {
        let mut k = layers.len();
        while layers.contains(&format!("Layer {k}")) {
            k += 1;
        }
        layers.push(format!("Layer {k}"));
        edited = true;
    }
    edited
}
