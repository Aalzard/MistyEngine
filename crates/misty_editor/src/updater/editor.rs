//! Editor glue: installing asks to save modified scenes first (the usual save prompt), and the
//! editor may close once the new version has started.

use crate::scene_files::{self, AfterPrompt};
use crate::state::EditorState;

/// Once per editor frame: the updater's windows, and the close after a restart.
pub fn frame(ctx: &egui::Context, s: &mut EditorState) {
    if super::ui::windows(ctx, &mut s.updater) {
        request_install(s);
    }
    if s.updater.restarting() {
        // Scenes were saved (or discarded) before installing: don't ask again on close.
        s.files.quit_confirmed = true;
    }
}

/// "Update now" / the toolbar button: save prompt, then install.
pub fn request_install(s: &mut EditorState) {
    if s.code_editor.any_dirty() {
        s.console.warn(
            "Save the scripts with unsaved changes in the Code Editor, then click Update again.",
        );
        return;
    }
    if s.play.in_play_mode() {
        crate::play::stop(s);
    }
    if let Some(action) = scene_files::guard(s, AfterPrompt::InstallUpdate) {
        scene_files::perform(s, action);
    }
}
