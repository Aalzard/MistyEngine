//! Window ▸ Version Control: the changed files (staged, not staged, conflicts), a diff of the
//! selected text asset (or a per-object diff for scenes), stage / unstage per file, a commit
//! message and Commit, Discard with a confirmation, and the scene merge helper. Pushing and
//! pulling stay in the terminal: the window shows the commands.

use egui::{Align, Color32, Layout, RichText, Ui, vec2};

use super::scene_diff;
use super::scene_merge::{SETTINGS, SceneMerge, Side, State};
use super::status::Entry;
use super::{Vcs, git};
use crate::state::EditorState;

/// How the selected file's changes are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffView {
    /// Unified text diff.
    #[default]
    Text,
    /// Scenes: objects added, removed and changed.
    Objects,
}

#[derive(Default)]
pub struct WindowState {
    pub open: bool,
    /// Selected file (project-relative) and whether its staged side is shown.
    pub selected: Option<(String, bool)>,
    pub view: DiffView,
    /// Diff of the selection, with the status generation and view it was made for.
    cache: Option<((String, bool, DiffView, u64), Result<String, String>)>,
    pub message: String,
    /// Discard asked for this file; waiting for the confirmation.
    pub confirm_discard: Option<String>,
    /// The scene merge helper's file and state.
    pub merge: Option<(String, SceneMerge)>,
    /// Result of the last action (commit summary or an error).
    pub note: Option<(String, bool)>,
}

/// Width of the file lists, width and height of the diff.
const LIST_W: f32 = 340.0;
const DIFF_W: f32 = 600.0;
const PANE_H: f32 = 440.0;

/// Draw the window if it is open.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.vcs.window.open;
    if !open {
        return;
    }
    let root = s.project.root.clone();
    egui::Window::new("Version Control")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| contents(ui, &mut s.vcs, &root));
    s.vcs.window.open = open;
    discard_dialog(ctx, &mut s.vcs, &root);
    merge_window(ctx, &mut s.vcs, &root);
}

/// The window's inside (also drawn on its own by tests).
pub fn contents(ui: &mut Ui, v: &mut Vcs, project: &std::path::Path) {
    let Some(repo) = v.repo.clone() else {
        ui.heading("No repository");
        ui.label(
            v.error
                .as_deref()
                .unwrap_or("Looking for a git repository…"),
        );
        ui.add_space(6.0);
        ui.label("To put this project under version control, run in a terminal:");
        command_row(ui, &format!("git -C \"{}\" init", project.display()));
        if ui.button("Refresh").clicked() {
            v.refresh_now(project);
        }
        return;
    };
    header(ui, v, &repo, project);
    ui.separator();
    // Fixed sizes: a window that sizes itself from its own available space keeps growing.
    ui.horizontal_top(|ui| {
        let left = vec2(LIST_W, PANE_H);
        ui.allocate_ui_with_layout(left, Layout::top_down(Align::Min), |ui| {
            ui.set_min_size(left);
            ui.set_max_width(LIST_W);
            file_lists(ui, v, &repo, project);
        });
        ui.separator();
        let right = vec2(DIFF_W, PANE_H);
        ui.allocate_ui_with_layout(right, Layout::top_down(Align::Min), |ui| {
            ui.set_min_size(right);
            ui.set_max_width(DIFF_W);
            diff_pane(ui, v, &repo);
        });
    });
    ui.separator();
    footer(ui, &repo, v);
}

fn header(ui: &mut Ui, v: &mut Vcs, repo: &git::Repo, project: &std::path::Path) {
    ui.horizontal(|ui| {
        let st = &v.status;
        let mut branch = format!(
            "On branch {}",
            if st.branch.is_empty() {
                "?"
            } else {
                &st.branch
            }
        );
        if let Some(up) = &st.upstream {
            branch.push_str(&format!(" (tracking {up}"));
            if st.ahead > 0 {
                branch.push_str(&format!(", {} to push", st.ahead));
            }
            if st.behind > 0 {
                branch.push_str(&format!(", {} to pull", st.behind));
            }
            branch.push(')');
        }
        ui.strong(branch);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button("⟳ Refresh")
                .on_hover_text("Read the status again")
                .clicked()
            {
                v.refresh_now(project);
            }
            ui.weak(repo.root.display().to_string());
        });
    });
    if let Some((text, error)) = &v.window.note {
        let color = if *error {
            Color32::from_rgb(230, 110, 100)
        } else {
            Color32::from_rgb(120, 200, 120)
        };
        ui.colored_label(color, text);
    }
}

