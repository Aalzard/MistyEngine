//! Shadow maps: the directional light's cascades (one array layer each) plus a layered array
//! shared by spot (1 layer) and point (6 layers) lights.

use glam::{Mat4, Vec3};

#[cfg(test)]
use super::MeshDraw;
use super::cascades::{self, Cascade, MAX_CASCADES};
use super::{Frame, LightData, SHADOW_FORMAT};
use crate::scene::{LightType, Shadows};

pub const DIRECTIONAL_SIZE: u32 = 2048;
pub const LOCAL_SIZE: u32 = 1024;
/// Spot = 1 layer, point = 6 layers, rectangle = 5 (one-sided) or 6 (two-sided) with soft
/// shadows on, else 1.
pub const MAX_LOCAL_LAYERS: usize = 16;
/// Field of view of a rectangle light's single shadow map (soft shadows off: points further off
/// its axis are unshadowed).
pub const RECT_SHADOW_FOV: f32 = 120.0;
/// Dynamic-offset stride for the per-pass view uniforms (a multiple of 256 that holds
/// `ViewUniform`).
pub const VIEW_STRIDE: u64 = 768;
/// Cascades first, then the local layers (the order of [`ShadowPlan::matrices`]).
pub const VIEW_SLOTS: usize = MAX_CASCADES + MAX_LOCAL_LAYERS;

/// Shadow maps can be read back in tests (render/shadow_stutter_tests).
pub(super) const TEST_COPY: wgpu::TextureUsages = if cfg!(test) {
    wgpu::TextureUsages::COPY_SRC
} else {
    wgpu::TextureUsages::empty()
};

/// Cube face directions and up vectors, in layer order +X, -X, +Y, -Y, +Z, -Z.
const CUBE_FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

pub struct ShadowMaps {
    /// Side of each sun cascade's map in pixels (Quality ▸ Shadow Resolution).
    pub directional_size: u32,
    /// Directional cascades (D2Array, sampled) and one view per cascade (rendered into).
    pub cascade_array_view: wgpu::TextureView,
    pub cascade_views: Vec<wgpu::TextureView>,
    pub local_array_view: wgpu::TextureView,
    pub local_layer_views: Vec<wgpu::TextureView>,
    pub sampler: wgpu::Sampler,
    pub view_buffer: wgpu::Buffer,
    pub view_bind_group: wgpu::BindGroup,
    /// Soft-shadow parameters (see `soft_shadows`).
    pub params_buffer: wgpu::Buffer,
    /// Tint the cascades in the next render (Scene view debug mode).
    pub debug_cascades: bool,
    /// The near-camera grass shadow map (Quality ▸ Grass Shadow Map).
    pub grass_near: super::grass_near_shadow::GrassNearShadow,
}

/// Which shadow maps to render this frame.
#[derive(Default, Debug)]
pub struct ShadowPlan {
    /// The first shadowed directional light's cascades (empty = none).
    pub cascades: Vec<Cascade>,
    /// View-projection per local layer.
    pub local: Vec<Mat4>,
    /// Per light (same order as `Frame::lights`): -1 = directional map, n = first local layer, None = no shadows.
    pub light_layers: Vec<Option<i32>>,
    /// The first shadowed directional light uses the virtual shadow map (render/vsm) instead
    /// of cascades: its direction.
    pub vsm: Option<Vec3>,
    /// View-projection of each page rectangle the virtual shadow map renders this frame
    /// (after the local layers in [`ShadowPlan::matrices`]).
    pub vsm_views: Vec<Mat4>,
    /// With the virtual shadow map: cascade boxes the fog and sun shafts read the sun's
    /// shadow through (filled from the virtual map, not drawn; render/vsm/fog.rs).
    pub fog_cascades: Vec<Cascade>,
}

impl ShadowPlan {
    /// Every shadow map's view-projection: cascades first, then the local layers, then the
    /// virtual shadow map's page rectangles.
    pub fn matrices(&self) -> impl Iterator<Item = &Mat4> {
        self.cascades
            .iter()
            .map(|c| &c.view_proj)
            .chain(&self.local)
            .chain(&self.vsm_views)
    }

    /// The sun casts shadows this frame (cascades or the virtual shadow map).
    pub fn has_sun(&self) -> bool {
        !self.cascades.is_empty() || self.vsm.is_some()
    }

    /// Slot (in [`ShadowPlan::matrices`]) of the virtual shadow map's first rectangle.
    pub fn vsm_first_slot(&self) -> usize {
        self.cascades.len() + self.local.len()
    }
}

impl ShadowMaps {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Self {
        Self::with_size(device, view_layout, DIRECTIONAL_SIZE, 1)
    }

