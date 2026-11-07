//! Decal Projector component (GameObject ▸ Rendering ▸ Decal Projector), like Unity's URP /
//! HDRP Decal Projector: a box that projects a texture set (base colour with opacity, normal
//! map, mask map) along its local +Z onto the opaque meshes and terrain inside it. The
//! renderer side lives in `render::decals`.

use serde::{Deserialize, Serialize};

use super::SceneComponent;
use super::fog_volume::defaults::is_default;

/// A box of projected material. The box is `size` (width, height, projection depth) in local
/// units, centred at `pivot`; with the default pivot it reaches forward from the object, so a
/// projector pointing down (rotated 90° about X) paints the ground below it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecalProjector {
    pub enabled: bool,
    /// Width (x), height (y) and projection depth (z), × the object's scale.
    pub size: [f32; 3],
    /// Box centre in local units (Unity's Pivot).
    pub pivot: [f32; 3],
    /// Base colour (sRGB) with opacity in alpha. None = the colour alone.
    pub base_map: Option<String>,
    /// Tangent-space normal map (X right, Y up the decal). None = flat.
    pub normal_map: Option<String>,
    /// Mask map like HDRP's: R metallic, G ambient occlusion, A smoothness. None = the
    /// Metallic and Smoothness values.
    pub mask_map: Option<String>,
    /// sRGB tint × the base map; alpha × opacity.
    pub color: [f32; 4],
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
    /// 0..1: fades the whole decal (Unity's Opacity / Fade Factor).
    pub opacity: f32,
    pub affects_albedo: bool,
    pub affects_normal: bool,
    /// Metallic, smoothness and occlusion.
    pub affects_mask: bool,
    /// 0..1: how much the decal's normal replaces the surface's.
    pub normal_strength: f32,
    /// Metallic (× the mask map's R when there is one).
    pub metallic: f32,
    /// Smoothness (× the mask map's A when there is one).
    pub smoothness: f32,
    /// Emission (sRGB) × intensity, added where the decal is (left out of files while black).
    #[serde(skip_serializing_if = "is_default")]
    pub emission: [f32; 3],
    #[serde(skip_serializing_if = "is_default")]
    pub emission_intensity: f32,
    /// Camera distance (m) past which the decal is not drawn.
    pub draw_distance: f32,
    /// 0..1 share of the draw distance where fading out starts.
    pub start_fade: f32,
    /// Fade by the angle (degrees) between the surface and the projection: full until the
    /// first, gone at the second. 0..180 = no angle fade (Unity's default).
    pub angle_fade: [f32; 2],
    /// Decals with a higher priority draw over lower ones (ties: Hierarchy order).
    pub priority: i32,
}

impl Default for DecalProjector {
    fn default() -> Self {
        Self {
            enabled: true,
            size: [1.0, 1.0, 1.0],
            pivot: [0.0, 0.0, 0.5],
            base_map: None,
            normal_map: None,
            mask_map: None,
            color: [1.0, 1.0, 1.0, 1.0],
            tiling: [1.0, 1.0],
            offset: [0.0, 0.0],
            opacity: 1.0,
            affects_albedo: true,
            affects_normal: true,
            affects_mask: true,
            normal_strength: 1.0,
            metallic: 0.0,
            smoothness: 0.5,
            emission: [0.0, 0.0, 0.0],
            emission_intensity: 0.0,
            draw_distance: 1000.0,
            start_fade: 0.9,
            angle_fade: [0.0, 180.0],
            priority: 0,
        }
    }
}

impl SceneComponent for DecalProjector {
    const NAME: &'static str = "Decal Projector";
    const CATEGORY: &'static str = "Rendering";
}

impl DecalProjector {
    /// Local box → object space: the unit cube (-0.5..0.5) scaled to `size`, moved to `pivot`.
    pub fn box_matrix(&self) -> glam::Mat4 {
        let size = glam::Vec3::from(self.size)
            .abs()
            .max(glam::Vec3::splat(1e-4));
        glam::Mat4::from_scale_rotation_translation(
            size,
            glam::Quat::IDENTITY,
            glam::Vec3::from(self.pivot),
        )
    }

    /// The textures it projects (base, normal, mask).
    pub fn textures(&self) -> [Option<&str>; 3] {
        [
            self.base_map.as_deref(),
            self.normal_map.as_deref(),
            self.mask_map.as_deref(),
        ]
    }
}