/// What a row asks for.
enum RowAction {
    Select(String, bool),
    Stage(String),
    Unstage(String),
    Discard(String),
    Merge(String),
    TakeSide(String, bool),
}

fn file_lists(ui: &mut Ui, v: &mut Vcs, repo: &git::Repo, project: &std::path::Path) {
    let entries = v.status.entries.clone();
    let mut act: Vec<RowAction> = Vec::new();
    let list_h = (ui.available_height() - 110.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("vcs_files")
        .max_height(list_h)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let conflicts: Vec<&Entry> = entries.iter().filter(|e| e.is_conflicted()).collect();
            if !conflicts.is_empty() {
                ui.strong(format!("Merge Conflicts ({})", conflicts.len()));
                for e in conflicts {
                    conflict_row(ui, v, e, &mut act);
                }
                ui.add_space(4.0);
            }
            let staged: Vec<&Entry> = entries.iter().filter(|e| e.is_staged()).collect();
            ui.strong(format!("Staged Changes ({})", staged.len()));
            for e in &staged {
                file_row(ui, v, e, true, &mut act);
            }
            ui.add_space(4.0);
            let unstaged: Vec<&Entry> = entries.iter().filter(|e| e.has_unstaged()).collect();
            ui.strong(format!("Changes ({})", unstaged.len()));
            for e in &unstaged {
                file_row(ui, v, e, false, &mut act);
            }
            if entries.is_empty() {
                ui.weak("Nothing to commit: the project matches the last commit.");
            }
        });
    ui.separator();
    let label = ui.label("Commit message");
    ui.add(
        egui::TextEdit::multiline(&mut v.window.message)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("What changed and why"),
    )
    .labelled_by(label.id);
    let any_staged = entries.iter().any(Entry::is_staged);
    let can_commit =
        any_staged && !v.window.message.trim().is_empty() && v.status.conflicts().next().is_none();
    ui.horizontal(|ui| {
        let commit = ui
            .add_enabled(can_commit, egui::Button::new("Commit"))
            .on_hover_text("Commit the staged changes")
            .on_disabled_hover_text("Stage some changes and write a message first");
        if commit.clicked() {
            let msg = v.window.message.trim().to_owned();
            v.window.note = Some(match repo.commit(&msg) {
                Ok(summary) => {
                    v.window.message.clear();
                    (format!("Committed {summary}"), false)
                }
                Err(e) => (format!("Commit failed: {e}"), true),
            });
            v.refresh_now(project);
        }
        let unstaged: Vec<String> = entries
            .iter()
            .filter(|e| e.has_unstaged() && !e.is_conflicted())
            .map(|e| e.path.clone())
            .collect();
        if ui
            .add_enabled(!unstaged.is_empty(), egui::Button::new("Stage All"))
            .clicked()
        {
            act.extend(unstaged.into_iter().map(RowAction::Stage));
        }
    });
    apply(v, repo, project, act);
}

fn file_row(ui: &mut Ui, v: &Vcs, e: &Entry, staged: bool, act: &mut Vec<RowAction>) {
    ui.horizontal(|ui| {
        let mut on = staged;
        let what = if staged { "Unstage" } else { "Stage" };
        let check = ui
            .add(egui::Checkbox::without_text(&mut on))
            .on_hover_text(format!("{what} this file"));
        let name = format!("{what} {}", e.path);
        check.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, staged, &name)
        });
        if check.changed() {
            act.push(if on {
                RowAction::Stage(e.path.clone())
            } else {
                RowAction::Unstage(e.path.clone())
            });
        }
        let badge = e.badge();
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
        super::paint_badge(ui, rect.center(), badge, 13.0);
        let selected = v.window.selected.as_ref() == Some(&(e.path.clone(), staged));
        let mut text = e.path.clone();
        if let Some(from) = &e.from {
            text = format!("{from} → {text}");
        }
        let row = ui
            .selectable_label(selected, text)
            .on_hover_text(badge.label());
        if row.clicked() {
            act.push(RowAction::Select(e.path.clone(), staged));
        }
        row.context_menu(|ui| {
            if ui.button("Discard Changes…").clicked() {
                act.push(RowAction::Discard(e.path.clone()));
                ui.close();
            }
        });
    });
}

