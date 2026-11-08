// Frame Debugger replay of a shadow map: 1 - depth, so near casters are bright.

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

@group(0) @binding(0) var depth_src: texture_depth_2d;

@fragment
fn fs_depth(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_src));
    let texel = clamp(vec2<i32>(in.uv * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let d = textureLoad(depth_src, texel, 0);
    let v = pow(clamp(1.0 - d, 0.0, 1.0), 0.5);
    return vec4<f32>(v, v, v, 1.0);
}

// Camera depth (D3D-style 0 = near, 1 = far): a steep curve so nearby surfaces are bright and
// the far plane and sky black.
@fragment
fn fs_scene_depth(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_src));
    let texel = clamp(vec2<i32>(in.uv * vec2<f32>(size)), vec2<i32>(0), size - 1);
    let d = textureLoad(depth_src, texel, 0);
    let v = pow(clamp(1.0 - d, 0.0, 1.0), 0.3);
    return vec4<f32>(v, v, v, 1.0);
}
