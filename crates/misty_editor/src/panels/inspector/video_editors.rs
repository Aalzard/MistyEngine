//! Inspector editors for video: the Video Player component (source, playback, render mode,
//! aspect ratio, audio output) with a preview and scrub bar, and a Video Clip asset (info,
//! import settings, the "unsupported format" message, transcoding to Motion-JPEG AVI).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{DragValue, RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::video::{
    VideoAspectRatio, VideoAudioOutput, VideoImporter, VideoMaterialProperty, VideoPlayer,
    VideoRenderMode, VideoSource, VideoSourceKind,
};

use super::card::{grid, grid_sub_header};
use crate::state::EditorState;
use crate::theme;

pub const ICON: &str = "🎞";

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Video Player"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Video Player").then_some(ICON)
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&VideoPlayer>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut VideoPlayer>(e)
        .map(|mut c| c.enabled = on);
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    v: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*v))
        .show_ui(ui, |ui| {
            for a in all {
                ch |= ui.selectable_value(v, *a, label(*a)).changed();
            }
        });
    ch
}

/// A scene object field limited to objects with component `need` ("" = any).
fn object_field(
    ui: &mut Ui,
    state: &EditorState,
    id: &str,
    v: &mut Option<Entity>,
    need: &str,
    none: &str,
) -> bool {
    let fits = |o: Entity| {
        need.is_empty()
            || misty_engine::scene::registry::find(need)
                .is_some_and(|info| (info.has)(&state.scene.world, o))
    };
    let objects: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| o)
        .filter(|o| fits(*o))
        .map(|o| (o, state.scene.name(o)))
        .collect();
    let name = v
        .and_then(|b| objects.iter().find(|(o, _)| *o == b))
        .map_or(none.to_owned(), |(_, n)| n.clone());
    let before = *v;
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .show_ui(ui, |ui| {
            ui.selectable_value(v, None, none);
            for (o, n) in &objects {
                ui.selectable_value(v, Some(*o), n);
            }
        });
    *v != before
}

