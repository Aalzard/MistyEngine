//! Mesh Renderer ▸ Lighting: Contribute Global Illumination, Receive Global Illumination and the
//! Lightmapping values (Scale In Lightmap, the baked lightmap index); model import settings
//! (Generate Lightmap UVs) and the baked lightmap asset preview.

use egui::Ui;
use misty_engine::models::ModelImporter;
use misty_engine::scene::{MeshRenderer, ReceiveGi};

use crate::state::EditorState;

use super::editors::{combo_k, grid};
use super::mixed;

pub fn renderer_section(ui: &mut Ui, r: &mut MeshRenderer, is_static: bool) -> bool {
    super::card::sub_header(ui, "Lighting");
    grid(ui, "meshrenderer_lighting_grid", |ui| {
        let mut ch = false;
        ui.label("Contribute Global Illumination").on_hover_text(
            "Static objects with this on are baked: they block and bounce light in lightmaps",
        );
        ch |= mixed::checkbox(ui, "contribute_gi", &mut r.contribute_gi, "").changed();
        ui.end_row();
        if r.contribute_gi {
            ui.label("Receive Global Illumination");
            ch |= combo_k(
                ui,
                "mr_receive_gi",
                "receive_gi",
                &mut r.receive_gi,
                &ReceiveGi::ALL,
                ReceiveGi::label,
            );
            ui.end_row();
        }
        if r.contribute_gi && r.receive_gi == ReceiveGi::Lightmaps {
            ui.label("Scale In Lightmap");
            ch |= ui
                .add(
                    mixed::drag(
                        "scale_in_lightmap",
                        egui::DragValue::new(&mut r.scale_in_lightmap),
                    )
                    .speed(0.01)
                    .range(0.0..=100.0),
                )
                .changed();
            ui.end_row();
            ui.label("Lightmap");
            if r.lightmap_index >= 0 {
                ui.weak(format!("Lightmap-{}", r.lightmap_index));
            } else {
                ui.weak("Not baked");
            }
            ui.end_row();
        }
        if r.contribute_gi && !is_static {
            ui.label("");
            ui.weak("Mark the object Static to bake it.");
            ui.end_row();
        }
        ch
    })
}

/// Model import settings: Generate Lightmap UVs, Virtual Geometry (written to the `.meta`,
/// re-imports the model).
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
    let mut s = ModelImporter::load(&abs);
    let changed = grid(ui, "model_import_grid", |ui| {
        ui.label("Generate Lightmap UVs").on_hover_text(
            "Unwrap a second UV set (non-overlapping charts with padding) for baked lighting",
        );
        let mut ch = ui.checkbox(&mut s.generate_lightmap_uvs, "").changed();
        ui.end_row();
        ui.label("Virtual Geometry").on_hover_text(
            "Build the meshes into clusters with per-cluster level of detail (Misty Virtual \
            Geometry, prototype): dense rocks, cliffs and props then draw about as many \
            triangles as they cover pixels. Needs Virtual Geometry on in the Quality level. \
            Opaque, non-skinned meshes; not for alpha-tested foliage. (GameObject ▸ Enable \
            Virtual Geometry and Tools ▸ Rendering ▸ Convert Scene to Virtual Geometry set it \
            for you.)",
        );
        ch |= ui.checkbox(&mut s.virtual_geometry, "").changed();
        ui.end_row();
        ch
    });
    if changed {
        match s.save(&abs) {
            Ok(()) => {
                misty_engine::models::poll_changes();
                state.console.info(format!("Re-imported {name}."));
            }
            Err(e) => state
                .console
                .warn(format!("Could not save import settings: {e}")),
        }
    }
    if let Ok(model) = misty_engine::models::model(rel) {
        ui.add_space(6.0);
        ui.weak(format!(
            "{} mesh{}, {} vertices",
            model.meshes.len(),
            if model.meshes.len() == 1 { "" } else { "es" },
            model
                .meshes
                .iter()
                .map(|m| m.data.vertices.len())
                .sum::<usize>()
        ));
        if model.virtual_geometry {
            ui.weak(virtual_geometry_summary(&model.virtual_builds.summary()));
            if !model.virtual_builds.complete() {
                // The build runs on a worker: show its result when it lands.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }
}

/// The import Inspector's line about a model's virtual geometry build.
fn virtual_geometry_summary(s: &misty_engine::models::virtual_build::BuildSummary) -> String {
    if s.built < s.meshes {
        return format!(
            "Virtual Geometry: building clusters ({} of {} meshes done)…",
            s.built, s.meshes
        );
    }
    format!(
        "Virtual Geometry: {} clusters, {} level{}, {} source triangles; built in {:.1} s{}",
        s.clusters,
        s.levels,
        if s.levels == 1 { "" } else { "s" },
        s.source_triangles,
        s.build_ms / 1000.0,
        if s.cached {
            " (from Library/VirtualGeometry)"
        } else {
            ""
        }
    )
}

/// A baked lightmap file: size and a small preview.
pub fn lightmap_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.label(crate::theme::bold(ui.ctx(), "Lightmap (baked lighting)"));
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let Some(data) = misty_engine::lightmap::store::load(&state.assets.absolute(rel)) else {
        ui.weak("The file can't be read.");
        return;
    };
    ui.weak(format!("{} × {} texels, HDR", data.width, data.height));
    // Preview: Reinhard-tonemapped, sRGB.
    let pixels: Vec<egui::Color32> = data
        .texels
        .iter()
        .map(|c| {
            let t = *c / (*c + glam::Vec3::ONE);
            let s = t
                .to_array()
                .map(|v| (v.max(0.0).powf(1.0 / 2.2) * 255.0) as u8);
            egui::Color32::from_rgb(s[0], s[1], s[2])
        })
        .collect();
    let image = egui::ColorImage::new([data.width as usize, data.height as usize], pixels);
    let tex = ui.ctx().load_texture(
        format!("lightmap:{rel}"),
        image,
        egui::TextureOptions::NEAREST,
    );
    let w = ui.available_width().min(256.0);
    ui.image((tex.id(), egui::vec2(w, w)));
}
