//! `scene.*`: the open scenes' objects and components. Every change is one undo step, recorded
//! the way the Hierarchy and Inspector record theirs.

use misty_engine::hecs::Entity;
use misty_engine::scene::registry::{self, ComponentInfo};
use misty_engine::scene::{GameObject, LightType, PrimitiveMesh, ScriptValue, Scripts, Transform};
use serde_json::{Value, json};

use super::{
    Cx, Kind, Outcome, Tool, assets_path, object_id, object_path, object_ref_schema, object_schema,
    opt_bool, opt_str, opt_u64, opt_vec3, resolve, resolve_many, str_arg, summary, vec3_schema,
};
use crate::actions::{Action, Create};
use crate::state::EditorState;

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "scene.list_hierarchy",
        title: "List scene hierarchy",
        description: "The open scenes and their objects as a tree (id, name, active, components, children). With \"filter\", a flat list of matching objects instead: a name substring, or \"t:Light\" for objects with that component or script.",
        kind: Kind::Read,
        idempotent: true,
        schema: list_schema,
        run: list_hierarchy,
    },
    Tool {
        name: "scene.get_object",
        title: "Get object",
        description: "One object in full: name, tag, layer, parent, children, prefab link and every component with its field values (the names scene.set_field takes).",
        kind: Kind::Read,
        idempotent: true,
        schema: get_schema,
        run: get_object,
    },
    Tool {
        name: "scene.create_object",
        title: "Create object",
        description: "Create an object (empty, a primitive, a light, a camera…) like the Object menu does, optionally under a parent, at a position, with extra components. One undo step.",
        kind: Kind::Edit,
        idempotent: false,
        schema: create_schema,
        run: create_object,
    },
    Tool {
        name: "scene.instantiate_prefab",
        title: "Instantiate prefab",
        description: "Place an instance of a .prefab asset (or a model file such as .glb) in the scene. One undo step.",
        kind: Kind::Edit,
        idempotent: false,
        schema: prefab_schema,
        run: instantiate_prefab,
    },
    Tool {
        name: "scene.set_field",
        title: "Set component field",
        description: "Set one field of a component (\"Light\", \"Transform\", \"GameObject\" for name/active/tag/layer, or an attached script by name for its public fields). \"field\" may be a dotted path into nested values (\"color.0\"). One undo step.",
        kind: Kind::Edit,
        idempotent: true,
        schema: set_field_schema,
        run: set_field,
    },
    Tool {
        name: "scene.add_component",
        title: "Add component",
        description: "Add a built-in component by name (see scene.component_types) or a Lua script by its path (\"Assets/Scripts/Spin.lua\"). One undo step.",
        kind: Kind::Edit,
        idempotent: false,
        schema: component_schema,
        run: add_component,
    },
    Tool {
        name: "scene.remove_component",
        title: "Remove component",
        description: "Remove a component by name, or a script by its name or path. One undo step.",
        kind: Kind::Edit,
        idempotent: false,
        schema: component_schema,
        run: remove_component,
    },
    Tool {
        name: "scene.set_parent",
        title: "Set parent",
        description: "Move an object under another one (keeping its world position), or to the scene root when \"parent\" is left out. One undo step.",
        kind: Kind::Edit,
        idempotent: true,
        schema: parent_schema,
        run: set_parent,
    },
    Tool {
        name: "scene.delete_objects",
        title: "Delete objects",
        description: "Delete objects and their children. One undo step (editor.undo brings them back).",
        kind: Kind::Edit,
        idempotent: false,
        schema: objects_schema,
        run: delete_objects,
    },
    Tool {
        name: "scene.select",
        title: "Select objects",
        description: "Select objects in the Scene Tree (an empty list clears the selection).",
        kind: Kind::Edit,
        idempotent: true,
        schema: objects_schema,
        run: select,
    },
    Tool {
        name: "scene.save",
        title: "Save scene",
        description: "Save the open scenes that have changes (the prefab, in Prefab Mode). A scene that has no file yet needs \"path\" (\"Assets/Scenes/Level.scene\").",
        kind: Kind::Edit,
        idempotent: true,
        schema: save_schema,
        run: save,
    },
    Tool {
        name: "scene.open",
        title: "Open scene",
        description: "Open a .scene file, replacing the open scenes (or next to them with \"additive\"). Refuses when there are unsaved changes unless \"discard_changes\" is true (the editor then asks the user).",
        kind: Kind::Edit,
        idempotent: true,
        schema: open_schema,
        run: open,
    },
    Tool {
        name: "scene.component_types",
        title: "List component types",
        description: "The built-in components scene.add_component accepts, with their categories.",
        kind: Kind::Read,
        idempotent: true,
        schema: types_schema,
        run: component_types,
    },
];

