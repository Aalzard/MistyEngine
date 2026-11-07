// Virtual shadow maps (render/vsm): copy rendered pages from the scratch atlas into the
// physical page pool, one instanced quad per page (xy = scratch texel, zw = pool texel).

@group(1) @binding(0) var vsm_scratch: texture_depth_2d;
// xy = 1 / pool size in texels.
@group(1) @binding(1) var<uniform> vsm_copy_info: vec4<f32>;

struct VsmCopyOut {
    @builtin(position) clip: vec4<f32>,
    // Scratch texel minus pool texel.
    @location(0) @interpolate(flat) delta: vec2<i32>,
};

@vertex
fn vs_vsm_copy(@builtin(vertex_index) vi: u32, @location(0) page: vec4<u32>) -> VsmCopyOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let px = vec2<f32>(page.zw) + corners[vi] * 128.0;
    let uv = px * vsm_copy_info.xy;
    var out: VsmCopyOut;
    out.clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.delta = vec2<i32>(page.xy) - vec2<i32>(page.zw);
    return out;
}

@fragment
fn fs_vsm_copy(in: VsmCopyOut) -> @builtin(frag_depth) f32 {
    let t = vec2<i32>(floor(in.clip.xy)) + in.delta;
    return textureLoad(vsm_scratch, t, 0);
}
