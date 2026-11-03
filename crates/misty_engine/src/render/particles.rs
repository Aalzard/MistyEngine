//! Particle billboards: per-instance data, back-to-front sorting and the instance buffer.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// One billboard: world position, size, linear HDR colour, flags (x = textured).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ParticleInstance {
    pub position: [f32; 3],
    pub size: f32,
    pub color: [f32; 4],
    pub params: [f32; 4],
}

/// The live particles of one Particle System.
#[derive(Clone, Debug)]
pub struct ParticleBatch {
    /// Emitter position, used to order systems back to front.
    pub origin: Vec3,
    pub particles: Vec<ParticleInstance>,
    /// Project-relative texture (None = built-in soft particle).
    pub texture: Option<String>,
    pub additive: bool,
}

/// One draw call: instance range and the batch it came from.
pub struct ParticleDraw {
    pub instances: Range<u32>,
    pub batch: usize,
}

/// All instances in draw order: systems back to front, and particles inside alpha-blended systems
/// back to front too (additive ones don't need it). `depth` = distance from the camera.
pub fn order(
    batches: &[ParticleBatch],
    depth: impl Fn(Vec3) -> f32,
) -> (Vec<ParticleInstance>, Vec<ParticleDraw>) {
    order_filtered(batches, |_| true, depth)
}

/// [`order`] for the systems `keep` accepts (the ones inside the camera frustum).
pub fn order_filtered(
    batches: &[ParticleBatch],
    keep: impl Fn(usize) -> bool,
    depth: impl Fn(Vec3) -> f32,
) -> (Vec<ParticleInstance>, Vec<ParticleDraw>) {
    let mut by_depth: Vec<usize> = (0..batches.len())
        .filter(|i| !batches[*i].particles.is_empty() && keep(*i))
        .collect();
    by_depth.sort_by(|a, b| depth(batches[*b].origin).total_cmp(&depth(batches[*a].origin)));
    let mut instances = Vec::new();
    let mut draws = Vec::new();
    for i in by_depth {
        let b = &batches[i];
        let start = instances.len() as u32;
        if b.additive {
            instances.extend_from_slice(&b.particles);
        } else {
            let mut keyed: Vec<(f32, ParticleInstance)> = b
                .particles
                .iter()
                .map(|p| (depth(Vec3::from(p.position)), *p))
                .collect();
            keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
            instances.extend(keyed.into_iter().map(|(_, p)| p));
        }
        draws.push(ParticleDraw {
            instances: start..instances.len() as u32,
            batch: i,
        });
    }
    (instances, draws)
}

/// Growable vertex buffer of instances.
pub struct InstanceBuffer {
    pub buffer: wgpu::Buffer,
    capacity: usize,
}

impl InstanceBuffer {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: Self::create(device, 1024),
            capacity: 1024,
        }
    }

    fn create(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty particles"),
            size: (capacity * std::mem::size_of::<ParticleInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[ParticleInstance],
    ) {
        if data.len() > self.capacity {
            self.capacity = data.len().next_power_of_two();
            self.buffer = Self::create(device, self.capacity);
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(z: f32) -> ParticleInstance {
        ParticleInstance {
            position: [0.0, 0.0, z],
            size: 1.0,
            color: [1.0; 4],
            params: [0.0; 4],
        }
    }

    fn batch(z: f32, additive: bool, zs: &[f32]) -> ParticleBatch {
        ParticleBatch {
            origin: Vec3::new(0.0, 0.0, z),
            particles: zs.iter().map(|z| at(*z)).collect(),
            texture: None,
            additive,
        }
    }

    #[test]
    fn instance_matches_shader_layout() {
        assert_eq!(std::mem::size_of::<ParticleInstance>(), 48);
    }

    #[test]
    fn alpha_particles_sort_back_to_front_and_systems_too() {
        let batches = [
            batch(1.0, false, &[1.0, 3.0, 2.0]),
            batch(10.0, true, &[9.0, 11.0]),
            batch(5.0, false, &[]),
        ];
        let (inst, draws) = order(&batches, |p| p.z);
        assert_eq!(draws.len(), 2, "empty systems are skipped");
        assert_eq!(draws[0].batch, 1, "farthest system first");
        assert_eq!(draws[0].instances, 0..2);
        let zs: Vec<f32> = inst.iter().map(|p| p.position[2]).collect();
        assert_eq!(zs, [9.0, 11.0, 3.0, 2.0, 1.0], "additive keeps its order");
    }
}
