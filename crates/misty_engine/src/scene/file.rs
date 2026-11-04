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

use super::render_settings::RenderSettings;
use super::{Node, Scene, SceneComponent, registry};
use crate::navigation::NavigationSettings;

pub const EXTENSION: &str = "scene";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SceneFile {
    version: u32,
    /// Lighting window settings (scenes only; read separately so a bad value only warns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lighting: Option<serde_json::Value>,
    /// Navigation settings and baked nav mesh (scenes only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    navigation: Option<serde_json::Value>,
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

/// GameObject reference fields: (component, list field, reference field in each item).
/// An empty list field means the reference is a field of the component itself.
const OBJECT_REFS: &[(&str, &str, &str)] = &[
    ("Button", "on_click", "target"),
    ("Fixed Joint", "", "connected_body"),
    ("Hinge Joint", "", "connected_body"),
    ("Spring Joint", "", "connected_body"),
    ("LOD Group", "lods", "renderers"),
];

/// Every object-reference field: the ones above plus the UI widgets' (see `ui_widgets`).
fn all_refs() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    OBJECT_REFS
        .iter()
        .chain(super::ui_widgets::WIDGET_REFS)
        .chain(super::ui_widgets::SELECTABLE_REFS)
}

/// Rewrite every object reference with `f` (entity bits ↔ object index); unmapped → null.
/// The middle entry names a list of items or a nested object holding the field.
fn remap_refs(
    components: &mut BTreeMap<String, serde_json::Value>,
    f: impl Fn(u64) -> Option<u64>,
) {
    let remap_one = |v: &mut serde_json::Value| {
        *v = v
            .as_u64()
            .and_then(&f)
            .map_or(serde_json::Value::Null, Into::into);
    };
    // A reference field may also hold a list of references.
    let remap = |v: &mut serde_json::Value| match v {
        serde_json::Value::Array(items) => items.iter_mut().for_each(remap_one),
        _ => remap_one(v),
    };
    for (comp, list, field) in all_refs() {
        let Some(c) = components.get_mut(*comp) else {
            continue;
        };
        if list.is_empty() {
            if let Some(v) = c.get_mut(*field) {
                remap(v);
            }
            continue;
        }
        match c.get_mut(*list) {
            Some(serde_json::Value::Array(items)) => {
                for item in items {
                    if let Some(v) = item.get_mut(*field) {
                        remap(v);
                    }
                }
            }
            Some(obj @ serde_json::Value::Object(_)) => {
                if let Some(v) = obj.get_mut(*field) {
                    remap(v);
                }
            }
            _ => {}
        }
    }
}

/// Point references of a copied object at the copies in `map` (others stay as they are).
pub(super) fn remap_copied_refs(
    world: &mut hecs::World,
    e: Entity,
    map: &std::collections::HashMap<Entity, Entity>,
) {
    let mut names: Vec<&str> = all_refs().map(|r| r.0).collect();
    names.dedup();
    for name in names {
        let Some(info) = registry::find(name) else {
            continue;
        };
        let Some(value) = (info.to_json)(world, e) else {
            continue;
        };
        let mut one = BTreeMap::from([(name.to_owned(), value)]);
        remap_refs(&mut one, |bits| {
            let old = Entity::from_bits(bits)?;
            Some(map.get(&old).map_or(bits, |n| n.to_bits().get()))
        });
        if let Some(v) = one.remove(name) {
            let _ = (info.from_json)(world, e, v);
        }
    }
}

/// Pretty JSON with tidy numbers (shared by scenes and prefabs).
pub(super) fn write_json(
    objects: Vec<ObjectEntry>,
    lighting: Option<&RenderSettings>,
    navigation: Option<&NavigationSettings>,
) -> String {
    let mut value = serde_json::to_value(SceneFile {
        version: VERSION,
        lighting: lighting.map(|l| serde_json::to_value(l).expect("settings serialise")),
        navigation: navigation.map(|n| serde_json::to_value(n).expect("settings serialise")),
        objects,
    })
    .expect("scene data serialises");
    tidy_numbers(&mut value);
    serde_json::to_string_pretty(&value).expect("scene data serialises")
}

