//! Edit ▸ Grid and Snap: grid size, Align Selected to Grid and the Ctrl-drag increments.
//! Every change is saved to the project's `Library/SnapSettings.json` right away.

use egui::Ui;

use crate::actions::Action;
use crate::snap::SnapSettings;
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Draw the window if it is open.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.snap_window_open;
    egui::Window::new("Grid and Snap")
        .open(&mut open)
        .default_size([340.0, 240.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.snap_window_open = open;
}

pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    let before = s.snap.clone();
    let snap = &mut s.snap;
    ui.strong("World Grid");
    vec3_row(ui, "Grid Size", &mut snap.grid_size, 0.05);
    ui.horizontal(|ui| {
        ui.checkbox(&mut snap.grid_snap, "Grid Snapping")
            .on_hover_text("Moves snap to the world grid (Global handle only)");
        if ui
            .button("Align Selected to Grid")
            .on_hover_text("Round the selected objects' positions to the grid")
            .clicked()
        {
            s.actions.push(Action::AlignToGrid);
        }
    });
    ui.separator();
    ui.strong("Increment Snap (hold Ctrl)");
    vec3_row(ui, "Move", &mut snap.move_step, 0.01);
    row(ui, "Rotate", |ui| {
        ui.add(
            egui::DragValue::new(&mut snap.rotate_step)
                .speed(0.5)
                .range(0.0..=360.0)
                .suffix("°"),
        );
    });
    row(ui, "Scale", |ui| {
        ui.add(
            egui::DragValue::new(&mut snap.scale_step)
                .speed(0.01)
                .range(0.0..=100.0),
        );
    });
    ui.separator();
    if ui.button("Reset").clicked() {
        *snap = SnapSettings::default();
    }
    for v in snap.grid_size.iter_mut().chain(snap.move_step.iter_mut()) {
        *v = v.max(0.0);
    }
    if s.snap != before {
        changed(s);
    }
}

/// Save the settings (called by the window and the Scene toolbar toggle).
pub fn changed(s: &mut EditorState) {
    if let Err(e) = s.snap.save(&s.project.root) {
        s.console
            .warn(format!("Could not save Grid and Snap settings: {e}"));
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([72.0, 18.0], egui::Label::new(label));
        add(ui);
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use glam::Vec3;

    use super::*;
    use crate::project::ProjectBrowser;

    #[test]
    fn grid_snapping_toggle_and_align_to_grid_are_saved_and_undoable() {
        let root = std::env::temp_dir().join(format!("misty_snapwin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let cube = s.scene.spawn("Cube", None);
        s.scene.set_world_position(cube, Vec3::new(1.4, 0.2, -2.6));
        s.selection.set(cube);
        let mut h = Harness::builder()
            .with_size([400.0, 300.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Grid Snapping").click();
        h.run();
        h.get_by_label("Align Selected to Grid").click();
        h.run();
        let s = h.state_mut();
        assert!(s.snap.grid_snap);
        assert!(crate::snap::SnapSettings::load(&root).grid_snap, "saved");
        for a in std::mem::take(&mut s.actions) {
            crate::commands::apply(s, a);
        }
        assert_eq!(s.scene.world_position(cube), Vec3::new(1.0, 0.0, -3.0));
        crate::commands::apply(s, Action::Undo);
        assert!((s.scene.world_position(cube) - Vec3::new(1.4, 0.2, -2.6)).length() < 1e-5);
        std::fs::remove_dir_all(root).ok();
    }
}
