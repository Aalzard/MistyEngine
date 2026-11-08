//! Inspector editors added with the round 19 UI work: Canvas render modes, the Canvas Scaler
//! modes, Image types (Sliced, Tiled, Filled, Preserve Aspect), Canvas Group, Aspect Ratio
//! Fitter, selectable Navigation (with the Scene view's Visualize toggle) and the Event
//! System's input module settings.

use egui::{RichText, Ui};
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    AspectMode, AspectRatioFitter, Canvas, CanvasGroup, CanvasRenderMode, CanvasScaler,
    EventSystem, FillMethod, ImageType, Navigation, NavigationMode, PhysicalUnit, ScreenMatchMode,
    UiImage, UiScaleMode,
};

use super::{grid, vec2};
use crate::theme;

/// Scene objects for reference pickers: (entity, name).
pub type Objects = Vec<(Entity, String)>;

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

/// Scene object picker (`None` clears it).
pub fn object_field(
    ui: &mut Ui,
    id: &str,
    v: &mut Option<Entity>,
    objs: &Objects,
    none: &str,
) -> bool {
    let name = v
        .and_then(|t| objs.iter().find(|(e, _)| *e == t))
        .map_or(none.to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, none).changed();
            for (e, n) in objs {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ch
}

fn drag(ui: &mut Ui, v: &mut f32, speed: f64, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(
        egui::DragValue::new(v)
            .speed(speed)
            .range(range)
            .max_decimals(3),
    )
    .changed()
}

// ---- Canvas ----------------------------------------------------------------------------------

/// Render Mode, Render Camera / Plane Distance (Screen Space - Camera), Sort Order.
/// `cameras` = scene objects with a Camera.
pub fn canvas(ui: &mut Ui, c: &mut Canvas, cameras: &Objects) -> bool {
    let mut ch = grid(ui, "canvas_grid", |ui| {
        let mut ch = false;
        ui.label("Render Mode");
        ch |= combo(
            ui,
            "canvas_mode",
            &mut c.render_mode,
            &CanvasRenderMode::ALL,
            CanvasRenderMode::label,
        );
        ui.end_row();
        match c.render_mode {
            CanvasRenderMode::ScreenSpaceCamera => {
                ui.label("Render Camera");
                ch |= object_field(
                    ui,
                    "canvas_cam",
                    &mut c.world_camera,
                    cameras,
                    "None (Camera)",
                );
                ui.end_row();
                ui.label("Plane Distance");
                ch |= drag(ui, &mut c.plane_distance, 0.5, 0.01..=10000.0);
                ui.end_row();
            }
            CanvasRenderMode::WorldSpace => {
                ui.label("Event Camera");
                ui.label(RichText::new("The game camera").color(theme::TEXT_DIM));
                ui.end_row();
            }
            CanvasRenderMode::ScreenSpaceOverlay => {}
        }
        ui.label("Sort Order");
        ch |= ui.add(egui::DragValue::new(&mut c.sort_order)).changed();
        ui.end_row();
        ch
    });
    if c.render_mode == CanvasRenderMode::ScreenSpaceCamera && c.world_camera.is_none() {
        ui.label(
            RichText::new(
                "A Screen Space Canvas with no Render Camera acts like an Overlay Canvas.",
            )
            .color(theme::TEXT_DIM),
        );
        if let Some((cam, _)) = cameras.first()
            && ui.button("Use Main Camera").clicked()
        {
            c.world_camera = Some(*cam);
            ch = true;
        }
    }
    ch
}

/// The scaler; `world` = its canvas is World Space (only the pixels-per-unit settings apply).
pub fn canvas_scaler(ui: &mut Ui, s: &mut CanvasScaler, world: bool) -> bool {
    grid(ui, "canvas_scaler_grid", |ui| {
        let mut ch = false;
        if world {
            ui.label("UI Scale Mode");
            ui.label(RichText::new("World").color(theme::TEXT_DIM));
            ui.end_row();
            ui.label("Dynamic Pixels Per Unit");
            ch |= drag(ui, &mut s.dynamic_pixels_per_unit, 0.05, 0.01..=100.0);
            ui.end_row();
            ui.label("Reference Pixels Per Unit");
            ch |= drag(ui, &mut s.reference_pixels_per_unit, 1.0, 0.01..=10000.0);
            ui.end_row();
            return ch;
        }
        ui.label("UI Scale Mode");
        ch |= combo(
            ui,
            "scaler_mode",
            &mut s.mode,
            &UiScaleMode::ALL,
            UiScaleMode::label,
        );
        ui.end_row();
        match s.mode {
            UiScaleMode::ConstantPixelSize => {
                ui.label("Scale Factor");
                ch |= drag(ui, &mut s.scale_factor, 0.01, 0.01..=100.0);
                ui.end_row();
            }
            UiScaleMode::ScaleWithScreenSize => {
                ui.label("Reference Resolution");
                ch |= vec2(ui, &mut s.reference_resolution, 1.0);
                s.reference_resolution = s.reference_resolution.max(Vec2::ONE);
                ui.end_row();
                ui.label("Screen Match Mode");
                ch |= combo(
                    ui,
                    "scaler_match_mode",
                    &mut s.screen_match_mode,
                    &ScreenMatchMode::ALL,
                    ScreenMatchMode::label,
                );
                ui.end_row();
                if s.screen_match_mode == ScreenMatchMode::MatchWidthOrHeight {
                    ui.label("Match");
                    ui.vertical(|ui| {
                        ch |= ui
                            .add(egui::Slider::new(&mut s.match_width_or_height, 0.0..=1.0))
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Width").small().color(theme::TEXT_DIM));
                            ui.add_space(110.0);
                            ui.label(RichText::new("Height").small().color(theme::TEXT_DIM));
                        });
                    });
                    ui.end_row();
                }
            }
            UiScaleMode::ConstantPhysicalSize => {
                ui.label("Physical Unit");
                ch |= combo(
                    ui,
                    "scaler_unit",
                    &mut s.physical_unit,
                    &PhysicalUnit::ALL,
                    PhysicalUnit::label,
                );
                ui.end_row();
                ui.label("Fallback Screen DPI");
                ch |= drag(ui, &mut s.fallback_screen_dpi, 1.0, 1.0..=1000.0);
                ui.end_row();
                ui.label("Default Sprite DPI");
                ch |= drag(ui, &mut s.default_sprite_dpi, 1.0, 1.0..=1000.0);
                ui.end_row();
            }
        }
        ui.label("Reference Pixels Per Unit");
        ch |= drag(ui, &mut s.reference_pixels_per_unit, 1.0, 0.01..=10000.0);
        ui.end_row();
        ch
    })
}

