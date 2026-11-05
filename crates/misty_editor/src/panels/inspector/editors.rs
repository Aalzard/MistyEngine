//! Per-component Inspector editors and the Add Component popup.

use egui::{Key, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Animation, AudioListener, AudioSource, BoxCollider, Camera, CapsuleCollider, ClearFlags,
    Interpolation, Light, LightType, MeshCollider, MeshRenderer, PhysicsSurface, PostProcessVolume,
    Rigidbody, SceneComponent, Shadows, SphereCollider, WheelCollider, registry,
};

use misty_engine::scene::{ScriptValue, Scripts};

use super::mixed;
use crate::actions::Action;
use crate::assets::AssetDatabase;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::vec3_row_mixed;

pub fn icon(name: &str) -> &'static str {
    match name {
        "Transform" => "⬚",
        "Camera" => "🎥",
        "Light" => "💡",
        "Mesh Filter" => "▦",
        "Mesh Renderer" => "◈",
        "Rigidbody" => "⚛",
        "Box Collider" | "Sphere Collider" | "Capsule Collider" | "Mesh Collider" => "⬡",
        "Audio Source" => crate::state::AUDIO_ICON,
        "Audio Listener" => "🎧",
        "Animation" => "⏵",
        "Animator" => crate::panels::animator::CONTROLLER_ICON,
        "Particle System" => crate::state::PARTICLE_ICON,
        "Post-process Volume" => "◐",
        other => super::ui_editors::icon(other)
            .or_else(|| super::physics_editors::icon(other))
            .or_else(|| super::audio_editors::icon(other))
            .or_else(|| super::terrain::icon(other))
            .or_else(|| super::nav_editors::icon(other))
            .or_else(|| super::sprite_editors::icon(other))
            .or_else(|| super::tilemap_editors::icon(other))
            .or_else(|| super::probe_editor::icon(other))
            .or_else(|| super::light_probe_editor::icon(other))
            .or_else(|| super::fog_editors::icon(other))
            .or_else(|| super::occlusion_editors::icon(other))
            .or_else(|| super::saveloc_editors::icon(other))
            .or_else(|| super::glow_editors::icon(other))
            .or_else(|| super::vcam_editors::icon(other))
            .or_else(|| super::timeline_editors::icon(other))
            .or_else(|| super::video_editors::icon(other))
            .or_else(|| super::skinned_editors::icon(other))
            .or_else(|| super::lod_editor::icon(other))
            .or_else(|| super::rig_editors::icon(other))
            .or_else(|| super::water::icon(other))
            .or_else(|| super::wind_editors::icon(other))
            .or_else(|| super::mesh_builder_editor::icon(other))
            .or_else(|| super::spline_editors::icon(other))
            .unwrap_or("▣"),
    }
}

