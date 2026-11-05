//! Save games and localization in the editor: Window ▸ General ▸ Save Data and
//! Window ▸ General ▸ Localization Tables (floating windows), the Project Settings ▸
//! Localization page, and per-frame upkeep (unique Save IDs, save thumbnails).

pub mod save_data;
pub mod settings_page;
pub mod tables;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use misty_engine::localization::{Localizer, StringTable};

use crate::state::EditorState;

/// Both windows' state.
#[derive(Default)]
pub struct SaveLocWindows {
    pub save_data_open: bool,
    pub save_data: save_data::SaveDataState,
    pub tables_open: bool,
    pub tables: tables::TablesState,
    /// The project's String Tables for Inspectors (re-read every few seconds or after edits).
    cache: Option<(Instant, Vec<StringTable>)>,
    /// `MISTY_SAVE_SLOT`: save this slot after some Play frames (screenshots).
    pending_save: Option<(String, u32)>,
}

impl SaveLocWindows {
    /// Forget cached tables (after a table was edited or imported).
    pub fn tables_changed(&mut self) {
        self.cache = None;
    }
}

/// Draw the windows that are open.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    save_data::show(ctx, s);
    tables::show(ctx, s);
}

/// Every editor frame: Save IDs stay unique in Edit mode (pasted or preset-made copies).
pub fn frame(s: &mut EditorState) {
    if !s.play.in_play_mode() && misty_engine::save::fix_duplicate_ids(&mut s.scene) > 0 {
        s.scene_dirty = true;
    }
    if let Some((slot, frames)) = s.saveloc.pending_save.take() {
        if frames > 0 {
            s.saveloc.pending_save = Some((slot, frames - 1));
        } else if let Some(rt) = s.runtime.as_mut() {
            if let Err(e) = rt.save_game(&mut s.scene, &slot) {
                s.console.warn(format!("Save: {e}"));
            }
            save_data::rescan(s);
        }
    }
}

/// Screenshot setup: `MISTY_WINDOW=Localization Tables,Save Data` opens those windows,
/// `MISTY_LOCALE=fr` switches the playing game's locale, `MISTY_SAVE_SLOT=demo` saves that slot
/// after a few Play frames (so Save Data has a slot with a thumbnail).
pub fn apply_env(s: &mut EditorState) {
    if let Some(list) = std::env::var_os("MISTY_WINDOW") {
        for w in list.to_string_lossy().split(',') {
            match w.trim() {
                "Localization Tables" => tables::open_window(s),
                "Save Data" => save_data::open(s),
                _ => {}
            }
        }
    }
    if let Some(code) = std::env::var_os("MISTY_LOCALE") {
        misty_engine::localization::set_session_locale(&code.to_string_lossy());
    }
    if let Some(slot) = std::env::var_os("MISTY_SAVE_SLOT") {
        s.saveloc.pending_save = Some((slot.to_string_lossy().into_owned(), 20));
    }
}

/// The project's String Tables (cached).
pub fn project_tables(s: &mut EditorState) -> &[StringTable] {
    let stale = s
        .saveloc
        .cache
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(3));
    if stale {
        let (tables, _) = misty_engine::localization::load_tables(&s.project.root);
        s.saveloc.cache = Some((Instant::now(), tables));
    }
    &s.saveloc.cache.as_ref().expect("filled above").1
}

/// Lookups with the project's settings and tables (Inspector previews).
pub fn localizer(s: &mut EditorState) -> Localizer {
    let tables = project_tables(s).to_vec();
    Localizer::new(s.project_settings.localization.clone(), tables)
}
