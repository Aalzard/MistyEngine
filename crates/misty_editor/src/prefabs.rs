//! Prefab commands: create from the Hierarchy, instantiate, select, revert, apply (all or one
//! override at a time), unpack, variants; and keeping instances in step with their assets.
//!
//! When an asset changes (an Apply, a Prefab Mode save, a file changed on disk), every instance
//! in the open scenes follows it in place and keeps its overrides, and so do the prefabs that use
//! it (variants of it, prefabs with instances of it inside), recursively ([`propagate`]).
//! Scene files store each instance's override record when they are saved, so an instance whose
//! asset changed while its scene was closed catches up when the scene opens ([`catch_up`]).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::{self, Override, OverrideItem, Prefab};
use misty_engine::scene::{Scene, SceneId};

use crate::actions::Action;
use crate::assets::AssetDatabase;
use crate::state::EditorState;

/// Apply a prefab command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::CreatePrefab { entity, folder } => create(s, entity, &folder),
        Action::InstantiatePrefab { path, parent } => {
            instantiate(s, &path, parent);
        }
        Action::SelectPrefabAsset(e) => {
            if let Some(rel) = s.scene.prefab_link(e) {
                select_asset(s, &rel);
            }
        }
        Action::RevertPrefab(e) => revert(s, e),
        Action::ApplyPrefab(e) => apply_all(s, e),
        Action::ApplyPrefabOverrides { root, keys } => apply_some(s, root, &keys),
        Action::RevertPrefabOverrides { root, keys } => revert_some(s, root, &keys),
        Action::UnpackPrefab(e) => unpack(s, e, false),
        Action::UnpackPrefabCompletely(e) => unpack(s, e, true),
        Action::CreatePrefabVariant(rel) => {
            create_variant(s, &rel);
        }
        a @ (Action::OpenPrefab { .. }
        | Action::ClosePrefab { .. }
        | Action::SavePrefabStage
        | Action::PrefabModeAnswer(_)) => {
            crate::prefab_mode::apply(s, a);
        }
        other => return Some(other),
    }
    s.selection.retain_existing(&s.scene);
    None
}

/// Model files act as read-only prefabs: their instances can be reverted, not applied.
pub fn read_only(rel: &str) -> bool {
    misty_engine::models::is_model(rel)
}

/// "Crate" for "Assets/Props/Crate.prefab".
pub fn display_name(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_owned())
}

/// The asset an instance links to (prefabs, and model files as read-only prefabs).
pub fn asset(s: &mut EditorState, rel: &str) -> Result<Rc<Prefab>, String> {
    s.assets.prefab(rel)
}

/// The overrides of the instance at `root` against its asset (`None` when the asset is missing).
pub fn overrides_of(s: &mut EditorState, root: Entity) -> Option<Vec<OverrideItem>> {
    let rel = s.scene.prefab_link(root)?;
    let asset = asset(s, &rel).ok()?;
    Some(s.scene.prefab_overrides(root, &asset))
}

/// "Name.prefab" in `folder`, numbered if taken ("Name 1.prefab").
fn free_path(folder: &Path, name: &str) -> PathBuf {
    // Characters Windows does not allow in file names.
    let clean: String = name
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    (0..)
        .map(|n| {
            folder.join(if n == 0 {
                format!("{clean}.{}", prefab::EXTENSION)
            } else {
                format!("{clean} {n}.{}", prefab::EXTENSION)
            })
        })
        .find(|p| !p.exists())
        .expect("some name is free")
}

/// Save `e` and its children as a new prefab asset and turn `e` into an instance of it.
fn create(s: &mut EditorState, e: Entity, folder: &Path) {
    if !s.scene.contains(e) {
        return;
    }
    let path = free_path(folder, &s.scene.name(e));
    let rel = s.assets.relative(&path);
    let prefab = Prefab::from_scene(&s.scene, e).with_ids();
    if let Err(err) = save_new(s, &rel, &prefab) {
        s.console.warn(format!("Could not create prefab: {err}"));
        return;
    }
    s.record("Create Prefab");
    s.scene.link_prefab(e, &rel);
    s.scene.store_prefab_ids(e);
    s.project.refresh();
    s.console.info(format!("Created prefab {rel}"));
}

/// Write a new prefab file (no instances to update yet).
fn save_new(s: &mut EditorState, rel: &str, prefab: &Prefab) -> Result<(), String> {
    let prefab = with_records(&mut s.assets, prefab);
    s.assets.save_prefab(rel, &prefab)?;
    crate::live_reload::assets::note_prefab(s, rel);
    Ok(())
}

/// `prefab` with fresh override records for its nested instances and its base.
fn with_records(assets: &mut AssetDatabase, prefab: &Prefab) -> Prefab {
    prefab.with_records(&mut |path| assets.prefab(path).ok().map(|p| (*p).clone()))
}

