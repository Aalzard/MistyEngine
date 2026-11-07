//! `EditorScene` (find, create and change objects of the scene being edited) and `AssetDB`
//! (find, read and create project assets).
//!
//! Changes are plain edits: call `Undo.Record(obj, "Name")` first to make them one undo step.
//! Creating or destroying objects marks the scene changed by itself.

use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use misty_engine::scene::{ScriptRef, Scripts};
use misty_engine::scripting::api::{LGameObject, LVec3, with_world};
use misty_engine::scripting::ui_api::LColor;
use mlua::{Lua, Result, Table, Value};

use super::api::entity_of;
use super::state_mut;

fn entity(v: &Value, what: &str) -> Result<Entity> {
    entity_of(v).ok_or_else(|| mlua::Error::runtime(format!("{what} expects a GameObject")))
}

fn alive(lua: &Lua, e: Entity) -> Result<()> {
    if with_world(lua, |w| w.scene.contains(e))? {
        Ok(())
    } else {
        Err(mlua::Error::runtime("object was destroyed"))
    }
}

fn dirty(lua: &Lua) -> Result<()> {
    state_mut(lua)?.scene_dirty = true;
    Ok(())
}

/// Does `e` have the built-in component or script `name`?
fn has(w: &misty_engine::scripting::World, e: Entity, name: &str) -> bool {
    if let Some(info) = registry::find(name) {
        return (info.has)(&w.scene.world, e);
    }
    let script = name.strip_suffix(".lua").unwrap_or(name);
    let script = script.rsplit('/').next().unwrap_or(script);
    w.scene
        .world
        .get::<&Scripts>(e)
        .is_ok_and(|s| s.list.iter().any(|r| r.name() == script))
}

fn list(lua: &Lua, items: Vec<Entity>) -> Result<Table> {
    let t = lua.create_table()?;
    for (i, e) in items.into_iter().enumerate() {
        t.set(i + 1, LGameObject(e))?;
    }
    Ok(t)
}

pub fn register(lua: &Lua) -> Result<()> {
    register_scene(lua)?;
    register_assets(lua)
}

/// `EditorScene.Find(name)`, `FindAll(query)` ("t:Light" by component or script, otherwise a
/// name filter), `Roots()`, `Create(name [, parent])`, `InstantiatePrefab(path [, position
/// [, parent]])`, `AddComponent(go, name)` (a component or a `.lua` script path),
/// `HasComponent(go, name)`, `SetParent(go, parent)` and `Destroy(go)`.
fn register_scene(lua: &Lua) -> Result<()> {
    let s = lua.create_table()?;
    s.set(
        "Find",
        lua.create_function(|lua, name: String| {
            with_world(lua, |w| w.scene.find_by_name(&name).map(LGameObject))
        })?,
    )?;
    s.set(
        "FindAll",
        lua.create_function(|lua, query: Option<String>| {
            let q = query.unwrap_or_default();
            let found = with_world(lua, |w| {
                let all = w.scene.walk().into_iter().map(|(e, _)| e);
                match q.trim().strip_prefix("t:") {
                    Some(kind) => all.filter(|e| has(w, *e, kind.trim())).collect::<Vec<_>>(),
                    None => {
                        let needle = q.trim().to_lowercase();
                        all.filter(|e| w.scene.name(*e).to_lowercase().contains(&needle))
                            .collect()
                    }
                }
            })?;
            list(lua, found)
        })?,
    )?;
    s.set(
        "Roots",
        lua.create_function(|lua, ()| {
            let roots = with_world(lua, |w| w.scene.roots().to_vec())?;
            list(lua, roots)
        })?,
    )?;
    s.set(
        "Create",
        lua.create_function(|lua, (name, parent): (Option<String>, Value)| {
            let parent = entity_of(&parent);
            let e = with_world(lua, |w| {
                let parent = parent.filter(|p| w.scene.contains(*p));
                w.scene
                    .spawn(name.as_deref().unwrap_or("GameObject"), parent)
            })?;
            dirty(lua)?;
            Ok(LGameObject(e))
        })?,
    )?;
    s.set(
        "InstantiatePrefab",
        lua.create_function(
            |lua, (path, position, parent): (String, Option<LVec3>, Value)| {
                let prefab = state_mut(lua)?
                    .assets
                    .prefab(&path)
                    .map_err(|e| mlua::Error::runtime(format!("InstantiatePrefab: {e}")))?;
                let parent = entity_of(&parent);
                let e = with_world(lua, |w| {
                    let parent = parent.filter(|p| w.scene.contains(*p));
                    let e = w.scene.instantiate_prefab(&prefab, Some(&path), parent);
                    if let Some(p) = position {
                        w.scene.set_world_position(e, p.0);
                    }
                    e
                })?;
                dirty(lua)?;
                Ok(LGameObject(e))
            },
        )?,
    )?;
    s.set(
        "AddComponent",
        lua.create_function(|lua, (go, name): (Value, String)| {
            let e = entity(&go, "AddComponent")?;
            alive(lua, e)?;
            let added = if name.ends_with(".lua") {
                with_world(lua, |w| {
                    let mut scripts = w
                        .scene
                        .world
                        .get::<&Scripts>(e)
                        .map(|s| (*s).clone())
                        .unwrap_or_default();
                    scripts.list.push(ScriptRef {
                        path: name.clone(),
                        ..Default::default()
                    });
                    w.scene.add(e, scripts);
                    true
                })?
            } else if registry::find(&name).is_some() {
                with_world(lua, |w| w.scene.add_by_name(e, &name))?
            } else {
                return Err(mlua::Error::runtime(format!(
                    "AddComponent: no component called '{name}'"
                )));
            };
            if added {
                dirty(lua)?;
            }
            Ok(added)
        })?,
    )?;
    s.set(
        "HasComponent",
        lua.create_function(|lua, (go, name): (Value, String)| {
            let e = entity(&go, "HasComponent")?;
            with_world(lua, |w| w.scene.contains(e) && has(w, e, &name))
        })?,
    )?;
    s.set(
        "SetParent",
        lua.create_function(|lua, (go, parent): (Value, Value)| {
            let e = entity(&go, "SetParent")?;
            alive(lua, e)?;
            let parent = entity_of(&parent);
            let ok = with_world(lua, |w| w.scene.set_parent(e, parent, None, true))?;
            dirty(lua)?;
            Ok(ok)
        })?,
    )?;
    s.set(
        "Destroy",
        lua.create_function(|lua, go: Value| {
            let e = entity(&go, "Destroy")?;
            with_world(lua, |w| {
                if w.scene.contains(e) {
                    w.scene.despawn(e);
                }
            })?;
            dirty(lua)
        })?,
    )?;
    lua.globals().set("EditorScene", s)
}

