//! The updater's UI: the green "⬆ Update" pill, the "New version" message box and the
//! progress / result windows. Shared by the editor and the Hub.

use egui::{Align2, Color32, CornerRadius, RichText};

use crate::i18n::tr;

use super::{Phase, Update, Updater, VERSION};

/// The update button's green.
pub const GREEN: Color32 = Color32::from_rgb(38, 158, 72);
const BUTTON_TEXT: &str = "⬆ Update";

/// The green pill, shown only while an update is available. Returns true when clicked.
pub fn update_button(ui: &mut egui::Ui, up: &Updater) -> bool {
    let Some(update) = up.available() else {
        return false;
    };
    let busy = up.phase().busy();
    let text = if busy { "⬆ Updating…" } else { BUTTON_TEXT };
    let button = egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
        .fill(GREEN)
        .corner_radius(CornerRadius::same(10))
        .min_size(egui::vec2(0.0, 20.0));
    ui.add_enabled(!busy, button)
        .on_hover_text(format!(
            "Misty Engine {} is available (you have {VERSION}). Click to update.",
            update.version
        ))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Preferences / Hub Settings: the "Check for updates at startup" checkbox. Returns true when
/// it was changed this frame.
pub fn startup_check_row(ui: &mut egui::Ui, on: &mut bool) -> bool {
    ui.checkbox(on, tr!("Check for updates at startup"))
        .on_hover_text(tr!(
            "Asks GitHub for the newest Misty Engine release ({repo}) when the editor or the Hub \
             starts. Nothing else is contacted and nothing about you or your projects is sent. \
             Help ▸ Check for Updates… works either way.",
            repo = super::REPO
        ))
        .changed()
}

/// The message box and progress windows; call once per frame. Returns true when the user
/// chose "Update now". Closes the window once the new version has been started.
pub fn windows(ctx: &egui::Context, up: &mut Updater) -> bool {
    if up.take_announce() {
        up.show_box = true;
    }
    let mut install = false;
    let phase = up.phase();
    if up.show_box
        && !phase.busy()
        && let Some(update) = up.available()
    {
        message_box(ctx, up, &update, &mut install);
    }
    match &phase {
        p if p.busy() => progress(ctx, p),
        Phase::Manual(why) => result(ctx, up, "Download the new version", why),
        Phase::Failed(why) => result(ctx, up, "The update didn't install", why),
        _ => {}
    }
    if phase == Phase::Restarting {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    if let Some(text) = up.notice() {
        notice(ctx, up, &text);
    }
    install
}

/// "New version 0.0.2 is available — Update now / Later / Skip this version".
fn message_box(ctx: &egui::Context, up: &mut Updater, u: &Update, install: &mut bool) {
    egui::Window::new("Update available")
        .id(egui::Id::new("misty_update_box"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::RIGHT_BOTTOM, [-16.0, -36.0])
        .fixed_size([330.0, 0.0])
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("⬆").size(20.0).color(GREEN));
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("New version {} is available", u.version))
                            .strong()
                            .size(15.0),
                    );
                    ui.label(RichText::new(format!("You have {VERSION}.")).weak());
                });
            });
            if !u.notes.is_empty() {
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(110.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| ui.label(RichText::new(&u.notes).size(12.5)));
                    });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let now = egui::Button::new(RichText::new("Update now").color(Color32::WHITE))
                    .fill(GREEN);
                if ui.add(now).clicked() {
                    *install = true;
                }
                if ui.button("Later").clicked() {
                    up.later();
                }
                if ui.button("Skip this version").clicked() {
                    up.skip();
                }
            });
            ui.add_space(2.0);
        });
}

/// Download bar, then "Installing…", then "Restarting…".
fn progress(ctx: &egui::Context, phase: &Phase) {
    egui::Modal::new(egui::Id::new("misty_update_progress")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Updating Misty Engine");
        ui.add_space(8.0);
        match phase {
            Phase::Downloading { received, total } => {
                let mb = |b: u64| b as f32 / (1024.0 * 1024.0);
                let (fraction, text) = match total {
                    Some(t) if *t > 0 => (
                        *received as f32 / *t as f32,
                        format!("Downloading… {:.1} / {:.1} MB", mb(*received), mb(*t)),
                    ),
                    _ => (0.0, format!("Downloading… {:.1} MB", mb(*received))),
                };
                ui.add(egui::ProgressBar::new(fraction).text(text).fill(GREEN));
            }
            Phase::Verifying => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking the download…");
                });
            }
            Phase::Installing => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Installing…");
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Restarting…");
                });
            }
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("Your projects are not touched.")
                .weak()
                .small(),
        );
    });
}

/// Failure or "download it yourself", with a link to the release page.
fn result(ctx: &egui::Context, up: &mut Updater, title: &str, why: &str) {
    let modal = egui::Modal::new(egui::Id::new("misty_update_result")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading(title);
        ui.add_space(6.0);
        ui.label(why);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let open = egui::Button::new(RichText::new("Open release page").color(Color32::WHITE))
                .fill(GREEN);
            if ui.add(open).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(up.page_url()));
                up.dismiss();
            }
            if ui.button("Close").clicked() {
                up.dismiss();
            }
        });
    });
    if modal.should_close() {
        up.dismiss();
    }
}

/// The answer to Help ▸ Check for Updates… when there is nothing to install.
fn notice(ctx: &egui::Context, up: &mut Updater, text: &str) {
    egui::Window::new("Check for Updates")
        .id(egui::Id::new("misty_update_notice"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::RIGHT_BOTTOM, [-16.0, -36.0])
        .show(ctx, |ui| {
            ui.label(text);
            ui.add_space(4.0);
            if ui.button("OK").clicked() {
                up.dismiss();
            }
        });
}