/// Instance of the asset at `rel`: under `parent`, or at the Scene view pivot. Undoable.
pub fn instantiate(s: &mut EditorState, rel: &str, parent: Option<Entity>) -> Option<Entity> {
    let prefab = match asset(s, rel) {
        Ok(p) => (*p).clone().with_ids(),
        Err(err) => {
            s.console.warn(format!("Could not instantiate: {err}"));
            return None;
        }
    };
    let parent = parent.filter(|p| s.scene.contains(*p));
    if creates_cycle(s, rel, parent) {
        s.console.warn(format!(
            "{} can't go inside itself (a prefab can't contain its own instance).",
            display_name(rel)
        ));
        return None;
    }
    s.record("Instantiate Prefab");
    let e = s.scene.instantiate_prefab(&prefab, Some(rel), parent);
    if parent.is_none() {
        let pivot = s.scene_view.camera.pivot().round();
        s.scene.set_world_position(e, pivot);
    }
    s.select(e);
    Some(e)
}

/// Placing an instance of `rel` under `parent` would put a prefab inside itself: in Prefab
/// Mode of a prefab that `rel` is or uses, or inside an instance of one.
fn creates_cycle(s: &mut EditorState, rel: &str, parent: Option<Entity>) -> bool {
    let mut around: Vec<String> = crate::prefab_mode::open_path(s).into_iter().collect();
    if let Some(p) = parent {
        let roots = s.scene.prefab_roots(p);
        around.extend(roots.into_iter().filter_map(|r| s.scene.prefab_link(r)));
    }
    around
        .iter()
        .any(|path| path == rel || uses_deep(&mut s.assets, rel, path, 0))
}

/// The prefab at `rel` uses `target` (as a base or nested), directly or further down.
fn uses_deep(assets: &mut AssetDatabase, rel: &str, target: &str, depth: usize) -> bool {
    if depth > 16 {
        return true;
    }
    let Ok(p) = assets.prefab(rel) else {
        return false;
    };
    let mut used: Vec<String> = p.nested_paths();
    used.extend(p.base().map(str::to_owned));
    used.iter()
        .any(|u| u == target || uses_deep(assets, u, target, depth + 1))
}

/// Show the asset in the Project window and the Inspector.
pub fn select_asset(s: &mut EditorState, rel: &str) {
    let abs = s.assets.absolute(rel);
    if let Some(dir) = abs.parent() {
        s.project.search.clear();
        s.project.open_folder(dir.to_path_buf());
    }
    s.project.selected = Some(abs);
    s.selection.clear();
    s.inspector_asset = Some(rel.to_owned());
}

fn revert(s: &mut EditorState, e: Entity) {
    let Some(rel) = s.scene.prefab_link(e) else {
        return;
    };
    match asset(s, &rel) {
        Ok(prefab) => {
            s.record("Revert Prefab");
            s.scene.revert_prefab(e, &prefab);
            s.select(e);
        }
        Err(err) => s.console.warn(format!("Could not revert: {err}")),
    }
}

/// Revert some overrides of the instance at `root`.
fn revert_some(s: &mut EditorState, root: Entity, keys: &[Override]) {
    let Some(rel) = s.scene.prefab_link(root) else {
        return;
    };
    match asset(s, &rel) {
        Ok(prefab) => {
            let label = match keys {
                [one] => format!("Revert {}", short_label(one)),
                _ => "Revert Prefab Overrides".to_owned(),
            };
            s.record(&label);
            s.scene.revert_prefab_overrides(root, &prefab, keys);
        }
        Err(err) => s.console.warn(format!("Could not revert: {err}")),
    }
}

/// Write the instance back to its asset (all of its overrides).
fn apply_all(s: &mut EditorState, e: Entity) {
    let Some(rel) = s.scene.prefab_link(e) else {
        return;
    };
    let Some(items) = overrides_of(s, e) else {
        s.console.warn(format!("Could not apply: {rel} is missing"));
        return;
    };
    let keys: Vec<Override> = items.into_iter().map(|o| o.key).collect();
    apply_some(s, e, &keys);
}

