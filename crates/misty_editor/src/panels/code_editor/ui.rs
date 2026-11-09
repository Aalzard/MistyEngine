//! The Code Editor tab around the text view: file tabs, info bars, the find/replace bar, Go To
//! Line, Find in Files, the status bar, and the windows (save prompts, preferences, shortcuts).

use std::time::Instant;

use egui::{Key, RichText, Ui};

use super::find::{self, FileMatch, FindOptions, Query};
use super::keys::{self, Cmd};
use super::prefs::{ScriptEditor, Settings};
use super::text::{Pos, Selection};
use super::view::{self, Completion, ViewCtx, colors};
use super::{CHECK_DELAY, CodeEditor};
use crate::actions::Action;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::widgets::{panel_toolbar, toggle_button};

/// Find / Replace bar (Ctrl+F / Ctrl+H).
#[derive(Default)]
pub struct FindBar {
    pub open: bool,
    pub replace: bool,
    pub query: String,
    pub replacement: String,
    pub opts: FindOptions,
    /// Give the query field keyboard focus next frame.
    pub focus: bool,
}

/// Find in Files panel (Ctrl+Shift+F).
#[derive(Default)]
pub struct FindInFiles {
    pub open: bool,
    pub query: String,
    pub opts: FindOptions,
    pub results: Vec<FileMatch>,
    pub searched: Option<String>,
    pub focus: bool,
}

/// A question waiting for an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Unsaved files (paths) are about to close; `quit` closes the editor afterwards.
    Close { docs: Vec<String>, quit: bool },
}

#[derive(Default)]
pub struct UiState {
    /// Give the text area keyboard focus next frame.
    pub focus_text: bool,
    /// The text area had keyboard focus last frame (it owns the shortcuts then).
    pub focused: bool,
    pub chord: Option<Key>,
    pub completion: Option<Completion>,
    pub line_clipboard: Option<String>,
    pub find: FindBar,
    /// Go To Line field text while open.
    pub goto: Option<String>,
    pub files: FindInFiles,
    pub help_open: bool,
    pub prefs_open: bool,
    pub prompt: Option<Prompt>,
    pub quit_confirmed: bool,
    /// Status-bar message and when it was shown.
    pub message: Option<(String, Instant)>,
    /// Keyboard zoom of the whole UI is off while the text area has focus.
    zoom_off: bool,
}

impl UiState {
    fn say(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }
}

/// F5 / Shift+F5 everywhere (enter / stop Play mode), unless a text field has focus.
/// Returns true while the Code Editor text area owns the keyboard (global shortcuts skip then).
pub fn global_shortcuts(ctx: &egui::Context, s: &mut EditorState) -> bool {
    if s.code_editor.ui.focused {
        return true;
    }
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    let play = s.play.in_play_mode();
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::SHIFT, Key::F5) {
            if play {
                s.push(Action::TogglePlay);
            }
        } else if i.consume_key(egui::Modifiers::NONE, Key::F5) && !play {
            s.push(Action::TogglePlay);
        }
    });
    false
}