/// Video Clip asset picker: project video files, or one dragged from the Project window.
fn clip_slot(ui: &mut Ui, list: &[String], value: &mut Option<String>) -> bool {
    let mut ch = false;
    let none = "None (Video Clip)";
    let current = value.as_deref().map(misty_engine::scene::clip_name);
    let slot = egui::ComboBox::from_id_salt("video_clip")
        .selected_text(format!("{ICON} {}", current.as_deref().unwrap_or(none)))
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, none).changed();
            for t in list {
                ch |= ui
                    .selectable_value(value, Some(t.clone()), misty_engine::scene::clip_name(t))
                    .changed();
            }
        })
        .response;
    if let Some(p) = slot.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::video::is_video_like(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// Video Player component.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut v) = state
        .scene
        .world
        .get::<&VideoPlayer>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let clips: Vec<String> = state.assets.videos.iter().cloned().collect();
    let changed = grid(ui, "video_player_grid", |ui| {
        let mut ch = false;
        ui.label("Source");
        ch |= combo(
            ui,
            "video_source",
            &mut v.source,
            &[VideoSourceKind::VideoClip, VideoSourceKind::Url],
            |s| match s {
                VideoSourceKind::VideoClip => "Video Clip",
                VideoSourceKind::Url => "URL",
            },
        );
        ui.end_row();
        match v.source {
            VideoSourceKind::VideoClip => {
                ui.label("Video Clip");
                ch |= clip_slot(ui, &clips, &mut v.clip);
            }
            VideoSourceKind::Url => {
                ui.label("URL");
                ch |= ui
                    .add(egui::TextEdit::singleline(&mut v.url).hint_text("file path"))
                    .on_hover_text("A local file: absolute, project-relative or file://")
                    .changed();
            }
        }
        ui.end_row();
        ui.label("Play On Awake");
        ch |= ui.checkbox(&mut v.play_on_awake, "").changed();
        ui.end_row();
        ui.label("Wait For First Frame");
        ch |= ui.checkbox(&mut v.wait_for_first_frame, "").changed();
        ui.end_row();
        ui.label("Loop");
        ch |= ui.checkbox(&mut v.looping, "").changed();
        ui.end_row();
        ui.label("Skip On Drop");
        ch |= ui.checkbox(&mut v.skip_on_drop, "").changed();
        ui.end_row();
        ui.label("Playback Speed");
        ch |= ui
            .add(egui::Slider::new(&mut v.playback_speed, 0.01..=10.0).logarithmic(true))
            .changed();
        ui.end_row();
        ui.label("Render Mode");
        ch |= combo(
            ui,
            "video_render_mode",
            &mut v.render_mode,
            &VideoRenderMode::ALL,
            VideoRenderMode::label,
        );
        ui.end_row();
        match v.render_mode {
            VideoRenderMode::CameraFarPlane | VideoRenderMode::CameraNearPlane => {
                ui.label("Camera");
                ch |= object_field(
                    ui,
                    state,
                    "video_camera",
                    &mut v.target_camera,
                    "Camera",
                    "Game camera",
                );
                ui.end_row();
                ui.label("Alpha");
                ch |= ui
                    .add(egui::Slider::new(&mut v.target_camera_alpha, 0.0..=1.0))
                    .changed();
                ui.end_row();
                ui.label("Aspect Ratio");
                ch |= combo(
                    ui,
                    "video_aspect",
                    &mut v.aspect_ratio,
                    &VideoAspectRatio::ALL,
                    VideoAspectRatio::label,
                );
                ui.end_row();
            }
            VideoRenderMode::RenderTexture => {
                ui.label("Target Texture");
                ch |= ui
                    .add(egui::TextEdit::singleline(&mut v.target_texture).hint_text("name"))
                    .on_hover_text(
                        "Frames are published as runtime://<name>: use that as a Raw Image or \
                         material texture",
                    )
                    .changed();
                ui.end_row();
            }
            VideoRenderMode::MaterialOverride => {
                ui.label("Renderer");
                ch |= object_field(
                    ui,
                    state,
                    "video_renderer",
                    &mut v.target_renderer,
                    "Mesh Renderer",
                    "This object",
                );
                ui.end_row();
                ui.label("Material Property");
                ch |= combo(
                    ui,
                    "video_property",
                    &mut v.target_material_property,
                    &[
                        VideoMaterialProperty::MainTexture,
                        VideoMaterialProperty::EmissionMap,
                    ],
                    VideoMaterialProperty::label,
                );
                ui.end_row();
                ui.label("Material Element");
                ch |= ui
                    .add(DragValue::new(&mut v.target_material_index).range(0..=31))
                    .changed();
                ui.end_row();
            }
            VideoRenderMode::ApiOnly => {}
        }
        grid_sub_header(ui, "Audio");
        ui.label("Audio Output Mode");
        ch |= combo(
            ui,
            "video_audio",
            &mut v.audio_output,
            &VideoAudioOutput::ALL,
            VideoAudioOutput::label,
        );
        ui.end_row();
        match v.audio_output {
            VideoAudioOutput::AudioSource => {
                ui.label("Audio Source");
                ch |= object_field(
                    ui,
                    state,
                    "video_audio_source",
                    &mut v.audio_source,
                    "Audio Source",
                    "This object",
                );
                ui.end_row();
            }
            VideoAudioOutput::Direct => {
                ui.label("Volume");
                ch |= ui
                    .add(egui::Slider::new(&mut v.direct_volume, 0.0..=1.0))
                    .changed();
                ui.end_row();
            }
            VideoAudioOutput::None => {}
        }
        if v.audio_output != VideoAudioOutput::None {
            ui.label("Mute");
            ch |= ui.checkbox(&mut v.mute, "").changed();
            ui.end_row();
        }
        ch
    });
    if let Some(file) = v.file() {
        let abs = if std::path::Path::new(&file).is_absolute() {
            std::path::PathBuf::from(&file)
        } else {
            state.assets.absolute(&file)
        };
        ui.add_space(4.0);
        preview(ui, &file, &abs);
    }
    if changed {
        let _ = state.scene.world.insert_one(e, v);
    }
    changed
}

/// An Inspector preview of one clip: the decoded source, the frame on screen and the scrub
/// position.
struct Preview {
    source: Result<Arc<VideoSource>, String>,
    texture: Option<egui::TextureHandle>,
    shown: Option<u64>,
    time: f64,
    playing: bool,
}

thread_local! {
    static PREVIEWS: RefCell<HashMap<std::path::PathBuf, Preview>> = RefCell::new(HashMap::new());
}

