//! Near-camera grass shadow map: a dedicated sun shadow map of the grass alone around the
//! camera at full blade density (alpha tested, the lit pass's own blade shapes) so blades cast
//! sharp blade-shaped shadows on each other and on the ground close up. Two slices, one map
//! layer each: an inner square of ±[`INNER_RADIUS`] m (at 2048² a 4 mm texel, so single
//! blades shadow the turf crisply where the camera sees them largest) inside the outer square
//! of ±[`RADIUS`] m. Each slice's texel grid is fixed in the world (the centre snaps to whole
//! texels), so the shadows don't shimmer as the camera moves. Lit shaders multiply it into the
//! sun's shadow (`grass_near_shadow` in mesh.wgsl) and fade it out towards the outer edge,
//! where the cheaper grass casters in the cascades take over. Quality ▸ Grass Shadow Map sets
//! its size.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::SHADOW_FORMAT;
use super::shadows::VIEW_STRIDE;

/// Half the side of the covered square around the camera (metres; the outer slice).
pub const RADIUS: f32 = 12.0;
/// Half the side of the sharp inner slice (metres).
pub const INNER_RADIUS: f32 = 4.0;
/// Slices (map layers): inner, outer.
pub const SLICES: usize = 2;
/// Half sides of the slices.
pub const SLICE_RADII: [f32; SLICES] = [INNER_RADIUS, RADIUS];
/// Blades within this × a slice's half side cast into it.
pub const CASTER_REACH: f32 = 1.25;
/// ... and fade out from this share of that reach (grass_blade.wgsl `grass_drawn_height`):
/// full height to 1.05 × the half side, past everywhere the lit shaders read the slice (the
/// inner one to 95 % of its half side, the outer one fading out by its edge) plus a blade's
/// shadow length. The casters used to fade from 60 % (75 % of the half side), shrinking the
/// blades inside the slices around the camera, so their shadows changed as it moved.
pub const CASTER_FADE: f32 = 0.84;
/// Depth reach towards and away from the light around the camera (metres).
const DEPTH: f32 = 60.0;
/// Most pixels on a side of the inner slice (its square is a third of the outer one's, so at
/// 2048² its texels are already 4 mm; a 4096² inner slice costs fill for nothing).
pub const INNER_MAX: u32 = 2048;
/// Share of the blades drawn into the outer slice (the rest widen the others, as the cascade
/// casters do); the inner slice draws every blade.
pub const SLICE_SHARES: [f32; SLICES] = [1.0, 0.6];

/// `GrassNearShadow` in mesh.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct NearUniform {
    /// Per slice.
    view_proj: [[[f32; 4]; 4]; SLICES],
    /// x = outer radius, y = 1 when on, z = inner radius, w = 1 / map size.
    params: [f32; 4],
    /// xyz = covered centre (the camera).
    centre: [f32; 4],
    /// x = inner and y = outer texel in metres, z = the inner slice's side as a share of the
    /// layer (its viewport).
    texels: [f32; 4],
}

pub struct GrassNearShadow {
    /// Map side in pixels (1 = off).
    pub size: u32,
    /// Both slices (a 2D array; sampled by the lit shaders).
    pub view: wgpu::TextureView,
    /// One view per slice (the casters' depth attachment).
    pub slice_views: Vec<wgpu::TextureView>,
    /// Sampling uniform (group 1 of the lit shaders).
    pub uniform: wgpu::Buffer,
    /// View uniforms of the casters' passes (shadow view layout, one `VIEW_STRIDE` slot per
    /// slice).
    view_buffer: wgpu::Buffer,
    pub view_group: wgpu::BindGroup,
}

