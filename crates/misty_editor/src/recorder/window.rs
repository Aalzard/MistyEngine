//! Window ▸ General ▸ Recorder: the project's recorders on the left (on / off, name), the
//! selected one's settings on the right, Start / Stop Recording on top.

use egui::{DragValue, RichText, Ui};
use misty_engine::recorder::{
    FRAME_RATES, OutputFormat, PatternContext, RangeMode, RecorderSettings, Source, WILDCARDS,
    effective_pattern, expand_pattern,
};

use super::{installed, request_start, request_stop};
use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::panel_toolbar;

pub const ICON: &str = "⏺";

/// Sizes offered next to the custom width and height.
const PRESETS: [(u32, u32); 5] = [
    (1280, 720),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
    (1080, 1920),
];

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    if !installed(s) {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(tr!("The Recorder package is not installed."));
            ui.weak(tr!(
                "Install it from Window ▸ Package Manager ▸ Built-in ▸ Recorder."
            ));
            if ui.button(tr!("Open Package Manager")).clicked() {
                s.push(crate::actions::Action::FocusTab(
                    crate::layout::Tab::PackageManager,
                ));
            }
        });
        return;
    }
    let project = s.project.root.clone();
    let recording = s.recorder.recording();
    let mut changed = false;
    panel_toolbar(ui, |ui| {
        if recording {
            let stop = egui::Button::new(
                RichText::new(format!("■ {}", tr!("Stop Recording"))).color(theme::warn()),
            );
            if ui.add(stop).clicked() {
                request_stop(s);
            }
        } else if ui
            .button(format!("⏺ {}", tr!("Start Recording")))
            .on_hover_text(tr!(
                "Record every enabled recorder. Scene Viewer and Camera sources enter Play mode."
            ))
            .clicked()
        {
            request_start(s, None);
        }
        ui.add_enabled_ui(!recording, |ui| {
            ui.menu_button(format!("＋ {} ⏷", tr!("Add Recorder")), |ui| {
                for f in OutputFormat::ALL {
                    if ui.button(tr(f.label())).clicked() {
                        let list = s.recorder.list(&project);
                        list.recorders.push(RecorderSettings::new(f));
                        s.recorder.selected = list.recorders.len() - 1;
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
        status_line(ui, s);
    });
    if let Some((err, msg, folder)) = s.recorder.status.clone() {
        ui.horizontal(|ui| {
            let color = if err {
                theme::warn()
            } else {
                theme::text_dim()
            };
            ui.label(RichText::new(msg).color(color));
            if let Some(f) = folder.filter(|f| f.exists())
                && ui.small_button(tr!("Show in Explorer")).clicked()
            {
                let _ = crate::explorer::show(&f, crate::explorer::Reveal::OpenFolders);
            }
        });
    }
    let cameras = super::camera_names(&s.scene);
    let selected = s.recorder.selected;
    let list = s.recorder.list(&project);
    let mut remove = None;
    let mut duplicate = None;
    let mut pick = None;
    egui::Panel::left("recorder_list")
        .resizable(true)
        .default_size(200.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    if list.recorders.is_empty() {
                        ui.weak(tr!("No recorders. Use Add Recorder."));
                    }
                    for (i, r) in list.recorders.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!recording, |ui| {
                                changed |= ui
                                    .checkbox(&mut r.enabled, "")
                                    .on_hover_text(tr!("Record with Start Recording"))
                                    .changed();
                            });
                            let resp = ui.selectable_label(i == selected, &r.name);
                            if resp.clicked() {
                                pick = Some(i);
                            }
                            resp.context_menu(|ui| {
                                if ui.button(tr!("Duplicate")).clicked() {
                                    duplicate = Some(i);
                                    ui.close();
                                }
                                if ui.button(tr!("Delete")).clicked() {
                                    remove = Some(i);
                                    ui.close();
                                }
                            });
                            ui.weak(r.format.extension().to_uppercase());
                        });
                    }
                });
        });
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| match list.recorders.get_mut(selected) {
                Some(r) => {
                    ui.add_enabled_ui(!recording, |ui| {
                        changed |= settings_ui(ui, &cameras, r, "recorder_settings", false);
                    });
                }
                None => {
                    ui.weak(tr!("Select a recorder to see its settings."));
                }
            });
    });
    if let Some(i) = pick {
        s.recorder.selected = i;
    }
    let list = s.recorder.list(&project);
    if let Some(i) = duplicate.filter(|i| *i < list.recorders.len()) {
        let mut copy = list.recorders[i].clone();
        copy.name = format!("{} (copy)", copy.name);
        list.recorders.insert(i + 1, copy);
        changed = true;
    }
    if let Some(i) = remove.filter(|i| *i < list.recorders.len()) {
        list.recorders.remove(i);
        changed = true;
    }
    let len = list.recorders.len();
    if changed {
        if let Err(e) = list.save(&project) {
            s.console.warn(format!("Recorder settings: {e}"));
        }
        s.recorder.selected = s.recorder.selected.min(len.saturating_sub(1));
    }
}

