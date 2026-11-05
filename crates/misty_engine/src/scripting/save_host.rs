//! The script host's side of save games and localization: restoring a loaded save
//! (`OnLoad`), and locale-change events with Localized Text refreshes.

use hecs::Entity;
use mlua::{Function, Value};

use super::{LogKind, ScriptHost, localization_api, save_api};
use crate::audio::Audio;
use crate::save::SaveData;
use crate::scene::Scene;

impl ScriptHost {
    /// The save `SaveSystem.Load` queued this frame, if any.
    pub fn take_pending_load(&self) -> Option<SaveData> {
        self.with_world(|w| w.save.pending_load.take())
    }

    /// The game's audio, leaving a silent one behind (a reloaded scene keeps the device).
    pub fn take_audio(&self) -> Audio {
        self.with_world(|w| {
            let mut audio = std::mem::replace(&mut w.audio, Audio::null(w.root.clone()).0);
            audio.stop_all();
            audio
        })
    }

    /// Where `SaveSystem` keeps this game's saves.
    pub fn save_folder(&self) -> std::path::PathBuf {
        self.with_world(|w| w.save.folder(&w.root))
    }

    /// Use another saves folder for this game (tests, tools).
    pub fn set_save_folder(&self, folder: std::path::PathBuf) {
        self.with_world(|w| w.save.folder = Some(folder));
    }

    /// The game's String Tables and locale.
    pub fn with_localization<R>(&self, f: impl FnOnce(&crate::localization::Localizer) -> R) -> R {
        self.with_world(|w| f(&w.localization))
    }

    /// Apply a loaded save to this game's scene (after `Awake`, before `Start`): destroyed
    /// objects go, spawned prefabs come back (with `Awake`), states are set, bodies moved and
    /// `OnLoad(data)` called. `in_place` = the scene wasn't reloaded, so this session's spawned
    /// objects are removed first.
    pub fn apply_loaded(&mut self, scene: &mut Scene, data: &SaveData, in_place: bool) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        if in_place {
            self.with_world(|w| {
                let roots = std::mem::take(&mut w.save.spawned);
                w.destroy.extend(roots.into_iter().map(|(e, _)| e));
            });
            self.process_destroy();
        }
        let scripts = self.with_world(|w| {
            let applied = crate::save::apply(&mut w.scene, data, &w.root);
            w.destroy.extend(applied.destroy.iter().copied());
            for (root, path) in &applied.spawned {
                w.physics.add_subtree(&w.scene, *root);
                w.physics2d.add_subtree(&w.scene, *root);
                w.spawned.push(*root);
                w.save.spawned.push((*root, path.clone()));
            }
            // Bodies follow the restored transforms (children move with their parents).
            let moved: Vec<Entity> = w
                .scene
                .walk()
                .into_iter()
                .map(|(e, _)| e)
                .filter(|e| applied.moved.iter().any(|m| w.scene.is_ancestor(*m, *e)))
                .collect();
            for e in moved {
                if w.physics.has_body(e) {
                    let (_, rot, pos) = w.scene.world_matrix(e).to_scale_rotation_translation();
                    w.physics.teleport(e, pos, rot);
                }
            }
            for (e, v, av) in &applied.velocities {
                if w.physics.has_body(*e) {
                    w.physics.set_velocity(*e, *v);
                    w.physics.set_angular_velocity(*e, *av);
                }
            }
            w.save.resume_play_time(data.info.play_time);
            w.logs.extend(
                applied
                    .warnings
                    .into_iter()
                    .map(|m| (LogKind::Warning, format!("SaveSystem.Load: {m}"))),
            );
            applied.scripts
        });
        self.process_destroy();
        self.flush_spawned();
        let tracked = save_api::tracked(&self.lua);
        for (e, key, value) in scripts {
            for (te, tkey, t) in &tracked {
                if *te != e || *tkey != key {
                    continue;
                }
                let Ok(Some(f)) = t.get::<Option<Function>>("OnLoad") else {
                    continue;
                };
                let result = save_api::to_lua(&self.lua, &value, 0)
                    .and_then(|arg| f.call::<()>((t.clone(), arg)));
                if let Err(err) = result {
                    self.log(LogKind::Error, format!("{key} (OnLoad): {err}"));
                }
                self.flush_spawned();
            }
        }
        self.localization_frame();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Locale changes reach `Localization.OnLocaleChanged` listeners and scripts'
    /// `OnLocaleChanged(code)`; then every Localized Text shows its current string.
    pub(super) fn localization_frame(&mut self) {
        let changed = self.with_world(|w| w.localization.take_changed());
        if changed {
            let code = self.with_world(|w| w.localization.locale());
            for f in localization_api::listeners(&self.lua) {
                if let Err(e) = f.call::<()>(code.as_str()) {
                    self.log(LogKind::Error, format!("OnLocaleChanged listener: {e}"));
                }
                self.flush_spawned();
            }
            for i in 0..self.instances.len() {
                if self.runnable(&self.instances[i]) {
                    let arg = self.lua.create_string(&code).ok().map(Value::String);
                    self.call(i, "OnLocaleChanged", arg);
                    self.flush_spawned();
                }
            }
        }
        self.with_world(|w| {
            crate::localization::refresh_texts(&mut w.scene, &w.localization);
        });
    }
}

impl ScriptHost {
    /// Put messages back in front of this host's log (carried over a scene reload).
    pub fn prepend_logs(&self, logs: Vec<(LogKind, String)>) {
        self.with_world(|w| {
            w.logs.splice(0..0, logs);
        });
    }
}

impl ScriptHost {
    /// `SaveSystem.Save(slot)` from Rust between frames (editor tools, tests).
    pub fn save_game(&mut self, scene: &mut Scene, slot: &str) -> Result<(), String> {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        let result = save_api::save(&self.lua, slot);
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
        result
    }

    /// `SaveSystem.Load(slot)` from Rust: restored by the runtime after its next frame.
    pub fn load_game(&mut self, slot: &str) -> Result<(), String> {
        save_api::load(&self.lua, slot)
    }
}
