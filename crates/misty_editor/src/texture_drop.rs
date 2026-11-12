//! A texture dragged from Asset Files onto an object (in the Editor Scene or on its Scene Tree
//! row) becomes a material on that object.
//!
//! The rule:
//!
//! - **Colour texture** (anything not detected as a normal map): the material
//!   `Assets/Materials/<TextureName>.mat` is made (the folder too when missing) with the default
//!   lit settings (the project's default Material preset when there is one) and the texture as
//!   its albedo map, then assigned to the renderer.
//!   - A material of that name whose albedo map is already this texture is assigned instead of
//!     making a copy. Materials are read with their GUID table resolved, so "this texture" is
//!     the same texture GUID, wherever the file was moved or renamed since.
//!   - A different material of that name is left alone: `<TextureName> 1.mat`, `… 2.mat` are
//!     tried in the same way (reused when they match, made when the name is free).
//! - **Normal map** (import settings say Texture Type: Normal Map, or the file name ends in
//!   `_Normal`, `_NormalMap`, `_Normals`, `_Nrm`, `_Nor` or `_N`, in any case, after `_`, `-`,
//!   `.` or a space): it goes into the Normal Map slot of the material the object has now.
//!   - That material is an asset under `Assets/` the project can write: it is edited in place
//!     (so every object using it shows the normal map).
//!   - The object uses Default-Material, a read-only package material or a missing one: a copy
//!     of it with the normal map is made in `Assets/Materials` (named after the material, or
//!     after the texture without its suffix for Default-Material: `Rock_Normal.png` gives
//!     `Rock.mat`) and assigned. An existing material of that name is reused only when it is
//!     exactly that copy.
//! - **Which element**: element 0 of the Materials list. Extra elements draw the whole mesh
//!   again on top, and picking works on bounds, so there is no element under the cursor.
//! - **Sprite Renderer** (no mesh renderer on the object): the texture becomes its sprite; no
//!   material is made.
//! - **No renderer**: a toast says so and nothing changes.
//!
//! Asset Files keeps the folder it shows (several textures can be dragged from it one after
//! another); the toast names the material, which is selected only when it is in that folder.
//!
//! Everything is one undo step. Undo takes the assignment (or the normal map) back; a material
//! file that was made stays, like one made with Assets ▸ Create. On a prefab instance the new
//! material shows as an override, like any other change to the instance.

use std::path::Path;

use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{MeshRenderer, SkinnedMeshRenderer, SpriteRenderer};
use misty_engine::sprites::{SpriteMode, TextureImporter, TextureType};

use crate::i18n::tr;
use crate::state::EditorState;

/// Where made materials go.
pub const FOLDER: &str = "Assets/Materials";

/// Name endings that mark a normal map (lower case, after a separator).
const NORMAL_SUFFIXES: [&str; 6] = ["normal", "normalmap", "normals", "nrm", "nor", "n"];
const SEPARATORS: [char; 4] = ['_', '-', '.', ' '];

/// What a drop did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A material was made and assigned.
    Created(String),
    /// An existing material was assigned (or already was).
    Reused(String),
    /// The object's material got the normal map.
    NormalMapSet(String),
    /// A Sprite Renderer got the texture as its sprite.
    Sprite,
    /// Nothing to put it on, or the material could not be written.
    Nothing,
}

/// File name of `rel` without its extension.
fn stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// `name` without a normal-map ending ("Rock_Normal" → "Rock"), when it has one.
fn without_normal_suffix(name: &str) -> Option<&str> {
    let at = name.rfind(SEPARATORS)?;
    let ending = name[at + 1..].to_ascii_lowercase();
    (at > 0 && NORMAL_SUFFIXES.contains(&ending.as_str())).then(|| &name[..at])
}

/// The texture at `rel` is a normal map: by its import settings, else by its name.
pub fn is_normal_map(state: &EditorState, rel: &str) -> bool {
    let importer = TextureImporter::load(&state.assets.absolute(rel));
    importer.texture_type == TextureType::NormalMap || without_normal_suffix(&stem(rel)).is_some()
}

/// The renderer a texture can go on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Mesh,
    Skinned,
    Sprite,
}

