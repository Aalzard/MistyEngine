//! Model import settings in the Inspector (selected `.fbx` / `.glb` / `.gltf` / `.obj`), laid
//! out like Unity's Model Importer: Scene (Scale Factor, Convert Units, Axis Conversion),
//! Meshes (Read/Write, Generate Lightmap UVs, Virtual Geometry, Generate LODs), Geometry
//! (Normals, Smoothing Angle, Tangents), Materials (Location, Extract Materials…) and
//! Animation (the file's takes, clips cut by frame range, with a preview). Revert / Apply.
//!
//! Unapplied edits live in egui's memory (per model) until Apply writes the `.meta` file.

use egui::{Color32, Stroke, Ui, pos2, vec2};
use misty_engine::models::{
    self, AxisConversion, ClipSettings, LodSetting, ModelImporter, SAMPLE_RATE, VectorSource,
};

use super::card::{PropertyGrid, sub_header};
use crate::state::EditorState;

/// Unapplied settings and the clip preview's state.
#[derive(Clone)]
struct Pending {
    settings: ModelImporter,
    /// Clip shown in the preview (index into the model's clips).
    clip: usize,
    time: f32,
    playing: bool,
}

fn pending_id(rel: &str) -> egui::Id {
    egui::Id::new(("misty_model_import", rel.to_owned()))
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> &'static str,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for v in all {
                ui.selectable_value(value, *v, label(*v));
            }
        });
}

pub fn model_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let name = std::path::Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.label(crate::theme::bold(
        ui.ctx(),
        format!("{name} (Model) Import Settings"),
    ));
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let abs = state.assets.absolute(rel);
    let on_disk = ModelImporter::load(&abs);
    let id = pending_id(rel);
    let mut p = ui
        .ctx()
        .data(|d| d.get_temp::<Pending>(id))
        .unwrap_or_else(|| Pending {
            settings: on_disk.clone(),
            clip: 0,
            time: 0.0,
            playing: false,
        });
    let model = models::model(rel);

    scene_section(ui, &mut p.settings, models::is_fbx(rel));
    meshes_section(ui, &mut p.settings);
    geometry_section(ui, &mut p.settings);
    materials_section(ui, state, rel, &mut p.settings, &on_disk);
    if let Ok(m) = &model
        && !m.takes.is_empty()
    {
        animation_section(ui, &mut p, m);
    }

    ui.add_space(6.0);
    let dirty = p.settings != on_disk;
    ui.horizontal(|ui| {
        if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
            p.settings = on_disk.clone();
        }
        if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
            apply(state, rel, &p.settings, &on_disk);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, p));
    summary(ui, rel);
}

/// Write the settings, re-import the model, and re-extract what the settings changed.
pub fn apply(state: &mut EditorState, rel: &str, settings: &ModelImporter, before: &ModelImporter) {
    let abs = state.assets.absolute(rel);
    let mut s = settings.clone();
    s.sanitize();
    if let Err(e) = s.save(&abs) {
        state
            .console
            .warn(format!("Could not save import settings of {rel}: {e}"));
        return;
    }
    models::forget(&[rel.to_owned()]);
    let name = std::path::Path::new(rel)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    state.console.info(format!("Re-imported {name}."));
    let Ok(model) = models::model(rel) else {
        return;
    };
    // New clips replace the extracted ones (the clips are made from the file again).
    if s.clips != before.clips || s.scale_factor != before.scale_factor {
        match models::refresh_animations(rel, &model) {
            Ok(written) => {
                for w in written {
                    state.console.info(format!("Wrote {w}"));
                }
            }
            Err(e) => state
                .console
                .warn(format!("Could not write the clips of {rel}: {e}")),
        }
    }
    if s.material_folder != before.material_folder {
        extract(state, rel, &model);
    }
    state.assets.refresh();
}

fn extract(state: &mut EditorState, rel: &str, model: &models::Model) {
    match models::extract_materials(rel, model) {
        Ok(created) if created.is_empty() => state
            .console
            .info(format!("The materials of {rel} are already extracted.")),
        Ok(created) => {
            for c in created {
                state.console.info(format!("Extracted {c}"));
            }
        }
        Err(e) => state
            .console
            .warn(format!("Could not extract the materials of {rel}: {e}")),
    }
}

