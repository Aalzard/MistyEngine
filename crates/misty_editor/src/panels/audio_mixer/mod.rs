//! Audio Mixer window (Window ▸ Audio ▸ Audio Mixer): the project's mixers, the shown mixer's
//! snapshots, group tree and exposed parameters on the left, one channel strip per group on
//! the right. The selected group / effect is edited in the Inspector. In Play mode faders and
//! meters are live, and edits go straight to the running game.

mod strip;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use misty_engine::audio::amplitude_to_db;
use misty_engine::audio::effects::MIN_DB;
use misty_engine::audio::mixer::{AudioMixer, MASTER};
use misty_engine::scene::clip_name;

use crate::actions::{Action, AssetKind};
use crate::anim_controllers::interacting;
use crate::audio_mixers;
use crate::state::EditorState;
use crate::theme;

pub use strip::add_effect_menu;

/// Icon of Audio Mixers (Project window, Inspector, tab title).
pub const MIXER_ICON: &str = "📶";
const SIDEBAR_W: f32 = 220.0;
/// How fast the VU meters fall (dB per second).
const METER_FALL: f32 = 30.0;

/// Something selected in the window (shown in the Inspector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MixerItem {
    /// A group, with one of its effects highlighted.
    Group(u32, Option<u32>),
}

impl MixerItem {
    pub fn group(self) -> u32 {
        match self {
            MixerItem::Group(g, _) => g,
        }
    }
}

/// What is being renamed in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rename {
    Group(u32),
    Snapshot(usize),
}

/// Audio Mixer window state (kept while the tab is closed).
#[derive(Default)]
pub struct MixerWindow {
    /// Mixer shown (project-relative path).
    pub mixer: Option<String>,
    /// Snapshot faders edit in Edit mode.
    pub snapshot: usize,
    pub selected: Option<MixerItem>,
    /// Displayed meter level per group (dB, falling).
    levels: HashMap<u32, f32>,
    /// Displayed gain reduction of each group's Duck Volume / Compressor (dB, recovering).
    reductions: HashMap<u32, f32>,
    /// Mixer and store generation last sent to the running game.
    pushed: Option<(String, u64)>,
    renaming: Option<(Rename, String)>,
}

impl MixerWindow {
    /// Show a mixer (a new one clears the selection).
    pub fn open(&mut self, path: Option<String>) {
        if self.mixer != path {
            self.mixer = path;
            self.selected = None;
            self.snapshot = 0;
            self.levels.clear();
            self.reductions.clear();
            self.renaming = None;
        }
    }

    /// Meter level of a group (dB).
    pub fn level(&self, group: u32) -> f32 {
        self.levels.get(&group).copied().unwrap_or(MIN_DB)
    }

    /// Gain reduction shown for a group (dB, 0 = none).
    pub fn reduction(&self, group: u32) -> f32 {
        self.reductions.get(&group).copied().unwrap_or(0.0)
    }
}

/// Values of the running game's copy of the mixer.
pub struct Live {
    values: HashMap<String, f32>,
    pub snapshot: usize,
}

impl Live {
    pub fn value(&self, key: &str, mixer: &AudioMixer) -> f32 {
        self.values
            .get(key)
            .copied()
            .or_else(|| mixer.default_value(key))
            .unwrap_or(0.0)
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if s.audio_mixer.mixer.is_none() {
        let first = s.assets.mixers.iter().next().cloned();
        s.audio_mixer.open(first);
    }
    let Some(path) = s.audio_mixer.mixer.clone() else {
        empty(ui, s, full);
        return;
    };
    let mixer = match s.anim_clips.mixers.get(&path) {
        Ok(m) => m.clone(),
        Err(e) => {
            centered(ui, full, &e);
            return;
        }
    };
    s.anim_clips.mixers.end_interaction(interacting(ui.ctx()));
    let live = sync_live(ui, s, &path, &mixer);
    let snap = edited_snapshot(s, &mixer, live.as_ref());

    let left = Rect::from_min_size(
        full.min,
        vec2(SIDEBAR_W.min(full.width() * 0.4), full.height()),
    );
    let right = Rect::from_min_max(pos2(left.right() + 1.0, full.top()), full.max);
    ui.painter().rect_filled(left, 0.0, theme::PANEL_BG);
    ui.painter().vline(
        left.right(),
        full.y_range(),
        egui::Stroke::new(1.0, theme::BORDER),
    );
    ui.scope_builder(UiBuilder::new().max_rect(left.shrink(4.0)), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("mixer_sidebar")
            .auto_shrink(false)
            .show(ui, |ui| sidebar(ui, s, &path, &mixer, snap, live.is_some()));
    });
    let bar = Rect::from_min_size(right.min, vec2(right.width(), 22.0));
    toolbar(ui, s, bar, &path, &mixer, snap, live.is_some());
    let area = Rect::from_min_max(pos2(right.left(), bar.bottom()), right.max);
    ui.scope_builder(UiBuilder::new().max_rect(area.shrink(6.0)), |ui| {
        egui::ScrollArea::horizontal()
            .id_salt("mixer_strips")
            .auto_shrink(false)
            .show(ui, |ui| strips(ui, s, &path, &mixer, snap, live.as_ref()));
    });
}