/// The Code Editor tab.
pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let now = Instant::now();
    if s.code_editor.tick(now) {
        ui.ctx().request_repaint_after(CHECK_DELAY);
    }
    file_tabs(ui, s);
    let ce = &mut s.code_editor;
    if ce.docs.is_empty() {
        ce.ui.focused = false;
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(tr!(
                    "Double-click a script in the Project window to edit it here."
                ))
                .weak(),
            );
        });
        restore_zoom(ui.ctx(), &mut ce.ui);
        return;
    }
    ce.active = ce.active.min(ce.docs.len() - 1);
    info_bars(ui, ce);
    status_bar(ui, s);
    let ce = &mut s.code_editor;
    if ce.ui.files.open {
        find_in_files(ui, ce);
    }
    if ce.ui.find.open {
        find_bar(ui, ce);
    }
    if ce.ui.goto.is_some() {
        goto_line(ui, ce);
    }
    let matches = find_matches(ce);

    let i = ce.active;
    let (settings, ui_state) = (&ce.settings, &mut ce.ui);
    let request_focus = std::mem::take(&mut ui_state.focus_text);
    let out = egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(colors::BG))
        .show(ui, |ui| {
            view::show(
                ui,
                &mut ce.docs[i],
                ViewCtx {
                    settings,
                    chord: &mut ui_state.chord,
                    completion: &mut ui_state.completion,
                    matches: &matches,
                    line_clipboard: &mut ui_state.line_clipboard,
                    request_focus,
                },
            )
        })
        .inner;
    ce.ui.focused = out.focused;
    if let Some(msg) = out.message {
        ce.ui.say(msg);
    }
    if let Some(text) = out.copy {
        ui.ctx().copy_text(text);
    }
    // Ctrl+wheel zooms the text.
    let zoom = ui.input(|i| i.zoom_delta());
    if zoom != 1.0 && ui.ui_contains_pointer() {
        ce.settings.zoom(if zoom > 1.0 { 1.0 } else { -1.0 });
        ce.settings_changed();
    }
    // Ctrl+= / Ctrl+- belong to the editor while it has focus (not to the UI zoom).
    if ce.ui.focused && !ce.ui.zoom_off {
        ui.ctx().options_mut(|o| o.zoom_with_keyboard = false);
        ce.ui.zoom_off = true;
    } else if !ce.ui.focused {
        restore_zoom(ui.ctx(), &mut ce.ui);
    }
    for cmd in out.cmds {
        run(ui.ctx(), s, cmd);
    }
}

fn restore_zoom(ctx: &egui::Context, st: &mut UiState) {
    if st.zoom_off {
        ctx.options_mut(|o| o.zoom_with_keyboard = true);
        st.zoom_off = false;
    }
}

