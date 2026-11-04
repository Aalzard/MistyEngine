//! Project Settings ▸ Player: product name, company, version, icon, splash screen and the
//! window the built game opens (screen mode, resolution, VSync, frame rate).

use std::path::Path;

use egui::Ui;
use misty_engine::build::BuildSettings;
use misty_engine::player_settings::{PlayerSettings, ScreenMode};
use misty_engine::settings::ProjectSettings;

use crate::project_settings::section;

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
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
        |ui| {
            ui.add(egui::TextEdit::singleline(&mut p.product_name).hint_text(hint))
                .changed()
        },
    );
    edited |= row(
        ui,
        "Company Name",
        "Your studio's name; saved data goes in <Company>/<Product> in the user's folder.",
        |ui| ui.text_edit_singleline(&mut p.company_name).changed(),
    );
    edited |= row(
        ui,
        "Version",
        "Shown to players and in store builds, e.g. 1.0 or 0.3.2.",
        |ui| ui.text_edit_singleline(&mut p.version).changed(),
    );
    edited |= row(
        ui,
        "Icon",
        "A square PNG from Assets (256×256 or larger is best). Used for the .exe and window.",
        |ui| image_picker(ui, "player_icon", &mut p.icon, root),
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
        |ui| {
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
                });
            ch
        },
    );
    edited |= row(
        ui,
        "Default Resolution",
        "Window size in Windowed mode, and the resolution Exclusive Fullscreen switches to.",
        |ui| {
            let w = ui.add(egui::DragValue::new(&mut p.width).range(64..=16384));
            ui.label("×");
            let h = ui.add(egui::DragValue::new(&mut p.height).range(64..=16384));
            w.changed() || h.changed()
        },
    );
    edited |= row(
        ui,
        "Resizable Window",
        "Players can drag the window's edges.",
        |ui| ui.checkbox(&mut p.resizable, "").changed(),
    );
    edited |= row(
        ui,
        "Run In Background",
        "Keep playing when the window loses focus (otherwise the game pauses).",
        |ui| ui.checkbox(&mut p.run_in_background, "").changed(),
    );
    edited |= row(
        ui,
        "VSync",
        "Match the monitor's refresh rate: no tearing, steady frame times.",
        |ui| ui.checkbox(&mut p.vsync, "").changed(),
    );
    edited |= row(
        ui,
        "Target Frame Rate",
        "Frame cap when VSync is off. -1 = as fast as possible.",
        |ui| {
            ui.add_enabled_ui(!p.vsync, |ui| {
                ui.add(egui::DragValue::new(&mut p.target_frame_rate).range(-1..=1000))
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
        |ui| ui.checkbox(&mut sp.enabled, "").changed(),
    );
    ui.add_enabled_ui(sp.enabled, |ui| {
        edited |= row(ui, "Image", "PNG or JPG from Assets.", |ui| {
            image_picker(ui, "player_splash", &mut sp.image, root)
        });
        edited |= row(ui, "Duration", "Seconds the splash stays up.", |ui| {
            ui.add(
                egui::DragValue::new(&mut sp.duration)
                    .speed(0.1)
                    .range(0.0..=30.0)
                    .suffix(" s"),
            )
            .changed()
        });
        edited |= row(ui, "Background", "Colour around the image.", |ui| {
            ui.color_edit_button_rgb(&mut sp.background).changed()
        });
    });
    edited
}

/// Dropdown of the project's PNG/JPG files (listed when opened); `None` = no image.
pub fn image_picker(ui: &mut Ui, id: &str, value: &mut Option<String>, root: &Path) -> bool {
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
                    let label = rel.trim_start_matches("Assets/").to_owned();
                    ch |= ui.selectable_value(value, Some(rel), label).changed();
                }
            }
        });
    ch
}