// ---- schemas -------------------------------------------------------------------------------------

fn list_schema() -> Value {
    object_schema(
        json!({
            "filter": { "type": "string", "description": "Name substring, or \"t:Component\" / \"t:ScriptName\"" },
            "max_depth": { "type": "integer", "minimum": 0, "description": "Levels of children to list (default: all)" },
            "components": { "type": "boolean", "description": "Include component names (default true)" },
        }),
        &[],
    )
}

fn get_schema() -> Value {
    object_schema(
        json!({ "object": object_ref_schema("The object") }),
        &["object"],
    )
}

const KINDS: &[&str] = &[
    "empty",
    "folder",
    "cube",
    "sphere",
    "capsule",
    "cylinder",
    "plane",
    "quad",
    "directional_light",
    "point_light",
    "spot_light",
    "area_light",
    "camera",
    "audio_source",
    "particle_system",
    "sprite",
    "decal_projector",
    "fog_volume",
    "weather",
];

fn create_kind(kind: &str) -> Option<Create> {
    Some(match kind {
        "empty" => Create::Empty,
        "folder" => Create::Folder,
        "cube" => Create::Primitive(PrimitiveMesh::Cube),
        "sphere" => Create::Primitive(PrimitiveMesh::Sphere),
        "capsule" => Create::Primitive(PrimitiveMesh::Capsule),
        "cylinder" => Create::Primitive(PrimitiveMesh::Cylinder),
        "plane" => Create::Primitive(PrimitiveMesh::Plane),
        "quad" => Create::Primitive(PrimitiveMesh::Quad),
        "directional_light" => Create::Light(LightType::Directional),
        "point_light" => Create::Light(LightType::Point),
        "spot_light" => Create::Light(LightType::Spot),
        "area_light" => Create::Light(LightType::Rectangle),
        "camera" => Create::Camera,
        "audio_source" => Create::AudioSource,
        "particle_system" => Create::ParticleSystem,
        "sprite" => Create::Sprite(None),
        "decal_projector" => Create::DecalProjector,
        "fog_volume" => Create::FogVolume,
        "weather" => Create::Weather,
        _ => return None,
    })
}

fn create_schema() -> Value {
    object_schema(
        json!({
            "name": { "type": "string", "description": "Object name (default: from the kind)" },
            "kind": { "type": "string", "enum": KINDS, "description": "What to create (default \"empty\")" },
            "parent": object_ref_schema("Parent object (default: scene root)"),
            "position": vec3_schema("Local position [x, y, z] (default: the Editor Scene pivot at the root)"),
            "rotation": vec3_schema("Local rotation as Euler angles in degrees [x, y, z]"),
            "scale": vec3_schema("Local scale [x, y, z]"),
            "components": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Extra components or script paths to add",
            },
        }),
        &[],
    )
}

fn prefab_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Project path of a .prefab or model asset" },
            "parent": object_ref_schema("Parent object (default: scene root)"),
            "position": vec3_schema("World position [x, y, z] (default: the Editor Scene pivot)"),
        }),
        &["path"],
    )
}

fn set_field_schema() -> Value {
    object_schema(
        json!({
            "object": object_ref_schema("The object"),
            "component": { "type": "string", "description": "Component name (\"Transform\", \"Light\", \"GameObject\"…) or a script name or path" },
            "field": { "type": "string", "description": "Field name as scene.get_object shows it, or a dotted path (\"color.0\"); Transform also takes \"euler\"" },
            "value": { "description": "New value (JSON: number, string, bool, array or object)" },
        }),
        &["object", "component", "field", "value"],
    )
}

