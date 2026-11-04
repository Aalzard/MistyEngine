// Depth + normal prepass of the screen-space effects (ambient occlusion, contact shadows):
// opaque and cutout meshes and terrains write their world-space vertex normal into a
// single-sample target, and their depth into the prepass depth buffer.

// `struct Draw` and `draw_uv` come from draw.wgsl.

@group(1) @binding(1) var<storage, read> draws: array<Draw>;
// Instance index → draw slot (instanced batches).
@group(1) @binding(2) var<storage, read> slots: array<u32>;
@group(2) @binding(0) var albedo_map: texture_2d<f32>;
@group(2) @binding(2) var material_sampler: sampler;

struct PrepassOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // x = colour alpha, y = cutoff, z = 1 when the albedo alpha holds smoothness.
    @location(2) @interpolate(flat) alpha: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @builtin(instance_index) instance: u32,
) -> PrepassOut {
    let d = draws[slots[instance]];
    var out: PrepassOut;
    out.clip = view.view_proj * d.model * vec4<f32>(position, 1.0);
    out.normal = (d.normal * vec4<f32>(normal, 0.0)).xyz;
    out.uv = draw_uv(d, uv);
    out.alpha = vec3<f32>(d.color.a, d.surface.y, d.surface.w);
    return out;
}

// Back faces (Cull Off) face the camera like the main pass lights them.
fn encode_normal(n: vec3<f32>, front: bool) -> vec4<f32> {
    let m = normalize(n);
    return vec4<f32>(select(-m, m, front), 1.0);
}

@fragment
fn fs_normal(in: PrepassOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return encode_normal(in.normal, front);
}

// Cutout: the same holes as the main pass.
@fragment
fn fs_normal_cutout(in: PrepassOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let base = textureSample(albedo_map, material_sampler, in.uv).a;
    let alpha = in.alpha.x * select(base, 1.0, in.alpha.z > 0.5);
    if (alpha < in.alpha.y) {
        discard;
    }
    return encode_normal(in.normal, front);
}
