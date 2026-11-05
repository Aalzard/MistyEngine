//! 2D lights on the GPU: one storage buffer (mesh group binding 30) with the lights and their
//! polar shadow maps, read by `fs_unlit` for draws marked `lit_2d` (see [`crate::light2d`]).

use bytemuck::{Pod, Zeroable};

use crate::light2d::{Light2DType, Lights2D, MAX_LIGHTS, SHADOW_BINS};

/// Mesh group binding of the buffer.
pub const BINDING: u32 = 30;

/// One `Light2DItem` (light2d section of mesh.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct Light2DGpu {
    /// xy = position, z = inner radius, w = outer radius.
    pos_radius: [f32; 4],
    /// rgb = linear colour × intensity, w = kind (0 global, 1 point, 2 spot).
    color_kind: [f32; 4],
    /// xy = spot direction, z = cos(outer half angle), w = cos(inner half angle).
    dir_angle: [f32; 4],
    /// x = falloff exponent, y = shadow row (−1 none), z = shadow strength, w = softness (bins).
    params: [f32; 4],
    /// x = Sorting Layer mask (bit = layer value + 16).
    mask: [u32; 4],
}

/// The whole buffer.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Lights2DGpu {
    /// x = light count, y = bins per shadow row.
    info: [f32; 4],
    items: [Light2DGpu; MAX_LIGHTS],
    shadows: [f32; MAX_LIGHTS * SHADOW_BINS],
}

pub(super) fn buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty lights 2d"),
        size: std::mem::size_of::<Lights2DGpu>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// GPU form of a frame's 2D lights.
pub(super) fn pack(l: &Lights2D) -> Box<Lights2DGpu> {
    let mut out = Box::new(Lights2DGpu::zeroed());
    out.info = [
        l.items.len().min(MAX_LIGHTS) as f32,
        SHADOW_BINS as f32,
        0.0,
        0.0,
    ];
    for (dst, src) in out.items.iter_mut().zip(&l.items) {
        let (co, ci) = if src.kind == Light2DType::Spot && src.outer_angle < 360.0 {
            let outer = src.outer_angle.clamp(0.0, 360.0);
            let inner = src.inner_angle.clamp(0.0, outer);
            (
                (outer * 0.5).to_radians().cos(),
                (inner * 0.5).to_radians().cos(),
            )
        } else {
            // A full circle: every direction passes.
            (-2.0, -1.0)
        };
        let kind = match src.kind {
            Light2DType::Global => 0.0,
            Light2DType::Point => 1.0,
            Light2DType::Spot => 2.0,
        };
        *dst = Light2DGpu {
            pos_radius: [
                src.position.x,
                src.position.y,
                src.inner_radius,
                src.outer_radius,
            ],
            color_kind: [src.color.x, src.color.y, src.color.z, kind],
            dir_angle: [src.direction.x, src.direction.y, co, ci],
            params: [
                crate::light2d::falloff_exponent(src.falloff_strength),
                src.shadow_row.map_or(-1.0, |r| r as f32),
                src.shadow_strength,
                // Softness → neighbouring bins sampled on each side (0..4).
                (src.shadow_softness * 4.0).clamp(0.0, 4.0),
            ],
            mask: [src.layer_mask, 0, 0, 0],
        };
    }
    let n = l.shadows.len().min(out.shadows.len());
    out.shadows[..n].copy_from_slice(&l.shadows[..n]);
    out
}

/// Upload a frame's 2D lights: just the (zero) count while there are none.
pub(super) fn upload(queue: &wgpu::Queue, buffer: &wgpu::Buffer, l: &Lights2D) {
    if l.is_empty() {
        queue.write_buffer(buffer, 0, bytemuck::bytes_of(&[0.0f32; 4]));
        return;
    }
    let packed = pack(l);
    let used = 16
        + std::mem::size_of::<Light2DGpu>() * MAX_LIGHTS
        + 4 * l.shadows.len().min(MAX_LIGHTS * SHADOW_BINS);
    let bytes = bytemuck::bytes_of(&*packed);
    queue.write_buffer(buffer, 0, &bytes[..used]);
}

/// `extra.w` of a draw: Sorting Layer bit + 1 for 2D-lit draws, else 0.
pub(super) fn draw_flag(lit: bool, layer: i32) -> f32 {
    if lit {
        ((layer + crate::light2d::LAYER_OFFSET).clamp(0, 31) + 1) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "light2d_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        // WGSL: Light2DItem = 4 × vec4<f32> + vec4<u32> = 80 bytes; header vec4 + 16 items +
        // 4096 floats.
        assert_eq!(std::mem::size_of::<Light2DGpu>(), 80);
        assert_eq!(
            std::mem::size_of::<Lights2DGpu>(),
            16 + 80 * MAX_LIGHTS + 4 * MAX_LIGHTS * SHADOW_BINS
        );
        assert_eq!(draw_flag(false, 0), 0.0);
        assert_eq!(draw_flag(true, 0), 17.0);
        assert_eq!(draw_flag(true, -40), 1.0);
    }
}