fn component_schema() -> Value {
    object_schema(
        json!({
            "object": object_ref_schema("The object"),
            "component": { "type": "string", "description": "Component name, or a script path / name" },
        }),
        &["object", "component"],
    )
}

fn parent_schema() -> Value {
    object_schema(
        json!({
            "object": object_ref_schema("The object to move"),
            "parent": object_ref_schema("The new parent (leave out for the scene root)"),
        }),
        &["object"],
    )
}

fn objects_schema() -> Value {
    object_schema(
        json!({
            "objects": {
                "type": "array",
                "items": object_ref_schema("An object"),
                "description": "Objects by id, path or unique name",
            },
        }),
        &["objects"],
    )
}

fn save_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Save the active scene to this file under Assets/ (\"Assets/Scenes/Level.scene\")" },
        }),
        &[],
    )
}

fn open_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Project path of the .scene file" },
            "additive": { "type": "boolean", "description": "Open next to the open scenes (default false)" },
            "discard_changes": { "type": "boolean", "description": "Throw away unsaved changes (default false)" },
        }),
        &["path"],
    )
}

fn types_schema() -> Value {
    object_schema(
        json!({ "filter": { "type": "string", "description": "Name or category substring" } }),
        &[],
    )
}

// ---- helpers -------------------------------------------------------------------------------------

/// The last Console line (why an editor command failed).
pub(super) fn last_log(s: &EditorState) -> String {
    s.console
        .last()
        .map(|e| e.message.clone())
        .unwrap_or_else(|| "unknown error".into())
}

/// A built-in component by name (case-insensitive), with suggestions when unknown.
fn component_info(name: &str) -> Result<&'static ComponentInfo, String> {
    if let Some(i) = registry::find(name) {
        return Ok(i);
    }
    let lower = name.to_lowercase();
    let all = registry::components();
    if let Some(i) = all.iter().find(|c| c.name.to_lowercase() == lower) {
        return Ok(i);
    }
    let close: Vec<&str> = all
        .iter()
        .filter(|c| c.user_visible() && c.name.to_lowercase().contains(&lower))
        .map(|c| c.name)
        .take(8)
        .collect();
    Err(if close.is_empty() {
        format!("no component called \"{name}\" (see scene.component_types)")
    } else {
        format!(
            "no component called \"{name}\"; did you mean {}?",
            close.join(", ")
        )
    })
}

fn is_script_ref(name: &str) -> bool {
    name.ends_with(".lua")
}

/// The slot of the script `name` (a path or a script name) on `e`.
fn script_slot(s: &EditorState, e: Entity, name: &str) -> Option<usize> {
    let scripts = s.scene.world.get::<&Scripts>(e).ok()?;
    let stem = name.strip_suffix(".lua").unwrap_or(name);
    let stem = stem.rsplit('/').next().unwrap_or(stem);
    scripts
        .list
        .iter()
        .position(|r| r.path == name || r.name() == stem)
}

/// Set `path` inside `root` to `v` (objects by key, arrays by index).
fn set_path(root: &mut Value, path: &[&str], v: Value) -> Result<(), String> {
    let Some((first, rest)) = path.split_first() else {
        *root = v;
        return Ok(());
    };
    match root {
        Value::Object(map) => {
            if rest.is_empty() {
                map.insert((*first).to_owned(), v);
                return Ok(());
            }
            let next = map
                .get_mut(*first)
                .ok_or_else(|| format!("no field \"{first}\""))?;
            set_path(next, rest, v)
        }
        Value::Array(items) => {
            let i: usize = first
                .parse()
                .map_err(|_| format!("\"{first}\" is not an index"))?;
            let len = items.len();
            let next = items
                .get_mut(i)
                .ok_or_else(|| format!("index {i} is out of range (length {len})"))?;
            set_path(next, rest, v)
        }
        _ => Err(format!("cannot go into \"{first}\": not an object or list")),
    }
}

