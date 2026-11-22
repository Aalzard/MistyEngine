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

/// The project-relative path of the script file `file_name` ("Health.cs", "Spin.lua") under
/// `Assets/`, searched in sorted order; None when there is none.
pub(super) fn find_script(root: &std::path::Path, file_name: &str) -> Option<String> {
    fn walk(dir: &std::path::Path, file_name: &str) -> Option<std::path::PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in &entries {
            if p.is_file() && p.file_name().is_some_and(|n| n == file_name) {
                return Some(p.clone());
            }
        }
        entries
            .iter()
            .filter(|p| p.is_dir())
            .find_map(|p| walk(p, file_name))
    }
    let found = walk(&root.join("Assets"), file_name)?;
    let rel = found.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Add script `path` to `e` while playing: it joins the object's Scripts list now and gets
/// its instance (Awake, OnEnable) right after the calling callback; Start comes before the
/// next Update. Returns its slot.
pub(super) fn add_script(w: &mut World, e: Entity, path: String) -> usize {
    use crate::scene::{ScriptRef, Scripts};
    let script = ScriptRef {
        path,
        ..Default::default()
    };
    let pushed = match w.scene.world.get::<&mut Scripts>(e) {
        Ok(mut s) => {
            s.list.push(script.clone());
            Some(s.list.len() - 1)
        }
        Err(_) => None,
    };
    let slot = pushed.unwrap_or_else(|| {
        w.scene.add(e, Scripts { list: vec![script] });
        0
    });
    w.added_scripts.push((e, slot));
    slot
}

impl super::ScriptHost {
    /// Create the instances of scripts added while playing (Awake, OnEnable). Whether there
    /// were any.
    pub(super) fn wake_added_scripts(&mut self) -> bool {
        let added = self.with_world(|w| {
            std::mem::take(&mut w.added_scripts)
                .into_iter()
                .filter_map(|(e, slot)| {
                    let s = w.scene.world.get::<&crate::scene::Scripts>(e).ok()?;
                    s.list.get(slot).cloned().map(|r| (e, slot, r))
                })
                .collect::<Vec<_>>()
        });
        if added.is_empty() {
            return false;
        }
        let first = self.instances.len();
        for (e, slot, script) in added {
            self.instantiate(e, slot, &script);
        }
        for i in first..self.instances.len() {
            self.reconcile(i, None, false);
        }
        true
    }
}

/// Which script of an object `Destroy(script)` names.
#[derive(Debug)]
pub enum ScriptSel {
    /// A Lua instance by script name.
    Name(String),
    /// A C# instance by Scripts slot.
    Slot(usize),
}

impl super::ScriptHost {
    /// End of the frame: remove the scripts `Destroy(script)` named (OnDisable, OnDestroy),
    /// taking them out of their objects' Scripts lists; later scripts move down one slot.
    pub(super) fn remove_destroyed_scripts(&mut self) {
        let queued = self.with_world(|w| std::mem::take(&mut w.destroy_scripts));
        for (e, sel) in queued {
            let Some(i) = self.instances.iter().position(|x| {
                x.entity == e
                    && match &sel {
                        ScriptSel::Name(n) => x.cs.is_none() && x.name == *n,
                        ScriptSel::Slot(s) => x.cs.is_some() && x.slot == *s,
                    }
            }) else {
                continue;
            };
            self.goodbye(&[i]);
            let inst = self.instances.remove(i);
            let slot = inst.slot;
            match inst.cs {
                Some(cs) => self.csharp.release(cs),
                None => super::unregister_script_instance(&self.lua, e, &inst.name),
            }
            self.csharp.shift_slots(e, slot);
            for x in self
                .instances
                .iter_mut()
                .filter(|x| x.entity == e && x.slot > slot)
            {
                x.slot -= 1;
            }
            self.graphs.shift_slots(e, slot);
            self.with_world(|w| {
                if let Ok(mut s) = w.scene.world.get::<&mut crate::scene::Scripts>(e)
                    && slot < s.list.len()
                {
                    s.list.remove(slot);
                }
            });
        }
    }
}
