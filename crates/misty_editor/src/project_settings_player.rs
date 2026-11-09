//! Project Settings ▸ Player: product name, company, version, icon, splash screen and the
//! window the built game opens (screen mode, resolution, VSync, frame rate).

use std::path::Path;

use egui::Ui;
use misty_engine::build::BuildSettings;
use misty_engine::player_settings::{PlayerSettings, ScreenMode};
use misty_engine::settings::ProjectSettings;

use crate::project_settings::section;

/// A labelled row; `add` gets the label's id to name its control for screen readers.
fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui, egui::Id) -> bool) -> bool {
    ui.horizontal(|ui| {
        let l = ui
            .add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui, l.id)
    })
    .inner
}

/// Draw the page; true when something changed.
pub fn page(ui: &mut Ui, s: &mut ProjectSettings, build: &BuildSettings, root: &Path) -> bool {
    // Projects from before this page start from the old Build Settings values.
    let mut p = s.player_or(build);
    let mut edited = false;
    section(ui, "Identity", true, |ui| {
        edited |= identity(ui, &mut p, root)
    });
    section(ui, "Window", true, |ui| edited |= window(ui, &mut p));
    section(ui, "Splash Screen", false, |ui| {
        edited |= splash(ui, &mut p, root)
    });
    section(ui, "Accessibility", true, |ui| {
        edited |= accessibility(ui, &mut p)
    });
    if edited {
        s.player = Some(p);
    }
    edited
}

fn identity(ui: &mut Ui, p: &mut PlayerSettings, root: &Path) -> bool {
    let hint = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut edited = row(
        ui,
        "Product Name",
        "The game's name: window title, executable name and the save-data folder.",
        |ui, l| {
            ui.add(egui::TextEdit::singleline(&mut p.product_name).hint_text(hint))
                .labelled_by(l)
                .changed()
        },
    );
    edited |= row(
        ui,
        "Company Name",
        "Your studio's name; saved data goes in <Company>/<Product> in the user's folder.",
        |ui, l| {
            ui.text_edit_singleline(&mut p.company_name)
                .labelled_by(l)
                .changed()
        },
    );
    edited |= row(
        ui,
        "Version",
        "Shown to players and in store builds, e.g. 1.0 or 0.3.2.",
        |ui, l| {
            ui.text_edit_singleline(&mut p.version)
                .labelled_by(l)
                .changed()
        },
    );
    edited |= row(
        ui,
        "Icon",
        "A square PNG from Assets (256×256 or larger is best). Used for the .exe and window.",
        |ui, l| image_picker(ui, "player_icon", &mut p.icon, root, l),
    );
    edited
}

fn window(ui: &mut Ui, p: &mut PlayerSettings) -> bool {
    let mut edited = row(
        ui,
        "Screen Mode",
        "Fullscreen Window: borderless, fast to Alt+Tab (recommended).\n\
         Exclusive Fullscreen: the monitor switches to the game's resolution.\n\
         Windowed / Maximized: a normal window.",
        |ui, l| {
            let mut ch = false;
            egui::ComboBox::from_id_salt("player_screen_mode")
                .selected_text(p.screen_mode.label())
                .width(170.0)
                .show_ui(ui, |ui| {
                    for m in ScreenMode::ALL {
                        ch |= ui
                            .selectable_value(&mut p.screen_mode, m, m.label())
                            .changed();
                    }
                })
                .response
                .labelled_by(l);
            ch
        },
    );
    edited |= row(
        ui,
        "Default Resolution",
        "Window size in Windowed mode, and the resolution Exclusive Fullscreen switches to.",
        |ui, l| {
            let w = ui
                .add(egui::DragValue::new(&mut p.width).range(64..=16384))
                .labelled_by(l);
            ui.label("×");
            let h = ui
                .add(egui::DragValue::new(&mut p.height).range(64..=16384))
                .labelled_by(l);
            w.changed() || h.changed()
        },
    );
    edited |= row(
        ui,
        "Resizable Window",
        "Players can drag the window's edges.",
        |ui, _| crate::a11y::checkbox(ui, &mut p.resizable, "Resizable Window").changed(),
    );
    edited |= row(
        ui,
        "Run In Background",
        "Keep playing when the window loses focus (otherwise the game pauses).",
        |ui, _| crate::a11y::checkbox(ui, &mut p.run_in_background, "Run In Background").changed(),
    );
    edited |= row(
        ui,
        "VSync",
        "Match the monitor's refresh rate: no tearing, steady frame times.",
        |ui, _| crate::a11y::checkbox(ui, &mut p.vsync, "VSync").changed(),
    );
    edited |= row(
        ui,
        "Target Frame Rate",
        "Frame cap when VSync is off. -1 = as fast as possible.",
        |ui, l| {
            ui.add_enabled_ui(!p.vsync, |ui| {
                ui.add(egui::DragValue::new(&mut p.target_frame_rate).range(-1..=1000))
                    .labelled_by(l)
                    .changed()
            })
            .inner
        },
    );
    edited
}