/// No mixer in the project yet.
fn empty(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.scope_builder(
        UiBuilder::new().max_rect(Rect::from_center_size(rect.center(), vec2(360.0, 60.0))),
        |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No Audio Mixer in this project.").color(theme::TEXT_DIM));
                if ui.button("Create Audio Mixer").clicked() {
                    s.push(Action::CreateAsset(AssetKind::AudioMixer));
                }
            });
        },
    );
}

/// In Play mode: send edits to the game, read its values and meters.
fn sync_live(ui: &Ui, s: &mut EditorState, path: &str, mixer: &AudioMixer) -> Option<Live> {
    let generation = s.anim_clips.mixers.generation;
    let rt = s.runtime.as_mut()?;
    let pushed = Some((path.to_owned(), generation));
    if s.audio_mixer.pushed != pushed {
        rt.with_audio(|a| a.replace_mixer(path, mixer.clone()));
        s.audio_mixer.pushed = pushed;
    }
    let (live, peaks, reductions) = rt.with_audio(|a| {
        let live = a.mixer_at(path).map(|m| Live {
            values: m.values(),
            snapshot: m.snapshot,
        });
        (live, a.mixer_peaks(path), a.mixer_reductions(path))
    });
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    for g in &mixer.groups {
        let now = amplitude_to_db(peaks.get(&g.id).copied().unwrap_or(0.0));
        let shown = s.audio_mixer.level(g.id) - METER_FALL * dt;
        s.audio_mixer
            .levels
            .insert(g.id, now.max(shown).max(MIN_DB));
        let gr = reductions.get(&g.id).copied().unwrap_or(0.0);
        let held = (s.audio_mixer.reduction(g.id) + METER_FALL * dt).min(0.0);
        s.audio_mixer.reductions.insert(g.id, gr.min(held));
    }
    ui.ctx().request_repaint();
    // `None` while no source uses the mixer: the window shows its own values.
    live
}

/// Snapshot the faders edit: the game's current one while playing.
fn edited_snapshot(s: &mut EditorState, mixer: &AudioMixer, live: Option<&Live>) -> usize {
    if let Some(l) = live {
        s.audio_mixer.snapshot = l.snapshot;
    }
    s.audio_mixer.snapshot = s.audio_mixer.snapshot.min(mixer.snapshots.len() - 1);
    s.audio_mixer.snapshot
}

/// Select a group (or one of its effects); the Inspector shows it.
pub(super) fn select(s: &mut EditorState, path: &str, item: MixerItem) {
    s.audio_mixer.selected = Some(item);
    s.selection.clear();
    s.inspector_asset = Some(path.to_owned());
}

fn toolbar(
    ui: &mut Ui,
    s: &EditorState,
    bar: Rect,
    path: &str,
    mixer: &AudioMixer,
    snap: usize,
    live: bool,
) {
    ui.painter().rect_filled(bar, 0.0, theme::TAB_BAR_BG);
    ui.scope_builder(
        UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 2.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(format!("{MIXER_ICON} {}", clip_name(path))).strong());
                ui.label(
                    RichText::new(format!("Snapshot: {}", mixer.snapshots[snap].name))
                        .color(theme::TEXT_DIM),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if live {
                        ui.label(RichText::new("● Edit in Play Mode").color(theme::PLAY_TINT));
                    } else if s.play.in_play_mode() {
                        ui.label(
                            RichText::new("Not used by the running scene").color(theme::TEXT_DIM),
                        );
                    }
                });
            });
        },
    );
}