fn get_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(root, |v, k| match v {
        Value::Object(m) => m.get(*k),
        Value::Array(a) => a.get(k.parse::<usize>().ok()?),
        _ => None,
    })
}

fn split_field(field: &str) -> Vec<&str> {
    field.split(['.', '/']).filter(|s| !s.is_empty()).collect()
}

/// Top-level field names a component accepts.
fn check_field(known: &[String], current: &Value, field: &str) -> Result<(), String> {
    if current.get(field).is_some() || known.iter().any(|k| k == field) {
        return Ok(());
    }
    let mut names: Vec<String> = known.to_vec();
    if let Some(m) = current.as_object() {
        names.extend(m.keys().cloned());
    }
    names.sort();
    names.dedup();
    Err(format!(
        "no field \"{field}\" (fields: {})",
        names.join(", ")
    ))
}

// ---- tools ---------------------------------------------------------------------------------------

fn list_hierarchy(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &*cx.s;
    let with_components = opt_bool(args, "components").unwrap_or(true);
    let max_depth = opt_u64(args, "max_depth").map(|d| d as usize);
    let dirty = crate::scene_files::dirty_now(s);
    let active = s.scene.active_scene();
    let scene_info = |id| {
        json!({
            "name": s.scene.scene_name(id),
            "path": s.scene.scene_path(id),
            "active": id == active,
            "unsaved_changes": dirty.contains(&id),
        })
    };
    if let Some(filter) = opt_str(args, "filter").filter(|f| !f.trim().is_empty()) {
        let f = filter.trim();
        let found: Vec<Value> = s
            .scene
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| matches(s, *e, f))
            .take(500)
            .map(|e| {
                let mut v = summary(s, e);
                v["scene"] = json!(s.scene.scene_name(s.scene.scene_of(e)));
                v
            })
            .collect();
        return Ok(Outcome::ok(
            json!({ "count": found.len(), "objects": found }),
        ));
    }
    fn node(s: &EditorState, e: Entity, depth: usize, max: Option<usize>, comps: bool) -> Value {
        let mut v = json!({
            "id": object_id(e),
            "name": s.scene.name(e),
        });
        if !s.scene.game_object(e).active {
            v["active"] = json!(false);
        }
        if comps {
            v["components"] = json!(s.scene.component_names(e));
        }
        let children = s.scene.children(e);
        if !children.is_empty() {
            if max.is_some_and(|m| depth >= m) {
                v["children_count"] = json!(children.len());
            } else {
                v["children"] = children
                    .into_iter()
                    .map(|c| node(s, c, depth + 1, max, comps))
                    .collect();
            }
        }
        v
    }
    let scenes: Vec<Value> = s
        .scene
        .loaded_scenes()
        .into_iter()
        .map(|id| {
            let mut v = scene_info(id);
            v["objects"] = s
                .scene
                .roots_in(id)
                .into_iter()
                .map(|e| node(s, e, 0, max_depth, with_components))
                .collect();
            v
        })
        .collect();
    Ok(Outcome::ok(json!({
        "prefab_mode": crate::prefab_mode::active(s),
        "play_mode": s.play.in_play_mode(),
        "scenes": scenes,
    })))
}

/// `filter` of list_hierarchy: "t:Kind" or a name substring.
fn matches(s: &EditorState, e: Entity, filter: &str) -> bool {
    match filter.strip_prefix("t:") {
        Some(kind) => {
            let kind = kind.trim();
            let lower = kind.to_lowercase();
            s.scene
                .component_names(e)
                .iter()
                .any(|n| n.to_lowercase() == lower)
                || script_slot(s, e, kind).is_some()
        }
        None => s
            .scene
            .name(e)
            .to_lowercase()
            .contains(&filter.to_lowercase()),
    }
}

