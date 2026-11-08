//! glTF material extensions mapped onto the Standard shader's optional features
//! (Standard Lit v2, see [`crate::material_features`]):
//!
//! - `KHR_materials_clearcoat`: factor, roughness and normal texture → Clear Coat.
//! - `KHR_materials_anisotropy`: strength and rotation → Anisotropy (along the tangent).
//! - `KHR_materials_specular`: specular and specular colour factors → Specular.
//! - `KHR_materials_diffuse_transmission`: factor and colour → Translucency.
//! - `KHR_materials_transmission` (with `KHR_materials_volume`'s attenuation colour): glass,
//!   so the Transparent rendering mode with alpha = 1 − transmission, tinted.
//! - `KHR_texture_transform` on the base colour texture: tiling, offset and UV rotation.
//!
//! Their textures other than the clear coat normal (clear coat and roughness masks, the
//! anisotropy direction map, transmission and thickness maps) are not imported.

use serde_json::Value;

use super::{ExtraMap, TextureSource};
use crate::assets::Material;
use crate::render::RenderingMode;

fn to_srgb(c: f32) -> f32 {
    c.max(0.0).powf(1.0 / 2.2)
}

fn number(v: &Value, key: &str, default: f32) -> f32 {
    v.get(key)
        .and_then(Value::as_f64)
        .map_or(default, |n| n as f32)
}

fn color3(v: &Value, key: &str) -> Option<[f32; 3]> {
    let a = v.get(key)?.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).map(|n| n as f32);
    Some([c(0)?, c(1)?, c(2)?])
}

/// `{ "index": n, "scale": s }` of a texture reference: (texture index, scale).
fn texture_ref(v: &Value, key: &str) -> Option<(usize, f32)> {
    let t = v.get(key)?;
    let index = t.get("index")?.as_u64()? as usize;
    Some((index, number(t, "scale", 1.0)))
}

/// Apply a material's extensions (`extensions` = its extension objects by name) to
/// `material`; textures they use are resolved through `texture` (glTF texture index → image).
pub fn apply_extensions(
    extensions: &serde_json::Map<String, Value>,
    material: &mut Material,
    extra: &mut Vec<(ExtraMap, TextureSource)>,
    texture: &dyn Fn(usize) -> Option<TextureSource>,
) {
    let f = &mut material.features;
    if let Some(c) = extensions.get("KHR_materials_clearcoat") {
        let strength = number(c, "clearcoatFactor", 0.0);
        if strength > 0.0 {
            f.clear_coat.enabled = true;
            f.clear_coat.strength = strength.min(1.0);
            f.clear_coat.smoothness =
                1.0 - number(c, "clearcoatRoughnessFactor", 0.0).clamp(0.0, 1.0);
            if let Some((index, scale)) = texture_ref(c, "clearcoatNormalTexture")
                && let Some(source) = texture(index)
            {
                f.clear_coat.normal_scale = scale;
                extra.push((ExtraMap::ClearCoatNormal, source));
            }
        }
    }
    if let Some(a) = extensions.get("KHR_materials_anisotropy") {
        let strength = number(a, "anisotropyStrength", 0.0);
        if strength > 0.0 {
            f.anisotropy.enabled = true;
            f.anisotropy.strength = strength.min(1.0);
            // Counter-clockwise in tangent space (bitangent up the image); the material's
            // angle turns in the uv plane (v down the image).
            f.anisotropy.rotation = -number(a, "anisotropyRotation", 0.0).to_degrees();
        }
    }
    if let Some(s) = extensions.get("KHR_materials_specular") {
        f.specular = number(s, "specularFactor", 1.0).max(0.0);
        if let Some(c) = color3(s, "specularColorFactor") {
            f.specular_color = c.map(to_srgb);
        }
    }
    if let Some(d) = extensions.get("KHR_materials_diffuse_transmission") {
        let strength = number(d, "diffuseTransmissionFactor", 0.0);
        if strength > 0.0 {
            f.translucency.enabled = true;
            f.translucency.strength = strength.min(1.0);
            if let Some(c) = color3(d, "diffuseTransmissionColorFactor") {
                f.translucency.color = c.map(to_srgb);
            }
        }
    }
    if let Some(t) = extensions.get("KHR_materials_transmission") {
        apply_transmission(number(t, "transmissionFactor", 0.0), extensions, material);
    }
}

