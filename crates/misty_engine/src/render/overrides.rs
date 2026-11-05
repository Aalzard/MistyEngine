//! Per-object material overrides: a Mesh Renderer can change how one of its materials looks on
//! this object only (UV tiling/offset/rotation, colour tint, smoothness/metallic/emission
//! multipliers) without a new material asset.
//!
//! Only the overridden values are stored. They are applied to the draw's own values (the
//! `draws[]` entry), never to the material: the material's textures and shader variant stay
//! the same, so objects sharing a material still draw in one instanced call.

use serde::{Deserialize, Serialize};

use super::MaterialParams;

/// Overrides of one Materials element; `None` = use the material's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialOverrides {
    /// Replaces the material's tiling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiling: Option<[f32; 2]>,
    /// Replaces the material's offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 2]>,
    /// UV rotation in degrees about the texture centre (applied before tiling and offset).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    /// sRGB colour + alpha multiplied with the material's colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 4]>,
    /// Multiplies the material's smoothness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoothness: Option<f32>,
    /// Multiplies the material's metallic value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    /// Multiplies the material's emission intensity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission: Option<f32>,
    /// Replaces the albedo texture (a Video Player's frames; a runtime texture name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_texture: Option<String>,
    /// Replaces the emission map (a Video Player's frames on a glowing screen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission_texture: Option<String>,
}

/// One override property's value (scripts and the Inspector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverrideValue {
    Vec2([f32; 2]),
    Color([f32; 4]),
    Float(f32),
}

impl MaterialOverrides {
    /// Property names, as scripts use them.
    pub const NAMES: [&str; 7] = [
        "tiling",
        "offset",
        "rotation",
        "color",
        "smoothness",
        "metallic",
        "emission",
    ];

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Change a draw's values: tiling/offset/rotation replace, the rest multiply.
    pub fn apply(&self, p: &mut MaterialParams) {
        if let Some(t) = self.tiling {
            p.tiling = t;
        }
        if let Some(o) = self.offset {
            p.offset = o;
        }
        if let Some(r) = self.rotation {
            p.uv_rotation = r;
        }
        if let Some(c) = self.color {
            p.color = std::array::from_fn(|i| p.color[i] * c[i]);
        }
        if let Some(s) = self.smoothness {
            p.smoothness = (p.smoothness * s).clamp(0.0, 1.0);
        }
        if let Some(m) = self.metallic {
            p.metallic = (p.metallic * m).clamp(0.0, 1.0);
        }
        if let Some(e) = self.emission {
            p.emission_intensity *= e.max(0.0);
        }
        if let Some(t) = &self.main_texture {
            p.albedo_map = Some(t.clone());
        }
        if let Some(t) = &self.emission_texture {
            p.emission_map = Some(t.clone());
        }
    }

    /// The value of property `name` when it is overridden.
    pub fn get(&self, name: &str) -> Option<OverrideValue> {
        use OverrideValue::*;
        match name {
            "tiling" => self.tiling.map(Vec2),
            "offset" => self.offset.map(Vec2),
            "rotation" => self.rotation.map(Float),
            "color" => self.color.map(Color),
            "smoothness" => self.smoothness.map(Float),
            "metallic" => self.metallic.map(Float),
            "emission" => self.emission.map(Float),
            _ => None,
        }
    }

    /// Override property `name`; false when the name is unknown or the value has the wrong kind.
    pub fn set(&mut self, name: &str, value: OverrideValue) -> bool {
        use OverrideValue::*;
        match (name, value) {
            ("tiling", Vec2(v)) => self.tiling = Some(v),
            ("offset", Vec2(v)) => self.offset = Some(v),
            ("rotation", Float(v)) => self.rotation = Some(v),
            ("color", Color(v)) => self.color = Some(v),
            ("smoothness", Float(v)) => self.smoothness = Some(v),
            ("metallic", Float(v)) => self.metallic = Some(v),
            ("emission", Float(v)) => self.emission = Some(v),
            _ => return false,
        }
        true
    }

    /// Stop overriding `name` (false when the name is unknown).
    pub fn clear(&mut self, name: &str) -> bool {
        match name {
            "tiling" => self.tiling = None,
            "offset" => self.offset = None,
            "rotation" => self.rotation = None,
            "color" => self.color = None,
            "smoothness" => self.smoothness = None,
            "metallic" => self.metallic = None,
            "emission" => self.emission = None,
            _ => return false,
        }
        true
    }

    /// The value shown when an override is first switched on: the material's own value
    /// (tiling/offset) or "no change" (rotation 0, white tint, multipliers 1).
    pub fn neutral(name: &str, material: &MaterialParams) -> Option<OverrideValue> {
        use OverrideValue::*;
        Some(match name {
            "tiling" => Vec2(material.tiling),
            "offset" => Vec2(material.offset),
            "rotation" => Float(0.0),
            "color" => Color([1.0; 4]),
            "smoothness" | "metallic" | "emission" => Float(1.0),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_overridden_values_change_and_are_saved() {
        let mut p = MaterialParams {
            color: [0.5, 0.8, 1.0, 1.0],
            smoothness: 0.6,
            metallic: 0.5,
            emission_intensity: 2.0,
            ..Default::default()
        };
        let none = MaterialOverrides::default();
        let before = p.clone();
        none.apply(&mut p);
        assert_eq!(p, before);
        assert_eq!(serde_json::to_string(&none).unwrap(), "{}");

        let mut o = MaterialOverrides::default();
        assert!(o.set("tiling", OverrideValue::Vec2([2.0, 3.0])));
        assert!(o.set("color", OverrideValue::Color([0.5, 1.0, 1.0, 0.5])));
        assert!(o.set("smoothness", OverrideValue::Float(0.5)));
        assert!(o.set("emission", OverrideValue::Float(3.0)));
        assert!(o.set("rotation", OverrideValue::Float(90.0)));
        assert!(!o.set("color", OverrideValue::Float(1.0)), "wrong kind");
        assert!(!o.set("nope", OverrideValue::Float(1.0)));
        o.apply(&mut p);
        assert_eq!(p.tiling, [2.0, 3.0]);
        assert_eq!(p.offset, [0.0, 0.0]);
        assert_eq!(p.color, [0.25, 0.8, 1.0, 0.5]);
        assert_eq!(
            (p.smoothness, p.metallic, p.emission_intensity),
            (0.3, 0.5, 6.0)
        );
        assert_eq!(p.uv_rotation, 90.0);
        // The textures (and so the batch key) never change.
        assert_eq!(p.textures(), before.textures());
        assert_eq!(p.pipeline_key(), before.pipeline_key());

        let json = serde_json::to_string(&o).unwrap();
        assert!(
            !json.contains("offset") && json.contains("tiling"),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<MaterialOverrides>(&json).unwrap(), o);
        assert_eq!(o.get("smoothness"), Some(OverrideValue::Float(0.5)));
        assert!(o.clear("smoothness") && o.get("smoothness").is_none());
        for n in MaterialOverrides::NAMES {
            o.clear(n);
        }
        assert!(o.is_empty());
    }
}