/// Components with an enable checkbox in their header.
pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    if super::ui_editors::NAMES.contains(&name) {
        return super::ui_editors::enabled_flag(state, e, name);
    }
    if super::terrain::NAMES.contains(&name) {
        return super::terrain::enabled_flag(state, e, name);
    }
    if super::audio_editors::NAMES.contains(&name) {
        return super::audio_editors::enabled_flag(state, e, name);
    }
    if super::nav_editors::NAMES.contains(&name) {
        return super::nav_editors::enabled_flag(state, e, name);
    }
    if super::spline_editors::NAMES.contains(&name) {
        return super::spline_editors::enabled_flag(state, e, name);
    }
    if super::sprite_editors::NAMES.contains(&name) {
        return super::sprite_editors::enabled_flag(state, e, name);
    }
    if super::tilemap_editors::NAMES.contains(&name) {
        return super::tilemap_editors::enabled_flag(state, e, name);
    }
    if super::probe_editor::NAMES.contains(&name) {
        return super::probe_editor::enabled_flag(state, e, name);
    }
    if super::light_probe_editor::NAMES.contains(&name) {
        return super::light_probe_editor::enabled_flag(state, e, name);
    }
    if super::fog_editors::NAMES.contains(&name) {
        return super::fog_editors::enabled_flag(state, e, name);
    }
    if super::occlusion_editors::NAMES.contains(&name) {
        return super::occlusion_editors::enabled_flag(state, e, name);
    }
    if super::saveloc_editors::NAMES.contains(&name) {
        return super::saveloc_editors::enabled_flag(state, e, name);
    }
    if super::glow_editors::NAMES.contains(&name) {
        return super::glow_editors::enabled_flag(state, e, name);
    }
    if super::vcam_editors::NAMES.contains(&name) {
        return super::vcam_editors::enabled_flag(state, e, name);
    }
    if super::timeline_editors::NAMES.contains(&name) {
        return super::timeline_editors::enabled_flag(state, e, name);
    }
    if super::video_editors::NAMES.contains(&name) {
        return super::video_editors::enabled_flag(state, e, name);
    }
    if super::skinned_editors::NAMES.contains(&name) {
        return super::skinned_editors::enabled_flag(state, e, name);
    }
    if super::lod_editor::NAMES.contains(&name) {
        return super::lod_editor::enabled_flag(state, e, name);
    }
    if super::rig_editors::NAMES.contains(&name) {
        return super::rig_editors::enabled_flag(state, e, name);
    }
    if super::water::NAMES.contains(&name) {
        return super::water::enabled_flag(state, e, name);
    }
    if super::wind_editors::NAMES.contains(&name) {
        return super::wind_editors::enabled_flag(state, e, name);
    }
    match name {
        "Camera" => w.get::<&Camera>(e).ok().map(|c| c.enabled),
        "Light" => w.get::<&Light>(e).ok().map(|c| c.enabled),
        "Mesh Renderer" => w.get::<&MeshRenderer>(e).ok().map(|c| c.enabled),
        "Box Collider" => w.get::<&BoxCollider>(e).ok().map(|c| c.enabled),
        "Sphere Collider" => w.get::<&SphereCollider>(e).ok().map(|c| c.enabled),
        "Capsule Collider" => w.get::<&CapsuleCollider>(e).ok().map(|c| c.enabled),
        "Mesh Collider" => w.get::<&MeshCollider>(e).ok().map(|c| c.enabled),
        "Wheel Collider" => w.get::<&WheelCollider>(e).ok().map(|c| c.enabled),
        "Audio Source" => w.get::<&AudioSource>(e).ok().map(|c| c.enabled),
        "Audio Listener" => w.get::<&AudioListener>(e).ok().map(|c| c.enabled),
        "Animation" => w.get::<&Animation>(e).ok().map(|c| c.enabled),
        "Animator" => w
            .get::<&misty_engine::scene::Animator>(e)
            .ok()
            .map(|c| c.enabled),
        "Post-process Volume" => w.get::<&PostProcessVolume>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if super::ui_editors::NAMES.contains(&name) {
        return super::ui_editors::set_enabled(state, e, name, on);
    }
    if super::terrain::NAMES.contains(&name) {
        return super::terrain::set_enabled(state, e, name, on);
    }
    if super::audio_editors::NAMES.contains(&name) {
        return super::audio_editors::set_enabled(state, e, name, on);
    }
    if super::nav_editors::NAMES.contains(&name) {
        return super::nav_editors::set_enabled(state, e, name, on);
    }
    if super::spline_editors::NAMES.contains(&name) {
        return super::spline_editors::set_enabled(state, e, name, on);
    }
    if super::sprite_editors::NAMES.contains(&name) {
        return super::sprite_editors::set_enabled(state, e, name, on);
    }
    if super::tilemap_editors::NAMES.contains(&name) {
        return super::tilemap_editors::set_enabled(state, e, name, on);
    }
    if super::probe_editor::NAMES.contains(&name) {
        return super::probe_editor::set_enabled(state, e, name, on);
    }
    if super::light_probe_editor::NAMES.contains(&name) {
        return super::light_probe_editor::set_enabled(state, e, name, on);
    }
    if super::fog_editors::NAMES.contains(&name) {
        return super::fog_editors::set_enabled(state, e, name, on);
    }
    if super::occlusion_editors::NAMES.contains(&name) {
        return super::occlusion_editors::set_enabled(state, e, name, on);
    }
    if super::saveloc_editors::NAMES.contains(&name) {
        return super::saveloc_editors::set_enabled(state, e, name, on);
    }
    if super::glow_editors::NAMES.contains(&name) {
        return super::glow_editors::set_enabled(state, e, name, on);
    }
    if super::vcam_editors::NAMES.contains(&name) {
        return super::vcam_editors::set_enabled(state, e, name, on);
    }
    if super::timeline_editors::NAMES.contains(&name) {
        return super::timeline_editors::set_enabled(state, e, name, on);
    }
    if super::video_editors::NAMES.contains(&name) {
        return super::video_editors::set_enabled(state, e, name, on);
    }
    if super::skinned_editors::NAMES.contains(&name) {
        return super::skinned_editors::set_enabled(state, e, name, on);
    }
    if super::lod_editor::NAMES.contains(&name) {
        return super::lod_editor::set_enabled(state, e, name, on);
    }
    if super::rig_editors::NAMES.contains(&name) {
        return super::rig_editors::set_enabled(state, e, name, on);
    }
    if super::water::NAMES.contains(&name) {
        return super::water::set_enabled(state, e, name, on);
    }
    if super::wind_editors::NAMES.contains(&name) {
        return super::wind_editors::set_enabled(state, e, name, on);
    }
    let w = &mut state.scene.world;
    match name {
        "Camera" => w.get::<&mut Camera>(e).map(|mut c| c.enabled = on).ok(),
        "Light" => w.get::<&mut Light>(e).map(|mut c| c.enabled = on).ok(),
        "Mesh Renderer" => w
            .get::<&mut MeshRenderer>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Box Collider" => w
            .get::<&mut BoxCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Sphere Collider" => w
            .get::<&mut SphereCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Capsule Collider" => w
            .get::<&mut CapsuleCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Mesh Collider" => w
            .get::<&mut MeshCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Wheel Collider" => w
            .get::<&mut WheelCollider>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Audio Source" => w
            .get::<&mut AudioSource>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Audio Listener" => w
            .get::<&mut AudioListener>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Animation" => w.get::<&mut Animation>(e).map(|mut c| c.enabled = on).ok(),
        "Animator" => w
            .get::<&mut misty_engine::scene::Animator>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        "Post-process Volume" => w
            .get::<&mut PostProcessVolume>(e)
            .map(|mut c| c.enabled = on)
            .ok(),
        _ => None,
    };
}

