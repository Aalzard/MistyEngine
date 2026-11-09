//! Standard Lit v2: optional material features as shader variants.
//!
//! Detail maps, parallax occlusion mapping, clear coat, translucency, anisotropy and a
//! specular tint are each a bit of [`feature`]. A material that uses none of them has no
//! [`LitExtras`], draws with the plain Standard variants and pays nothing. One that does draws
//! with a variant compiled for exactly its bits (`MAT_FEATURES` in mesh.wgsl, see
//! [`lit_source`]) and a bigger group 2 ([`LitSet`]: the Standard textures, a uniform of its
//! values and the feature maps; `mesh_lit.wgsl`).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bytemuck::{Pod, Zeroable};

use super::texture::TextureSet;

/// Feature bits (the same values as the `FEAT_*` constants of mesh.wgsl).
pub mod feature {
    pub const DETAIL: u8 = 1;
    pub const PARALLAX: u8 = 2;
    pub const CLEARCOAT: u8 = 4;
    pub const TRANSLUCENCY: u8 = 8;
    pub const ANISOTROPY: u8 = 16;
    pub const SPECULAR: u8 = 32;
    pub const ALL: [(u8, &str); 6] = [
        (DETAIL, "Detail"),
        (PARALLAX, "Parallax"),
        (CLEARCOAT, "Clear Coat"),
        (TRANSLUCENCY, "Translucency"),
        (ANISOTROPY, "Anisotropy"),
        (SPECULAR, "Specular"),
    ];
}

/// The line of mesh.wgsl a feature variant replaces.
const FEATURE_LINE: &str = "const MAT_FEATURES: u32 = 0u;";

/// `LitParams` in mesh_lit.wgsl (8 × vec4).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LitUniform {
    /// xy = detail tiling, zw = detail offset.
    pub detail_st: [f32; 4],
    /// x = detail normal scale, y = detail albedo on, z = detail normal on.
    pub detail: [f32; 4],
    /// x = height scale, y = fewest steps, z = most steps.
    pub parallax: [f32; 4],
    /// x = strength, y = roughness (alpha), z = coat normal scale (0 = no map).
    pub coat: [f32; 4],
    /// rgb = linear colour, w = transmission strength.
    pub translucency: [f32; 4],
    /// x = wrap, y = thickness.
    pub subsurface: [f32; 4],
    /// x = strength (signed), y = cos, z = sin of the direction angle, w = 1 when radial.
    pub aniso: [f32; 4],
    /// rgb = specular colour × strength.
    pub specular: [f32; 4],
}

/// Words of a [`LitUniform`].
pub const UNIFORM_WORDS: usize = std::mem::size_of::<LitUniform>() / 4;

/// Renderer values of a material's optional features ([`super::MaterialParams::extras`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LitExtras {
    /// [`feature`] bits in use.
    pub features: u8,
    pub detail_albedo: Option<String>,
    pub detail_normal: Option<String>,
    pub detail_mask: Option<String>,
    pub height_map: Option<String>,
    pub coat_normal_map: Option<String>,
    pub thickness_map: Option<String>,
    pub values: LitUniform,
}

impl LitExtras {
    /// The feature maps in binding order (7..=12).
    pub fn maps(&self) -> [Option<String>; 6] {
        [
            self.detail_albedo.clone(),
            self.detail_normal.clone(),
            self.detail_mask.clone(),
            self.height_map.clone(),
            self.coat_normal_map.clone(),
            self.thickness_map.clone(),
        ]
    }
}

/// Group 2 of a feature variant: the Standard textures, the feature maps and the values.
/// Draws with equal sets share one bind group.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LitSet {
    pub base: TextureSet,
    pub maps: [Option<String>; 6],
    /// The [`LitUniform`] as bits.
    pub values: [u32; UNIFORM_WORDS],
}

impl LitSet {
    pub fn new(base: TextureSet, extras: &LitExtras) -> Self {
        let mut values = [0u32; UNIFORM_WORDS];
        values.copy_from_slice(bytemuck::cast_slice(&[extras.values]));
        Self {
            base,
            maps: extras.maps(),
            values,
        }
    }

    pub fn uniform(&self) -> LitUniform {
        bytemuck::cast_slice::<u32, LitUniform>(&self.values)[0]
    }
}