fn conflict_row(ui: &mut Ui, v: &Vcs, e: &Entry, act: &mut Vec<RowAction>) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
        super::paint_badge(ui, rect.center(), e.badge(), 13.0);
        let selected = v.window.selected.as_ref() == Some(&(e.path.clone(), false));
        if ui.selectable_label(selected, &e.path).clicked() {
            act.push(RowAction::Select(e.path.clone(), false));
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        if e.path.ends_with(".scene") && ui.button("Merge Scene…").clicked() {
            act.push(RowAction::Merge(e.path.clone()));
        }
        if ui
            .button("Use Ours")
            .on_hover_text("Keep your version of the whole file")
            .clicked()
        {
            act.push(RowAction::TakeSide(e.path.clone(), true));
        }
        if ui
            .button("Use Theirs")
            .on_hover_text("Take the incoming version of the whole file")
            .clicked()
        {
            act.push(RowAction::TakeSide(e.path.clone(), false));
        }
    });
}

fn apply(v: &mut Vcs, repo: &git::Repo, project: &std::path::Path, actions: Vec<RowAction>) {
    if actions.is_empty() {
        return;
    }
    let mut changed = false;
    for a in actions {
        let result = match a {
            RowAction::Select(path, staged) => {
                v.window.selected = Some((path, staged));
                continue;
            }
            RowAction::Discard(path) => {
                v.window.confirm_discard = Some(path);
                continue;
            }
            RowAction::Merge(path) => {
                v.window.merge = match open_merge(repo, &path) {
                    Ok(m) => Some((path, m)),
                    Err(e) => {
                        v.window.note = Some((format!("Can't merge {path}: {e}"), true));
                        None
                    }
                };
                continue;
            }
            RowAction::Stage(path) => repo.stage(&path).map(|_| format!("Staged {path}")),
            RowAction::Unstage(path) => repo.unstage(&path).map(|_| format!("Unstaged {path}")),
            RowAction::TakeSide(path, ours) => repo.take_side(&path, ours).map(|_| {
                format!(
                    "Resolved {path} with {}",
                    if ours { "ours" } else { "theirs" }
                )
            }),
        };
        changed = true;
        v.window.note = Some(match result {
            Ok(t) => (t, false),
            Err(e) => (e, true),
        });
    }
    if changed {
        v.refresh_now(project);
        // A staged / unstaged file: follow it to the side it is on now.
        if let Some((path, _)) = v.window.selected.clone()
            && let Some(e) = v.status.entry(&path)
        {
            let staged = e.is_staged() && !e.has_unstaged();
            v.window.selected = Some((path, staged));
        }
    }
}

/// The base, ours and theirs versions of a conflicted scene.
pub fn open_merge(repo: &git::Repo, rel: &str) -> Result<SceneMerge, String> {
    let base = repo.show(":1", rel).unwrap_or_default();
    let ours = repo.show(":2", rel)?;
    let theirs = repo.show(":3", rel)?;
    SceneMerge::new(&base, &ours, &theirs)
}

fn diff_pane(ui: &mut Ui, v: &mut Vcs, repo: &git::Repo) {
    let Some((path, staged)) = v.window.selected.clone() else {
        ui.weak("Select a file to see its changes.");
        return;
    };
    let Some(entry) = v.status.entry(&path).cloned() else {
        ui.weak("Select a file to see its changes.");
        return;
    };
    let is_scene = path.ends_with(".scene");
    ui.horizontal(|ui| {
        ui.strong(&path);
        ui.weak(if staged { "(staged)" } else { "" });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !entry.is_conflicted() && ui.button("Discard Changes…").clicked() {
                v.window.confirm_discard = Some(path.clone());
            }
            if is_scene && !entry.is_untracked() && !entry.is_conflicted() {
                ui.selectable_value(&mut v.window.view, DiffView::Objects, "Objects");
                ui.selectable_value(&mut v.window.view, DiffView::Text, "Text");
            }
        });
    });
    ui.separator();
    let view = if is_scene {
        v.window.view
    } else {
        DiffView::Text
    };
    let key = (path.clone(), staged, view, v.generation);
    if v.window.cache.as_ref().is_none_or(|(k, _)| *k != key) {
        let made = make_diff(repo, &entry, staged, view);
        v.window.cache = Some((key, made));
    }
    let Some((_, made)) = &v.window.cache else {
        return;
    };
    egui::ScrollArea::both()
        .id_salt("vcs_diff")
        .auto_shrink(false)
        .show(ui, |ui| match made {
            Err(e) => {
                ui.colored_label(Color32::from_rgb(230, 110, 100), e);
            }
            Ok(text) if view == DiffView::Objects => object_diff_ui(ui, text),
            Ok(text) => text_diff_ui(ui, text),
        });
}