fn strips(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    mixer: &AudioMixer,
    snap: usize,
    live: Option<&Live>,
) {
    let tree = mixer.tree();
    let height = ui.available_height().max(200.0);
    let max_fx = mixer
        .groups
        .iter()
        .map(|g| g.effects.len())
        .max()
        .unwrap_or(0);
    let fader_h = strip::fader_height(height, max_fx);
    let (rect, _) = ui.allocate_exact_size(
        vec2(tree.len() as f32 * (strip::STRIP_W + 6.0), height),
        Sense::hover(),
    );
    for (i, (g, _)) in tree.iter().enumerate() {
        let Some(group) = mixer.group(*g) else {
            continue;
        };
        let r = Rect::from_min_size(
            pos2(rect.left() + i as f32 * (strip::STRIP_W + 6.0), rect.top()),
            vec2(strip::STRIP_W, height),
        );
        strip::show(ui, s, path, mixer, group, snap, live, r, fader_h);
    }
}

// ---- sidebar --------------------------------------------------------------------------------

fn section(ui: &mut Ui, title: &str, add: Option<&str>) -> bool {
    let mut clicked = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        if let Some(tip) = add {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = ui.small_button("+").on_hover_text(tip).clicked();
            });
        }
    });
    ui.separator();
    clicked
}

