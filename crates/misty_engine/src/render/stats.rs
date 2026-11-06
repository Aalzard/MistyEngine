//! Counters of the last rendered view (Game view Stats, Profiler Rendering/Memory modules)
//! and the renderer switches the optimisations can be compared with.

use std::collections::VecDeque;

use serde::Serialize;

/// What one `Renderer::render` call drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RenderStats {
    /// Draw calls of meshes, terrains and particles (an instanced batch counts once).
    pub batches: u32,
    /// Draws that joined another draw's instanced batch.
    pub saved_by_batching: u32,
    pub triangles: u64,
    pub vertices: u64,
    /// Pipeline (shader variant) changes.
    pub set_pass: u32,
    /// Material texture bind group changes.
    pub texture_binds: u32,
    /// Distinct meshes drawn into at least one shadow map.
    pub shadow_casters: u32,
    /// Mesh draws inside the camera frustum / outside it.
    pub visible: u32,
    pub culled: u32,
    /// Of `culled`: inside the frustum but outside the camera cell's baked visible set.
    pub occlusion_culled: u32,
    /// Shadow-map draws skipped because the caster was outside the light's frustum.
    pub shadow_culled: u32,
    /// CPU time of the call (preparing, culling, encoding, submitting).
    pub cpu_ms: f32,
    /// Grass chunks still being scattered on the workers or waiting for upload.
    pub grass_pending: u32,
    /// Render-thread time spent uploading finished grass chunks (budgeted per frame).
    pub grass_upload_ms: f32,
    /// Of that, the time already spent when the last chunk started: under the budget.
    pub grass_upload_last_start_ms: f32,
}

impl RenderStats {
    /// Count one draw call of `instances` copies of a mesh.
    pub(crate) fn draw(&mut self, instances: u32, indices: u32, vertices: u32) {
        self.batches += 1;
        self.saved_by_batching += instances.saturating_sub(1);
        self.triangles += instances as u64 * (indices / 3) as u64;
        self.vertices += instances as u64 * vertices as u64;
    }
}

/// GPU resources the renderer holds (Profiler Memory module; sizes are estimates).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RenderMemory {
    /// Uploaded meshes (primitives, imported meshes, terrains).
    pub meshes: usize,
    /// Loaded material/particle/terrain textures.
    pub textures: usize,
    /// Vertex, index, uniform and storage buffers.
    pub buffer_bytes: u64,
    /// Texture memory (loaded textures, shadow maps).
    pub texture_bytes: u64,
}

/// Renderer switches (the defaults are what measured fastest; turning one off reproduces the
/// unoptimised path for comparisons).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    /// Skip meshes, terrains and particle systems outside the camera / light frusta.
    pub culling: bool,
    /// Draw identical mesh + material draws with one instanced call.
    pub instancing: bool,
    /// Quality ▸ LOD Bias: multiplies every LOD Group's screen height (> 1 keeps finer levels
    /// longer).
    pub lod_bias: f32,
    /// Cull opaque draws on the GPU (frustum + Hi-Z occlusion) and draw them indirectly.
    pub gpu_culling: bool,
    /// Draw opaque depth first, then shade with an equal depth test.
    pub depth_prepass: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            culling: true,
            instancing: true,
            lod_bias: 1.0,
            gpu_culling: false,
            depth_prepass: false,
        }
    }
}

/// Frames kept by [`FrameHistory`].
pub const FRAME_HISTORY: usize = 240;

/// Recent frame times and the hitches among them: a hitch is a frame longer than twice the
/// median of the kept frames (the Stats overlay's graph and counter).
#[derive(Clone, Debug, Default)]
pub struct FrameHistory {
    times: VecDeque<f32>,
    /// Hitches since the history was started or cleared.
    hitches: u32,
    /// Longest frame so far.
    worst_ms: f32,
}

impl FrameHistory {
    /// Frames longer than this many times the median count as hitches.
    pub const HITCH_FACTOR: f32 = 2.0;

    /// Record a frame of `ms` milliseconds.
    pub fn push(&mut self, ms: f32) {
        if !ms.is_finite() || ms <= 0.0 {
            return;
        }
        let median = self.median_ms();
        // Needs a few frames to know what normal looks like.
        if self.times.len() >= 10 && ms > median * Self::HITCH_FACTOR {
            self.hitches += 1;
        }
        self.worst_ms = self.worst_ms.max(ms);
        self.times.push_back(ms);
        while self.times.len() > FRAME_HISTORY {
            self.times.pop_front();
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Median of the kept frames (0 when empty).
    pub fn median_ms(&self) -> f32 {
        if self.times.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f32> = self.times.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        sorted[sorted.len() / 2]
    }

    pub fn hitches(&self) -> u32 {
        self.hitches
    }

    pub fn worst_ms(&self) -> f32 {
        self.worst_ms
    }

    /// The kept frame times, oldest first.
    pub fn times(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.times.iter().copied()
    }

    /// The frame at `i` (oldest first) counts as a hitch against the current median.
    pub fn is_hitch(&self, ms: f32) -> bool {
        ms > self.median_ms() * Self::HITCH_FACTOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hitches_are_frames_over_twice_the_median() {
        let mut h = FrameHistory::default();
        for _ in 0..20 {
            h.push(16.0);
        }
        h.push(20.0);
        assert_eq!(h.hitches(), 0);
        h.push(40.0);
        h.push(16.0);
        h.push(100.0);
        assert_eq!(h.hitches(), 2);
        assert_eq!(h.worst_ms(), 100.0);
        assert!((h.median_ms() - 16.0).abs() < 1e-6);
        assert!(h.is_hitch(40.0) && !h.is_hitch(30.0));
        for _ in 0..FRAME_HISTORY + 5 {
            h.push(8.0);
        }
        assert_eq!(h.times().len(), FRAME_HISTORY);
        h.clear();
        assert_eq!((h.hitches(), h.times().len()), (0, 0));
    }
}