fn make_diff(repo: &git::Repo, e: &Entry, staged: bool, view: DiffView) -> Result<String, String> {
    if e.is_conflicted() {
        return Ok(
            "This file has merge conflicts. Use the buttons on the left to resolve it.".into(),
        );
    }
    if !git::is_text_asset(&e.path) {
        return Ok(format!(
            "{} — binary file (no text diff).",
            e.badge().label()
        ));
    }
    if view == DiffView::Objects {
        // Before: HEAD (or the index for unstaged changes); after: the index or the file.
        let before = if staged {
            repo.show("HEAD", &e.path).unwrap_or_default()
        } else {
            repo.show("", &e.path)
                .or_else(|_| repo.show("HEAD", &e.path))
                .unwrap_or_default()
        };
        let after = if staged {
            repo.show("", &e.path)?
        } else {
            std::fs::read_to_string(repo.project.join(&e.path)).unwrap_or_default()
        };
        let empty = r#"{"objects": []}"#;
        let pick = |t: &str| {
            if t.trim().is_empty() {
                empty.to_owned()
            } else {
                t.to_owned()
            }
        };
        return scene_diff::diff_text(&pick(&before), &pick(&after)).map(|d| d.text());
    }
    let text = repo.diff(&e.path, staged, e.is_untracked())?;
    Ok(if text.trim().is_empty() {
        "No text changes.".into()
    } else {
        text
    })
}

const MAX_LINES: usize = 5000;

fn text_diff_ui(ui: &mut Ui, text: &str) {
    for (i, line) in text.lines().enumerate() {
        if i == MAX_LINES {
            ui.weak("… (diff cut short)");
            break;
        }
        let color = if line.starts_with("+++") || line.starts_with("---") {
            Color32::from_rgb(170, 170, 170)
        } else if line.starts_with('+') {
            Color32::from_rgb(120, 200, 120)
        } else if line.starts_with('-') {
            Color32::from_rgb(230, 110, 100)
        } else if line.starts_with("@@") {
            Color32::from_rgb(110, 160, 230)
        } else {
            ui.visuals().text_color()
        };
        ui.label(RichText::new(line).monospace().color(color));
    }
}

fn object_diff_ui(ui: &mut Ui, text: &str) {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let color = if trimmed.starts_with("+ ") {
            Color32::from_rgb(120, 200, 120)
        } else if trimmed.starts_with("- ") {
            Color32::from_rgb(230, 110, 100)
        } else if line.starts_with("~ ") {
            Color32::from_rgb(220, 170, 80)
        } else {
            ui.visuals().text_color()
        };
        let rt = RichText::new(line).monospace().color(color);
        ui.label(if line.starts_with(' ') {
            rt
        } else {
            rt.strong()
        });
    }
}

fn footer(ui: &mut Ui, repo: &git::Repo, v: &Vcs) {
    ui.horizontal_wrapped(|ui| {
        ui.weak("Pushing and pulling happen in a terminal:");
        let root = repo.root.display().to_string();
        let push = format!("git -C \"{root}\" push");
        let pull = format!("git -C \"{root}\" pull");
        command_row(ui, &push);
        command_row(ui, &pull);
        if v.status.upstream.is_none() && !v.status.branch.is_empty() {
            ui.weak(format!(
                "(no upstream yet: git push -u origin {})",
                v.status.branch
            ));
        }
    });
}