fn scene_section(ui: &mut Ui, s: &mut ModelImporter, fbx: bool) {
    sub_header(ui, "Scene");
    PropertyGrid::new("model_import_scene").show(ui, |ui| {
        ui.label("Scale Factor").on_hover_text(
            "Multiplies the whole model: meshes, node positions, skins and animation.",
        );
        ui.add(
            egui::DragValue::new(&mut s.scale_factor)
                .speed(0.01)
                .range(0.0001..=10000.0),
        );
        ui.end_row();
        if fbx {
            ui.label("Convert Units").on_hover_text(
                "Bring the file's units (centimetres in most FBX files) in as metres. Off: one \
                file unit is one unit.",
            );
            ui.checkbox(&mut s.convert_units, "");
            ui.end_row();
        }
        ui.label("Axis Conversion").on_hover_text(
            "Which way is up in the file. Automatic reads it from FBX files (glTF and OBJ are \
            Y up). Z Up turns the model -90° about X, on its root objects.",
        );
        combo(
            ui,
            "model_axis",
            &mut s.axis_conversion,
            &AxisConversion::ALL,
            AxisConversion::label,
        );
        ui.end_row();
    });
}

fn meshes_section(ui: &mut Ui, s: &mut ModelImporter) {
    sub_header(ui, "Meshes");
    PropertyGrid::new("model_import_meshes").show(ui, |ui| {
        ui.label("Read/Write").on_hover_text(
            "Kept for imported settings: Misty keeps imported meshes readable on the CPU either \
            way (physics, picking and lightmap baking read them).",
        );
        ui.checkbox(&mut s.read_write, "");
        ui.end_row();
        ui.label("Generate Lightmap UVs").on_hover_text(
            "Unwrap a second UV set (non-overlapping charts with padding) for baked lighting",
        );
        ui.checkbox(&mut s.generate_lightmap_uvs, "");
        ui.end_row();
        ui.label("Misty Mesh").on_hover_text(
            "Build the meshes into clusters with per-cluster level of detail (virtual \
            geometry, prototype): dense rocks, cliffs and props then draw about as many \
            triangles as they cover pixels. Needs Misty Mesh on in the Quality level. \
            Opaque, non-skinned meshes; not for alpha-tested foliage. (Object ▸ Enable \
            Misty Mesh and Tools ▸ Rendering ▸ Convert Scene to Misty Mesh set it \
            for you.)",
        );
        ui.checkbox(&mut s.virtual_geometry, "");
        ui.end_row();
        ui.label("Generate LODs").on_hover_text(
            "Simplified copies of the static meshes (edge collapse; borders and UV seams kept) \
            and a LOD Group on the model's root that switches between them by screen height.",
        );
        ui.checkbox(&mut s.generate_lods, "");
        ui.end_row();
    });
    if s.generate_lods {
        lod_table(ui, &mut s.lods);
    }
}

fn lod_table(ui: &mut Ui, lods: &mut Vec<LodSetting>) {
    let mut remove = None;
    egui::Grid::new("model_import_lods")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.weak("Level");
            ui.weak("Triangles");
            ui.weak("Screen height");
            ui.label("");
            ui.end_row();
            let n = lods.len();
            for (i, l) in lods.iter_mut().enumerate() {
                ui.label(format!("LOD {i}"));
                if i == 0 {
                    ui.weak("100%");
                } else {
                    let mut pct = l.quality * 100.0;
                    if ui
                        .add(
                            egui::DragValue::new(&mut pct)
                                .range(2.0..=100.0)
                                .suffix("%")
                                .speed(0.5),
                        )
                        .changed()
                    {
                        l.quality = pct / 100.0;
                    }
                }
                let mut h = l.screen_height * 100.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut h)
                            .range(0.0..=100.0)
                            .suffix("%")
                            .speed(0.25),
                    )
                    .on_hover_text(
                        "This level draws while the model is at least this tall on screen",
                    )
                    .changed()
                {
                    l.screen_height = h / 100.0;
                }
                if i > 0 && n > 2 && ui.small_button("✖").clicked() {
                    remove = Some(i);
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        lods.remove(i);
    }
    if lods.len() < misty_engine::scene::LodGroup::MAX_LEVELS && ui.button("Add Level").clicked() {
        let last = lods.last().copied().unwrap_or_default();
        lods.push(LodSetting {
            quality: (last.quality * 0.5).max(0.02),
            screen_height: last.screen_height * 0.5,
        });
    }
}

fn geometry_section(ui: &mut Ui, s: &mut ModelImporter) {
    sub_header(ui, "Geometry");
    PropertyGrid::new("model_import_geometry").show(ui, |ui| {
        ui.label("Normals").on_hover_text(
            "Import: the file's normals (calculated when it has none). Calculate: from the \
            faces, with the Smoothing Angle. None: no lighting normals (they point up).",
        );
        combo(
            ui,
            "model_normals",
            &mut s.normals,
            &VectorSource::ALL,
            VectorSource::label,
        );
        ui.end_row();
        if s.normals == VectorSource::Calculate {
            ui.label("Smoothing Angle")
                .on_hover_text("Faces meeting at a sharper angle than this get a hard edge.");
            ui.add(
                egui::Slider::new(&mut s.smoothing_angle, 0.0..=180.0)
                    .suffix("°")
                    .integer(),
            );
            ui.end_row();
        }
        ui.label("Tangents").on_hover_text(
            "Calculate: from the normals and UVs (needed by normal maps). Import: the file's \
            when it has them. None: constant tangents (no normal mapping).",
        );
        combo(
            ui,
            "model_tangents",
            &mut s.tangents,
            &VectorSource::ALL,
            VectorSource::label,
        );
        ui.end_row();
    });
}