/// Parse the object list and check the format version.
pub(super) fn read_json(text: &str, what: &str) -> Result<Vec<ObjectEntry>, String> {
    read_file(text, what).map(|f| f.objects)
}

fn read_file(text: &str, what: &str) -> Result<SceneFile, String> {
    let file: SceneFile =
        serde_json::from_str(text).map_err(|e| format!("invalid {what} file: {e}"))?;
    if file.version > VERSION {
        return Err(format!(
            "{what} was saved by a newer version (format {}, this editor reads {VERSION})",
            file.version
        ));
    }
    Ok(file)
}

/// Lighting settings from a scene file; missing = defaults (the look of older scenes).
fn read_lighting(value: Option<serde_json::Value>, report: &mut LoadReport) -> RenderSettings {
    let Some(v) = value else {
        return RenderSettings::default();
    };
    serde_json::from_value(v).unwrap_or_else(|e| {
        report
            .warnings
            .push(format!("lighting settings: {e}; using defaults"));
        RenderSettings::default()
    })
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
            // Object references become list indices (null when outside the saved objects).
            remap_refs(&mut o.components, |bits| {
                Entity::from_bits(bits).and_then(|e| index.get(&e).map(|i| *i as u64))
            });
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
        // Spawn first so object references (by index) can point forward.
        let all: Vec<Entity> = objects
            .iter()
            .map(|_| self.world.spawn((Node::default(),)))
            .collect();
        let mut entities: Vec<Entity> = Vec::with_capacity(objects.len());
        for (i, mut obj) in objects.into_iter().enumerate() {
            let e = all[i];
            remap_refs(&mut obj.components, |index| {
                all.get(index as usize).map(|e| e.to_bits().get())
            });
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
        // Default settings are left out, so older scenes save byte-identical.
        let lighting = Some(&self.render_settings).filter(|r| **r != RenderSettings::default());
        let navigation = Some(&self.navigation).filter(|n| **n != NavigationSettings::default());
        write_json(self.entries(None), lighting, navigation)
    }

    /// Parse `.scene` JSON. Unknown components are skipped with a warning.
    pub fn from_json(name: &str, text: &str) -> Result<(Scene, LoadReport), String> {
        let file = read_file(text, "scene")?;
        let mut scene = Scene::new(name);
        let mut report = LoadReport::default();
        scene.render_settings = read_lighting(file.lighting, &mut report);
        if let Some(v) = file.navigation {
            scene.navigation = serde_json::from_value(v).unwrap_or_else(|e| {
                report
                    .warnings
                    .push(format!("navigation settings: {e}; using defaults"));
                NavigationSettings::default()
            });
        }
        scene.spawn_entries(file.objects, None, &mut report);
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
                mesh: Some(PrimitiveMesh::Cube.into()),
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
    fn audio_components_round_trip_and_fill_defaults() {
        let mut s = Scene::default_3d("Level");
        let e = s.spawn("Speaker", None);
        let source = crate::scene::AudioSource {
            clip: Some("Assets/Audio/Beep.wav".into()),
            looping: true,
            volume: 0.5,
            spatial_blend: 1.0,
            rolloff: crate::scene::AudioRolloff::Linear,
            max_distance: 40.0,
            ..Default::default()
        };
        s.add(e, source.clone());
        let json = s.to_json();
        assert!(json.contains("\"loop\": true"), "{json}");
        let (loaded, report) = Scene::from_json("Level", &json).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let e2 = loaded.find_by_name("Speaker").unwrap();
        assert_eq!(
            *loaded.world.get::<&crate::scene::AudioSource>(e2).unwrap(),
            source
        );
        let cam = loaded.find_by_name("Main Camera").unwrap();
        assert!(
            loaded
                .world
                .get::<&crate::scene::AudioListener>(cam)
                .is_ok()
        );

        // Files missing a field (or with unknown ones) get the default for it.
        let partial = json.replace("\"volume\": 0.5", "\"old_field\": 0.5");
        assert_ne!(partial, json);
        let (old, _) = Scene::from_json("Level", &partial).unwrap();
        let e3 = old.find_by_name("Speaker").unwrap();
        assert_eq!(
            old.world
                .get::<&crate::scene::AudioSource>(e3)
                .unwrap()
                .volume,
            1.0
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

    /// Scenes saved before imported meshes stored the primitive name; both forms load and save.
    #[test]
    fn mesh_filter_reads_old_primitive_names_and_asset_paths() {
        let text = r#"{ "version": 1, "objects": [
            { "parent": null, "components": { "GameObject": { "name": "Old" },
              "Transform": {}, "Mesh Filter": { "mesh": "Sphere" } } },
            { "parent": null, "components": { "GameObject": { "name": "Ship" },
              "Transform": {}, "Mesh Filter": { "mesh": "Assets/Models/Ship.glb#2" } } },
            { "parent": null, "components": { "GameObject": { "name": "None" },
              "Transform": {}, "Mesh Filter": { "mesh": null } } }
        ] }"#;
        let (s, report) = Scene::from_json("t", text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let mesh = |name| {
            let e = s.find_by_name(name).unwrap();
            s.world.get::<&MeshFilter>(e).unwrap().mesh.clone()
        };
        assert_eq!(mesh("Old"), Some(PrimitiveMesh::Sphere.into()));
        let ship = mesh("Ship").unwrap();
        assert_eq!(ship.split_asset(), Some(("Assets/Models/Ship.glb", 2)));
        assert_eq!(mesh("None"), None);
        // Written back in the same shapes.
        let json = s.to_json();
        assert!(json.contains(r#""mesh": "Sphere""#), "{json}");
        assert!(json.contains(r#""mesh": "Assets/Models/Ship.glb#2""#));
    }

    #[test]
    fn lighting_settings_round_trip_and_old_files_get_defaults() {
        use crate::scene::render_settings::{AmbientSource, FogMode, SkyboxKind};
        let mut s = sample();
        let rs = &mut s.render_settings;
        rs.skybox.kind = SkyboxKind::SolidColor;
        rs.skybox.sky_tint = [0.8, 0.4, 0.2];
        rs.environment_lighting.source = AmbientSource::Gradient;
        rs.environment_lighting.intensity = 1.5;
        rs.reflection_intensity = 0.25;
        rs.fog.enabled = true;
        rs.fog.mode = FogMode::Linear;
        rs.fog.end = 42.0;
        let json = s.to_json();
        assert!(json.contains("\"lighting\""), "{json}");
        let (loaded, report) = Scene::from_json("Level", &json).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(loaded.render_settings, s.render_settings);

        // Scenes saved before the Lighting window have no "lighting" block.
        let old = r#"{ "version": 1, "objects": [] }"#;
        let (old, report) = Scene::from_json("Old", old).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(old.render_settings, RenderSettings::default());
        assert!(!old.render_settings.fog.enabled);
        assert_eq!(old.render_settings.reflection_intensity, 1.0);

        // Partial or broken blocks keep the rest of the scene.
        let partial =
            r#"{ "version": 1, "lighting": { "fog": { "enabled": true } }, "objects": [] }"#;
        let (p, _) = Scene::from_json("P", partial).unwrap();
        assert!(p.render_settings.fog.enabled);
        assert_eq!(p.render_settings.fog.density, 0.01);
        let broken = r#"{ "version": 1, "lighting": { "fog": 5 }, "objects": [] }"#;
        let (b, report) = Scene::from_json("B", broken).unwrap();
        assert_eq!(b.render_settings, RenderSettings::default());
        assert!(report.warnings.iter().any(|w| w.contains("lighting")));
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
