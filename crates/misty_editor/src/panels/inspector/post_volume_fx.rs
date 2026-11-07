//! Post-process Volume sections of the camera effects: Depth of Field, Motion Blur and Auto
//! Exposure (render/postfx). Like Unity's overrides, each only applies while ticked.

use egui::Ui;
use misty_engine::scene::{
    AdaptationMode, DofMode, MeteringMode, PostFxQuality, PostProcessVolume,
};

use super::editors::combo;
use super::post_volume::{drag, row, section, slider};

pub fn sections(ui: &mut Ui, v: &mut PostProcessVolume) -> bool {
    let d = &mut v.depth_of_field;
    let mut ch = section(ui, "Depth of Field", &mut d.active, |ui| {
        let mut ch = row(ui, "Mode", |ui| {
            combo(
                ui,
                "ppv_dof_mode",
                &mut d.mode,
                &DofMode::ALL,
                DofMode::label,
            )
        });
        match d.mode {
            DofMode::Gaussian => {
                ch |= row(ui, "Start", |ui| drag(ui, &mut d.gaussian_start, 0.0..=1e4));
                ch |= row(ui, "End", |ui| drag(ui, &mut d.gaussian_end, 0.0..=1e4));
                ch |= row(ui, "Max Radius", |ui| {
                    slider(ui, &mut d.gaussian_max_radius, 0.5..=1.5)
                });
                ch |= row(ui, "High Quality Sampling", |ui| {
                    ui.checkbox(&mut d.high_quality_sampling, "").changed()
                });
            }
            DofMode::Bokeh => {
                ch |= row(ui, "Focus Distance", |ui| {
                    drag(ui, &mut d.focus_distance, 0.1..=1e4)
                });
                ch |= row(ui, "Focal Length (mm)", |ui| {
                    slider(ui, &mut d.focal_length, 1.0..=300.0)
                });
                ch |= row(ui, "Aperture (f-stop)", |ui| {
                    slider(ui, &mut d.aperture, 1.0..=32.0)
                });
                ch |= row(ui, "Blade Count", |ui| {
                    ui.add(egui::Slider::new(&mut d.blade_count, 3..=9))
                        .changed()
                });
                ch |= row(ui, "Blade Curvature", |ui| {
                    slider(ui, &mut d.blade_curvature, 0.0..=1.0)
                });
                ch |= row(ui, "Blade Rotation", |ui| {
                    slider(ui, &mut d.blade_rotation, -180.0..=180.0)
                });
                ch |= row(ui, "Quality", |ui| {
                    combo(
                        ui,
                        "ppv_dof_quality",
                        &mut d.quality,
                        &PostFxQuality::ALL,
                        PostFxQuality::label,
                    )
                });
            }
        }
        ch
    });
    if d.active && d.mode == DofMode::Bokeh {
        ui.weak("Focus Distance is in focus; a lower f-stop or longer lens blurs more.");
    }
    let m = &mut v.motion_blur;
    ch |= section(ui, "Motion Blur", &mut m.active, |ui| {
        let mut ch = row(ui, "Intensity", |ui| {
            slider(ui, &mut m.intensity, 0.0..=1.0)
        });
        ch |= row(ui, "Maximum Velocity", |ui| {
            slider(ui, &mut m.maximum_velocity, 0.0..=0.2)
        });
        ch |= row(ui, "Sample Count", |ui| {
            ui.add(egui::Slider::new(&mut m.sample_count, 4..=32))
                .changed()
        });
        ch |= row(ui, "Camera Motion Blur", |ui| {
            ui.checkbox(&mut m.camera_motion_blur, "").changed()
        });
        let camera = m.camera_motion_blur;
        ch |= row(ui, "Camera Clamp", |ui| {
            let s = egui::Slider::new(&mut m.camera_clamp, 0.0..=0.2);
            ui.add_enabled(camera, s).changed()
        });
        ch
    });
    let a = &mut v.auto_exposure;
    ch |= section(ui, "Auto Exposure", &mut a.active, |ui| {
        let mut ch = row(ui, "Metering Mode", |ui| {
            combo(
                ui,
                "ppv_ae_metering",
                &mut a.metering,
                &MeteringMode::ALL,
                MeteringMode::label,
            )
        });
        ch |= row(ui, "Limit Min", |ui| {
            drag(ui, &mut a.limit_min, -10.0..=10.0)
        });
        ch |= row(ui, "Limit Max", |ui| {
            drag(ui, &mut a.limit_max, -10.0..=10.0)
        });
        ch |= row(ui, "Compensation", |ui| {
            drag(ui, &mut a.compensation, -10.0..=10.0)
        });
        ch |= row(ui, "Adaptation", |ui| {
            combo(
                ui,
                "ppv_ae_adaptation",
                &mut a.adaptation,
                &AdaptationMode::ALL,
                AdaptationMode::label,
            )
        });
        if a.adaptation == AdaptationMode::Progressive {
            ch |= row(ui, "Speed Dark to Light", |ui| {
                drag(ui, &mut a.speed_dark_to_light, 0.01..=20.0)
            });
            ch |= row(ui, "Speed Light to Dark", |ui| {
                drag(ui, &mut a.speed_light_to_dark, 0.01..=20.0)
            });
        }
        ch |= row(ui, "Histogram Low %", |ui| {
            slider(ui, &mut a.histogram_low, 0.0..=100.0)
        });
        ch |= row(ui, "Histogram High %", |ui| {
            slider(ui, &mut a.histogram_high, 0.0..=100.0)
        });
        ch
    });
    if a.active {
        ui.weak("EV 0 is the fixed exposure; a scene a stop brighter than mid grey gets EV 1.");
    }
    ch
}