fn target(state: &EditorState, e: Entity) -> Option<Target> {
    let w = &state.scene.world;
    if w.get::<&MeshRenderer>(e).is_ok() {
        Some(Target::Mesh)
    } else if w.get::<&SkinnedMeshRenderer>(e).is_ok() {
        Some(Target::Skinned)
    } else if w.get::<&SpriteRenderer>(e).is_ok() {
        Some(Target::Sprite)
    } else {
        None
    }
}

/// Element 0 of the Materials list (`None` = Default-Material).
fn material_of(state: &EditorState, e: Entity, target: Target) -> Option<String> {
    let w = &state.scene.world;
    match target {
        Target::Mesh => w.get::<&MeshRenderer>(e).ok()?.material.clone(),
        Target::Skinned => w
            .get::<&SkinnedMeshRenderer>(e)
            .ok()?
            .materials
            .first()
            .cloned()
            .flatten(),
        Target::Sprite => None,
    }
}

/// Assign material `rel` to element 0 as one undo step (none when it is there already).
fn assign(state: &mut EditorState, e: Entity, target: Target, rel: &str) {
    if material_of(state, e, target).as_deref() == Some(rel) {
        return;
    }
    state.record("Assign Material");
    let w = &mut state.scene.world;
    match target {
        Target::Mesh => {
            if let Ok(mut r) = w.get::<&mut MeshRenderer>(e) {
                r.material = Some(rel.to_owned());
            }
        }
        Target::Skinned => {
            if let Ok(mut r) = w.get::<&mut SkinnedMeshRenderer>(e) {
                match r.materials.first_mut() {
                    Some(first) => *first = Some(rel.to_owned()),
                    None => r.materials.push(Some(rel.to_owned())),
                }
            }
        }
        Target::Sprite => {}
    }
}

/// The material named `name` in [`FOLDER`] that `same` accepts, else `material` written under
/// the first free name (`name`, `name 1`, …). Returns its path and whether it was made.
fn find_or_create(
    state: &mut EditorState,
    name: &str,
    material: Material,
    same: impl Fn(&Material) -> bool,
) -> Result<(String, bool), String> {
    let dir = state.project.root.join(FOLDER);
    for n in 0usize.. {
        let file = if n == 0 {
            format!("{name}.mat")
        } else {
            format!("{name} {n}.mat")
        };
        let rel = format!("{FOLDER}/{file}");
        let path = dir.join(&file);
        if !path.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            // Written whole (temp file, then rename), with the GUIDs of its textures.
            material.save(&path)?;
            // The usual path of a new asset: its `.meta` (GUID) is written by the scan, and
            // Asset Files and the material list pick it up.
            state.refresh_assets();
            state.assets.materials.insert(rel.clone(), material);
            return Ok((rel, true));
        }
        let existing = match state.assets.materials.get(&rel) {
            Some(m) => Some(m.clone()),
            None => Material::load(&path).ok(),
        };
        if existing.is_some_and(|m| same(&m)) {
            return Ok((rel, false));
        }
    }
    unreachable!("some name is free")
}

/// A new material's values: the defaults, with the project's default Material preset.
fn new_material(state: &EditorState) -> Material {
    let m = Material::default();
    match crate::presets::default_for(state, misty_engine::preset::MATERIAL) {
        Some(p) => p.apply_value(&m).unwrap_or(m),
        None => m,
    }
}

/// Select a made material in Asset Files only when it is already in view (the folder shown,
/// no search). Asset Files never moves to another folder: that would get in the way of
/// dragging several textures from one folder one after another. The scene selection and
/// Properties stay.
fn show_in_asset_files(state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    if state.project.search.is_empty() && abs.parent() == Some(state.project.current.as_path()) {
        state.project.selected = Some(abs);
    }
}

/// Toast and Console line for a material that was made or assigned.
fn report(state: &mut EditorState, rel: &str, created: bool) -> Outcome {
    let name = file_name(rel).to_owned();
    if created {
        show_in_asset_files(state, rel);
        state.console.info(format!("Created {rel}"));
        state
            .live
            .toast(tr!("Created material {name}", name = name));
        Outcome::Created(rel.to_owned())
    } else {
        state
            .live
            .toast(tr!("Assigned material {name}", name = name));
        Outcome::Reused(rel.to_owned())
    }
}

fn failed(state: &mut EditorState, err: &str) -> Outcome {
    state
        .console
        .warn(format!("Could not create the material: {err}"));
    state.live.toast(tr!("Could not create the material"));
    Outcome::Nothing
}