fn get_object(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &*cx.s;
    let e = resolve(s, str_arg(args, "object")?)?;
    let go = GameObject::clone(&s.scene.game_object(e));
    let mut components = Vec::new();
    for name in s.scene.component_names(e) {
        if name == "Scripts" {
            if let Ok(scripts) = s.scene.world.get::<&Scripts>(e) {
                for r in &scripts.list {
                    components.push(json!({
                        "name": r.name(),
                        "script": r.path,
                        "enabled": r.enabled,
                        "fields": r.fields,
                    }));
                }
            }
            continue;
        }
        let Some(info) = registry::find(&name) else {
            continue;
        };
        let mut fields = (info.to_json)(&s.scene.world, e).unwrap_or(Value::Null);
        if name == "Transform" {
            let euler = s.scene.transform(e).euler();
            fields["euler"] = json!([euler.x, euler.y, euler.z]);
        }
        components.push(json!({ "name": name, "fields": fields }));
    }
    let children: Vec<Value> = s
        .scene
        .children(e)
        .into_iter()
        .map(|c| json!({ "id": object_id(c), "name": s.scene.name(c) }))
        .collect();
    Ok(Outcome::ok(json!({
        "id": object_id(e),
        "name": go.name,
        "path": object_path(s, e),
        "scene": s.scene.scene_name(s.scene.scene_of(e)),
        "active": go.active,
        "active_in_hierarchy": s.scene.active_in_hierarchy(e),
        "tag": go.tag,
        "layer": go.layer,
        "static": go.is_static,
        "parent": s.scene.parent(e).map(object_id),
        "prefab": s.scene.prefab_link(e),
        "children": children,
        "components": components,
    })))
}

fn create_object(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let kind = opt_str(args, "kind").unwrap_or("empty");
    let what = create_kind(kind).ok_or_else(|| format!("unknown kind \"{kind}\""))?;
    let parent = opt_str(args, "parent").map(|p| resolve(s, p)).transpose()?;
    let (position, rotation, scale) = (
        opt_vec3(args, "position")?,
        opt_vec3(args, "rotation")?,
        opt_vec3(args, "scale")?,
    );
    let extra: Vec<String> = args
        .get("components")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    // Check the extra components before anything changes.
    for c in &extra {
        if is_script_ref(c) {
            script_exists(s, c)?;
        } else {
            component_info(c)?;
        }
    }
    let before = s.selection.active();
    crate::commands::apply(s, Action::Create { what, parent });
    let e = s
        .selection
        .active()
        .filter(|e| Some(*e) != before && s.scene.contains(*e))
        .ok_or_else(|| last_log(s))?;
    // Still the same undo step: "Create …" was recorded before the object was made.
    if let Some(name) = opt_str(args, "name").filter(|n| !n.trim().is_empty()) {
        s.scene.game_object_mut(e).name = name.to_owned();
    }
    if position.is_some() || rotation.is_some() || scale.is_some() {
        let mut t = s.scene.transform(e);
        if let Some(p) = position {
            t.position = p;
        }
        if let Some(r) = rotation {
            t.set_euler(r);
        }
        if let Some(sc) = scale {
            t.scale = sc;
        }
        s.scene.set_transform(e, t);
    }
    for c in &extra {
        add_inner(s, e, c)?;
    }
    s.scene_dirty = true;
    Ok(Outcome::ok(summary(s, e)))
}

fn script_exists(s: &EditorState, rel: &str) -> Result<String, String> {
    let rel = assets_path(rel)?;
    if !s.project.root.join(&rel).is_file() {
        return Err(format!("no script at {rel}"));
    }
    Ok(rel)
}

/// Add a component or script without recording (the caller has).
fn add_inner(s: &mut EditorState, e: Entity, name: &str) -> Result<(), String> {
    if is_script_ref(name) {
        let rel = script_exists(s, name)?;
        let mut scripts = s
            .scene
            .world
            .get::<&Scripts>(e)
            .map(|x| (*x).clone())
            .unwrap_or_default();
        scripts.list.push(misty_engine::scene::ScriptRef {
            path: rel,
            ..Default::default()
        });
        s.scene.add(e, scripts);
        return Ok(());
    }
    let info = component_info(name)?;
    if !info.user_visible() || !info.removable {
        return Err(format!("{} can't be added", info.name));
    }
    if (info.has)(&s.scene.world, e) {
        return Err(format!("'{}' already has a {}", s.scene.name(e), info.name));
    }
    s.scene.add_by_name(e, info.name);
    crate::presets::apply_default(s, e, info.name);
    Ok(())
}