    /// Maps with a directional shadow map of `directional_size`² pixels.
    pub fn with_size(
        device: &wgpu::Device,
        view_layout: &wgpu::BindGroupLayout,
        directional_size: u32,
        grass_near_size: u32,
    ) -> Self {
        let texture = |label, size: u32, layers: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | TEST_COPY,
                view_formats: &[],
            })
        };
        let directional = texture(
            "misty directional shadow",
            directional_size,
            MAX_CASCADES as u32,
        );
        let local = texture("misty local shadows", LOCAL_SIZE, MAX_LOCAL_LAYERS as u32);
        let local_array_view = local.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layer_views = |texture: &wgpu::Texture, layers: usize| {
            (0..layers as u32)
                .map(|layer| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_array_layer: layer,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect()
        };
        let local_layer_views = layer_views(&local, MAX_LOCAL_LAYERS);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty shadow views"),
            size: VIEW_STRIDE * VIEW_SLOTS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty shadow view group"),
            layout: view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &view_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(VIEW_STRIDE),
                }),
            }],
        });
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty soft shadow params"),
            size: std::mem::size_of::<super::soft_shadows::ShadowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            directional_size,
            cascade_array_view: directional.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            cascade_views: layer_views(&directional, MAX_CASCADES),
            local_array_view,
            local_layer_views,
            sampler,
            view_buffer,
            view_bind_group,
            params_buffer,
            debug_cascades: false,
            grass_near: super::grass_near_shadow::GrassNearShadow::new(
                device,
                view_layout,
                grass_near_size,
            ),
        }
    }
}

/// [`plan_with`] measuring the casters' bounds itself (square view, installed quality).
#[cfg(test)]
pub fn plan(frame: &Frame) -> ShadowPlan {
    plan_with(
        frame,
        &caster_bounds(frame),
        1.0,
        &crate::quality::active().shadow_settings(),
    )
}

/// Assign shadow maps: the first shadowed directional light gets the cascades; spot and point
/// lights take array layers in order until the budget runs out. `casters` = world bounds of the
/// shadow casters (they extend each cascade towards the light).
pub fn plan_with(
    frame: &Frame,
    casters: &[(Vec3, Vec3)],
    aspect: f32,
    settings: &crate::quality::ShadowSettings,
) -> ShadowPlan {
    let mut plan = ShadowPlan::default();
    for l in frame.lights {
        let wants =
            l.shadows != Shadows::None && l.intensity > 0.0 && frame.shaded && !casters.is_empty();
        let layer = match l.kind {
            _ if !wants => None,
            LightType::Directional if !plan.has_sun() => {
                if settings.mode == crate::quality::ShadowMode::Virtual {
                    plan.vsm = Some(l.direction);
                    let size = super::vsm::fog::FOG_SIZE;
                    plan.fog_cascades =
                        cascades::fit(frame.camera, aspect, l.direction, settings, casters, size);
                } else {
                    let size = settings.map_size;
                    plan.cascades =
                        cascades::fit(frame.camera, aspect, l.direction, settings, casters, size);
                }
                Some(-1)
            }
            LightType::Directional => None,
            LightType::Spot if plan.local.len() < MAX_LOCAL_LAYERS => {
                let first = plan.local.len() as i32;
                plan.local.push(spot_matrix(l));
                Some(first)
            }
            // Rectangles: a hemicube in their own frame (soft shadows), else one wide map.
            LightType::Rectangle => {
                let faces = rect_faces(l, settings.soft != crate::quality::Tier::Off);
                (plan.local.len() + faces <= MAX_LOCAL_LAYERS).then(|| {
                    let first = plan.local.len() as i32;
                    if faces == 1 {
                        plan.local.push(spot_matrix(l));
                    } else {
                        plan.local.extend(rect_matrices(l, faces));
                    }
                    first
                })
            }
            LightType::Point if plan.local.len() + 6 <= MAX_LOCAL_LAYERS => {
                let first = plan.local.len() as i32;
                plan.local.extend(point_matrices(l));
                Some(first)
            }
            _ => None,
        };
        plan.light_layers.push(layer);
    }
    plan
}

/// World bounds of every shadow caster: meshes and terrains.
#[cfg(test)]
fn caster_bounds(frame: &Frame) -> Vec<(Vec3, Vec3)> {
    let meshes = frame
        .meshes
        .iter()
        .filter(|m| m.cast_shadows)
        .map(mesh_bounds);
    let terrains = frame
        .terrains
        .iter()
        .filter(|t| t.cast_shadows)
        .map(super::terrain::world_bounds);
    meshes.chain(terrains).collect()
}