/// Draw the fields for component `name`. Returns true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, targets: &[Entity], name: &str) -> bool {
    let e = targets[0];
    match name {
        "Transform" => transform(ui, state, targets),
        "Camera" => with::<Camera>(ui, state, e, camera),
        "Light" => with::<Light>(ui, state, e, light),
        "Mesh Filter" => super::slots::mesh_filter(ui, state, e),
        "Mesh Renderer" => mesh_renderer(ui, state, e),
        "LOD Group" => super::lod_editor::lod_group(ui, state, e),
        "Rigidbody" => with::<Rigidbody>(ui, state, e, rigidbody),
        "Box Collider" => with::<BoxCollider>(ui, state, e, box_collider),
        "Sphere Collider" => with::<SphereCollider>(ui, state, e, sphere_collider),
        "Capsule Collider" => with::<CapsuleCollider>(ui, state, e, capsule_collider),
        "Mesh Collider" => with::<MeshCollider>(ui, state, e, mesh_collider),
        "Audio Source" => super::audio_editors::audio_source(ui, state, e),
        "Animation" => super::anim::animation(ui, state, e),
        "Animator" => super::animator::animator(ui, state, e),
        "Particle System" => super::particles::particle_system(ui, state, e),
        "Visual Effect" => super::visual_effect::visual_effect(ui, state, e),
        "Post-process Volume" => super::post_volume::post_process_volume(ui, state, e),
        "Audio Listener" => {
            ui.weak("Hears every Audio Source in the scene.");
            false
        }
        ui_name if super::ui_editors::NAMES.contains(&ui_name) => {
            super::ui_editors::edit(ui, state, e, ui_name)
        }
        audio if super::audio_editors::NAMES.contains(&audio) => {
            super::audio_editors::edit(ui, state, e, audio)
        }
        physics if super::physics_editors::NAMES.contains(&physics) => {
            super::physics_editors::edit(ui, state, e, physics)
        }
        terrain if super::terrain::NAMES.contains(&terrain) => {
            super::terrain::edit(ui, state, e, terrain)
        }
        nav if super::nav_editors::NAMES.contains(&nav) => {
            super::nav_editors::edit(ui, state, e, nav)
        }
        two_d if super::sprite_editors::NAMES.contains(&two_d) => {
            super::sprite_editors::edit(ui, state, e, two_d)
        }
        tiles if super::tilemap_editors::NAMES.contains(&tiles) => {
            super::tilemap_editors::edit(ui, state, e, tiles)
        }
        lp if super::light_probe_editor::NAMES.contains(&lp) => {
            super::light_probe_editor::edit(ui, state, e, lp)
        }
        probe if super::probe_editor::NAMES.contains(&probe) => {
            super::probe_editor::edit(ui, state, e, probe)
        }
        fog if super::fog_editors::NAMES.contains(&fog) => {
            super::fog_editors::edit(ui, state, e, fog)
        }
        area if super::occlusion_editors::NAMES.contains(&area) => {
            super::occlusion_editors::edit(ui, state, e, area)
        }
        sl if super::saveloc_editors::NAMES.contains(&sl) => {
            super::saveloc_editors::edit(ui, state, e, sl)
        }
        glow if super::glow_editors::NAMES.contains(&glow) => {
            super::glow_editors::edit(ui, state, e, glow)
        }
        rig if super::rig_editors::NAMES.contains(&rig) => {
            super::rig_editors::edit(ui, state, e, rig)
        }
        vcam if super::vcam_editors::NAMES.contains(&vcam) => {
            super::vcam_editors::edit(ui, state, e, vcam)
        }
        tl if super::timeline_editors::NAMES.contains(&tl) => {
            super::timeline_editors::edit(ui, state, e, tl)
        }
        vp if super::video_editors::NAMES.contains(&vp) => {
            super::video_editors::edit(ui, state, e, vp)
        }
        skinned if super::skinned_editors::NAMES.contains(&skinned) => {
            super::skinned_editors::edit(ui, state, e, skinned)
        }
        wb if crate::panels::world_builder::NAMES.contains(&wb) => {
            crate::panels::world_builder::inspector(ui, state, e, wb)
        }
        water if super::water::NAMES.contains(&water) => super::water::edit(ui, state, e, water),
        wind if super::wind_editors::NAMES.contains(&wind) => {
            super::wind_editors::edit(ui, state, e, wind)
        }
        mb if super::mesh_builder_editor::NAMES.contains(&mb) => {
            super::mesh_builder_editor::edit(ui, state, e, mb)
        }
        spline if super::spline_editors::NAMES.contains(&spline) => {
            super::spline_editors::edit(ui, state, e, spline)
        }
        _ => {
            ui.weak("No editor for this component.");
            false
        }
    }
}

