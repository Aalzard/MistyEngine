// Copies sample 0 of the MSAA depth buffer into a single-sample R32Float texture, so water and
// custom shaders in the transparent queue can read the opaque scene's depth.

@group(0) @binding(1) var depth_ms: texture_depth_multisampled_2d;

@fragment
fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth_ms, vec2<i32>(p.xy), 0);
    return vec4<f32>(d, 0.0, 0.0, 1.0);
}
