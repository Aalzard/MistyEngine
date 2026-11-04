//! In-editor interactive tutorials (Help ▸ Tutorials).
//!
//! A tutorial is data (`data.rs`, JSON files in `assets/tutorials/`): steps with a short text,
//! the UI elements to highlight (`targets.rs`), an optional condition that finishes the step on
//! its own (`conditions.rs`) and a demo that does the step in code (`demos.rs`). Starting one
//! opens a fresh Untitled scene (`scenes.rs`), then a floating step card (`card.rs`) walks
//! through the steps. Finished tutorials are remembered per user (`progress.rs`).
//!
//! **Misty Hub:** [`list`] returns every tutorial's id, title, description and minutes for the
//! Learn page; [`progress_file`] is where finished IDs are saved (JSON `{"done": [...]}`).
//! Launching the editor with `MISTY_TUTORIAL=<id>` starts that tutorial (see [`from_env`]).

mod card;
mod conditions;
mod data;
mod demos;
mod progress;
mod scenes;
pub mod targets;
#[cfg(test)]
mod tests;
mod window;

use crate::scene_files::{self, AfterPrompt};
use crate::state::EditorState;
use data::Tutorial;
pub use progress::Progress;

/// One row of the tutorial list.
#[derive(Clone, Debug, PartialEq)]
pub struct TutorialInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Estimated time in minutes.
    pub minutes: u32,
}

/// Every built-in tutorial, in display order (for the Tutorials window and the Hub).
pub fn list() -> Vec<TutorialInfo> {
    data::all()
        .iter()
        .map(|t| TutorialInfo {
            id: &t.id,
            title: &t.title,
            description: &t.description,
            minutes: t.minutes,
        })
        .collect()
}

/// Where finished tutorials are saved for this user.
pub fn progress_file() -> Option<std::path::PathBuf> {
    progress::default_file()
}

/// Editor-wide tutorial state.
pub struct Tutorials {
    /// The Tutorials window is shown.
    pub window_open: bool,
    pub run: Option<Run>,
    pub progress: Progress,
    /// Waiting for the save prompt before starting this tutorial.
    pending: Option<String>,
}

impl Default for Tutorials {
    fn default() -> Self {
        // Tests never touch the real per-user file.
        let file = if cfg!(test) { None } else { progress_file() };
        Self {
            window_open: false,
            run: None,
            progress: Progress::load(file),
            pending: None,
        }
    }
}

/// The tutorial being followed.
pub struct Run {
    pub tutorial: &'static Tutorial,
    pub step: usize,
    /// Undo step names when the step began (`undo:` conditions).
    undo_before: Vec<String>,
    /// The condition was false at some point during this step (it advances on a change).
    seen_false: bool,
    /// When the condition became true (the step advances shortly after).
    done_at: Option<f64>,
    /// When the step began (the highlight pulses for a moment).
    started_at: Option<f64>,
}

impl Run {
    pub fn step(&self) -> &'static data::Step {
        &self.tutorial.steps[self.step]
    }
}

/// Seconds between a step's condition coming true and the next step.
const ADVANCE_DELAY: f64 = 0.7;

/// Start tutorial `id` in a new scene (asks to save changes first). False if unknown.
pub fn start(s: &mut EditorState, id: &str) -> bool {
    let Some(t) = data::find(id) else {
        return false;
    };
    s.tutorials.pending = None;
    match scene_files::guard(s, AfterPrompt::NewScene) {
        Some(a) => {
            scene_files::perform(s, a);
            begin(s, t);
        }
        // The save prompt is up: start once it closes with a new scene.
        None if s.files.prompt.is_some() => s.tutorials.pending = Some(id.to_owned()),
        None => {}
    }
    true
}

/// Set up the starting scene and show the first step.
fn begin(s: &mut EditorState, t: &'static Tutorial) {
    if !scenes::setup(s, &t.scene) {
        s.console
            .warn(format!("Tutorial '{}': unknown scene '{}'.", t.id, t.scene));
    }
    // The setup is part of the new scene, not an edit.
    scene_files::reset_after_load(s);
    if !s.world_builder.busy() {
        s.world_builder = Default::default();
    }
    s.terrain_tools.layer = 0;
    s.push(crate::actions::Action::FocusTab(crate::layout::Tab::Scene));
    s.tutorials.window_open = false;
    s.tutorials.run = Some(Run {
        tutorial: t,
        step: 0,
        undo_before: Vec::new(),
        seen_false: false,
        done_at: None,
        started_at: None,
    });
    enter_step(s, 0);
    targets::set_enabled(true);
}

/// Show step `i` of the running tutorial.
fn enter_step(s: &mut EditorState, i: usize) {
    let undo: Vec<String> = s.undo.steps().into_iter().map(str::to_owned).collect();
    let Some(run) = s.tutorials.run.as_mut() else {
        return;
    };
    run.step = i.min(run.tutorial.steps.len() - 1);
    run.undo_before = undo;
    run.seen_false = false;
    run.done_at = None;
    run.started_at = None;
    targets::request_scroll(run.step().targets.first().map(String::as_str));
}

