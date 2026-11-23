//! Animator Override Controllers open in the editor: loaded on first use, edited in memory and
//! written to their `.overrideController` file right after every edit, with undo (one step per
//! interaction). Changes made while playing reach the running Animators (`play.rs`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use misty_engine::animation::{AnimatorController, OverrideController, is_override_controller};

use crate::anim_clips::ClipSnapshot;
use crate::state::EditorState;

#[derive(Default)]
pub struct OverrideStore {
    root: PathBuf,
    items: BTreeMap<String, OverrideController>,
    /// Undo label of the edit grouped with the current interaction.
    grouping: Option<String>,
    /// Overrides changed since Play mode last picked them up.
    changed: Vec<String>,
}

impl OverrideStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            ..Default::default()
        }
    }

    /// The override at a project-relative path, read from disk the first time.
    pub fn get(&mut self, path: &str) -> Result<&OverrideController, String> {
        if !self.items.contains_key(path) {
            let o = OverrideController::load(&self.root.join(path))?;
            self.items.insert(path.to_owned(), o);
        }
        Ok(&self.items[path])
    }

    pub fn snapshot(&mut self, path: &str) -> Option<ClipSnapshot> {
        self.get(path)
            .ok()
            .map(|o| ClipSnapshot::Override(path.to_owned(), o.clone()))
    }

    /// Put an undo snapshot back (and save it); returns the override it replaced.
    pub fn restore(&mut self, path: String, o: OverrideController) -> ClipSnapshot {
        let _ = o.save(&self.root.join(&path));
        let old = self.items.insert(path.clone(), o).unwrap_or_default();
        self.touch(&path);
        ClipSnapshot::Override(path, old)
    }

    fn touch(&mut self, path: &str) {
        if !self.changed.iter().any(|p| p == path) {
            self.changed.push(path.to_owned());
        }
    }

    /// Overrides changed since the last call, with their current contents.
    pub fn take_changed(&mut self) -> Vec<(String, OverrideController)> {
        std::mem::take(&mut self.changed)
            .into_iter()
            .filter_map(|p| self.items.get(&p).cloned().map(|o| (p, o)))
            .collect()
    }

    /// Follow an asset rename/move of `old` (an override or a folder) to `new`; references to
    /// renamed controllers and clips inside held overrides follow too.
    pub fn rename(&mut self, old: &str, new: &str) {
        let keys: Vec<String> = self.items.keys().cloned().collect();
        for key in keys {
            if let Some(to) = crate::asset_ops::remap(&key, old, new) {
                let o = self.items.remove(&key).expect("listed key");
                self.items.insert(to, o);
            }
        }
        for o in self.items.values_mut() {
            if let Some(c) = o
                .controller
                .as_deref()
                .and_then(|c| crate::asset_ops::remap(c, old, new))
            {
                o.controller = Some(c);
            }
            let moved: BTreeMap<String, String> = std::mem::take(&mut o.overrides)
                .into_iter()
                .map(|(k, v)| {
                    let k = crate::asset_ops::remap(&k, old, new).unwrap_or(k);
                    let v = crate::asset_ops::remap(&v, old, new).unwrap_or(v);
                    (k, v)
                })
                .collect();
            o.overrides = moved;
        }
    }

    /// Forget the grouped edit once the mouse is up and no text field has focus.
    pub fn end_interaction(&mut self, interacting: bool) {
        if !interacting {
            self.grouping = None;
        }
    }
}

/// Change an override with undo and save it. Returns false if it does not load.
pub fn edit(
    s: &mut EditorState,
    path: &str,
    label: &str,
    interacting: bool,
    f: impl FnOnce(&mut OverrideController),
) -> bool {
    let store = &mut s.anim_clips.overrides;
    let Some(snap) = store.snapshot(path) else {
        return false;
    };
    let grouped = interacting && store.grouping.as_deref() == Some(label);
    if !grouped {
        s.undo.push_asset(label, &s.selection, snap);
        s.anim_clips.overrides.grouping = interacting.then(|| label.to_owned());
    }
    let store = &mut s.anim_clips.overrides;
    let Some(o) = store.items.get_mut(path) else {
        return false;
    };
    let before = o.clone();
    f(o);
    if *o == before {
        return true;
    }
    if let Err(e) = o.save(&store.root.join(path)) {
        s.console.warn(format!("Could not save {path}: {e}"));
    }
    store.touch(path);
    true
}

// ---- the editing logic (no UI) -------------------------------------------------------------

/// Every clip `base` plays (state motions, blend tree children, synced-layer motions), each
/// once, in the order the controller lists them.
pub fn original_clips(base: &AnimatorController) -> Vec<String> {
    let found = std::cell::RefCell::new(Vec::<String>::new());
    let mut c = base.clone();
    c.remap_motions(|clip| {
        let mut f = found.borrow_mut();
        if !f.iter().any(|x| x == clip) {
            f.push(clip.to_owned());
        }
        None
    });
    found.into_inner()
}

