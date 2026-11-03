//! Edit-mode preview and Record mode, run around the UI pass, plus clip edits with undo.
//!
//! While previewing, the clip is sampled onto the scene at the playhead every frame. The values
//! the clip overrides are backed up the first time they are touched and written back when the
//! preview ends, so previewing never changes the saved scene. In Record mode, any change to an
//! animatable property of the animated objects during a frame (Inspector, gizmos, …) is turned
//! into a key at the playhead.

use misty_engine::animation::{AnimationClip, Property, binding};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Animation, clip_name};

use crate::state::EditorState;

/// Values differing by less than this are unchanged (float noise from Euler conversion).
const CHANGE_EPSILON: f32 = 1e-4;

/// The object the Animation window animates: the selection or its nearest ancestor with an
/// Animation component.
pub fn target_root(s: &EditorState) -> Option<Entity> {
    let mut cur = s.selection.active().filter(|e| s.scene.contains(*e));
    while let Some(e) = cur {
        if s.scene.world.get::<&Animation>(e).is_ok() {
            return Some(e);
        }
        cur = s.scene.parent(e);
    }
    None
}

/// Clips of the target's Animation component (default first).
pub fn target_clips(s: &EditorState, root: Entity) -> Vec<String> {
    s.scene
        .world
        .get::<&Animation>(root)
        .map(|a| a.all_clips())
        .unwrap_or_default()
}

/// Make sure the window's clip is one of the target's clips.
fn pick_clip(s: &mut EditorState, root: Option<Entity>) {
    let clips = root.map(|r| target_clips(s, r)).unwrap_or_default();
    let w = &mut s.animation;
    if w.clip.as_ref().is_none_or(|c| !clips.contains(c)) {
        w.clip = clips.first().cloned();
        w.selected.clear();
    }
}

/// Before the UI: advance window playback, pose the scene and remember values for Record mode.
pub fn pre_frame(s: &mut EditorState, dt: f32) {
    if s.play.in_play_mode() {
        stop_preview(s);
        return;
    }
    let root = target_root(s);
    // Selecting another object ends the preview of the previous one.
    if s.animation.preview_root.is_some() && s.animation.preview_root != root {
        stop_preview(s);
    }
    pick_clip(s, root);
    s.animation.baseline.clear();
    let (Some(root), true) = (root, s.animation.preview) else {
        return;
    };
    let Some(clip) = active_clip(s) else {
        return;
    };
    let w = &mut s.animation;
    w.preview_root = Some(root);
    if w.playing {
        w.time += dt;
        if clip.length > 0.0 && w.time > clip.length {
            w.time = w.time.rem_euclid(clip.length);
        }
    }
    for c in &clip.curves {
        if let Some(e) = binding::resolve(&s.scene, root, &c.path)
            && let Some(v) = binding::read(&s.scene, e, c.property)
        {
            w.backup.entry((e, c.property)).or_insert(v);
        }
    }
    binding::sample(&mut s.scene, root, &clip, w.time);
    if w.recording {
        let subtree: Vec<Entity> = s
            .scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| s.scene.is_ancestor(root, *e))
            .collect();
        for e in subtree {
            for p in Property::ALL {
                if binding::has(&s.scene, e, p)
                    && let Some(v) = binding::read(&s.scene, e, p)
                {
                    w.baseline.push((e, p, v));
                }
            }
        }
    }
}

/// After the UI: key what changed in Record mode, and save finished clip edits.
pub fn post_frame(s: &mut EditorState, interacting: bool) {
    let skip = std::mem::take(&mut s.animation.skip_record);
    if !skip && s.animation.recording && s.animation.preview {
        record_changes(s, interacting);
    }
    if !interacting {
        for err in s.anim_clips.save_dirty() {
            s.console.warn(err);
        }
    }
}

