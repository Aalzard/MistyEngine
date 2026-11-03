// Position-only mesh passes: shadow map depth and the selection mask.
// Cutout shadow casters also sample the albedo alpha and clip like the main pass.

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    emission: vec4<f32>,
    uv_st: vec4<f32>,
    surface: vec4<f32>,
};

@group(1) @binding(1) var<storage, read> draws: array<Draw>;
@group(2) @binding(0) var albedo_map: texture_2d<f32>;
@group(2) @binding(2) var material_sampler: sampler;

@vertex
fn vs_main(@location(0) position: vec3<f32>, @builtin(instance_index) draw_index: u32) -> @builtin(position) vec4<f32> {
    return view.view_proj * draws[draw_index].model * vec4<f32>(position, 1.0);
}

struct CutoutOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // x = colour alpha, y = cutoff, z = 1 when the albedo alpha holds smoothness.
    @location(1) @interpolate(flat) alpha: vec3<f32>,
};

@vertex
fn vs_uv(
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @builtin(instance_index) draw_index: u32,
) -> CutoutOut {
    let d = draws[draw_index];
    var out: CutoutOut;
    out.clip = view.view_proj * d.model * vec4<f32>(position, 1.0);
    out.uv = uv * d.uv_st.xy + d.uv_st.zw;
    out.alpha = vec3<f32>(d.color.a, d.surface.y, d.surface.w);
    return out;
}

// Cutout shadow caster: no shadow where the surface has a hole.
@fragment
fn fs_shadow_cutout(in: CutoutOut) {
    let base = textureSample(albedo_map, material_sampler, in.uv).a;
    let alpha = in.alpha.x * select(base, 1.0, in.alpha.z > 0.5);
    if (alpha < in.alpha.y) {
        discard;
    }
}

// Selection mask: covered pixels = 1.
@fragment
fn fs_mask() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}
