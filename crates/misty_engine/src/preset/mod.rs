//! Presets (`.preset`, JSON): the saved field values of one component type or asset kind, applied
//! to others in one go. Values are stored exactly as the scene serializer writes the component
//! (fields left out are at their defaults); `excluded` fields are left alone when applying.

pub mod fields;

use std::path::Path;

use hecs::{Entity, World};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::assets::Material;
use crate::scene::registry;
use crate::sprites::TextureImporter;

/// Where "Save current to…" puts new presets.
pub const FOLDER: &str = "Assets/Presets";
/// Asset kinds presets can target besides components.
pub const MATERIAL: &str = "Material";
pub const TEXTURE_IMPORTER: &str = "Texture Importer";

pub fn is_preset(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".preset")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    /// Component name ("Light") or asset kind ([`MATERIAL`], [`TEXTURE_IMPORTER`]).
    pub target: String,
    /// Field values as the serializer writes them; fields left out use their defaults.
    pub values: Map<String, Value>,
    /// Fields that applying the preset leaves alone (a partial preset).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
}

impl Preset {
    /// A preset of `target` holding `value` (a serialised struct).
    pub fn new(target: &str, value: Value) -> Self {
        Self {
            target: target.to_owned(),
            values: match value {
                Value::Object(m) => m,
                _ => Map::new(),
            },
            excluded: Vec::new(),
        }
    }

    /// The values of component `name` on `e`.
    pub fn capture(world: &World, e: Entity, name: &str) -> Option<Self> {
        let info = registry::find(name)?;
        (info.to_json)(world, e).map(|v| Self::new(name, v))
    }

    /// A preset of an asset kind from its settings struct.
    pub fn of_value<T: Serialize>(target: &str, value: &T) -> Self {
        Self::new(target, serde_json::to_value(value).unwrap_or(Value::Null))
    }

    pub fn includes(&self, field: &str) -> bool {
        !self.excluded.iter().any(|f| f == field)
    }

    pub fn set_included(&mut self, field: &str, on: bool) {
        self.excluded.retain(|f| f != field);
        if !on {
            self.excluded.push(field.to_owned());
        }
    }

    /// Every field of the target type (declaration order), then any unknown stored ones.
    pub fn fields(&self) -> Vec<String> {
        let mut all = fields_of(&self.target);
        for k in self.values.keys() {
            if !all.contains(k) {
                all.push(k.clone());
            }
        }
        all
    }

    /// `current` with the preset applied: included fields take the preset's value (or their
    /// default when the preset leaves them out); excluded fields and `keep` stay as they are.
    pub fn applied_to(&self, current: &Value, keep: &[&str]) -> Value {
        let mut out = match current {
            Value::Object(m) => m.clone(),
            _ => Map::new(),
        };
        for field in self.fields() {
            if !self.includes(&field) || keep.contains(&field.as_str()) {
                continue;
            }
            match self.values.get(&field) {
                Some(v) => out.insert(field, v.clone()),
                None => out.remove(&field),
            };
        }
        Value::Object(out)
    }

    /// Apply to component `self.target` on `e` (which must have it).
    pub fn apply(&self, world: &mut World, e: Entity, keep: &[&str]) -> Result<(), String> {
        let info = registry::find(&self.target)
            .ok_or_else(|| format!("Unknown component '{}'", self.target))?;
        let current = (info.to_json)(world, e)
            .ok_or_else(|| format!("The object has no {}", self.target))?;
        (info.from_json)(world, e, self.applied_to(&current, keep))
    }

    /// Apply to an asset settings struct (material, import settings).
    pub fn apply_value<T: Serialize + DeserializeOwned>(&self, current: &T) -> Result<T, String> {
        let now = serde_json::to_value(current).map_err(|e| e.to_string())?;
        serde_json::from_value(self.applied_to(&now, &[])).map_err(|e| e.to_string())
    }

    /// The stored values are valid for the target type.
    pub fn check(&self) -> Result<(), String> {
        let v = Value::Object(self.values.clone());
        match self.target.as_str() {
            MATERIAL => serde_json::from_value::<Material>(v).map(drop),
            TEXTURE_IMPORTER => serde_json::from_value::<TextureImporter>(v).map(drop),
            name => {
                let info =
                    registry::find(name).ok_or_else(|| format!("Unknown component '{name}'"))?;
                let mut w = World::new();
                let e = w.spawn(());
                return (info.from_json)(&mut w, e, v);
            }
        }
        .map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = crate::vfs::read_to_string(path).map_err(|e| e.to_string())?;
        let p: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if p.target.is_empty() {
            return Err("not a preset (no target)".into());
        }
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }
}

