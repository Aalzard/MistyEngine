//! Adding and removing engine components while playing (`AddComponent`, `Destroy(component)`),
//! shared by Lua and C#.

use hecs::Entity;

use super::World;
use crate::scene::registry;

/// The registry name of a component as scripts spell it: "BoxCollider" or "Box Collider" →
/// "Box Collider"; also the familiar engine's names for the few that differ.
pub(crate) fn registry_name(script_name: &str) -> Option<&'static str> {
    let wanted = match script_name {
        "Text" => "Text",
        "Image" => "Image",
        "Button" => "Button",
        "InputField" => "Input Field",
        "Renderer" => "Mesh Renderer",
        other => other,
    };
    let squash = |s: &str| s.replace(' ', "");
    let wanted = squash(wanted);
    registry::components()
        .iter()
        .find(|c| squash(c.name) == wanted)
        .map(|c| c.name)
}

/// Physics bodies and colliders depend on the components of an object and of its parents:
/// rebuild them for `e` and everything under it.
fn refresh_physics(w: &mut World, e: Entity) {
    let subtree = w.scene.subtree(e);
    for x in &subtree {
        w.physics.remove(*x);
        w.physics2d.remove(*x);
    }
    if w.scene.active_in_hierarchy(e) {
        w.physics.add_subtrees(&w.scene, &[e]);
        w.physics2d.add_subtrees(&w.scene, &[e]);
    }
}

fn is_physics(name: &str) -> bool {
    registry::find(name).is_some_and(|c| c.category == "Physics" || c.category == "Physics 2D")
}

/// `AddComponent(name)`: add a default component (and the ones it requires). Ok(false) when
/// the object already has one (nothing changes, as the familiar engine returns the existing
/// one for components that can't be doubled); Err for an unknown name.
pub(crate) fn add(w: &mut World, e: Entity, script_name: &str) -> Result<bool, String> {
    let name = registry_name(script_name).ok_or_else(|| {
        format!("AddComponent: '{script_name}' is not a component (scripts can't be added during Play yet)")
    })?;
    if name == "Transform" || name == "Game Object" {
        return Ok(false);
    }
    let added = w.scene.add_by_name(e, name);
    if added && (is_physics(name) || registry::requires(name).iter().any(|r| is_physics(r))) {
        refresh_physics(w, e);
    }
    Ok(added)
}

/// `Destroy(component)`: remove it. Ok(false) when the object doesn't have it or it can't be
/// removed (Transform).
pub(crate) fn remove(w: &mut World, e: Entity, script_name: &str) -> Result<bool, String> {
    let name = registry_name(script_name)
        .ok_or_else(|| format!("Destroy: '{script_name}' is not a component"))?;
    let removed = w.scene.remove_by_name(e, name);
    if removed && is_physics(name) {
        refresh_physics(w, e);
    }
    Ok(removed)
}