fn sidebar(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    mixer: &AudioMixer,
    snap: usize,
    live: bool,
) {
    // Mixers.
    if section(ui, "Mixers", Some("Create Audio Mixer")) {
        s.push(Action::CreateAsset(AssetKind::AudioMixer));
    }
    let mixers: Vec<String> = s.assets.mixers.iter().cloned().collect();
    for m in mixers {
        let label = format!("{MIXER_ICON} {}", clip_name(&m));
        if ui.selectable_label(m == path, label).clicked() {
            s.audio_mixer.open(Some(m));
        }
    }

    // Snapshots: ★ marks the one Play starts with.
    if section(ui, "Snapshots", Some("Add a copy of the current snapshot")) {
        let mut added = 0;
        audio_mixers::edit(s, path, "Add Snapshot", false, |m| {
            added = m.add_snapshot(snap, "Snapshot")
        });
        s.audio_mixer.snapshot = added;
    }
    for (i, sn) in mixer.snapshots.iter().enumerate() {
        if rename_field(ui, s, path, Rename::Snapshot(i)) {
            continue;
        }
        let star = if i == mixer.start_snapshot {
            "★ "
        } else {
            "   "
        };
        let resp = ui.selectable_label(i == snap, format!("{star}{}", sn.name));
        if resp.clicked() {
            s.audio_mixer.snapshot = i;
            // While playing, picking a snapshot switches the game to it.
            if live && let Some(rt) = s.runtime.as_mut() {
                rt.with_audio(|a| {
                    if let Some(m) = a.load_mixer(path).and_then(|m| a.mixer_mut(m)) {
                        m.transition_to(i, 0.0);
                    }
                });
            }
        }
        resp.context_menu(|ui| {
            if ui.button("Set as start Snapshot").clicked() {
                audio_mixers::edit(s, path, "Set Start Snapshot", false, |m| {
                    m.start_snapshot = i
                });
                ui.close();
            }
            if ui.button("Rename").clicked() {
                s.audio_mixer.renaming = Some((Rename::Snapshot(i), sn.name.clone()));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                audio_mixers::edit(s, path, "Duplicate Snapshot", false, |m| {
                    let name = m.snapshots[i].name.clone();
                    m.add_snapshot(i, &name);
                });
                ui.close();
            }
            let can_delete = mixer.snapshots.len() > 1;
            if ui
                .add_enabled(can_delete, egui::Button::new("Delete"))
                .clicked()
            {
                audio_mixers::edit(s, path, "Delete Snapshot", false, |m| m.remove_snapshot(i));
                ui.close();
            }
        });
    }

    // Groups: + adds a child to the selected group (or Master).
    if section(
        ui,
        "Groups",
        Some("Add a child group to the selected group"),
    ) {
        let parent = s.audio_mixer.selected.map_or(MASTER, |i| i.group());
        let mut id = MASTER;
        audio_mixers::edit(s, path, "Add Group", false, |m| {
            id = m.add_group(parent, "New Group")
        });
        select(s, path, MixerItem::Group(id, None));
        let name = s
            .anim_clips
            .mixers
            .get(path)
            .ok()
            .and_then(|m| m.group(id).map(|g| g.name.clone()))
            .unwrap_or_default();
        s.audio_mixer.renaming = Some((Rename::Group(id), name));
    }
    for (g, depth) in mixer.tree() {
        let Some(group) = mixer.group(g) else {
            continue;
        };
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if rename_field(ui, s, path, Rename::Group(g)) {
                return;
            }
            let sel = s.audio_mixer.selected.is_some_and(|i| i.group() == g);
            let resp = ui.selectable_label(sel, &group.name);
            if resp.clicked() {
                select(s, path, MixerItem::Group(g, None));
            }
            if resp.double_clicked() && g != MASTER {
                s.audio_mixer.renaming = Some((Rename::Group(g), group.name.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button("Add child group").clicked() {
                    audio_mixers::edit(s, path, "Add Group", false, |m| {
                        m.add_group(g, "New Group");
                    });
                    ui.close();
                }
                if ui
                    .add_enabled(g != MASTER, egui::Button::new("Rename"))
                    .clicked()
                {
                    s.audio_mixer.renaming = Some((Rename::Group(g), group.name.clone()));
                    ui.close();
                }
                if ui
                    .add_enabled(g != MASTER, egui::Button::new("Remove"))
                    .clicked()
                {
                    audio_mixers::edit(s, path, "Remove Group", false, |m| m.remove_group(g));
                    s.audio_mixer.selected = None;
                    ui.close();
                }
            });
        });
    }

    // Exposed parameters.
    section(
        ui,
        &format!("Exposed Parameters ({})", mixer.exposed.len()),
        None,
    );
    if mixer.exposed.is_empty() {
        ui.weak("Right-click a value in the Properties panel to expose it to scripts.");
    }
    for p in &mixer.exposed {
        ui.horizontal(|ui| {
            let id = ui.id().with(("exposed", &p.name));
            let mut name: String = ui
                .data(|d| d.get_temp(id))
                .unwrap_or_else(|| p.name.clone());
            let resp = ui.add(egui::TextEdit::singleline(&mut name).desired_width(100.0));
            if resp.changed() {
                ui.data_mut(|d| d.insert_temp(id, name.clone()));
            }
            if resp.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
                if name != p.name {
                    let old = p.name.clone();
                    audio_mixers::edit(s, path, "Rename Exposed Parameter", false, |m| {
                        m.rename_exposed(&old, &name);
                    });
                }
            }
            ui.label(
                RichText::new(mixer.key_label(&p.key))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        })
        .response
        .context_menu(|ui| {
            if ui.button("Unexpose").clicked() {
                let key = p.key.clone();
                audio_mixers::edit(s, path, "Unexpose Parameter", false, |m| m.unexpose(&key));
                ui.close();
            }
        });
    }
}

/// Inline rename box for `what` when it is being renamed; true if it was drawn.
fn rename_field(ui: &mut Ui, s: &mut EditorState, path: &str, what: Rename) -> bool {
    let Some((target, mut text)) = s.audio_mixer.renaming.clone() else {
        return false;
    };
    if target != what {
        return false;
    }
    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
    if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
    }
    let done = resp.lost_focus();
    s.audio_mixer.renaming = Some((target, text.clone()));
    if done {
        s.audio_mixer.renaming = None;
        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            audio_mixers::edit(s, path, "Rename", false, |m| match what {
                Rename::Group(g) => {
                    m.rename_group(g, &text);
                }
                Rename::Snapshot(i) => {
                    let name = text.trim();
                    if !name.is_empty()
                        && m.snapshot_index(name).is_none_or(|j| j == i)
                        && let Some(sn) = m.snapshots.get_mut(i)
                    {
                        sn.name = name.to_owned();
                    }
                }
            });
        }
    }
    true
}

fn centered(ui: &mut Ui, rect: Rect, text: &str) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
}
