//! Editor accessibility: screen readers and the UI scale.
//!
//! Screen readers get the editor through AccessKit (eframe's `accesskit` feature, turned on in
//! the workspace manifest): egui builds the accessibility tree from each widget's label, so
//! controls without visible text are named here ([`name`], `labelled_by`). The UI scale
//! (Edit ▸ Preferences ▸ General) is egui's zoom factor, saved per user in `hub.json`.

use egui::{Response, WidgetInfo, WidgetType};

pub const MIN_UI_SCALE: f32 = 0.75;
pub const MAX_UI_SCALE: f32 = 2.0;
/// The scales offered in Preferences.
pub const UI_SCALES: [f32; 7] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];

/// Give a control without visible text (an icon or a bare checkbox) a name for screen readers.
pub fn name(resp: Response, kind: WidgetType, text: &str) -> Response {
    let enabled = resp.enabled();
    resp.widget_info(|| WidgetInfo::labeled(kind, enabled, text));
    resp
}

/// A checkbox without visible text (its caption is a label beside it) that screen readers still
/// announce by `name`. (`labelled_by` doesn't help here: an empty checkbox text counts as a name.)
pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, name: &str) -> Response {
    let resp = ui.add(egui::Checkbox::without_text(value));
    let (enabled, on) = (resp.enabled(), *value);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, on, name));
    resp
}

/// The editor's UI scale preference and what was last applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiScale {
    /// 1 = 100 %.
    pub value: f32,
    applied: Option<f32>,
}

impl Default for UiScale {
    fn default() -> Self {
        Self {
            value: 1.0,
            applied: None,
        }
    }
}

pub fn clamp_scale(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
    } else {
        1.0
    }
}

impl UiScale {
    /// The saved preference; `MISTY_UI_SCALE=1.5` overrides it (screenshots).
    pub fn load() -> Self {
        let value = std::env::var("MISTY_UI_SCALE")
            .ok()
            .and_then(|v| v.trim().trim_end_matches('%').parse::<f32>().ok())
            .map(|v| if v > 4.0 { v / 100.0 } else { v })
            .unwrap_or_else(crate::hub::config::editor_ui_scale);
        Self {
            value: clamp_scale(value),
            applied: None,
        }
    }

    /// Set the zoom factor when the preference changed (once per frame).
    pub fn apply(&mut self, ctx: &egui::Context) {
        let v = clamp_scale(self.value);
        if self.applied != Some(v) {
            self.applied = Some(v);
            ctx.set_zoom_factor(v);
        }
    }

    /// Change and save the preference.
    pub fn set(&mut self, value: f32) -> Result<(), String> {
        self.value = clamp_scale(value);
        crate::hub::config::set_editor_ui_scale(self.value)
    }
}

/// Preferences ▸ General: the UI scale dropdown; the scale picked this frame, if any.
pub fn ui_scale_row(ui: &mut egui::Ui, scale: &mut UiScale) -> Option<f32> {
    let mut picked = None;
    ui.horizontal(|ui| {
        let label = ui.label("UI Scale");
        let r = egui::ComboBox::from_id_salt("pref_ui_scale")
            .selected_text(format!("{:.0}%", scale.value * 100.0))
            .show_ui(ui, |ui| {
                for s in UI_SCALES {
                    if ui
                        .selectable_label(
                            (scale.value - s).abs() < 1e-3,
                            format!("{:.0}%", s * 100.0),
                        )
                        .clicked()
                    {
                        picked = Some(s);
                    }
                }
            });
        r.response
            .labelled_by(label.id)
            .on_hover_text("Size of the whole editor interface (text, buttons, panels)");
    });
    picked
}

/// Interactive controls a screen reader would announce without a name (tests).
#[cfg(test)]
pub fn unnamed_controls<S>(h: &egui_kittest::Harness<'_, S>) -> Vec<String> {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    const CONTROLS: [Role; 9] = [
        Role::Button,
        Role::CheckBox,
        Role::RadioButton,
        Role::TextInput,
        Role::MultilineTextInput,
        Role::Slider,
        Role::SpinButton,
        Role::ComboBox,
        Role::Tab,
    ];
    h.query_all_by(|n| {
        CONTROLS.contains(&n.role()) && n.label().is_none_or(|l| l.trim().is_empty())
    })
    .map(|n| {
        let a = n.accesskit_node();
        format!("{:?} value={:?}", a.role(), a.value())
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_clamped_and_applied_once() {
        assert_eq!(clamp_scale(9.0), MAX_UI_SCALE);
        assert_eq!(clamp_scale(f32::NAN), 1.0);
        let start = UiScale {
            value: 1.5,
            applied: None,
        };
        // The zoom factor takes effect on the next frame.
        let mut h =
            egui_kittest::Harness::new_ui_state(|ui, s: &mut UiScale| s.apply(ui.ctx()), start);
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.5);
        assert_eq!(h.state().applied, Some(1.5));
        h.state_mut().value = 1.25;
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.25);
    }

    /// The toolbar, Hierarchy, Project window and Preferences name every control.
    #[test]
    fn main_panels_name_their_controls() {
        use egui_kittest::Harness;
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root));
        let cube = s.scene.spawn("Cube", None);
        s.selection.set(cube);
        s.code_editor.ui.prefs_open = true;
        let mut h = Harness::builder()
            .with_size([1200.0, 800.0])
            .build_ui_state(
                |ui, s: &mut crate::state::EditorState| {
                    crate::toolbar::show(ui, s);
                    ui.columns(2, |cols| {
                        crate::panels::hierarchy::show(&mut cols[0], s);
                        crate::panels::project::show(&mut cols[1], s);
                    });
                    crate::panels::code_editor::show_windows(ui.ctx(), s);
                },
                s,
            );
        h.run_steps(3);
        let unnamed = unnamed_controls(&h);
        assert!(unnamed.is_empty(), "unnamed controls: {unnamed:#?}");
    }

    #[test]
    fn named_icon_buttons_are_found_by_screen_readers() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let h = Harness::new_ui(|ui| {
            name(ui.button("⟳"), WidgetType::Button, "Refresh");
            let mut on = false;
            ui.add(egui::Checkbox::without_text(&mut on));
        });
        h.get_by_label("Refresh");
        let unnamed = unnamed_controls(&h);
        assert_eq!(unnamed.len(), 1, "{unnamed:?}");
        assert!(unnamed[0].starts_with("CheckBox"));
    }
}
