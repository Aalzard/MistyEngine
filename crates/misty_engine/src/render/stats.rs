//! Counters of the last rendered view (Game view Stats, Profiler Rendering/Memory modules)
//! and the renderer switches the optimisations can be compared with.

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
    /// Shadow-map draws skipped because the caster was outside the light's frustum.
    pub shadow_culled: u32,
    /// CPU time of the call (preparing, culling, encoding, submitting).
    pub cpu_ms: f32,
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