fn materials_section(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    s: &mut ModelImporter,
    on_disk: &ModelImporter,
) {
    sub_header(ui, "Materials");
    PropertyGrid::new("model_import_materials").show(ui, |ui| {
        ui.label("Location").on_hover_text(
            "Where the model's materials (and embedded textures) are extracted as .mat files. \
            Existing files are never overwritten, so edits survive re-imports.",
        );
        let shown = if s.material_folder.is_empty() {
            "Next to the model".to_owned()
        } else {
            s.material_folder.clone()
        };
        ui.horizontal(|ui| {
            ui.label(shown);
            if !s.material_folder.is_empty() && ui.small_button("Reset").clicked() {
                s.material_folder.clear();
            }
        });
        ui.end_row();
    });
    if ui
        .button("Extract Materials…")
        .on_hover_text("Choose a folder in the project and extract the materials there now.")
        .clicked()
    {
        let root = state.assets.absolute("Assets");
        let picked = rfd::FileDialog::new()
            .set_title("Extract Materials to")
            .set_directory(&root)
            .pick_folder();
        if let Some(dir) = picked {
            let folder = state.assets.relative(&dir);
            if std::path::Path::new(&folder).is_absolute() {
                state
                    .console
                    .warn("Choose a folder inside the project's Assets folder.");
            } else {
                s.material_folder = folder;
                let mut applied = on_disk.clone();
                applied.material_folder = s.material_folder.clone();
                apply(state, rel, &applied, on_disk);
            }
        }
    }
}

fn animation_section(ui: &mut Ui, p: &mut Pending, model: &models::Model) {
    sub_header(ui, "Animation");
    for t in &model.takes {
        ui.weak(format!(
            "Take \"{}\": {:.2} s ({} frames at {SAMPLE_RATE:.0} fps)",
            t.name,
            t.length,
            (t.length * SAMPLE_RATE).round()
        ));
    }
    let clips = &mut p.settings.clips;
    if clips.is_empty() {
        ui.weak("One clip per take. Add clips to cut takes into several by frame range.");
    }
    let mut remove = None;
    egui::Grid::new("model_import_clips")
        .num_columns(6)
        .striped(true)
        .show(ui, |ui| {
            if !clips.is_empty() {
                for h in ["Name", "Take", "Start", "End", "Loop", ""] {
                    ui.weak(h);
                }
                ui.end_row();
            }
            for (i, c) in clips.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(90.0));
                let take = if c.take.is_empty() {
                    model
                        .takes
                        .first()
                        .map(|t| t.name.clone())
                        .unwrap_or_default()
                } else {
                    c.take.clone()
                };
                egui::ComboBox::from_id_salt(("model_clip_take", i))
                    .selected_text(take)
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for t in &model.takes {
                            ui.selectable_value(&mut c.take, t.name.clone(), &t.name);
                        }
                    });
                ui.add(
                    egui::DragValue::new(&mut c.first_frame)
                        .range(0.0..=1e6)
                        .speed(0.5),
                );
                ui.add(
                    egui::DragValue::new(&mut c.last_frame)
                        .range(0.0..=1e6)
                        .speed(0.5),
                );
                ui.checkbox(&mut c.looping, "");
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        clips.remove(i);
    }
    if ui.button("Add Clip").clicked() {
        let take = model.takes.first().cloned();
        let frames = take
            .as_ref()
            .map_or(30.0, |t| (t.length * SAMPLE_RATE).round());
        clips.push(ClipSettings {
            name: format!("Clip{}", clips.len() + 1),
            take: take.map(|t| t.name).unwrap_or_default(),
            first_frame: 0.0,
            last_frame: frames,
            looping: true,
        });
    }
    clip_preview(ui, p, model);
}