/// Window-level commands from the keyboard.
pub fn run(ctx: &egui::Context, s: &mut EditorState, cmd: Cmd) {
    let ce = &mut s.code_editor;
    let i = ce.active;
    match cmd {
        Cmd::Save => save(s, i),
        Cmd::SaveAll => {
            for i in 0..ce.docs.len() {
                if s.code_editor.docs[i].dirty() {
                    save(s, i);
                }
            }
        }
        Cmd::CloseDoc => request_close(ce, vec![i]),
        Cmd::NextDoc | Cmd::PrevDoc if !ce.docs.is_empty() => {
            let n = ce.docs.len();
            ce.active = if cmd == Cmd::NextDoc {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            ce.ui.focus_text = true;
        }
        Cmd::Find | Cmd::Replace => {
            let f = &mut ce.ui.find;
            f.open = true;
            f.replace = cmd == Cmd::Replace;
            f.focus = true;
            if let Some(doc) = ce.docs.get(i) {
                let p = doc.editor.primary();
                if !p.is_empty() && p.start().line == p.end().line {
                    f.query = doc.editor.buf.slice(p.start(), p.end());
                }
            }
        }
        Cmd::FindWord => {
            if let Some(doc) = ce.docs.get(i) {
                let ed = &doc.editor;
                let p = ed.primary();
                let word = if p.is_empty() {
                    ed.buf.word_at(p.head).map(|(a, b)| ed.buf.slice(a, b))
                } else {
                    Some(ed.buf.slice(p.start(), p.end()))
                };
                if let Some(w) = word {
                    ce.ui.find.query = w;
                    ce.ui.find.open = true;
                    find_step(ce, true);
                }
            }
        }
        Cmd::FindNext | Cmd::FindPrev => {
            if ce.ui.find.query.is_empty() {
                ce.ui.find.open = true;
                ce.ui.find.focus = true;
            } else {
                find_step(ce, cmd == Cmd::FindNext);
            }
        }
        Cmd::GoToLine => ce.ui.goto = Some(String::new()),
        Cmd::FindInFiles => {
            let f = &mut ce.ui.files;
            f.open = true;
            f.focus = true;
            if let Some(doc) = ce.docs.get(i) {
                let p = doc.editor.primary();
                if !p.is_empty() && p.start().line == p.end().line {
                    f.query = doc.editor.buf.slice(p.start(), p.end());
                }
            }
        }
        Cmd::ToggleWordWrap => {
            ce.settings.word_wrap = !ce.settings.word_wrap;
            ce.settings_changed();
        }
        Cmd::ZoomIn | Cmd::ZoomOut | Cmd::ZoomReset => {
            match cmd {
                Cmd::ZoomIn => ce.settings.zoom(1.0),
                Cmd::ZoomOut => ce.settings.zoom(-1.0),
                _ => ce.settings.font_size = Settings::default().font_size,
            }
            ce.settings_changed();
        }
        Cmd::Help => ce.ui.help_open = true,
        Cmd::Play if !s.play.in_play_mode() => s.push(Action::TogglePlay),
        Cmd::Stop if s.play.in_play_mode() => s.push(Action::TogglePlay),
        _ => {}
    }
    ctx.request_repaint();
}

/// Save one document, reporting to the status bar (and the Console on failure).
fn save(s: &mut EditorState, i: usize) {
    let playing = s.play.in_play_mode();
    let ce = &mut s.code_editor;
    let Some(name) = ce.docs.get(i).map(|d| d.name().to_owned()) else {
        return;
    };
    match ce.save(i) {
        Ok(()) => {
            // Scripts reload on save in Edit mode; a running game keeps the version it started with.
            ce.ui.say(if playing {
                format!("Saved {name}. Scripts reload when Play mode ends.")
            } else {
                format!("Saved {name}")
            });
            s.refresh_assets();
        }
        Err(e) => s.console.warn(format!("Could not save {name}: {e}")),
    }
}

/// Close documents, asking first when some have unsaved changes.
pub fn request_close(ce: &mut CodeEditor, docs: Vec<usize>) {
    let dirty: Vec<String> = docs
        .iter()
        .filter_map(|&i| ce.docs.get(i))
        .filter(|d| d.dirty())
        .map(|d| d.path.clone())
        .collect();
    if dirty.is_empty() {
        let mut docs = docs;
        docs.sort_unstable();
        for i in docs.into_iter().rev() {
            ce.close(i);
        }
    } else {
        // Clean ones close now; the prompt handles the rest.
        let clean: Vec<String> = docs
            .iter()
            .filter_map(|&i| ce.docs.get(i))
            .filter(|d| !d.dirty())
            .map(|d| d.path.clone())
            .collect();
        close_paths(ce, &clean);
        ce.ui.prompt = Some(Prompt::Close {
            docs: dirty,
            quit: false,
        });
    }
}

fn close_paths(ce: &mut CodeEditor, paths: &[String]) {
    for p in paths {
        if let Some(i) = ce.docs.iter().position(|d| &d.path == p) {
            ce.close(i);
        }
    }
}

/// Matches of the find bar's query in the active document (for highlighting).
fn find_matches(ce: &mut CodeEditor) -> Vec<(Pos, Pos)> {
    let f = &ce.ui.find;
    if !f.open || f.query.is_empty() {
        return Vec::new();
    }
    match (Query::new(&f.query, f.opts), ce.docs.get(ce.active)) {
        (Ok(q), Some(doc)) => q.find_all(doc.editor.buf.lines()),
        _ => Vec::new(),
    }
}

/// Select the next (or previous) match of the find bar's query.
fn find_step(ce: &mut CodeEditor, forward: bool) {
    let Ok(q) = Query::new(&ce.ui.find.query, ce.ui.find.opts) else {
        return;
    };
    let Some(doc) = ce.docs.get_mut(ce.active) else {
        return;
    };
    let matches = q.find_all(doc.editor.buf.lines());
    match find::next_match(&matches, doc.editor.primary(), forward) {
        Some((a, b)) => {
            doc.editor.set_selection(Selection::new(a, b));
            doc.view.scroll_to_caret = true;
            doc.view.center = true;
        }
        None => ce.ui.say(format!(
            "The following text was not found: {}",
            ce.ui.find.query
        )),
    }
}

/// The strip of open files.
fn file_tabs(ui: &mut Ui, s: &mut EditorState) {
    let mut close = Vec::new();
    let mut close_others = None;
    let mut reveal = None;
    let mut external = None;
    let mut save_one = None;
    let ce = &mut s.code_editor;
    panel_toolbar(ui, |ui| {
        egui::ScrollArea::horizontal()
            .id_salt("code_tabs")
            .max_width(ui.available_width() - 60.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, d) in ce.docs.iter().enumerate() {
                        let active = i == ce.active;
                        let label = if d.dirty() {
                            format!("{} ●", d.name())
                        } else {
                            d.name().to_owned()
                        };
                        let text = if active {
                            RichText::new(label).strong().color(egui::Color32::WHITE)
                        } else {
                            RichText::new(label)
                        };
                        let r = ui.selectable_label(active, text).on_hover_text(&d.path);
                        if r.clicked() {
                            ce.active = i;
                            ce.ui.focus_text = true;
                        }
                        if r.middle_clicked() {
                            close.push(i);
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr!("Save")).clicked() {
                                save_one = Some(i);
                                ui.close();
                            }
                            if ui.button(tr!("Close")).clicked() {
                                close.push(i);
                                ui.close();
                            }
                            if ui.button(tr!("Close All But This")).clicked() {
                                close_others = Some(i);
                                ui.close();
                            }
                            if ui.button(tr!("Close All")).clicked() {
                                close.extend(0..ce.docs.len());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(tr!("Copy Full Path")).clicked() {
                                ui.ctx().copy_text(d.path.clone());
                                ui.close();
                            }
                            if ui.button(tr!("Select in Project")).clicked() {
                                reveal = Some(d.path.clone());
                                ui.close();
                            }
                            if ui.button(tr!("Open in External Editor")).clicked() {
                                external = Some(d.path.clone());
                                ui.close();
                            }
                        });
                        if ui
                            .add(egui::Button::new("×").small().frame(false))
                            .on_hover_text(tr!("Close (Ctrl+F4)"))
                            .clicked()
                        {
                            close.push(i);
                        }
                        ui.separator();
                    }
                });
            });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("?")
                .on_hover_text(tr!("Keyboard shortcuts (F1)"))
                .clicked()
            {
                ce.ui.help_open = true;
            }
            ui.menu_button("⚙", |ui| settings_menu(ui, ce));
        });
    });
    if let Some(i) = save_one {
        save(s, i);
    }
    let ce = &mut s.code_editor;
    if let Some(keep) = close_others {
        close.extend((0..ce.docs.len()).filter(|&j| j != keep));
    }
    if !close.is_empty() {
        close.sort_unstable();
        close.dedup();
        request_close(ce, close);
    }
    if let Some(p) = external {
        let abs = s.assets.absolute(&p);
        if let Err(e) = crate::assets::open_external(&abs) {
            s.console.warn(format!("Could not open {p}: {e}"));
        }
    }
    if let Some(p) = reveal {
        let abs = s.assets.absolute(&p);
        if let Some(dir) = abs.parent() {
            s.project.open_folder(dir.to_path_buf());
        }
        s.project.selected = Some(abs);
        s.selection.clear();
        s.inspector_asset = Some(p);
        s.push(Action::FocusTab(crate::layout::Tab::Project));
    }
}