/// Field names of a preset target type.
pub fn fields_of(target: &str) -> Vec<String> {
    match target {
        MATERIAL => fields::names(&Material::default()),
        TEXTURE_IMPORTER => fields::names(&TextureImporter::default()),
        name => registry::find(name)
            .map(|i| (i.fields)())
            .unwrap_or_default(),
    }
}

/// Fields a default preset never changes: the choice the Create menu item made
/// (Create ▸ Directional Light stays directional with a point-light default preset).
pub fn identity_fields(target: &str) -> &'static [&'static str] {
    match target {
        "Light" | "Water Surface" => &["kind"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Light, LightType, SphereCollider};

    fn world_with<T: hecs::Component>(c: T) -> (World, Entity) {
        let mut w = World::new();
        let e = w.spawn((c,));
        (w, e)
    }

    #[test]
    fn save_load_apply_round_trip() {
        let mut lamp = Light {
            intensity: 3.5,
            color: [1.0, 0.7, 0.4],
            ..Default::default()
        };
        lamp.halo.enabled = true;
        let (w, src) = world_with(lamp.clone());
        let preset = Preset::capture(&w, src, "Light").unwrap();
        let path = std::env::temp_dir().join("misty_preset_round_trip/Lamp.preset");
        preset.save(&path).unwrap();
        let loaded = Preset::load(&path).unwrap();
        assert_eq!(loaded, preset);
        loaded.check().unwrap();

        let (mut w, dst) = world_with(Light::directional());
        loaded.apply(&mut w, dst, &[]).unwrap();
        let got = w.get::<&Light>(dst).unwrap();
        assert_eq!(got.kind, LightType::Point);
        assert_eq!(got.intensity, 3.5);
        assert!(got.halo.enabled);
    }

    #[test]
    fn fields_left_at_default_reset_the_target() {
        // The preset's halo is off (left out of its values): applying turns the target's off.
        let (w, src) = world_with(Light::default());
        let preset = Preset::capture(&w, src, "Light").unwrap();
        assert!(!preset.values.contains_key("halo"));
        let mut lit = Light::default();
        lit.halo.enabled = true;
        let (mut w, dst) = world_with(lit);
        preset.apply(&mut w, dst, &[]).unwrap();
        assert!(!w.get::<&Light>(dst).unwrap().halo.enabled);
    }

    #[test]
    fn excluded_and_kept_fields_stay() {
        let bouncy = SphereCollider {
            radius: 0.2,
            surface: crate::scene::PhysicsSurface {
                friction: 0.1,
                bounciness: 0.9,
            },
            ..Default::default()
        };
        let (w, src) = world_with(bouncy);
        let mut preset = Preset::capture(&w, src, "Sphere Collider").unwrap();
        preset.set_included("radius", false);
        assert!(!preset.includes("radius") && preset.includes("surface"));
        let (mut w, dst) = world_with(SphereCollider {
            radius: 3.0,
            ..Default::default()
        });
        preset.apply(&mut w, dst, &[]).unwrap();
        let got = w.get::<&SphereCollider>(dst).unwrap();
        assert_eq!(got.radius, 3.0);
        assert_eq!(got.surface.bounciness, 0.9);

        // `keep` protects identity fields such as a Light's type.
        let (w, src) = world_with(Light::default());
        let point = Preset::capture(&w, src, "Light").unwrap();
        let (mut w, dst) = world_with(Light::directional());
        point.apply(&mut w, dst, identity_fields("Light")).unwrap();
        assert_eq!(w.get::<&Light>(dst).unwrap().kind, LightType::Directional);
    }

    #[test]
    fn asset_presets_apply_to_settings_structs() {
        let mut m = Material::default();
        m.metallic = 0.9;
        let mut preset = Preset::of_value(MATERIAL, &m);
        preset.set_included("color", false);
        let target = Material {
            color: [1.0, 0.0, 0.0, 1.0],
            ..Default::default()
        };
        let out = preset.apply_value(&target).unwrap();
        assert_eq!(out.metallic, 0.9);
        assert_eq!(out.color, [1.0, 0.0, 0.0, 1.0]);
        assert!(fields_of(TEXTURE_IMPORTER).contains(&"pixels_per_unit".to_owned()));
        preset.values.insert("metallic".into(), Value::String("x".into()));
        assert!(preset.check().is_err());
    }
}