fn record_changes(s: &mut EditorState, interacting: bool) {
    let (Some(root), Some(path)) = (s.animation.preview_root, s.animation.clip.clone()) else {
        return;
    };
    if target_root(s) != Some(root) {
        return;
    }
    let changes: Vec<(Entity, Property, f32, f32)> = s
        .animation
        .baseline
        .iter()
        .filter_map(|&(e, p, old)| {
            let new = binding::read(&s.scene, e, p)?;
            ((new - old).abs() > CHANGE_EPSILON).then_some((e, p, old, new))
        })
        .collect();
    if changes.is_empty() {
        return;
    }
    // Edits from a drag or typing are already grouped into one undo step with the clip.
    if !interacting && let Some(snap) = s.anim_clips.snapshot(&path) {
        s.undo
            .push_with_clip("Record Key", &s.scene, &s.selection, Some(snap));
    }
    let time = s.animation.time;
    let mut keyed = Vec::new();
    for (e, p, old, new) in changes {
        let Some(curve_path) = binding::path_of(&s.scene, root, e) else {
            continue;
        };
        let Some(clip) = s.anim_clips.get_mut(&path) else {
            return;
        };
        let time = clip.snap(time);
        let existed = clip.curve(&curve_path, p).is_some();
        clip.set_key(&curve_path, p, time, new);
        if !existed {
            // A newly recorded property also gets its original value at the start.
            if time > 0.0 {
                clip.set_key(&curve_path, p, 0.0, old);
            }
            s.animation.backup.entry((e, p)).or_insert(old);
        }
        keyed.push((e, p, new));
    }
    // Keep the new values for the rest of this frame's baseline.
    for entry in &mut s.animation.baseline {
        if let Some((_, _, v)) = keyed
            .iter()
            .find(|(e, p, _)| *e == entry.0 && *p == entry.1)
        {
            entry.2 = *v;
        }
    }
}

/// End the preview: animated values go back to what they were before it started.
pub fn stop_preview(s: &mut EditorState) {
    let w = &mut s.animation;
    for ((e, p), v) in std::mem::take(&mut w.backup) {
        binding::write(&mut s.scene, e, p, v);
    }
    w.preview = false;
    w.playing = false;
    w.recording = false;
    w.preview_root = None;
    w.baseline.clear();
}

/// Copy of the window's clip, if it loads.
pub fn active_clip(s: &mut EditorState) -> Option<AnimationClip> {
    let path = s.animation.clip.clone()?;
    match s.anim_clips.get(&path) {
        Ok(c) => Some(c.clone()),
        Err(e) => {
            if s.animation.load_error.as_deref() != Some(e.as_str()) {
                s.console.warn(e.clone());
                s.animation.load_error = Some(e);
            }
            None
        }
    }
}

/// Change the window's clip with one undo step. Returns false if there is no clip.
pub fn edit_clip(s: &mut EditorState, label: &str, f: impl FnOnce(&mut AnimationClip)) -> bool {
    let Some(path) = s.animation.clip.clone() else {
        return false;
    };
    let Some(snap) = s.anim_clips.snapshot(&path) else {
        return false;
    };
    s.undo
        .push_with_clip(label, &s.scene, &s.selection, Some(snap));
    match s.anim_clips.get_mut(&path) {
        Some(c) => {
            f(c);
            true
        }
        None => false,
    }
}

/// "Create New Clip": a new `.anim` in Assets/Animations named after the object, added to its
/// Animation component (created if missing) and opened in the window.
pub fn create_clip(s: &mut EditorState, e: Entity) -> Option<String> {
    let base = s.scene.name(e);
    let taken = |rel: &str| s.project.root.join(rel).exists();
    let rel = (0..)
        .map(|n| match n {
            0 => format!("Assets/Animations/{base}.anim"),
            n => format!("Assets/Animations/{base} {n}.anim"),
        })
        .find(|r| !taken(r))?;
    if let Err(err) = s.anim_clips.create(&rel, AnimationClip::default()) {
        s.console
            .warn(format!("Could not create animation clip: {err}"));
        return None;
    }
    s.record("Create Animation Clip");
    if s.scene.world.get::<&Animation>(e).is_err() {
        s.scene.add(e, Animation::default());
    }
    if let Ok(mut a) = s.scene.world.get::<&mut Animation>(e) {
        if a.clip.is_none() {
            a.clip = Some(rel.clone());
        }
        if !a.clips.contains(&rel) {
            a.clips.push(rel.clone());
        }
    }
    s.refresh_assets();
    s.animation.clip = Some(rel.clone());
    s.animation.selected.clear();
    s.console.info(format!("Created {rel}"));
    Some(rel)
}