/// Apply some overrides of the instance at `root` to its asset, then update everything that
/// uses the asset.
fn apply_some(s: &mut EditorState, root: Entity, keys: &[Override]) {
    let Some(rel) = s.scene.prefab_link(root) else {
        return;
    };
    if read_only(&rel) {
        s.console.warn(format!(
            "{} is a model file: its instances can't be applied (make a Prefab Variant of it).",
            display_name(&rel)
        ));
        return;
    }
    let old = match asset(s, &rel) {
        Ok(p) => p,
        Err(err) => {
            s.console.warn(format!("Could not apply: {err}"));
            return;
        }
    };
    let before = s.scene.clone();
    // Objects the instance added get ids as they join the asset: part of the same undo step.
    let ids_given = keys
        .iter()
        .any(|k| matches!(k, Override::AddedObject { .. }));
    let new = s.scene.apply_prefab_overrides(root, &old, keys);
    match save_and_follow(s, &rel, &old, &new) {
        Ok(changed) => {
            if changed > 0 || ids_given {
                s.undo.push("Apply Prefab", &before, &s.selection);
                s.scene_dirty = true;
            }
            s.console.info(format!("Applied changes to {rel}"));
        }
        Err(err) => {
            // Applying gave instance objects new prefab ids: put the scene back as it was
            // (nothing was saved, and no undo step records the change).
            let mut before = before;
            before.retire_ids_of(&s.scene);
            s.scene = before;
            s.selection.retain_existing(&s.scene);
            s.console.warn(format!("Could not apply: {err}"));
        }
    }
}

/// Save a changed asset and update its instances and dependents, as one undo step for the
/// scene being edited. Returns how many instances in it changed.
pub fn save_changed(
    s: &mut EditorState,
    rel: &str,
    old: &Prefab,
    new: &Prefab,
) -> Result<usize, String> {
    let before = s.scene.clone();
    let changed = save_and_follow(s, rel, old, new)?;
    if changed > 0 {
        s.undo
            .push("Update Prefab Instances", &before, &s.selection);
        s.scene_dirty = true;
    }
    Ok(changed)
}

/// [`save_changed`] without the undo step.
fn save_and_follow(
    s: &mut EditorState,
    rel: &str,
    old: &Prefab,
    new: &Prefab,
) -> Result<usize, String> {
    // An instance of the prefab dragged or pasted inside one of its own instances (or into
    // its Prefab Mode) would be written into it: every Apply then nested it one level deeper.
    let mut used = new.nested_paths();
    used.extend(new.base().map(str::to_owned));
    if used
        .iter()
        .any(|u| u == rel || uses_deep(&mut s.assets, u, rel, 0))
    {
        return Err(format!(
            "{} can't contain an instance of itself",
            display_name(rel)
        ));
    }
    let new = with_records(&mut s.assets, new);
    s.assets.save_prefab(rel, &new)?;
    crate::live_reload::assets::note_prefab(s, rel);
    Ok(propagate(s, rel, old, &new))
}

/// The asset at `rel` changed from `old` to `new` (already saved): instances in the open
/// scenes (and the scenes Prefab Mode keeps aside) follow it, keeping their overrides, and
/// prefabs that use it are updated and saved, and so on up. Not an undo step by itself (see
/// [`save_changed`]). Returns how many instances changed in the scene being edited.
pub fn propagate(s: &mut EditorState, rel: &str, old: &Prefab, new: &Prefab) -> usize {
    let mut queue: Vec<(String, Prefab, Prefab)> = vec![(rel.to_owned(), old.clone(), new.clone())];
    let mut done: Vec<String> = Vec::new();
    let mut changed = 0;
    while let Some((path, old, new)) = queue.pop() {
        if done.contains(&path) || done.len() > 64 {
            continue;
        }
        done.push(path.clone());
        changed += follow_in(&mut s.scene, &path, &old, &new);
        for scene in crate::prefab_mode::stashed_scenes(s) {
            follow_in(scene, &path, &old, &new);
        }
        for dep in dependents(s, &path) {
            if done.contains(&dep) {
                continue;
            }
            let Ok(dep_old) = s.assets.prefab(&dep) else {
                continue;
            };
            let Some(dep_new) = dep_old.follow(&path, &old, &new) else {
                continue;
            };
            let dep_new = with_records(&mut s.assets, &dep_new);
            match s.assets.save_prefab(&dep, &dep_new) {
                Ok(()) => {
                    crate::live_reload::assets::note_prefab(s, &dep);
                    s.console
                        .info(format!("Updated {dep} (uses {})", display_name(&path)));
                    queue.push((dep, (*dep_old).clone(), dep_new));
                }
                Err(e) => s.console.warn(format!("Could not update {dep}: {e}")),
            }
        }
    }
    if changed > 0 {
        s.selection.retain_existing(&s.scene);
    }
    changed
}

/// Instances of `path` in `scene` follow the change, innermost first. Returns how many changed.
fn follow_in(scene: &mut Scene, path: &str, old: &Prefab, new: &Prefab) -> usize {
    let mut roots = scene.prefab_instances(path);
    roots.reverse();
    roots
        .into_iter()
        .filter(|r| scene.resync_prefab(*r, old, new))
        .count()
}