/// Edit a copy of component `T` and write it back if the UI changed it.
fn with<T: SceneComponent>(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    draw: fn(&mut Ui, &mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = draw(ui, &mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

/// Local position / Euler rotation / scale. Changed axes are applied to every target.
fn transform(ui: &mut Ui, state: &mut EditorState, targets: &[Entity]) -> bool {
    let t0 = state.scene.transform(targets[0]);
    let old = [
        t0.position.to_array(),
        t0.euler().to_array(),
        t0.scale.to_array(),
    ];
    let mut new = old;
    // An axis is mixed when any other target's value differs from the active one.
    let mut mixed = [[false; 3]; 3];
    for &t in &targets[1..] {
        let tr = state.scene.transform(t);
        let vals = [
            tr.position.to_array(),
            tr.euler().to_array(),
            tr.scale.to_array(),
        ];
        for f in 0..3 {
            for a in 0..3 {
                mixed[f][a] |= (vals[f][a] - old[f][a]).abs() > 1e-5;
            }
        }
    }
    let mut changed = vec3_row_mixed(ui, "Position", &mut new[0], 0.1, mixed[0]);
    changed |= vec3_row_mixed(ui, "Rotation", &mut new[1], 1.0, mixed[1]);
    ui.horizontal(|ui| {
        let lock = if state.scale_locked { "🔗" } else { "⛓" };
        if ui
            .add(
                egui::Button::new(lock)
                    .frame(false)
                    .selected(state.scale_locked),
            )
            .on_hover_text("Constrain Proportions")
            .clicked()
        {
            state.scale_locked = !state.scale_locked;
        }
        changed |= vec3_row_mixed(ui, "Scale", &mut new[2], 0.01, mixed[2]);
    });
    if !changed {
        return false;
    }
    // Constrained scale: the edited axis scales the other two by the same ratio.
    let scale_ratio = (0..3)
        .find(|&i| new[2][i] != old[2][i] && old[2][i].abs() > 1e-6)
        .map(|i| new[2][i] / old[2][i]);
    for &e in targets {
        let mut t = state.scene.transform(e);
        let mut cur = [
            t.position.to_array(),
            t.euler().to_array(),
            t.scale.to_array(),
        ];
        for field in 0..3 {
            for axis in 0..3 {
                if new[field][axis] != old[field][axis] {
                    cur[field][axis] = new[field][axis];
                }
            }
        }
        if state.scale_locked
            && let Some(r) = scale_ratio
        {
            cur[2] = (Vec3::from(t.scale.to_array()) * r).to_array();
        }
        t.position = Vec3::from(cur[0]);
        if new[1] != old[1] {
            t.set_euler(Vec3::from(cur[1]));
        }
        t.scale = Vec3::from(cur[2]);
        state.scene.set_transform(e, t);
    }
    true
}

/// Two-column property grid like the Inspector's label/value layout.
pub(super) fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    super::card::grid(ui, id, body)
}

pub(super) fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    combo_k(ui, id, "", value, options, label)
}

/// [`combo`] for the component field `key` (shows "—" when the targets differ).
pub(super) fn combo_k<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    key: &str,
    value: &mut T,
    options: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(super::mixed::text(key, label(*value)))
        .show_ui(ui, |ui| {
            for &o in options {
                changed |= ui.selectable_value(value, o, label(o)).changed();
            }
        });
    changed
}

/// sRGB float colour edited through egui's sRGB colour picker.
pub(super) fn color_rgb(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut srgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed =
        crate::panels::inspector::card::wide(ui, |ui| ui.color_edit_button_srgb(&mut srgb))
            .changed();
    if changed {
        *c = srgb.map(|v| v as f32 / 255.0);
    }
    changed
}

fn camera(ui: &mut Ui, c: &mut Camera) -> bool {
    grid(ui, "camera_grid", |ui| {
        let mut ch = false;
        ui.label("Clear Flags");
        ch |= combo_k(
            ui,
            "cam_clear",
            "clear_flags",
            &mut c.clear_flags,
            &ClearFlags::ALL,
            ClearFlags::label,
        );
        ui.end_row();
        ui.label("Background");
        let mut rgb = [c.background[0], c.background[1], c.background[2]];
        if color_rgb(ui, &mut rgb) {
            c.background[..3].copy_from_slice(&rgb);
            ch = true;
        }
        ui.end_row();
        ui.label("Culling Mask");
        ch |= super::layer_mask::mask_field(ui, "cam_cull", &mut c.culling_mask);
        ui.end_row();
        ui.label("Occlusion Culling");
        ch |= mixed::checkbox(ui, "occlusion_culling", &mut c.occlusion_culling, "").changed();
        ui.end_row();
        ui.label("Projection");
        let mut ortho = c.orthographic;
        egui::ComboBox::from_id_salt("cam_proj")
            .selected_text(mixed::text(
                "orthographic",
                if ortho { "Orthographic" } else { "Perspective" },
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ortho, false, "Perspective");
                ui.selectable_value(&mut ortho, true, "Orthographic");
            });
        if ortho != c.orthographic {
            c.orthographic = ortho;
            ch = true;
        }
        ui.end_row();
        if c.orthographic {
            ui.label("Size");
            ch |= ui
                .add(
                    mixed::drag("ortho_size", egui::DragValue::new(&mut c.ortho_size))
                        .speed(0.05)
                        .range(0.01..=1e5),
                )
                .changed();
        } else {
            ui.label("Field of View");
            ch |= ui
                .add(mixed::slider(
                    "fov",
                    egui::Slider::new(&mut c.fov, 1.0..=179.0),
                ))
                .changed();
        }
        ui.end_row();
        ui.label("Clipping Planes");
        ui.horizontal(|ui| {
            ui.label("Near");
            ch |= ui
                .add(
                    mixed::drag("near", egui::DragValue::new(&mut c.near))
                        .speed(0.01)
                        .range(0.01..=c.far - 0.01),
                )
                .changed();
            ui.label("Far");
            ch |= ui
                .add(
                    mixed::drag("far", egui::DragValue::new(&mut c.far))
                        .speed(1.0)
                        .range(c.near + 0.01..=1e7),
                )
                .changed();
        });
        ui.end_row();
        ui.label("Viewport Rect");
        ui.horizontal(|ui| {
            for (i, l) in ["X", "Y", "W", "H"].iter().enumerate() {
                ui.label(*l);
                ch |= ui
                    .add(
                        mixed::drag(
                            &format!("viewport/{i}"),
                            egui::DragValue::new(&mut c.viewport[i]),
                        )
                        .speed(0.01)
                        .range(0.0..=1.0),
                    )
                    .changed();
            }
        });
        ui.end_row();
        ui.label("Depth");
        ch |= ui
            .add(mixed::drag("depth", egui::DragValue::new(&mut c.depth)).speed(0.1))
            .changed();
        ui.end_row();
        ch
    })
}