/// ⚙ menu: view options (also in Edit ▸ Preferences).
fn settings_menu(ui: &mut Ui, ce: &mut CodeEditor) {
    let before = ce.settings.clone();
    settings_ui(ui, &mut ce.settings);
    if ce.settings != before {
        ce.settings_changed();
    }
}

fn settings_ui(ui: &mut Ui, st: &mut Settings) {
    ui.checkbox(&mut st.word_wrap, tr!("Word Wrap (Ctrl+E, W)"));
    ui.checkbox(&mut st.minimap, tr!("Minimap"));
    ui.checkbox(&mut st.line_numbers, tr!("Line Numbers"));
    ui.checkbox(&mut st.auto_close, tr!("Auto-close Brackets and Quotes"));
    ui.horizontal(|ui| {
        let l = ui.label(tr!("Font Size"));
        ui.add(
            egui::DragValue::new(&mut st.font_size)
                .range(super::prefs::MIN_FONT..=super::prefs::MAX_FONT),
        )
        .labelled_by(l.id);
    });
    ui.horizontal(|ui| {
        let l = ui.label(tr!("Tab Size"));
        ui.add(egui::DragValue::new(&mut st.tab_size).range(1..=8))
            .labelled_by(l.id);
    });
}

/// "Changed on disk" / "deleted" bars above the text.
fn info_bars(ui: &mut Ui, ce: &mut CodeEditor) {
    let i = ce.active;
    let doc = &ce.docs[i];
    if doc.disk_conflict {
        let mut reload = false;
        let mut keep = false;
        bar(ui, |ui| {
            ui.label(
                "⚠ This file has been changed outside the editor. Reload it and lose your changes?",
            );
            reload = ui.button(tr!("Reload")).clicked();
            keep = ui.button(tr!("Keep Mine")).clicked();
        });
        if reload {
            ce.reload(i);
        } else if keep {
            ce.docs[i].disk_conflict = false;
        }
    } else if doc.missing {
        bar(ui, |ui| {
            ui.label("⚠ This file no longer exists on disk. Save (Ctrl+S) to create it again.");
        });
    }
}