/// Add Property: curves for `props` on the object at `path`, keyed at the start and end of the
/// clip with the object's current values.
pub fn add_property(s: &mut EditorState, root: Entity, path: &str, props: &[Property]) {
    let Some(e) = binding::resolve(&s.scene, root, path) else {
        return;
    };
    let values: Vec<(Property, f32)> = props
        .iter()
        .filter_map(|p| Some((*p, binding::read(&s.scene, e, *p)?)))
        .collect();
    let label = format!("Add {}", props.first().map_or("Property", |p| p.group()));
    edit_clip(s, &label, |clip| {
        let end = clip.length.max(0.0);
        for &(p, v) in &values {
            if clip.curve(path, p).is_none() {
                clip.set_key(path, p, 0.0, v);
                if end > 0.0 {
                    clip.set_key(path, p, end, v);
                }
            }
        }
    });
    for (p, v) in values {
        s.animation.backup.entry((e, p)).or_insert(v);
    }
    s.animation.preview = true;
}

/// Remove the curves `curves` (indices into the clip).
pub fn remove_curves(s: &mut EditorState, curves: &[usize]) {
    let mut sorted = curves.to_vec();
    sorted.sort_unstable();
    edit_clip(s, "Remove Properties", |clip| {
        for &i in sorted.iter().rev() {
            if i < clip.curves.len() {
                clip.curves.remove(i);
            }
        }
    });
    s.animation.selected.clear();
}

/// Set (or add) a key for curve `i` at the playhead.
pub fn key_curve(s: &mut EditorState, i: usize, value: f32) {
    let time = s.animation.time;
    edit_clip(s, "Add Key", |clip| {
        let time = clip.snap(time);
        if let Some(c) = clip.curves.get_mut(i) {
            c.keys.set(time, value);
        }
        clip.fit_length();
    });
    s.animation.preview = true;
}

/// Key every curve at the playhead with its current value ("Add Keyframe" button).
pub fn key_all(s: &mut EditorState) {
    let (Some(root), Some(clip)) = (target_root(s), active_clip(s)) else {
        return;
    };
    let values: Vec<Option<f32>> = clip
        .curves
        .iter()
        .map(|c| {
            let e = binding::resolve(&s.scene, root, &c.path)?;
            binding::read(&s.scene, e, c.property)
        })
        .collect();
    let time = s.animation.time;
    edit_clip(s, "Add Keyframe", |clip| {
        let time = clip.snap(time);
        for (c, v) in clip.curves.iter_mut().zip(values) {
            let v = v.unwrap_or_else(|| c.keys.evaluate(time));
            c.keys.set(time, v);
        }
        clip.fit_length();
    });
    s.animation.preview = true;
}

/// Delete the selected keys.
pub fn delete_selected_keys(s: &mut EditorState) {
    let selected: Vec<(usize, f32)> = s
        .animation
        .selected
        .iter()
        .map(|&(c, t)| (c, f32::from_bits(t)))
        .collect();
    if selected.is_empty() {
        return;
    }
    edit_clip(s, "Delete Keys", |clip| {
        for (c, t) in selected {
            if let Some(curve) = clip.curves.get_mut(c)
                && let Some(i) = curve.keys.find(t)
            {
                curve.keys.remove(i);
            }
        }
    });
    s.animation.selected.clear();
}

/// Display name of the window's clip.
pub fn clip_label(path: Option<&str>) -> String {
    path.map(clip_name).unwrap_or_else(|| "(no clip)".into())
}