fn splash(ui: &mut Ui, p: &mut PlayerSettings, root: &Path) -> bool {
    let sp = &mut p.splash;
    let mut edited = row(
        ui,
        "Show Splash Screen",
        "Show an image (your logo) before the first scene.",
        |ui, _| crate::a11y::checkbox(ui, &mut sp.enabled, "Show Splash Screen").changed(),
    );
    ui.add_enabled_ui(sp.enabled, |ui| {
        edited |= row(ui, "Image", "PNG or JPG from Assets.", |ui, l| {
            image_picker(ui, "player_splash", &mut sp.image, root, l)
        });
        edited |= row(ui, "Duration", "Seconds the splash stays up.", |ui, l| {
            ui.add(
                egui::DragValue::new(&mut sp.duration)
                    .speed(0.1)
                    .range(0.0..=30.0)
                    .suffix(" s"),
            )
            .labelled_by(l)
            .changed()
        });
        edited |= row(ui, "Background", "Colour around the image.", |ui, l| {
            ui.color_edit_button_rgb(&mut sp.background)
                .labelled_by(l)
                .changed()
        });
    });
    edited
}

/// UI text scale and the colour-blind filter the game starts with (the Game view previews them).
fn accessibility(ui: &mut Ui, p: &mut PlayerSettings) -> bool {
    use misty_engine::accessibility::{ColorBlindMode, MAX_TEXT_SCALE, MIN_TEXT_SCALE};
    let a = &mut p.accessibility;
    let mut edited = row(
        ui,
        "UI Text Scale",
        "Multiplies every UI Text's font size. Scripts change it with Accessibility.textScale.",
        |ui, l| {
            let range = MIN_TEXT_SCALE..=MAX_TEXT_SCALE;
            let slider = ui
                .add(
                    egui::Slider::new(&mut a.text_scale, range.clone())
                        .step_by(0.05)
                        .show_value(false),
                )
                .labelled_by(l);
            let value = ui
                .add(
                    egui::DragValue::new(&mut a.text_scale)
                        .range(range)
                        .speed(0.01)
                        .suffix("×"),
                )
                .labelled_by(l);
            slider.changed() || value.changed()
        },
    );
    edited |= row(
        ui,
        "Colour-blind Filter",
        "Recolours the final image (UI included) so colours a colour-blind player confuses \
         stay apart. Scripts change it with Accessibility.colorBlindMode.",
        |ui, l| {
            let mut ch = false;
            egui::ComboBox::from_id_salt("player_color_filter")
                .selected_text(a.color_filter.label())
                .width(170.0)
                .show_ui(ui, |ui| {
                    for m in ColorBlindMode::ALL {
                        ch |= ui
                            .selectable_value(&mut a.color_filter, m, m.label())
                            .changed();
                    }
                })
                .response
                .labelled_by(l);
            ch
        },
    );
    let on = a.color_filter != ColorBlindMode::Off;
    ui.add_enabled_ui(on, |ui| {
        edited |= row(
            ui,
            "Filter Strength",
            "0 = no change, 1 = the full filter.",
            |ui, l| {
                let slider = ui
                    .add(egui::Slider::new(&mut a.filter_strength, 0.0..=1.0).show_value(false))
                    .labelled_by(l);
                let value = ui
                    .add(
                        egui::DragValue::new(&mut a.filter_strength)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    )
                    .labelled_by(l);
                slider.changed() || value.changed()
            },
        );
        edited |= row(
            ui,
            "Simulate",
            "Show how a colour-blind player sees the game instead of correcting it (for \
             checking your colours).",
            |ui, _| crate::a11y::checkbox(ui, &mut a.simulate, "Simulate").changed(),
        );
    });
    edited
}

/// Dropdown of the project's PNG/JPG files (listed when opened); `None` = no image.
pub fn image_picker(
    ui: &mut Ui,
    id: &str,
    value: &mut Option<String>,
    root: &Path,
    label: egui::Id,
) -> bool {
    let mut ch = false;
    let text = value.as_deref().unwrap_or("None");
    egui::ComboBox::from_id_salt(id)
        .selected_text(text.rsplit('/').next().unwrap_or(text))
        .width(200.0)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, "None").changed();
            for path in misty_engine::vfs::files_under(&root.join("Assets")) {
                let Ok(rel) = path.strip_prefix(root) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if misty_engine::assets::is_texture(&rel) {
                    let name = rel.trim_start_matches("Assets/").to_owned();
                    ch |= ui.selectable_value(value, Some(rel), name).changed();
                }
            }
        })
        .response
        .labelled_by(label);
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::accessibility::ColorBlindMode;

    use super::*;

    type State = (ProjectSettings, BuildSettings, std::path::PathBuf);

    #[test]
    fn accessibility_section_edits_the_player_settings_and_controls_are_named() {
        let root = std::env::temp_dir().join(format!("misty_player_a11y_{}", std::process::id()));
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let state: State = (
            ProjectSettings::default(),
            BuildSettings::default(),
            root.clone(),
        );
        let mut h = Harness::builder().with_size([720.0, 900.0]).build_ui_state(
            |ui, (s, b, r): &mut State| {
                page(ui, s, b, r);
            },
            state,
        );
        h.run_steps(3);
        // Drawing the page changes nothing by itself (no save every frame).
        assert!(
            h.state().0.player.is_none(),
            "the page edited the settings unasked"
        );
        // Every control on the page has a name for screen readers.
        let unnamed = crate::a11y::unnamed_controls(&h);
        assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
        h.get_by_label("Colour-blind Filter").click();
        h.run();
        h.get_by_label("Deuteranopia").click();
        h.run_steps(2);
        let a = h.state().0.player.as_ref().unwrap().accessibility;
        assert_eq!(a.color_filter, ColorBlindMode::Deuteranopia);
        // The strength and simulate rows are enabled now.
        h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Simulate")
            .click();
        h.run();
        assert!(h.state().0.player.as_ref().unwrap().accessibility.simulate);
        std::fs::remove_dir_all(root).ok();
    }
}