fn instantiate_prefab(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let rel = assets_path(str_arg(args, "path")?)?;
    if !s.project.root.join(&rel).is_file() {
        return Err(format!("no asset at {rel}"));
    }
    let parent = opt_str(args, "parent").map(|p| resolve(s, p)).transpose()?;
    let position = opt_vec3(args, "position")?;
    let e = if rel.ends_with(".prefab") {
        crate::prefabs::instantiate(s, &rel, parent)
    } else if misty_engine::models::is_model(&rel) {
        crate::model_import::instantiate(s, &rel, parent)
    } else {
        return Err(format!("{rel} is not a prefab or a model"));
    };
    let e = e.ok_or_else(|| last_log(s))?;
    if let Some(p) = position {
        s.scene.set_world_position(e, p);
    }
    Ok(Outcome::ok(summary(s, e)))
}

fn set_field(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let e = resolve(s, str_arg(args, "object")?)?;
    let component = str_arg(args, "component")?;
    let field = str_arg(args, "field")?;
    let value = args.get("value").cloned().unwrap_or(Value::Null);
    let path = split_field(field);
    let Some(top) = path.first().copied() else {
        return Err("\"field\" is empty".into());
    };
    let label = format!("Set {component} {field}");
    let new_value = if component.eq_ignore_ascii_case("GameObject") {
        let mut v = serde_json::to_value(GameObject::clone(&s.scene.game_object(e)))
            .map_err(|e| e.to_string())?;
        let known = vec!["static_flags".to_owned()];
        check_field(&known, &v, top)?;
        set_path(&mut v, &path, value)?;
        let go: GameObject =
            serde_json::from_value(v.clone()).map_err(|e| format!("{field}: {e}"))?;
        s.record(&label);
        *s.scene.game_object_mut(e) = go;
        get_path(&v, &path).cloned()
    } else if component.eq_ignore_ascii_case("Transform") {
        let mut t = s.scene.transform(e);
        if matches!(top, "euler" | "rotation_euler") {
            let euler: [f32; 3] = serde_json::from_value(value.clone())
                .map_err(|_| "\"euler\" must be [x, y, z] in degrees".to_owned())?;
            t.set_euler(glam::Vec3::from(euler));
        } else {
            let mut v = serde_json::to_value(t).map_err(|e| e.to_string())?;
            check_field(&[], &v, top)?;
            set_path(&mut v, &path, value.clone())?;
            t = serde_json::from_value::<Transform>(v).map_err(|e| format!("{field}: {e}"))?;
        }
        let after = serde_json::to_value(t).map_err(|e| e.to_string())?;
        s.record(&label);
        s.scene.set_transform(e, t);
        get_path(&after, &path).cloned().or(Some(value))
    } else if let Some(slot) = script_slot(s, e, component) {
        if path.len() != 1 {
            return Err("script fields are plain values: use the field name alone".into());
        }
        let sv: ScriptValue = serde_json::from_value(value.clone())
            .map_err(|_| "script fields take a number, a bool, a string or [x, y, z]".to_owned())?;
        s.record(&label);
        if let Ok(mut scripts) = s.scene.world.get::<&mut Scripts>(e)
            && let Some(r) = scripts.list.get_mut(slot)
        {
            r.fields.insert(top.to_owned(), sv);
        }
        Some(value)
    } else {
        let info = component_info(component)?;
        if !(info.has)(&s.scene.world, e) {
            return Err(format!("'{}' has no {}", s.scene.name(e), info.name));
        }
        let mut v = (info.to_json)(&s.scene.world, e).ok_or("could not read the component")?;
        check_field(&(info.fields)(), &v, top)?;
        set_path(&mut v, &path, value)?;
        // Try the new values on a scratch object first: a bad value changes nothing.
        let mut scratch = misty_engine::hecs::World::new();
        let tmp = scratch.spawn(());
        (info.from_json)(&mut scratch, tmp, v.clone()).map_err(|e| format!("{field}: {e}"))?;
        s.record(&label);
        (info.from_json)(&mut s.scene.world, e, v).map_err(|e| format!("{field}: {e}"))?;
        let after = (info.to_json)(&s.scene.world, e).unwrap_or(Value::Null);
        get_path(&after, &path).cloned()
    };
    s.scene_dirty = true;
    Ok(Outcome::ok(json!({
        "object": object_id(e),
        "component": component,
        "field": field,
        "value": new_value,
        "undo": label,
    })))
}