/// A project path a script may write: under `Assets/`, no `..`.
fn writable(path: &str) -> Result<String> {
    let p = path.replace('\\', "/");
    if !p.starts_with("Assets/") || p.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err(mlua::Error::runtime(format!(
            "\"{path}\" must be a path inside Assets/, like \"Assets/Data/Notes.txt\""
        )));
    }
    Ok(p)
}

/// After a script made an asset: rescan, then run the asset processors on it.
fn created(lua: &Lua, rel: &str) -> Result<()> {
    let st = state_mut(lua)?;
    st.refresh_assets();
    st.assets.imported.push(rel.to_owned());
    Ok(())
}

/// `AssetDB.Find(query [, folder])` ("t:prefab" / "t:texture" / … and a name filter) → paths;
/// `Exists(path)`, `ReadText(path)`, `CreateAsset(path, text)`, `CreateFolder(path)`,
/// `CreateMaterial(path [, color])`, `Refresh()`.
fn register_assets(lua: &Lua) -> Result<()> {
    let a = lua.create_table()?;
    a.set(
        "Find",
        lua.create_function(|lua, (query, folder): (Option<String>, Option<String>)| {
            let root = state_mut(lua)?.project.root.clone();
            let query = query.unwrap_or_default();
            let mut kind = String::new();
            let mut words = Vec::new();
            for w in query.split_whitespace() {
                match w.strip_prefix("t:") {
                    Some(k) => kind = k.to_owned(),
                    None => words.push(w.to_lowercase()),
                }
            }
            let folder = folder.map(|f| f.trim_end_matches('/').to_owned() + "/");
            let mut out: Vec<String> = misty_engine::vfs::files_under(&root.join("Assets"))
                .into_iter()
                .filter_map(|p| {
                    Some(
                        p.strip_prefix(&root)
                            .ok()?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    )
                })
                .filter(|rel| super::gui::asset_matches(rel, &kind))
                .filter(|rel| folder.as_ref().is_none_or(|f| rel.starts_with(f.as_str())))
                .filter(|rel| {
                    let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
                    words.iter().all(|w| name.contains(w.as_str()))
                })
                .collect();
            out.sort();
            Ok(out)
        })?,
    )?;
    a.set(
        "Exists",
        lua.create_function(|lua, path: String| {
            Ok(state_mut(lua)?.project.root.join(&path).exists())
        })?,
    )?;
    a.set(
        "ReadText",
        lua.create_function(|lua, path: String| {
            let abs = state_mut(lua)?.project.root.join(&path);
            misty_engine::vfs::read_to_string(&abs)
                .map_err(|e| mlua::Error::runtime(format!("ReadText: {path}: {e}")))
        })?,
    )?;
    a.set(
        "CreateAsset",
        lua.create_function(|lua, (path, text): (String, String)| {
            let rel = writable(&path)?;
            let abs = state_mut(lua)?.project.root.join(&rel);
            if let Some(dir) = abs.parent() {
                std::fs::create_dir_all(dir).map_err(|e| mlua::Error::runtime(e.to_string()))?;
            }
            std::fs::write(&abs, text)
                .map_err(|e| mlua::Error::runtime(format!("CreateAsset: {rel}: {e}")))?;
            created(lua, &rel)?;
            Ok(rel)
        })?,
    )?;
    a.set(
        "CreateFolder",
        lua.create_function(|lua, path: String| {
            let rel = writable(&path)?;
            let abs = state_mut(lua)?.project.root.join(&rel);
            std::fs::create_dir_all(&abs)
                .map_err(|e| mlua::Error::runtime(format!("CreateFolder: {rel}: {e}")))?;
            state_mut(lua)?.refresh_assets();
            Ok(rel)
        })?,
    )?;
    a.set(
        "CreateMaterial",
        lua.create_function(|lua, (path, color): (String, Option<LColor>)| {
            let mut rel = writable(&path)?;
            if !rel.ends_with(".mat") {
                rel.push_str(".mat");
            }
            let abs = state_mut(lua)?.project.root.join(&rel);
            if let Some(dir) = abs.parent() {
                std::fs::create_dir_all(dir).map_err(|e| mlua::Error::runtime(e.to_string()))?;
            }
            let mut m = misty_engine::assets::Material::default();
            if let Some(c) = color {
                m.color = c.0;
            }
            m.save(&abs)
                .map_err(|e| mlua::Error::runtime(format!("CreateMaterial: {rel}: {e}")))?;
            created(lua, &rel)?;
            Ok(rel)
        })?,
    )?;
    a.set(
        "Refresh",
        lua.create_function(|lua, ()| {
            state_mut(lua)?.refresh_assets();
            Ok(())
        })?,
    )?;
    lua.globals().set("AssetDB", a)?;
    Ok(())
}
