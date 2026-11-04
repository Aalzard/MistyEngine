//! Edit ▸ Undo History: every undo step by name, newest at the top. The current step is
//! highlighted, redoable steps are greyed below it, and clicking a row jumps to that point.

use egui::{RichText, Ui};

use crate::actions::Action;
use crate::state::EditorState;
use crate::theme;

/// Search text (kept per editor session, not saved).
#[derive(Default)]
pub struct UndoHistoryWindow {
    pub search: String,
}

/// Name of the row that goes back to before the first step.
pub const INITIAL: &str = "Initial State";

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    toolbar(ui, state);
    ui.separator();
    let steps: Vec<String> = state.undo.steps().iter().map(|s| s.to_string()).collect();
    let position = state.undo.position();
    let needle = state.undo_history.search.trim().to_lowercase();
    let shown = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);

    let footer = 22.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height((ui.available_height() - footer).max(40.0))
        .show(ui, |ui| {
            // Full-width rows with left-aligned names, like the Hierarchy.
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                rows(ui, state, &steps, position, &shown);
            });
        });
    ui.separator();
    let mb = state.undo.bytes() as f32 / (1024.0 * 1024.0);
    let redo = steps.len() - position;
    ui.label(
        RichText::new(format!(
            "{} steps · {redo} redoable · ~{mb:.1} MB",
            steps.len()
        ))
        .small()
        .color(theme::TEXT_DIM),
    );
}

/// Newest first; row i is applied once i + 1 steps are.
fn rows(
    ui: &mut Ui,
    state: &mut EditorState,
    steps: &[String],
    position: usize,
    shown: &dyn Fn(&str) -> bool,
) {
    for (i, name) in steps.iter().enumerate().rev() {
        if shown(name) {
            row(ui, state, name, i + 1, position);
        }
    }
    if shown(INITIAL) {
        row(ui, state, INITIAL, 0, position);
    }
}

/// Search box and Clear History.
fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    ui.horizontal(|ui| {
        let empty = state.undo.steps().is_empty();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!empty, egui::Button::new("Clear History"))
                .on_hover_text("Forget every step (the scene stays as it is)")
                .clicked()
            {
                state.push(Action::ClearUndoHistory);
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.undo_history.search)
                    .hint_text("🔍 Search")
                    .desired_width(f32::INFINITY),
            );
        });
    });
}

/// One step: highlighted when it is the current one, greyed when it would be redone.
fn row(ui: &mut Ui, state: &mut EditorState, name: &str, target: usize, position: usize) {
    let current = target == position;
    let redoable = target > position;
    let mut text = RichText::new(name);
    if redoable {
        text = text.color(theme::TEXT_DIM).italics();
    }
    let hint = if current {
        "Current state"
    } else if redoable {
        "Redo up to this step"
    } else {
        "Undo back to this step"
    };
    let response = ui.add(egui::Button::selectable(current, text));
    if response.on_hover_text(hint).clicked() && !current {
        state.push(Action::UndoJump(target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use egui_kittest::{Harness, kittest::Queryable};

    fn state() -> EditorState {
        let root = std::env::temp_dir().join("misty_undo_history_window_test");
        let mut s = EditorState::new(ProjectBrowser::open(root));
        for name in ["Create Cube", "Move Cube", "Change Light Intensity"] {
            s.record(name);
        }
        s.undo_step();
        s
    }

    #[test]
    fn lists_steps_and_clicking_jumps() {
        let mut h = Harness::builder()
            .with_size([320.0, 300.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), state());
        h.run();
        h.get_by_label("Change Light Intensity");
        h.get_by_label(INITIAL);
        h.get_by_label("Create Cube").click();
        h.run();
        let s = h.state_mut();
        let actions = std::mem::take(&mut s.actions);
        assert!(
            matches!(actions.as_slice(), [Action::UndoJump(1)]),
            "{actions:?}"
        );
        crate::commands::apply(s, Action::UndoJump(1));
        assert_eq!(s.undo.position(), 1);
        assert_eq!(s.undo.redo_label(), Some("Move Cube"));
    }

    #[test]
    fn search_filters_rows_and_clear_empties() {
        let mut s = state();
        s.undo_history.search = "cube".into();
        let mut h = Harness::builder()
            .with_size([320.0, 300.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h.get_by_label("Move Cube");
        assert!(h.query_by_label("Change Light Intensity").is_none());
        h.get_by_label("Clear History").click();
        h.run();
        let s = h.state_mut();
        let actions = std::mem::take(&mut s.actions);
        for a in actions {
            crate::commands::apply(s, a);
        }
        assert!(s.undo.steps().is_empty());
    }
}
