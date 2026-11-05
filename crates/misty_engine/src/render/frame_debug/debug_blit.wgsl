// Frame Debugger replay: show the stopped event's render target in the output image.
// HDR images are tonemapped (Reinhard), 8-bit images copied, depth maps shown as 1 - depth.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs_tonemap(in: VsOut) -> @location(0) vec4<f32> {
    let c = max(textureSampleLevel(src, samp, in.uv, 0.0).rgb, vec3<f32>(0.0));
    return vec4<f32>(c / (1.0 + c), 1.0);
}

@fragment
fn fs_copy(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(src, samp, in.uv, 0.0).rgb, 1.0);
}