/// A colour texture: `<TextureName>.mat` with it as the albedo map, assigned to `e`.
fn drop_albedo(state: &mut EditorState, e: Entity, target: Target, texture: &str) -> Outcome {
    let mut material = new_material(state);
    material.albedo_map = Some(texture.to_owned());
    let same = |m: &Material| m.albedo_map.as_deref() == Some(texture);
    match find_or_create(state, &stem(texture), material, same) {
        Ok((rel, created)) => {
            assign(state, e, target, &rel);
            report(state, &rel, created)
        }
        Err(err) => failed(state, &err),
    }
}

/// A normal map: into the object's material, or into a copy when that one cannot be edited.
fn drop_normal(state: &mut EditorState, e: Entity, target: Target, texture: &str) -> Outcome {
    let current = material_of(state, e, target);
    let loaded = current
        .as_ref()
        .and_then(|rel| state.assets.materials.get(rel).cloned());
    let editable = current.as_ref().is_some_and(|rel| {
        rel.starts_with("Assets/") && !state.project.is_read_only(&state.assets.absolute(rel))
    });
    if let (Some(rel), Some(before), true) = (&current, &loaded, editable) {
        if before.normal_map.as_deref() != Some(texture) {
            let mut after = before.clone();
            after.normal_map = Some(texture.to_owned());
            let (scene, selection) = (&state.scene, &state.selection);
            state
                .undo
                .push_material("Assign Normal Map", scene, selection, rel, before);
            if let Err(err) = state.assets.update_material(rel, after) {
                state.console.warn(format!("Could not save {rel}: {err}"));
            }
        }
        let name = file_name(rel).to_owned();
        state
            .live
            .toast(tr!("Set the normal map of {name}", name = name));
        return Outcome::NormalMapSet(rel.clone());
    }
    // Default-Material, a package's material or a missing one: a copy takes the normal map.
    let texture_stem = stem(texture);
    let name = match (&current, &loaded) {
        (Some(rel), Some(_)) => stem(rel),
        _ => without_normal_suffix(&texture_stem)
            .unwrap_or(&texture_stem)
            .to_owned(),
    };
    let mut material = loaded.unwrap_or_else(|| new_material(state));
    material.normal_map = Some(texture.to_owned());
    let wanted = material.clone();
    match find_or_create(state, &name, material, |m| *m == wanted) {
        Ok((rel, created)) => {
            assign(state, e, target, &rel);
            report(state, &rel, created)
        }
        Err(err) => failed(state, &err),
    }
}

/// The first sprite the texture at `rel` provides (itself, or its first slice).
fn first_sprite(state: &EditorState, rel: &str) -> String {
    let importer = TextureImporter::load(&state.assets.absolute(rel));
    match (importer.sprite_mode, importer.sprites.first()) {
        (SpriteMode::Multiple, Some(s)) => format!("{rel}#{}", s.name),
        _ => rel.to_owned(),
    }
}

/// Texture `texture` ("Assets/…/Rock.png") was dropped on object `e`.
pub fn drop_on_object(state: &mut EditorState, e: Entity, texture: &str) -> Outcome {
    let Some(target) = target(state, e) else {
        let name = state.scene.name(e);
        state.live.toast(tr!(
            "{name} has no renderer to put a texture on",
            name = name
        ));
        return Outcome::Nothing;
    };
    if target == Target::Sprite {
        let sprite = first_sprite(state, texture);
        let unchanged = state
            .scene
            .world
            .get::<&SpriteRenderer>(e)
            .is_ok_and(|r| r.sprite.as_deref() == Some(sprite.as_str()));
        if !unchanged {
            state.record("Assign Sprite");
            if let Ok(mut r) = state.scene.world.get::<&mut SpriteRenderer>(e) {
                r.sprite = Some(sprite);
            }
        }
        return Outcome::Sprite;
    }
    if is_normal_map(state, texture) {
        drop_normal(state, e, target, texture)
    } else {
        drop_albedo(state, e, target, texture)
    }
}

/// An asset being dragged that an object takes as its material: a material or a texture.
pub fn dragged_onto_objects(ctx: &egui::Context) -> bool {
    egui::DragAndDrop::payload::<crate::panels::AssetPayload>(ctx)
        .is_some_and(|p| p.0.ends_with(".mat") || misty_engine::assets::is_texture(&p.0))
}

#[cfg(test)]
mod tests;