/// The replacement of `original`, if it really is replaced (a clip mapped to itself, as
/// Create lists them, is not).
pub fn override_of<'a>(o: &'a OverrideController, original: &str) -> Option<&'a str> {
    o.overrides
        .get(original)
        .map(String::as_str)
        .filter(|r| *r != original && !r.is_empty())
}

/// Set (or with `None`, clear) the override of `original`.
pub fn set_override(o: &mut OverrideController, original: &str, replacement: Option<String>) {
    match replacement.filter(|r| r != original && !r.is_empty()) {
        Some(r) => {
            o.overrides.insert(original.to_owned(), r);
        }
        None => {
            o.overrides.remove(original);
        }
    }
}

/// Overrides whose original clip the base controller doesn't play (left from another base).
pub fn unused(o: &OverrideController, originals: &[String]) -> Vec<String> {
    o.overrides
        .keys()
        .filter(|k| !originals.iter().any(|c| c == *k))
        .cloned()
        .collect()
}

/// Read the base controller of an override as the editor sees it: an open `.controller` from
/// the controller store (unsaved edits included), or an override of one (its overrides
/// applied). `seen` stops chains that loop.
pub fn load_base(
    s: &mut EditorState,
    path: &str,
    seen: &mut Vec<String>,
) -> Result<AnimatorController, String> {
    if seen.iter().any(|p| p == path) {
        return Err(format!("{path} overrides itself"));
    }
    seen.push(path.to_owned());
    if is_override_controller(path) {
        let o = s.anim_clips.overrides.get(path)?.clone();
        let base = o
            .controller
            .clone()
            .ok_or_else(|| format!("{path} has no base controller"))?;
        let base = load_base(s, &base, seen)?;
        return Ok(o.apply(&base));
    }
    s.anim_clips.controllers.get(path).cloned()
}

#[cfg(test)]
mod tests {
    use misty_engine::animation::controller::{BlendChild, BlendTree, State};

    use super::*;

    fn base() -> AnimatorController {
        let mut c = AnimatorController::default();
        for (n, clip) in [
            ("Idle", "Assets/Idle.anim"),
            ("Run", "Assets/Run.anim"),
            ("Run2", "Assets/Run.anim"),
        ] {
            c.base.states.push(State {
                name: n.into(),
                motion: Some(clip.into()),
                ..Default::default()
            });
        }
        c.base.states.push(State {
            name: "Move".into(),
            blend_tree: Some(BlendTree {
                children: vec![BlendChild {
                    motion: Some("Assets/Walk.anim".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        c
    }

    #[test]
    fn originals_are_unique_and_ordered() {
        assert_eq!(
            original_clips(&base()),
            vec!["Assets/Idle.anim", "Assets/Run.anim", "Assets/Walk.anim"]
        );
    }

    #[test]
    fn set_clear_and_unused() {
        let mut o = OverrideController::default();
        // Create maps clips to themselves: not an override.
        o.overrides
            .insert("Assets/Idle.anim".into(), "Assets/Idle.anim".into());
        assert_eq!(override_of(&o, "Assets/Idle.anim"), None);
        set_override(&mut o, "Assets/Run.anim", Some("Assets/OrcRun.anim".into()));
        assert_eq!(
            override_of(&o, "Assets/Run.anim"),
            Some("Assets/OrcRun.anim")
        );
        // Mapping back to itself or to nothing clears it.
        set_override(&mut o, "Assets/Run.anim", Some("Assets/Run.anim".into()));
        assert!(!o.overrides.contains_key("Assets/Run.anim"));
        set_override(&mut o, "Assets/Walk.anim", Some("Assets/Crawl.anim".into()));
        set_override(&mut o, "Assets/Walk.anim", None);
        assert!(!o.overrides.contains_key("Assets/Walk.anim"));
        o.overrides
            .insert("Assets/Old.anim".into(), "Assets/X.anim".into());
        assert_eq!(
            unused(&o, &original_clips(&base())),
            vec!["Assets/Old.anim"]
        );
        // The applied controller plays the replacement.
        set_override(&mut o, "Assets/Run.anim", Some("Assets/OrcRun.anim".into()));
        let applied = o.apply(&base());
        assert_eq!(
            applied.base.states[1].motion.as_deref(),
            Some("Assets/OrcRun.anim")
        );
        assert_eq!(
            applied.base.states[2].motion.as_deref(),
            Some("Assets/OrcRun.anim")
        );
        assert_eq!(
            applied.base.states[0].motion.as_deref(),
            Some("Assets/Idle.anim")
        );
    }
}
