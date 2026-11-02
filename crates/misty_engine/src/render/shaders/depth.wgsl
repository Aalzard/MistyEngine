// Position-only mesh passes: shadow map depth and the selection mask.

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    emission: vec4<f32>,
};

@group(1) @binding(1) var<storage, read> draws: array<Draw>;

@vertex
fn vs_main(@location(0) position: vec3<f32>, @builtin(instance_index) draw_index: u32) -> @builtin(position) vec4<f32> {
    return view.view_proj * draws[draw_index].model * vec4<f32>(position, 1.0);
}

// Selection mask: covered pixels = 1.
@fragment
fn fs_mask() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}