/// Prefab files under Assets that use `path` (variants of it, prefabs with instances of it).
fn dependents(s: &mut EditorState, path: &str) -> Vec<String> {
    let guid = misty_engine::asset_db::guid_of(path).map(|g| g.to_string());
    let mut files = Vec::new();
    collect_prefabs(&s.project.root.join("Assets"), &mut files);
    let mut out = Vec::new();
    for abs in files {
        let rel = s.assets.relative(&abs);
        if rel == path {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&abs) else {
            continue;
        };
        let named = text.contains(&format!("\"{path}\""))
            || guid.as_ref().is_some_and(|g| text.contains(g.as_str()));
        if named && s.assets.prefab(&rel).is_ok_and(|p| p.uses(path)) {
            out.push(rel);
        }
    }
    out
}

fn collect_prefabs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_prefabs(&p, out);
        } else if p.extension().is_some_and(|e| e == prefab::EXTENSION) {
            out.push(p);
        }
    }
}

/// Unpack the instance at `e` (its root): nested instances stay linked unless `completely`.
fn unpack(s: &mut EditorState, e: Entity, completely: bool) {
    if s.scene.prefab_link(e).is_none() {
        return;
    }
    s.record(if completely {
        "Unpack Prefab Completely"
    } else {
        "Unpack Prefab"
    });
    s.scene.unpack_prefab(e, completely);
}

/// Create ▸ Prefab Variant: a new prefab next to `rel` whose root is an instance of it.
pub fn create_variant(s: &mut EditorState, rel: &str) -> Option<String> {
    let base = match asset(s, rel) {
        Ok(p) => p,
        Err(err) => {
            s.console.warn(format!("Could not create a variant: {err}"));
            return None;
        }
    };
    let name = format!("{} Variant", display_name(rel));
    let abs = s.assets.absolute(rel);
    let folder = abs.parent().map(Path::to_path_buf).unwrap_or(abs);
    let path = free_path(&folder, &name);
    let new_rel = s.assets.relative(&path);
    let variant = Prefab::variant_of(rel, &base, &name);
    if let Err(err) = save_new(s, &new_rel, &variant) {
        s.console.warn(format!("Could not create a variant: {err}"));
        return None;
    }
    s.refresh_assets();
    select_asset(s, &new_rel);
    s.console
        .info(format!("Created prefab variant {new_rel} (of {rel})"));
    Some(new_rel)
}

/// Store each instance's override record in scene `id` (or every scene), before saving.
pub fn record_overrides(scene: &mut Scene, assets: &mut AssetDatabase, id: Option<SceneId>) {
    for root in scene.all_prefab_instances() {
        if id.is_some_and(|id| scene.scene_of(root) != id) {
            continue;
        }
        let Some(rel) = scene.prefab_link(root) else {
            continue;
        };
        if let Ok(asset) = assets.prefab(&rel) {
            let keys = scene
                .prefab_overrides(root, &asset)
                .into_iter()
                .map(|o| o.key);
            scene.set_prefab_record(root, Some(prefab::diff::record(keys)));
        }
    }
}

/// After scene `id` (or every scene) loaded: instances whose asset changed since the scene was
/// saved catch up with it, keeping the overrides their records list. The scene is marked
/// changed (its file holds the old copies). Returns how many instances changed.
pub fn catch_up(s: &mut EditorState, id: Option<SceneId>) -> usize {
    let mut roots = s.scene.all_prefab_instances();
    roots.retain(|r| id.is_none_or(|id| s.scene.scene_of(*r) == id));
    roots.reverse();
    let mut changed = 0;
    for root in roots {
        if !s.scene.contains(root) {
            continue;
        }
        let Some(rel) = s.scene.prefab_link(root) else {
            continue;
        };
        if let Ok(asset) = s.assets.prefab(&rel)
            && s.scene.sync_prefab_to_record(root, &asset)
        {
            changed += 1;
        }
    }
    if changed > 0 {
        s.scene_dirty = true;
        s.console.info(format!(
            "{changed} prefab instance(s) caught up with prefabs changed since the scene was \
             saved (overrides kept). Save the scene to keep it."
        ));
    }
    changed
}

/// "Box Collider" / "Transform ▸ Scale" / "Handle": a short name for undo steps.
pub fn short_label(o: &Override) -> String {
    match o {
        Override::Property {
            component,
            property,
            ..
        } => format!("{component} {}", field_label(property)),
        Override::AddedComponent { component, .. }
        | Override::RemovedComponent { component, .. } => component.clone(),
        Override::AddedObject { name, .. } => name.clone(),
        Override::RemovedObject { .. } => "Removed Object".to_owned(),
    }
}

/// "use_gravity" → "Use Gravity".
pub fn field_label(field: &str) -> String {
    if field.is_empty() {
        return "Value".to_owned();
    }
    field
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