fn add_component(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let e = resolve(s, str_arg(args, "object")?)?;
    let name = str_arg(args, "component")?;
    // Check first, so a refused add records no undo step.
    if is_script_ref(name) {
        script_exists(s, name)?;
        if script_slot(s, e, name).is_some() {
            return Err(format!("'{}' already has {name}", s.scene.name(e)));
        }
        s.record("Add Script");
    } else {
        let info = component_info(name)?;
        if !info.user_visible() || !info.removable {
            return Err(format!("{} can't be added", info.name));
        }
        if (info.has)(&s.scene.world, e) {
            return Err(format!("'{}' already has a {}", s.scene.name(e), info.name));
        }
        s.record(&format!("Add {}", info.name));
    }
    add_inner(s, e, name)?;
    Ok(Outcome::ok(summary(s, e)))
}

fn remove_component(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let e = resolve(s, str_arg(args, "object")?)?;
    let name = str_arg(args, "component")?;
    if let Some(slot) = script_slot(s, e, name) {
        crate::commands::apply(s, Action::RemoveScript { entity: e, slot });
        return Ok(Outcome::ok(summary(s, e)));
    }
    if is_script_ref(name) {
        return Err(format!("'{}' has no script {name}", s.scene.name(e)));
    }
    let info = component_info(name)?;
    if !info.removable {
        return Err(format!("{} can't be removed", info.name));
    }
    if !(info.has)(&s.scene.world, e) {
        return Err(format!("'{}' has no {}", s.scene.name(e), info.name));
    }
    crate::commands::apply(
        s,
        Action::RemoveComponent {
            entities: vec![e],
            name: info.name.to_owned(),
        },
    );
    Ok(Outcome::ok(summary(s, e)))
}

fn set_parent(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let e = resolve(s, str_arg(args, "object")?)?;
    let parent = opt_str(args, "parent").map(|p| resolve(s, p)).transpose()?;
    if let Some(p) = parent
        && (p == e || s.scene.is_ancestor(e, p))
    {
        return Err("an object can't go inside itself or its children".into());
    }
    s.record(&format!("Set Parent of {}", s.scene.name(e)));
    s.scene.set_parent(e, parent, None, true);
    Ok(Outcome::ok(summary(s, e)))
}

fn delete_objects(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let list = resolve_many(s, &args["objects"])?;
    if list.is_empty() {
        return Err("no objects given".into());
    }
    // Children of listed objects go with their parent.
    let top: Vec<Entity> = list
        .iter()
        .copied()
        .filter(|e| !list.iter().any(|p| p != e && s.scene.is_ancestor(*p, *e)))
        .collect();
    let names: Vec<String> = top.iter().map(|e| s.scene.name(*e)).collect();
    let label = match names.as_slice() {
        [one] => format!("Delete {one}"),
        many => format!("Delete {} Objects", many.len()),
    };
    s.record(&label);
    for e in top {
        if s.scene.contains(e) {
            s.scene.despawn(e);
        }
    }
    s.selection.retain_existing(&s.scene);
    Ok(Outcome::ok(json!({ "deleted": names, "undo": label })))
}

fn select(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let list = resolve_many(s, &args["objects"])?;
    match list.as_slice() {
        [] => s.selection.clear(),
        [one] => s.select(*one),
        _ => {
            s.selection.set_many(list.clone());
            for e in &list {
                s.reveal(*e);
            }
        }
    }
    let selected: Vec<Value> = s.selection.list().iter().map(|e| summary(s, *e)).collect();
    Ok(Outcome::ok(json!({ "selection": selected })))
}