fn light(ui: &mut Ui, l: &mut Light) -> bool {
    let mut ch = grid(ui, "light_grid", |ui| {
        let mut ch = false;
        ui.label("Type");
        ch |= combo_k(
            ui,
            "light_type",
            "kind",
            &mut l.kind,
            &LightType::ALL,
            LightType::label,
        );
        ui.end_row();
        ui.label("Mode").on_hover_text(
            "Realtime: lit every frame. Mixed: realtime direct light, baked bounce light.              Baked: direct and bounce light baked into lightmaps (Lighting ▸ Generate Lighting)",
        );
        ch |= combo_k(
            ui,
            "light_mode",
            "mode",
            &mut l.mode,
            &misty_engine::scene::LightMode::ALL,
            misty_engine::scene::LightMode::label,
        );
        ui.end_row();
        if l.kind != LightType::Directional {
            ui.label("Range");
            ch |= ui
                .add(
                    mixed::drag("range", egui::DragValue::new(&mut l.range))
                        .speed(0.1)
                        .range(0.0..=1e5),
                )
                .changed();
            ui.end_row();
        }
        if l.kind == LightType::Spot {
            ui.label("Spot Angle");
            ch |= ui
                .add(mixed::slider(
                    "spot_angle",
                    egui::Slider::new(&mut l.spot_angle, 1.0..=179.0),
                ))
                .changed();
            ui.end_row();
        }
        if l.kind == LightType::Rectangle {
            ch |= super::light_area::rows(ui, l);
        }
        ui.label("Color");
        ch |= color_rgb(ui, &mut l.color);
        ui.end_row();
        ui.label("Intensity");
        ch |= ui
            .add(
                mixed::drag("intensity", egui::DragValue::new(&mut l.intensity))
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ch
    });
    super::card::sub_header(ui, "Shadow Settings");
    ch |= grid(ui, "light_shadow_grid", |ui| {
        let mut ch = false;
        crate::tutorials::targets::label(ui, "Shadow Type");
        ch |= combo_k(
            ui,
            "light_shadows",
            "shadows",
            &mut l.shadows,
            &Shadows::ALL,
            Shadows::label,
        );
        ui.end_row();
        ch |= super::light_shadows::rows(ui, l);
        ui.label("Contact Shadows").on_hover_text(
            "Screen-space contact shadows (needs a volume's Contact Shadows override)",
        );
        ch |= mixed::checkbox(ui, "contact_shadows", &mut l.contact_shadows, "").changed();
        ui.end_row();
        ch
    });
    super::card::sub_header(ui, "Volumetrics");
    ch |= grid(ui, "light_volumetric_grid", |ui| {
        super::fog_editors::light_rows(ui, l)
    });
    ch | super::glow_editors::halo_section(ui, l)
}

/// Mesh Renderer: material slot (pick from project materials, or drop one from the Project window).
fn mesh_renderer(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut r) = state
        .scene
        .world
        .get::<&MeshRenderer>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let names: Vec<String> = state.assets.materials.keys().cloned().collect();
    let mut changed = grid(ui, "meshrenderer_grid", |ui| {
        ui.label("Materials");
        let mut size = r.material_count();
        let resp = ui.add(egui::DragValue::new(&mut size).range(1..=8).prefix("Size "));
        if resp.changed() {
            r.set_material_count(size);
        }
        ui.end_row();
        resp.changed()
    });
    // Each element, then its per-object overrides.
    for i in 0..r.material_count() {
        changed |= grid(ui, &format!("meshrenderer_element_{i}"), |ui| {
            ui.label(format!("    Element {i}"));
            let ch = material_slot(ui, &mut r, i, &names);
            ui.end_row();
            ch
        });
        let material = state.assets.params(r.material_at(i));
        ui.indent(("mr_overrides_indent", i), |ui| {
            changed |= super::overrides::section(ui, &mut r, i, &material);
        });
    }
    changed |= grid(ui, "meshrenderer_shadows_grid", |ui| {
        let mut ch = false;
        ui.label("Cast Shadows");
        egui::ComboBox::from_id_salt("mr_cast")
            .selected_text(mixed::text(
                "cast_shadows",
                if r.cast_shadows { "On" } else { "Off" },
            ))
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, true, "On")
                    .changed();
                ch |= ui
                    .selectable_value(&mut r.cast_shadows, false, "Off")
                    .changed();
            });
        ui.end_row();
        ui.label("Receive Shadows");
        ch |= mixed::checkbox(ui, "receive_shadows", &mut r.receive_shadows, "").changed();
        ui.end_row();
        ui.label("Light Probes");
        egui::ComboBox::from_id_salt("mr_light_probes")
            .selected_text(mixed::text("light_probes", r.light_probes.label()))
            .show_ui(ui, |ui| {
                for u in misty_engine::light_probes::LightProbeUsage::ALL {
                    ch |= ui
                        .selectable_value(&mut r.light_probes, u, u.label())
                        .changed();
                }
            });
        ui.end_row();
        ch
    });
    let is_static = state.scene.game_object(e).is_static;
    changed |= super::lightmap_editors::renderer_section(ui, &mut r, is_static);
    if changed {
        let _ = state.scene.world.insert_one(e, r);
    }
    changed
}

