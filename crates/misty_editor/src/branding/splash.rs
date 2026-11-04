//! Startup splash: the window shows a centred logo, and the project loads on the next frames
//! while it is on screen (instead of a blank window during the load). The `launcher` drives it,
//! for `misty <project>` and for projects opened from the Hub.
//!
//! Nothing waits on purpose: the load starts as soon as the splash has been presented, and the
//! editor replaces it in the same frame the load finishes. The last open time is kept in
//! `Library/EditorStartup.txt`; projects that opened quickly last time skip the splash.

use egui::{Align2, Color32, FontId, Rect, Vec2};

/// Frames drawn before loading: the first presented frame also makes the window visible.
pub const SPLASH_FRAMES: u32 = 2;
/// Loads shorter than this (last time) open without the splash.
const SPLASH_MIN_MS: u64 = 400;
/// The last open time, per project.
const STARTUP_FILE: &str = "Library/EditorStartup.txt";

/// Show the splash unless the project opened quickly last time (unknown: show it).
pub fn wants_splash(project: &std::path::Path) -> bool {
    std::fs::read_to_string(project.join(STARTUP_FILE))
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
        .is_none_or(|ms| ms >= SPLASH_MIN_MS)
}

pub fn remember(project: &std::path::Path, ms: u64) {
    let path = project.join(STARTUP_FILE);
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(path, ms.to_string());
    }
}

/// The splash: logo, name and the project being opened, on the tab-bar background.
pub fn paint(ui: &mut egui::Ui, project: &std::path::Path) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, crate::theme::TAB_BAR_BG);
    let logo = super::logo_texture(ui.ctx());
    let side = (rect.height() * 0.28).clamp(96.0, 220.0);
    let center = rect.center() - Vec2::new(0.0, side * 0.25);
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(
        logo.id(),
        Rect::from_center_size(center, Vec2::splat(side)),
        uv,
        Color32::WHITE,
    );
    let below = center.y + side * 0.5 + 8.0;
    painter.text(
        egui::pos2(center.x, below),
        Align2::CENTER_TOP,
        super::PRODUCT,
        FontId::proportional(26.0),
        Color32::from_gray(235),
    );
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    painter.text(
        egui::pos2(center.x, below + 38.0),
        Align2::CENTER_TOP,
        format!("Opening {name}…"),
        FontId::proportional(13.0),
        crate::theme::TEXT_DIM,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_projects_skip_the_splash() {
        let root = std::env::temp_dir().join(format!("misty_splash_{}", std::process::id()));
        assert!(wants_splash(&root), "first open: unknown time");
        remember(&root, 120);
        assert!(!wants_splash(&root));
        remember(&root, 2500);
        assert!(wants_splash(&root));
        std::fs::remove_dir_all(root).ok();
    }
}