/// Go to the next step, or finish after the last one.
pub fn next(s: &mut EditorState) {
    let Some(run) = &s.tutorials.run else {
        return;
    };
    if run.step + 1 < run.tutorial.steps.len() {
        let i = run.step + 1;
        enter_step(s, i);
    } else {
        finish(s);
    }
}

pub fn back(s: &mut EditorState) {
    if let Some(i) = s.tutorials.run.as_ref().map(|r| r.step) {
        enter_step(s, i.saturating_sub(1));
    }
}

/// The last step is done: remember it and close the card.
pub fn finish(s: &mut EditorState) {
    let Some(run) = s.tutorials.run.take() else {
        return;
    };
    if let Err(e) = s.tutorials.progress.complete(&run.tutorial.id) {
        s.console
            .warn(format!("Could not save tutorial progress: {e}"));
    }
    s.console
        .info(format!("Tutorial finished: {}", run.tutorial.title));
    targets::set_enabled(false);
}

/// Skip the rest: counts as done (for people who know it already).
pub fn skip(s: &mut EditorState) {
    finish(s);
}

/// Close the card without finishing.
pub fn exit(s: &mut EditorState) {
    s.tutorials.run = None;
    targets::set_enabled(false);
}

/// Jump to step `i` doing the demos of the steps before it (screenshots, tests).
pub fn fast_forward(s: &mut EditorState, i: usize) {
    let Some(t) = s.tutorials.run.as_ref().map(|r| r.tutorial) else {
        return;
    };
    for step in t.steps.iter().take(i) {
        if let Some(demo) = &step.demo
            && !demos::run(s, demo)
        {
            s.console.warn(format!("Unknown tutorial demo '{demo}'."));
        }
    }
    enter_step(s, i);
}

/// `MISTY_TUTORIAL=<id>` starts a tutorial at launch; `<id>:<step>` (1-based) jumps to a step
/// by doing the earlier steps (screenshots); `window` opens the Tutorials window.
pub fn from_env(s: &mut EditorState) {
    let Some(v) = std::env::var_os("MISTY_TUTORIAL") else {
        return;
    };
    let v = v.to_string_lossy().into_owned();
    if v == "window" {
        s.tutorials.window_open = true;
        return;
    }
    let (id, step) = match v.split_once(':') {
        Some((id, n)) => (id.to_owned(), n.trim().parse::<usize>().unwrap_or(1)),
        None => (v, 1),
    };
    // Launch: the open scene is replaced without asking (nothing is saved or lost on disk).
    s.scene_dirty = false;
    s.files.dirty.clear();
    if start(s, &id) {
        fast_forward(s, step.saturating_sub(1));
    } else {
        s.console
            .warn(format!("MISTY_TUTORIAL: no tutorial '{id}'."));
    }
}

/// Per frame, after the panels: pending starts, the step condition, the card and the window.
pub fn frame(ctx: &egui::Context, s: &mut EditorState) {
    if let Some(id) = s.tutorials.pending.clone()
        && s.files.prompt.is_none()
    {
        s.tutorials.pending = None;
        let fresh = !s.scene_dirty && s.scene.scene_path(0).is_none();
        if let (true, Some(t)) = (fresh, data::find(&id)) {
            begin(s, t);
        }
    }
    let now = ctx.input(|i| i.time);
    update(ctx, s, now);
    if let Some(nav) = card::show(ctx, s, now) {
        match nav {
            card::Nav::Back => back(s),
            card::Nav::Next => next(s),
            card::Nav::Skip => skip(s),
            card::Nav::Exit => exit(s),
        }
    }
    window::show(ctx, s);
    targets::set_enabled(s.tutorials.run.is_some());
}

/// Check the step's condition and advance a moment after it comes true.
fn update(ctx: &egui::Context, s: &mut EditorState, now: f64) {
    if s.tutorials.run.is_none() {
        return;
    }
    let met = condition_met(ctx, s);
    let run = s.tutorials.run.as_mut().expect("checked above");
    run.started_at.get_or_insert(now);
    match (met, run.seen_false, run.done_at) {
        (Some(false), _, _) => {
            run.seen_false = true;
            run.done_at = None;
        }
        (Some(true), true, None) => run.done_at = Some(now),
        (Some(true), true, Some(t)) if now - t >= ADVANCE_DELAY => next(s),
        _ => {}
    }
    if s.tutorials
        .run
        .as_ref()
        .is_some_and(|r| r.done_at.is_some())
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// The current step's condition: `None` without one (or an unknown one).
fn condition_met(ctx: &egui::Context, s: &EditorState) -> Option<bool> {
    let run = s.tutorials.run.as_ref()?;
    let cond = run.step().done_when.as_deref()?;
    let check = conditions::Check {
        state: s,
        ctx,
        undo_before: &run.undo_before,
    };
    conditions::eval(cond, &check)
}