/// Materials element `i`: pick from project materials, or drop one from the Project window.
fn material_slot(ui: &mut Ui, r: &mut MeshRenderer, i: usize, names: &[String]) -> bool {
    let mut ch = false;
    let mut value = r.material_at(i).map(str::to_owned);
    let current = AssetDatabase::material_name(value.as_deref());
    let key = if i == 0 {
        "material"
    } else {
        "extra_materials"
    };
    let slot = egui::ComboBox::from_id_salt(("mr_material", i))
        .selected_text(mixed::text(key, format!("◍ {current}")))
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(&mut value, None, crate::assets::DEFAULT_MATERIAL)
                .changed();
            for n in names {
                let label = AssetDatabase::material_name(Some(n));
                ch |= ui
                    .selectable_value(&mut value, Some(n.clone()), label)
                    .changed();
            }
        })
        .response;
    if let Some(path) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && path.0.ends_with(".mat")
    {
        value = Some(path.0.clone());
        ch = true;
    }
    if ch {
        r.set_material_at(i, value);
    }
    ch
}

/// File stem of a clip path ("Beep").
pub fn clip_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(super) fn slider(
    ui: &mut Ui,
    key: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.add(mixed::slider(key, egui::Slider::new(v, range)))
        .changed()
}

fn rigidbody(ui: &mut Ui, r: &mut Rigidbody) -> bool {
    let mut ch = grid(ui, "rigidbody_grid", |ui| {
        let mut ch = false;
        ui.label("Mass");
        ch |= ui
            .add(
                mixed::drag("mass", egui::DragValue::new(&mut r.mass))
                    .speed(0.05)
                    .range(1e-4..=1e6),
            )
            .changed();
        ui.end_row();
        ui.label("Drag");
        ch |= ui
            .add(
                mixed::drag("drag", egui::DragValue::new(&mut r.drag))
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ui.label("Angular Drag");
        ch |= ui
            .add(
                mixed::drag("angular_drag", egui::DragValue::new(&mut r.angular_drag))
                    .speed(0.01)
                    .range(0.0..=1e4),
            )
            .changed();
        ui.end_row();
        ui.label("Use Gravity");
        ch |= mixed::checkbox(ui, "use_gravity", &mut r.use_gravity, "").changed();
        ui.end_row();
        ui.label("Is Kinematic");
        ch |= mixed::checkbox(ui, "is_kinematic", &mut r.is_kinematic, "").changed();
        ui.end_row();
        ui.label("Interpolate");
        ch |= combo(
            ui,
            "rb_interpolate",
            &mut r.interpolation,
            &[Interpolation::None, Interpolation::Interpolate],
            |i| match i {
                Interpolation::None => "None",
                Interpolation::Interpolate => "Interpolate",
            },
        );
        ui.end_row();
        ui.label("Collision Detection");
        egui::ComboBox::from_id_salt("rb_ccd")
            .selected_text(mixed::text(
                "continuous",
                if r.continuous {
                    "Continuous"
                } else {
                    "Discrete"
                },
            ))
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut r.continuous, false, "Discrete")
                    .changed();
                ch |= ui
                    .selectable_value(&mut r.continuous, true, "Continuous")
                    .changed();
            });
        ui.end_row();
        ch
    });
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Constraints"))
        .id_salt("rb_constraints")
        .default_open(false)
        .show(ui, |ui| {
            for (label, key, flags) in [
                ("Freeze Position", "freeze_position", &mut r.freeze_position),
                ("Freeze Rotation", "freeze_rotation", &mut r.freeze_rotation),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized([110.0, 18.0], egui::Label::new(label));
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ch |= mixed::checkbox(ui, &format!("{key}/{i}"), &mut flags[i], *axis)
                            .changed();
                    }
                });
            }
        });
    ch
}

/// Is Trigger + friction/bounciness rows shared by every collider.
fn collider_common(
    ui: &mut Ui,
    id: &str,
    trigger: &mut bool,
    surface: &mut PhysicsSurface,
) -> bool {
    grid(ui, id, |ui| {
        let mut ch = false;
        ui.label("Is Trigger");
        ch |= mixed::checkbox(ui, "is_trigger", trigger, "").changed();
        ui.end_row();
        ui.label("Friction");
        ch |= ui
            .add(mixed::slider(
                "surface/friction",
                egui::Slider::new(&mut surface.friction, 0.0..=2.0),
            ))
            .changed();
        ui.end_row();
        ui.label("Bounciness");
        ch |= ui
            .add(mixed::slider(
                "surface/bounciness",
                egui::Slider::new(&mut surface.bounciness, 0.0..=1.0),
            ))
            .changed();
        ui.end_row();
        ch
    })
}

fn vec3_field(ui: &mut Ui, label: &str, key: &str, v: &mut Vec3, speed: f32) -> bool {
    let mut a = v.to_array();
    let ch = vec3_row_mixed(ui, label, &mut a, speed, mixed::axes(key));
    if ch {
        *v = Vec3::from(a);
    }
    ch
}

