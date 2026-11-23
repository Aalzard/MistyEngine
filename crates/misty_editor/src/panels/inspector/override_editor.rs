//! Inspector of an Animator Override Controller (`.overrideController`): the base Controller
//! slot, then every clip the base plays with an Override slot next to it (pick a project clip or
//! drop one from the Project window; × keeps the original), "Clear All", and overrides left
//! from another base. Every change is saved at once, with undo.

use egui::{RichText, Ui};
use misty_engine::animation::{
    AnimatorController, OverrideController, is_clip, is_runtime_controller,
};
use misty_engine::scene::clip_name;

use super::anim::CLIP_ICON;
use crate::anim_controllers::interacting;
use crate::override_controllers::{self as oc, edit};
use crate::panels::AssetPayload;
use crate::panels::animator::CONTROLLER_ICON;
use crate::state::EditorState;
use crate::theme;

/// Labels tests click.
pub const CLEAR_ALL_LABEL: &str = "Clear All Overrides";
pub const REMOVE_UNUSED_LABEL: &str = "Remove Unused";

pub fn override_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let o = match state.anim_clips.overrides.get(rel) {
        Ok(o) => o.clone(),
        Err(e) => {
            ui.colored_label(theme::shade(theme::AXIS_X), e);
            return;
        }
    };
    let busy = interacting(ui.ctx());
    state.anim_clips.overrides.end_interaction(busy);
    ui.horizontal(|ui| {
        ui.label(RichText::new(CONTROLLER_ICON).size(22.0));
        ui.label(theme::bold(
            ui.ctx(),
            format!("{} (Animator Override Controller)", clip_name(rel)),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    // The base controller.
    let choices: Vec<String> = state
        .assets
        .controllers
        .iter()
        .chain(&state.assets.override_controllers)
        .filter(|c| *c != rel)
        .cloned()
        .collect();
    let mut base = o.controller.clone();
    ui.horizontal(|ui| {
        ui.label("Controller");
        if controller_slot(ui, rel, &choices, &mut base) {
            edit(state, rel, "Override Base Controller", false, |o| {
                o.controller = base.clone();
            });
        }
    });
    let Some(base_path) = o.controller.clone() else {
        ui.weak("Pick the Animator Controller whose states and transitions this one reuses.");
        return;
    };
    let base: Result<AnimatorController, String> =
        oc::load_base(state, &base_path, &mut vec![rel.to_owned()]);
    let originals = match base {
        Ok(b) => oc::original_clips(&b),
        Err(e) => {
            ui.colored_label(theme::shade(theme::AXIS_X), e);
            Vec::new()
        }
    };
    ui.add_space(4.0);
    clip_table(ui, state, rel, &o, &originals);
    ui.add_space(4.0);
    let any = originals.iter().any(|c| oc::override_of(&o, c).is_some());
    if ui
        .add_enabled(any, egui::Button::new(CLEAR_ALL_LABEL))
        .on_hover_text("Every state plays its original clip again")
        .clicked()
    {
        edit(state, rel, "Clear Overrides", false, |o| {
            for c in &originals {
                oc::set_override(o, c, None);
            }
        });
    }
    let unused = oc::unused(&o, &originals);
    if !unused.is_empty() {
        ui.add_space(6.0);
        super::card::sub_header(ui, "Unused overrides");
        ui.weak("The base controller doesn't play these clips (left from another base).");
        for k in &unused {
            ui.label(format!(
                "{CLIP_ICON} {} → {}",
                clip_name(k),
                o.overrides
                    .get(k)
                    .map_or_else(String::new, |v| clip_name(v))
            ));
        }
        if ui.small_button(REMOVE_UNUSED_LABEL).clicked() {
            edit(state, rel, "Remove Unused Overrides", false, |o| {
                for k in &unused {
                    o.overrides.remove(k);
                }
            });
        }
    }
}

/// Base controller picker: project controllers and other overrides, or one dropped on it.
fn controller_slot(ui: &mut Ui, rel: &str, choices: &[String], value: &mut Option<String>) -> bool {
    let mut ch = false;
    let none = "None (Animator Controller)";
    let r = egui::ComboBox::from_id_salt(("override_base", rel))
        .selected_text(match value {
            Some(p) => format!("{CONTROLLER_ICON} {}", clip_name(p)),
            None => none.to_owned(),
        })
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, none).changed();
            for c in choices {
                ch |= ui
                    .selectable_value(value, Some(c.clone()), clip_name(c))
                    .on_hover_text(c)
                    .changed();
            }
        })
        .response;
    if let Some(p) = r.dnd_release_payload::<AssetPayload>()
        && is_runtime_controller(&p.0)
        && p.0 != rel
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// "Original | Override" rows.
fn clip_table(
    ui: &mut Ui,
    state: &mut EditorState,
    rel: &str,
    o: &OverrideController,
    originals: &[String],
) {
    if originals.is_empty() {
        ui.weak("The base controller plays no clips.");
        return;
    }
    let clips: Vec<String> = state.assets.anims.iter().cloned().collect();
    let overridden = originals
        .iter()
        .filter(|c| oc::override_of(o, c).is_some())
        .count();
    ui.weak(format!(
        "{overridden} of {} clip{} overridden",
        originals.len(),
        if originals.len() == 1 { "" } else { "s" }
    ));
    let mut change: Option<(String, Option<String>)> = None;
    let mut select: Option<String> = None;
    egui::Grid::new(("override_clips", rel))
        .num_columns(3)
        .striped(true)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Original").strong());
            ui.label(RichText::new("Override").strong());
            ui.label("");
            ui.end_row();
            for original in originals {
                if ui
                    .link(format!("{CLIP_ICON} {}", clip_name(original)))
                    .on_hover_text(original)
                    .clicked()
                {
                    select = Some(original.clone());
                }
                let current = oc::override_of(o, original).map(str::to_owned);
                let mut value = current.clone();
                let none = "None (original)";
                let r = egui::ComboBox::from_id_salt(("override_clip", rel, original))
                    .width(160.0)
                    .selected_text(match &value {
                        Some(v) => format!("{CLIP_ICON} {}", clip_name(v)),
                        None => none.to_owned(),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut value, None, none);
                        for c in clips.iter().filter(|c| *c != original) {
                            ui.selectable_value(&mut value, Some(c.clone()), clip_name(c))
                                .on_hover_text(c);
                        }
                    })
                    .response;
                if let Some(p) = r.dnd_release_payload::<AssetPayload>()
                    && is_clip(&p.0)
                {
                    value = Some(p.0.clone());
                }
                let clear = ui
                    .add_enabled(current.is_some(), egui::Button::new("×").small())
                    .on_hover_text("Play the original clip")
                    .clicked();
                if clear {
                    value = None;
                }
                if value != current {
                    change = Some((original.clone(), value));
                }
                ui.end_row();
            }
        });
    if let Some((original, value)) = change {
        let label = if value.is_some() {
            "Override Clip"
        } else {
            "Clear Override"
        };
        edit(state, rel, label, false, |o| {
            oc::set_override(o, &original, value)
        });
    }
    if let Some(clip) = select {
        crate::prefabs::select_asset(state, &clip);
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::animation::controller::{AnimatorController, State};
    use misty_engine::animation::{AnimationClip, OverrideController};

    use crate::actions::Action;
    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    const PATH: &str = "Assets/Orc.overrideController";

    /// A project with Idle / Run / OrcRun clips, `Hero.controller` (Idle, Run) and
    /// `Orc.overrideController` on it with no overrides yet.
    fn setup() -> (std::path::PathBuf, EditorState) {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_override_insp_{n}"));
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        for c in ["Idle", "Run", "OrcRun"] {
            AnimationClip::default()
                .save(&root.join(format!("Assets/{c}.anim")))
                .unwrap();
        }
        let mut c = AnimatorController::default();
        for (n, clip) in [("Idle", "Assets/Idle.anim"), ("Run", "Assets/Run.anim")] {
            c.base.states.push(State {
                name: n.into(),
                motion: Some(clip.into()),
                ..Default::default()
            });
        }
        c.save(&root.join("Assets/Hero.controller")).unwrap();
        OverrideController {
            controller: Some("Assets/Hero.controller".into()),
            ..Default::default()
        }
        .save(&root.join("Assets/Orc.overrideController"))
        .unwrap();
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        s.assets.refresh();
        (root, s)
    }

    fn saved(root: &std::path::Path) -> OverrideController {
        OverrideController::load(&root.join("Assets/Orc.overrideController")).unwrap()
    }

    #[test]
    fn override_inspector_lists_clips_sets_clears_and_undoes() {
        let (root, s) = setup();
        let mut h = Harness::builder().with_size([520.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| super::override_asset(ui, s, PATH),
            s,
        );
        h.run();
        h.get_by_label("Orc (Animator Override Controller)");
        h.get_by_label("0 of 2 clips overridden");
        // Run's slot set to OrcRun (as picking it in the slot's list does).
        crate::override_controllers::edit(h.state_mut(), PATH, "Override Clip", false, |o| {
            crate::override_controllers::set_override(
                o,
                "Assets/Run.anim",
                Some("Assets/OrcRun.anim".into()),
            )
        });
        h.run();
        assert_eq!(
            saved(&root)
                .overrides
                .get("Assets/Run.anim")
                .map(String::as_str),
            Some("Assets/OrcRun.anim")
        );
        h.get_by_label("1 of 2 clips overridden");
        h.get_by_value(&format!("{} OrcRun", super::CLIP_ICON));
        // Undo puts it back, redo again.
        crate::commands::apply(h.state_mut(), Action::Undo);
        h.run();
        assert!(!saved(&root).overrides.contains_key("Assets/Run.anim"));
        h.get_by_label("0 of 2 clips overridden");
        crate::commands::apply(h.state_mut(), Action::Redo);
        h.run();
        assert!(saved(&root).overrides.contains_key("Assets/Run.anim"));
        // × on Run's row keeps the original again (one undo step).
        h.get_all_by_label("×").nth(1).unwrap().click();
        h.run();
        assert!(saved(&root).overrides.is_empty());
        crate::commands::apply(h.state_mut(), Action::Undo);
        h.run();
        assert!(saved(&root).overrides.contains_key("Assets/Run.anim"));
        // Clear All.
        h.get_by_label(super::CLEAR_ALL_LABEL).click();
        h.run();
        assert!(saved(&root).overrides.is_empty());
        h.get_by_label("0 of 2 clips overridden");
        // Picking a clip in Idle's Override slot.
        h.get_all_by_value("None (original)")
            .next()
            .unwrap()
            .click();
        h.run();
        h.get_by_label("OrcRun").click();
        h.run();
        assert_eq!(
            saved(&root)
                .overrides
                .get("Assets/Idle.anim")
                .map(String::as_str),
            Some("Assets/OrcRun.anim")
        );
    }

    #[test]
    fn override_inspector_shows_unused_overrides_and_removes_them() {
        let (root, s) = setup();
        let mut o = saved(&root);
        o.overrides
            .insert("Assets/Gone.anim".into(), "Assets/OrcRun.anim".into());
        o.save(&root.join("Assets/Orc.overrideController")).unwrap();
        let mut h = Harness::builder().with_size([520.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| super::override_asset(ui, s, PATH),
            s,
        );
        h.run();
        h.get_by_label("Unused overrides");
        h.get_by_label(super::REMOVE_UNUSED_LABEL).click();
        h.run();
        assert!(saved(&root).overrides.is_empty());
    }

    /// An override edited while playing reaches the Animator that plays it (and so does undo).
    #[test]
    fn override_edits_during_play_reach_the_running_animator() {
        use misty_engine::animation::clip::Property;
        let (root, mut s) = setup();
        // Idle holds Body at y = 1, OrcRun at y = 5.
        for (clip, y) in [("Idle", 1.0), ("OrcRun", 5.0)] {
            let mut c = AnimationClip {
                length: 1.0,
                ..Default::default()
            };
            c.set_key("Body", Property::Position(1), 0.0, y);
            c.set_key("Body", Property::Position(1), 1.0, y);
            c.save(&root.join(format!("Assets/{clip}.anim"))).unwrap();
        }
        let hero = s.scene.spawn("Hero", None);
        let body = s.scene.spawn("Body", Some(hero));
        s.scene.add(
            hero,
            misty_engine::scene::Animator {
                controller: Some(PATH.into()),
                ..Default::default()
            },
        );
        crate::play::start(&mut s);
        crate::play::tick(&mut s, 1.0 / 60.0);
        let y = |s: &EditorState| s.scene.transform(body).position.y;
        assert_eq!(y(&s), 1.0, "the base controller's Idle");
        assert!(crate::override_controllers::edit(
            &mut s,
            PATH,
            "Override Clip",
            false,
            |o| crate::override_controllers::set_override(
                o,
                "Assets/Idle.anim",
                Some("Assets/OrcRun.anim".into())
            )
        ));
        crate::play::tick(&mut s, 1.0 / 60.0);
        assert_eq!(y(&s), 5.0, "the override plays OrcRun for Idle");
        crate::commands::apply(&mut s, Action::Undo);
        crate::play::tick(&mut s, 1.0 / 60.0);
        assert_eq!(y(&s), 1.0, "undo reaches the game too");
        crate::play::stop(&mut s);
        std::fs::remove_dir_all(root).ok();
    }
}
