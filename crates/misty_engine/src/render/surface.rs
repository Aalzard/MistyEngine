//! Material surface options: shader family, rendering mode and culling, plus what they mean for
//! the GPU (pipeline variant, blend state, depth writes, shadows) and for draw order.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::MeshDraw;

/// Standard shader Rendering Mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RenderingMode {
    #[default]
    Opaque,
    /// Opaque with holes: pixels below the alpha cutoff are discarded.
    Cutout,
    /// Everything (specular too) fades with alpha, like a hologram.
    Fade,
    /// Glass: only the diffuse part fades, reflections and highlights stay.
    Transparent,
}

impl RenderingMode {
    pub const ALL: [Self; 4] = [Self::Opaque, Self::Cutout, Self::Fade, Self::Transparent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Opaque => "Opaque",
            Self::Cutout => "Cutout",
            Self::Fade => "Fade",
            Self::Transparent => "Transparent",
        }
    }

    /// Alpha blended: drawn back to front after opaque geometry, without depth writes.
    pub fn is_blended(self) -> bool {
        matches!(self, Self::Fade | Self::Transparent)
    }

    /// Unity-style render queue (Geometry 2000, AlphaTest 2450, Transparent 3000).
    pub fn render_queue(self) -> u32 {
        match self {
            Self::Opaque => 2000,
            Self::Cutout => 2450,
            Self::Fade | Self::Transparent => 3000,
        }
    }

    /// Shader `_Mode` value.
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }
}

/// Built-in shaders a material can use (stored by name in `.mat` files).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShaderKind {
    #[default]
    Standard,
    /// One flat colour, no lighting.
    UnlitColor,
    /// Albedo texture, no lighting.
    UnlitTexture,
    /// Albedo texture with alpha blending, no lighting.
    UnlitTransparent,
}

impl ShaderKind {
    pub const ALL: [Self; 4] = [
        Self::Standard,
        Self::UnlitColor,
        Self::UnlitTexture,
        Self::UnlitTransparent,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::UnlitColor => "Unlit/Color",
            Self::UnlitTexture => "Unlit/Texture",
            Self::UnlitTransparent => "Unlit/Transparent",
        }
    }

    /// Unknown names fall back to Standard.
    pub fn from_name(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .unwrap_or_default()
    }

    pub fn is_unlit(self) -> bool {
        self != Self::Standard
    }
}

/// Which mesh pipeline a draw needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipelineKey {
    pub unlit: bool,
    pub mode: RenderingMode,
    /// Cull Off (both faces drawn).
    pub double_sided: bool,
    /// Optional Standard features compiled into the variant
    /// ([`super::material_v2::feature`] bits; 0 = the plain Standard shader).
    pub features: u8,
}

impl PipelineKey {
    /// Fragment entry point in `mesh.wgsl` (feature variants: `mesh_lit.wgsl`).
    pub fn entry(self) -> &'static str {
        match (self.unlit, self.mode, self.features != 0) {
            (true, _, _) => "fs_unlit",
            (false, RenderingMode::Cutout, false) => "fs_cutout",
            (false, RenderingMode::Cutout, true) => "fs_lit_cutout",
            (false, _, false) => "fs_main",
            (false, _, true) => "fs_lit",
        }
    }

    /// Fade = straight alpha; Transparent = premultiplied (the shader scales only the diffuse).
    pub fn blend(self) -> Option<wgpu::BlendState> {
        match self.mode {
            RenderingMode::Opaque | RenderingMode::Cutout => None,
            RenderingMode::Fade => Some(wgpu::BlendState::ALPHA_BLENDING),
            RenderingMode::Transparent => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        }
    }

    pub fn depth_write(self) -> bool {
        !self.mode.is_blended()
    }

    pub fn cull(self) -> Option<wgpu::Face> {
        (!self.double_sided).then_some(wgpu::Face::Back)
    }
}

/// Which shadow-caster pipeline a draw needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShadowKey {
    /// Clip by albedo alpha (Cutout).
    pub cutout: bool,
    pub double_sided: bool,
}

/// Sorting Layer value (relative to Default) and Order in Layer of a draw. Transparent draws are
/// sorted by these before distance; 3D meshes stay at Default / 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortingKey {
    pub layer: i32,
    pub order: i32,
}

/// Mesh indices in draw order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawOrder {
    /// Opaque then Cutout, each front to back.
    pub opaque: Vec<usize>,
    /// Fade / Transparent, back to front (drawn after opaque geometry and before particles).
    pub transparent: Vec<usize>,
}

