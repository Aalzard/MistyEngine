//! `OnEnable`, `OnDisable` and `OnDestroy` for Lua scripts, as in the familiar engine.
//!
//! A script is *enabled* while its object is active in the hierarchy and its component is
//! enabled. `Awake` runs the first time it can run, `OnEnable` right after it and every time it
//! can run again, `OnDisable` when it stops being able to (object or a parent switched off,
//! component disabled, object destroyed while enabled, Stop or the scene unloading), then
//! `OnDestroy` when it goes (only if it woke). The two always alternate.
//!
//! The host looks for scripts to switch only when `Scene::activity` moved (after each callback,
//! before Start, FixedUpdate, Update and LateUpdate), and only under the objects that changed
//! (`Scene::activity_changes`): nothing changes, nothing is looked at. `SetActive(false)` then
//! `SetActive(true)` in one callback is still a switch (`dipped`): OnDisable, then OnEnable.
//! Callbacks run after the one that switched the object, like `Awake` after `Instantiate`.

use hecs::Entity;

use super::{Instance, LogKind, ScriptHost};

/// Rounds of OnEnable / OnDisable that switch more objects before the host gives up (an
/// OnEnable switching its object off and an OnDisable switching it on again never end).
const MAX_PASSES: usize = 32;

impl ScriptHost {
    /// Whether the scripts of `i`'s object hear collisions and triggers: object alive and
    /// active; a disabled script still does (not its Update).
    pub(super) fn hears_contacts(&self, i: &Instance) -> bool {
        self.with_world(|w| w.scene.contains(i.entity) && w.scene.active_in_hierarchy(i.entity))
    }

    /// Bring instance `i` in line with its object: OnDisable when it can't run any more (or
    /// was switched off and on after its `stamp`: `dipped_at`), then Awake (the first time)
    /// and OnEnable when it can. `flush` runs what each callback queued right after it.
    pub(super) fn reconcile(&mut self, i: usize, dipped_at: Option<u64>, flush: bool) {
        let inst = &self.instances[i];
        let dipped = dipped_at.is_some_and(|at| at > inst.stamp);
        let enabled = inst.enabled;
        let run = self.runnable(inst);
        if enabled && (!run || dipped) {
            self.set_enabled(i, false);
            self.call(i, "OnDisable", None);
            if flush {
                self.flush_spawned();
            }
        } else if !run || enabled {
            return;
        }
        if self.instances[i].enabled || !self.runnable(&self.instances[i]) {
            return;
        }
        if !self.instances[i].awoken {
            self.instances[i].awoken = true;
            self.call(i, "Awake", None);
            if flush {
                self.flush_spawned();
            }
            // Awake may have switched its own object off: no OnEnable then.
            if self.instances[i].enabled || !self.runnable(&self.instances[i]) {
                return;
            }
        }
        self.set_enabled(i, true);
        self.call(i, "OnEnable", None);
        if flush {
            self.flush_spawned();
        }
    }

    fn set_enabled(&mut self, i: usize, on: bool) {
        let now = self.with_world(|w| w.scene.activity());
        let inst = &mut self.instances[i];
        inst.enabled = on;
        inst.stamp = now;
    }

    /// OnEnable / OnDisable (and Awake) for scripts whose objects were switched on or off, or
    /// whose component was enabled or disabled, since the last look. Free when nothing changed.
    pub(super) fn sync_enabled(&mut self) {
        if self.syncing {
            return;
        }
        for _ in 0..MAX_PASSES {
            let seen = self.seen_activity;
            let (now, changes) = self.with_world(|w| {
                let now = w.scene.activity();
                (now, (now != seen).then(|| w.scene.activity_changes(seen)))
            });
            // `Some(None)`: too many changes to know which; every script is looked at.
            let Some(changes) = changes else {
                return;
            };
            self.seen_activity = now;
            self.syncing = true;
            for i in 0..self.instances.len() {
                let e: Entity = self.instances[i].entity;
                let dipped_at = match &changes {
                    Some(c) if !c.touched.contains(&e) => continue,
                    Some(c) => c.dipped.get(&e).copied(),
                    None => None,
                };
                // While Play starts, scripts wake in order (the start pass reaches them).
                if self.starting && !self.instances[i].awoken {
                    continue;
                }
                self.reconcile(i, dipped_at, true);
            }
            self.syncing = false;
        }
        // Give up on the rest instead of hanging; later changes are looked at as usual.
        self.seen_activity = self.with_world(|w| w.scene.activity());
        self.log(
            LogKind::Error,
            "OnEnable / OnDisable kept switching objects on and off; stopped".into(),
        );
    }