/// The model posed by a clip (the applied clips), seen from the front: skeleton and a sample of
/// the vertices, with a time scrubber and Play.
fn clip_preview(ui: &mut Ui, p: &mut Pending, model: &models::Model) {
    if model.animations.is_empty() {
        return;
    }
    ui.add_space(4.0);
    p.clip = p.clip.min(model.animations.len() - 1);
    let anim = &model.animations[p.clip];
    let length = anim.clip.length.max(1e-3);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("model_preview_clip")
            .selected_text(&anim.name)
            .show_ui(ui, |ui| {
                for (i, a) in model.animations.iter().enumerate() {
                    ui.selectable_value(&mut p.clip, i, &a.name);
                }
            });
        if ui
            .button(if p.playing { "⏸" } else { "▶" })
            .on_hover_text("Play the clip in the preview")
            .clicked()
        {
            p.playing = !p.playing;
        }
        ui.add(egui::Slider::new(&mut p.time, 0.0..=length).show_value(false));
        ui.weak(format!(
            "{:.0} / {:.0}",
            p.time * SAMPLE_RATE,
            length * SAMPLE_RATE
        ));
    });
    if p.playing {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        p.time += dt;
        if p.time > length {
            p.time = if anim.clip.looping {
                p.time % length
            } else {
                length
            };
            p.playing &= anim.clip.looping;
        }
        ui.ctx().request_repaint();
    }
    let shape = models::preview::preview_shape(model, Some(&anim.clip), p.time, 3000);
    let rest = models::preview::preview_shape(model, None, 0.0, 3000);
    let mut lo = glam::Vec2::splat(f32::MAX);
    let mut hi = glam::Vec2::splat(f32::MIN);
    for q in rest
        .points
        .iter()
        .chain(rest.bones.iter().flat_map(|(a, b)| [a, b]))
    {
        lo = lo.min(q.truncate());
        hi = hi.max(q.truncate());
    }
    if lo.x > hi.x {
        return;
    }
    let size = (hi - lo).max(glam::Vec2::splat(1e-3)) * 1.3;
    let center = (lo + hi) * 0.5;
    let w = ui.available_width().min(320.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 200.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(32));
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let to_screen = |v: glam::Vec3| {
        pos2(
            rect.center().x + (v.x - center.x) * scale,
            rect.center().y - (v.y - center.y) * scale,
        )
    };
    for q in &shape.points {
        painter.circle_filled(to_screen(*q), 1.0, Color32::from_gray(150));
    }
    let bone = Stroke::new(2.0, Color32::from_rgb(110, 190, 255));
    for (a, b) in &shape.bones {
        painter.line_segment([to_screen(*a), to_screen(*b)], bone);
        painter.circle_filled(to_screen(*b), 2.5, Color32::from_rgb(110, 190, 255));
    }
    ui.weak("Preview of the applied clips (front view).");
}

fn summary(ui: &mut Ui, rel: &str) {
    let Ok(model) = models::model(rel) else {
        if let Err(e) = models::model(rel) {
            ui.colored_label(Color32::from_rgb(230, 120, 90), e);
        }
        return;
    };
    ui.add_space(6.0);
    let lod_meshes: std::collections::HashSet<usize> = model
        .nodes
        .iter()
        .flat_map(|n| n.lods.iter().flatten().copied())
        .collect();
    let base: Vec<&models::ModelMesh> = model
        .meshes
        .iter()
        .enumerate()
        .filter(|(i, _)| !lod_meshes.contains(i))
        .map(|(_, m)| m)
        .collect();
    ui.weak(format!(
        "{} mesh{}, {} vertices, {} triangles; {} material{}, {} bone{}, {} clip{}",
        base.len(),
        if base.len() == 1 { "" } else { "es" },
        base.iter().map(|m| m.data.vertices.len()).sum::<usize>(),
        base.iter().map(|m| m.data.indices.len() / 3).sum::<usize>(),
        model.materials.len(),
        if model.materials.len() == 1 { "" } else { "s" },
        model.skins.iter().map(|s| s.joints.len()).sum::<usize>(),
        if model.skins.iter().map(|s| s.joints.len()).sum::<usize>() == 1 {
            ""
        } else {
            "s"
        },
        model.animations.len(),
        if model.animations.len() == 1 { "" } else { "s" },
    ));
    if !model.lod_heights.is_empty() {
        let levels = model.lod_heights.len();
        let tris: Vec<String> = (0..levels)
            .map(|l| {
                let n: usize = model
                    .nodes
                    .iter()
                    .flat_map(|n| {
                        if l == 0 {
                            n.meshes.clone()
                        } else {
                            n.lods.get(l - 1).cloned().unwrap_or_default()
                        }
                    })
                    .filter(|&m| l == 0 || lod_meshes.contains(&m))
                    .map(|m| model.meshes[m].data.indices.len() / 3)
                    .sum();
                format!("LOD{l} {n}")
            })
            .collect();
        ui.weak(format!("LOD triangles: {}", tris.join(", ")));
    }
    if model.virtual_geometry {
        ui.weak(super::lightmap_editors::virtual_geometry_summary(
            &model.virtual_builds.summary(),
        ));
        if !model.virtual_builds.complete() {
            // The build runs on a worker: show its result when it lands.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}