/// Frame preview with ▶ / ❚❚ and a scrub bar (decodes on demand on the UI thread).
fn preview(ui: &mut Ui, key: &str, abs: &std::path::Path) {
    PREVIEWS.with(|cell| {
        let mut map = cell.borrow_mut();
        let p = map.entry(abs.to_owned()).or_insert_with(|| Preview {
            source: VideoSource::open_file(abs).map(Arc::new),
            texture: None,
            shown: None,
            time: 0.0,
            playing: false,
        });
        let src = match &p.source {
            Ok(s) => s.clone(),
            Err(e) => {
                ui.colored_label(theme::WARN, e);
                if ui.small_button("Retry").clicked() {
                    p.source = VideoSource::open_file(abs).map(Arc::new);
                }
                return;
            }
        };
        let info = src.info().clone();
        if p.playing {
            p.time += ui.input(|i| i.stable_dt.min(0.1)) as f64;
            if p.time >= info.length {
                p.time = 0.0;
            }
            ui.ctx().request_repaint();
        }
        let frame = src.frame_at(p.time);
        if p.shown != Some(frame) {
            match src.frame(frame) {
                Ok(img) => {
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    match &mut p.texture {
                        Some(t) => t.set(color, egui::TextureOptions::LINEAR),
                        None => {
                            p.texture = Some(ui.ctx().load_texture(
                                format!("video preview {key}"),
                                color,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                    }
                }
                Err(e) => {
                    ui.colored_label(theme::WARN, e);
                }
            }
            p.shown = Some(frame);
        }
        if let Some(t) = &p.texture {
            let w = ui.available_width().min(320.0);
            let h = w * info.height as f32 / info.width.max(1) as f32;
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(w, h)));
        }
        ui.horizontal(|ui| {
            let label = if p.playing { "❚❚" } else { "▶" };
            if ui.button(label).on_hover_text("Preview playback").clicked() {
                p.playing = !p.playing;
            }
            let mut t = p.time as f32;
            let len = info.length.max(0.001) as f32;
            let r = ui.add(
                egui::Slider::new(&mut t, 0.0..=len)
                    .show_value(false)
                    .trailing_fill(true),
            );
            if r.changed() {
                p.time = t as f64;
            }
            ui.label(
                RichText::new(format!(
                    "{:.2} / {:.2} s · frame {}",
                    p.time, info.length, frame
                ))
                .color(theme::TEXT_DIM),
            );
        });
    });
}

fn describe(info: &misty_engine::video::VideoInfo) -> String {
    let audio = match info.audio {
        Some((rate, ch)) => format!("{rate} Hz {}", if ch == 1 { "mono" } else { "stereo" }),
        None => "no audio".into(),
    };
    format!(
        "{} · {}×{} · {:.2} fps · {} frames · {:.2} s · {audio}",
        info.codec, info.width, info.height, info.fps, info.frame_count, info.length
    )
}

/// Video Clip asset: what it is, its import settings and a preview — or why it can't play.
pub fn video_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(ICON).size(22.0));
        ui.label(theme::bold(
            ui.ctx(),
            format!("{} (Video Clip)", misty_engine::scene::clip_name(rel)),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let abs = state.assets.absolute(rel);
    let ext = abs
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(msg) = misty_engine::video::unsupported_message(&ext) {
        ui.colored_label(theme::WARN, msg);
        return;
    }
    let mut imp = VideoImporter::load(&abs);
    let before = imp.clone();
    let opened = VideoSource::open_file(&abs);
    match &opened {
        Ok(src) => {
            ui.label(describe(src.info()));
        }
        Err(e) => {
            ui.colored_label(theme::WARN, e);
        }
    }
    grid(ui, "video_import_grid", |ui| {
        ui.label("Import Audio");
        ui.checkbox(&mut imp.import_audio, "");
        ui.end_row();
        ui.label("Transcode Quality");
        ui.add(egui::Slider::new(&mut imp.transcode_quality, 1..=100));
        ui.end_row();
    });
    if imp != before
        && let Err(e) = imp.save(&abs)
    {
        state
            .console
            .warn(format!("Could not save import settings: {e}"));
    }
    if let Ok(src) = &opened {
        let is_mjpeg = src.info().codec == "Motion JPEG";
        if !is_mjpeg
            && ui
                .button("Transcode to Motion-JPEG AVI")
                .on_hover_text("Write a .avi next to this clip (pure Rust re-encode)")
                .clicked()
        {
            let out = abs.with_extension("avi");
            let out = if out == abs {
                abs.with_file_name(format!(
                    "{} (MJPEG).avi",
                    abs.file_stem().unwrap_or_default().to_string_lossy()
                ))
            } else {
                out
            };
            match misty_engine::video::transcode(src, imp.transcode_quality)
                .and_then(|bytes| std::fs::write(&out, bytes).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    state
                        .console
                        .info(format!("Transcoded {rel} → {}", out.display()));
                    state.assets.refresh();
                }
                Err(e) => state.console.warn(format!("Transcoding {rel} failed: {e}")),
            }
        }
        ui.add_space(4.0);
        preview(ui, rel, &abs);
    }
}