/// "Recording: frame 120 / 300" while recording, "Writing files…" after.
fn status_line(ui: &mut Ui, s: &EditorState) {
    if let Some(a) = s.recorder.active.first() {
        let r = &a.recording;
        let text = match r.total() {
            Some(t) => tr!(
                "Recording: frame {frame} of {total}",
                frame = r.requested(),
                total = t
            ),
            None => tr!("Recording: frame {frame}", frame = r.requested()),
        };
        ui.label(RichText::new(format!("● {text}")).color(theme::warn()));
    } else if s.recorder.finishing() > 0 {
        ui.label(tr!("Writing files…"));
    }
}

/// "Manual" here means by hand, not the documentation.
fn range_label(r: RangeMode) -> String {
    match r {
        RangeMode::Manual => tr!("Manual#range"),
        other => tr(other.label()),
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(tr(label));
    add(ui);
    ui.end_row();
}

/// One recorder's settings (the window and Timeline Recorder clips, `clip` = no start / stop
/// or take settings: the clip is the range). Returns true when something changed.
pub fn settings_ui(
    ui: &mut Ui,
    cameras: &[String],
    r: &mut RecorderSettings,
    id: &str,
    clip: bool,
) -> bool {
    let before = r.clone();
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            row(ui, "Name", |ui| {
                ui.text_edit_singleline(&mut r.name);
            });
            row(ui, "Source", |ui| {
                egui::ComboBox::from_id_salt((id, "source"))
                    .selected_text(tr(r.source.label()))
                    .show_ui(ui, |ui| {
                        for v in Source::ALL {
                            ui.selectable_value(&mut r.source, v, tr(v.label()));
                        }
                    });
            });
            if r.source == Source::Camera {
                row(ui, "Camera", |ui| {
                    let shown = if r.camera.is_empty() {
                        tr!("(pick a camera)")
                    } else {
                        r.camera.clone()
                    };
                    egui::ComboBox::from_id_salt((id, "camera"))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            if cameras.is_empty() {
                                ui.weak(tr!("No cameras in the scene."));
                            }
                            for c in cameras {
                                ui.selectable_value(&mut r.camera, c.clone(), c);
                            }
                        });
                });
            }
            row(ui, "Output", |ui| {
                egui::ComboBox::from_id_salt((id, "format"))
                    .selected_text(tr(r.format.label()))
                    .show_ui(ui, |ui| {
                        for v in OutputFormat::ALL {
                            ui.selectable_value(&mut r.format, v, tr(v.label()));
                        }
                    });
            });
            if matches!(
                r.format,
                OutputFormat::Mp4 | OutputFormat::WebM | OutputFormat::JpgSequence
            ) {
                row(ui, "Quality", |ui| {
                    ui.add(egui::Slider::new(&mut r.quality, 1..=100));
                });
            }
            if r.format.is_video() {
                row(ui, "Capture Audio", |ui| {
                    r.audio = false;
                    ui.add_enabled(false, egui::Checkbox::new(&mut r.audio, ""))
                        .on_disabled_hover_text(tr!(
                            "Sound is not recorded yet: videos are silent."
                        ));
                });
            }
            row(ui, "Resolution", |ui| {
                ui.vertical(|ui| {
                    let label = match r.source {
                        Source::EditorScene => tr!("Match Editor Scene"),
                        _ => tr!("Match Scene Viewer"),
                    };
                    ui.checkbox(&mut r.match_game, label);
                    if !r.match_game {
                        ui.horizontal(|ui| {
                            ui.add(DragValue::new(&mut r.width).range(2..=8192).suffix(" px"));
                            ui.label("×");
                            ui.add(DragValue::new(&mut r.height).range(2..=8192).suffix(" px"));
                            ui.menu_button("⏷", |ui| {
                                for (w, h) in PRESETS {
                                    if ui.button(format!("{w} × {h}")).clicked() {
                                        r.width = w;
                                        r.height = h;
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
            });
            row(ui, "Frame Rate", |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt((id, "fps"))
                        .selected_text(format!("{} fps", r.frame_rate()))
                        .show_ui(ui, |ui| {
                            for f in FRAME_RATES {
                                ui.selectable_value(&mut r.fps, f, format!("{f} fps"));
                            }
                        });
                    ui.add(DragValue::new(&mut r.fps).range(1..=240));
                });
            });
            if r.format == OutputFormat::Gif && 100 % r.frame_rate() != 0 {
                ui.label("");
                ui.weak(tr!(
                    "GIF frames last whole hundredths of a second: delays alternate to keep the rate."
                ));
                ui.end_row();
            }
            if !clip {
                row(ui, "Start / Stop", |ui| {
                    egui::ComboBox::from_id_salt((id, "range"))
                        .selected_text(range_label(r.range))
                        .show_ui(ui, |ui| {
                            for v in RangeMode::ALL {
                                ui.selectable_value(&mut r.range, v, range_label(v));
                            }
                        });
                });
                match r.range {
                    RangeMode::Manual => {}
                    RangeMode::FrameRange => {
                        row(ui, "Frames", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(DragValue::new(&mut r.start_frame).prefix("▶ "));
                                ui.add(DragValue::new(&mut r.end_frame).prefix("■ "));
                            });
                        });
                        r.end_frame = r.end_frame.max(r.start_frame);
                    }
                    RangeMode::TimeRange => {
                        row(ui, "Seconds", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    DragValue::new(&mut r.start_time)
                                        .speed(0.05)
                                        .range(0.0..=86_400.0)
                                        .prefix("▶ ")
                                        .suffix(" s"),
                                );
                                ui.add(
                                    DragValue::new(&mut r.end_time)
                                        .speed(0.05)
                                        .range(0.0..=86_400.0)
                                        .prefix("■ ")
                                        .suffix(" s"),
                                );
                            });
                        });
                        r.end_time = r.end_time.max(r.start_time);
                    }
                    RangeMode::TimelineClip => {
                        ui.label("");
                        ui.weak(tr!(
                            "Add a Recorder track to a Timeline: each clip records while it plays."
                        ));
                        ui.end_row();
                    }
                }
                if let Some((a, b)) = r.frame_range() {
                    ui.label("");
                    ui.weak(tr!(
                        "{count} frames ({first} to {last})",
                        count = b - a + 1,
                        first = a,
                        last = b
                    ));
                    ui.end_row();
                }
            }
            if r.source != Source::EditorScene {
                row(ui, "Include UI", |ui| {
                    ui.checkbox(&mut r.include_ui, "");
                });
            }
            row(ui, "Output Folder", |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut r.folder).desired_width(180.0));
                    if ui.button("…").on_hover_text(tr!("Choose a folder")).clicked()
                        && let Some(dir) = rfd::FileDialog::new().pick_folder()
                    {
                        r.folder = dir.to_string_lossy().into_owned();
                    }
                });
            });
            row(ui, "File Name", |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut r.pattern).desired_width(180.0));
                    ui.menu_button("<>", |ui| {
                        for w in WILDCARDS {
                            if ui.button(w).clicked() {
                                r.pattern.push_str(w);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr!("Add a wildcard"));
                });
            });
            ui.label("");
            let ctx = PatternContext {
                scene: "Scene",
                recorder: &r.name,
                project: "Project",
                take: r.take,
                width: r.width,
                height: r.height,
                source: r.source.label(),
            };
            let pattern = effective_pattern(&r.pattern, r.format);
            let frame = r.format.is_sequence().then_some(0);
            ui.weak(format!(
                "{}.{}",
                expand_pattern(&pattern, &ctx, frame),
                r.format.extension()
            ));
            ui.end_row();
            row(ui, "Take", |ui| {
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut r.take).range(0..=99_999));
                    ui.checkbox(&mut r.auto_increment, tr!("Auto-increment"));
                });
            });
        });
    *r != before
}
