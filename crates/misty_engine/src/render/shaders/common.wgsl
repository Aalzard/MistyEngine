// Shared per-view uniform and helpers. Prepended to every shader.

struct View {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 when orthographic.
    camera_pos: vec4<f32>,
    // x = plane normal axis (0 = X, 1 = Y, 2 = Z), y = fade distance, z = enabled, w = LOD reference size.
    grid: vec4<f32>,
    // rgb = background colour (linear), w = 1 to draw the procedural sky.
    background: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

// World position on the near (z = 0) or far (z = 1) plane under an NDC point.
fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let p = view.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return p.xyz / p.w;
}

struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// One triangle covering the screen.
@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> FullscreenOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: FullscreenOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.ndc = ndc;
    return out;
}