// ---- Image -----------------------------------------------------------------------------------

/// Image Type and its settings (rows inside the Image grid).
pub fn image_type_rows(ui: &mut Ui, img: &mut UiImage, has_border: bool) -> bool {
    let mut ch = false;
    ui.label("Image Type");
    ch |= combo(
        ui,
        "ui_image_type",
        &mut img.image_type,
        &ImageType::ALL,
        ImageType::label,
    );
    ui.end_row();
    match img.image_type {
        ImageType::Simple => {
            ui.label("Preserve Aspect");
            ch |= ui.checkbox(&mut img.preserve_aspect, "").changed();
            ui.end_row();
        }
        ImageType::Sliced | ImageType::Tiled => {
            if img.image_type == ImageType::Sliced && !has_border {
                ui.label("");
                ui.label(
                    RichText::new(
                        "This image has no border (set it in the sprite's import settings).",
                    )
                    .color(theme::TEXT_DIM),
                );
                ui.end_row();
            }
            ui.label("Fill Center");
            ch |= ui.checkbox(&mut img.fill_center, "").changed();
            ui.end_row();
            ui.label("Pixels Per Unit Multiplier");
            ch |= drag(ui, &mut img.pixels_per_unit_multiplier, 0.01, 0.01..=100.0);
            ui.end_row();
        }
        ImageType::Filled => {
            let f = &mut img.fill;
            ui.label("Fill Method");
            let before = f.method;
            ch |= combo(
                ui,
                "ui_fill_method",
                &mut f.method,
                &FillMethod::ALL,
                FillMethod::label,
            );
            if f.method != before {
                f.origin = 0;
            }
            ui.end_row();
            ui.label("Fill Origin");
            let origins = f.method.origins();
            let current = origins.get(f.origin as usize).copied().unwrap_or("?");
            egui::ComboBox::from_id_salt("ui_fill_origin")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, o) in origins.iter().enumerate() {
                        ch |= ui.selectable_value(&mut f.origin, i as u32, *o).changed();
                    }
                });
            ui.end_row();
            ui.label("Fill Amount");
            ch |= ui
                .add(egui::Slider::new(&mut f.amount, 0.0..=1.0))
                .changed();
            ui.end_row();
            if f.method.is_radial() {
                ui.label("Clockwise");
                ch |= ui.checkbox(&mut f.clockwise, "").changed();
                ui.end_row();
            }
            ui.label("Preserve Aspect");
            ch |= ui.checkbox(&mut img.preserve_aspect, "").changed();
            ui.end_row();
        }
    }
    ch
}

