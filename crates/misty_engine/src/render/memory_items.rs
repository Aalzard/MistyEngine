//! The renderer's GPU allocations item by item (Memory Profiler snapshots).

use super::texture::texture_bytes;
use super::{RenderTarget, Renderer, cascades, shadows};
use crate::memory_snapshot::{MemCategory, MemItem};

impl Renderer {
    /// Textures, meshes, shadow maps, effect volumes, buffers and lightmaps the renderer holds.
    /// View targets belong to their owners (see [`RenderTarget::memory_bytes`]).
    pub fn memory_items(&self) -> Vec<MemItem> {
        let mut out = Vec::new();
        for t in self.textures.entries() {
            let name = format!("{} ({}×{}, {:?})", t.path, t.width, t.height, t.format);
            let item = MemItem::new(MemCategory::Textures, "Texture2D", name, t.bytes());
            out.push(item.with_refs(t.refs));
        }
        let primitives: u64 = self.meshes.iter().map(|m| m.bytes()).sum();
        out.push(MemItem::new(
            MemCategory::Meshes,
            "Mesh",
            "Built-in primitives",
            primitives,
        ));
        let mut imported: Vec<(&String, &super::GpuMesh)> = self
            .imported
            .iter()
            .filter_map(|(k, m)| m.as_ref().map(|m| (k, m)))
            .collect();
        imported.sort_by(|a, b| a.0.cmp(b.0));
        for (key, m) in imported {
            let kind = match key.starts_with("static-batch-") {
                true => "Combined Mesh (Static)",
                false => "Mesh",
            };
            out.push(MemItem::new(
                MemCategory::Meshes,
                kind,
                key.clone(),
                m.bytes(),
            ));
        }
        let mut push = |category, kind: &str, name: &str, bytes: u64| {
            if bytes > 0 {
                out.push(MemItem::new(category, kind, name, bytes));
            }
        };
        push(
            MemCategory::Meshes,
            "Mesh",
            "Skinned mesh poses",
            self.skins.bytes(),
        );
        let (_, terrain_bytes) = self.terrains.memory();
        push(
            MemCategory::Meshes,
            "Terrain",
            "Terrain meshes and layers",
            terrain_bytes,
        );
        push(
            MemCategory::RenderTargets,
            "Shadow Map",
            &format!(
                "Shadow cascades ({} × {}²)",
                cascades::MAX_CASCADES,
                self.shadows.directional_size
            ),
            self.cascade_bytes(),
        );
        push(
            MemCategory::RenderTargets,
            "Shadow Map",
            &format!(
                "Local light shadow layers ({} × {}²)",
                self.shadows.local_layers,
                shadows::LOCAL_SIZE
            ),
            local_shadow_bytes(self.shadows.local_layers),
        );
        push(
            MemCategory::RenderTargets,
            "Volume",
            "Volumetric fog and sun shafts",
            self.volumetrics.texture_bytes(),
        );
        push(
            MemCategory::RenderTargets,
            "Volume",
            "Volumetric clouds",
            self.clouds.texture_bytes(),
        );
        push(
            MemCategory::Textures,
            "Cubemap",
            "Reflection probe atlas",
            self.probes.bytes(),
        );
        push(
            MemCategory::Textures,
            "Cubemap",
            "HDRI sky cubemaps",
            self.sky.bytes(),
        );
        push(
            MemCategory::Lightmaps,
            "Texture2DArray",
            "Baked lightmaps",
            self.lightmaps.bytes(),
        );
        let buffers = [
            ("Per-draw values", &self.draws_buffer),
            ("Draw slots", &self.slots_buffer),
            ("Gizmo lines", &self.line_buffer),
            ("View uniform", &self.view_buffer),
            ("Lights uniform", &self.lights_buffer),
            ("2D lights", &self.lights_2d),
            ("Area light LTC table", &self.ltc_table),
        ];
        for (name, b) in buffers {
            push(MemCategory::Buffers, "GraphicsBuffer", name, b.size());
        }
        push(
            MemCategory::Particles,
            "GraphicsBuffer",
            "Particle instances",
            self.particle_instances.buffer.size(),
        );
        push(
            MemCategory::Particles,
            "GraphicsBuffer",
            "Visual Effect buffers",
            self.effects.bytes(),
        );
        out
    }