#[cfg(test)]
fn mesh_bounds(m: &MeshDraw) -> (Vec3, Vec3) {
    let (min, max) = crate::models::bounds(&m.mesh).unwrap_or((Vec3::ZERO, Vec3::ZERO));
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for corner in 0..8 {
        let s = Vec3::new(
            if corner & 1 == 0 { min.x } else { max.x },
            if corner & 2 == 0 { min.y } else { max.y },
            if corner & 4 == 0 { min.z } else { max.z },
        );
        let p = m.model.transform_point3(s);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

/// Shadow-map layers of a rectangle light: a hemicube in its own frame with soft shadows
/// (6 faces when it lights both sides), else one wide perspective map.
pub fn rect_faces(light: &LightData, soft: bool) -> usize {
    match (soft, light.area.two_sided) {
        (false, _) => 1,
        (true, false) => 5,
        (true, true) => 6,
    }
}

/// Near and far plane and tan(half field of view) of a local light's shadow frusta
/// (`faces` = a rectangle's layer count, see [`rect_faces`]).
pub fn local_clip(light: &LightData, faces: usize) -> (f32, f32, f32) {
    let near = (light.range * 0.005).clamp(0.02, 0.2);
    let fov: f32 = match light.kind {
        LightType::Point => 92.0,
        LightType::Rectangle if faces > 1 => 92.0,
        LightType::Rectangle => RECT_SHADOW_FOV,
        _ => (light.spot_angle + 2.0).min(170.0),
    };
    (
        near,
        light.range.max(near + 0.1),
        (fov.to_radians() * 0.5).tan(),
    )
}

fn local_projection(light: &LightData, faces: usize) -> Mat4 {
    let (near, far, tan_half) = local_clip(light, faces);
    glam::camera::lh::proj::directx::perspective(tan_half.atan() * 2.0, 1.0, near, far)
}

fn spot_matrix(light: &LightData) -> Mat4 {
    let dir = light.direction.normalize_or(Vec3::Z);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let view = glam::camera::lh::view::look_to_mat4(light.position, dir, up);
    local_projection(light, 1) * view
}

/// Six 90° frusta (slightly wider to hide seams).
fn point_matrices(light: &LightData) -> Vec<Mat4> {
    let proj = local_projection(light, 6);
    CUBE_FACES
        .iter()
        .map(|(dir, up)| proj * glam::camera::lh::view::look_to_mat4(light.position, *dir, *up))
        .collect()
}

/// A rectangle's hemicube: 90° frusta along its forward, right, -right, up, -up (and back
/// when two-sided) axes. Forward comes first, so code that only reads a light's first layer
/// (the fog) sees the faces right under the light.
fn rect_matrices(light: &LightData, faces: usize) -> Vec<Mat4> {
    let proj = local_projection(light, faces);
    let f = light.direction.normalize_or(Vec3::Z);
    let r = light.area.right.normalize_or(f.any_orthonormal_vector());
    let u = f.cross(r).normalize_or(light.area.up);
    let dirs = [(f, u), (r, u), (-r, u), (u, f), (-u, f), (-f, u)];
    dirs[..faces]
        .iter()
        .map(|(dir, up)| proj * glam::camera::lh::view::look_to_mat4(light.position, *dir, *up))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::render::MaterialParams;
    use crate::scene::PrimitiveMesh;

    fn light(kind: LightType) -> LightData {
        LightData {
            kind,
            position: Vec3::new(0.0, 3.0, 0.0),
            direction: Vec3::NEG_Y,
            color: [1.0; 3],
            intensity: 1.0,
            range: 10.0,
            spot_angle: 30.0,
            shadows: Shadows::Soft,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }
    }

    #[test]
    fn layers_are_allocated_per_light_type_within_budget() {
        let camera = Camera {
            position: Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let meshes = [MeshDraw {
            mesh: PrimitiveMesh::Cube.into(),
            model: Mat4::IDENTITY,
            material: MaterialParams::default(),
            cast_shadows: true,
            receive_shadows: true,
            outline: false,
            sorting: Default::default(),
            lod: None,
        }];
        let lights = [
            light(LightType::Directional),
            light(LightType::Directional),
            light(LightType::Spot),
            light(LightType::Point),
            light(LightType::Point),
            light(LightType::Point),
        ];
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &Default::default(),
            post: Default::default(),
            lines: &[],
            overlay: &[],
            meshes: &meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };

        let p = plan(&frame);
        assert_eq!(p.cascades.len(), 4);
        assert_eq!(p.matrices().count(), 4 + 13);
        assert_eq!(
            p.light_layers,
            vec![Some(-1), None, Some(0), Some(1), Some(7), None]
        );
        assert_eq!(p.local.len(), 13);
    }

    #[test]
    fn point_faces_cover_their_axis() {
        let l = light(LightType::Point);
        for (face, (dir, _)) in point_matrices(&l).iter().zip(CUBE_FACES) {
            let p = face.project_point3(l.position + dir * 2.0);
            assert!(
                p.x.abs() < 1e-3 && p.y.abs() < 1e-3 && (0.0..1.0).contains(&p.z),
                "{dir:?} -> {p:?}"
            );
        }
    }
}