    /// The scripts of `subtree` are about to be destroyed: OnDisable for the enabled ones,
    /// then OnDestroy for the ones that woke. What these callbacks spawn wakes later.
    pub(super) fn destroying(&mut self, subtree: &[Entity]) {
        if subtree.is_empty() {
            return;
        }
        let set: std::collections::HashSet<Entity> = subtree.iter().copied().collect();
        let mine: Vec<usize> = (0..self.instances.len())
            .filter(|i| set.contains(&self.instances[*i].entity))
            .collect();
        self.goodbye(&mine);
    }

    /// OnDisable for the enabled of `which`, then OnDestroy for the ones that woke.
    fn goodbye(&mut self, which: &[usize]) {
        let was = std::mem::replace(&mut self.syncing, true);
        for &i in which {
            if self.instances[i].enabled {
                self.set_enabled(i, false);
                self.call(i, "OnDisable", None);
            }
        }
        for &i in which {
            if std::mem::take(&mut self.instances[i].awoken) {
                self.call(i, "OnDestroy", None);
            }
        }
        self.syncing = was;
    }

    /// The game stops (Stop, a scene load replacing this one): OnDisable for every enabled
    /// script, then OnDestroy for every script that woke, while the objects still exist.
    /// Call it before dropping the host; nothing they spawn or queue happens.
    pub fn shutdown(&mut self, scene: &mut crate::scene::Scene) {
        let root = self.root.clone();
        let _root = crate::assets::scoped_root(&root);
        super::arm_deadline(&self.lua, true);
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        let all: Vec<usize> = (0..self.instances.len()).collect();
        self.goodbye(&all);
        self.graphs.shutdown(&self.lua);
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
        super::arm_deadline(&self.lua, false);
    }
}

impl ScriptHost {
    /// Carry out the Stop Actions of particle systems that stopped for good this frame
    /// ([`crate::particles::update`]): switch the object off, destroy it, or call
    /// `OnParticleSystemStopped()` on its scripts. Between frames.
    pub fn particle_stop_actions(
        &mut self,
        scene: &mut crate::scene::Scene,
        stopped: &[(Entity, crate::particles::StopAction)],
    ) {
        use crate::particles::StopAction;
        if stopped.is_empty() {
            return;
        }
        let root = self.root.clone();
        let _root = crate::assets::scoped_root(&root);
        super::arm_deadline(&self.lua, true);
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        for &(e, action) in stopped {
            if !self.with_world(|w| w.scene.contains(e)) {
                continue;
            }
            match action {
                StopAction::None => {}
                StopAction::Disable => self.with_world(|w| w.scene.set_active(e, false)),
                StopAction::Destroy => self.with_world(|w| w.destroy.push(e)),
                StopAction::Callback => {
                    for i in 0..self.instances.len() {
                        if self.instances[i].entity == e && self.runnable(&self.instances[i]) {
                            self.call(i, "OnParticleSystemStopped", None);
                            self.flush_spawned();
                        }
                    }
                }
            }
        }
        // OnDisable for the switched-off ones, then the destroyed go.
        self.flush_spawned();
        self.process_destroy();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
        super::arm_deadline(&self.lua, false);
    }
}