fn box_collider(ui: &mut Ui, c: &mut BoxCollider) -> bool {
    let mut ch = collider_common(ui, "box_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", "center", &mut c.center, 0.01);
    if vec3_field(ui, "Size", "size", &mut c.size, 0.01) {
        c.size = c.size.max(Vec3::ZERO);
        ch = true;
    }
    ch
}

fn sphere_collider(ui: &mut Ui, c: &mut SphereCollider) -> bool {
    let mut ch = collider_common(ui, "sphere_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", "center", &mut c.center, 0.01);
    ch |= grid(ui, "sphere_col_r", |ui| {
        ui.label("Radius");
        let r = ui
            .add(
                mixed::drag("radius", egui::DragValue::new(&mut c.radius))
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        r
    });
    ch
}

fn capsule_collider(ui: &mut Ui, c: &mut CapsuleCollider) -> bool {
    let mut ch = collider_common(ui, "capsule_col", &mut c.is_trigger, &mut c.surface);
    ch |= vec3_field(ui, "Center", "center", &mut c.center, 0.01);
    ch |= grid(ui, "capsule_col_dims", |ui| {
        let mut ch = false;
        ui.label("Radius");
        ch |= ui
            .add(
                mixed::drag("radius", egui::DragValue::new(&mut c.radius))
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        ui.label("Height");
        ch |= ui
            .add(
                mixed::drag("height", egui::DragValue::new(&mut c.height))
                    .speed(0.01)
                    .range(0.0..=1e5),
            )
            .changed();
        ui.end_row();
        ui.label("Direction");
        let names = ["X-Axis", "Y-Axis", "Z-Axis"];
        egui::ComboBox::from_id_salt("capsule_dir")
            .selected_text(mixed::text("direction", names[c.direction.min(2) as usize]))
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ch |= ui.selectable_value(&mut c.direction, i as u8, *n).changed();
                }
            });
        ui.end_row();
        ch
    });
    ch
}

fn mesh_collider(ui: &mut Ui, c: &mut MeshCollider) -> bool {
    let mut ch = grid(ui, "mesh_col_convex", |ui| {
        ui.label("Convex");
        let r = mixed::checkbox(ui, "convex", &mut c.convex, "").changed();
        ui.end_row();
        r
    });
    ch |= collider_common(ui, "mesh_col", &mut c.is_trigger, &mut c.surface);
    ui.weak("Uses the Mesh Filter's mesh.");
    ch
}

/// Wide "Add Component" button with a searchable popup of addable components.
pub fn add_component_button(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let resp = ui
        .vertical_centered(|ui| ui.add_sized([230.0, 24.0], egui::Button::new("Add Component")))
        .inner;
    crate::tutorials::targets::mark_response(ui, "inspector.add_component", &resp);
    if resp.clicked() {
        state.inspector_add_open = !state.inspector_add_open;
    }
    if !state.inspector_add_open {
        return;
    }

    let search_id = egui::Id::new("add_component_search");
    let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
    let present = state.scene.component_names(e);
    let mut chosen = None;
    let mut chosen_script: Option<String> = None;
    let mut new_script = false;
    let area = egui::Area::new(egui::Id::new("add_component_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 2.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(resp.rect.width());
                let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text("🔍 Search"));
                field.request_focus();
                ui.separator();
                // The list scrolls under the search field so it never runs off screen.
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let needle = search.to_lowercase();
                        let mut last_cat = "";
                        for info in registry::components()
                            .iter()
                            .filter(|c| !c.category.is_empty())
                        {
                            if !needle.is_empty() && !info.name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            if info.category != last_cat {
                                ui.label(
                                    RichText::new(info.category).color(theme::TEXT_DIM).small(),
                                );
                                last_cat = info.category;
                            }
                            let has = present.iter().any(|p| p == info.name);
                            if ui
                                .add_enabled(!has, egui::Button::new(info.name).frame(false))
                                .clicked()
                            {
                                chosen = Some(info.name);
                            }
                        }
                        // Project scripts, then "New script".
                        let scripts: Vec<String> = state
                            .assets
                            .scripts
                            .keys()
                            .filter(|p| needle.is_empty() || p.to_lowercase().contains(&needle))
                            .cloned()
                            .collect();
                        ui.label(RichText::new("Scripts").color(theme::TEXT_DIM).small());
                        for path in scripts {
                            let label = format!("📜 {}", AssetDatabase::material_name(Some(&path)));
                            if ui.add(egui::Button::new(label).frame(false)).clicked() {
                                chosen_script = Some(path);
                            }
                        }
                        if ui
                            .add(egui::Button::new("➕ New script").frame(false))
                            .clicked()
                        {
                            new_script = true;
                        }
                    });
            });
        });

    let clicked_outside = ui.input(|i| i.pointer.any_pressed())
        && !area.response.contains_pointer()
        && !resp.contains_pointer();
    if let Some(name) = chosen {
        state.push(Action::AddComponent(name));
    }
    if new_script {
        let folder = state.project.root.join("Assets").join("Scripts");
        let _ = std::fs::create_dir_all(&folder);
        match state.assets.create_script(&folder) {
            Ok(path) => {
                state.console.info(format!("Created {path}"));
                state.project.refresh();
                chosen_script = Some(path);
            }
            Err(err) => state
                .console
                .warn(format!("Could not create script: {err}")),
        }
    }
    let picked = chosen.is_some() || chosen_script.is_some();
    if let Some(path) = chosen_script {
        state.push(Action::AddScript { entity: e, path });
    }
    if picked || clicked_outside || ui.input(|i| i.key_pressed(Key::Escape)) {
        state.inspector_add_open = false;
        search.clear();
    }
    ui.data_mut(|d| d.insert_temp(search_id, search));
}

