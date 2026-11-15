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

/// OnEnable / OnDisable one script may get in one look before the host gives up (an OnEnable
/// switching its object off and an OnDisable switching it on again never end). Counted per
/// script, not per round: a chain of OnEnables switching on the next object down a deep
/// hierarchy takes one round per level but switches each script once.
const MAX_SWITCHES: usize = 32;

impl ScriptHost {
    /// Whether the scripts of `i`'s object hear collisions and triggers: object alive and
    /// active; a disabled script still does (not its Update).
    pub(super) fn hears_contacts(&self, i: &Instance) -> bool {
        self.with_world(|w| w.scene.contains(i.entity) && w.scene.active_in_hierarchy(i.entity))
    }

    /// Bring instance `i` in line with its object: OnDisable when it can't run any more (or
    /// was switched off and on after its `stamp`: `dipped_at`), Awake the first time its
    /// object is active (even with the component disabled: such a script hears collisions, so
    /// it must have woken), then OnEnable when it can run. `flush` runs what each callback
    /// queued right after it. Returns how many of OnDisable / OnEnable it called.
    pub(super) fn reconcile(&mut self, i: usize, dipped_at: Option<u64>, flush: bool) -> usize {
        let mut switched = 0;
        let inst = &self.instances[i];
        let dipped = dipped_at.is_some_and(|at| at > inst.stamp);
        if inst.enabled && (dipped || !self.runnable(inst)) {
            self.set_enabled(i, false);
            switched += 1;
            self.call(i, "OnDisable", None);
            if flush {
                self.flush_spawned();
            }
        }
        if !self.instances[i].awoken && self.hears_contacts(&self.instances[i]) {
            self.instances[i].awoken = true;
            self.call(i, "Awake", None);
            if flush {
                self.flush_spawned();
            }
        }
        // Awake may have switched its own object off: no OnEnable then.
        let inst = &self.instances[i];
        if inst.enabled || !inst.awoken || !self.runnable(inst) {
            return switched;
        }
        self.set_enabled(i, true);
        switched += 1;
        self.call(i, "OnEnable", None);
        if flush {
            self.flush_spawned();
        }
        switched
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
        // OnEnable / OnDisable per script in this look (indices only grow meanwhile:
        // instantiated scripts are added at the end, destroyed ones go between frames).
        let mut flips: Vec<usize> = Vec::new();
        let mut rounds = 0;
        'look: loop {
            let seen = self.seen_activity;
            let (now, changes) = self.with_world(|w| {
                let now = w.scene.activity();
                (now, (now != seen).then(|| w.scene.activity_changes(seen)))
            });
            // `Some(None)`: too many changes to know which; every script is looked at.
            let Some(changes) = changes else {
                return;
            };
            // A chain is at most one round per script; more is a loop the flips missed.
            rounds += 1;
            if rounds > self.instances.len() + MAX_SWITCHES {
                break 'look;
            }
            self.seen_activity = now;
            self.syncing = true;
            for i in 0..self.instances.len() {
                let e: Entity = self.instances[i].entity;
                let dipped_at = match &changes {
                    Some(c) if !c.touched.contains(&e) => continue,
                    Some(c) => c.dipped.get(&e).copied(),
                    None => None,
                };
                // While Play starts, scripts wake in order (the start pass reaches them); one
                // it already went by, switched on by a later Awake, wakes now.
                if self.start_pending.contains(&i) && !self.instances[i].awoken {
                    continue;
                }
                let n = self.reconcile(i, dipped_at, true);
                if n > 0 {
                    if flips.len() <= i {
                        flips.resize(i + 1, 0);
                    }
                    flips[i] += n;
                    if flips[i] > MAX_SWITCHES {
                        self.syncing = false;
                        break 'look;
                    }
                }
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