/// `KHR_materials_transmission`: see-through glass as the Transparent mode (reflections
/// stay, the diffuse fades), tinted by `KHR_materials_volume`'s attenuation colour.
pub fn apply_transmission(
    factor: f32,
    extensions: &serde_json::Map<String, Value>,
    material: &mut Material,
) {
    if factor <= 0.0 {
        return;
    }
    material.rendering_mode = RenderingMode::Transparent;
    material.color[3] *= (1.0 - factor.min(1.0)).max(0.05);
    if let Some(v) = extensions.get("KHR_materials_volume")
        && let Some(c) = color3(v, "attenuationColor")
    {
        for (k, tint) in c.iter().enumerate() {
            material.color[k] *= to_srgb(*tint);
        }
    }
}

/// `KHR_texture_transform` of the base colour texture: offset, rotation (radians,
/// counter-clockwise in uv space) and scale, as the material's tiling, offset and UV
/// rotation (which turns about the texture centre, then tiles).
pub fn apply_texture_transform(offset: [f32; 2], rotation: f32, scale: [f32; 2], m: &mut Material) {
    m.tiling = scale;
    let (s, c) = rotation.sin_cos();
    // glTF: uv' = R·(S·uv) + o with R = [c s; -s c]. The material: uv' = t·(c0 + R'·(uv − c0))
    // + offset with R' turning by −rotation, so offset = o − t·(c0 − R'·c0), c0 = (0.5, 0.5).
    let r = |p: [f32; 2]| [c * p[0] + s * p[1], -s * p[0] + c * p[1]];
    let rc = r([0.5, 0.5]);
    m.offset = [
        offset[0] - scale[0] * (0.5 - rc[0]),
        offset[1] - scale[1] * (0.5 - rc[1]),
    ];
    m.uv_rotation = -rotation.to_degrees();
    if m.uv_rotation == 0.0 {
        // No negative zero in the file.
        m.uv_rotation = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material_features::AnisotropyDirection;

    fn map(json: &str) -> serde_json::Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn clearcoat_anisotropy_specular_and_diffuse_transmission() {
        let ext = map(r#"{
            "KHR_materials_clearcoat": { "clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.1,
                "clearcoatNormalTexture": { "index": 2, "scale": 0.5 } },
            "KHR_materials_anisotropy": { "anisotropyStrength": 0.6, "anisotropyRotation": 1.5707964 },
            "KHR_materials_specular": { "specularFactor": 0.5, "specularColorFactor": [1.0, 0.5, 0.25] },
            "KHR_materials_diffuse_transmission": { "diffuseTransmissionFactor": 0.4,
                "diffuseTransmissionColorFactor": [0.2, 0.8, 0.1] }
        }"#);
        let mut m = Material::default();
        let mut extra = Vec::new();
        let seen = std::cell::Cell::new(None);
        apply_extensions(&ext, &mut m, &mut extra, &|i| {
            seen.set(Some(i));
            Some(TextureSource::Embedded(vec![1, 2, 3]))
        });
        let f = &m.features;
        assert!(f.clear_coat.enabled && f.clear_coat.strength == 1.0);
        assert!((f.clear_coat.smoothness - 0.9).abs() < 1e-6);
        assert_eq!(f.clear_coat.normal_scale, 0.5);
        assert_eq!(seen.get(), Some(2));
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].0, ExtraMap::ClearCoatNormal);
        assert!(f.anisotropy.enabled && f.anisotropy.strength == 0.6);
        assert_eq!(f.anisotropy.direction, AnisotropyDirection::Horizontal);
        assert!((f.anisotropy.rotation + 90.0).abs() < 1e-3);
        assert_eq!(f.specular, 0.5);
        assert!((f.specular_color[1] - 0.5f32.powf(1.0 / 2.2)).abs() < 1e-6);
        assert!(f.translucency.enabled && f.translucency.strength == 0.4);
        assert_eq!(m.rendering_mode, RenderingMode::Opaque);
        // Every feature on: the renderer gets one variant with all their bits.
        use crate::render::material_v2::feature;
        let bits = m.params().features();
        assert_eq!(
            bits,
            feature::CLEARCOAT | feature::ANISOTROPY | feature::SPECULAR | feature::TRANSLUCENCY
        );
    }

    /// A whole file: a triangle whose material has a clear coat (with its normal texture),
    /// anisotropy and a texture transform the file requires.
    #[test]
    fn gltf_file_with_material_extensions_imports_them() {
        let dir = std::env::temp_dir().join(format!("misty_gltf_ext_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut bin = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in p {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        bin.extend_from_slice(&[0, 0]);
        std::fs::write(dir.join("tri.bin"), &bin).unwrap();
        let json = r#"{
  "asset": { "version": "2.0" },
  "extensionsUsed": ["KHR_materials_clearcoat", "KHR_texture_transform", "KHR_materials_anisotropy"],
  "extensionsRequired": ["KHR_texture_transform"],
  "buffers": [{ "uri": "tri.bin", "byteLength": 44 }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 6 }
  ],
  "accessors": [
    { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] },
    { "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }
  ],
  "images": [{ "uri": "albedo.png" }, { "uri": "coat.png" }],
  "textures": [{ "source": 0 }, { "source": 1 }],
  "materials": [{
    "name": "Paint",
    "pbrMetallicRoughness": { "baseColorTexture": { "index": 0,
      "extensions": { "KHR_texture_transform": { "scale": [2, 2], "offset": [0.5, 0] } } } },
    "extensions": {
      "KHR_materials_clearcoat": { "clearcoatFactor": 1, "clearcoatRoughnessFactor": 0.05,
        "clearcoatNormalTexture": { "index": 1 } },
      "KHR_materials_anisotropy": { "anisotropyStrength": 0.5 }
    }
  }],
  "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] }],
  "nodes": [{ "mesh": 0 }],
  "scenes": [{ "nodes": [0] }],
  "scene": 0
}"#;
        let path = dir.join("Paint.gltf");
        std::fs::write(&path, json).unwrap();
        let model = super::super::load(&path).unwrap();
        let mm = &model.materials[0];
        assert_eq!(mm.name, "Paint");
        let m = &mm.material;
        assert_eq!((m.tiling, m.offset), ([2.0, 2.0], [0.5, 0.0]));
        assert!(m.features.clear_coat.enabled);
        assert!((m.features.clear_coat.smoothness - 0.95).abs() < 1e-6);
        assert!(m.features.anisotropy.enabled && m.features.anisotropy.strength == 0.5);
        assert!(matches!(&mm.albedo, Some(TextureSource::File(p)) if p.ends_with("albedo.png")));
        assert_eq!(mm.extra_maps.len(), 1);
        assert!(matches!(&mm.extra_maps[0].1, TextureSource::File(p) if p.ends_with("coat.png")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zero_factors_change_nothing() {
        let ext = map(
            r#"{ "KHR_materials_clearcoat": {}, "KHR_materials_anisotropy": {},
                 "KHR_materials_transmission": {} }"#,
        );
        let mut m = Material::default();
        apply_extensions(&ext, &mut m, &mut Vec::new(), &|_| None);
        assert_eq!(m, Material::default());
    }

    #[test]
    fn transmission_is_tinted_glass() {
        let ext = map(
            r#"{ "KHR_materials_transmission": { "transmissionFactor": 0.9 },
                 "KHR_materials_volume": { "attenuationColor": [1.0, 0.5, 0.5] } }"#,
        );
        let mut m = Material::default();
        apply_extensions(&ext, &mut m, &mut Vec::new(), &|_| None);
        assert_eq!(m.rendering_mode, RenderingMode::Transparent);
        assert!((m.color[3] - 0.1).abs() < 1e-5);
        assert!(m.color[1] < 0.8 && m.color[0] == 1.0);
    }

    #[test]
    fn texture_transform_matches_the_gltf_formula() {
        let (offset, rotation, scale) = ([0.25, 0.5], 0.7f32, [2.0, 2.0]);
        let mut m = Material::default();
        apply_texture_transform(offset, rotation, scale, &mut m);
        assert_eq!(m.tiling, scale);
        // The engine's draw_uv (draw.wgsl) against glTF's T·R·S for a few points.
        let (sa, ca) = m.uv_rotation.to_radians().sin_cos();
        let ours = |uv: [f32; 2]| {
            let p = [uv[0] - 0.5, uv[1] - 0.5];
            let r = [ca * p[0] - sa * p[1] + 0.5, sa * p[0] + ca * p[1] + 0.5];
            [
                r[0] * m.tiling[0] + m.offset[0],
                r[1] * m.tiling[1] + m.offset[1],
            ]
        };
        let (s, c) = rotation.sin_cos();
        let gltf = |uv: [f32; 2]| {
            let p = [uv[0] * scale[0], uv[1] * scale[1]];
            [
                c * p[0] + s * p[1] + offset[0],
                -s * p[0] + c * p[1] + offset[1],
            ]
        };
        for uv in [[0.0, 0.0], [1.0, 0.0], [0.3, 0.8], [1.0, 1.0]] {
            let (a, b) = (ours(uv), gltf(uv));
            assert!(
                (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4,
                "{uv:?}: {a:?} vs {b:?}"
            );
        }
        // No rotation: plain tiling and offset.
        let mut m = Material::default();
        apply_texture_transform([0.1, 0.2], 0.0, [3.0, 1.0], &mut m);
        assert_eq!(
            (m.tiling, m.offset, m.uv_rotation),
            ([3.0, 1.0], [0.1, 0.2], 0.0)
        );
    }
}