/// "moveSpeed" / "move_speed" → "Move Speed".
pub fn nicify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        if out.is_empty() || out.ends_with(' ') {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    out
}

/// Inspector sections for every script on `e`: "Name (Script)" with its public fields.
pub fn script_sections(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let Ok(scripts) = state.scene.world.get::<&Scripts>(e).map(|s| (*s).clone()) else {
        return;
    };
    for (slot, script) in scripts.list.iter().enumerate() {
        let id = ui.make_persistent_id(("script", slot, &script.path));
        let header = |ui: &mut Ui, state: &mut EditorState| {
            let mut on = script.enabled;
            if ui.checkbox(&mut on, "").changed() {
                if let Ok(mut s) = state.scene.world.get::<&mut Scripts>(e) {
                    s.list[slot].enabled = on;
                }
                let verb = if on { "Enable" } else { "Disable" };
                state.mark_edited(&format!("{verb} Script"));
            }
            ui.label("📜");
            let title =
                crate::theme::bold(ui.ctx(), format!("{} (Script)", nicify(&script.name())));
            let title = ui.add(egui::Label::new(title).sense(egui::Sense::click()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("⋮", |ui| {
                    if ui.button("Reset").clicked() {
                        state.push(Action::ResetScript { entity: e, slot });
                        ui.close();
                    }
                    if ui.button("Remove Component").clicked() {
                        state.push(Action::RemoveScript { entity: e, slot });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Edit Script").clicked() {
                        open_script(state, &script.path);
                        ui.close();
                    }
                    if ui.button("Open in External Editor").clicked() {
                        let abs = state.assets.absolute(&script.path);
                        if let Err(err) = crate::assets::open_external(&abs) {
                            state
                                .console
                                .warn(format!("Could not open {}: {err}", script.path));
                        }
                        ui.close();
                    }
                });
            });
            title
        };
        super::card::card(ui, id, state, header, |ui, state| {
            script_fields(ui, state, e, slot, &script.path)
        });
    }
}

fn script_fields(ui: &mut Ui, state: &mut EditorState, e: Entity, slot: usize, path: &str) {
    let meta = state.assets.scripts.get(path).cloned();
    let overrides = state
        .scene
        .world
        .get::<&Scripts>(e)
        .ok()
        .and_then(|s| s.list.get(slot).map(|r| r.fields.clone()))
        .unwrap_or_default();
    let mut edits: Vec<(String, ScriptValue)> = Vec::new();
    grid(ui, &format!("script_grid_{slot}"), |ui| {
        ui.label("Script");
        if ui
            .add(
                egui::Button::new(format!("📜 {}", AssetDatabase::material_name(Some(path))))
                    .frame(false),
            )
            .on_hover_text("Double-click to edit")
            .double_clicked()
        {
            open_script(state, path);
        }
        ui.end_row();
        match meta.as_ref().map(|m| &m.fields) {
            None => {
                ui.label(RichText::new("Missing script file").color(LogLevelColor::ERROR));
                ui.end_row();
            }
            Some(Err(err)) => {
                ui.label(RichText::new("Error").color(LogLevelColor::ERROR));
                ui.label(RichText::new(err).color(LogLevelColor::ERROR).small());
                ui.end_row();
            }
            Some(Ok(fields)) => {
                for (name, default) in fields {
                    let mut value = overrides
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| default.clone());
                    ui.label(nicify(name));
                    if script_value_ui(ui, &format!("{slot}_{name}"), &mut value) {
                        edits.push((name.clone(), value));
                    }
                    ui.end_row();
                }
            }
        }
        false
    });
    if !edits.is_empty() {
        let label = format!("Change {}", crate::undo_names::humanize(&edits[0].0));
        if let Ok(mut s) = state.scene.world.get::<&mut Scripts>(e) {
            for (k, v) in edits {
                s.list[slot].fields.insert(k, v);
            }
        }
        state.mark_edited(&label);
    }
}

/// Widget for one script field value.
fn script_value_ui(ui: &mut Ui, id: &str, v: &mut ScriptValue) -> bool {
    match v {
        ScriptValue::Bool(b) => ui.checkbox(b, "").changed(),
        ScriptValue::Number(n) => ui.add(egui::DragValue::new(n).speed(0.1)).changed(),
        ScriptValue::Text(t) => ui
            .add(egui::TextEdit::singleline(t).desired_width(150.0))
            .changed(),
        ScriptValue::Vector3(a) => {
            ui.push_id(id, |ui| {
                ui.horizontal(|ui| {
                    let mut ch = false;
                    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.label(*axis);
                        ch |= ui
                            .add(egui::DragValue::new(&mut a[i]).speed(0.05))
                            .changed();
                    }
                    ch
                })
                .inner
            })
            .inner
        }
    }
}

/// Open a script in the Code Editor (or the external editor chosen in Preferences).
pub fn open_script(state: &mut EditorState, rel: &str) {
    crate::panels::code_editor::open_script(state, rel, None);
}

/// Error colour for inline messages.
struct LogLevelColor;

impl LogLevelColor {
    const ERROR: egui::Color32 = egui::Color32::from_rgb(255, 110, 100);
}
