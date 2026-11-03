//! Scene files (`.scene`): pretty JSON, objects in Hierarchy order, parents by index.
//!
//! ```json
//! { "version": 1, "objects": [
//!     { "parent": null, "components": { "GameObject": {…}, "Transform": {…}, "Camera": {…} } },
//!     { "parent": 0,    "components": { … } } ] }
//! ```
//! Entity ids are not stored; links are rebuilt on load. Asset references stay as
//! project-relative paths.

use std::collections::BTreeMap;
use std::path::Path;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::{Node, Scene, SceneComponent, registry};

pub const EXTENSION: &str = "scene";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SceneFile {
    version: u32,
    objects: Vec<ObjectEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct ObjectEntry {
    /// Index of the parent in `objects` (always earlier in the list).
    pub parent: Option<usize>,
    /// Component name → its fields.
    pub components: BTreeMap<String, serde_json::Value>,
}

/// Problems found while loading that didn't stop the load.
#[derive(Debug, Default)]
pub struct LoadReport {
    pub warnings: Vec<String>,
}

/// Components store f32; write each number in its shortest f32 form (0.3, not 0.30000001192092896).
fn tidy_numbers(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Number(n) if n.is_f64() => {
            let short = (n.as_f64().unwrap_or(0.0) as f32).to_string();
            if let Ok(parsed) = short.parse::<f64>()
                && let Some(num) = serde_json::Number::from_f64(parsed)
            {
                *n = num;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(tidy_numbers),
        serde_json::Value::Object(map) => map.values_mut().for_each(tidy_numbers),
        _ => {}
    }
}

/// Pretty JSON with tidy numbers (shared by scenes and prefabs).
pub(super) fn write_json(objects: Vec<ObjectEntry>) -> String {
    let mut value = serde_json::to_value(SceneFile {
        version: VERSION,
        objects,
    })
    .expect("scene data serialises");
    tidy_numbers(&mut value);
    serde_json::to_string_pretty(&value).expect("scene data serialises")
}

/// Parse the object list and check the format version.
pub(super) fn read_json(text: &str, what: &str) -> Result<Vec<ObjectEntry>, String> {
    let file: SceneFile =
        serde_json::from_str(text).map_err(|e| format!("invalid {what} file: {e}"))?;
    if file.version > VERSION {
        return Err(format!(
            "{what} was saved by a newer version (format {}, this editor reads {VERSION})",
            file.version
        ));
    }
    Ok(file.objects)
}

impl Scene {
    /// Object entries for every scene object, or only `root` and its children.
    pub(super) fn entries(&self, root: Option<Entity>) -> Vec<ObjectEntry> {
        let order: Vec<Entity> = self
            .walk()
            .into_iter()
            .map(|(e, _)| e)
            .filter(|e| root.is_none_or(|r| self.is_ancestor(r, *e)))
            .collect();
        let index: BTreeMap<Entity, usize> =
            order.iter().enumerate().map(|(i, e)| (*e, i)).collect();
        let mut objects: Vec<ObjectEntry> = order
            .iter()
            .map(|e| ObjectEntry {
                parent: self.parent(*e).and_then(|p| index.get(&p).copied()),
                components: registry::components()
                    .iter()
                    .filter(|info| info.name != Node::NAME)
                    .filter_map(|info| {
                        (info.to_json)(&self.world, *e).map(|v| (info.name.to_owned(), v))
                    })
                    .collect(),
            })
            .collect();
        // Same numbers as a saved file, so entries can be compared.
        for o in &mut objects {
            o.components.values_mut().for_each(tidy_numbers);
        }
        objects
    }

    /// Create objects from entries; top-level ones go under `parent`. Returns them in order.
    pub(super) fn spawn_entries(
        &mut self,
        objects: Vec<ObjectEntry>,
        parent: Option<Entity>,
        report: &mut LoadReport,
    ) -> Vec<Entity> {
        let mut entities: Vec<Entity> = Vec::with_capacity(objects.len());
        for (i, obj) in objects.into_iter().enumerate() {
            let e = self.world.spawn((Node::default(),));
            for (comp, value) in obj.components {
                match registry::find(&comp) {
                    Some(info) if info.name != Node::NAME => {
                        if let Err(err) = (info.from_json)(&mut self.world, e, value) {
                            report.warnings.push(format!("object {i}: {comp}: {err}"));
                        }
                    }
                    _ => report
                        .warnings
                        .push(format!("object {i}: unknown component '{comp}' skipped")),
                }
            }
            // Every object needs a GameObject header and a Transform.
            for required in ["GameObject", "Transform"] {
                let info = registry::find(required).expect("built-in component");
                if !(info.has)(&self.world, e) {
                    (info.add_default)(&mut self.world, e);
                    report
                        .warnings
                        .push(format!("object {i}: missing {required}, added default"));
                }
            }
            let p = obj.parent.and_then(|p| entities.get(p).copied());
            if obj.parent.is_some() && p.is_none() {
                report.warnings.push(format!(
                    "object {i}: parent index out of order, placed at root"
                ));
            }
            self.attach(e, p.or(parent), None);
            entities.push(e);
        }
        entities
    }

    /// Serialise to the `.scene` JSON text.
    pub fn to_json(&self) -> String {
        write_json(self.entries(None))
    }

    /// Parse `.scene` JSON. Unknown components are skipped with a warning.
    pub fn from_json(name: &str, text: &str) -> Result<(Scene, LoadReport), String> {
        let objects = read_json(text, "scene")?;
        let mut scene = Scene::new(name);
        let mut report = LoadReport::default();
        scene.spawn_entries(objects, None, &mut report);
        Ok((scene, report))
    }

    /// Write the scene to disk.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Read a scene file; its name is the file name without extension.
    pub fn load(path: &Path) -> Result<(Scene, LoadReport), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        Scene::from_json(&name, &text)
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::scene::{
        BoxCollider, Light, MeshFilter, MeshRenderer, PrimitiveMesh, Rigidbody, ScriptRef,
        ScriptValue, Scripts, Transform,
    };

    fn sample() -> Scene {
        let mut s = Scene::default_3d("Level");
        let parent = s.spawn("Props", None);
        let cube = s.spawn("Cube", Some(parent));
        s.set_transform(
            cube,
            Transform::from_position(Vec3::new(1.0, 2.0, 3.0))
                .with_euler(Vec3::new(0.0, 360.0, 0.0)),
        );
        s.add(
            cube,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Cube),
            },
        );
        s.add(
            cube,
            MeshRenderer {
                material: Some("Assets/Materials/Red.mat".into()),
                ..Default::default()
            },
        );
        s.add(cube, BoxCollider::default());
        s.add(
            cube,
            Rigidbody {
                mass: 3.0,
                ..Default::default()
            },
        );
        s.add(
            cube,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Rotator.lua".into(),
                    enabled: false,
                    fields: [("speed".to_owned(), ScriptValue::Number(45.0))]
                        .into_iter()
                        .collect(),
                }],
            },
        );
        s.spawn("Second child", Some(parent));
        s.game_object_mut(parent).active = false;
        s
    }

    fn names(s: &Scene) -> Vec<(String, usize)> {
        s.walk().into_iter().map(|(e, d)| (s.name(e), d)).collect()
    }

    #[test]
    fn round_trip_keeps_hierarchy_components_and_values() {
        let original = sample();
        let (loaded, report) = Scene::from_json("Level", &original.to_json()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(names(&loaded), names(&original));

        let cube = loaded.find_by_name("Cube").unwrap();
        assert_eq!(
            loaded.component_names(cube),
            [
                "Transform",
                "Mesh Filter",
                "Mesh Renderer",
                "Box Collider",
                "Rigidbody",
                "Scripts"
            ]
        );
        let t = loaded.transform(cube);
        assert_eq!(t.position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            t.euler_hint.y, 360.0,
            "Euler hint survives (Inspector keeps showing 360)"
        );
        assert_eq!(loaded.world.get::<&Rigidbody>(cube).unwrap().mass, 3.0);
        let scripts = loaded.world.get::<&Scripts>(cube).unwrap();
        assert!(!scripts.list[0].enabled);
        assert_eq!(scripts.list[0].fields["speed"], ScriptValue::Number(45.0));
        assert!(
            !loaded
                .game_object(loaded.find_by_name("Props").unwrap())
                .active
        );
        assert!(
            loaded
                .world
                .get::<&Light>(loaded.find_by_name("Directional Light").unwrap())
                .is_ok()
        );
    }

    #[test]
    fn saving_twice_is_byte_identical() {
        let s = sample();
        let (loaded, _) = Scene::from_json("Level", &s.to_json()).unwrap();
        assert_eq!(
            s.to_json(),
            loaded.to_json(),
            "stable output keeps version-control diffs small"
        );
    }

    #[test]
    fn unknown_components_and_missing_parts_are_reported_not_fatal() {
        let text = r#"{ "version": 1, "objects": [
            { "parent": null, "components": { "GameObject": { "name": "A" }, "Teleporter": { "x": 1 } } },
            { "parent": 0, "components": { "GameObject": { "name": "B" }, "Transform": {
                "position": [0,1,0], "rotation": [0,0,0,1], "scale": [1,1,1], "euler_hint": [0,0,0] } } }
        ] }"#;
        let (s, report) = Scene::from_json("t", text).unwrap();
        assert_eq!(names(&s), [("A".to_owned(), 0), ("B".to_owned(), 1)]);
        assert!(report.warnings.iter().any(|w| w.contains("Teleporter")));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("missing Transform"))
        );
        assert!(Scene::from_json("t", "not json").is_err());
        let newer = Scene::from_json("t", r#"{"version": 99, "objects": []}"#);
        assert!(newer.err().is_some_and(|e| e.contains("newer")));
    }

    #[test]
    fn save_and_load_files() {
        let dir = std::env::temp_dir().join(format!("misty_scene_file_{}", std::process::id()));
        let path = dir.join("Scenes").join("Level.scene");
        sample().save(&path).unwrap();
        let (loaded, _) = Scene::load(&path).unwrap();
        assert_eq!(loaded.name, "Level");
        assert_eq!(loaded.len(), sample().len());
        std::fs::remove_dir_all(dir).ok();
    }
}
