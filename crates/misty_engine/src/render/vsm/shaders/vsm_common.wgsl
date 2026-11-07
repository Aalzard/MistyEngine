// Virtual shadow maps (render/vsm): the parameters and the level / page math shared by the lit
// shaders (through shadows.wgsl) and the page marking pass (vsm_mark.wgsl).

struct VsmParams {
    // xyz = light-space x axis (world), w = 1 when the sun uses the virtual shadow map.
    light_x: vec4<f32>,
    // xyz = light-space y axis, w = clipmap level count.
    light_y: vec4<f32>,
    // xyz = light direction (light-space z), w = 1 / depth range (m).
    light_z: vec4<f32>,
    // x = texel of level 0 (m), y = pixel footprint per metre of distance, z = the camera's
    // depth (0..1), w = footprint floor (orthographic views).
    info: vec4<f32>,
    // x = debug view, y = soft-shadow search level, z = safety level, w = shadow distance.
    misc: vec4<f32>,
    // xy = 1 / pool size (texels), z = pages per pool row.
    pool: vec4<f32>,
    // Per level: xy = window origin relative to the camera (light-space q metres), z = 1 /
    // texel, w = texel.
    levels: array<vec4<f32>, 12>,
    // Per level: xy = window origin page mod 64.
    wrap: array<vec4<f32>, 12>,
};

const VSM_PAGE: f32 = 128.0;
const VSM_PAGES: f32 = 64.0;
// Soft-shadow kernels run at the finest level whose texel is at least 1/8 of their radius
// (clipmap.rs `KERNEL_TEXELS`).
const VSM_KERNEL_TEXELS: f32 = 8.0;
// Marked pages also cover this many texels beyond a pixel's own (normal offset, kernels).
const VSM_MARGIN: f32 = 12.0;

// Clipmap level of a footprint (metres): the finest level whose texel covers it
// (clipmap.rs `level_for`).
fn vsm_level_of(footprint: f32, finest: f32, levels: f32) -> i32 {
    let l = ceil(log2(max(footprint, 1e-9) / finest));
    return i32(clamp(l, 0.0, levels - 1.0));
}

// Level the grass's four taps (shadows.wgsl `fast_shadow_factor`) sample at `dist`: their
// spread is at most four texels.
fn vsm_taps_footprint(dist: f32) -> f32 {
    return 0.025 * (1.0 + 0.08 * dist) * 0.25;
}
