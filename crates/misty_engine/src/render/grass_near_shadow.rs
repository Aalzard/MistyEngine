//! Near-camera grass shadow map: a dedicated sun shadow map of the grass alone, covering the
//! first [`RADIUS`] metres around the camera at full blade density (alpha tested, the lit
//! pass's own blade shapes) so blades cast sharp blade-shaped shadows on each other and on the
//! ground close up. Its texel grid is fixed in the world (the centre snaps to whole texels), so
//! the shadows don't shimmer as the camera moves. Lit shaders multiply it into the sun's shadow
//! (`grass_near_shadow` in mesh.wgsl) and fade it out towards its edge, where the cheaper
//! grass casters in the cascades take over. Quality ▸ Grass Shadow Map sets its size.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::SHADOW_FORMAT;
use super::shadows::VIEW_STRIDE;

/// Half the side of the covered square around the camera (metres).
pub const RADIUS: f32 = 12.0;
/// Depth reach towards and away from the light around the camera (metres).
const DEPTH: f32 = 60.0;

/// `GrassNearShadow` in mesh.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct NearUniform {
    view_proj: [[f32; 4]; 4],
    /// x = radius, y = 1 when on, z = one texel in metres, w = 1 / map size.
    params: [f32; 4],
    /// xyz = covered centre (the camera).
    centre: [f32; 4],
}

pub struct GrassNearShadow {
    /// Map side in pixels (1 = off).
    pub size: u32,
    pub view: wgpu::TextureView,
    /// Sampling uniform (group 1 of the lit shaders).
    pub uniform: wgpu::Buffer,
    /// View uniform of the casters' pass (shadow view layout, offset 0).
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
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
            size: VIEW_STRIDE,
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
        Self {
            size,
            view: texture.create_view(&Default::default()),
            uniform,
            view_buffer,
            view_group,
        }
    }

    /// Write this frame's uniforms: the map around `camera` for a sun travelling along `dir`
    /// (None = off this frame). Returns the casters' view-projection when on.
    pub fn prepare(&self, queue: &wgpu::Queue, sun: Option<Vec3>, camera: Vec3) -> Option<Mat4> {
        let on = sun.filter(|_| self.size > 1);
        let (m, texel) = match on {
            Some(dir) => {
                let (m, texel) = matrix(dir, camera, self.size);
                (m, texel)
            }
            None => (Mat4::IDENTITY, 0.0),
        };
        let u = NearUniform {
            view_proj: m.to_cols_array_2d(),
            params: [
                RADIUS,
                if on.is_some() { 1.0 } else { 0.0 },
                texel,
                1.0 / self.size as f32,
            ],
            centre: [camera.x, camera.y, camera.z, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        on?;
        let mut v = super::ViewUniform::zeroed();
        v.view_proj = m.to_cols_array_2d();
        v.camera_pos = [camera.x, camera.y, camera.z, 0.0];
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&v));
        Some(m)
    }
}

/// View-projection of the map (same conventions as the cascades) and its texel size: a square
/// of 2 × [`RADIUS`] around `camera` seen along `dir`, the centre snapped to whole texels of a
/// grid fixed in the world.
pub(super) fn matrix(dir: Vec3, camera: Vec3, size: u32) -> (Mat4, f32) {
    let dir = dir.normalize_or(Vec3::NEG_Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let light_view = glam::camera::lh::view::look_to_mat4(Vec3::ZERO, dir, up);
    let texel = 2.0 * RADIUS / size as f32;
    let c = light_view.transform_point3(camera);
    let x = (c.x / texel).floor() * texel;
    let y = (c.y / texel).floor() * texel;
    let proj = glam::camera::lh::proj::directx::orthographic(
        x - RADIUS,
        x + RADIUS,
        y - RADIUS,
        y + RADIUS,
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
        let (a, texel) = matrix(dir, Vec3::new(3.0, 1.0, 2.0), 2048);
        // Moving by less than a texel across the light keeps the same matrix.
        let (b, _) = matrix(dir, Vec3::new(3.0 + texel * 0.01, 1.0, 2.0), 2048);
        let p = Vec3::new(5.0, 0.0, 4.0);
        let (pa, pb) = (a.project_point3(p), b.project_point3(p));
        assert!((pa.x - pb.x).abs() < 1e-5 && (pa.y - pb.y).abs() < 1e-5);
        // The camera's ground lies inside the map.
        let c = a.project_point3(Vec3::new(3.0, 1.0, 2.0));
        assert!(c.x.abs() < 1.0 && c.y.abs() < 1.0 && (0.0..1.0).contains(&c.z));
        assert!((texel - 24.0 / 2048.0).abs() < 1e-6);
    }
}