fn save(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    if s.play.in_play_mode() {
        return Err("scenes can't be saved in Play mode (editor.stop first)".into());
    }
    if crate::prefab_mode::active(s) {
        if args.get("path").is_some_and(|p| !p.is_null()) {
            return Err("in Prefab Mode scene.save saves the prefab; leave out \"path\"".into());
        }
        if !crate::prefab_mode::save(s) {
            return Err(last_log(s));
        }
        let open = crate::prefab_mode::open_path(s);
        return Ok(Outcome::ok(json!({ "saved": [open] })));
    }
    if let Some(path) = opt_str(args, "path") {
        let mut rel = assets_path(path)?;
        if !rel.ends_with(".scene") {
            rel.push_str(".scene");
        }
        let id = s.scene.active_scene();
        if !crate::scene_files::write(s, id, &rel) {
            return Err(last_log(s));
        }
        return Ok(Outcome::ok(json!({ "saved": [rel] })));
    }
    let mut list = crate::scene_files::dirty_now(s);
    if list.is_empty() {
        list.push(s.scene.active_scene());
    }
    if let Some(id) = list.iter().find(|id| s.scene.scene_path(**id).is_none()) {
        return Err(format!(
            "scene '{}' has no file yet: pass \"path\" (\"Assets/Scenes/{}.scene\")",
            s.scene.scene_name(*id),
            s.scene.scene_name(*id)
        ));
    }
    let mut saved = Vec::new();
    for id in list {
        if !crate::scene_files::save_scene(s, id) {
            return Err(last_log(s));
        }
        saved.extend(s.scene.scene_path(id));
    }
    Ok(Outcome::ok(json!({ "saved": saved })))
}

fn open(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let rel = assets_path(str_arg(args, "path")?)?;
    if !rel.ends_with(".scene") || !s.project.root.join(&rel).is_file() {
        return Err(format!("no scene file at {rel}"));
    }
    if s.play.in_play_mode() {
        return Err("stop Play mode first (editor.stop)".into());
    }
    if crate::prefab_mode::active(s) {
        return Err("the editor is in Prefab Mode; leave it first".into());
    }
    if opt_bool(args, "additive").unwrap_or(false) {
        crate::open_scenes::open_additive(s, &rel).ok_or_else(|| last_log(s))?;
        return Ok(Outcome::ok(json!({ "opened": rel, "additive": true })));
    }
    let dirty: Vec<String> = crate::scene_files::dirty_now(s)
        .into_iter()
        .map(|id| s.scene.scene_name(id))
        .collect();
    let then = move |cx: &mut Cx<'_>| {
        crate::panels::animation::preview::stop_preview(cx.s);
        crate::panels::timeline::preview::stop_preview(cx.s);
        if crate::scene_files::load(cx.s, &rel) {
            Outcome::ok(json!({ "opened": rel }))
        } else {
            Outcome::Done(super::ToolResult::error(last_log(cx.s)))
        }
    };
    if dirty.is_empty() {
        return Ok(then(cx));
    }
    if !opt_bool(args, "discard_changes").unwrap_or(false) {
        return Err(format!(
            "unsaved changes in {}: save them (scene.save) or pass \"discard_changes\": true",
            dirty.join(", ")
        ));
    }
    Ok(Outcome::Confirm {
        what: format!(
            "open {} and discard unsaved changes in {}",
            args["path"].as_str().unwrap_or_default(),
            dirty.join(", ")
        ),
        then: Box::new(then),
    })
}

fn component_types(_cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let filter = opt_str(args, "filter").unwrap_or("").to_lowercase();
    let list: Vec<Value> = registry::components()
        .iter()
        .filter(|c| c.user_visible() && c.removable)
        .filter(|c| {
            filter.is_empty()
                || c.name.to_lowercase().contains(&filter)
                || c.category.to_lowercase().contains(&filter)
        })
        .map(|c| json!({ "name": c.name, "category": c.category }))
        .collect();
    Ok(Outcome::ok(json!({ "components": list })))
}