/// Sort meshes like the reference renderer: by render queue, opaque front to back and
/// transparent by Sorting Layer, Order in Layer, then back to front. `depth` = distance of a
/// world point from the camera; meshes are placed by their origin. Ties keep scene order.
pub fn draw_order(meshes: &[MeshDraw], depth: impl Fn(Vec3) -> f32) -> DrawOrder {
    let keyed: Vec<(usize, u32, f32)> = meshes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let queue = m.material.render_queue();
            (i, queue, depth(m.model.w_axis.truncate()))
        })
        .collect();
    let mut opaque: Vec<_> = keyed.iter().filter(|k| k.1 < 3000).copied().collect();
    opaque.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
    let mut transparent: Vec<_> = keyed.iter().filter(|k| k.1 >= 3000).copied().collect();
    let sorting = |i: usize| meshes[i].sorting;
    transparent.sort_by(|a, b| sorting(a.0).cmp(&sorting(b.0)).then(b.2.total_cmp(&a.2)));
    DrawOrder {
        opaque: opaque.into_iter().map(|k| k.0).collect(),
        transparent: transparent.into_iter().map(|k| k.0).collect(),
    }
}

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::render::MaterialParams;
    use crate::scene::PrimitiveMesh;

    fn draw(z: f32, mode: RenderingMode) -> MeshDraw {
        MeshDraw {
            mesh: PrimitiveMesh::Sphere.into(),
            model: Mat4::from_translation(Vec3::new(0.0, 0.0, z)),
            material: MaterialParams {
                mode,
                ..Default::default()
            },
            cast_shadows: true,
            receive_shadows: true,
            outline: false,
            sorting: SortingKey::default(),
            lod: None,
        }
    }

    #[test]
    fn opaque_front_to_back_then_transparent_back_to_front() {
        use RenderingMode::*;
        let meshes = [
            draw(5.0, Fade),        // 0
            draw(9.0, Opaque),      // 1
            draw(2.0, Opaque),      // 2
            draw(1.0, Cutout),      // 3
            draw(8.0, Transparent), // 4
            draw(3.0, Fade),        // 5
        ];
        // Camera at the origin looking down +Z.
        let order = draw_order(&meshes, |p| p.z);
        assert_eq!(order.opaque, vec![2, 1, 3], "Geometry before AlphaTest");
        assert_eq!(order.transparent, vec![4, 0, 5]);
    }

    #[test]
    fn sorting_layer_then_order_then_distance() {
        let sorted = |layer, order, z| MeshDraw {
            sorting: SortingKey { layer, order },
            ..draw(z, RenderingMode::Fade)
        };
        let meshes = [
            sorted(1, 0, 1.0),  // 0: foreground layer, drawn last
            sorted(0, 5, 9.0),  // 1: default layer, high order
            sorted(0, 0, 2.0),  // 2
            sorted(0, 0, 8.0),  // 3: same key as 2 but farther: first
            sorted(-1, 9, 0.5), // 4: background layer, drawn first
        ];
        let order = draw_order(&meshes, |p| p.z);
        assert_eq!(order.transparent, vec![4, 3, 2, 1, 0]);
    }

    #[test]
    fn pipeline_state_per_mode() {
        let key = |mode| PipelineKey {
            unlit: false,
            mode,
            double_sided: false,
            features: 0,
        };
        let opaque = key(RenderingMode::Opaque);
        assert_eq!(opaque.blend(), None);
        assert!(opaque.depth_write());
        assert_eq!(opaque.cull(), Some(wgpu::Face::Back));
        assert_eq!(opaque.entry(), "fs_main");

        let cutout = key(RenderingMode::Cutout);
        assert_eq!(cutout.blend(), None);
        assert!(cutout.depth_write());
        assert_eq!(cutout.entry(), "fs_cutout");

        let fade = key(RenderingMode::Fade);
        assert_eq!(fade.blend(), Some(wgpu::BlendState::ALPHA_BLENDING));
        assert!(!fade.depth_write());

        let glass = key(RenderingMode::Transparent);
        assert_eq!(
            glass.blend(),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
        );
        assert!(!glass.depth_write());

        let two_sided = PipelineKey {
            double_sided: true,
            ..opaque
        };
        assert_eq!(two_sided.cull(), None);
        let unlit = PipelineKey {
            unlit: true,
            ..fade
        };
        assert_eq!(unlit.entry(), "fs_unlit");
        let coat = PipelineKey {
            features: super::super::material_v2::feature::CLEARCOAT,
            ..opaque
        };
        assert_eq!(coat.entry(), "fs_lit");
        assert_eq!(
            PipelineKey {
                mode: RenderingMode::Cutout,
                ..coat
            }
            .entry(),
            "fs_lit_cutout"
        );
        assert_eq!(
            PipelineKey {
                unlit: true,
                ..coat
            }
            .entry(),
            "fs_unlit"
        );
    }

    #[test]
    fn shader_names_round_trip() {
        for s in ShaderKind::ALL {
            assert_eq!(ShaderKind::from_name(s.name()), s);
        }
        assert_eq!(ShaderKind::from_name("Legacy/Bumped"), ShaderKind::Standard);
        assert_eq!(RenderingMode::Transparent.index(), 3);
    }
}