fn bar(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(90, 75, 30))
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        });
}

/// Find / Replace bar.
fn find_bar(ui: &mut Ui, ce: &mut CodeEditor) {
    let total = find_matches(ce);
    let current = ce
        .docs
        .get(ce.active)
        .map(|d| d.editor.primary())
        .and_then(|p| {
            total
                .iter()
                .position(|m| (m.0, m.1) == (p.start(), p.end()))
        });
    let mut step = None;
    let mut close = false;
    let mut replace_one = false;
    let mut replace_all = false;
    let mut changed = false;
    let f = &mut ce.ui.find;
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(45, 45, 48))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let toggle = if f.replace { "⏷" } else { "⏵" };
                if ui
                    .small_button(toggle)
                    .on_hover_text(tr!("Toggle Replace (Ctrl+H)"))
                    .clicked()
                {
                    f.replace = !f.replace;
                }
                let field = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .hint_text(tr!("Find"))
                        .desired_width(220.0)
                        .id_salt("code_find_query"),
                );
                if std::mem::take(&mut f.focus) {
                    field.request_focus();
                }
                changed |= field.changed();
                // Enter = next, Shift+Enter = previous (keeping the field focused), Esc closes.
                if field.has_focus() || field.lost_focus() {
                    let (enter, shift, esc) = ui.input(|i| {
                        (
                            i.key_pressed(Key::Enter),
                            i.modifiers.shift,
                            i.key_pressed(Key::Escape),
                        )
                    });
                    if enter {
                        step = Some(!shift);
                        field.request_focus();
                    }
                    if esc {
                        close = true;
                    }
                }
                let opt = |ui: &mut Ui, on: &mut bool, label: &str, tip: &str| {
                    if toggle_button(ui, *on, label, tip).clicked() {
                        *on = !*on;
                        true
                    } else {
                        false
                    }
                };
                changed |= opt(ui, &mut f.opts.match_case, "Aa", "Match case");
                changed |= opt(ui, &mut f.opts.whole_word, "ab", "Match whole word");
                changed |= opt(ui, &mut f.opts.regex, ".*", "Use regular expressions");
                let status = match Query::new(&f.query, f.opts) {
                    Err(e) if !f.query.is_empty() => {
                        RichText::new(format!("⚠ {}", first_line(&e))).color(colors::ERROR)
                    }
                    _ if f.query.is_empty() => RichText::new(tr!("No results")).weak(),
                    _ if total.is_empty() => RichText::new(tr!("No results")).color(colors::ERROR),
                    _ => RichText::new(match current {
                        Some(c) => format!("{} of {}", c + 1, total.len()),
                        None => format!("{} results", total.len()),
                    }),
                };
                ui.label(status);
                if ui
                    .small_button("↑")
                    .on_hover_text(tr!("Previous (Shift+F3)"))
                    .clicked()
                {
                    step = Some(false);
                }
                if ui
                    .small_button("↓")
                    .on_hover_text(tr!("Next (F3)"))
                    .clicked()
                {
                    step = Some(true);
                }
                if ui
                    .small_button("×")
                    .on_hover_text(tr!("Close (Esc)"))
                    .clicked()
                {
                    close = true;
                }
            });
            if f.replace {
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut f.replacement)
                            .hint_text(tr!("Replace"))
                            .desired_width(220.0)
                            .id_salt("code_find_replacement"),
                    );
                    replace_one = ui
                        .button(tr!("Replace"))
                        .on_hover_text(tr!("Replace next"))
                        .clicked();
                    replace_all = ui.button(tr!("Replace All")).clicked();
                });
            }
        });
    // Typing in the field jumps to the first match from the caret, like incremental search.
    if changed && !ce.ui.find.query.is_empty() {
        if let Some(doc) = ce.docs.get_mut(ce.active) {
            let p = doc.editor.primary().start();
            doc.editor.set_caret(p);
        }
        find_step(ce, true);
    }
    if let Some(forward) = step {
        find_step(ce, forward);
    }
    if replace_one || replace_all {
        let (q, r) = (
            Query::new(&ce.ui.find.query, ce.ui.find.opts),
            ce.ui.find.replacement.clone(),
        );
        if let (Ok(q), Some(doc)) = (q, ce.docs.get_mut(ce.active)) {
            if replace_all {
                let n = find::replace_all(&mut doc.editor, &q, &r);
                ce.ui.say(format!("{n} occurrence(s) replaced."));
            } else {
                find::replace_next(&mut doc.editor, &q, &r);
                doc.view.scroll_to_caret = true;
            }
        }
    }
    if close {
        ce.ui.find.open = false;
        ce.ui.focus_text = true;
    }
}