/// Group 2 entries 6..=12 of the feature variants (after the six Standard ones).
pub(super) fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    let texture = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let mut out = vec![wgpu::BindGroupLayoutEntry {
        binding: 6,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<LitUniform>() as u64),
        },
        count: None,
    }];
    out.extend((7..=12).map(texture));
    out
}

/// The mesh shader for a set of features: mesh.wgsl with `MAT_FEATURES` set, plus
/// mesh_lit.wgsl. Built once per mask and kept for the process (at most 64 of them).
pub fn lit_source(features: u8) -> &'static str {
    static SOURCES: OnceLock<Mutex<HashMap<u8, &'static str>>> = OnceLock::new();
    let mut map = SOURCES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    map.entry(features).or_insert_with(|| {
        let src = with_features(super::pipelines::MESH_SRC, features);
        let src = format!("{src}\n{}", include_str!("shaders/mesh_lit.wgsl"));
        Box::leak(src.into_boxed_str())
    })
}

/// `source` (which includes mesh.wgsl) with `MAT_FEATURES` set to `features`.
pub fn with_features(source: &str, features: u8) -> String {
    debug_assert!(source.contains(FEATURE_LINE));
    source.replacen(
        FEATURE_LINE,
        &format!("const MAT_FEATURES: u32 = {features}u;"),
        1,
    )
}

/// Shading features a custom surface shader writes (`o.coat`, `o.anisotropy`,
/// `o.subsurface`), so its module is compiled with them.
pub fn features_of_surface_source(source: &str) -> u8 {
    let mut f = 0;
    if source.contains("o.coat") {
        f |= feature::CLEARCOAT;
    }
    if source.contains("o.anisotropy") {
        f |= feature::ANISOTROPY;
    }
    if source.contains("o.subsurface") {
        f |= feature::TRANSLUCENCY;
    }
    f
}

/// Names of the features in a mask ("Clear Coat + Anisotropy"), for the Frame Debugger.
pub fn feature_names(features: u8) -> String {
    let names: Vec<&str> = feature::ALL
        .iter()
        .filter(|(bit, _)| features & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    names.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga;

    #[test]
    fn uniform_is_eight_vec4s_and_round_trips_through_the_set() {
        assert_eq!(std::mem::size_of::<LitUniform>(), 128);
        let extras = LitExtras {
            features: feature::CLEARCOAT,
            values: LitUniform {
                coat: [1.0, 0.04, 0.0, 0.0],
                ..Default::default()
            },
            ..Default::default()
        };
        let set = LitSet::new(TextureSet::default(), &extras);
        assert_eq!(set.uniform(), extras.values);
        let other = LitSet::new(
            TextureSet::default(),
            &LitExtras {
                values: LitUniform {
                    coat: [0.5, 0.04, 0.0, 0.0],
                    ..Default::default()
                },
                ..extras.clone()
            },
        );
        assert_ne!(set, other, "different values, different groups");
    }

    #[test]
    fn variant_source_sets_only_its_features() {
        let src = lit_source(feature::CLEARCOAT | feature::DETAIL);
        assert!(src.contains("const MAT_FEATURES: u32 = 5u;"));
        assert!(!src.contains(FEATURE_LINE));
        assert!(src.contains("fn fs_lit("));
        // The base shader keeps 0 and has no feature bindings.
        let base = super::super::pipelines::MESH_SRC;
        assert!(base.contains(FEATURE_LINE));
        assert!(!base.contains("detail_albedo_map"));
        assert_eq!(lit_source(5).as_ptr(), src.as_ptr(), "built once");
    }

    #[test]
    fn custom_shaders_get_the_features_they_write() {
        assert_eq!(features_of_surface_source("o.albedo = c;"), 0);
        let f = features_of_surface_source("o.coat = 1.0;\no.anisotropy = 0.5;");
        assert_eq!(f, feature::CLEARCOAT | feature::ANISOTROPY);
        assert_eq!(feature_names(f), "Clear Coat + Anisotropy");
    }

    /// The WGSL of every feature variant parses and validates (no GPU needed).
    #[test]
    fn every_feature_variant_validates() {
        for mask in 1u8..64 {
            let src = format!("{}\n{}", super::super::pipelines::COMMON, lit_source(mask));
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("mask {mask}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("mask {mask}: {e:?}"));
        }
    }
}