impl GrassNearShadow {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout, size: u32) -> Self {
        let size = size.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty grass near shadow"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: SLICES as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | super::shadows::TEST_COPY,
            view_formats: &[],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass near shadow params"),
            size: std::mem::size_of::<NearUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty grass near shadow view"),
            size: VIEW_STRIDE * SLICES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty grass near shadow view group"),
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
        let slice_views = (0..SLICES as u32)
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("misty grass near shadow slice"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: i,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            size,
            view: texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            slice_views,
            uniform,
            view_buffer,
            view_group,
        }
    }

    /// Pixels on a side of slice `i` (the inner slice is capped at [`INNER_MAX`]).
    pub fn slice_size(&self, i: usize) -> u32 {
        if i == 0 {
            self.size.min(INNER_MAX)
        } else {
            self.size
        }
    }

    /// Write this frame's uniforms: the slices around `camera` for a sun travelling along
    /// `dir` (None = off this frame). Returns the casters' view-projection per slice when on.
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        sun: Option<Vec3>,
        camera: Vec3,
    ) -> Option<[Mat4; SLICES]> {
        let on = sun.filter(|_| self.size > 1);
        let slices: [(Mat4, f32); SLICES] = match on {
            Some(dir) => [
                matrix(dir, camera, self.slice_size(0), SLICE_RADII[0]),
                matrix(dir, camera, self.slice_size(1), SLICE_RADII[1]),
            ],
            None => [(Mat4::IDENTITY, 0.0); SLICES],
        };
        let u = NearUniform {
            view_proj: slices.map(|(m, _)| m.to_cols_array_2d()),
            params: [
                RADIUS,
                if on.is_some() { 1.0 } else { 0.0 },
                INNER_RADIUS,
                1.0 / self.size as f32,
            ],
            centre: [camera.x, camera.y, camera.z, 0.0],
            texels: [
                slices[0].1,
                slices[1].1,
                self.slice_size(0) as f32 / self.size as f32,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        on?;
        for (i, (m, _)) in slices.iter().enumerate() {
            let mut v = super::ViewUniform::zeroed();
            v.view_proj = m.to_cols_array_2d();
            v.camera_pos = [camera.x, camera.y, camera.z, 0.0];
            queue.write_buffer(
                &self.view_buffer,
                i as u64 * VIEW_STRIDE,
                bytemuck::bytes_of(&v),
            );
        }
        Some(slices.map(|(m, _)| m))
    }

    /// Dynamic offset of slice `i`'s view uniform in [`GrassNearShadow::view_group`].
    pub fn view_offset(i: usize) -> u32 {
        (i as u64 * VIEW_STRIDE) as u32
    }
}

/// View-projection of one slice (same conventions as the cascades) and its texel size: a
/// square of 2 × `radius` around `camera` seen along `dir`, the centre snapped to whole texels
/// of a grid fixed in the world.
pub(super) fn matrix(dir: Vec3, camera: Vec3, size: u32, radius: f32) -> (Mat4, f32) {
    let dir = dir.normalize_or(Vec3::NEG_Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let light_view = glam::camera::lh::view::look_to_mat4(Vec3::ZERO, dir, up);
    let texel = 2.0 * radius / size as f32;
    let c = light_view.transform_point3(camera);
    let x = (c.x / texel).floor() * texel;
    let y = (c.y / texel).floor() * texel;
    let proj = glam::camera::lh::proj::directx::orthographic(
        x - radius,
        x + radius,
        y - radius,
        y + radius,
        c.z - DEPTH,
        c.z + DEPTH,
    );
    (proj * light_view, texel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_stays_on_its_texel_grid() {
        let dir = Vec3::new(0.4, -0.5, 0.7);
        for radius in SLICE_RADII {
            let (a, texel) = matrix(dir, Vec3::new(3.0, 1.0, 2.0), 2048, radius);
            // Moving by less than a texel across the light keeps the same matrix.
            let (b, _) = matrix(dir, Vec3::new(3.0 + texel * 0.01, 1.0, 2.0), 2048, radius);
            let p = Vec3::new(5.0, 0.0, 4.0);
            let (pa, pb) = (a.project_point3(p), b.project_point3(p));
            assert!((pa.x - pb.x).abs() < 1e-5 && (pa.y - pb.y).abs() < 1e-5);
            // The camera's ground lies inside the map.
            let c = a.project_point3(Vec3::new(3.0, 1.0, 2.0));
            assert!(c.x.abs() < 1.0 && c.y.abs() < 1.0 && (0.0..1.0).contains(&c.z));
            assert!((texel - 2.0 * radius / 2048.0).abs() < 1e-6);
        }
        // The inner slice is three times as sharp as the outer one.
        let (_, inner) = matrix(dir, Vec3::ZERO, 2048, INNER_RADIUS);
        let (_, outer) = matrix(dir, Vec3::ZERO, 2048, RADIUS);
        assert!((outer / inner - RADIUS / INNER_RADIUS).abs() < 1e-4);
    }
}
