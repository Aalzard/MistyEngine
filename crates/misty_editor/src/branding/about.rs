//! Help ▸ About Misty Engine: logo, version, credits and license.

use egui::{Align, Context, Layout, RichText};

/// Credits line under the version.
pub const CREDITS: &str = "Made by the Misty Engine team, with Rust, wgpu and egui.";
/// The license line (LICENSE and RUNTIME-EXCEPTION.md in the source and the download).
pub const LICENSE: &str = "Free software under the GNU GPL v3 or later. Games you build with it      may use any license (runtime exception).";

/// The About window while `open` is set (closed by its × button or Close).
pub fn show(ctx: &Context, open: &mut bool) {
    if !*open {
        return;
    }
    let mut keep = true;
    let mut close = false;
    egui::Window::new("About Misty Engine")
        .open(&mut keep)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .fixed_size([400.0, 0.0])
        .show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(6.0);
                let logo = super::logo_texture(ctx);
                ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(140.0, 140.0)));
                ui.label(title(ui));
                ui.add_space(8.0);
                ui.label(CREDITS);
                ui.label(RichText::new(LICENSE).size(11.0).weak());
                ui.add_space(8.0);
                close = ui.button("Close").clicked();
                ui.add_space(4.0);
            });
        });
    *open = keep && !close;
}

/// "Misty Engine v0.1.0": the name large, the version smaller and dimmer.
fn title(ui: &egui::Ui) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let v = ui.visuals();
    let format = |size: f32, color| egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        valign: Align::BOTTOM,
        ..Default::default()
    };
    job.append(super::PRODUCT, 0.0, format(22.0, v.strong_text_color()));
    let version = format!(" v{}", super::version());
    job.append(&version, 0.0, format(15.0, v.weak_text_color()));
    job
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;

    #[test]
    fn shows_version_and_closes() {
        let mut h = egui_kittest::Harness::builder()
            .with_size([600.0, 500.0])
            .build_ui_state(|ui, open: &mut bool| super::show(ui.ctx(), open), true);
        h.run();
        h.get_by_label(&format!("Misty Engine v{}", env!("CARGO_PKG_VERSION")));
        h.get_by_label(super::CREDITS);
        h.get_by_label("Close").click();
        h.run();
        assert!(!*h.state());
    }
}