// ---- Canvas Group / Aspect Ratio Fitter --------------------------------------------------------

pub fn canvas_group(ui: &mut Ui, g: &mut CanvasGroup) -> bool {
    grid(ui, "ui_canvas_group", |ui| {
        let mut c = false;
        ui.label("Alpha");
        c |= ui.add(egui::Slider::new(&mut g.alpha, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Interactable");
        c |= ui.checkbox(&mut g.interactable, "").changed();
        ui.end_row();
        ui.label("Blocks Raycasts");
        c |= ui.checkbox(&mut g.blocks_raycasts, "").changed();
        ui.end_row();
        ui.label("Ignore Parent Groups");
        c |= ui.checkbox(&mut g.ignore_parent_groups, "").changed();
        ui.end_row();
        c
    })
}

pub fn aspect_ratio_fitter(ui: &mut Ui, f: &mut AspectRatioFitter) -> bool {
    grid(ui, "ui_aspect_fitter", |ui| {
        let mut c = false;
        ui.label("Aspect Mode");
        c |= combo(
            ui,
            "aspect_mode",
            &mut f.aspect_mode,
            &AspectMode::ALL,
            AspectMode::label,
        );
        ui.end_row();
        ui.label("Aspect Ratio");
        c |= drag(ui, &mut f.aspect_ratio, 0.01, 0.001..=1000.0);
        ui.end_row();
        c
    })
}

// ---- Navigation --------------------------------------------------------------------------------

/// Navigation rows of a selectable (inside its grid): mode, Wrap Around or the explicit
/// targets, and the Visualize toggle.
pub fn navigation_rows(ui: &mut Ui, id: &str, nav: &mut Navigation, objs: &Objects) -> bool {
    let mut c = false;
    ui.label("Navigation");
    ui.horizontal(|ui| {
        c |= combo(
            ui,
            &format!("{id}_nav"),
            &mut nav.mode,
            &NavigationMode::ALL,
            NavigationMode::label,
        );
        let mut on = crate::ui_gizmos::visualizing_navigation(ui.ctx());
        if ui
            .selectable_label(on, "Visualize")
            .on_hover_text("Show navigation links in the Scene view")
            .clicked()
        {
            on = !on;
            ui.ctx()
                .data_mut(|d| d.insert_temp(crate::ui_gizmos::visualize_id(), on));
        }
    });
    ui.end_row();
    match nav.mode {
        NavigationMode::Horizontal | NavigationMode::Vertical => {
            ui.label("Wrap Around");
            c |= ui.checkbox(&mut nav.wrap_around, "").changed();
            ui.end_row();
        }
        NavigationMode::Explicit => {
            for (label, key, v) in [
                ("Select On Up", "up", &mut nav.select_on_up),
                ("Select On Down", "down", &mut nav.select_on_down),
                ("Select On Left", "left", &mut nav.select_on_left),
                ("Select On Right", "right", &mut nav.select_on_right),
            ] {
                ui.label(label);
                c |= object_field(ui, &format!("{id}_nav_{key}"), v, objs, "None (Selectable)");
                ui.end_row();
            }
        }
        _ => {}
    }
    c
}

// ---- Event System ------------------------------------------------------------------------------

/// The input module rows of the Event System (inside its grid).
pub fn event_system_input(ui: &mut Ui, es: &mut EventSystem, assets: &[String]) -> bool {
    let mut c = false;
    ui.label("Input Module");
    ui.label(
        RichText::new("Input System UI (falls back to the Input Manager)").color(theme::TEXT_DIM),
    );
    ui.end_row();
    ui.label("Actions Asset");
    let shown = es
        .actions_asset
        .clone()
        .unwrap_or_else(|| "Default UI Actions".into());
    egui::ComboBox::from_id_salt("es_actions")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            c |= ui
                .selectable_value(&mut es.actions_asset, None, "Default UI Actions")
                .changed();
            for a in assets {
                c |= ui
                    .selectable_value(&mut es.actions_asset, Some(a.clone()), a)
                    .changed();
            }
        });
    ui.end_row();
    ui.label("Move Repeat Delay");
    c |= drag(ui, &mut es.move_repeat_delay, 0.01, 0.0..=5.0);
    ui.end_row();
    ui.label("Move Repeat Rate");
    c |= drag(ui, &mut es.move_repeat_rate, 0.01, 0.01..=5.0);
    ui.end_row();
    c
}