/// A command in monospace with a Copy button.
fn command_row(ui: &mut Ui, cmd: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(cmd).monospace());
        if ui
            .small_button("Copy")
            .on_hover_text("Copy the command")
            .clicked()
        {
            ui.ctx().copy_text(cmd.to_owned());
        }
    });
}

/// "Discard changes to X?" with Discard / Cancel.
fn discard_dialog(ctx: &egui::Context, v: &mut Vcs, project: &std::path::Path) {
    let Some(path) = v.window.confirm_discard.clone() else {
        return;
    };
    let mut answer = None;
    egui::Window::new("Discard changes?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| answer = discard_prompt(ui, v, &path));
    match answer {
        Some(true) => discard(v, project, &path),
        Some(false) => v.window.confirm_discard = None,
        None => {}
    }
}

/// The prompt's inside; `Some(true)` = discard.
pub fn discard_prompt(ui: &mut Ui, v: &Vcs, path: &str) -> Option<bool> {
    let untracked = v
        .status
        .entry(path)
        .is_some_and(|e| e.is_untracked() || e.index == 'A');
    ui.label(format!("Discard all changes to {path}?"));
    ui.label(if untracked {
        "The file is new: it will be moved to the trash."
    } else {
        "It goes back to the last commit. Your changes can't be recovered."
    });
    let mut answer = None;
    ui.horizontal(|ui| {
        if ui.button("Discard").clicked() {
            answer = Some(true);
        }
        if ui.button("Cancel").clicked() {
            answer = Some(false);
        }
    });
    answer
}

pub fn discard(v: &mut Vcs, project: &std::path::Path, path: &str) {
    v.window.confirm_discard = None;
    let (Some(repo), Some(e)) = (v.repo.clone(), v.status.entry(path).cloned()) else {
        return;
    };
    v.window.note = Some(match repo.discard(path, e.index, e.worktree) {
        Ok(()) => (format!("Discarded changes to {path}"), false),
        Err(err) => (format!("Discard failed: {err}"), true),
    });
    v.refresh_now(project);
}

/// The scene merge helper window.
fn merge_window(ctx: &egui::Context, v: &mut Vcs, project: &std::path::Path) {
    let Some((path, _)) = &v.window.merge else {
        return;
    };
    let title = format!("Merge {}", path.rsplit('/').next().unwrap_or(path));
    let mut open = true;
    let mut done = false;
    egui::Window::new(title)
        .id(egui::Id::new("vcs_merge_window"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| done = merge_contents(ui, v, project));
    if !open || done {
        v.window.merge = None;
    }
}

/// The merge helper's inside; true once the merged scene was written.
pub fn merge_contents(ui: &mut Ui, v: &mut Vcs, project: &std::path::Path) -> bool {
    let Some((path, merge)) = v.window.merge.as_mut() else {
        return false;
    };
    let path = path.clone();
    ui.label(format!(
        "{} objects differ, {} changed on both sides. Pick whose version each object keeps.",
        merge.choices().count(),
        merge.conflicts()
    ));
    ui.horizontal(|ui| {
        if ui.button("All Ours").clicked() {
            merge.set_all(Side::Ours);
        }
        if ui.button("All Theirs").clicked() {
            merge.set_all(Side::Theirs);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("vcs_merge_items")
        .max_height(360.0)
        .min_scrolled_width(640.0)
        .auto_shrink([true, true])
        .show(ui, |ui| {
            egui::Grid::new("vcs_merge_grid")
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    ui.strong("Object");
                    ui.strong("Keep");
                    ui.strong("Ours");
                    ui.strong("Theirs");
                    ui.end_row();
                    let keys: Vec<String> = merge.choices().map(|i| i.key.clone()).collect();
                    for key in keys {
                        let Some(item) = merge.items.iter_mut().find(|i| i.key == key) else {
                            continue;
                        };
                        let name = if item.state == State::Conflict {
                            RichText::new(format!("⚠ {key}")).color(Color32::from_rgb(230, 150, 80))
                        } else {
                            RichText::new(&key)
                        };
                        ui.label(name);
                        ui.horizontal(|ui| {
                            let o = ui.radio_value(&mut item.pick, Side::Ours, "Ours");
                            o.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::RadioButton,
                                    true,
                                    item_pick_is(&item.pick, Side::Ours),
                                    format!("Ours for {key}"),
                                )
                            });
                            let t = ui.radio_value(&mut item.pick, Side::Theirs, "Theirs");
                            t.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::RadioButton,
                                    true,
                                    item_pick_is(&item.pick, Side::Theirs),
                                    format!("Theirs for {key}"),
                                )
                            });
                        });
                        ui.label(&item.ours);
                        ui.label(&item.theirs);
                        ui.end_row();
                    }
                });
        });
    ui.separator();
    let mut done = false;
    ui.horizontal(|ui| {
        if ui
            .button("Save Merged Scene")
            .on_hover_text("Write the merged scene and mark the conflict resolved (staged)")
            .clicked()
        {
            let text = merge.merged_text();
            let result = v.repo.as_ref().map(|r| r.resolve_with(&path, &text));
            v.window.note = Some(match result {
                Some(Ok(())) => (format!("Merged {path} and marked it resolved"), false),
                Some(Err(e)) => (format!("Merge not saved: {e}"), true),
                None => ("No repository".into(), true),
            });
            done = true;
        }
        ui.weak(format!(
            "Settings (lighting, navigation) are the \"{SETTINGS}\" row."
        ));
    });
    if done {
        v.refresh_now(project);
    }
    done
}