fn first_line(s: &str) -> &str {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or(s)
}

/// Ctrl+G field.
fn goto_line(ui: &mut Ui, ce: &mut CodeEditor) {
    let n = ce.docs[ce.active].editor.buf.line_count();
    let mut go = None;
    let mut close = false;
    let text = ce.ui.goto.as_mut().expect("open");
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(45, 45, 48))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Line number (1 - {n}):"));
                let field = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(80.0)
                        .id_salt("code_goto_line"),
                );
                if !field.has_focus() && !field.lost_focus() {
                    field.request_focus();
                }
                let (enter, esc) =
                    ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                if (field.lost_focus() && enter) || ui.button("OK").clicked() {
                    go = text.trim().parse::<usize>().ok();
                    close = true;
                }
                if esc || ui.button(tr!("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    if let Some(line) = go {
        let doc = &mut ce.docs[ce.active];
        doc.editor.go_to_line(line.clamp(1, n));
        doc.view.scroll_to_caret = true;
        doc.view.center = true;
    }
    if close {
        ce.ui.goto = None;
        ce.ui.focus_text = true;
    }
}

/// Find in Files: query, options and the clickable results list.
fn find_in_files(ui: &mut Ui, ce: &mut CodeEditor) {
    let mut search = false;
    let mut open: Option<FileMatch> = None;
    let mut close = false;
    egui::Panel::bottom("code_find_in_files")
        .resizable(true)
        .default_size(180.0)
        .show(ui, |ui| {
            let f = &mut ce.ui.files;
            ui.horizontal(|ui| {
                ui.strong(tr!("Find in Files"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .hint_text(tr!("Find in Assets/**/*.lua"))
                        .desired_width(220.0)
                        .id_salt("code_find_files_query"),
                );
                if std::mem::take(&mut f.focus) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    search = true;
                }
                for (on, label, tip) in [
                    (&mut f.opts.match_case, "Aa", "Match case"),
                    (&mut f.opts.whole_word, "ab", "Match whole word"),
                    (&mut f.opts.regex, ".*", "Use regular expressions"),
                ] {
                    if toggle_button(ui, *on, label, tip).clicked() {
                        *on = !*on;
                    }
                }
                if ui.button(tr!("Find All")).clicked() {
                    search = true;
                }
                if let Some(q) = &f.searched {
                    let files: std::collections::BTreeSet<&str> =
                        f.results.iter().map(|r| r.path.as_str()).collect();
                    ui.label(format!(
                        "{} matches for \"{q}\" in {} files",
                        f.results.len(),
                        files.len()
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.small_button("×").clicked();
                });
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut last = "";
                    for r in &f.results {
                        if r.path != last {
                            ui.label(RichText::new(&r.path).strong());
                            last = &r.path;
                        }
                        let text = format!("    {}: {}", r.line + 1, r.text);
                        if ui
                            .add(
                                egui::Label::new(RichText::new(text).monospace())
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(tr!("Click to open"))
                            .clicked()
                        {
                            open = Some(r.clone());
                        }
                    }
                });
        });
    if search {
        let f = &mut ce.ui.files;
        match Query::new(&f.query, f.opts) {
            Ok(q) => {
                f.results = find::find_in_files(&ce.root, &q);
                f.searched = Some(f.query.clone());
            }
            Err(e) => ce.ui.say(first_line(&e).to_owned()),
        }
    }
    if let Some(r) = open
        && ce.open(&r.path, None).is_ok()
    {
        let doc = &mut ce.docs[ce.active];
        let (a, b) = (Pos::new(r.line, r.col), Pos::new(r.line, r.col + r.len));
        doc.editor.set_selection(Selection::new(a, b));
        doc.view.scroll_to_caret = true;
        doc.view.center = true;
    }
    if close {
        ce.ui.files.open = false;
    }
}

/// Caret position, file format and the syntax status.
fn status_bar(ui: &mut Ui, s: &mut EditorState) {
    let ce = &mut s.code_editor;
    let i = ce.active;
    let mut jump = None;
    egui::Panel::bottom("code_status")
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(0, 102, 184))
                .inner_margin(egui::Margin::symmetric(6, 2)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let doc = &ce.docs[i];
                match &doc.diagnostic {
                    Some(d) => {
                        let t = RichText::new(format!("⛔ Line {}: {}", d.line + 1, d.message))
                            .color(egui::Color32::WHITE);
                        if ui
                            .add(egui::Label::new(t).sense(egui::Sense::click()).truncate())
                            .on_hover_text(tr!("Go to the error"))
                            .clicked()
                        {
                            jump = Some(Pos::new(d.line, d.start));
                        }
                    }
                    None => {
                        ui.label(RichText::new("✔ No issues found").color(egui::Color32::WHITE));
                    }
                }
                if let Some((msg, at)) = &ce.ui.message
                    && at.elapsed().as_secs_f32() < 6.0
                {
                    ui.separator();
                    ui.label(RichText::new(msg).color(egui::Color32::WHITE));
                }
                if ce.ui.chord.is_none()
                    && ce
                        .ui
                        .message
                        .as_ref()
                        .is_some_and(|(m, _)| m.contains("Waiting"))
                {
                    ce.ui.message = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let white = |t: String| RichText::new(t).color(egui::Color32::WHITE);
                    ui.label(white("Lua".into()));
                    ui.label(white("UTF-8".into()));
                    ui.label(white(
                        if doc.editor.buf.crlf { "CRLF" } else { "LF" }.into(),
                    ));
                    ui.label(white(format!("Spaces: {}", doc.editor.tab_size)));
                    let p = doc.editor.primary();
                    if doc.editor.sels.len() > 1 {
                        ui.label(white(format!("{} selections", doc.editor.sels.len())));
                    } else if !p.is_empty() {
                        let n = doc.editor.buf.slice(p.start(), p.end()).chars().count();
                        ui.label(white(format!("({n} selected)")));
                    }
                    ui.label(white(format!(
                        "Ln {}, Col {}",
                        p.head.line + 1,
                        p.head.col + 1
                    )));
                });
            });
        });
    if let Some(p) = jump {
        let doc = &mut ce.docs[i];
        doc.editor.set_caret(p);
        doc.view.scroll_to_caret = true;
        doc.view.center = true;
        ce.ui.focus_text = true;
    }
}