    /// Sun cascade shadow array bytes.
    pub(super) fn cascade_bytes(&self) -> u64 {
        let size = self.shadows.directional_size;
        let format = super::SHADOW_FORMAT;
        texture_bytes(format, size, size, 1, cascades::MAX_CASCADES as u32)
    }
}

/// Bytes of a local light shadow array of `layers` layers.
pub(super) fn local_shadow_bytes(layers: u32) -> u64 {
    let size = shadows::LOCAL_SIZE;
    texture_bytes(super::SHADOW_FORMAT, size, size, 1, layers)
}

impl RenderTarget {
    /// Bytes of this view's images: MSAA HDR colour, emission and depth, the resolved HDR and
    /// emission images, the output, the pre-finish image, the selection mask and the bloom mips.
    pub fn memory_bytes(&self) -> u64 {
        use super::{COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, MASK_FORMAT};
        let (w, h) = self.size();
        let at = |format, samples: u32| texture_bytes(format, w, h, 1, 1) * samples as u64;
        // Post-processing images are output-sized (larger than the scene's when upscaled).
        let (ow, oh) = self.output_size();
        let out = |format| texture_bytes(format, ow, oh, 1, 1);
        let upscaled = if self.upscaled() { out(HDR_FORMAT) } else { 0 };
        let full = at(HDR_FORMAT, self.samples()) * 2
            + at(DEPTH_FORMAT, self.samples())
            + at(HDR_FORMAT, 1) * 2
            + out(COLOR_FORMAT) * 2
            + upscaled
            + at(MASK_FORMAT, 1);
        let bloom: u64 = (0..self.bloom.len() as u32)
            .map(|i| {
                let (bw, bh) = ((ow >> (i + 1)).max(1), (oh >> (i + 1)).max(1));
                texture_bytes(HDR_FORMAT, bw, bh, 1, 1)
            })
            .sum();
        full + bloom
    }
}

/// NoonValley's GPU allocations after the hero view settles at High 1920 × 1080, largest first,
/// with the process working set (`MISTY_NOON_VIEWS`, default `hero`). Prints only.
#[cfg(test)]
mod noon_memory {
    use crate::quality::QualityLevel;
    use crate::render::Renderer;
    use crate::render::gpu_tests::headless;
    use crate::render::noon_valley_tests::noon_view;

    #[test]
    #[ignore = "memory report, prints only"]
    fn noon_valley_memory() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
        let names = std::env::var("MISTY_NOON_VIEWS").unwrap_or_else(|_| "hero".into());
        for name in names.split(',') {
            let Some(view) = noon_view(name) else {
                continue;
            };
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &QualityLevel::high());
            let target = r.create_view_target(&device, 1920, 1080);
            let frame = view.frame();
            for _ in 0..60 {
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
            let mut items = r.memory_items();
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            let total: u64 = items.iter().map(|i| i.bytes).sum();
            eprintln!(
                "== noon {name}: renderer {:.1} MB, view target {:.1} MB, working set {:.1} MB",
                mb(total),
                mb(target.memory_bytes()),
                mb(crate::memory_snapshot::working_set().unwrap_or(0))
            );
            for (name, bytes) in r.made_texture_bytes() {
                eprintln!("   made textures: {name:<20} {:9.2} MB", mb(bytes));
            }
            for i in items.iter().take(40) {
                eprintln!("   {:9.2} MB  {:<16} {}", mb(i.bytes), i.kind, i.name);
            }
        }
    }
}
