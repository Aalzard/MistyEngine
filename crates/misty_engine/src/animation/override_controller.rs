//! Animator Override Controller (`.overrideController`, JSON): another controller's states,
//! transitions and parameters with some of its clips swapped, so characters that move alike
//! share one state machine.
//!
//! ```json
//! { "controller": "Assets/Animations/Hero.controller",
//!   "overrides": { "Assets/Animations/Run.anim": "Assets/Animations/OrcRun.anim" } }
//! ```
//!
//! An Animator takes either kind of file as its controller; the override is applied when the
//! controller is loaded (every state motion, blend tree child and synced-layer motion naming
//! an overridden clip plays the replacement).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::AnimatorController;

/// File extension of override controllers.
pub const OVERRIDE_EXTENSION: &str = "overrideController";

pub fn is_override_controller(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(OVERRIDE_EXTENSION))
}

/// A `.controller` or `.overrideController`: what an Animator can play.
pub fn is_runtime_controller(path: &str) -> bool {
    super::is_controller(path) || is_override_controller(path)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverrideController {
    /// The controller whose machine is reused (a `.controller` or another override).
    pub controller: Option<String>,
    /// Original clip path → replacement clip path.
    pub overrides: BTreeMap<String, String>,
}

impl OverrideController {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(&crate::asset_db::resolve_json(text)).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::asset_db::write_asset(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// `base` with this override's clips swapped in.
    pub fn apply(&self, base: &AnimatorController) -> AnimatorController {
        let mut c = base.clone();
        c.remap_motions(|clip| self.overrides.get(clip).cloned());
        c
    }
}

impl AnimatorController {
    /// Replace every clip path `f` maps (state motions, blend tree children, synced-layer
    /// motions) in all layers.
    pub fn remap_motions(&mut self, f: impl Fn(&str) -> Option<String>) {
        let swap = |m: &mut Option<String>| {
            if let Some(new) = m.as_deref().and_then(&f) {
                *m = Some(new);
            }
        };
        let machines =
            std::iter::once(&mut self.base).chain(self.layers.iter_mut().map(|l| &mut l.machine));
        for machine in machines {
            for s in &mut machine.states {
                swap(&mut s.motion);
                if let Some(tree) = &mut s.blend_tree {
                    for child in &mut tree.children {
                        swap(&mut child.motion);
                    }
                }
            }
        }
        for layer in &mut self.layers {
            for clip in layer.sync_motions.values_mut() {
                if let Some(new) = f(clip) {
                    *clip = new;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::controller::{BlendChild, BlendTree, Layer, State};

    #[test]
    fn overrides_swap_motions_in_states_trees_and_synced_layers() {
        let mut base = AnimatorController::default();
        base.base.states.push(State {
            name: "Run".into(),
            motion: Some("Assets/Run.anim".into()),
            ..Default::default()
        });
        base.base.states.push(State {
            name: "Move".into(),
            blend_tree: Some(BlendTree {
                children: vec![
                    BlendChild::clip("Assets/Walk.anim"),
                    BlendChild::clip("Assets/Run.anim"),
                ],
                ..Default::default()
            }),
            ..Default::default()
        });
        let mut synced = Layer {
            sync_source: Some(0),
            ..Default::default()
        };
        synced
            .sync_motions
            .insert("Run".into(), "Assets/Run.anim".into());
        base.layers.push(synced);
        let o = OverrideController::from_json(
            r#"{ "controller": "Assets/Hero.controller",
                 "overrides": { "Assets/Run.anim": "Assets/OrcRun.anim" } }"#,
        )
        .unwrap();
        let c = o.apply(&base);
        assert_eq!(
            c.base.states[0].motion.as_deref(),
            Some("Assets/OrcRun.anim")
        );
        let tree = c.base.states[1].blend_tree.as_ref().unwrap();
        assert_eq!(tree.children[0].motion.as_deref(), Some("Assets/Walk.anim"));
        assert_eq!(
            tree.children[1].motion.as_deref(),
            Some("Assets/OrcRun.anim")
        );
        assert_eq!(c.layers[0].sync_motions["Run"], "Assets/OrcRun.anim");
        assert!(is_runtime_controller("Assets/Orc.overrideController"));
        assert!(!crate::animation::is_controller(
            "Assets/Orc.overrideController"
        ));
        assert_eq!(OverrideController::from_json(&o.to_json()).unwrap(), o);
    }

    #[test]
    fn animators_play_the_override_clips_and_loops_fail_cleanly() {
        use crate::animation::AnimatorPlayer;
        use crate::animation::clip::{AnimationClip, Property};
        use crate::animation::controller::StateMachine;
        use crate::scene::{Animator, Scene};
        let dir = std::env::temp_dir().join(format!(
            "misty_override_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let base = AnimatorController {
            base: StateMachine {
                states: vec![State {
                    name: "Idle".into(),
                    motion: Some("idle.anim".into()),
                    ..Default::default()
                }],
                default_state: Some("Idle".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        base.save(&dir.join("Assets/Hero.controller")).unwrap();
        let mut o = OverrideController {
            controller: Some("Assets/Hero.controller".into()),
            ..Default::default()
        };
        o.overrides.insert("idle.anim".into(), "orc.anim".into());
        o.save(&dir.join("Assets/Orc.overrideController")).unwrap();
        OverrideController {
            controller: Some("Assets/Loop.overrideController".into()),
            ..Default::default()
        }
        .save(&dir.join("Assets/Loop.overrideController"))
        .unwrap();

        let held = |y: f32| {
            let mut c = AnimationClip {
                length: 1.0,
                ..Default::default()
            };
            c.set_key("", Property::Position(1), 0.0, y);
            c.set_key("", Property::Position(1), 1.0, y);
            c
        };
        let mut scene = Scene::new("t");
        let mut spawn = |name: &str, ctrl: &str| {
            let e = scene.spawn(name, None);
            scene.add(
                e,
                Animator {
                    controller: Some(ctrl.into()),
                    ..Default::default()
                },
            );
            e
        };
        let hero = spawn("Hero", "Assets/Hero.controller");
        let orc = spawn("Orc", "Assets/Orc.overrideController");
        let looped = spawn("Loop", "Assets/Loop.overrideController");
        let mut p = AnimatorPlayer::new(dir.clone());
        p.insert_clip("idle.anim", held(1.0));
        p.insert_clip("orc.anim", held(3.0));
        p.update(&mut scene, 0.1);
        assert_eq!(scene.transform(hero).position.y, 1.0);
        assert_eq!(scene.transform(orc).position.y, 3.0);
        assert_eq!(scene.transform(looped).position.y, 0.0);
        assert_eq!(p.status(&scene, orc).unwrap().current.unwrap().name, "Idle");
        let logs = p.take_logs();
        assert!(
            logs.iter().any(|l| l.contains("Loop.overrideController")),
            "{logs:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