/// Prompts, Preferences and the shortcuts list (drawn even when the tab is closed).
pub fn show_windows(ctx: &egui::Context, s: &mut EditorState) {
    prompt(ctx, s);
    preferences(ctx, s);
    help(ctx, &mut s.code_editor.ui.help_open);
}

fn prompt(ctx: &egui::Context, s: &mut EditorState) {
    let Some(Prompt::Close { docs, quit }) = s.code_editor.ui.prompt.clone() else {
        return;
    };
    let mut answer = None;
    egui::Modal::new(egui::Id::new("code_editor_save_prompt")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading(tr!("Save changes?"));
        ui.add_space(6.0);
        ui.label(tr!("Save changes to the following script(s)?"));
        for d in &docs {
            ui.label(RichText::new(format!("    {d}")).monospace());
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("Save")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Don't Save")).clicked() {
                answer = Some(false);
            }
            if ui.button(tr!("Cancel")).clicked() {
                s.code_editor.ui.prompt = None;
            }
        });
    });
    let Some(save_them) = answer else { return };
    s.code_editor.ui.prompt = None;
    if save_them {
        for p in &docs {
            if let Some(i) = s.code_editor.docs.iter().position(|d| &d.path == p) {
                save(s, i);
                if s.code_editor.docs[i].dirty() {
                    return; // The save failed: stay open.
                }
            }
        }
    }
    if quit {
        s.code_editor.ui.quit_confirmed = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else {
        close_paths(&mut s.code_editor, &docs);
    }
}