fn item_pick_is(pick: &Side, side: Side) -> bool {
    *pick == side
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::super::git::{git, tests::temp_repo};
    use super::*;

    struct Ui0 {
        vcs: Vcs,
        project: std::path::PathBuf,
    }

    fn harness(vcs: Vcs, project: std::path::PathBuf) -> Harness<'static, Ui0> {
        let mut h = Harness::builder()
            .with_size([1000.0, 640.0])
            .build_ui_state(
                |ui, s: &mut Ui0| {
                    let p = s.project.clone();
                    contents(ui, &mut s.vcs, &p);
                },
                Ui0 { vcs, project },
            );
        h.run_steps(2);
        h
    }

    #[test]
    fn stage_commit_and_discard_from_the_window() {
        let (top, project) = temp_repo("window");
        std::fs::write(
            project.join("Assets/Scripts/Player.lua"),
            "local speed = 3\n",
        )
        .unwrap();
        std::fs::write(project.join("Assets/Readme.txt"), "edited\n").unwrap();
        let mut v = Vcs::default();
        v.refresh_now(&project);
        let mut h = harness(v, project.clone());
        h.get_by_label("Changes (2)");
        let unnamed = crate::a11y::unnamed_controls(&h);
        assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
        // Select the script: its diff shows.
        h.get_by_label("Assets/Scripts/Player.lua").click();
        h.run_steps(2);
        h.get_by_label("+local speed = 3");
        // Stage it with its checkbox, write a message, commit.
        h.get_by_label("Stage Assets/Scripts/Player.lua").click();
        h.run_steps(2);
        h.get_by_label("Staged Changes (1)");
        h.state_mut().vcs.window.message = "Faster player".into();
        h.run();
        h.get_by_label("Commit").click();
        h.run_steps(2);
        let log = git(&top, &["log", "-1", "--format=%s"]).unwrap();
        assert_eq!(log.trim(), "Faster player");
        assert!(
            h.state()
                .vcs
                .status
                .entry("Assets/Scripts/Player.lua")
                .is_none()
        );

        // Discard the Readme: confirmation first, then it is back to the commit.
        h.state_mut().vcs.window.selected = Some(("Assets/Readme.txt".into(), false));
        h.run_steps(2);
        h.get_by_label("Discard Changes…").click();
        h.run();
        assert_eq!(
            h.state().vcs.window.confirm_discard.as_deref(),
            Some("Assets/Readme.txt")
        );
        assert_eq!(
            std::fs::read_to_string(project.join("Assets/Readme.txt")).unwrap(),
            "edited\n"
        );
        let s = h.state_mut();
        discard(&mut s.vcs, &project, "Assets/Readme.txt");
        assert_eq!(
            std::fs::read_to_string(project.join("Assets/Readme.txt")).unwrap(),
            "hello\n"
        );
        assert!(s.vcs.status.entries.is_empty());
        h.run_steps(2);
        h.get_by_label("Nothing to commit: the project matches the last commit.");
        // No push / pull buttons, only the commands.
        assert!(h.query_by_label("Push").is_none() && h.query_by_label("Pull").is_none());
        assert!(h.query_all_by_label_contains("push").count() >= 1);
        std::fs::remove_dir_all(top).ok();
    }

    #[test]
    fn scene_object_diff_in_the_window() {
        let (top, project) = temp_repo("scenediff");
        let scenes = project.join("Assets/Scenes");
        std::fs::create_dir_all(&scenes).unwrap();
        let base = super::super::scene_doc::tests::sample();
        std::fs::write(scenes.join("Main.scene"), &base).unwrap();
        git(&top, &["add", "--", "Proj/Assets/Scenes/Main.scene"]).unwrap();
        git(&top, &["commit", "--quiet", "-m", "scene"]).unwrap();
        std::fs::write(scenes.join("Main.scene"), base.replace("60.0", "45.0")).unwrap();
        let mut v = Vcs::default();
        v.refresh_now(&project);
        v.window.selected = Some(("Assets/Scenes/Main.scene".into(), false));
        v.window.view = DiffView::Objects;
        let h = harness(v, project);
        h.get_by_label("~ Main Camera");
        h.get_by_label("    Camera.fov: 60 → 45");
        std::fs::remove_dir_all(top).ok();
    }

    #[test]
    fn merging_a_conflicted_scene_per_object() {
        let (top, project) = temp_repo("merge");
        let scenes = project.join("Assets/Scenes");
        std::fs::create_dir_all(&scenes).unwrap();
        let file = scenes.join("Main.scene");
        let base = super::super::scene_doc::tests::sample();
        std::fs::write(&file, &base).unwrap();
        let rel = "Proj/Assets/Scenes/Main.scene";
        git(&top, &["add", "--", rel]).unwrap();
        git(&top, &["commit", "--quiet", "-m", "scene"]).unwrap();
        git(&top, &["checkout", "--quiet", "-b", "other"]).unwrap();
        std::fs::write(
            &file,
            base.replace("60.0", "90.0")
                .replace("[0.0, 1.0, 0.0]", "[0.0, 9.0, 0.0]"),
        )
        .unwrap();
        git(&top, &["commit", "--quiet", "-am", "theirs"]).unwrap();
        git(&top, &["checkout", "--quiet", "main"]).unwrap();
        std::fs::write(&file, base.replace("60.0", "70.0")).unwrap();
        git(&top, &["commit", "--quiet", "-am", "ours"]).unwrap();
        assert!(
            git(&top, &["merge", "--quiet", "other"]).is_err(),
            "conflict"
        );

        let mut v = Vcs::default();
        v.refresh_now(&project);
        let entry = v.status.entry("Assets/Scenes/Main.scene").unwrap();
        assert!(entry.is_conflicted());
        let m = open_merge(v.repo.as_ref().unwrap(), "Assets/Scenes/Main.scene").unwrap();
        v.window.merge = Some(("Assets/Scenes/Main.scene".into(), m));
        let mut h = Harness::builder().with_size([900.0, 500.0]).build_ui_state(
            |ui, s: &mut Ui0| {
                let p = s.project.clone();
                merge_contents(ui, &mut s.vcs, &p);
            },
            Ui0 {
                vcs: v,
                project: project.clone(),
            },
        );
        h.run_steps(2);
        // Camera changed on both sides (conflict, starts on ours); Player only on theirs.
        h.get_by_label("⚠ Main Camera");
        h.get_by_label("Theirs for Main Camera").click();
        h.run();
        h.get_by_label("Save Merged Scene").click();
        h.run_steps(2);
        let merged =
            super::super::scene_doc::SceneDoc::parse(&std::fs::read_to_string(&file).unwrap())
                .unwrap();
        assert_eq!(
            merged.get("Main Camera").unwrap().components["Camera"]["fov"],
            90.0
        );
        assert_eq!(
            merged.get("Player").unwrap().components["Transform"]["position"][1],
            9.0
        );
        let s = h.state();
        assert!(
            s.vcs.status.conflicts().next().is_none(),
            "resolved and staged"
        );
        assert!(
            s.vcs
                .status
                .entry("Assets/Scenes/Main.scene")
                .unwrap()
                .is_staged()
        );
        std::fs::remove_dir_all(top).ok();
    }
}