/// Edit ▸ Preferences: the external script editor choice and the Code Editor options.
fn preferences(ctx: &egui::Context, s: &mut EditorState) {
    let (mcp, project) = (&mut s.mcp, &s.project.root);
    let (ui_scale, console) = (&mut s.ui_scale, &mut s.console);
    let ce = &mut s.code_editor;
    let mut open = ce.ui.prefs_open;
    let before = ce.settings.clone();
    egui::Window::new(tr!("Preferences"))
        .id(egui::Id::new("preferences_window"))
        .open(&mut open)
        .default_size([380.0, 260.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.strong(tr!("General"));
            // The editor's language: applied from the next frame, saved per user.
            if let Some(lang) = crate::i18n::picker_row(ui, "pref_language")
                && let Err(e) = crate::i18n::choose(lang)
            {
                console.warn(format!("Language not saved: {e}"));
            }
            if let Some(v) = crate::a11y::ui_scale_row(ui, ui_scale)
                && let Err(e) = ui_scale.set(v)
            {
                console.warn(format!("UI scale not saved: {e}"));
            }
            ui.separator();
            ui.strong(tr!("External Tools"));
            ui.horizontal(|ui| {
                let l = ui.label(tr!("Script Editor"));
                egui::ComboBox::from_id_salt("pref_script_editor")
                    .selected_text(match ce.settings.script_editor {
                        ScriptEditor::Builtin => tr!("Misty Code Editor"),
                        ScriptEditor::External => tr!("System default (external)"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut ce.settings.script_editor,
                            ScriptEditor::Builtin,
                            tr!("Misty Code Editor"),
                        );
                        ui.selectable_value(
                            &mut ce.settings.script_editor,
                            ScriptEditor::External,
                            tr!("System default (external)"),
                        );
                    })
                    .response
                    .labelled_by(l.id);
            });
            ui.separator();
            ui.strong(tr!("Code Editor"));
            settings_ui(ui, &mut ce.settings);
            ui.separator();
            crate::mcp::live::preferences_ui(ui, mcp, project);
        });
    ce.ui.prefs_open = open;
    if ce.settings != before {
        ce.settings_changed();
    }
}

/// The keyboard shortcuts list.
fn help(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(tr!("Code Editor Shortcuts"))
        .open(open)
        .default_size([460.0, 520.0])
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("code_shortcuts")
                    .striped(true)
                    .num_columns(2)
                    .show(ui, |ui| {
                        let mut seen = Vec::new();
                        for b in keys::BINDINGS {
                            if seen.contains(&b.cmd) {
                                continue;
                            }
                            seen.push(b.cmd);
                            ui.label(RichText::new(b.cmd.shortcuts()).monospace());
                            ui.label(b.cmd.label());
                            ui.end_row();
                        }
                        for (k, what) in keys::BUILTIN_KEYS {
                            ui.label(RichText::new(*k).monospace());
                            ui.label(*what);
                            ui.end_row();
                        }
                    });
            });
        });
}
